//! LaunchX shared memory: the small wiki of pages about the user that their
//! apps and agents keep together (contract §12 of LaunchX's `SPEC.md`).
//!
//! Under LaunchX, `rmd serve` gets the memory API's address and a token of
//! its own (`LAUNCHX_MEMORY_URL`, `LAUNCHX_MEMORY_TOKEN`). The user decides
//! in LaunchX whether rmd may read memory, read and write it, or neither
//! (`lx memory access rmd write`). The launcher checks that switch on every
//! request, so rmd asks (`GET /me`) instead of assuming.
//!
//! - With read access, Suggest gives the model the memory's index and the
//!   `profile` and `preferences` pages ([`suggest_context`]).
//! - With write access, rmd keeps its section of the well-known `schedule`
//!   page current: the shape of the weekly routine and the upcoming deadlines
//!   ([`schedule_section`]). The section sits between two marker lines
//!   ([`merge_section`]); the rest of the page belongs to other writers and is
//!   never changed. Live data such as the task list stays out of memory: other
//!   apps read it from `/api/state`.
//!
//! Every write names the revision it read; when another writer got there
//! first (`409`), rmd puts its section into the page as it is now and tries
//! once more ([`sync_section`]). Writes follow changes to the routine and
//! deadlines, a few seconds after the last one ([`schedule_changed`]), and a
//! check every two minutes catches edits made with the CLI or by hand.
//!
//! Memory is optional. When it is off, unreachable or refuses a request, rmd
//! works as before and says so in its settings ([`status`]). The token is
//! never logged or shown.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use chrono::{Datelike, NaiveDate, NaiveTime};
use serde_json::{Value, json};

use crate::config::{ScheduleConfig, WeeklySchedule};
use crate::py::{self, Dict};
use crate::recurrence::{anchor_day, repeat_rule};
use crate::synced_schedule::{format_event_label, get_event_block_name};

pub const URL_ENV: &str = "LAUNCHX_MEMORY_URL";
pub const TOKEN_ENV: &str = "LAUNCHX_MEMORY_TOKEN";
/// The access `launchx.toml` asks for.
pub const WANTS: Access = Access::Write;
/// The well-known page that holds the shape of the user's time.
pub const SCHEDULE_PAGE: &str = "schedule";
/// The summary rmd gives the `schedule` page when it creates it, or finds it without one.
const SCHEDULE_SUMMARY: &str =
    "The shape of the user's time: weekly routine, fixed commitments, deadlines that matter beyond one app";
/// The log note of rmd's writes.
const NOTE: &str = "Schedule Everything: routine or deadlines changed";
/// What begins the first line of rmd's section; the rest of the line may change.
const BEGIN: &str = "<!-- rmd:begin";
const BEGIN_LINE: &str = "<!-- rmd:begin · Schedule Everything rewrites the lines up to rmd:end from the routine and deadlines; write notes outside them -->";
const END_LINE: &str = "<!-- rmd:end -->";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
/// The first check, shortly after the server starts.
const FIRST_SYNC: Duration = Duration::from_secs(2);
/// Quiet time after a change before writing, so a burst of edits makes one write.
const DEBOUNCE: Duration = Duration::from_secs(5);
/// The longest a write waits while changes keep coming.
const MAX_WAIT: Duration = Duration::from_secs(30);
/// How often the page is checked without a change in the workspace.
const POLL: Duration = Duration::from_secs(120);
/// How old the known access may be before the settings page asks again.
const STALE: Duration = Duration::from_secs(5);

/// How much of the index and of each page Suggest passes on.
const INDEX_LIMIT: usize = 4_000;
const PAGE_LIMIT: usize = 3_000;
/// One-off deadlines further ahead than this stay out of the page.
const DEADLINE_DAYS: i64 = 183;
const MAX_DEADLINES: usize = 20;
const LABEL_LIMIT: usize = 60;

const WEEKDAYS: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
const DAY_NAMES: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

// =============================================================================
// The API
// =============================================================================

/// What the user lets rmd do with memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Access {
    None,
    Read,
    Write,
}

impl Access {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "none" => Some(Self::None),
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

/// One page as the API returns it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    pub rev: String,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub updated_by: Option<String>,
}

impl Page {
    fn from_json(value: &Value) -> Option<Self> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        value.get("rev").and_then(Value::as_str)?;
        Some(Self {
            rev: text("rev"),
            title: text("title"),
            summary: text("summary"),
            body: text("body"),
            updated_by: value.get("updated_by").and_then(Value::as_str).map(str::to_string),
        })
    }
}

/// Why a request to memory did not succeed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// `403`: the user has not allowed this (`access` is what they allow).
    Denied {
        access: Access,
        message: String,
    },
    /// `409`: the page changed since the revision the write named; `current`
    /// is the page now, `None` when it was deleted.
    Conflict {
        current: Option<Page>,
    },
    NotFound,
    /// No answer: the launcher is not running, or the request timed out.
    Unreachable(String),
    Status {
        status: u16,
        message: String,
    },
}

impl Failure {
    /// One line for the log and the settings page.
    pub fn message(&self) -> String {
        match self {
            Self::Denied { message, .. } => message.clone(),
            Self::Conflict { .. } => "Another writer changed the page at the same time.".into(),
            Self::NotFound => "Not found.".into(),
            Self::Unreachable(error) => format!("LaunchX did not answer: {error}"),
            Self::Status { status: 401, .. } => "LaunchX did not accept this server's memory token.".into(),
            Self::Status { status, message } => format!("LaunchX answered {status}: {message}"),
        }
    }
}

/// The memory API at `LAUNCHX_MEMORY_URL`, with this service's token.
#[derive(Clone)]
pub struct Client {
    url: String,
    token: String,
    agent: ureq::Agent,
}

/// Never the token.
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("url", &self.url).finish_non_exhaustive()
    }
}

impl Client {
    pub fn new(url: &str, token: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .proxy(None)
            .timeout_global(Some(REQUEST_TIMEOUT))
            .http_status_as_error(false)
            .user_agent("schedule-everything")
            .build()
            .into();
        Self { url: url.trim().trim_end_matches('/').to_string(), token: token.trim().to_string(), agent }
    }

    /// The memory LaunchX gives a service it manages; `None` outside LaunchX,
    /// or from a launcher without memory.
    pub fn from_env() -> Option<Self> {
        std::env::var_os("LAUNCHX")?;
        let var = |name: &str| std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        Some(Self::new(&var(URL_ENV)?, &var(TOKEN_ENV)?))
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, Failure> {
        let url = format!("{}{path}", self.url);
        let authorization = format!("Bearer {}", self.token);
        let response = match body {
            None => self.agent.get(&url).header("Authorization", &authorization).call(),
            Some(body) => {
                let request = match method {
                    "PUT" => self.agent.put(&url),
                    _ => self.agent.post(&url),
                };
                request
                    .header("Authorization", &authorization)
                    .header("Content-Type", "application/json")
                    .send(body.to_string())
            }
        }
        .map_err(|e| Failure::Unreachable(e.to_string()))?;
        let status = response.status().as_u16();
        let text = response.into_body().read_to_string().map_err(|e| Failure::Unreachable(e.to_string()))?;
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let message = || value.get("error").and_then(Value::as_str).unwrap_or("no detail").to_string();
        match status {
            200..=299 => Ok(value),
            403 => Err(Failure::Denied {
                access: value.get("access").and_then(Value::as_str).and_then(Access::parse).unwrap_or(Access::None),
                message: message(),
            }),
            404 => Err(Failure::NotFound),
            409 => Err(Failure::Conflict { current: value.get("current").and_then(Page::from_json) }),
            _ => Err(Failure::Status { status, message: message() }),
        }
    }

    /// `GET /me`: what the user lets this service do, now.
    pub fn me(&self) -> Result<Access, Failure> {
        let me = self.call("GET", "/me", None)?;
        me.get("access")
            .and_then(Value::as_str)
            .and_then(Access::parse)
            .ok_or_else(|| Failure::Status { status: 200, message: "no access level in /me".into() })
    }

    /// `GET /index`: one line per page, and how many pages there are.
    pub fn index(&self) -> Result<(String, u64), Failure> {
        let index = self.call("GET", "/index", None)?;
        let markdown = index.get("markdown").and_then(Value::as_str).unwrap_or_default().to_string();
        Ok((markdown, index.get("pages").and_then(Value::as_u64).unwrap_or(0)))
    }

    /// `GET /pages/<page>`; `None` when there is no such page.
    pub fn page(&self, page: &str) -> Result<Option<Page>, Failure> {
        match self.call("GET", &format!("/pages/{page}"), None) {
            Ok(value) => Ok(Page::from_json(&value)),
            Err(Failure::NotFound) => Ok(None),
            Err(failure) => Err(failure),
        }
    }

    /// `PUT /pages/<page>`: create it (`base` `None`) or update the revision
    /// `base`. Title and summary stay as they are unless `summary` is given.
    pub fn put(&self, page: &str, body: &str, base: Option<&str>, summary: Option<&str>) -> Result<Page, Failure> {
        let mut request = json!({"body": body, "base": base, "note": NOTE});
        if let Some(summary) = summary {
            request["summary"] = json!(summary);
        }
        let written = self.call("PUT", &format!("/pages/{page}"), Some(&request))?;
        Page::from_json(&written)
            .ok_or_else(|| Failure::Status { status: 200, message: "no page in the answer".into() })
    }
}

// =============================================================================
// rmd's section of a page
// =============================================================================

/// `content` between rmd's marker lines.
pub fn section(content: &str) -> String {
    format!("{BEGIN_LINE}\n{}\n{END_LINE}", content.trim_matches('\n'))
}

fn is_begin(line: &str) -> bool {
    line.trim_start().starts_with(BEGIN)
}

fn is_end(line: &str) -> bool {
    line.trim() == END_LINE
}

/// `body` with `section` in place of rmd's section, or added at the end when
/// it has none. Every line outside rmd's markers is kept as it is; a copy of
/// the section is removed, and a begin marker without its end loses only the
/// marker line.
pub fn merge_section(body: &str, section: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut placed = false;
    let mut i = 0;
    while i < lines.len() {
        if !is_begin(lines[i]) {
            out.push(lines[i]);
            i += 1;
            continue;
        }
        match lines[i + 1..].iter().position(|line| is_end(line)) {
            Some(offset) => {
                if !placed {
                    out.push(section);
                    placed = true;
                }
                i += offset + 2;
            }
            None => i += 1,
        }
    }
    let mut text = out.join("\n").trim_end().to_string();
    if !placed {
        if !text.trim().is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(section);
    }
    text.push('\n');
    text
}

/// What [`sync_section`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Synced {
    /// The page already held the section.
    Unchanged(Page),
    Written(Page),
}

/// Put `section` into `page`: read it, merge, and write with the revision
/// read as `base`. When another writer changed it in between (`409`), merge
/// into the page as it is now and write once more.
pub fn sync_section(client: &Client, page: &str, section: &str) -> Result<Synced, Failure> {
    let current = client.page(page)?;
    match write_section(client, page, section, current.as_ref()) {
        Err(Failure::Conflict { current }) => write_section(client, page, section, current.as_ref()),
        other => other,
    }
}

fn write_section(client: &Client, page: &str, section: &str, current: Option<&Page>) -> Result<Synced, Failure> {
    let body = merge_section(current.map_or("", |page| page.body.as_str()), section);
    if let Some(current) = current
        && current.body.trim() == body.trim()
    {
        return Ok(Synced::Unchanged(current.clone()));
    }
    let summary = match current {
        Some(current) if !current.summary.trim().is_empty() => None,
        _ => Some(SCHEDULE_SUMMARY),
    };
    client.put(page, &body, current.map(|page| page.rev.as_str()), summary).map(Synced::Written)
}

// =============================================================================
// What rmd writes: the routine's shape and the upcoming deadlines
// =============================================================================

/// Text for the page: one line, no marker, at most `limit` characters.
fn clean(text: &str, limit: usize) -> String {
    let text = text.replace("<!--", "").replace("-->", "");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= limit {
        return text;
    }
    let cut: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// An event as the page names it: a block or time point by its name with
/// spaces (`long_break` → `long break`), anything else as the workspace shows it.
fn event_label(event: &Value, config: &ScheduleConfig) -> String {
    match event {
        Value::String(name)
            if config.time_blocks.contains_key(name.trim()) || config.time_points.contains_key(name.trim()) =>
        {
            name.trim().replace('_', " ")
        }
        _ => format_event_label(event),
    }
}

/// One day of the routine on one line: the span of the day, then its events
/// in order, with a run of the same block counted (`14:00–15:55 pomodoro ×4`).
fn day_shape(schedule: &Dict, config: &ScheduleConfig) -> String {
    struct Run {
        label: String,
        first: NaiveTime,
        last: NaiveTime,
        count: usize,
        minutes: Option<i64>,
    }
    let mut events: Vec<(NaiveTime, &Value)> =
        schedule.iter().filter_map(|(time, event)| Some((py::parse_hhmm(time)?, event))).collect();
    events.sort_by_key(|(time, _)| *time);
    let mut runs: Vec<Run> = Vec::new();
    for (at, event) in events {
        let label = clean(&event_label(event, config), LABEL_LIMIT);
        if label.is_empty() {
            continue;
        }
        match runs.last_mut() {
            Some(run) if run.label == label => {
                run.last = at;
                run.count += 1;
            }
            _ => {
                let minutes = get_event_block_name(event).and_then(|block| {
                    config.block_minutes(&block).or_else(|| config.block_minutes(&block.replace(' ', "_")))
                });
                runs.push(Run { label, first: at, last: at, count: 1, minutes })
            }
        }
    }
    let (Some(first), Some(last)) = (runs.first(), runs.last()) else {
        return "nothing scheduled".into();
    };
    if runs.len() == 1 && first.count == 1 {
        return format!("{} {}", py::hhmm(first.first), first.label);
    }
    let end = |run: &Run| run.minutes.map_or(run.last, |m| run.last + chrono::Duration::minutes(m));
    let parts: Vec<String> = runs
        .iter()
        .map(|run| match run.count {
            1 => format!("{} {}", py::hhmm(run.first), run.label),
            n => format!("{}–{} {} ×{n}", py::hhmm(run.first), py::hhmm(end(run)), run.label),
        })
        .collect();
    format!("{}–{} · {}", py::hhmm(first.first), py::hhmm(end(last)), parts.join(", "))
}

/// `Monday–Friday, Sunday`: weekdays by index, three or more in a row as a range.
fn days_label(days: &[usize]) -> String {
    if days.len() == 7 {
        return "Every day".into();
    }
    let mut parts = Vec::new();
    let mut i = 0;
    while i < days.len() {
        let mut j = i;
        while j + 1 < days.len() && days[j + 1] == days[j] + 1 {
            j += 1;
        }
        if j - i >= 2 {
            parts.push(format!("{}–{}", DAY_NAMES[days[i]], DAY_NAMES[days[j]]));
        } else {
            parts.extend(days[i..=j].iter().map(|&day| DAY_NAMES[day].to_string()));
        }
        i = j + 1;
    }
    parts.join(", ")
}

/// A week of the routine: days with the same shape share a line.
fn week_lines(weekly: &WeeklySchedule, parity: &str, config: &ScheduleConfig) -> Vec<String> {
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (index, day) in WEEKDAYS.iter().enumerate() {
        let shape = if config.skips_weekday(day) {
            "no reminders (a skip day)".to_string()
        } else {
            day_shape(&weekly.schedule_for_day(parity, day), config)
        };
        match groups.iter_mut().find(|(known, _)| *known == shape) {
            Some((_, days)) => days.push(index),
            None => groups.push((shape, vec![index])),
        }
    }
    groups.into_iter().map(|(shape, days)| format!("- {}: {shape}", days_label(&days))).collect()
}

/// Upcoming deadlines, soonest first: one-offs in the next six months, and
/// recurring ones by their rule.
fn deadline_lines(deadlines: &[Value], task_types: &Dict, today: NaiveDate) -> Vec<String> {
    let horizon = today + chrono::Duration::days(DEADLINE_DAYS);
    let mut rows: Vec<(NaiveDate, String)> = Vec::new();
    for item in deadlines.iter().filter_map(Value::as_object) {
        let event = clean(&py::get_str_or_empty(item, "event"), LABEL_LIMIT);
        let date = py::strptime_date(py::get_str_or_empty(item, "deadline").trim(), "%Y-%m-%d");
        let (false, Some(date)) = (event.is_empty(), date) else { continue };
        let rule = repeat_rule(item);
        if date < today || (rule.is_none() && date > horizon) {
            continue;
        }
        let weekday = DAY_NAMES[date.weekday().num_days_from_monday() as usize];
        let when = match rule {
            Some("weekly") => format!("Every {weekday}"),
            Some("biweekly") => format!("Every other {weekday}, next {date}"),
            Some("monthly") => format!("Monthly on day {}", anchor_day(item, date)),
            _ => format!("{date} ({})", &weekday[..3]),
        };
        let kind = item
            .get("type")
            .map(py::py_str)
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .map(|id| task_types.get(&id).map(py::py_str).unwrap_or(id))
            .map(|name| format!(" · {}", clean(&name, LABEL_LIMIT)))
            .unwrap_or_default();
        rows.push((date, format!("- {when}: {event}{kind}")));
    }
    rows.sort();
    rows.into_iter().take(MAX_DEADLINES).map(|(_, line)| line).collect()
}

/// rmd's part of the `schedule` page (without markers): the routine week by
/// week, and the upcoming deadlines. Absolute dates only, so it changes when
/// the routine or deadlines do, not every day.
pub fn render_schedule(
    config: &ScheduleConfig,
    weekly: &WeeklySchedule,
    deadlines: &[Value],
    task_types: &Dict,
    today: NaiveDate,
) -> String {
    let mut text = String::from("## From Schedule Everything\n\n");
    let odd = week_lines(weekly, "odd", config);
    let even = week_lines(weekly, "even", config);
    if odd == even {
        text.push_str("Weekly routine (reminder blocks, local time):\n");
        text.push_str(&odd.join("\n"));
    } else {
        text.push_str("Weekly routine in odd ISO weeks (reminder blocks, local time):\n");
        text.push_str(&odd.join("\n"));
        text.push_str("\n\nIn even ISO weeks:\n");
        text.push_str(&even.join("\n"));
    }
    text.push_str("\n\nUpcoming deadlines:\n");
    let deadlines = deadline_lines(deadlines, task_types, today);
    if deadlines.is_empty() {
        text.push_str("- None.");
    } else {
        text.push_str(&deadlines.join("\n"));
    }
    text
}

/// rmd's section of the `schedule` page from the active configuration, with
/// its markers; `None` while the schedule configuration does not load.
pub fn schedule_section() -> Option<String> {
    let paths = crate::web::services::snapshot_runtime_paths();
    let config = ScheduleConfig::load(&paths.settings_path).ok()?;
    let weekly = WeeklySchedule::load(&paths.odd_path, &paths.even_path).ok()?;
    let today = crate::clock::today();
    // Read without saving: recurring deadlines roll forward in memory only.
    let deadlines = std::fs::read_to_string(&paths.ddl_path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let deadlines = crate::commands::deadlines::refresh_deadlines(&deadlines, Some(today)).kept;
    let task_types = crate::web::services::load_task_types();
    Some(section(&render_schedule(&config, &weekly, &deadlines, &task_types, today)))
}

// =============================================================================
// What Suggest reads
// =============================================================================

fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let cut: String = text.chars().take(limit).collect();
    format!("{}\n…", cut.trim_end())
}

/// The memory's index and the `profile` and `preferences` pages, short
/// enough for a prompt; `None` when memory holds nothing yet.
pub fn read_context(client: &Client) -> Result<Option<String>, Failure> {
    let (index, pages) = client.index()?;
    if pages == 0 {
        return Ok(None);
    }
    let mut text = format!("<index>\n{}\n</index>", clip(index.trim(), INDEX_LIMIT));
    for name in ["profile", "preferences"] {
        if let Some(page) = client.page(name)?.filter(|page| !page.body.trim().is_empty()) {
            text.push_str(&format!("\n<page name=\"{name}\">\n{}\n</page>", clip(page.body.trim(), PAGE_LIMIT)));
        }
    }
    Ok(Some(text))
}

// =============================================================================
// The server's memory
// =============================================================================

/// What the settings page shows.
#[derive(Clone, Debug, Default)]
struct Status {
    access: Option<Access>,
    reachable: Option<bool>,
    checked: Option<Instant>,
    error: Option<String>,
    synced_at: Option<String>,
    rev: Option<String>,
    sync_error: Option<String>,
}

enum Wake {
    /// Ask LaunchX for the access now.
    Check,
    /// The routine or deadlines changed.
    Changed,
}

struct Memory {
    id: String,
    client: Client,
    status: Mutex<Status>,
    wake: Mutex<Sender<Wake>>,
    check_pending: AtomicBool,
}

static MEMORY: OnceLock<Memory> = OnceLock::new();

/// Use `client` as service `id` for the rest of the process: Suggest reads
/// from it, and a background thread keeps the `schedule` page current.
pub fn start(id: &str, client: Client) {
    let (sender, receiver) = mpsc::channel();
    let memory = Memory {
        id: id.to_string(),
        client,
        status: Mutex::new(Status::default()),
        wake: Mutex::new(sender),
        check_pending: AtomicBool::new(false),
    };
    if MEMORY.set(memory).is_ok()
        && let Err(e) = std::thread::Builder::new().name("rmd-memory".into()).spawn(move || run(receiver))
    {
        eprintln!("LaunchX memory: could not start its thread: {e}");
    }
}

/// The routine or deadlines may have changed: update the `schedule` page
/// once things are quiet. Does nothing outside LaunchX.
pub fn schedule_changed() {
    if let Some(memory) = MEMORY.get() {
        memory.send(Wake::Changed);
    }
}

/// What Suggest gives the model from memory, when rmd may read it.
pub fn suggest_context() -> Option<String> {
    let memory = MEMORY.get()?;
    match read_context(&memory.client) {
        Ok(context) => {
            memory.answered(None);
            context
        }
        Err(failure) => {
            memory.failed(&failure);
            None
        }
    }
}

/// Memory as the state document and the settings page show it.
pub fn status() -> Value {
    status_of(MEMORY.get())
}

/// [`status`] of `memory`; asks LaunchX again in the background when what
/// is known is more than a few seconds old.
fn status_of(memory: Option<&Memory>) -> Value {
    let command = |id: &str| format!("lx memory access {id} {}", WANTS.as_str());
    let Some(memory) = memory else {
        return json!({
            "available": false,
            "access": null,
            "wants": WANTS.as_str(),
            "reachable": null,
            "error": null,
            "change": command(crate::launchx::SERVICE_ID),
            "schedule": null,
        });
    };
    let status = memory.status().clone();
    if status.checked.is_none_or(|checked| checked.elapsed() > STALE)
        && !memory.check_pending.swap(true, Ordering::SeqCst)
    {
        memory.send(Wake::Check);
    }
    json!({
        "available": true,
        "access": status.access.map(Access::as_str),
        "wants": WANTS.as_str(),
        "reachable": status.reachable,
        "error": status.error,
        "change": command(&memory.id),
        "schedule": {
            "page": SCHEDULE_PAGE,
            "rev": status.rev,
            "syncedAt": status.synced_at,
            "error": status.sync_error,
        },
    })
}

impl Memory {
    fn status(&self) -> MutexGuard<'_, Status> {
        self.status.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn send(&self, wake: Wake) {
        let _ = self.wake.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).send(wake);
    }

    /// LaunchX answered; `access` when the answer said what it is.
    fn answered(&self, access: Option<Access>) {
        let mut status = self.status();
        if status.reachable != Some(true) {
            println!("LaunchX memory: connected at {}", self.client.url());
        }
        if let Some(access) = access
            && status.access != Some(access)
        {
            println!("LaunchX memory: Schedule Everything may {}", access_words(access));
            status.access = Some(access);
        }
        status.reachable = Some(true);
        status.error = None;
    }

    /// A request failed. A refusal tells the access; anything else is logged
    /// once, until it changes.
    fn failed(&self, failure: &Failure) {
        let mut status = self.status();
        if let Failure::Denied { access, .. } = failure {
            if status.access != Some(*access) {
                println!("LaunchX memory: Schedule Everything may {}", access_words(*access));
            }
            status.access = Some(*access);
            status.reachable = Some(true);
            status.error = None;
            return;
        }
        let message = failure.message();
        if status.error.as_deref() != Some(message.as_str()) {
            eprintln!("LaunchX memory: {message}");
        }
        let reachable = !matches!(failure, Failure::Unreachable(_));
        if !reachable {
            status.access = None;
        }
        status.reachable = Some(reachable);
        status.error = Some(message);
    }

    /// Ask LaunchX what rmd may do; the access, or `None` without an answer.
    fn check(&self) -> Option<Access> {
        self.check_pending.store(false, Ordering::SeqCst);
        let result = self.client.me();
        self.status().checked = Some(Instant::now());
        match result {
            Ok(access) => {
                self.answered(Some(access));
                Some(access)
            }
            Err(failure) => {
                self.failed(&failure);
                None
            }
        }
    }

    /// Bring rmd's section of the `schedule` page up to date, if rmd may write.
    fn sync(&self) {
        if self.check() != Some(Access::Write) {
            return;
        }
        let Some(section) = schedule_section() else { return };
        let result = sync_section(&self.client, SCHEDULE_PAGE, &section);
        let mut status = self.status();
        match result {
            Ok(synced) => {
                let page = match synced {
                    Synced::Written(page) => {
                        println!("LaunchX memory: updated the {SCHEDULE_PAGE} page (rev {})", page.rev);
                        page
                    }
                    Synced::Unchanged(page) => page,
                };
                status.rev = Some(page.rev);
                status.synced_at = Some(crate::clock::now().format("%Y-%m-%dT%H:%M:%S").to_string());
                status.sync_error = None;
            }
            // Refused or unanswered: what the connection shows.
            Err(failure @ (Failure::Denied { .. } | Failure::Unreachable(_))) => {
                drop(status);
                self.failed(&failure);
            }
            // The write itself failed (a page too large, say): logged once,
            // shown in Settings, and tried again at the next change or check.
            Err(failure) => {
                let message = failure.message();
                if status.sync_error.as_deref() != Some(message.as_str()) {
                    eprintln!("LaunchX memory: could not update the {SCHEDULE_PAGE} page: {message}");
                }
                status.sync_error = Some(message);
            }
        }
    }
}

fn access_words(access: Access) -> &'static str {
    match access {
        Access::None => "not use the shared memory",
        Access::Read => "read the shared memory",
        Access::Write => "read and write the shared memory",
    }
}

/// The background thread: sync shortly after start, a few seconds after the
/// last change, when rmd is first allowed to write, and every two minutes.
fn run(wake: Receiver<Wake>) {
    let Some(memory) = MEMORY.get() else { return };
    let mut due = Some(Instant::now() + FIRST_SYNC);
    let mut first_change: Option<Instant> = None;
    loop {
        let timeout = due.map_or(POLL, |due| due.saturating_duration_since(Instant::now()));
        match wake.recv_timeout(timeout) {
            Ok(Wake::Check) => {
                let before = memory.status().access;
                if memory.check() == Some(Access::Write) && before != Some(Access::Write) {
                    due = Some(Instant::now());
                }
            }
            Ok(Wake::Changed) => {
                let now = Instant::now();
                let first = *first_change.get_or_insert(now);
                due = Some((now + DEBOUNCE).min(first + MAX_WAIT));
            }
            Err(RecvTimeoutError::Timeout) => {
                due = None;
                first_change = None;
                memory.sync();
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    // -------------------------------------------------------------------------
    // A memory API to talk to: the parts of LaunchX's that rmd uses.
    // -------------------------------------------------------------------------

    #[derive(Default)]
    struct Fake {
        access: Option<&'static str>,
        pages: BTreeMap<String, (String, String)>,
        revisions: u32,
        /// Text another writer adds to a page between rmd's read and its write.
        sneak_in: Option<(String, String)>,
        /// An answer to every write instead of writing.
        refuse_writes: Option<(u16, &'static str)>,
        requests: Vec<String>,
    }

    impl Fake {
        fn page_json(&self, name: &str) -> Value {
            let (rev, body) = &self.pages[name];
            json!({"page": name, "rev": rev, "title": "Schedule", "summary": "", "body": body, "updated_by": "someone"})
        }

        fn set(&mut self, name: &str, body: &str) {
            self.revisions += 1;
            self.pages.insert(name.into(), (format!("rev{}", self.revisions), body.into()));
        }

        fn answer(&mut self, method: &str, path: &str, authorization: &str, body: &str) -> (u16, Value) {
            self.requests.push(format!("{method} {path}"));
            if authorization != "Bearer memory-token" {
                return (401, json!({"error": "Unauthorized"}));
            }
            let access = self.access.unwrap_or("none");
            let level = Access::parse(access).unwrap();
            let refuse = || (403, json!({"error": "rmd may not use the user's memory", "access": access}));
            match (method, path) {
                ("GET", "/me") => (200, json!({"role": "app", "id": "rmd", "access": access})),
                ("GET", "/index") if level >= Access::Read => {
                    let lines: Vec<String> = self.pages.keys().map(|name| format!("- `{name}`")).collect();
                    (200, json!({"markdown": format!("# Memory\n{}", lines.join("\n")), "pages": self.pages.len()}))
                }
                ("GET", page) if page.starts_with("/pages/") => {
                    if level < Access::Read {
                        return refuse();
                    }
                    let name = &page["/pages/".len()..];
                    if !self.pages.contains_key(name) {
                        return (404, json!({"error": "No page"}));
                    }
                    let page = self.page_json(name);
                    if let Some((target, extra)) = self.sneak_in.take().filter(|(target, _)| target == name) {
                        let body = format!("{}\n{extra}\n", self.pages[&target].1.trim_end());
                        self.set(&target, &body);
                    }
                    (200, page)
                }
                ("PUT", page) if page.starts_with("/pages/") => {
                    if level < Access::Write {
                        return refuse();
                    }
                    if let Some((status, error)) = self.refuse_writes {
                        return (status, json!({"error": error}));
                    }
                    let name = &page["/pages/".len()..];
                    let request: Value = serde_json::from_str(body).unwrap();
                    let base = request["base"].as_str();
                    match (base, self.pages.get(name)) {
                        (None, Some(_)) => return (409, json!({"error": "exists", "current": self.page_json(name)})),
                        (Some(_), None) => return (409, json!({"error": "gone", "current": null})),
                        (Some(base), Some((rev, _))) if base != rev => {
                            return (409, json!({"error": "changed", "current": self.page_json(name)}));
                        }
                        _ => {}
                    }
                    assert!(request["note"].as_str().is_some_and(|note| !note.is_empty()));
                    self.set(name, request["body"].as_str().unwrap());
                    (200, self.page_json(name))
                }
                ("GET", _) => refuse(),
                _ => (404, json!({"error": "Not found"})),
            }
        }
    }

    /// Serve `fake` on a local port until the test ends; the client for it.
    fn serve(fake: Fake) -> (Client, Arc<Mutex<Fake>>) {
        let fake = Arc::new(Mutex::new(fake));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/api/memory", listener.local_addr().unwrap().port());
        let shared = fake.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let mut reader = BufReader::new(stream);
                let mut head = String::new();
                let (mut length, mut authorization) = (0, String::new());
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        if name.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse().unwrap();
                        } else if name.eq_ignore_ascii_case("authorization") {
                            authorization = value.trim().to_string();
                        }
                    }
                    if head.is_empty() {
                        head = line;
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let mut parts = head.split_whitespace();
                let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                let path = path.strip_prefix("/api/memory").unwrap_or(path).to_string();
                let (status, answer) =
                    shared.lock().unwrap().answer(method, &path, &authorization, &String::from_utf8_lossy(&body));
                let answer = answer.to_string();
                let mut stream = reader.into_inner();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
            }
        });
        (Client::new(&url, "memory-token"), fake)
    }

    fn with_access(access: &'static str) -> Fake {
        Fake { access: Some(access), ..Fake::default() }
    }

    // -------------------------------------------------------------------------
    // The client
    // -------------------------------------------------------------------------

    #[test]
    fn the_client_reads_its_access_and_never_shows_its_token() {
        let (client, _) = serve(with_access("read"));
        assert_eq!(client.me(), Ok(Access::Read));
        assert!(!format!("{client:?}").contains("memory-token"));
        let (wrong, _) = serve(with_access("read"));
        let wrong = Client::new(wrong.url(), "other");
        let failure = wrong.me().unwrap_err();
        assert_eq!(failure.message(), "LaunchX did not accept this server's memory token.");
        assert!(!failure.message().contains("other"));
    }

    #[test]
    fn a_refusal_says_what_the_user_allows() {
        let (client, _) = serve(with_access("none"));
        assert_eq!(client.me(), Ok(Access::None));
        assert!(matches!(client.index(), Err(Failure::Denied { access: Access::None, .. })));
        assert!(matches!(client.page("profile"), Err(Failure::Denied { access: Access::None, .. })));
        let (client, _) = serve(with_access("read"));
        assert_eq!(client.page("profile"), Ok(None));
        assert!(matches!(client.put("schedule", "x", None, None), Err(Failure::Denied { access: Access::Read, .. })));
    }

    #[test]
    fn an_absent_launcher_is_unreachable_not_an_error() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let client = Client::new(&format!("http://127.0.0.1:{port}/api/memory"), "memory-token");
        assert!(matches!(client.me(), Err(Failure::Unreachable(_))));
        assert!(matches!(read_context(&client), Err(Failure::Unreachable(_))));
    }

    #[test]
    fn managed_mode_alone_brings_a_memory() {
        let mut env = crate::test_support::TestEnv::new();
        for name in ["LAUNCHX", URL_ENV, TOKEN_ENV] {
            env.remove_var(name);
        }
        env.set_var(URL_ENV, "http://127.0.0.1:7600/api/memory/");
        env.set_var(TOKEN_ENV, "secret");
        assert!(Client::from_env().is_none());
        env.set_var("LAUNCHX", "0.1");
        let client = Client::from_env().unwrap();
        assert_eq!(client.url(), "http://127.0.0.1:7600/api/memory");
        env.set_var(TOKEN_ENV, " ");
        assert!(Client::from_env().is_none());
    }

    // -------------------------------------------------------------------------
    // Merging rmd's section into a page
    // -------------------------------------------------------------------------

    #[test]
    fn the_section_goes_at_the_end_of_a_page_without_one() {
        let ours = section("## From Schedule Everything\n\n- Monday: 09:00 lecture");
        assert_eq!(merge_section("", &ours), format!("{ours}\n"));
        assert_eq!(
            merge_section("- 1:1 with [[people/ann]] on Tuesdays\n\n", &ours),
            format!("- 1:1 with [[people/ann]] on Tuesdays\n\n{ours}\n")
        );
    }

    #[test]
    fn only_the_lines_between_the_markers_change() {
        let old = section("- Monday: old");
        let new = section("- Monday: new");
        let body = format!("Intro by someone else.\n\n{old}\n\n- Added below by an agent.\n");
        let merged = merge_section(&body, &new);
        assert_eq!(merged, format!("Intro by someone else.\n\n{new}\n\n- Added below by an agent.\n"));
        // Merging again changes nothing.
        assert_eq!(merge_section(&merged, &new), merged);
        // A begin line whose wording changed still counts as rmd's.
        let reworded = format!("Intro.\n<!-- rmd:begin (older wording) -->\n- Monday: old\n{END_LINE}\nAfter.");
        assert_eq!(merge_section(&reworded, &new), format!("Intro.\n{new}\nAfter.\n"));
    }

    #[test]
    fn copies_go_and_a_lone_marker_keeps_its_text() {
        let new = section("- new");
        let doubled = format!("{}\nmiddle\n{}\n", section("- a"), section("- b"));
        assert_eq!(merge_section(&doubled, &new), format!("{new}\nmiddle\n"));
        // A begin marker without its end (an editor cut it off): only the marker line goes.
        let broken = format!("Notes.\n{BEGIN_LINE}\n- the user's own line\n");
        assert_eq!(merge_section(&broken, &new), format!("Notes.\n- the user's own line\n\n{new}\n"));
        // An end marker on its own is someone else's text.
        let stray = format!("{END_LINE}\nNotes.");
        assert_eq!(merge_section(&stray, &new), format!("{END_LINE}\nNotes.\n\n{new}\n"));
    }

    // -------------------------------------------------------------------------
    // Writing without losing writes
    // -------------------------------------------------------------------------

    #[test]
    fn rmd_creates_the_page_with_a_summary_and_then_leaves_it_alone() {
        let (client, fake) = serve(with_access("write"));
        let ours = section("- Monday: 09:00 lecture");
        let Ok(Synced::Written(page)) = sync_section(&client, "schedule", &ours) else { panic!() };
        assert_eq!(page.body, format!("{ours}\n"));
        assert_eq!(sync_section(&client, "schedule", &ours), Ok(Synced::Unchanged(page)));
        let requests = fake.lock().unwrap().requests.clone();
        assert_eq!(requests, ["GET /pages/schedule", "PUT /pages/schedule", "GET /pages/schedule"]);
    }

    #[test]
    fn a_conflict_is_merged_into_the_current_page_and_retried_once() {
        let mut fake = with_access("write");
        fake.set("schedule", &format!("Mornings: writing.\n\n{}\n", section("- Monday: old")));
        // Between rmd's read and its write, another app adds a line.
        fake.sneak_in = Some(("schedule".into(), "- Tuesday 11:00: 1:1 with Ann".into()));
        let (client, fake) = serve(fake);
        let new = section("- Monday: new");
        let Ok(Synced::Written(page)) = sync_section(&client, "schedule", &new) else { panic!() };
        assert_eq!(page.body, format!("Mornings: writing.\n\n{new}\n- Tuesday 11:00: 1:1 with Ann\n"));
        let fake = fake.lock().unwrap();
        assert_eq!(fake.requests, ["GET /pages/schedule", "PUT /pages/schedule", "PUT /pages/schedule"]);
    }

    #[test]
    fn a_page_deleted_meanwhile_is_created_again() {
        let mut fake = with_access("write");
        fake.set("schedule", "old");
        let (client, fake) = serve(fake);
        let current = client.page("schedule").unwrap();
        fake.lock().unwrap().pages.clear();
        let ours = section("- new");
        let Err(Failure::Conflict { current: None }) = write_section(&client, "schedule", &ours, current.as_ref())
        else {
            panic!()
        };
        assert!(matches!(write_section(&client, "schedule", &ours, None), Ok(Synced::Written(_))));
    }

    #[test]
    fn reading_is_not_writing() {
        let (client, fake) = serve(with_access("read"));
        let failure = sync_section(&client, "schedule", &section("- x")).unwrap_err();
        assert!(matches!(failure, Failure::Denied { access: Access::Read, .. }));
        assert!(fake.lock().unwrap().pages.is_empty());
    }

    // -------------------------------------------------------------------------
    // What Suggest reads
    // -------------------------------------------------------------------------

    #[test]
    fn suggest_reads_the_index_profile_and_preferences_kept_short() {
        let mut fake = with_access("read");
        fake.set("profile", "PhD student in robotics. Time zone: America/Los_Angeles.");
        fake.set("preferences", &"Mornings are for writing. ".repeat(400));
        fake.set("resume", "Not for Suggest.");
        let (client, fake) = serve(fake);
        let context = read_context(&client).unwrap().unwrap();
        assert!(context.starts_with("<index>\n# Memory\n- `preferences`\n- `profile`\n- `resume`\n</index>"));
        assert!(context.contains("<page name=\"profile\">\nPhD student in robotics."));
        assert!(!context.contains("Not for Suggest."));
        let preferences = context.split("<page name=\"preferences\">\n").nth(1).unwrap();
        assert!(preferences.chars().count() < PAGE_LIMIT + 20, "{}", preferences.len());
        assert!(preferences.ends_with("\n…\n</page>"));
        assert_eq!(fake.lock().unwrap().requests, ["GET /index", "GET /pages/profile", "GET /pages/preferences"]);
        // Nothing in memory yet, or no access: nothing to add.
        let (empty, _) = serve(with_access("write"));
        assert_eq!(read_context(&empty), Ok(None));
        let (off, _) = serve(with_access("none"));
        assert!(matches!(read_context(&off), Err(Failure::Denied { .. })));
    }

    // -------------------------------------------------------------------------
    // What rmd writes
    // -------------------------------------------------------------------------

    /// Days of the week by name, for tests that build a schedule.
    fn week(days: &[(&str, &[(&str, &str)])]) -> Dict {
        let mut out = Dict::new();
        for (day, events) in days {
            let mut table = Dict::new();
            for (time, event) in *events {
                table.insert(time.to_string(), json!(event));
            }
            out.insert(day.to_string(), Value::Object(table));
        }
        out
    }

    fn config(text: &str) -> ScheduleConfig {
        ScheduleConfig::from_dict(&crate::config::parse_toml_str(text).unwrap())
    }

    #[test]
    fn days_are_named_with_ranges() {
        assert_eq!(days_label(&[0, 1, 2, 3, 4]), "Monday–Friday");
        assert_eq!(days_label(&[0, 1, 3, 5, 6]), "Monday, Tuesday, Thursday, Saturday, Sunday");
        assert_eq!(days_label(&[0, 2, 3, 4, 6]), "Monday, Wednesday–Friday, Sunday");
        assert_eq!(days_label(&[0, 1, 2, 3, 4, 5, 6]), "Every day");
    }

    #[test]
    fn the_routine_is_drawn_by_its_shape() {
        let config = config(
            "[settings]\nskip_days = [\"sunday\"]\n[time_blocks]\npomodoro = 25\nlong_break = 40\nnapping = 30\n",
        );
        let day: &[(&str, &str)] = &[
            ("08:30", "pomodoro"),
            ("09:00", "pomodoro"),
            ("09:26", "long_break"),
            ("13:20", "napping"),
            ("14:00", "pomodoro"),
            ("14:30", "pomodoro"),
        ];
        let saturday: &[(&str, &str)] = &[("10:00", "Long run <!-- not a marker -->")];
        let odd = week(&[
            ("monday", day),
            ("tuesday", day),
            ("wednesday", day),
            ("thursday", day),
            ("friday", day),
            ("saturday", saturday),
        ]);
        let weekly = WeeklySchedule { odd_data: odd.clone(), even_data: odd };
        let mut task_types = Dict::new();
        task_types.insert("1".into(), json!("thesis"));
        let today = NaiveDate::from_ymd_opt(2026, 11, 17).unwrap();
        let deadlines = vec![
            json!({"event": "Thesis draft to advisor", "deadline": "2026-12-01", "type": "1"}),
            json!({"event": "Passed already", "deadline": "2026-11-16"}),
            json!({"event": "Far away", "deadline": "2027-08-01"}),
            json!({"event": "Group meeting notes", "deadline": "2026-11-20", "repeat": "weekly"}),
            json!({"event": "Rent", "deadline": "2026-12-05", "repeat": "monthly", "repeat_day": 5}),
            json!({"event": "Seminar", "deadline": "2026-11-26", "repeat": "biweekly"}),
            json!({"event": "", "deadline": "2026-11-30"}),
        ];
        assert_eq!(
            render_schedule(&config, &weekly, &deadlines, &task_types, today),
            "## From Schedule Everything

Weekly routine (reminder blocks, local time):
- Monday–Friday: 08:30–14:55 · 08:30–09:25 pomodoro ×2, 09:26 long break, 13:20 napping, 14:00–14:55 pomodoro ×2
- Saturday: 10:00 Long run not a marker
- Sunday: no reminders (a skip day)

Upcoming deadlines:
- Every Friday: Group meeting notes
- Every other Thursday, next 2026-11-26: Seminar
- 2026-12-01 (Tue): Thesis draft to advisor · thesis
- Monthly on day 5: Rent"
        );
    }

    #[test]
    fn odd_and_even_weeks_are_told_apart_when_they_differ() {
        let config = config("[time_blocks]\nmeeting = 50\n");
        let weekly = WeeklySchedule {
            odd_data: week(&[("tuesday", &[("11:00", "meeting")])]),
            even_data: week(&[("tuesday", &[("15:00", "meeting")])]),
        };
        let text = render_schedule(&config, &weekly, &[], &Dict::new(), NaiveDate::from_ymd_opt(2026, 11, 17).unwrap());
        assert_eq!(
            text,
            "## From Schedule Everything

Weekly routine in odd ISO weeks (reminder blocks, local time):
- Monday, Wednesday–Sunday: nothing scheduled
- Tuesday: 11:00 meeting

In even ISO weeks:
- Monday, Wednesday–Sunday: nothing scheduled
- Tuesday: 15:00 meeting

Upcoming deadlines:
- None."
        );
    }

    #[test]
    fn the_section_comes_from_the_active_configuration() {
        let mut env = crate::test_support::TestEnv::with_fixture();
        env.set_now("2026-11-17T10:00:00");
        env.write(
            "user_config_0/ddl.json",
            r#"[{"event": "Paper submission", "deadline": "2026-12-01", "type": "3"}]"#,
        );
        let ours = schedule_section().unwrap();
        assert!(ours.starts_with(BEGIN_LINE), "{ours}");
        assert!(ours.ends_with(END_LINE));
        assert!(ours.contains("- Sunday: no reminders (a skip day)"), "{ours}");
        assert!(ours.contains(", 21:00 summary time, 22:45 go to bed\n"), "{ours}");
        assert!(ours.contains("- 2026-12-01 (Tue): Paper submission · coding"), "{ours}");
        // Reading rolls nothing forward on disk.
        env.write(
            "user_config_0/ddl.json",
            r#"[{"event": "Weekly report", "deadline": "2026-11-06", "repeat": "weekly"}]"#,
        );
        assert!(schedule_section().unwrap().contains("- Every Friday: Weekly report"));
        assert!(env.read("user_config_0/ddl.json").contains("2026-11-06"));
        // A configuration that does not load writes nothing.
        env.write("user_config_0/settings.toml", "[settings\nbroken");
        assert_eq!(schedule_section(), None);
    }

    fn memory(client: Client) -> Memory {
        Memory {
            id: "rmd-dev".into(),
            client,
            status: Mutex::new(Status::default()),
            wake: Mutex::new(mpsc::channel().0),
            check_pending: AtomicBool::new(false),
        }
    }

    #[test]
    fn the_server_keeps_its_section_current_while_it_may_write() {
        let mut env = crate::test_support::TestEnv::with_fixture();
        env.set_now("2026-11-17T10:00:00");
        env.write("user_config_0/ddl.json", r#"[{"event": "Paper submission", "deadline": "2026-12-01"}]"#);
        let mut fake = with_access("read");
        fake.set("schedule", "- Tuesdays 11:00: 1:1 with [[people/ann]]\n");
        let (client, fake) = serve(fake);
        let memory = memory(client);

        // Read access: nothing is written, and the status says what to run.
        memory.sync();
        let status = status_of(Some(&memory));
        assert_eq!((&status["available"], &status["access"]), (&json!(true), &json!("read")));
        assert_eq!(status["change"], json!("lx memory access rmd-dev write"));
        assert_eq!(status["schedule"]["syncedAt"], Value::Null);
        assert_eq!(fake.lock().unwrap().requests, ["GET /me"]);

        // Write access: rmd's section joins the page, below the other writer's line.
        fake.lock().unwrap().access = Some("write");
        memory.sync();
        let body = fake.lock().unwrap().pages["schedule"].1.clone();
        assert!(body.starts_with("- Tuesdays 11:00: 1:1 with [[people/ann]]\n\n<!-- rmd:begin"), "{body}");
        assert!(body.contains("- 2026-12-01 (Tue): Paper submission"), "{body}");
        let status = status_of(Some(&memory));
        assert_eq!(status["access"], json!("write"));
        assert_eq!(status["schedule"]["syncedAt"], json!("2026-11-17T10:00:00"));
        assert_eq!(status["schedule"]["rev"], json!("rev2"));

        // Unchanged: read, compare, no write.
        fake.lock().unwrap().requests.clear();
        memory.sync();
        assert_eq!(fake.lock().unwrap().requests, ["GET /me", "GET /pages/schedule"]);

        // A deadline moves: only rmd's lines change.
        env.write("user_config_0/ddl.json", r#"[{"event": "Paper submission", "deadline": "2026-12-08"}]"#);
        memory.sync();
        let body = fake.lock().unwrap().pages["schedule"].1.clone();
        assert!(body.starts_with("- Tuesdays 11:00: 1:1 with [[people/ann]]\n"));
        assert!(body.contains("- 2026-12-08 (Tue): Paper submission") && !body.contains("2026-12-01"), "{body}");

        // Switched off: the status says so, and nothing else is asked.
        fake.lock().unwrap().access = Some("none");
        fake.lock().unwrap().requests.clear();
        memory.sync();
        assert_eq!(fake.lock().unwrap().requests, ["GET /me"]);
        assert_eq!(status_of(Some(&memory))["access"], json!("none"));
    }

    #[test]
    fn a_failed_write_shows_in_settings_until_one_succeeds() {
        let mut env = crate::test_support::TestEnv::with_fixture();
        env.set_now("2026-11-17T10:00:00");
        let mut fake = with_access("write");
        fake.refuse_writes = Some((507, "Memory holds 1000 pages, the most it keeps."));
        let (client, fake) = serve(fake);
        let memory = memory(client);
        memory.sync();
        let status = status_of(Some(&memory));
        assert_eq!(
            status["schedule"]["error"],
            json!("LaunchX answered 507: Memory holds 1000 pages, the most it keeps.")
        );
        assert_eq!((&status["access"], &status["error"]), (&json!("write"), &Value::Null));
        fake.lock().unwrap().refuse_writes = None;
        memory.sync();
        let status = status_of(Some(&memory));
        assert_eq!(status["schedule"]["error"], Value::Null);
        assert!(status["schedule"]["rev"].is_string());
    }

    #[test]
    fn an_unreachable_launcher_shows_as_such() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let memory = memory(Client::new(&format!("http://127.0.0.1:{port}/api/memory"), "memory-token"));
        memory.sync();
        let status = status_of(Some(&memory));
        assert_eq!((&status["access"], &status["reachable"]), (&Value::Null, &json!(false)));
        assert!(status["error"].as_str().unwrap().starts_with("LaunchX did not answer"), "{status}");
        assert!(!status.to_string().contains("memory-token"));
    }

    #[test]
    fn outside_launchx_the_status_says_where_memory_comes_from() {
        if MEMORY.get().is_some() {
            return;
        }
        let status = status();
        assert_eq!(status["available"], json!(false));
        assert_eq!(status["access"], Value::Null);
        assert_eq!(status["change"], json!("lx memory access rmd write"));
    }
}
