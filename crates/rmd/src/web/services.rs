//! Structured service operations for the browser interface (port of
//! `web/services.py`).
//!
//! These functions wrap the storage and schedule helpers without printing CLI
//! tables or prompts. Each takes the request payload (a JSON object) and
//! returns a JSON value; failures are [`WebError`]s with a stable code.

use std::collections::HashSet;
use std::path::Path;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};
use indexmap::IndexMap;
use serde_json::{Value, json};

use crate::commands::deadlines::refresh_deadlines;
use crate::commands::history::{format_duration, pair_task_activities, parse_log_timestamp};
use crate::commands::settings::{SettingsModel, default_task_types};
use crate::commands::status::current_and_next_at;
use crate::config::{ScheduleConfig, TomlLoadError, WeeklySchedule};
use crate::data::{
    get_procrastinate_age_days, load_deadlines, load_habit_records, load_habits, load_procrastinate_records,
    load_task_log, load_tasks, log_task_action, save_deadlines, save_habit_records, save_procrastinate_list,
    save_task_log, save_tasks,
};
use crate::paths::{self, RuntimePaths};
use crate::py::{self, Dict};
use crate::recurrence::{REPEAT_RULES, anchor_day, repeat_rule};
use crate::synced_schedule::{
    apply_synced_schedule, format_event_label, get_event_block_name, iter_syncable_slots, load_synced_schedule,
    synced_schedule_matches_today,
};
use crate::time_utils::get_week_parity;

// =============================================================================
// Errors
// =============================================================================

/// Structured error for browser API responses.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{message}")]
pub struct WebError {
    pub code: String,
    pub message: String,
    pub details: Dict,
}

/// Result of a browser API operation.
pub type WebResult<T = Value> = Result<T, WebError>;

impl WebError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_string(), message: message.into(), details: Dict::new() }
    }

    pub fn with_details(code: &str, message: impl Into<String>, details: Dict) -> Self {
        Self { code: code.to_string(), message: message.into(), details }
    }

    /// An unexpected failure (the Python API reported any other exception as
    /// `internal_error` with `str(exc)`).
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal_error", message)
    }

    /// `{"code", "message", "details"}`.
    pub fn to_value(&self) -> Value {
        json!({"code": self.code, "message": self.message, "details": self.details})
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> WebError {
    WebError::new("invalid_input", message)
}

// =============================================================================
// Python semantics shared by the browser modules
// =============================================================================

/// Named marker colors for deadlines; the browser maps each name to a themed hue.
pub const DEADLINE_COLORS: [&str; 8] = ["blue", "orange", "aqua", "yellow", "magenta", "green", "violet", "red"];
pub const SUBTASK_LIMIT: usize = 50;
pub const SUBTASK_TEXT_LIMIT: usize = 200;
pub const TASK_TYPE_NAME_LIMIT: usize = 60;
/// Saved type colors, so a type keeps its color when the types before it change.
pub const TASK_TYPE_COLORS: &str = "task_type_colors";
const CLOSING_ACTIONS: [&str; 3] = ["deleted", "cancelled", "dropped"];
const DUE_FORMATS: [&str; 3] = ["%Y-%m-%dT%H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M"];

/// Python `str.strip()` (its whitespace also covers `\x1c`-`\x1f`).
pub(crate) fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// Python's type name for a JSON value (for `TypeError`-style messages).
pub(crate) fn py_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// `str(value).strip()` for a present, non-null value; `""` otherwise.
fn stripped_or_empty(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(v) => py_strip(&py::py_str(v)).to_string(),
    }
}

/// ISO date (`date.isoformat()`).
pub(crate) fn iso_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Ordering of `int(x)` for a digit string (any length): shorter is smaller
/// once leading zeros are gone.
fn int_key(digits: &str) -> (usize, &str) {
    let trimmed = digits.trim_start_matches('0');
    (trimmed.len(), trimmed)
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// `sorted(ids, key=lambda x: int(x) if x.isdigit() else 999)`.
fn sort_ids_numeric_or_999(ids: &mut [&String]) {
    ids.sort_by(|a, b| {
        let ka = if is_digits(a) { int_key(a) } else { int_key("999") };
        let kb = if is_digits(b) { int_key(b) } else { int_key("999") };
        ka.cmp(&kb)
    });
}

/// `_type_order`: numeric IDs first in numeric order, then the rest by text.
fn type_order(a: &str, b: &str) -> std::cmp::Ordering {
    match (is_digits(a), is_digits(b)) {
        (true, true) => int_key(a).cmp(&int_key(b)),
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (false, false) => a.cmp(b),
    }
}

/// `str(int(digits) + 1)` for a digit string of any length.
fn increment_digits(digits: &str) -> String {
    let trimmed = digits.trim_start_matches('0');
    let mut bytes: Vec<u8> = if trimmed.is_empty() { b"0".to_vec() } else { trimmed.as_bytes().to_vec() };
    let mut index = bytes.len();
    loop {
        if index == 0 {
            bytes.insert(0, b'1');
            break;
        }
        index -= 1;
        if bytes[index] == b'9' {
            bytes[index] = b'0';
        } else {
            bytes[index] += 1;
            break;
        }
    }
    String::from_utf8(bytes).expect("ASCII digits")
}

/// `OSError.__str__`: `[Errno 13] Permission denied: '/path'`.
pub(crate) fn os_error(error: &std::io::Error, path: &Path) -> String {
    match error.raw_os_error() {
        Some(code) => {
            let text = error.to_string();
            let suffix = format!(" (os error {code})");
            let reason = text.strip_suffix(&suffix).unwrap_or(&text);
            format!("[Errno {code}] {reason}: {}", py::repr_str(&path.display().to_string()))
        }
        None => error.to_string(),
    }
}

/// `'utf-8' codec can't decode ...` for invalid bytes.
pub(crate) fn utf8_error(bytes: &[u8], error: &std::str::Utf8Error) -> String {
    let position = error.valid_up_to();
    let (length, reason) = match error.error_len() {
        None => (bytes.len() - position, "unexpected end of data"),
        Some(length) => {
            let start = bytes[position];
            let invalid_start = (0x80..=0xc1).contains(&start) || start >= 0xf5;
            (length, if invalid_start { "invalid start byte" } else { "invalid continuation byte" })
        }
    };
    if length <= 1 {
        format!("'utf-8' codec can't decode byte 0x{:02x} in position {position}: {reason}", bytes[position])
    } else {
        format!("'utf-8' codec can't decode bytes in position {position}-{}: {reason}", position + length - 1)
    }
}

/// `date + timedelta(days=days)` with Python's range limits.
pub(crate) fn add_days(date: NaiveDate, days: i64) -> Result<NaiveDate, String> {
    if days.abs() > 999_999_999 {
        return Err(format!("days={days}; must have magnitude <= 999999999"));
    }
    date.checked_add_signed(Duration::days(days))
        .filter(|d| (1..=9999).contains(&d.year()))
        .ok_or_else(|| "date value out of range".to_string())
}

/// Current local date.
pub fn today() -> NaiveDate {
    crate::clock::today()
}

fn obj(value: &Value) -> Option<&Dict> {
    value.as_object()
}

/// `item.get(key) == text` for a list entry that should be an object.
fn field_is(item: &Value, key: &str, text: &str) -> bool {
    obj(item).and_then(|m| m.get(key)).and_then(Value::as_str) == Some(text)
}

// =============================================================================
// Payload validation
// =============================================================================

/// A non-blank string field, stripped.
pub fn require_text(payload: &Dict, key: &str) -> WebResult<String> {
    match payload.get(key) {
        Some(Value::String(value)) if !py_strip(value).is_empty() => Ok(py_strip(value).to_string()),
        _ => Err(invalid(format!("{key} is required."))),
    }
}

fn require_priority(payload: &Dict) -> WebResult<i64> {
    let priority =
        payload.get("priority").and_then(py::py_int).ok_or_else(|| invalid("priority must be an integer."))?;
    if !(1..=10).contains(&priority) {
        return Err(invalid("priority must be between 1 and 10."));
    }
    Ok(priority)
}

/// A timed task's finish-by time, or `None` when it has none or it is unreadable.
fn parse_due(value: Option<&Value>) -> Option<NaiveDateTime> {
    let text = py_strip(value?.as_str()?);
    if text.is_empty() {
        return None;
    }
    DUE_FORMATS.iter().find_map(|format| py::strptime_datetime(text, format))
}

fn format_due(due: NaiveDateTime) -> String {
    due.format("%Y-%m-%dT%H:%M").to_string()
}

/// `due` from a payload: a local `YYYY-MM-DDTHH:MM` makes the task timed;
/// empty or null clears it.
fn require_due(payload: &Dict) -> WebResult<Option<String>> {
    match payload.get("due") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if py_strip(s).is_empty() => Ok(None),
        value => parse_due(value)
            .map(|due| Some(format_due(due)))
            .ok_or_else(|| invalid("due must be a local date and time like 2026-11-05T18:00.")),
    }
}

/// A closing action from a payload (`deleted`, `cancelled`, or `dropped`).
fn closing_action(value: &Value) -> WebResult<String> {
    match value {
        Value::Array(_) => Err(WebError::internal("unhashable type: 'list'")),
        Value::Object(_) => Err(WebError::internal("unhashable type: 'dict'")),
        Value::String(action) if CLOSING_ACTIONS.contains(&action.as_str()) => Ok(action.clone()),
        _ => Err(invalid("invalid task action.")),
    }
}

// =============================================================================
// Subtasks and progress
// =============================================================================

/// A task's subtasks as `{"text", "done"}` pairs; unreadable entries are skipped.
fn subtasks_of(task: &Dict) -> Vec<Value> {
    let Some(Value::Array(raw)) = task.get("subtasks") else {
        return Vec::new();
    };
    raw.iter()
        .filter_map(|item| {
            let item = item.as_object()?;
            let text = py_strip(item.get("text")?.as_str()?);
            if text.is_empty() {
                return None;
            }
            Some(json!({"text": text, "done": item.get("done") == Some(&Value::Bool(true))}))
        })
        .collect()
}

/// The share of subtasks done, in whole percent, rounding halves up like the browser.
fn subtask_progress(subtasks: &[Value]) -> i64 {
    let total = subtasks.len() as i64;
    let done = subtasks.iter().filter(|item| item["done"] == Value::Bool(true)).count() as i64;
    (200 * done + total).div_euclid(2 * total)
}

/// How far along a task is: the share of its subtasks done, or else its own progress.
fn task_progress_of(task: &Dict) -> i64 {
    let subtasks = subtasks_of(task);
    if !subtasks.is_empty() {
        return subtask_progress(&subtasks);
    }
    let progress = match task.get("progress") {
        None => 0,
        Some(value) => match py::py_int(value) {
            Some(progress) => progress,
            None => return 0,
        },
    };
    progress.clamp(0, 100)
}

fn require_subtasks(payload: &Dict) -> WebResult<Vec<Value>> {
    let Some(Value::Array(raw)) = payload.get("subtasks") else {
        return Err(invalid("subtasks must be a list."));
    };
    if raw.len() > SUBTASK_LIMIT {
        return Err(invalid(format!("a task can have at most {SUBTASK_LIMIT} subtasks.")));
    }
    let mut subtasks = Vec::new();
    for item in raw {
        let text = match item.as_object().and_then(|m| m.get("text")) {
            Some(Value::String(text)) if !py_strip(text).is_empty() => py_strip(text).to_string(),
            _ => return Err(invalid("every subtask needs text.")),
        };
        if text.chars().count() > SUBTASK_TEXT_LIMIT {
            return Err(invalid(format!("subtasks can be at most {SUBTASK_TEXT_LIMIT} characters.")));
        }
        let done = match item.get("done") {
            None => false,
            Some(Value::Bool(done)) => *done,
            Some(_) => return Err(invalid("a subtask is either done or not: use true or false.")),
        };
        subtasks.push(json!({"text": text, "done": done}));
    }
    Ok(subtasks)
}

// =============================================================================
// Storage wrappers
// =============================================================================

fn save_tasks_or_error(tasks: &[Value]) -> WebResult<()> {
    save_tasks(tasks).map_err(|e| {
        WebError::new("storage_error", format!("failed to save tasks: {}", os_error(&e, &paths::runtime().tasks_path)))
    })
}

fn log_task_or_error(action: &str, task: &Value, metadata: Option<&Value>) -> WebResult<Value> {
    log_task_action(action, task, metadata).map_err(|e| {
        WebError::new(
            "storage_error",
            format!("failed to log task action: {}", os_error(&e, &paths::runtime().task_log_path)),
        )
    })
}

fn save_deadlines_or_error(deadlines: &[Value]) -> WebResult<()> {
    save_deadlines(deadlines).map_err(|e| {
        WebError::new(
            "storage_error",
            format!("failed to save deadlines: {}", os_error(&e, &paths::runtime().ddl_path)),
        )
    })
}

fn save_habit_records_or_error(records: &[Value]) -> WebResult<()> {
    save_habit_records(records).map_err(|e| {
        WebError::new(
            "storage_error",
            format!("failed to save habit records: {}", os_error(&e, &paths::runtime().record_path)),
        )
    })
}

/// Write procrastination records as `json.dumps(records, indent=2)`.
fn write_procrastinate_records(records: Vec<Value>, create_parent: bool) -> WebResult<()> {
    let path = paths::runtime().procrastinate_path;
    if create_parent && let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| WebError::internal(os_error(&e, parent)))?;
    }
    std::fs::write(&path, py::dumps_pretty(&records)).map_err(|e| WebError::internal(os_error(&e, &path)))
}

// =============================================================================
// Paths, task types, and configuration
// =============================================================================

/// Runtime paths for the snapshot without migrating the on-disk layout; a
/// legacy flat `settings.toml` in the root is used while the active set has none.
pub fn snapshot_runtime_paths() -> RuntimePaths {
    let root_dir = paths::resolve_config_root_dir();
    let mut active_config_dir = paths::preview_active_config_dir(Some(&root_dir));
    let active_id = active_config_dir
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix(paths::CONFIG_SET_PREFIX))
        .and_then(py::parse_int_str)
        .unwrap_or(0);
    if !active_config_dir.join("settings.toml").exists() && root_dir.join("settings.toml").exists() {
        active_config_dir = root_dir.clone();
    }
    let tasks_dir = root_dir.join("tasks");
    RuntimePaths {
        active_id,
        settings_path: active_config_dir.join("settings.toml"),
        odd_path: active_config_dir.join("odd_weeks.toml"),
        even_path: active_config_dir.join("even_weeks.toml"),
        ddl_path: active_config_dir.join("ddl.json"),
        habit_path: active_config_dir.join("habits.toml"),
        profile_path: active_config_dir.join("profile.md"),
        tasks_path: tasks_dir.join("tasks.json"),
        task_log_path: tasks_dir.join("tasks.log"),
        record_path: tasks_dir.join("record.json"),
        procrastinate_path: tasks_dir.join("procrastinate.json"),
        mode_path: tasks_dir.join("mode.txt"),
        active_config_dir,
        root_dir,
    }
}

/// Task types from settings.toml (`{id: name}`), or the defaults.
pub fn load_task_types() -> Dict {
    let task_types = ScheduleConfig::load(&snapshot_runtime_paths().settings_path)
        .map(|config| config.task_types)
        .unwrap_or_default();
    if task_types.is_empty() { default_task_types() } else { task_types }
}

/// The lowest numeric type ID (the type of tasks saved without one).
pub fn default_task_type(task_types: &Dict) -> String {
    let mut ids: Vec<&String> = task_types.keys().collect();
    sort_ids_numeric_or_999(&mut ids);
    ids.first().map(|id| id.to_string()).unwrap_or_else(|| "1".to_string())
}

/// Saved type colors; only the palette's color names count.
pub fn load_task_type_colors() -> Dict {
    let Ok(data) = crate::config::load_toml_file(&snapshot_runtime_paths().settings_path) else {
        return Dict::new();
    };
    let Some(Value::Object(saved)) = data.get(TASK_TYPE_COLORS) else {
        return Dict::new();
    };
    saved
        .iter()
        .filter(|(_, value)| value.as_str().is_some_and(|color| DEADLINE_COLORS.contains(&color)))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Each type's color as the browser shows it now: its saved color, or else
/// the palette color of its place among the types (the browser's fallback).
fn shown_type_colors(task_types: &Dict) -> Dict {
    let saved = load_task_type_colors();
    let mut ordered: Vec<&String> = task_types.keys().collect();
    ordered.sort_by(|a, b| type_order(a, b));
    ordered
        .into_iter()
        .enumerate()
        .map(|(index, type_id)| {
            let color = match saved.get(type_id.as_str()) {
                Some(color) if py::truthy(color) => color.clone(),
                _ => Value::String(DEADLINE_COLORS[index % DEADLINE_COLORS.len()].to_string()),
            };
            (type_id.clone(), color)
        })
        .collect()
}

/// The color the browser gives a type ID that is not in the settings.
fn hashed_type_color(type_id: &str) -> &'static str {
    let sum: u64 = type_id.chars().map(|c| u64::from(u32::from(c))).sum();
    DEADLINE_COLORS[(sum % DEADLINE_COLORS.len() as u64) as usize]
}

fn least_used_color(colors: &Dict) -> &'static str {
    let mut counts: IndexMap<String, usize> = DEADLINE_COLORS.iter().map(|c| (c.to_string(), 0)).collect();
    for color in colors.values() {
        *counts.entry(py::py_str(color)).or_insert(0) += 1;
    }
    DEADLINE_COLORS.iter().copied().min_by_key(|color| counts[*color]).expect("palette is not empty")
}

fn save_task_types(task_types: &Dict, colors: Option<&Dict>) -> WebResult<()> {
    let runtime_paths = snapshot_runtime_paths();
    let mut model = SettingsModel::new(&runtime_paths.settings_path)
        .map_err(|e| WebError::new("config_error", format!("failed to load settings: {e}")))?;
    model.data.insert("task_types".into(), Value::Object(task_types.clone()));
    if let Some(colors) = colors {
        let mut ids: Vec<&String> = colors.keys().collect();
        ids.sort_by(|a, b| type_order(a, b));
        let saved: Dict = ids
            .into_iter()
            .filter(|id| task_types.contains_key(id.as_str()))
            .map(|id| (id.clone(), colors[id.as_str()].clone()))
            .collect();
        model.data.insert(TASK_TYPE_COLORS.into(), Value::Object(saved));
    }
    model.dirty = true;
    model.save().map_err(|e| WebError::new("storage_error", format!("failed to save settings: {e}")))
}

/// One past the highest numeric type ID ever used, by a type, a task, a
/// deadline, or the task history, so a new type never inherits another's past.
fn next_task_type_id(task_types: &Dict) -> String {
    let mut used: HashSet<String> = task_types.keys().cloned().collect();
    let tasks = load_tasks();
    let deadlines = load_deadlines();
    for item in tasks.iter().chain(deadlines.iter()) {
        if let Some(type_id) = obj(item).and_then(|m| m.get("type")).filter(|v| !v.is_null()) {
            used.insert(py::py_str(type_id));
        }
    }
    for entry in load_task_log() {
        if let Some(type_id) = obj(&entry)
            .and_then(|m| m.get("task"))
            .and_then(Value::as_object)
            .and_then(|task| task.get("type"))
            .filter(|v| !v.is_null())
        {
            used.insert(py::py_str(type_id));
        }
    }
    let highest = used.iter().filter(|id| is_digits(id)).max_by(|a, b| int_key(a).cmp(&int_key(b)));
    increment_digits(highest.map(String::as_str).unwrap_or("0"))
}

// =============================================================================
// Status snapshot
// =============================================================================

fn today_schedule(runtime_paths: &RuntimePaths) -> Result<(Dict, String, bool, ScheduleConfig), TomlLoadError> {
    let config = ScheduleConfig::load(&runtime_paths.settings_path)?;
    let weekly = WeeklySchedule::load(&runtime_paths.odd_path, &runtime_paths.even_path)?;
    let is_skipped = config.should_skip_today();
    let parity = get_week_parity();
    let mut schedule = if is_skipped { Dict::new() } else { weekly.get_today_schedule(&config) };
    if !schedule.is_empty() {
        let current = today();
        schedule =
            apply_synced_schedule(&schedule, Some(current), Some(&parity), Some(py::weekday_name(current)), None);
    }
    Ok((schedule, parity, is_skipped, config))
}

fn sorted_tasks() -> Vec<Value> {
    let current = today();
    let task_types = load_task_types();
    let default_type = default_task_type(&task_types);
    let records = load_procrastinate_records();

    let mut normalized: Vec<(u8, i64, Value)> = Vec::new();
    for task in load_tasks() {
        let Value::Object(task) = task else { continue };
        let Some(Value::String(description)) = task.get("description") else { continue };
        if py_strip(description).is_empty() {
            continue;
        }
        let priority = task.get("priority").and_then(py::py_int).unwrap_or(0);
        let task_type = task.get("type").map(py::py_str).unwrap_or_else(|| default_type.clone());
        let type_name = task_types
            .get(&task_type)
            .or_else(|| task_types.get(&default_type))
            .cloned()
            .unwrap_or_else(|| Value::String("other".into()));

        let mut alarm_from: Option<String> = None;
        let mut days_left = 0;
        if let Some(Value::String(raw)) = task.get("alarm_from")
            && !py_strip(raw).is_empty()
        {
            let stripped = py_strip(raw).to_string();
            days_left = py::strptime_date(&stripped, "%Y-%m-%d").map(|d| (d - current).num_days()).unwrap_or(0);
            alarm_from = Some(stripped);
        }
        let postponed = days_left > 0;
        let record = records.get(description.as_str());
        let procrastinated = !postponed && record.is_some();
        let procrastinate_days = if procrastinated {
            let since = record.and_then(|r| r.get("since")).and_then(Value::as_str);
            get_procrastinate_age_days(since, Some(current))
        } else {
            None
        };
        let section = if procrastinated {
            0
        } else if !postponed {
            1
        } else {
            2
        };
        let due = parse_due(task.get("due"));
        normalized.push((
            section,
            priority,
            json!({
                "description": py_strip(description),
                "priority": priority,
                "type": task_type,
                "typeName": type_name,
                "alarmFrom": alarm_from,
                "procrastinated": procrastinated,
                "procrastinateDays": procrastinate_days,
                "due": due.map(format_due),
                "progress": task_progress_of(&task),
                "subtasks": subtasks_of(&task),
            }),
        ));
    }
    normalized.sort_by_key(|(section, priority, _)| (*section, std::cmp::Reverse(*priority)));
    normalized.into_iter().map(|(_, _, row)| row).collect()
}

fn deadline_status(deadline: NaiveDate, current: NaiveDate) -> &'static str {
    let days_left = (deadline - current).num_days();
    if days_left < 0 {
        "overdue"
    } else if days_left == 0 {
        "today"
    } else if days_left <= 3 {
        "urgent"
    } else if days_left <= 7 {
        "soon"
    } else {
        "ok"
    }
}

fn deadline_rows(current: NaiveDate) -> WebResult<Vec<Value>> {
    let refreshed = refresh_deadlines(&load_deadlines(), Some(current));
    if refreshed.changed {
        save_deadlines_or_error(&refreshed.kept)?;
    }
    let task_types = load_task_types();
    let mut items: Vec<&Dict> = refreshed.kept.iter().filter_map(Value::as_object).collect();
    items.sort_by_cached_key(|item| item.get("deadline").map(py::py_str).unwrap_or_default());

    let mut rows = Vec::new();
    for item in items {
        let event = py_strip(&py::get_str_or_empty(item, "event")).to_string();
        let raw_deadline = py_strip(&py::get_str_or_empty(item, "deadline")).to_string();
        if event.is_empty() || raw_deadline.is_empty() {
            continue;
        }
        let type_id = Some(stripped_or_empty(item.get("type"))).filter(|id| !id.is_empty());
        let raw_color = match item.get("color") {
            Some(color) if py::truthy(color) => py_strip(&py::py_str(color)).to_lowercase(),
            _ => String::new(),
        };
        let rule = repeat_rule(item);
        let type_name = type_id.as_ref().map(|id| task_types.get(id).cloned().unwrap_or_else(|| json!(id)));
        let color = DEADLINE_COLORS.contains(&raw_color.as_str()).then_some(raw_color);
        let Some(deadline) = py::strptime_date(&raw_deadline, "%Y-%m-%d") else {
            rows.push(json!({
                "event": event,
                "deadline": raw_deadline,
                "daysLeft": null,
                "status": "invalid",
                "type": type_id,
                "typeName": type_name,
                "color": color,
                "repeat": rule,
                "repeatDay": null,
            }));
            continue;
        };
        let repeat_day = (rule == Some("monthly")).then(|| anchor_day(item, deadline));
        rows.push(json!({
            "event": event,
            "deadline": iso_date(deadline),
            "daysLeft": (deadline - current).num_days(),
            "status": deadline_status(deadline, current),
            "type": type_id,
            "typeName": type_name,
            "color": color,
            "repeat": rule,
            "repeatDay": repeat_day,
        }));
    }
    Ok(rows)
}

fn habit_rows(current: NaiveDate) -> Vec<Value> {
    let habits = load_habits();
    let records = load_habit_records();
    let today_iso = iso_date(current);
    let mut completed: HashSet<String> = HashSet::new();
    for record in &records {
        if record.get("date").and_then(Value::as_str) == Some(today_iso.as_str()) {
            if let Some(Value::Object(raw)) = record.get("completed") {
                completed = raw.keys().cloned().collect();
            }
            break;
        }
    }
    let mut sorted: Vec<(&String, &String)> = habits.iter().collect();
    sorted.sort();
    sorted
        .into_iter()
        .map(|(id, description)| json!({"id": id, "description": description, "completed": completed.contains(id)}))
        .collect()
}

fn schedule_rows(schedule: &Dict) -> Vec<Value> {
    let mut times: Vec<&String> = schedule.keys().collect();
    times.sort();
    times
        .into_iter()
        .map(|time| {
            let event = &schedule[time.as_str()];
            let mut single = Dict::new();
            single.insert(time.clone(), event.clone());
            json!({
                "time": time,
                "label": format_event_label(event),
                "block": get_event_block_name(event),
                "syncable": !iter_syncable_slots(&single).is_empty(),
            })
        })
        .collect()
}

/// Everything the browser's pages show for today.
pub fn status_snapshot(_payload: &Dict) -> WebResult {
    let current = today();
    let runtime_paths = snapshot_runtime_paths();

    let (schedule, parity, is_skipped, config, configuration_error) = match today_schedule(&runtime_paths) {
        Ok((schedule, parity, is_skipped, config)) => (schedule, parity, is_skipped, Some(config), None),
        // A fresh install must still be able to build or repair its schedule
        // entirely in the browser.
        Err(error) => (Dict::new(), get_week_parity(), false, None, Some(error.to_string())),
    };
    let events = current_and_next_at(&schedule, config.as_ref(), crate::clock::now());
    let synced = load_synced_schedule(None);
    let weekday = py::weekday_name(current);

    let config_info = json!({
        "rootDir": runtime_paths.root_dir.display().to_string(),
        "activeId": runtime_paths.active_id,
        "activeConfigDir": runtime_paths.active_config_dir.display().to_string(),
        "tasksPath": runtime_paths.tasks_path.display().to_string(),
        "deadlinesPath": runtime_paths.ddl_path.display().to_string(),
        "habitsPath": runtime_paths.habit_path.display().to_string(),
        "recordsPath": runtime_paths.record_path.display().to_string(),
        "error": configuration_error,
    });
    let today_info = json!({"date": iso_date(current), "weekday": weekday, "parity": parity});
    let schedule_info = json!({
        "isSkipped": is_skipped,
        "current": events.current,
        "next": events.next,
        "timeToNext": events.time_to_next,
        "events": schedule_rows(&schedule),
        "hasSyncedOverlay": synced_schedule_matches_today(synced.as_ref(), Some(current), Some(&parity), Some(weekday)),
    });
    let tasks = sorted_tasks();
    let deadlines = deadline_rows(current)?;
    let habits = habit_rows(current);
    Ok(json!({
        "config": config_info,
        "today": today_info,
        "schedule": schedule_info,
        "tasks": tasks,
        "deadlines": deadlines,
        "habits": habits,
        "taskTypes": load_task_types(),
        "taskTypeColors": load_task_type_colors(),
        "history": history_rows(Some(5)),
        "timedDone": timed_done_today(current),
    }))
}

/// Timed tasks completed today that were due by tonight. They still count, as
/// finished work, toward today's progress; a task that is open again is left out.
fn timed_done_today(current: NaiveDate) -> Vec<Value> {
    let log_entries = load_task_log();
    let open: HashSet<String> =
        load_tasks().iter().filter_map(|task| obj(task)?.get("description")?.as_str().map(str::to_string)).collect();
    let mut done: IndexMap<String, (String, Value)> = IndexMap::new();
    for entry in &log_entries {
        let Some(entry) = obj(entry) else { continue };
        if entry.get("action").and_then(Value::as_str) != Some("deleted") {
            continue;
        }
        let Some(Value::Object(task)) = entry.get("task") else { continue };
        let due = parse_due(task.get("due"));
        let ended = entry.get("timestamp").and_then(parse_log_timestamp);
        let (Some(Value::String(description)), Some(due), Some(ended)) = (task.get("description"), due, ended) else {
            continue;
        };
        if py_strip(description).is_empty() {
            continue;
        }
        if ended.date() != current || due.date() > current || open.contains(description) {
            continue;
        }
        let priority = task.get("priority").and_then(py::py_int).unwrap_or(0);
        let task_type = task.get("type").filter(|v| !v.is_null()).map(py::py_str);
        let ended_at = py::isoformat_naive(&ended);
        done.insert(
            description.clone(),
            (
                ended_at.clone(),
                json!({
                    "description": description,
                    "priority": priority,
                    "type": task_type,
                    "due": format_due(due),
                    "endedAt": ended_at,
                }),
            ),
        );
    }
    let mut rows: Vec<(String, Value)> = done.into_values().collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.into_iter().map(|(_, row)| row).collect()
}

// =============================================================================
// Tasks
// =============================================================================

/// Add a task, or replace the task with the same description.
pub fn task_add(payload: &Dict) -> WebResult {
    let description = require_text(payload, "description")?;
    let priority = require_priority(payload)?;

    let task_types = load_task_types();
    let mut task_type = stripped_or_empty(payload.get("type"));
    if task_type.is_empty() {
        let mut ids: Vec<&String> = task_types.keys().collect();
        sort_ids_numeric_or_999(&mut ids);
        task_type = ids.first().map(|id| id.to_string()).unwrap_or_else(|| "1".to_string());
    } else if !task_types.contains_key(&task_type) {
        return Err(invalid(format!("unknown task type: {task_type}")));
    }

    let mut alarm_from = None;
    if let Some(postpone) = payload.get("postpone").filter(|v| !v.is_null()) {
        let days = py::py_int(postpone).ok_or_else(|| invalid("postpone must be an integer."))?;
        if days < 0 {
            return Err(invalid("postpone days must be non-negative."));
        }
        if days > 0 {
            alarm_from = Some(iso_date(add_days(today(), days).map_err(WebError::internal)?));
        }
    }
    let due = require_due(payload)?;

    let mut tasks = load_tasks();
    let mut new_task = Dict::new();
    new_task.insert("description".into(), json!(description));
    new_task.insert("priority".into(), json!(priority));
    new_task.insert("type".into(), json!(task_type));
    if let Some(alarm_from) = alarm_from {
        new_task.insert("alarm_from".into(), json!(alarm_from));
    }
    if let Some(due) = due {
        new_task.insert("due".into(), json!(due));
    }
    let new_task = Value::Object(new_task);

    let (action, metadata) = match tasks.iter().position(|task| field_is(task, "description", &description)) {
        None => {
            tasks.push(new_task.clone());
            ("added", None)
        }
        Some(index) => {
            let old_priority = obj(&tasks[index]).and_then(|m| m.get("priority")).cloned().unwrap_or(Value::Null);
            tasks[index] = new_task.clone();
            ("updated", Some(json!({"old_priority": old_priority})))
        }
    };
    save_tasks_or_error(&tasks)?;
    log_task_or_error(action, &new_task, metadata.as_ref())?;
    Ok(new_task)
}

/// Edit a task's description, priority, type, deferral, or finish-by time.
pub fn task_update(payload: &Dict) -> WebResult {
    let original = require_text(payload, "originalDescription")?;
    let description = require_text(payload, "description")?;
    let priority = require_priority(payload)?;
    let mut tasks = load_tasks();

    if tasks.iter().any(|task| field_is(task, "description", &description) && !field_is(task, "description", &original))
    {
        return Err(WebError::new("duplicate", format!("task already exists: {description}")));
    }

    let Some(index) = tasks.iter().position(|task| field_is(task, "description", &original)) else {
        return Err(WebError::new("not_found", format!("task not found: {original}")));
    };
    let old_task = tasks[index].clone();
    let mut updated = old_task.as_object().cloned().unwrap_or_default();
    updated.insert("description".into(), json!(description));
    updated.insert("priority".into(), json!(priority));
    if let Some(raw_type) = payload.get("type") {
        let task_type = py::py_str(raw_type);
        if !load_task_types().contains_key(&task_type) {
            return Err(invalid(format!("unknown task type: {task_type}")));
        }
        updated.insert("type".into(), json!(task_type));
    }
    if let Some(postpone) = payload.get("postpone") {
        let error = || invalid("postpone must be a non-negative integer.");
        let days = py::py_int(postpone).filter(|days| *days >= 0).ok_or_else(error)?;
        if days > 0 {
            let alarm_from = add_days(today(), days).map_err(|_| error())?;
            updated.insert("alarm_from".into(), json!(iso_date(alarm_from)));
        } else {
            updated.shift_remove("alarm_from");
        }
    }
    if payload.contains_key("due") {
        match require_due(payload)? {
            Some(due) => {
                updated.insert("due".into(), json!(due));
            }
            None => {
                updated.shift_remove("due");
            }
        }
    }
    let updated = Value::Object(updated);
    tasks[index] = updated.clone();
    save_tasks_or_error(&tasks)?;
    log_task_or_error("updated", &updated, Some(&json!({"old_task": old_task})))?;
    if original != description {
        let mut records = load_procrastinate_records();
        if let Some(mut record) = records.shift_remove(&original) {
            record.insert("description".into(), json!(description));
            records.insert(description.clone(), record);
            write_procrastinate_records(records.into_values().map(Value::Object).collect(), false)?;
        }
    }
    Ok(updated)
}

/// Set how far along a task is, from 0 to 100 percent. Progress moves often,
/// so it is saved on the task without a history entry.
pub fn task_progress(payload: &Dict) -> WebResult {
    let description = require_text(payload, "description")?;
    let invalid_progress = || invalid("progress must be a whole number from 0 to 100.");
    let number = match payload.get("progress") {
        Some(Value::Number(n)) => n.as_f64().ok_or_else(invalid_progress)?,
        Some(text @ Value::String(_)) => py::py_float(text).ok_or_else(invalid_progress)?,
        _ => return Err(invalid_progress()),
    };
    if !(number.is_finite() && number.fract() == 0.0 && (0.0..=100.0).contains(&number)) {
        return Err(invalid_progress());
    }
    let progress = number as i64;
    let mut tasks = load_tasks();
    let Some(index) = tasks.iter().position(|task| field_is(task, "description", &description)) else {
        return Err(WebError::new("not_found", format!("task not found: {description}")));
    };
    let task = tasks[index].as_object_mut().expect("matched tasks are objects");
    if !subtasks_of(task).is_empty() {
        return Err(invalid("this task's progress follows its subtasks."));
    }
    if progress != 0 {
        task.insert("progress".into(), json!(progress));
    } else {
        task.shift_remove("progress");
    }
    save_tasks_or_error(&tasks)?;
    Ok(json!({"description": description, "progress": progress}))
}

/// Replace a task's subtasks (each `{"text", "done"}`); the task's progress
/// then follows them. Clearing every subtask keeps the last progress as the
/// task's own. No history entry is written.
pub fn task_subtasks(payload: &Dict) -> WebResult {
    let description = require_text(payload, "description")?;
    let subtasks = require_subtasks(payload)?;
    let mut tasks = load_tasks();
    let Some(index) = tasks.iter().position(|task| field_is(task, "description", &description)) else {
        return Err(WebError::new("not_found", format!("task not found: {description}")));
    };
    let task = tasks[index].as_object_mut().expect("matched tasks are objects");
    if subtasks.is_empty() {
        task.shift_remove("subtasks");
    } else {
        task.insert("subtasks".into(), Value::Array(subtasks.clone()));
        let progress = subtask_progress(&subtasks);
        if progress != 0 {
            task.insert("progress".into(), json!(progress));
        } else {
            task.shift_remove("progress");
        }
    }
    let progress = task_progress_of(task);
    save_tasks_or_error(&tasks)?;
    Ok(json!({"description": description, "subtasks": subtasks, "progress": progress}))
}

/// Complete, cancel, or drop a task. The result carries an `undo` token with
/// everything `task_restore` needs to put the task back as it was.
pub fn task_delete(payload: &Dict) -> WebResult {
    let description = require_text(payload, "description")?;
    let action = closing_action(payload.get("action").unwrap_or(&json!("deleted")))?;
    let tasks = load_tasks();
    let deleted: Vec<Value> = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| field_is(task, "description", &description))
        .map(|(index, task)| json!({"index": index, "task": task}))
        .collect();
    if deleted.is_empty() {
        return Err(WebError::new("not_found", format!("task not found: {description}")));
    }
    let remaining: Vec<Value> = tasks.into_iter().filter(|task| !field_is(task, "description", &description)).collect();
    save_tasks_or_error(&remaining)?;
    let mut logged = Vec::new();
    for item in &deleted {
        let entry = log_task_or_error(&action, &item["task"], None)?;
        if let Some(Value::String(stamp)) = entry.get("timestamp") {
            logged.push(stamp.clone());
        }
    }
    let records = load_procrastinate_records();
    let procrastinated = records.get(&description).cloned();
    if procrastinated.is_some() {
        let keep = records.keys().filter(|key| **key != description);
        save_procrastinate_list(keep, None)
            .map_err(|e| WebError::internal(os_error(&e, &paths::runtime().procrastinate_path)))?;
    }
    let procrastinated = procrastinated.map(|record| json!({"since": record.get("since")}));
    Ok(json!({
        "description": description,
        "deleted": deleted.len(),
        "undo": {
            "description": description,
            "action": action,
            "tasks": deleted,
            "procrastinated": procrastinated,
            "logged": logged,
        },
    }))
}

/// Mark a task as deferred again, keeping the day it was first deferred.
fn restore_procrastinated(description: &str, since: Option<&str>) -> WebResult<()> {
    let mut records = load_procrastinate_records();
    let mut record = Dict::new();
    record.insert("description".into(), json!(description));
    if let Some(since) = since.filter(|s| !s.is_empty()) {
        record.insert("since".into(), json!(since));
    }
    records.insert(description.to_string(), record);
    let mut keys: Vec<&String> = records.keys().collect();
    keys.sort();
    let sorted = keys.into_iter().map(|key| Value::Object(records[key.as_str()].clone())).collect();
    write_procrastinate_records(sorted, true)
}

/// The stamps in `payload["logged"]` (`{stamp for stamp in logged or [] if isinstance(stamp, str)}`).
fn logged_stamps(value: Option<&Value>) -> WebResult<HashSet<String>> {
    match value {
        Some(value) if py::truthy(value) => match value {
            Value::Array(items) => Ok(items.iter().filter_map(Value::as_str).map(str::to_string).collect()),
            Value::String(text) => Ok(text.chars().map(String::from).collect()),
            Value::Object(map) => Ok(map.keys().cloned().collect()),
            other => Err(WebError::internal(format!("'{}' object is not iterable", py_type_name(other)))),
        },
        _ => Ok(HashSet::new()),
    }
}

/// Undo `task_delete` with the `undo` token it returned.
pub fn task_restore(payload: &Dict) -> WebResult {
    let description = require_text(payload, "description")?;
    let action = closing_action(payload.get("action").unwrap_or(&Value::Null))?;
    let items = match payload.get("tasks") {
        Some(Value::Array(items)) if !items.is_empty() => items,
        _ => return Err(invalid("tasks are required.")),
    };
    let mut restored: Vec<(u64, Value)> = Vec::new();
    for item in items {
        let item = obj(item);
        let task = item.and_then(|m| m.get("task")).filter(|t| t.is_object());
        let Some(task) = task.filter(|t| field_is(t, "description", &description)) else {
            return Err(invalid("every task must match the description."));
        };
        let index = item.and_then(|m| m.get("index")).and_then(|i| if i.is_f64() { None } else { i.as_u64() });
        let Some(index) = index else {
            return Err(invalid("task index must be a non-negative integer."));
        };
        restored.push((index, task.clone()));
    }

    let mut tasks = load_tasks();
    if tasks.iter().any(|task| field_is(task, "description", &description)) {
        return Err(WebError::new("duplicate", format!("“{description}” is already on your list.")));
    }
    // Indexes were taken from the list before the removal, so inserting in
    // order puts each task back in place.
    restored.sort_by_key(|(index, _)| *index);
    let count = restored.len();
    for (index, task) in restored {
        let at = usize::try_from(index).unwrap_or(usize::MAX).min(tasks.len());
        tasks.insert(at, task);
    }
    save_tasks_or_error(&tasks)?;

    // Take back the history entries the removal wrote, so it never counts as
    // completed, cancelled, or dropped.
    let logged = logged_stamps(payload.get("logged"))?;
    if !logged.is_empty() {
        let log_entries = load_task_log();
        let kept: Vec<Value> = log_entries
            .iter()
            .filter(|entry| {
                let Some(entry) = obj(entry) else { return true };
                let stamp_logged = entry.get("timestamp").and_then(Value::as_str).is_some_and(|s| logged.contains(s));
                let same_action = entry.get("action").and_then(Value::as_str) == Some(action.as_str());
                let same_task = entry.get("task").is_some_and(|task| field_is(task, "description", &description));
                !(stamp_logged && same_action && same_task)
            })
            .cloned()
            .collect();
        if kept.len() != log_entries.len() {
            save_task_log(&kept).map_err(|e| {
                WebError::new(
                    "storage_error",
                    format!("failed to save the task log: {}", os_error(&e, &paths::runtime().task_log_path)),
                )
            })?;
        }
    }

    if let Some(Value::Object(procrastinated)) = payload.get("procrastinated") {
        restore_procrastinated(&description, procrastinated.get("since").and_then(Value::as_str))?;
    }
    Ok(json!({"description": description, "restored": count}))
}

// =============================================================================
// Deadlines
// =============================================================================

fn deadline_date_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"^([0-9]{4})-(1[0-2]|0[1-9]|[1-9])-(3[01]|[12][0-9]|0[1-9]|[1-9]| [1-9])").unwrap()
    })
}

/// `date(year, month, day)` with Python's error messages.
fn py_date(year: i64, month: i64, day: i64) -> Result<NaiveDate, String> {
    if !(1..=9999).contains(&year) {
        return Err(format!("year {year} is out of range"));
    }
    if !(1..=12).contains(&month) {
        return Err("month must be in 1..12".to_string());
    }
    NaiveDate::from_ymd_opt(year as i32, month as u32, u32::try_from(day).unwrap_or(0))
        .ok_or_else(|| "day is out of range for month".to_string())
}

/// `datetime.strptime(text, "%Y-%m-%d").date()` with Python's error messages.
fn strptime_ymd(text: &str) -> Result<NaiveDate, String> {
    let Some(caps) = deadline_date_regex().captures(text) else {
        return Err(format!("time data {} does not match format '%Y-%m-%d'", py::repr_str(text)));
    };
    let matched = caps.get(0).expect("whole match").end();
    if matched != text.len() {
        return Err(format!("unconverted data remains: {}", &text[matched..]));
    }
    let number = |i: usize| caps[i].trim().parse::<i64>().expect("regex digits");
    py_date(number(1), number(2), number(3))
}

/// A Python `int(text)` result for the `M.D` deadline form.
enum IntText {
    Value(i64),
    /// Valid digits, but beyond a C long.
    TooLarge,
    Invalid,
}

fn parse_int_text(text: &str) -> IntText {
    if let Some(value) = py::parse_int_str(text) {
        return IntText::Value(value);
    }
    let trimmed = py_strip(text);
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    let well_formed = !digits.is_empty()
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
        && digits.chars().all(|c| c.is_ascii_digit() || c == '_');
    if well_formed { IntText::TooLarge } else { IntText::Invalid }
}

/// Converting to a C int (Python raises `OverflowError`, which the API reports
/// as an internal error).
fn c_int(value: &IntText) -> WebResult<i64> {
    match value {
        IntText::Value(v) if *v > i64::from(i32::MAX) => {
            Err(WebError::internal("signed integer is greater than maximum"))
        }
        IntText::Value(v) if *v < i64::from(i32::MIN) => Err(WebError::internal("signed integer is less than minimum")),
        IntText::Value(v) => Ok(*v),
        _ => Err(WebError::internal("Python int too large to convert to C long")),
    }
}

/// `YYYY-MM-DD`, or `M.D` (the next such day, this year or next).
fn parse_deadline_date(raw_date: &str) -> WebResult<String> {
    let text = py_strip(raw_date);
    let error = |message: String| invalid(format!("invalid deadline date: {message}"));
    if text.contains('-') {
        return strptime_ymd(text).map(iso_date).map_err(error);
    }
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 2 {
        return Err(error("date must be YYYY-MM-DD or M.D".to_string()));
    }
    let mut values = Vec::new();
    for part in &parts {
        match parse_int_text(part) {
            IntText::Invalid => {
                return Err(error(format!("invalid literal for int() with base 10: {}", py::repr_str(part))));
            }
            parsed => values.push(parsed),
        }
    }
    let current = today();
    let month = c_int(&values[0])?;
    let day = c_int(&values[1])?;
    let mut parsed = py_date(i64::from(current.year()), month, day).map_err(error)?;
    if parsed < current {
        parsed = py_date(i64::from(current.year()) + 1, month, day).map_err(error)?;
    }
    Ok(iso_date(parsed))
}

/// Keep legacy deadlines untyped and preserve types omitted by older clients.
fn apply_deadline_type(item: &mut Dict, payload: &Dict) -> WebResult<()> {
    if !payload.contains_key("type") {
        return Ok(());
    }
    let type_id = stripped_or_empty(payload.get("type"));
    if !type_id.is_empty()
        && !load_task_types().contains_key(&type_id)
        && type_id != py::py_str(item.get("type").unwrap_or(&Value::Null))
    {
        return Err(invalid(format!("unknown deadline type: {type_id}")));
    }
    if type_id.is_empty() {
        item.shift_remove("type");
    } else {
        item.insert("type".into(), json!(type_id));
    }
    Ok(())
}

/// Set or clear a deadline marker color; omitted colors stay unchanged.
fn apply_deadline_color(item: &mut Dict, payload: &Dict) -> WebResult<()> {
    if !payload.contains_key("color") {
        return Ok(());
    }
    let color = stripped_or_empty(payload.get("color")).to_lowercase();
    if !color.is_empty() && !DEADLINE_COLORS.contains(&color.as_str()) {
        return Err(invalid(format!("unknown deadline color: {color}")));
    }
    if color.is_empty() {
        item.shift_remove("color");
    } else {
        item.insert("color".into(), json!(color));
    }
    Ok(())
}

/// Set, clear, or keep a deadline's repeat rule. A monthly deadline remembers
/// the day of the month the user picked (`repeat_day`) and updates it only
/// when the user chooses a new date.
fn apply_deadline_repeat(item: &mut Dict, payload: &Dict, previous_deadline: Option<&Value>) -> WebResult<()> {
    if payload.contains_key("repeat") {
        let rule = stripped_or_empty(payload.get("repeat")).to_lowercase();
        if !rule.is_empty() && !REPEAT_RULES.contains(&rule.as_str()) {
            return Err(invalid(format!("unknown repeat rule: {rule}")));
        }
        if rule.is_empty() {
            item.shift_remove("repeat");
        } else {
            if item.get("repeat").and_then(Value::as_str) != Some(rule.as_str()) {
                item.shift_remove("repeat_day");
            }
            item.insert("repeat".into(), json!(rule));
        }
    }
    if item.get("repeat").and_then(Value::as_str) == Some("monthly") {
        let deadline = item.get("deadline").cloned().unwrap_or(Value::Null);
        if Some(&deadline) != previous_deadline || !item.contains_key("repeat_day") {
            let day = deadline.as_str().and_then(py::parse_iso_date).map(|d| d.day()).unwrap_or(1);
            item.insert("repeat_day".into(), json!(day));
        }
    } else {
        item.shift_remove("repeat_day");
    }
    Ok(())
}

/// Add a deadline, or update the deadline with the same event.
pub fn deadline_add(payload: &Dict) -> WebResult {
    let event = require_text(payload, "event")?;
    let deadline = parse_deadline_date(&require_text(payload, "date")?)?;
    let mut deadlines = load_deadlines();
    let existing = deadlines.iter().position(|item| field_is(item, "event", &event));
    let mut new_deadline = existing.and_then(|i| deadlines[i].as_object().cloned()).unwrap_or_default();
    new_deadline.insert("event".into(), json!(event));
    new_deadline.insert("deadline".into(), json!(deadline));
    new_deadline.insert("added".into(), json!(py::utc_now_isoformat()));
    apply_deadline_type(&mut new_deadline, payload)?;
    apply_deadline_color(&mut new_deadline, payload)?;
    let previous = existing.and_then(|i| obj(&deadlines[i])?.get("deadline").cloned());
    apply_deadline_repeat(&mut new_deadline, payload, previous.as_ref())?;
    let new_deadline = Value::Object(new_deadline);
    match existing {
        None => deadlines.push(new_deadline.clone()),
        Some(index) => deadlines[index] = new_deadline.clone(),
    }
    save_deadlines_or_error(&deadlines)?;
    Ok(new_deadline)
}

/// Rename a deadline or move it to another date.
pub fn deadline_update(payload: &Dict) -> WebResult {
    let original = require_text(payload, "originalEvent")?;
    let event = require_text(payload, "event")?;
    let deadline = parse_deadline_date(&require_text(payload, "date")?)?;
    let mut deadlines = load_deadlines();

    if deadlines.iter().any(|item| field_is(item, "event", &event) && !field_is(item, "event", &original)) {
        return Err(WebError::new("duplicate", format!("deadline already exists: {event}")));
    }
    let Some(index) = deadlines.iter().position(|item| field_is(item, "event", &original)) else {
        return Err(WebError::new("not_found", format!("deadline not found: {original}")));
    };
    let item = deadlines[index].as_object().cloned().unwrap_or_default();
    let added = match item.get("added") {
        Some(added) if py::truthy(added) => py::py_str(added),
        _ => py::utc_now_isoformat(),
    };
    let mut updated = item.clone();
    updated.insert("event".into(), json!(event));
    updated.insert("deadline".into(), json!(deadline));
    updated.insert("added".into(), json!(added));
    apply_deadline_type(&mut updated, payload)?;
    apply_deadline_color(&mut updated, payload)?;
    apply_deadline_repeat(&mut updated, payload, item.get("deadline"))?;
    let updated = Value::Object(updated);
    deadlines[index] = updated.clone();
    save_deadlines_or_error(&deadlines)?;
    Ok(updated)
}

/// Remove every deadline with the given event.
pub fn deadline_delete(payload: &Dict) -> WebResult {
    let event = require_text(payload, "event")?;
    let deadlines = load_deadlines();
    let deleted = deadlines.iter().filter(|item| field_is(item, "event", &event)).count();
    if deleted == 0 {
        return Err(WebError::new("not_found", format!("deadline not found: {event}")));
    }
    let remaining: Vec<Value> = deadlines.into_iter().filter(|item| !field_is(item, "event", &event)).collect();
    save_deadlines_or_error(&remaining)?;
    Ok(json!({"event": event, "deleted": deleted}))
}

// =============================================================================
// Habits
// =============================================================================

/// Record today's completed habits (replacing today's earlier record).
pub fn habit_mark(payload: &Dict) -> WebResult {
    let empty = Value::Array(Vec::new());
    let Value::Array(raw_ids) = payload.get("habitIds").unwrap_or(&empty) else {
        return Err(invalid("habitIds must be a list."));
    };
    let habits = load_habits();
    let mut valid_ids = Vec::new();
    let mut invalid_ids = Vec::new();
    for raw_id in raw_ids {
        let habit_id = py::py_str(raw_id);
        if habits.contains_key(&habit_id) {
            valid_ids.push(habit_id);
        } else {
            invalid_ids.push(Value::String(habit_id));
        }
    }
    if !invalid_ids.is_empty() {
        let mut details = Dict::new();
        details.insert("invalidIds".into(), Value::Array(invalid_ids));
        return Err(WebError::with_details("invalid_input", "one or more habit IDs are invalid.", details));
    }
    let current = iso_date(today());
    let completed: Dict = valid_ids.into_iter().map(|id| (id.clone(), json!(habits[&id]))).collect();
    let new_record = json!({"date": current, "completed": completed, "timestamp": py::utc_now_isoformat()});
    let mut records = load_habit_records();
    match records.iter().position(|record| field_is(record, "date", &current)) {
        Some(index) => records[index] = new_record.clone(),
        None => records.push(new_record.clone()),
    }
    save_habit_records_or_error(&records)?;
    Ok(new_record)
}

// =============================================================================
// History
// =============================================================================

/// The most recent finished tasks (newest first); `None` for all of them.
fn history_rows(count: Option<usize>) -> Vec<Value> {
    let log_entries = load_task_log();
    if log_entries.is_empty() {
        return Vec::new();
    }
    let activities = pair_task_activities(&log_entries);
    if activities.is_empty() {
        return Vec::new();
    }
    let skip = count.map_or(0, |count| activities.len().saturating_sub(count));
    let task_types = load_task_types();
    let default_type = default_task_type(&task_types);
    activities[skip..]
        .iter()
        .rev()
        .map(|activity| {
            let type_id =
                if py::truthy(&activity.task_type) { py::py_str(&activity.task_type) } else { default_type.clone() };
            let type_name = task_types.get(&type_id).cloned().unwrap_or_else(|| json!(type_id));
            json!({
                "description": activity.description,
                "priority": activity.priority,
                "startedAt": py::isoformat_naive(&activity.started_at),
                "endedAt": py::isoformat_naive(&activity.ended_at),
                "duration": format_duration(activity.started_at, activity.ended_at),
                "status": activity.status,
                "type": type_id,
                "typeName": type_name,
            })
        })
        .collect()
}

/// Finished tasks: `{"all": true}` for every one, else the latest `count` (1-50, default 10).
pub fn task_history(payload: &Dict) -> WebResult {
    if payload.get("all") == Some(&Value::Bool(true)) {
        return Ok(json!({"activities": history_rows(None)}));
    }
    let count = match payload.get("count") {
        None => 10,
        Some(value) => py::py_int(value).unwrap_or(10),
    };
    Ok(json!({"activities": history_rows(Some(count.clamp(1, 50) as usize))}))
}

// =============================================================================
// Task types
// =============================================================================

/// `{"taskTypes": {id: name}}`.
pub fn settings_get_task_types(_payload: &Dict) -> WebResult {
    Ok(json!({"taskTypes": load_task_types()}))
}

/// Replace every task type.
pub fn settings_set_task_types(payload: &Dict) -> WebResult {
    let Some(Value::Object(raw_types)) = payload.get("taskTypes") else {
        return Err(invalid("taskTypes must be an object."));
    };
    let mut validated = Dict::new();
    for (key, value) in raw_types {
        let key = py_strip(key).to_string();
        let name = stripped_or_empty(Some(value));
        if key.is_empty() {
            return Err(invalid("task type keys must be non-empty."));
        }
        if name.is_empty() {
            return Err(invalid(format!("task type '{key}' name must be non-empty.")));
        }
        validated.insert(key, json!(name));
    }
    save_task_types(&validated, None)?;
    Ok(json!({"taskTypes": validated}))
}

/// Add a task type, or rename the type with `id`. Naming an ID that only
/// existing tasks still use brings that type back under the new name.
pub fn task_type_save(payload: &Dict) -> WebResult {
    let name = require_text(payload, "name")?;
    if name.chars().count() > TASK_TYPE_NAME_LIMIT {
        return Err(invalid(format!("task type names can be at most {TASK_TYPE_NAME_LIMIT} characters.")));
    }
    let mut task_types = load_task_types();
    let mut type_id = stripped_or_empty(payload.get("id"));
    if type_id.is_empty() {
        type_id = next_task_type_id(&task_types);
    } else if !task_types.contains_key(&type_id)
        && !load_tasks()
            .iter()
            .any(|task| task.is_object() && py::py_str(task.get("type").unwrap_or(&Value::Null)) == type_id)
    {
        return Err(WebError::new("not_found", format!("task type not found: {type_id}")));
    }
    let folded = py::casefold(&name);
    for (key, value) in &task_types {
        if *key == type_id {
            continue;
        }
        let Value::String(existing) = value else {
            return Err(WebError::internal(format!("'{}' object has no attribute 'casefold'", py_type_name(value))));
        };
        if py::casefold(existing) == folded {
            return Err(WebError::new("duplicate", format!("a task type named “{name}” already exists.")));
        }
    }
    // Every color is saved as it shows now, so later changes to the list never
    // shift it. A new type gets the least-used color; a type named back from
    // its tasks keeps the color it already shows.
    let mut colors = shown_type_colors(&task_types);
    if !colors.contains_key(&type_id) {
        let color =
            if py::opt_truthy(payload.get("id")) { hashed_type_color(&type_id) } else { least_used_color(&colors) };
        colors.insert(type_id.clone(), json!(color));
    }
    task_types.insert(type_id.clone(), json!(name));
    save_task_types(&task_types, Some(&colors))?;
    Ok(json!({"id": type_id, "name": name, "taskTypes": task_types, "color": colors[&type_id]}))
}

/// Delete a task type and move what uses it: open tasks to `moveTo` (required
/// when there are any), deadlines there too or else uncategorized. Finished
/// tasks in the history keep the old type ID.
pub fn task_type_delete(payload: &Dict) -> WebResult {
    let type_id = require_text(payload, "id")?;
    let task_types = load_task_types();
    let default_type = default_task_type(&task_types);
    let mut tasks = load_tasks();
    // Tasks saved without a type belong to the default type.
    let owned: Vec<usize> = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| {
            obj(task)
                .is_some_and(|task| task.get("type").map(py::py_str).unwrap_or_else(|| default_type.clone()) == type_id)
        })
        .map(|(index, _)| index)
        .collect();
    if !task_types.contains_key(&type_id) && owned.is_empty() {
        return Err(WebError::new("not_found", format!("task type not found: {type_id}")));
    }
    let remaining: Dict =
        task_types.iter().filter(|(key, _)| **key != type_id).map(|(k, v)| (k.clone(), v.clone())).collect();
    if remaining.is_empty() {
        return Err(invalid("keep at least one task type."));
    }
    let move_to = stripped_or_empty(payload.get("moveTo"));
    if !move_to.is_empty() && !remaining.contains_key(&move_to) {
        return Err(invalid(format!("unknown task type: {move_to}")));
    }
    if !owned.is_empty() && move_to.is_empty() {
        return Err(invalid("choose a type for this type's tasks."));
    }

    for &index in &owned {
        if let Some(task) = tasks[index].as_object_mut() {
            task.insert("type".into(), json!(move_to));
        }
    }
    if !owned.is_empty() {
        save_tasks_or_error(&tasks)?;
    }
    let mut deadlines = load_deadlines();
    let mut moved_deadlines = 0;
    for item in deadlines.iter_mut().filter_map(Value::as_object_mut) {
        let matches = item.get("type").is_some_and(|t| !t.is_null() && py::py_str(t) == type_id);
        if matches {
            if move_to.is_empty() {
                item.shift_remove("type");
            } else {
                item.insert("type".into(), json!(move_to));
            }
            moved_deadlines += 1;
        }
    }
    if moved_deadlines > 0 {
        save_deadlines_or_error(&deadlines)?;
    }
    if task_types.contains_key(&type_id) {
        // The other types keep the colors they show now instead of moving up the palette.
        save_task_types(&remaining, Some(&shown_type_colors(&task_types)))?;
    }
    Ok(json!({"taskTypes": remaining, "movedTasks": owned.len(), "movedDeadlines": moved_deadlines}))
}
