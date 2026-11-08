//! AI conference dates for the browser's year calendar.
//!
//! Submission deadlines, rebuttal periods, and conference dates for 15 major
//! AI conferences come from the community-maintained Hugging Face AI Deadlines
//! data (<https://github.com/huggingface/ai-deadlines>). Data is fetched only
//! when the user turns the conference layer on, then kept in a small
//! normalized cache file that is refreshed at most once a week or on request.
//!
//! The YAML is read with PyYAML's (YAML 1.1) typing for the fields used:
//! unquoted `2026-06-03` is a date, `2026-05-04 23:59:59` a datetime, quoted
//! values stay text, so deadlines resolve to the same instants as before.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveDateTime, Offset, TimeZone, Utc};
use regex::Regex;
use serde_json::{Value, json};
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser, Tag};
use yaml_rust2::scanner::{Marker, TScalarStyle};

use crate::py::{self, Dict, PyDateTime};
use crate::web::services::{WebError, WebResult, invalid, utf8_error};

pub const SOURCE_URL: &str = "https://github.com/huggingface/ai-deadlines";
const RAW_URL: &str = "https://raw.githubusercontent.com/huggingface/ai-deadlines/main/src/data/conferences/";
/// Days a fetched cache stays fresh.
pub const CACHE_TTL_DAYS: i64 = 7;
/// Bump when [`normalize_editions`] changes, since the cache stores its output.
pub const CACHE_VERSION: i64 = 2;
pub const FETCH_TIMEOUT_SECONDS: u64 = 10;
const FETCH_WORKERS: usize = 6;

/// `(id, display name, group)`; groups drive the calendar colors.
pub const CONFERENCES: [(&str, &str, &str); 15] = [
    ("neurips", "NeurIPS", "ml"),
    ("iclr", "ICLR", "ml"),
    ("icml", "ICML", "ml"),
    ("cvpr", "CVPR", "vision"),
    ("iccv", "ICCV", "vision"),
    ("eccv", "ECCV", "vision"),
    ("acl", "ACL", "language"),
    ("emnlp", "EMNLP", "language"),
    ("colm", "COLM", "language"),
    ("aaai", "AAAI", "ai"),
    ("ijcai", "IJCAI", "ai"),
    ("icra", "ICRA", "robotics"),
    ("iros", "IROS", "robotics"),
    ("rss", "RSS", "robotics"),
    ("corl", "CoRL", "robotics"),
];

// Deadline types shown on the calendar: abstract registration and paper submission.
const ABSTRACT_TYPES: [&str; 2] = ["abstract", "registration"];
const SUBMISSION_TYPES: [&str; 3] = ["paper", "submission", "full_paper"];
const FIXED_ZONES: [(&str, i32); 20] = [
    ("aoe", -12 * 60),
    ("utc", 0),
    ("gmt", 0),
    ("z", 0),
    ("pst", -8 * 60),
    ("pdt", -7 * 60),
    ("mst", -7 * 60),
    ("mdt", -6 * 60),
    ("cst", -6 * 60),
    ("cdt", -5 * 60),
    ("est", -5 * 60),
    ("edt", -4 * 60),
    ("cet", 60),
    ("cest", 2 * 60),
    ("bst", 60),
    ("ist", 5 * 60 + 30),
    ("jst", 9 * 60),
    ("kst", 9 * 60),
    ("sgt", 8 * 60),
    ("aest", 10 * 60),
];
const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

/// Side tracks the generic deadline types also cover ("Workshop Proposal Submission", ...).
fn side_track() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(
        &RE,
        r"(?i)\b(workshops?|tutorials?|proposals?|art|videos?|demos?|demonstrations?|supplement\w*|challenges?|competitions?|doctoral|industry|camera|journal|posters?|late[- ]breaking)\b",
    )
}

fn rebuttal_type() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(&RE, r"rebuttal|author[_ -]?response")
}

fn rebuttal_word() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(&RE, r"(?i)\brebuttal")
}

fn start_word() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(&RE, r"(?i)\b(starts?|begins?|beginning|opens?|opening)\b")
}

fn period_edge() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(&RE, r"(?i)\b(?:starts?|begins?|opens?|ends?|closes?)\b(?:\s+of(?:\s+the)?\b)?")
}

// =============================================================================
// YAML with PyYAML's types
// =============================================================================

/// A value as `yaml.safe_load` builds it.
#[derive(Clone, Debug, PartialEq)]
pub enum Yaml {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Date(NaiveDate),
    DateTime(NaiveDateTime, Option<FixedOffset>),
    List(Vec<Yaml>),
    Map(Vec<(Yaml, Yaml)>),
}

impl Yaml {
    /// `mapping.get(key)` for a text key.
    fn get(&self, key: &str) -> Option<&Yaml> {
        match self {
            Yaml::Map(pairs) => pairs.iter().find(|(k, _)| matches!(k, Yaml::Str(s) if s == key)).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Python truthiness.
    fn truthy(&self) -> bool {
        match self {
            Yaml::Null => false,
            Yaml::Bool(b) => *b,
            Yaml::Int(i) => *i != 0,
            Yaml::Float(f) => *f != 0.0,
            Yaml::Str(s) => !s.is_empty(),
            Yaml::List(items) => !items.is_empty(),
            Yaml::Map(pairs) => !pairs.is_empty(),
            Yaml::Date(_) | Yaml::DateTime(..) => true,
        }
    }

    /// Python `str(value)`.
    fn py_str(&self) -> String {
        match self {
            Yaml::Null => "None".to_string(),
            Yaml::Bool(b) => if *b { "True" } else { "False" }.to_string(),
            Yaml::Int(i) => i.to_string(),
            Yaml::Float(f) => py::float_repr(*f),
            Yaml::Str(s) => s.clone(),
            Yaml::Date(d) => d.format("%Y-%m-%d").to_string(),
            Yaml::DateTime(naive, offset) => {
                let text = py::isoformat_naive(naive).replacen('T', " ", 1);
                match offset {
                    Some(offset) => {
                        let aware = py::isoformat_aware(&offset.from_utc_datetime(naive));
                        format!("{text}{}", &aware[aware.len() - 6..])
                    }
                    None => text,
                }
            }
            Yaml::List(items) => format!("[{}]", items.iter().map(Yaml::py_repr).collect::<Vec<_>>().join(", ")),
            Yaml::Map(pairs) => format!(
                "{{{}}}",
                pairs.iter().map(|(k, v)| format!("{}: {}", k.py_repr(), v.py_repr())).collect::<Vec<_>>().join(", ")
            ),
        }
    }

    fn py_repr(&self) -> String {
        match self {
            Yaml::Str(s) => py::repr_str(s),
            other => other.py_str(),
        }
    }

    /// `str(value or default)`.
    fn text_or(value: Option<&Yaml>, default: &str) -> String {
        match value {
            Some(v) if v.truthy() => v.py_str(),
            _ => default.to_string(),
        }
    }

    /// The value as JSON (for the edition year).
    fn to_json(&self) -> Value {
        match self {
            Yaml::Bool(b) => json!(b),
            Yaml::Int(i) => json!(i),
            other => json!(other.py_str()),
        }
    }
}

fn first_chars(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

fn yaml_bool() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(&RE, r"^(?:yes|Yes|YES|no|No|NO|true|True|TRUE|false|False|FALSE|on|On|ON|off|Off|OFF)$")
}

fn yaml_float() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(
        &RE,
        r"^(?:[-+]?(?:[0-9][0-9_]*)\.[0-9_]*(?:[eE][-+][0-9]+)?|\.[0-9_]+(?:[eE][-+][0-9]+)?|[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$",
    )
}

fn yaml_int() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(
        &RE,
        r"^(?:[-+]?0b[0-1_]+|[-+]?0[0-7_]+|[-+]?(?:0|[1-9][0-9_]*)|[-+]?0x[0-9a-fA-F_]+|[-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+)$",
    )
}

fn yaml_timestamp() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(
        &RE,
        r"^(?:[0-9]{4}-[0-9]{2}-[0-9]{2}|[0-9]{4}-[0-9]{1,2}-[0-9]{1,2}(?:[Tt]|[ \t]+)[0-9]{1,2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9]{1,2}(?::[0-9]{2})?))?)$",
    )
}

fn timestamp_parts() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(
        &RE,
        r"^(?P<year>[0-9]{4})-(?P<month>[0-9]{1,2})-(?P<day>[0-9]{1,2})(?:(?:[Tt]|[ \t]+)(?P<hour>[0-9]{1,2}):(?P<minute>[0-9]{2}):(?P<second>[0-9]{2})(?:\.(?P<fraction>[0-9]*))?(?:[ \t]*(?P<tz>Z|(?P<sign>[-+])(?P<tzh>[0-9]{1,2})(?::(?P<tzm>[0-9]{2}))?))?)?$",
    )
}

fn sexagesimal(text: &str) -> f64 {
    text.split(':').fold(0.0, |total, part| total * 60.0 + part.parse::<f64>().unwrap_or(0.0))
}

fn construct_int(text: &str) -> Option<i64> {
    let cleaned = text.replace('_', "");
    let (sign, digits) = match cleaned.strip_prefix('-') {
        Some(rest) => (-1, rest.to_string()),
        None => (1, cleaned.strip_prefix('+').unwrap_or(&cleaned).to_string()),
    };
    let value = if digits == "0" {
        0
    } else if let Some(bin) = digits.strip_prefix("0b") {
        i64::from_str_radix(bin, 2).ok()?
    } else if let Some(hex) = digits.strip_prefix("0x") {
        i64::from_str_radix(hex, 16).ok()?
    } else if digits.starts_with('0') {
        i64::from_str_radix(&digits, 8).ok()?
    } else if digits.contains(':') {
        sexagesimal(&digits) as i64
    } else {
        digits.parse().ok()?
    };
    Some(sign * value)
}

fn construct_float(text: &str) -> f64 {
    let cleaned = text.replace('_', "").to_lowercase();
    let (sign, body) = match cleaned.strip_prefix('-') {
        Some(rest) => (-1.0, rest.to_string()),
        None => (1.0, cleaned.strip_prefix('+').unwrap_or(&cleaned).to_string()),
    };
    let value = match body.as_str() {
        ".inf" => f64::INFINITY,
        ".nan" => f64::NAN,
        _ if body.contains(':') => sexagesimal(&body),
        _ => body.parse().unwrap_or(f64::NAN),
    };
    sign * value
}

fn construct_timestamp(text: &str) -> Option<Yaml> {
    let caps = timestamp_parts().captures(text)?;
    let number = |name: &str| caps.name(name).map(|m| m.as_str().parse::<u32>().unwrap_or(0));
    let date = NaiveDate::from_ymd_opt(caps["year"].parse().ok()?, number("month")?, number("day")?)?;
    let Some(hour) = number("hour") else {
        return Some(Yaml::Date(date));
    };
    let micros = caps.name("fraction").map_or(0, |fraction| {
        let mut digits: String = fraction.as_str().chars().take(6).collect();
        while digits.len() < 6 {
            digits.push('0');
        }
        digits.parse().unwrap_or(0)
    });
    let time = chrono::NaiveTime::from_hms_micro_opt(hour, number("minute")?, number("second")?, micros)?;
    let offset = match (caps.name("sign"), caps.name("tz")) {
        (Some(sign), _) => {
            let minutes = number("tzh")? as i32 * 60 + number("tzm").unwrap_or(0) as i32;
            let minutes = if sign.as_str() == "-" { -minutes } else { minutes };
            Some(FixedOffset::east_opt(minutes * 60)?)
        }
        (None, Some(_)) => Some(FixedOffset::east_opt(0)?),
        (None, None) => None,
    };
    Some(Yaml::DateTime(date.and_time(time), offset))
}

/// PyYAML's implicit typing of a plain scalar.
fn resolve_plain(text: &str) -> Yaml {
    match text {
        "" | "~" | "null" | "Null" | "NULL" => return Yaml::Null,
        _ => {}
    }
    if yaml_bool().is_match(text) {
        return Yaml::Bool(matches!(text.to_lowercase().as_str(), "yes" | "true" | "on"));
    }
    if yaml_float().is_match(text) {
        return Yaml::Float(construct_float(text));
    }
    if yaml_int().is_match(text) {
        return construct_int(text).map(Yaml::Int).unwrap_or_else(|| Yaml::Str(text.to_string()));
    }
    if yaml_timestamp().is_match(text) {
        return construct_timestamp(text).unwrap_or_else(|| Yaml::Str(text.to_string()));
    }
    Yaml::Str(text.to_string())
}

fn resolve_tagged(text: &str, tag: &Tag) -> Yaml {
    if tag.handle != "tag:yaml.org,2002:" {
        return Yaml::Str(text.to_string());
    }
    let fallback = || Yaml::Str(text.to_string());
    match tag.suffix.as_str() {
        "null" => Yaml::Null,
        "bool" => Yaml::Bool(matches!(text.to_lowercase().as_str(), "yes" | "true" | "on")),
        "int" => construct_int(text).map(Yaml::Int).unwrap_or_else(fallback),
        "float" => Yaml::Float(construct_float(text)),
        "timestamp" => construct_timestamp(text).unwrap_or_else(fallback),
        _ => fallback(),
    }
}

enum Building {
    List(Vec<Yaml>),
    Map(Vec<(Yaml, Yaml)>, Option<Yaml>),
}

#[derive(Default)]
struct Loader {
    documents: Vec<Yaml>,
    root: Option<Yaml>,
    stack: Vec<(Building, usize)>,
    anchors: HashMap<usize, Yaml>,
}

impl Loader {
    fn insert(&mut self, node: Yaml, anchor: usize) {
        if anchor > 0 {
            self.anchors.insert(anchor, node.clone());
        }
        match self.stack.last_mut() {
            None => self.root = Some(node),
            Some((Building::List(items), _)) => items.push(node),
            Some((Building::Map(pairs, pending), _)) => match pending.take() {
                None => *pending = Some(node),
                Some(key) => match pairs.iter_mut().find(|(k, _)| *k == key) {
                    // A repeated key keeps its place and takes the last value, like a dict.
                    Some(pair) => pair.1 = node,
                    None => pairs.push((key, node)),
                },
            },
        }
    }
}

impl MarkedEventReceiver for Loader {
    fn on_event(&mut self, event: Event, _mark: Marker) {
        match event {
            Event::DocumentEnd => {
                let root = self.root.take().unwrap_or(Yaml::Null);
                self.documents.push(root);
            }
            Event::SequenceStart(anchor, _) => self.stack.push((Building::List(Vec::new()), anchor)),
            Event::MappingStart(anchor, _) => self.stack.push((Building::Map(Vec::new(), None), anchor)),
            Event::SequenceEnd | Event::MappingEnd => {
                if let Some((building, anchor)) = self.stack.pop() {
                    let node = match building {
                        Building::List(items) => Yaml::List(items),
                        Building::Map(pairs, _) => Yaml::Map(pairs),
                    };
                    self.insert(node, anchor);
                }
            }
            Event::Scalar(text, style, anchor, tag) => {
                let node = match (&tag, style) {
                    (Some(tag), _) => resolve_tagged(&text, tag),
                    (None, TScalarStyle::Plain) => resolve_plain(&text),
                    (None, _) => Yaml::Str(text),
                };
                self.insert(node, anchor);
            }
            Event::Alias(id) => {
                let node = self.anchors.get(&id).cloned().unwrap_or(Yaml::Null);
                self.insert(node, 0);
            }
            Event::Nothing | Event::StreamStart | Event::StreamEnd | Event::DocumentStart => {}
        }
    }
}

/// `yaml.safe_load(text)` (`None` for an empty stream).
pub fn safe_load(text: &str) -> Result<Option<Yaml>, String> {
    let mut loader = Loader::default();
    Parser::new_from_str(text).load(&mut loader, true).map_err(|e| e.to_string())?;
    match loader.documents.len() {
        0 => Ok(None),
        1 => Ok(loader.documents.pop()),
        _ => Err("expected a single document in the stream, but found another document".to_string()),
    }
}

// =============================================================================
// Time zones
// =============================================================================

/// A deadline's time zone, or the zone calendar events are shown in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zone {
    Fixed(FixedOffset),
    Named(chrono_tz::Tz),
    /// The system time zone, applied per date so daylight saving time is respected.
    Local,
}

impl Zone {
    /// Local wall-clock time of an instant in this zone.
    fn local(&self, moment: DateTime<Utc>) -> NaiveDateTime {
        match self {
            Zone::Fixed(offset) => moment.with_timezone(offset).naive_local(),
            Zone::Named(tz) => moment.with_timezone(tz).naive_local(),
            Zone::Local => moment.with_timezone(&chrono::Local).naive_local(),
        }
    }

    /// `naive.replace(tzinfo=zone)` as an instant: an ambiguous time is its
    /// first occurrence, a skipped time uses the offset from before the change.
    fn instant(&self, naive: NaiveDateTime) -> DateTime<Utc> {
        fn resolve<Tz: TimeZone>(tz: &Tz, naive: NaiveDateTime) -> DateTime<Utc> {
            match tz.from_local_datetime(&naive).earliest() {
                Some(aware) => aware.with_timezone(&Utc),
                None => {
                    let before = tz.offset_from_utc_datetime(&(naive - Duration::days(1))).fix();
                    Utc.from_utc_datetime(&(naive - Duration::seconds(i64::from(before.local_minus_utc()))))
                }
            }
        }
        match self {
            Zone::Fixed(offset) => resolve(offset, naive),
            Zone::Named(tz) => resolve(tz, naive),
            Zone::Local => resolve(&chrono::Local, naive),
        }
    }
}

fn hours(minutes: i32) -> Result<Zone, String> {
    FixedOffset::east_opt(minutes * 60).map(Zone::Fixed).ok_or_else(|| {
        "offset must be a timedelta strictly between -timedelta(hours=24) and timedelta(hours=24).".to_string()
    })
}

fn utc_offset_name() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(&RE, r"(?i)^(?:UTC|GMT)\s*([+-])\s*(\d{1,2})(?::?(\d{2}))?$")
}

/// Resolve a deadline time zone (AoE, UTC-8, PST, or an IANA name); unknown
/// zones count as AoE.
pub fn zone(name: &Value) -> Result<Zone, String> {
    let text = match name {
        value if py::truthy(value) => py::py_str(value),
        _ => "AoE".to_string(),
    };
    let text = text.trim();
    if let Some((_, minutes)) = FIXED_ZONES.iter().find(|(key, _)| *key == text.to_lowercase()) {
        return hours(*minutes);
    }
    if let Some(caps) = utc_offset_name().captures(text) {
        let sign = if &caps[1] == "+" { 1 } else { -1 };
        let minutes =
            caps[2].parse::<i32>().unwrap_or(0) * 60 + caps.get(3).map_or(0, |m| m.as_str().parse().unwrap_or(0));
        return hours(sign * minutes);
    }
    match text.parse::<chrono_tz::Tz>() {
        Ok(tz) => Ok(Zone::Named(tz)),
        Err(_) => hours(-12 * 60),
    }
}

/// A YAML time-zone field as a JSON value (for [`zone`]).
fn zone_name(value: Option<&Yaml>) -> Value {
    match value {
        Some(v) if v.truthy() => json!(v.py_str()),
        _ => Value::Null,
    }
}

/// Parse a deadline value into an instant.
fn instant(value: Option<&Yaml>, zone_value: Option<&Yaml>) -> Result<Option<DateTime<Utc>>, String> {
    let naive = match value {
        Some(Yaml::DateTime(naive, Some(offset))) => {
            return Ok(offset.from_local_datetime(naive).single().map(|aware| aware.with_timezone(&Utc)));
        }
        Some(Yaml::DateTime(naive, None)) => *naive,
        Some(Yaml::Date(date)) => date.and_hms_opt(23, 59, 59).expect("valid time"),
        other => {
            let text = other.map(Yaml::py_str).unwrap_or_else(|| "None".to_string());
            match py::parse_iso_datetime(text.trim()) {
                Some(PyDateTime::Aware(aware)) => return Ok(Some(aware.with_timezone(&Utc))),
                Some(PyDateTime::Naive(naive)) => naive,
                None => return Ok(None),
            }
        }
    };
    Ok(Some(zone(&zone_name(zone_value))?.instant(naive)))
}

fn iso_date_of(value: Option<&Yaml>) -> Option<String> {
    match value {
        Some(Yaml::DateTime(naive, _)) => Some(naive.date().format("%Y-%m-%d").to_string()),
        Some(Yaml::Date(date)) => Some(date.format("%Y-%m-%d").to_string()),
        other => {
            let text = other.map(Yaml::py_str).unwrap_or_else(|| "None".to_string());
            py::parse_iso_date(&first_chars(text.trim(), 10)).map(|d| d.format("%Y-%m-%d").to_string())
        }
    }
}

fn date_range() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(&RE, r"^\s*([A-Za-z]+)\s+([0-9]{1,2})\s*(?:-|–)\s*(?:([A-Za-z]+)\s+)?([0-9]{1,2}),?\s*([0-9]{4})?\s*$")
}

/// Parse "June 3-7, 2026" or "September 27 - October 1, 2026" into ISO start and end.
fn range_from_text(text: Option<&Yaml>, year: &Yaml) -> (Option<String>, Option<String>) {
    let text = Yaml::text_or(text, "");
    let Some(caps) = date_range().captures(&text) else {
        return (None, None);
    };
    let month = |name: &str| MONTHS.iter().position(|m| *m == name.to_lowercase()).map(|i| i as i64 + 1);
    let first_month = month(&caps[1]);
    let last_month = month(caps.get(3).map_or(&caps[1], |m| m.as_str()));
    let end_year = match caps.get(5) {
        Some(y) => y.as_str().parse::<i64>().ok(),
        None => match year {
            Yaml::Int(y) => Some(*y),
            Yaml::Bool(b) => Some(i64::from(*b)),
            _ => None,
        },
    };
    let (Some(end_year), Some(first_month), Some(last_month)) = (end_year, first_month, last_month) else {
        return (None, None);
    };
    let start_year = if first_month > last_month { end_year - 1 } else { end_year };
    let day = |i: usize| caps[i].parse::<i64>().unwrap_or(0);
    let date = |y: i64, m: i64, d: i64| {
        if !(1..=9999).contains(&y) {
            return None;
        }
        NaiveDate::from_ymd_opt(y as i32, m as u32, d as u32).map(|d| d.format("%Y-%m-%d").to_string())
    };
    match (date(start_year, first_month, day(2)), date(end_year, last_month, day(4))) {
        (Some(start), Some(end)) => (Some(start), Some(end)),
        _ => (None, None),
    }
}

fn safe_link(value: Option<&Yaml>) -> Option<String> {
    let link = Yaml::text_or(value, "").trim().to_string();
    (link.starts_with("https://") || link.starts_with("http://")).then_some(link)
}

/// `'May 6, 23:59'` in a deadline's own time zone.
fn stamp(moment: DateTime<Utc>, zone_value: &Value) -> Result<String, String> {
    let local = zone(zone_value)?.local(moment);
    Ok(format!("{} {}, {}", local.format("%b"), local.day(), local.format("%H:%M")))
}

/// `"start"` or `"end"` for a rebuttal (author response) date; `None` for any other deadline type.
fn rebuttal_role(kind: &str, label: &str) -> Option<&'static str> {
    if !rebuttal_type().is_match(kind) {
        return None;
    }
    if ["start", "begin", "open"].iter().any(|word| kind.contains(word)) {
        return Some("start");
    }
    if ["end", "close"].iter().any(|word| kind.contains(word)) {
        return Some("end");
    }
    Some(if start_word().is_match(label) { "start" } else { "end" })
}

/// Name a period after its labels ("Rebuttal Period Starts" / "Ends"); labels
/// that disagree get a generic name.
fn period_name(start_label: &str, end_label: &str) -> String {
    let whitespace = {
        static RE: OnceLock<Regex> = OnceLock::new();
        regex(&RE, r"\s+")
    };
    let names: Vec<String> = [start_label, end_label]
        .iter()
        .map(|label| {
            let without_edge = period_edge().replacen(label, 1, "");
            whitespace.replace_all(&without_edge, " ").trim_matches(|c| " -–:,".contains(c)).to_string()
        })
        .collect();
    if !names[0].is_empty() && names[0].to_lowercase() == names[1].to_lowercase() {
        let mut chars = names[0].chars();
        let first = chars.next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
        return first + chars.as_str();
    }
    "Rebuttal period".to_string()
}

struct Point {
    moment: DateTime<Utc>,
    role: &'static str,
    label: String,
    zone: String,
}

fn utc_iso(moment: DateTime<Utc>) -> String {
    py::isoformat_aware(&moment)
}

/// Pair each rebuttal start with the next end in time order; a date without a
/// partner stays on its own.
fn rebuttal_periods(mut points: Vec<Point>) -> Vec<Value> {
    fn single(point: &Point) -> Value {
        let label = if point.label.is_empty() {
            if point.role == "start" { "Rebuttal period starts" } else { "Rebuttal deadline" }.to_string()
        } else {
            point.label.clone()
        };
        let at = utc_iso(point.moment);
        json!({
            "label": first_chars(&label, 120),
            "start": (point.role == "start").then(|| at.clone()),
            "end": (point.role == "end").then_some(at),
            "timezone": point.zone,
        })
    }
    points.sort_by_key(|point| point.moment);
    let mut periods = Vec::new();
    let mut pending: Option<Point> = None;
    for point in points {
        if point.role == "start" {
            if let Some(previous) = pending.take() {
                periods.push(single(&previous));
            }
            pending = Some(point);
        } else if let Some(start) = pending.take() {
            periods.push(json!({
                "label": first_chars(&period_name(&start.label, &point.label), 120),
                "start": utc_iso(start.moment),
                "end": utc_iso(point.moment),
                "timezone": point.zone,
            }));
        } else {
            periods.push(single(&point));
        }
    }
    if let Some(start) = pending {
        periods.push(single(&start));
    }
    periods
}

fn zone_label(value: Option<&Yaml>) -> String {
    first_chars(&Yaml::text_or(value, "AoE"), 40)
}

fn text_map(pairs: &[(&str, Option<&Yaml>)]) -> Yaml {
    Yaml::Map(
        pairs.iter().map(|(key, value)| (Yaml::Str(key.to_string()), value.cloned().unwrap_or(Yaml::Null))).collect(),
    )
}

/// Turn one conference's YAML file into compact editions with UTC deadline
/// and rebuttal instants.
pub fn normalize_editions(text: &str) -> Result<Vec<Value>, String> {
    let data = safe_load(text)?.filter(Yaml::truthy).unwrap_or(Yaml::List(Vec::new()));
    let Yaml::List(entries) = data else {
        return Err("conference data must be a list of editions".to_string());
    };
    let mut editions = Vec::new();
    for entry in &entries {
        let Some(year @ (Yaml::Int(_) | Yaml::Bool(_))) = entry.get("year") else { continue };
        let mut raw_deadlines = match entry.get("deadlines") {
            Some(Yaml::List(items)) => items.clone(),
            _ => Vec::new(),
        };
        if raw_deadlines.is_empty() {
            // Older editions use flat fields with one shared time zone.
            let zone = entry.get("timezone");
            let abstract_label = Yaml::Str("Abstract deadline".into());
            let paper_label = Yaml::Str("Paper deadline".into());
            raw_deadlines = vec![
                text_map(&[
                    ("type", Some(&Yaml::Str("abstract".into()))),
                    ("label", Some(&abstract_label)),
                    ("date", entry.get("abstract_deadline")),
                    ("timezone", zone),
                ]),
                text_map(&[
                    ("type", Some(&Yaml::Str("paper".into()))),
                    ("label", Some(&paper_label)),
                    ("date", entry.get("deadline")),
                    ("timezone", zone),
                ]),
            ];
        }
        let mut deadlines = Vec::new();
        let mut rebuttal_points = Vec::new();
        for item in &raw_deadlines {
            if !matches!(item, Yaml::Map(_)) {
                continue;
            }
            match item.get("date") {
                None | Some(Yaml::Null) => continue,
                Some(Yaml::Str(s)) if s.is_empty() => continue,
                _ => {}
            }
            let kind = Yaml::text_or(item.get("type"), "").to_lowercase();
            let label = Yaml::text_or(item.get("label"), "");
            if let Some(role) = rebuttal_role(&kind, &label) {
                if side_track().is_match(&label) {
                    continue;
                }
                if let Some(moment) = instant(item.get("date"), item.get("timezone"))? {
                    rebuttal_points.push(Point { moment, role, label, zone: zone_label(item.get("timezone")) });
                }
                continue;
            }
            let is_abstract = ABSTRACT_TYPES.contains(&kind.as_str());
            if !(is_abstract || SUBMISSION_TYPES.contains(&kind.as_str()))
                || side_track().is_match(&label)
                || rebuttal_word().is_match(&label)
            {
                continue;
            }
            // "Registration" can also mean attendee registration; only paper or abstract registration counts.
            static PAPER_OR_ABSTRACT: OnceLock<Regex> = OnceLock::new();
            if kind == "registration" && !regex(&PAPER_OR_ABSTRACT, r"(?i)\b(paper|abstract)\b").is_match(&label) {
                continue;
            }
            let Some(moment) = instant(item.get("date"), item.get("timezone"))? else { continue };
            let default_label = if is_abstract { "Abstract deadline" } else { "Paper deadline" };
            deadlines.push(json!({
                "kind": if is_abstract { "abstract" } else { "submission" },
                "label": first_chars(if label.is_empty() { default_label } else { &label }, 120),
                "utc": utc_iso(moment),
                "timezone": zone_label(item.get("timezone")),
            }));
        }
        if rebuttal_points.is_empty() {
            // Older editions keep the rebuttal period in flat fields that share the edition's time zone.
            for (role, field) in [("start", "rebuttal_period_start"), ("end", "rebuttal_period_end")] {
                let value = entry.get(field).filter(|v| v.truthy());
                if value.is_none() {
                    continue;
                }
                if let Some(moment) = instant(value, entry.get("timezone"))? {
                    rebuttal_points.push(Point {
                        moment,
                        role,
                        label: format!("Rebuttal period {role}"),
                        zone: zone_label(entry.get("timezone")),
                    });
                }
            }
        }
        let (mut start, mut end) = (iso_date_of(entry.get("start")), iso_date_of(entry.get("end")));
        if start.is_none() || end.is_none() {
            (start, end) = range_from_text(entry.get("date"), year);
        }
        let place: Vec<String> = [entry.get("city"), entry.get("country")]
            .into_iter()
            .flatten()
            .filter(|p| p.truthy())
            .map(Yaml::py_str)
            .collect();
        editions.push(json!({
            "year": year.to_json(),
            "fullName": first_chars(&Yaml::text_or(entry.get("full_name"), ""), 200),
            "link": safe_link(entry.get("link")),
            "place": first_chars(&place.join(", "), 120),
            "dateText": first_chars(&Yaml::text_or(entry.get("date"), ""), 80),
            "start": start,
            "end": end,
            "deadlines": deadlines,
            "rebuttals": rebuttal_periods(rebuttal_points),
        }));
    }
    Ok(editions)
}

// =============================================================================
// Fetching and the cache
// =============================================================================

/// Download one conference file (`None` until the user asks for the layer).
pub type Fetcher<'a> = dyn Fn(&str) -> Result<String, String> + Sync + 'a;

fn raw_url(conference_id: &str) -> String {
    format!("{RAW_URL}{conference_id}.yml")
}

/// Fetch a URL over HTTPS (at most 1 MB, like the original).
pub fn fetch_text(url: &str) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(FETCH_TIMEOUT_SECONDS)))
        .user_agent("schedule-everything")
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(|e| format!("<urlopen error {e}>"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP Error {}: {}", status.as_u16(), status.canonical_reason().unwrap_or("")));
    }
    let mut bytes = Vec::new();
    response.body_mut().as_reader().take(1_000_000).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| utf8_error(e.as_bytes(), &e.utf8_error()))
}

fn cache_path() -> PathBuf {
    match std::env::var("REMINDER_CONFERENCE_CACHE_PATH") {
        Ok(path) if !path.is_empty() => crate::paths::expand_user(&path),
        _ => crate::paths::resolve_config_root_dir().join("cache").join("ai_conferences.json"),
    }
}

fn read_cache() -> Option<Dict> {
    let text = std::fs::read_to_string(cache_path()).ok()?;
    match crate::web::pyjson::loads(&text).ok()? {
        Value::Object(cache) if cache.get("version").and_then(Value::as_f64) == Some(CACHE_VERSION as f64) => {
            Some(cache)
        }
        _ => None,
    }
}

/// One conference's editions, or why they could not be fetched.
type Editions = Result<Vec<Value>, String>;

/// Fetch every conference, at most [`FETCH_WORKERS`] at a time, in list order.
fn fetch_all(fetch: &Fetcher<'_>) -> Vec<Editions> {
    let next = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<Editions>>> = CONFERENCES.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..FETCH_WORKERS {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    let Some((conference_id, _, _)) = CONFERENCES.get(index) else { break };
                    let result = fetch(&raw_url(conference_id))
                        .and_then(|text| normalize_editions(&text))
                        .map_err(|error| first_chars(&error, 200));
                    *results[index].lock().unwrap_or_else(|p| p.into_inner()) = Some(result);
                }
            });
        }
    });
    results
        .into_iter()
        .map(|slot| slot.into_inner().unwrap_or_else(|p| p.into_inner()).unwrap_or(Ok(Vec::new())))
        .collect()
}

/// Fetch every conference; keep the previous data for any conference that fails.
fn refresh(previous: Option<&Dict>, now: DateTime<Utc>, fetch: &Fetcher<'_>) -> Result<Dict, String> {
    let results = fetch_all(fetch);
    let old = previous.and_then(|cache| cache.get("conferences")).and_then(Value::as_object);
    let mut conferences = Dict::new();
    let mut errors = Dict::new();
    for ((conference_id, _, _), result) in CONFERENCES.iter().zip(results) {
        let editions = match result {
            Ok(editions) => Value::Array(editions),
            Err(error) => {
                errors.insert(conference_id.to_string(), json!(error));
                old.and_then(|o| o.get(*conference_id)).cloned().unwrap_or_else(|| json!([]))
            }
        };
        conferences.insert(conference_id.to_string(), editions);
    }
    if errors.len() == CONFERENCES.len() {
        let first = errors.values().next().and_then(Value::as_str).unwrap_or_default().to_string();
        return Err(first);
    }
    let mut cache = Dict::new();
    cache.insert("version".into(), json!(CACHE_VERSION));
    cache.insert("source".into(), json!(SOURCE_URL));
    cache.insert("fetchedAt".into(), json!(utc_iso(now)));
    cache.insert("conferences".into(), Value::Object(conferences));
    cache.insert("errors".into(), Value::Object(errors));
    let path = cache_path();
    let write = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, serde_json::to_string(&cache).expect("JSON values serialize"))?;
        std::fs::rename(&temporary, &path)
    };
    write().map_err(|e| crate::web::services::os_error(&e, &path))?;
    Ok(cache)
}

/// `(cache, error)`: fetch only when forced, missing, or older than a week.
pub fn load_conference_data(
    refresh_now: bool,
    now: DateTime<Utc>,
    fetch: &Fetcher<'_>,
) -> (Option<Dict>, Option<String>) {
    let cache = read_cache();
    let fresh = cache.as_ref().is_some_and(|cache| {
        match cache.get("fetchedAt").and_then(Value::as_str).and_then(py::parse_iso_datetime) {
            Some(PyDateTime::Aware(fetched)) => now.signed_duration_since(fetched) < Duration::days(CACHE_TTL_DAYS),
            _ => false,
        }
    });
    if cache.is_some() && fresh && !refresh_now {
        return (cache, None);
    }
    match refresh(cache.as_ref(), now, fetch) {
        Ok(fetched) => (Some(fetched), None),
        Err(error) => (cache, Some(first_chars(&format!("Could not fetch conference dates: {error}"), 240))),
    }
}

// =============================================================================
// Calendar events
// =============================================================================

fn key<'a>(value: &'a Value, name: &str) -> Result<&'a Value, String> {
    match value {
        Value::Object(map) => map.get(name).ok_or_else(|| py::repr_str(name)),
        other => Err(format!("'{}' object is not subscriptable", crate::web::services::py_type_name(other))),
    }
}

fn instant_of(value: &Value) -> Result<DateTime<Utc>, String> {
    let text = value.as_str().ok_or("fromisoformat: argument must be str")?;
    py::parse_iso_datetime(text)
        .map(PyDateTime::to_utc)
        .ok_or_else(|| format!("Invalid isoformat string: {}", py::repr_str(text)))
}

/// Deadlines, rebuttal periods, and conference dates that fall in `year`;
/// deadlines and rebuttals are shown in `local` time.
pub fn calendar_events(cache: &Dict, year: i64, local: Zone) -> Result<Vec<Value>, String> {
    let mut events = Vec::new();
    let empty = Dict::new();
    let conferences = match cache.get("conferences") {
        None => &empty,
        Some(Value::Object(map)) => map,
        Some(other) => {
            return Err(format!("'{}' object has no attribute 'items'", crate::web::services::py_type_name(other)));
        }
    };
    for (conference_id, editions) in conferences {
        let Some((_, name, group)) = CONFERENCES.iter().find(|(id, _, _)| id == conference_id) else { continue };
        for edition in editions.as_array().into_iter().flatten() {
            let edition_year = py::py_str(key(edition, "year")?);
            let field = |name: &str| edition.get(name).cloned().unwrap_or(Value::Null);
            let mut common = Dict::new();
            common.insert("conference".into(), json!(conference_id));
            common.insert("name".into(), json!(name));
            common.insert("group".into(), json!(group));
            common.insert("edition".into(), key(edition, "year")?.clone());
            common.insert("fullName".into(), field("fullName"));
            common.insert("link".into(), field("link"));
            common.insert("place".into(), field("place"));
            let event = |extra: Value| {
                let mut event = common.clone();
                event.extend(extra.as_object().cloned().unwrap_or_default());
                Value::Object(event)
            };

            let deadlines = edition.get("deadlines").and_then(Value::as_array).cloned().unwrap_or_default();
            for (index, deadline) in deadlines.iter().enumerate() {
                let moment = instant_of(key(deadline, "utc")?)?;
                let at = local.local(moment);
                if i64::from(at.year()) != year {
                    continue;
                }
                let kind = py::py_str(key(deadline, "kind")?);
                let timezone = key(deadline, "timezone")?;
                events.push(event(json!({
                    "id": format!("{conference_id}{edition_year}-{kind}-{index}"),
                    "kind": key(deadline, "kind")?,
                    "label": key(deadline, "label")?,
                    "date": at.date().format("%Y-%m-%d").to_string(),
                    "endDate": null,
                    "time": at.format("%H:%M").to_string(),
                    "sourceTime": format!("{} {}", stamp(moment, timezone)?, py::py_str(timezone)),
                })));
            }

            let rebuttals = edition.get("rebuttals").and_then(Value::as_array).cloned().unwrap_or_default();
            for (index, rebuttal) in rebuttals.iter().enumerate() {
                // A period spans its days in local time and keeps its closing time; a lone start or end is one day.
                let mut moments = Vec::new();
                for name in ["start", "end"] {
                    if let Some(value) = rebuttal.get(name).filter(|v| py::truthy(v)) {
                        moments.push(instant_of(value)?);
                    }
                }
                let locals: Vec<NaiveDateTime> = moments.iter().map(|m| local.local(*m)).collect();
                let (Some(first), Some(last)) = (locals.first(), locals.last()) else { continue };
                if !(i64::from(first.year()) <= year && year <= i64::from(last.year())) {
                    continue;
                }
                let timezone = key(rebuttal, "timezone")?;
                let stamps: Result<Vec<String>, String> = moments.iter().map(|m| stamp(*m, timezone)).collect();
                events.push(event(json!({
                    "id": format!("{conference_id}{edition_year}-rebuttal-{index}"),
                    "kind": "rebuttal",
                    "label": key(rebuttal, "label")?,
                    "date": first.date().format("%Y-%m-%d").to_string(),
                    "endDate": (moments.len() == 2).then(|| last.date().format("%Y-%m-%d").to_string()),
                    "time": last.format("%H:%M").to_string(),
                    "sourceTime": format!("{} {}", stamps?.join(" – "), py::py_str(timezone)),
                })));
            }

            let (start, end) = (field("start"), field("end"));
            if py::truthy(&start) && py::truthy(&end) {
                let (start_year, end_year) = (first_chars(&py::py_str(&start), 4), first_chars(&py::py_str(&end), 4));
                let year_text = year.to_string();
                if start_year <= year_text && year_text <= end_year {
                    let date_text = field("dateText");
                    events.push(event(json!({
                        "id": format!("{conference_id}{edition_year}-conference"),
                        "kind": "conference",
                        "label": format!("{name} {edition_year}"),
                        "date": start,
                        "endDate": end,
                        "time": null,
                        "sourceTime": if py::truthy(&date_text) { date_text } else { Value::Null },
                    })));
                }
            }
        }
    }
    let sort_key =
        |event: &Value| ["date", "name", "kind"].map(|name| event.get(name).map(py::py_str).unwrap_or_default());
    events.sort_by_cached_key(sort_key);
    Ok(events)
}

// =============================================================================
// Browser API
// =============================================================================

/// `conference_calendar`: AI conference events for one calendar year.
pub fn conference_calendar(payload: &Dict) -> WebResult {
    conference_calendar_with(payload, &fetch_text, Zone::Local)
}

/// [`conference_calendar`] with an explicit downloader and display zone.
pub fn conference_calendar_with(payload: &Dict, fetch: &Fetcher<'_>, local: Zone) -> WebResult {
    let year = payload.get("year").and_then(py::py_int).ok_or_else(|| invalid("year must be an integer."))?;
    if !(1970..=2200).contains(&year) {
        return Err(invalid("year is out of range."));
    }
    let refresh_now = payload.get("refresh") == Some(&Value::Bool(true));
    let (cache, error) = load_conference_data(refresh_now, crate::clock::now_utc(), fetch);
    let events = match &cache {
        Some(cache) => calendar_events(cache, year, local).map_err(WebError::internal)?,
        None => Vec::new(),
    };
    let conferences: Vec<Value> =
        CONFERENCES.iter().map(|(id, name, group)| json!({"id": id, "name": name, "group": group})).collect();
    Ok(json!({
        "year": year,
        "source": SOURCE_URL,
        "fetchedAt": cache.as_ref().and_then(|c| c.get("fetchedAt")).cloned(),
        "error": error,
        "conferences": conferences,
        "events": events,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    const NEURIPS: &str = "
- title: NeurIPS
  year: 2026
  id: neurips26
  full_name: Conference on Neural Information Processing Systems
  link: https://neurips.cc/
  deadlines:
    - type: abstract
      label: Abstract registration deadline
      date: '2026-05-04 23:59:59'
      timezone: AoE
    - type: paper
      label: Paper submission deadline
      date: '2026-05-06 23:59:59'
      timezone: AoE
    - type: notification
      label: Author notification
      date: '2026-09-24 23:59:59'
      timezone: AoE
    - type: submission
      label: Workshop Proposal Submission
      date: '2026-04-10 23:59:59'
      timezone: AoE
    - type: paper
      label: Paper video submission deadline
      date: '2026-05-10 23:59:59'
      timezone: AoE
    - type: registration
      label: Early registration
      date: '2026-10-01 23:59:59'
      timezone: AoE
  city: Sydney
  country: Australia
  date: December 6-12, 2026
  start: '2026-12-06'
  end: '2026-12-12'
";
    const CVPR: &str = "
- title: CVPR
  year: 2025
  id: cvpr25
  link: javascript:alert(1)
  deadline: '2024-11-14 23:59:00'
  abstract_deadline: '2024-11-07 23:59:00'
  timezone: UTC-8
  date: June 10-17, 2025
  rebuttal_period_end: '2025-01-30 23:59:00'
- title: CVPR
  year: 2026
  id: cvpr26
  link: https://cvpr.thecvf.com/
  deadlines:
  - type: registration
    label: Paper Registration
    date: '2025-11-06 23:59:59'
    timezone: AoE
  - type: paper
    label: Paper Submission
    date: '2025-11-13 23:59:59'
    timezone: AoE
  - type: rebuttal_start
    label: Rebuttal Period Starts
    date: '2026-01-22 23:59:59'
    timezone: AoE
  - type: rebuttal_end
    label: Rebuttal Period Ends
    date: '2026-01-29 23:59:59'
    timezone: AoE
  - type: rebuttal_end
    label: Workshop paper rebuttal deadline
    date: '2026-03-01 23:59:59'
    timezone: AoE
  date: June 3-7, 2026
  start: 2026-06-03
  end: 2026-06-07
  rebuttal_period_end: '2026-01-29 23:59:59'
";
    const ACL: &str = "
- title: ACL
  year: 2025
  id: acl25
  deadlines:
    - type: paper
      label: Paper submission deadline
      date: '2025-02-15 23:59:59'
      timezone: AoE
    - type: author_response
      label: Author response period ends
      date: '2025-04-03 23:59:59'
      timezone: AoE
    - type: author_response
      label: Author response period begins
      date: '2025-03-27 23:59:59'
      timezone: AoE
  date: July 27 - August 1, 2025
";
    const ECCV: &str = "
- title: ECCV
  year: 2026
  id: eccv26
  deadlines:
  - type: paper
    label: Paper Submission
    date: '2026-03-05 22:00:00'
    timezone: UTC
  - type: rebuttal_end
    label: Rebuttal Deadline
    date: '2026-05-11 21:00:00'
    timezone: UTC
  date: September 8-12, 2026
";
    const IROS: &str = "
- title: IROS
  year: 2026
  id: iros26
  link: https://www.iros2026.org/
  deadlines:
  - type: submission
    label: Paper Submission
    date: '2026-03-01 23:59:59'
    timezone: PST
  date: September 27 - October 1, 2026
";

    fn pacific() -> Zone {
        Zone::Fixed(FixedOffset::west_opt(8 * 3600).unwrap())
    }

    /// A downloader serving the sample files; other conferences are missing.
    struct FakeSource {
        calls: Mutex<Vec<String>>,
    }

    impl FakeSource {
        fn new() -> Self {
            Self { calls: Mutex::new(Vec::new()) }
        }

        fn fetch(&self, url: &str) -> Result<String, String> {
            let id = url.rsplit('/').next().unwrap().trim_end_matches(".yml").to_string();
            self.calls.lock().unwrap().push(id.clone());
            match id.as_str() {
                "neurips" => Ok(NEURIPS.to_string()),
                "cvpr" => Ok(CVPR.to_string()),
                "iros" => Ok(IROS.to_string()),
                "acl" => Ok(ACL.to_string()),
                "eccv" => Ok(ECCV.to_string()),
                _ => Err("404 Not Found".to_string()),
            }
        }

        fn count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    fn env() -> TestEnv {
        let mut env = TestEnv::new();
        let cache = env.root().join("cache").join("ai_conferences.json");
        env.set_var("REMINDER_CONFERENCE_CACHE_PATH", cache.to_str().unwrap());
        env
    }

    fn utc(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap()
    }

    #[test]
    fn normalizes_deadline_lists_legacy_fields_and_date_text() {
        let editions = normalize_editions(CVPR).unwrap();
        let (legacy, current) = (&editions[0], &editions[1]);
        let kinds: Vec<&Value> = legacy["deadlines"].as_array().unwrap().iter().map(|d| &d["kind"]).collect();
        assert_eq!(kinds, [&json!("abstract"), &json!("submission")]);
        assert_eq!(legacy["deadlines"][1]["utc"], json!("2024-11-15T07:59:00+00:00"), "UTC-8");
        assert_eq!(legacy["link"], Value::Null, "only http(s) links are kept");
        assert_eq!(
            (&legacy["start"], &legacy["end"]),
            (&json!("2025-06-10"), &json!("2025-06-17")),
            "parsed from the date text"
        );
        assert_eq!(
            (&current["start"], &current["end"]),
            (&json!("2026-06-03"), &json!("2026-06-07")),
            "bare YAML dates"
        );
        let iros = &normalize_editions(IROS).unwrap()[0];
        assert_eq!((&iros["start"], &iros["end"]), (&json!("2026-09-27"), &json!("2026-10-01")));
        assert_eq!(iros["deadlines"][0]["utc"], json!("2026-03-02T07:59:59+00:00"), "PST");
        let neurips = &normalize_editions(NEURIPS).unwrap()[0];
        // Notifications, side tracks (workshops, videos), and attendee registration are left out.
        let labels: Vec<&Value> = neurips["deadlines"].as_array().unwrap().iter().map(|d| &d["label"]).collect();
        assert_eq!(labels, [&json!("Abstract registration deadline"), &json!("Paper submission deadline")]);
        let current_deadlines: Vec<(&Value, &Value)> =
            current["deadlines"].as_array().unwrap().iter().map(|d| (&d["kind"], &d["label"])).collect();
        assert_eq!(
            current_deadlines,
            [(&json!("abstract"), &json!("Paper Registration")), (&json!("submission"), &json!("Paper Submission"))]
        );
        assert_eq!(neurips["place"], json!("Sydney, Australia"));
        assert_eq!(neurips["year"], json!(2026));
        assert_eq!(neurips["fullName"], json!("Conference on Neural Information Processing Systems"));
        assert_eq!(neurips["dateText"], json!("December 6-12, 2026"));
    }

    #[test]
    fn rebuttal_dates_pair_into_periods() {
        let editions = normalize_editions(CVPR).unwrap();
        // A start and an end make one period; side tracks and the duplicate flat field are left out.
        assert_eq!(
            editions[1]["rebuttals"],
            json!([{"label": "Rebuttal Period", "start": "2026-01-23T11:59:59+00:00", "end": "2026-01-30T11:59:59+00:00", "timezone": "AoE"}])
        );
        // Older editions keep the rebuttal dates in flat fields with the edition's time zone.
        assert_eq!(
            editions[0]["rebuttals"],
            json!([{"label": "Rebuttal period end", "start": null, "end": "2025-01-31T07:59:00+00:00", "timezone": "UTC-8"}])
        );
        // "author_response" dates say in the label whether they begin or end the period, in any order.
        let acl = &normalize_editions(ACL).unwrap()[0];
        let rebuttal = &acl["rebuttals"][0];
        assert_eq!(acl["rebuttals"].as_array().unwrap().len(), 1);
        assert_eq!(rebuttal["label"], json!("Author response period"));
        assert!(rebuttal["start"].as_str().unwrap().starts_with("2025-03-28"));
        assert!(rebuttal["end"].as_str().unwrap().starts_with("2025-04-04"));
        assert_eq!(acl["deadlines"].as_array().unwrap().len(), 1);
        assert_eq!(acl["deadlines"][0]["kind"], json!("submission"));
        // A conference that publishes only one end of the period keeps that single date.
        assert_eq!(
            normalize_editions(ECCV).unwrap()[0]["rebuttals"],
            json!([{"label": "Rebuttal Deadline", "start": null, "end": "2026-05-11T21:00:00+00:00", "timezone": "UTC"}])
        );
        let lone = normalize_editions(
            "- year: 2026\n  deadlines:\n  - {type: rebuttal_start, label: Rebuttal opens, date: '2026-02-01 00:00:00', timezone: UTC}\n",
        )
        .unwrap();
        assert_eq!(
            lone[0]["rebuttals"],
            json!([{"label": "Rebuttal opens", "start": "2026-02-01T00:00:00+00:00", "end": null, "timezone": "UTC"}])
        );
    }

    #[test]
    fn rebuttal_period_names() {
        for (start, end, name) in [
            ("Rebuttal Period Starts", "Rebuttal Period Ends", "Rebuttal Period"),
            (
                "Author + Reviewer + AC Discussion Period starts",
                "Author + Reviewer + AC Discussion Period ends",
                "Author + Reviewer + AC Discussion Period",
            ),
            (
                "Rebuttal period start (First Cycle)",
                "Rebuttal period end (First Cycle)",
                "Rebuttal period (First Cycle)",
            ),
            ("Start of the rebuttal", "End of the rebuttal", "Rebuttal"),
            ("Initial author response due", "Discussion period ends", "Rebuttal period"),
        ] {
            assert_eq!(period_name(start, end), name);
        }
    }

    #[test]
    fn time_zone_names() {
        let moment = utc(2026, 1, 1) + Duration::hours(12);
        let offset = |name: &str| {
            let local = zone(&json!(name)).unwrap().local(moment);
            (local - moment.naive_utc()).num_minutes()
        };
        for (name, minutes) in
            [("AoE", -720), ("UTC", 0), ("UTC+5:30", 330), ("GMT-3", -180), ("PST", -480), ("Mars/Olympus", -720)]
        {
            assert_eq!(offset(name), minutes, "{name}");
        }
        assert_eq!(offset("Europe/Vienna"), 60);
        assert_eq!(offset(" ist "), 330);
        assert_eq!(zone(&Value::Null).unwrap(), zone(&json!("AoE")).unwrap());
        assert!(zone(&json!("UTC+25")).is_err());
    }

    #[test]
    fn yaml_values_follow_pyyaml_types() {
        let doc = "a: 2026-06-03\nb: '2026-06-03'\nc: 2026-05-04 23:59:59\nd: 2026-05-04T23:59:59Z\ne: yes\nf: 1:30\ng: 017\nh: ~\ni: 1.5\nj: 1e5\nk: \"7\"\nl: 2026-05-04 23:59:59 -5\n";
        let yaml = safe_load(doc).unwrap().unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 6, 3).unwrap();
        let datetime = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap().and_hms_opt(23, 59, 59).unwrap();
        assert_eq!(yaml.get("a"), Some(&Yaml::Date(date)));
        assert_eq!(yaml.get("b"), Some(&Yaml::Str("2026-06-03".into())));
        assert_eq!(yaml.get("c"), Some(&Yaml::DateTime(datetime, None)));
        assert_eq!(yaml.get("d"), Some(&Yaml::DateTime(datetime, FixedOffset::east_opt(0))));
        assert_eq!(yaml.get("e"), Some(&Yaml::Bool(true)));
        assert_eq!(yaml.get("f"), Some(&Yaml::Int(90)));
        assert_eq!(yaml.get("g"), Some(&Yaml::Int(15)));
        assert_eq!(yaml.get("h"), Some(&Yaml::Null));
        assert_eq!(yaml.get("i"), Some(&Yaml::Float(1.5)));
        assert_eq!(yaml.get("j"), Some(&Yaml::Str("1e5".into())));
        assert_eq!(yaml.get("k"), Some(&Yaml::Str("7".into())));
        assert_eq!(yaml.get("l").unwrap().py_str(), "2026-05-04 23:59:59-05:00");
        assert_eq!(safe_load("").unwrap(), None);
        assert!(safe_load("a: 1\n---\nb: 2\n").is_err());
        assert_eq!(normalize_editions("").unwrap(), Vec::<Value>::new());
        assert_eq!(normalize_editions("a: 1").unwrap_err(), "conference data must be a list of editions");
        // An unquoted date means the end of that day in the deadline's zone; a quoted one, midnight.
        let typed = normalize_editions("- year: 2026\n  deadline: 2026-05-04\n  timezone: UTC\n").unwrap();
        assert_eq!(typed[0]["deadlines"][0]["utc"], json!("2026-05-04T23:59:59+00:00"));
        let text = normalize_editions("- year: 2026\n  deadline: '2026-05-04'\n  timezone: UTC\n").unwrap();
        assert_eq!(text[0]["deadlines"][0]["utc"], json!("2026-05-04T00:00:00+00:00"));
    }

    #[test]
    fn calendar_api_returns_year_events_in_local_time() {
        let mut env = env();
        env.set_now("2026-11-04T10:00:00");
        let source = FakeSource::new();
        let fetch = |url: &str| source.fetch(url);
        let request = |value: Value| conference_calendar_with(value.as_object().unwrap(), &fetch, pacific());
        let data = request(json!({"year": 2026})).unwrap();
        assert_eq!(data["error"], Value::Null);
        let ids: Vec<&Value> = data["conferences"].as_array().unwrap().iter().map(|c| &c["id"]).collect();
        assert_eq!(ids[..3], [&json!("neurips"), &json!("iclr"), &json!("icml")]);
        let events: HashMap<String, Value> = data["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| (e["id"].as_str().unwrap().to_string(), e.clone()))
            .collect();
        let paper = &events["neurips2026-submission-1"];
        // 23:59 AoE on May 6 is 03:59 on May 7 in Pacific time.
        assert_eq!(
            [&paper["date"], &paper["time"], &paper["sourceTime"]],
            [&json!("2026-05-07"), &json!("03:59"), &json!("May 6, 23:59 AoE")]
        );
        assert_eq!(
            [&paper["kind"], &paper["group"], &paper["name"]],
            [&json!("submission"), &json!("ml"), &json!("NeurIPS")]
        );
        let conference = &events["neurips2026-conference"];
        assert_eq!(
            [&conference["date"], &conference["endDate"], &conference["place"]],
            [&json!("2026-12-06"), &json!("2026-12-12"), &json!("Sydney, Australia")]
        );
        assert!(!events.keys().any(|id| id.contains("notification")), "notifications are left out");
        let mut cvpr: Vec<&String> = events.keys().filter(|id| id.starts_with("cvpr2026-")).collect();
        cvpr.sort();
        assert_eq!(cvpr, ["cvpr2026-conference", "cvpr2026-rebuttal-0"], "CVPR 2026 deadlines fell in 2025");
        assert!(events.contains_key("iros2026-conference"));
        // The rebuttal period spans its days in local time: 23:59 AoE is 03:59 the next day in Pacific time.
        let rebuttal = &events["cvpr2026-rebuttal-0"];
        assert_eq!(
            ["kind", "label", "date", "endDate", "time"].map(|key| rebuttal[key].clone()),
            [json!("rebuttal"), json!("Rebuttal Period"), json!("2026-01-23"), json!("2026-01-30"), json!("03:59")]
        );
        assert_eq!(rebuttal["sourceTime"], json!("Jan 22, 23:59 – Jan 29, 23:59 AoE"));
        let single = &events["eccv2026-rebuttal-0"];
        assert_eq!(
            ["date", "endDate", "time", "sourceTime"].map(|key| single[key].clone()),
            [json!("2026-05-11"), Value::Null, json!("13:00"), json!("May 11, 21:00 UTC")]
        );
        let dates: Vec<&str> = data["events"].as_array().unwrap().iter().map(|e| e["date"].as_str().unwrap()).collect();
        let mut sorted = dates.clone();
        sorted.sort();
        assert_eq!(dates, sorted);

        assert_eq!(request(json!({"year": "soon"})).unwrap_err().message, "year must be an integer.");
        assert_eq!(request(json!({"year": 1900})).unwrap_err().message, "year is out of range.");
        // Validation happens before any download.
        assert_eq!(source.count(), 15);
    }

    #[test]
    fn rebuttal_periods_show_in_every_year_they_touch() {
        let cache = json!({"conferences": {"iclr": [{"year": 2027, "deadlines": [], "rebuttals": [
            {"label": "Discussion period", "start": "2026-12-28T12:00:00+00:00", "end": "2027-01-04T11:59:59+00:00", "timezone": "AoE"}]}]}});
        let cache = cache.as_object().unwrap();
        let in_2026 = calendar_events(cache, 2026, pacific()).unwrap();
        assert_eq!((&in_2026[0]["date"], &in_2026[0]["endDate"]), (&json!("2026-12-28"), &json!("2027-01-04")));
        assert_eq!(in_2026.len(), 1);
        let in_2027 = calendar_events(cache, 2027, pacific()).unwrap();
        assert_eq!(in_2027.iter().map(|e| e["id"].clone()).collect::<Vec<_>>(), [json!("iclr2027-rebuttal-0")]);
        assert!(calendar_events(cache, 2028, pacific()).unwrap().is_empty());
    }

    #[test]
    fn local_times_follow_daylight_saving_time() {
        let cache = json!({"conferences": {"neurips": normalize_editions(NEURIPS).unwrap()}});
        let cache = cache.as_object().unwrap().clone();
        let submission = |events: Vec<Value>| {
            let event = events.into_iter().find(|e| e["id"] == json!("neurips2026-submission-1")).unwrap();
            (event["date"].clone(), event["time"].clone())
        };
        // 23:59 AoE on May 6 is 04:59 in Los Angeles in May (PDT), not 03:59 (PST).
        let named = Zone::Named(chrono_tz::America::Los_Angeles);
        assert_eq!(submission(calendar_events(&cache, 2026, named).unwrap()), (json!("2026-05-07"), json!("04:59")));
        // The real default is the system zone; a fresh thread reads TZ afresh.
        let mut env = TestEnv::new();
        env.set_var("TZ", "America/Los_Angeles");
        let events = std::thread::spawn(move || calendar_events(&cache, 2026, Zone::Local).unwrap()).join().unwrap();
        assert_eq!(submission(events), (json!("2026-05-07"), json!("04:59")));
        drop(env);
    }

    #[test]
    fn fetches_at_most_weekly_and_on_request() {
        let _env = env();
        let source = FakeSource::new();
        let fetch = |url: &str| source.fetch(url);
        let start = utc(2026, 11, 4);
        let (cache, error) = load_conference_data(false, start, &fetch);
        assert_eq!((error, source.count()), (None, 15));
        assert_eq!(cache.unwrap()["errors"]["icml"], json!("404 Not Found"));
        let stored: Value = serde_json::from_str(&std::fs::read_to_string(cache_path()).unwrap()).unwrap();
        let first = stored["conferences"]["neurips"][0].as_object().unwrap();
        assert!(["year", "deadlines", "start", "end"].iter().all(|key| first.contains_key(*key)));
        assert_eq!(stored["fetchedAt"], json!("2026-11-04T00:00:00+00:00"));
        let text = std::fs::read_to_string(cache_path()).unwrap();
        assert!(
            text.starts_with(r#"{"version":2,"source":"https://github.com/huggingface/ai-deadlines","fetchedAt":"#),
            "{text}"
        );

        load_conference_data(false, start + Duration::days(6), &fetch);
        assert_eq!(source.count(), 15, "a fresh cache is reused");
        load_conference_data(true, start + Duration::days(1), &fetch);
        assert_eq!(source.count(), 30, "refresh always fetches");
        load_conference_data(false, start + Duration::days(9), &fetch);
        assert_eq!(source.count(), 45, "a week-old cache is refreshed");
    }

    #[test]
    fn cache_from_an_older_version_is_refetched() {
        let _env = env();
        let path = cache_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"version": 1, "fetchedAt": "2026-11-04T00:00:00+00:00", "conferences": {"cvpr": []}}"#,
        )
        .unwrap();
        let source = FakeSource::new();
        let fetch = |url: &str| source.fetch(url);
        let (cache, error) = load_conference_data(false, utc(2026, 11, 5), &fetch);
        assert_eq!((error, source.count()), (None, 15), "a cache saved before rebuttal dates is replaced");
        assert!(!cache.unwrap()["conferences"]["cvpr"][1]["rebuttals"].as_array().unwrap().is_empty());
    }

    #[test]
    fn offline_keeps_the_last_data_and_reports_errors() {
        let mut env = env();
        let source = FakeSource::new();
        let start = utc(2026, 11, 4);
        load_conference_data(false, start, &|url: &str| source.fetch(url));

        let offline = |_: &str| -> Result<String, String> { Err("network is unreachable".to_string()) };
        let (cache, error) = load_conference_data(false, start + Duration::days(30), &offline);
        assert!(!cache.unwrap()["conferences"]["neurips"].as_array().unwrap().is_empty(), "stale data is still shown");
        assert_eq!(error.unwrap(), "Could not fetch conference dates: network is unreachable");

        std::fs::remove_file(cache_path()).unwrap();
        let (cache, error) = load_conference_data(true, start, &offline);
        assert!(cache.is_none() && error.unwrap().contains("Could not fetch"));
        env.set_now("2026-11-04T10:00:00");
        let data = conference_calendar_with(json!({"year": 2026}).as_object().unwrap(), &offline, pacific()).unwrap();
        assert_eq!((&data["events"], &data["fetchedAt"]), (&json!([]), &Value::Null));
        assert!(data["error"].is_string());
    }
}
