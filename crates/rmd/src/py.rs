//! Small re-implementations of the Python behaviours the original code relied
//! on: truthiness, `str()`/`repr()`/`int()`, `json.dumps` layout, ISO date
//! parsing and `strptime`. Keeping them here lets the rest of the port read
//! almost line for line like the Python it replaces.

use std::sync::OnceLock;

use chrono::{
    DateTime, Datelike, FixedOffset, Local, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeZone, Timelike, Weekday,
};
use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value};

/// A JSON object with insertion order preserved (a Python `dict`).
pub type Dict = Map<String, Value>;

// =============================================================================
// Truthiness and conversions
// =============================================================================

/// Python truthiness of a JSON value.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else {
                n.as_f64().is_some_and(|f| f != 0.0)
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Truthiness of an optional value (`dict.get(...)` returning None is falsy).
pub fn opt_truthy(value: Option<&Value>) -> bool {
    value.is_some_and(truthy)
}

/// `value or default` for an optional JSON value.
pub fn or_default<'a>(value: Option<&'a Value>, default: &'a Value) -> &'a Value {
    match value {
        Some(v) if truthy(v) => v,
        _ => default,
    }
}

/// Python `str(value)`.
pub fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(n) => number_str(n),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => py_repr(value),
    }
}

/// `str(d.get(key, ""))` — missing keys become the empty string.
pub fn get_str_or_empty(dict: &Dict, key: &str) -> String {
    dict.get(key).map(py_str).unwrap_or_default()
}

fn number_str(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        i.to_string()
    } else if let Some(u) = n.as_u64() {
        u.to_string()
    } else {
        float_repr(n.as_f64().unwrap_or(0.0))
    }
}

/// Python `repr(value)` for JSON-like values.
pub fn py_repr(value: &Value) -> String {
    match value {
        Value::String(s) => repr_str(s),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map.iter().map(|(k, v)| format!("{}: {}", repr_str(k), py_repr(v))).collect();
            format!("{{{}}}", inner.join(", "))
        }
        other => py_str(other),
    }
}

/// Python `repr(str)`.
pub fn repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python `int(value)`; `None` where Python would raise.
pub fn py_int(value: &Value) -> Option<i64> {
    match value {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_u64().and_then(|u| i64::try_from(u).ok()))
            .or_else(|| n.as_f64().filter(|f| f.is_finite()).map(|f| f.trunc() as i64)),
        Value::String(s) => parse_int_str(s),
        _ => None,
    }
}

/// Python `int(str)` (base 10, surrounding whitespace, sign and `_` separators).
pub fn parse_int_str(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() || t.starts_with('_') || t.ends_with('_') || t.contains("__") {
        return None;
    }
    let (sign, digits) = match t.as_bytes()[0] {
        b'+' => (1, &t[1..]),
        b'-' => (-1, &t[1..]),
        _ => (1, t),
    };
    if digits.is_empty() || digits.starts_with('_') {
        return None;
    }
    let cleaned: String = digits.chars().filter(|c| *c != '_').collect();
    if !cleaned.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    cleaned.parse::<i64>().ok().map(|v| sign * v)
}

/// Python `float(value)`.
pub fn py_float(value: &Value) -> Option<f64> {
    match value {
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Python `repr(float)`.
pub fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf".to_string() } else { "-inf".to_string() };
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0".to_string() } else { "0.0".to_string() };
    }
    // Shortest round-trip digits from Rust, re-laid out with Python's rules.
    let sci = format!("{:e}", f.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let decpt = exp + 1;
    let sign = if f < 0.0 { "-" } else { "" };
    if decpt <= -4 || decpt > 16 {
        let mut body = digits[..1].to_string();
        if digits.len() > 1 {
            body.push('.');
            body.push_str(&digits[1..]);
        }
        let e = decpt - 1;
        let esign = if e < 0 { '-' } else { '+' };
        format!("{sign}{body}e{esign}{:02}", e.abs())
    } else if decpt <= 0 {
        format!("{sign}0.{}{}", "0".repeat((-decpt) as usize), digits)
    } else {
        let decpt = decpt as usize;
        if digits.len() <= decpt {
            format!("{sign}{}{}.0", digits, "0".repeat(decpt - digits.len()))
        } else {
            format!("{sign}{}.{}", &digits[..decpt], &digits[decpt..])
        }
    }
}

/// Python's `" ".join(text.split())`.
pub fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Python `str.casefold()` (close enough for the comparisons in this app).
pub fn casefold(text: &str) -> String {
    text.to_lowercase().replace('ß', "ss")
}

// =============================================================================
// JSON output with Python's layout
// =============================================================================

struct PyFormatter {
    pretty: bool,
    indent: usize,
    has_value: bool,
}

impl PyFormatter {
    fn new(pretty: bool) -> Self {
        Self { pretty, indent: 0, has_value: false }
    }

    fn newline_indent<W: ?Sized + std::io::Write>(&self, w: &mut W) -> std::io::Result<()> {
        w.write_all(b"\n")?;
        for _ in 0..self.indent {
            w.write_all(b"  ")?;
        }
        Ok(())
    }
}

impl serde_json::ser::Formatter for PyFormatter {
    fn write_f64<W: ?Sized + std::io::Write>(&mut self, w: &mut W, value: f64) -> std::io::Result<()> {
        let text = if value.is_nan() {
            "NaN".to_string()
        } else if value.is_infinite() {
            if value > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
        } else {
            float_repr(value)
        };
        w.write_all(text.as_bytes())
    }

    fn begin_array<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        self.indent += 1;
        self.has_value = false;
        w.write_all(b"[")
    }

    fn end_array<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        self.indent -= 1;
        if self.pretty && self.has_value {
            self.newline_indent(w)?;
        }
        w.write_all(b"]")
    }

    fn begin_array_value<W: ?Sized + std::io::Write>(&mut self, w: &mut W, first: bool) -> std::io::Result<()> {
        if !first {
            w.write_all(if self.pretty { b"," } else { b", " })?;
        }
        if self.pretty {
            self.newline_indent(w)?;
        }
        Ok(())
    }

    fn end_array_value<W: ?Sized + std::io::Write>(&mut self, _w: &mut W) -> std::io::Result<()> {
        self.has_value = true;
        Ok(())
    }

    fn begin_object<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        self.indent += 1;
        self.has_value = false;
        w.write_all(b"{")
    }

    fn end_object<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        self.indent -= 1;
        if self.pretty && self.has_value {
            self.newline_indent(w)?;
        }
        w.write_all(b"}")
    }

    fn begin_object_key<W: ?Sized + std::io::Write>(&mut self, w: &mut W, first: bool) -> std::io::Result<()> {
        if !first {
            w.write_all(if self.pretty { b"," } else { b", " })?;
        }
        if self.pretty {
            self.newline_indent(w)?;
        }
        Ok(())
    }

    fn begin_object_value<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        w.write_all(b": ")
    }

    fn end_object_value<W: ?Sized + std::io::Write>(&mut self, _w: &mut W) -> std::io::Result<()> {
        self.has_value = true;
        Ok(())
    }
}

fn dump_with(value: &impl Serialize, pretty: bool) -> String {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, PyFormatter::new(pretty));
    value.serialize(&mut ser).expect("serializing JSON values cannot fail");
    String::from_utf8(out).expect("serde_json writes UTF-8")
}

/// `json.dumps(value, ensure_ascii=False, indent=2)`.
pub fn dumps_pretty(value: &impl Serialize) -> String {
    dump_with(value, true)
}

/// `json.dumps(value, ensure_ascii=False)` (with `", "` and `": "` separators).
pub fn dumps(value: &impl Serialize) -> String {
    dump_with(value, false)
}

/// Write `json.dump(value, f, indent=2, ensure_ascii=False)` to `path`,
/// creating parent directories first.
pub fn write_json_pretty(path: &std::path::Path, value: &impl Serialize) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, dumps_pretty(value))
}

// =============================================================================
// Dates and times
// =============================================================================

/// A parsed `datetime.fromisoformat` result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PyDateTime {
    Naive(NaiveDateTime),
    Aware(DateTime<FixedOffset>),
}

impl PyDateTime {
    /// `ts.astimezone().replace(tzinfo=None) if ts.tzinfo else ts`.
    pub fn to_local_naive(self) -> NaiveDateTime {
        match self {
            PyDateTime::Naive(n) => n,
            PyDateTime::Aware(a) => a.with_timezone(&Local).naive_local(),
        }
    }

    /// Instant in UTC; naive values are interpreted as local time.
    pub fn to_utc(self) -> DateTime<chrono::Utc> {
        match self {
            PyDateTime::Naive(n) => crate::clock::local_from_naive(n).with_timezone(&chrono::Utc),
            PyDateTime::Aware(a) => a.with_timezone(&chrono::Utc),
        }
    }
}

fn iso_date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\d{4})-?(\d{2})-?(\d{2})$").unwrap())
}

/// `date.fromisoformat(text)` (calendar dates; week dates are not used here).
pub fn parse_iso_date(text: &str) -> Option<NaiveDate> {
    let caps = iso_date_re().captures(text)?;
    // Python accepts "YYYY-MM-DD" and "YYYYMMDD" but not mixed separators.
    let dashes = text.matches('-').count();
    if dashes != 0 && dashes != 2 {
        return None;
    }
    let year: i32 = caps[1].parse().ok()?;
    let month: u32 = caps[2].parse().ok()?;
    let day: u32 = caps[3].parse().ok()?;
    if year < 1 {
        return None;
    }
    NaiveDate::from_ymd_opt(year, month, day)
}

fn iso_time_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(\d{2})(?::?(\d{2})(?::?(\d{2})(?:[.,](\d+))?)?)?(Z|[+-]\d{2}(?::?\d{2}(?::?\d{2}(?:\.\d+)?)?)?)?$",
        )
        .unwrap()
    })
}

fn parse_offset(text: &str) -> Option<FixedOffset> {
    if text == "Z" {
        return FixedOffset::east_opt(0);
    }
    let sign = if text.starts_with('-') { -1 } else { 1 };
    let digits: String = text[1..].chars().filter(|c| c.is_ascii_digit()).collect();
    let hours: i32 = digits.get(0..2)?.parse().ok()?;
    let minutes: i32 = digits.get(2..4).map(|m| m.parse().unwrap_or(0)).unwrap_or(0);
    let seconds: i32 = digits.get(4..6).map(|s| s.parse().unwrap_or(0)).unwrap_or(0);
    if hours > 23 || minutes > 59 || seconds > 59 {
        return None;
    }
    FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60 + seconds))
}

/// `datetime.fromisoformat(text)` for the formats this app writes and reads.
pub fn parse_iso_datetime(text: &str) -> Option<PyDateTime> {
    let text = text.trim_end();
    if text.len() < 8 {
        return None;
    }
    // Date part: "YYYY-MM-DD" (10 chars) or "YYYYMMDD" (8 chars).
    let date_len = if text.as_bytes().get(4) == Some(&b'-') { 10 } else { 8 };
    if !text.is_char_boundary(date_len.min(text.len())) {
        return None;
    }
    let date = parse_iso_date(text.get(..date_len)?)?;
    let rest = &text[date_len..];
    if rest.is_empty() {
        return Some(PyDateTime::Naive(date.and_hms_opt(0, 0, 0)?));
    }
    // Any single separator character, then the time.
    let mut chars = rest.chars();
    chars.next()?;
    let time_text = chars.as_str();
    let caps = iso_time_re().captures(time_text)?;
    let hour: u32 = caps[1].parse().ok()?;
    let minute: u32 = caps.get(2).map_or(Some(0), |m| m.as_str().parse().ok())?;
    let second: u32 = caps.get(3).map_or(Some(0), |m| m.as_str().parse().ok())?;
    let micros: u32 = match caps.get(4) {
        Some(frac) => {
            let mut digits: String = frac.as_str().chars().take(6).collect();
            while digits.len() < 6 {
                digits.push('0');
            }
            digits.parse().ok()?
        }
        None => 0,
    };
    let time = NaiveTime::from_hms_micro_opt(hour, minute, second, micros)?;
    let naive = date.and_time(time);
    match caps.get(5) {
        Some(tz) => {
            let offset = parse_offset(tz.as_str())?;
            let aware = offset.from_local_datetime(&naive).single()?;
            Some(PyDateTime::Aware(aware))
        }
        None => Some(PyDateTime::Naive(naive)),
    }
}

/// Parse a local naive datetime (aware values are converted to local time).
pub fn parse_naive_datetime(text: &str) -> Option<NaiveDateTime> {
    parse_iso_datetime(text).map(PyDateTime::to_local_naive)
}

/// Python `datetime.isoformat()` for a naive datetime.
pub fn isoformat_naive(dt: &NaiveDateTime) -> String {
    let base = dt.format("%Y-%m-%dT%H:%M:%S").to_string();
    let micros = dt.and_utc().timestamp_subsec_micros();
    if micros == 0 { base } else { format!("{base}.{micros:06}") }
}

/// Python `datetime.isoformat()` for an aware datetime (`+HH:MM` offset).
pub fn isoformat_aware<Tz: TimeZone>(dt: &DateTime<Tz>) -> String {
    let naive = dt.naive_local();
    let offset = dt.offset().fix().local_minus_utc();
    let sign = if offset < 0 { '-' } else { '+' };
    let abs = offset.abs();
    let (h, m, s) = (abs / 3600, (abs % 3600) / 60, abs % 60);
    let tz = if s != 0 { format!("{sign}{h:02}:{m:02}:{s:02}") } else { format!("{sign}{h:02}:{m:02}") };
    format!("{}{}", isoformat_naive(&naive), tz)
}

/// `datetime.now(timezone.utc).isoformat()` using the crate clock.
pub fn utc_now_isoformat() -> String {
    isoformat_aware(&crate::clock::now_utc())
}

/// Lowercase English weekday name (`date.strftime("%A").lower()`).
pub fn weekday_name(date: NaiveDate) -> &'static str {
    weekday_str(date.weekday())
}

/// Lowercase English name for a weekday.
pub fn weekday_str(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "monday",
        Weekday::Tue => "tuesday",
        Weekday::Wed => "wednesday",
        Weekday::Thu => "thursday",
        Weekday::Fri => "friday",
        Weekday::Sat => "saturday",
        Weekday::Sun => "sunday",
    }
}

/// Capitalised English weekday name (`strftime("%A")`).
pub fn weekday_title(date: NaiveDate) -> String {
    date.format("%A").to_string()
}

// --- strptime ---------------------------------------------------------------

/// Parsed fields from [`strptime`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StrpFields {
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
    pub hour: Option<u32>,
    pub minute: Option<u32>,
    pub second: Option<u32>,
}

impl StrpFields {
    /// Date with Python's defaults (1900-01-01) for missing fields.
    pub fn date(&self) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(self.year.unwrap_or(1900), self.month.unwrap_or(1), self.day.unwrap_or(1))
    }

    pub fn time(&self) -> Option<NaiveTime> {
        NaiveTime::from_hms_opt(self.hour.unwrap_or(0), self.minute.unwrap_or(0), self.second.unwrap_or(0))
    }

    pub fn datetime(&self) -> Option<NaiveDateTime> {
        Some(self.date()?.and_time(self.time()?))
    }
}

fn strptime_regex(format: &str) -> Option<Regex> {
    let mut pattern = String::from("(?i)^");
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let directive = chars.next()?;
            let piece = match directive {
                'Y' => r"(?P<Y>\d\d\d\d)",
                'm' => r"(?P<m>1[0-2]|0[1-9]|[1-9])",
                'd' => r"(?P<d>3[01]|[12]\d|0[1-9]|[1-9]| [1-9])",
                'H' => r"(?P<H>2[0-3]|[0-1]\d|\d)",
                'M' => r"(?P<M>[0-5]\d|\d)",
                'S' => r"(?P<S>6[0-1]|[0-5]\d|\d)",
                '%' => "%",
                _ => return None,
            };
            pattern.push_str(piece);
        } else if c.is_whitespace() {
            pattern.push_str(r"\s+");
            while chars.peek().is_some_and(|n| n.is_whitespace()) {
                chars.next();
            }
        } else {
            pattern.push_str(&regex::escape(&c.to_string()));
        }
    }
    pattern.push('$');
    Regex::new(&pattern).ok()
}

/// Python `datetime.strptime(text, format)` for `%Y %m %d %H %M %S`.
pub fn strptime(text: &str, format: &str) -> Option<StrpFields> {
    let re = strptime_regex(format)?;
    let caps = re.captures(text)?;
    let num = |name: &str| caps.name(name).and_then(|m| m.as_str().trim().parse::<u32>().ok());
    let fields = StrpFields {
        year: caps.name("Y").and_then(|m| m.as_str().parse().ok()),
        month: num("m"),
        day: num("d"),
        hour: num("H"),
        minute: num("M"),
        second: num("S"),
    };
    // Validate the date the way Python does (e.g. Feb 30 raises).
    fields.date()?;
    if fields.second.is_some_and(|s| s > 59) {
        return None;
    }
    Some(fields)
}

/// `datetime.strptime(text, format).date()`.
pub fn strptime_date(text: &str, format: &str) -> Option<NaiveDate> {
    strptime(text, format)?.date()
}

/// `datetime.strptime(text, format)`.
pub fn strptime_datetime(text: &str, format: &str) -> Option<NaiveDateTime> {
    strptime(text, format)?.datetime()
}

/// `datetime.strptime(text, "%H:%M").time()`.
pub fn parse_hhmm(text: &str) -> Option<NaiveTime> {
    strptime(text, "%H:%M")?.time()
}

/// `time.strftime("%H:%M")`.
pub fn hhmm(time: NaiveTime) -> String {
    format!("{:02}:{:02}", time.hour(), time.minute())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn float_repr_matches_python() {
        assert_eq!(float_repr(1.0), "1.0");
        assert_eq!(float_repr(0.1), "0.1");
        assert_eq!(float_repr(1e16), "1e+16");
        assert_eq!(float_repr(1e15), "1000000000000000.0");
        assert_eq!(float_repr(0.0001), "0.0001");
        assert_eq!(float_repr(0.00001), "1e-05");
        assert_eq!(float_repr(-2.5), "-2.5");
        assert_eq!(float_repr(123456789.125), "123456789.125");
        assert_eq!(float_repr(1.5e300), "1.5e+300");
    }

    #[test]
    fn json_layout_matches_python() {
        let v = json!({"a": [1, 2.0, "é"], "b": {}, "c": [], "d": {"x": null}});
        assert_eq!(dumps(&v), r#"{"a": [1, 2.0, "é"], "b": {}, "c": [], "d": {"x": null}}"#);
        assert_eq!(
            dumps_pretty(&v),
            "{\n  \"a\": [\n    1,\n    2.0,\n    \"é\"\n  ],\n  \"b\": {},\n  \"c\": [],\n  \"d\": {\n    \"x\": null\n  }\n}"
        );
    }

    #[test]
    fn repr_and_str() {
        assert_eq!(py_repr(&json!("09:00")), "'09:00'");
        assert_eq!(py_repr(&json!("it's")), "\"it's\"");
        assert_eq!(py_repr(&Value::Null), "None");
        assert_eq!(py_str(&json!(true)), "True");
        assert_eq!(py_str(&json!(5)), "5");
    }

    #[test]
    fn int_parsing() {
        assert_eq!(py_int(&json!(" 7 ")), Some(7));
        assert_eq!(py_int(&json!("1_000")), Some(1000));
        assert_eq!(py_int(&json!("x")), None);
        assert_eq!(py_int(&json!(3.9)), Some(3));
        assert_eq!(py_int(&json!(true)), Some(1));
    }

    #[test]
    fn iso_parsing() {
        assert_eq!(parse_iso_date("2025-01-05"), NaiveDate::from_ymd_opt(2025, 1, 5));
        assert_eq!(parse_iso_date("2025-1-5"), None);
        let ts = parse_iso_datetime("2025-01-05T10:20:30.123456+00:00").unwrap();
        assert!(matches!(ts, PyDateTime::Aware(_)));
        let naive = parse_iso_datetime("2025-01-05 10:20").unwrap();
        assert_eq!(
            naive,
            PyDateTime::Naive(NaiveDate::from_ymd_opt(2025, 1, 5).unwrap().and_hms_opt(10, 20, 0).unwrap())
        );
        assert_eq!(
            isoformat_naive(&NaiveDate::from_ymd_opt(2025, 1, 5).unwrap().and_hms_micro_opt(1, 2, 3, 40).unwrap()),
            "2025-01-05T01:02:03.000040"
        );
    }

    #[test]
    fn strptime_is_lenient_like_python() {
        assert_eq!(strptime_date("2025-1-5", "%Y-%m-%d"), NaiveDate::from_ymd_opt(2025, 1, 5));
        assert_eq!(parse_hhmm("9:05").map(hhmm), Some("09:05".to_string()));
        assert!(parse_hhmm("24:00").is_none());
        assert!(strptime_date("2025-02-30", "%Y-%m-%d").is_none());
        assert!(strptime_date("2025-01-05x", "%Y-%m-%d").is_none());
    }
}
