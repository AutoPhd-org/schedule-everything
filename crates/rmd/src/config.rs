//! `settings.toml` and the odd/even week schedules.
//!
//! TOML documents are converted to ordered JSON values so the rest of the
//! port can treat them the way the Python code treated `tomllib` dicts.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::py::{self, Dict};

const DEFAULT_SOUND_MACOS: &str = "/System/Library/Sounds/Ping.aiff";
const DEFAULT_SOUND_LINUX: &str = "/usr/share/sounds/freedesktop/stereo/complete.oga";

/// Errors from loading a TOML file, worded like the Python exceptions.
#[derive(Debug, thiserror::Error)]
pub enum TomlLoadError {
    #[error("[Errno 2] No such file or directory: '{}'", .0.display())]
    NotFound(PathBuf),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Parse(String),
}

/// Convert a parsed TOML value into an ordered JSON value.
pub fn toml_to_json(value: toml::Value) -> Value {
    match value {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::from(i),
        toml::Value::Float(f) => serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(dt) => Value::String(dt.to_string()),
        toml::Value::Array(items) => Value::Array(items.into_iter().map(toml_to_json).collect()),
        toml::Value::Table(table) => Value::Object(table_to_dict(table)),
    }
}

/// Convert a TOML table into an ordered JSON object.
pub fn table_to_dict(table: toml::Table) -> Dict {
    table.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect()
}

/// Parse TOML text into an ordered JSON object.
pub fn parse_toml_str(text: &str) -> Result<Dict, TomlLoadError> {
    let table: toml::Table =
        text.parse().map_err(|e: toml::de::Error| TomlLoadError::Parse(describe_toml_error(&e, text)))?;
    Ok(table_to_dict(table))
}

/// One-line description of a TOML parse error in `tomllib`'s style:
/// `"Invalid table header (at line 1, column 10)"`.
pub fn describe_toml_error(error: &toml::de::Error, text: &str) -> String {
    let message = error.message().lines().next().unwrap_or("Invalid TOML").trim().trim_end_matches('.');
    let mut chars = message.chars();
    let message = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Invalid TOML".to_string(),
    };
    match error.span() {
        Some(span) => {
            let offset = span.start.min(text.len());
            let before = &text[..offset];
            let line = before.matches('\n').count() + 1;
            let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
            format!("{message} (at line {line}, column {column})")
        }
        None => message,
    }
}

/// One-line rendering of a TOML parse error, without its position.
pub fn clean_toml_error(error: &toml::de::Error) -> String {
    error.message().lines().next().unwrap_or("Invalid TOML").trim().to_string()
}

/// Load and parse a TOML file (`tomllib.load`).
pub fn load_toml_file(path: &Path) -> Result<Dict, TomlLoadError> {
    let bytes = std::fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            TomlLoadError::NotFound(path.to_path_buf())
        } else {
            TomlLoadError::Io(e)
        }
    })?;
    let text = String::from_utf8(bytes).map_err(|e| TomlLoadError::Parse(format!("invalid UTF-8: {e}")))?;
    parse_toml_str(&text)
}

fn section(config: &Dict, name: &str) -> Dict {
    match config.get(name) {
        Some(Value::Object(map)) => map.clone(),
        _ => Dict::new(),
    }
}

fn default_sound_file() -> &'static str {
    if crate::platform::get_platform() == "macos" { DEFAULT_SOUND_MACOS } else { DEFAULT_SOUND_LINUX }
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items.iter().map(py::py_str).collect(),
        Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// Each kind of reminder the service gives, by its switch in `[reminders]`.
/// Every kind is on unless its switch is `false`; when the timed ones ring is
/// set in `[tasks]` (see [`ReminderKind::time_key`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReminderKind {
    /// A time block from the week schedule starts.
    BlockStart,
    /// A time block from the week schedule ends.
    BlockEnd,
    /// A fixed message from the week schedule (a `[time_points]` name or plain text).
    TimePoints,
    /// The Yes/No prompt about important and procrastinated tasks.
    UrgentTasks,
    /// The alarm for deadlines that are close.
    Deadlines,
    /// The daily summary popup.
    DailySummary,
    /// The habit check-in popup.
    HabitPrompt,
    /// The weekly report.
    WeeklyReport,
    /// The monthly report.
    MonthlyReport,
}

impl ReminderKind {
    pub const ALL: [ReminderKind; 9] = [
        Self::BlockStart,
        Self::BlockEnd,
        Self::TimePoints,
        Self::UrgentTasks,
        Self::Deadlines,
        Self::DailySummary,
        Self::HabitPrompt,
        Self::WeeklyReport,
        Self::MonthlyReport,
    ];

    /// Its switch in `[reminders]`.
    pub fn key(self) -> &'static str {
        match self {
            Self::BlockStart => "block_start",
            Self::BlockEnd => "block_end",
            Self::TimePoints => "time_points",
            Self::UrgentTasks => "urgent_tasks",
            Self::Deadlines => "deadlines",
            Self::DailySummary => "daily_summary",
            Self::HabitPrompt => "habit_prompt",
            Self::WeeklyReport => "weekly_report",
            Self::MonthlyReport => "monthly_report",
        }
    }

    /// The `[tasks]` key holding when it rings (`None` for the kinds the week
    /// schedule times).
    pub fn time_key(self) -> Option<&'static str> {
        match self {
            Self::BlockStart | Self::BlockEnd | Self::TimePoints => None,
            Self::UrgentTasks => Some("daily_urgent"),
            Self::Deadlines => Some("ddl_urgent"),
            Self::DailySummary => Some("daily_summary"),
            Self::HabitPrompt => Some("habit_prompt"),
            Self::WeeklyReport => Some("weekly_review"),
            Self::MonthlyReport => Some("monthly_review"),
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }
}

/// `[reminders] urgent_importance` when unset: tasks of importance 8 or more.
pub const DEFAULT_URGENT_IMPORTANCE: i64 = 8;
/// `[reminders] deadline_days` when unset: deadlines three days away or closer.
pub const DEFAULT_DEADLINE_DAYS: i64 = 3;
/// The longest `[reminders] deadline_days` the service honors.
pub const MAX_DEADLINE_DAYS: i64 = 60;

/// Main configuration from `settings.toml`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScheduleConfig {
    pub settings: Dict,
    pub time_blocks: Dict,
    pub time_points: Dict,
    pub tasks: Dict,
    pub paths: Dict,
    pub task_types: Dict,
    /// Which reminders ring (`[reminders]`).
    pub reminders: Dict,
}

impl ScheduleConfig {
    /// Load from a settings file.
    pub fn load(settings_path: &Path) -> Result<Self, TomlLoadError> {
        Ok(Self::from_dict(&load_toml_file(settings_path)?))
    }

    /// Load the active config set's settings.
    pub fn load_active() -> Result<Self, TomlLoadError> {
        Self::load(&crate::paths::runtime().settings_path)
    }

    /// Build from an already parsed document.
    pub fn from_dict(config: &Dict) -> Self {
        Self {
            settings: section(config, "settings"),
            time_blocks: section(config, "time_blocks"),
            time_points: section(config, "time_points"),
            tasks: section(config, "tasks"),
            paths: section(config, "paths"),
            task_types: section(config, "task_types"),
            reminders: section(config, "reminders"),
        }
    }

    /// Whether reminders of `kind` are switched on (anything but `false` is on).
    pub fn reminder_on(&self, kind: ReminderKind) -> bool {
        !matches!(self.reminders.get(kind.key()), Some(Value::Bool(false)))
    }

    /// Time block names that ring no alarm (`[reminders] muted_blocks`).
    pub fn muted_blocks(&self) -> Vec<String> {
        string_list(self.reminders.get("muted_blocks"))
    }

    /// Time point names, or plain schedule messages, that ring no alarm
    /// (`[reminders] muted_points`).
    pub fn muted_points(&self) -> Vec<String> {
        string_list(self.reminders.get("muted_points"))
    }

    /// Whether a time block of type `block` rings.
    pub fn block_rings(&self, block: &str) -> bool {
        !self.muted_blocks().iter().any(|name| name == block)
    }

    /// Whether the schedule label `label` (a time point name or plain message)
    /// rings; `message` is what it says.
    pub fn point_rings(&self, label: &str, message: &str) -> bool {
        !self.muted_points().iter().any(|name| name == label || name == message)
    }

    /// The least importance (1-10) a task needs for the urgent-task prompt to
    /// ask about it (`[reminders] urgent_importance`, default 8).
    pub fn urgent_importance(&self) -> i64 {
        self.reminders
            .get("urgent_importance")
            .and_then(py::py_int)
            .map_or(DEFAULT_URGENT_IMPORTANCE, |n| n.clamp(1, 10))
    }

    /// How many days ahead a deadline starts ringing (`[reminders]
    /// deadline_days`, default 3; 0 is the day it is due).
    pub fn deadline_days(&self) -> i64 {
        self.reminders
            .get("deadline_days")
            .and_then(py::py_int)
            .map_or(DEFAULT_DEADLINE_DAYS, |n| n.clamp(0, MAX_DEADLINE_DAYS))
    }

    /// Notification sound, falling back to a platform default.
    pub fn sound_file(&self) -> String {
        self.settings.get("sound_file").map(py::py_str).unwrap_or_else(|| default_sound_file().to_string())
    }

    /// Seconds between alarm repetitions (default 5).
    pub fn alarm_interval(&self) -> f64 {
        self.settings.get("alarm_interval").and_then(py::py_float).unwrap_or(5.0)
    }

    /// Maximum seconds an alarm repeats (default 300).
    pub fn max_alarm_duration(&self) -> f64 {
        self.settings.get("max_alarm_duration").and_then(py::py_float).unwrap_or(300.0)
    }

    /// Whether `rmd add`/`rmd rm` print the task list afterwards.
    pub fn show_tasks_after_change(&self) -> bool {
        matches!(self.settings.get("show_tasks_after_change"), Some(Value::Bool(true)))
    }

    /// Whether today is one of the configured `skip_days`.
    pub fn should_skip_today(&self) -> bool {
        self.should_skip_date(crate::clock::today())
    }

    /// Whether `date` falls on one of the configured `skip_days`.
    pub fn should_skip_date(&self, date: chrono::NaiveDate) -> bool {
        self.skips_weekday(py::weekday_name(date))
    }

    /// Whether `weekday` (`"monday"`, ...) is one of the configured `skip_days`.
    pub fn skips_weekday(&self, weekday: &str) -> bool {
        match self.settings.get("skip_days") {
            Some(Value::Array(days)) => days.iter().any(|d| d.as_str() == Some(weekday)),
            Some(Value::String(days)) => !days.is_empty() && days.contains(weekday),
            _ => false,
        }
    }

    fn task_setting(&self, key: &str) -> Option<&Value> {
        self.tasks.get(key)
    }

    /// Daily summary popup time (default "22:00").
    pub fn daily_summary_time(&self) -> String {
        self.task_setting("daily_summary").map(py::py_str).unwrap_or_else(|| "22:00".to_string())
    }

    /// Weekly review schedule such as "sunday 20:00" (or empty).
    pub fn weekly_review_time(&self) -> String {
        self.task_setting("weekly_review").map(py::py_str).unwrap_or_default()
    }

    /// Monthly review schedule such as "1 20:00" (or empty).
    pub fn monthly_review_time(&self) -> String {
        self.task_setting("monthly_review").map(py::py_str).unwrap_or_default()
    }

    /// Times for urgent-task reminders.
    pub fn daily_urgent_times(&self) -> Vec<String> {
        string_list(self.task_setting("daily_urgency").or_else(|| self.task_setting("daily_urgent")))
    }

    /// Times for deadline reminders.
    pub fn ddl_urgent_times(&self) -> Vec<String> {
        string_list(self.task_setting("ddl_urgency").or_else(|| self.task_setting("ddl_urgent")))
    }

    /// Habit prompt time (or empty).
    pub fn habit_prompt_time(&self) -> String {
        self.task_setting("habit_prompt")
            .or_else(|| self.task_setting("habit_tracking"))
            .map(py::py_str)
            .unwrap_or_default()
    }

    /// Duration in minutes of a named time block.
    pub fn block_minutes(&self, block: &str) -> Option<i64> {
        self.time_blocks.get(block).and_then(py::py_int)
    }

    /// Task types as `{id: name}` strings.
    pub fn task_type_names(&self) -> indexmap::IndexMap<String, String> {
        self.task_types.iter().map(|(k, v)| (k.clone(), py::py_str(v))).collect()
    }

    /// Legacy `[paths] config_dir`.
    pub fn config_dir(&self) -> String {
        self.paths.get("config_dir").map(py::py_str).unwrap_or_else(|| "config".to_string())
    }

    /// Legacy `[paths] tasks_path`.
    pub fn tasks_path(&self) -> String {
        self.paths.get("tasks_path").map(py::py_str).unwrap_or_else(|| "tasks.json".to_string())
    }

    /// Legacy `[paths] log_path`, with `~` expanded.
    pub fn log_path(&self) -> String {
        let raw = self
            .paths
            .get("log_path")
            .map(py::py_str)
            .unwrap_or_else(|| "~/.schedule_management/task/tasks.log".to_string());
        if raw.contains('~') || raw.contains("$HOME") {
            crate::paths::expand_user(&raw).display().to_string()
        } else {
            raw
        }
    }

    /// Legacy `[paths] record_path`.
    pub fn record_path(&self) -> String {
        self.paths.get("record_path").map(py::py_str).unwrap_or_else(|| "task/record.json".to_string())
    }
}

/// The odd- and even-week schedules.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WeeklySchedule {
    pub odd_data: Dict,
    pub even_data: Dict,
}

impl WeeklySchedule {
    /// Load both schedule files.
    pub fn load(odd_path: &Path, even_path: &Path) -> Result<Self, TomlLoadError> {
        Ok(Self { odd_data: load_toml_file(odd_path)?, even_data: load_toml_file(even_path)? })
    }

    /// Load the active config set's schedules.
    pub fn load_active() -> Result<Self, TomlLoadError> {
        let paths = crate::paths::runtime();
        Self::load(&paths.odd_path, &paths.even_path)
    }

    /// Full schedule document for `"odd"` or `"even"`.
    pub fn get_schedule_for_parity(&self, parity: &str) -> &Dict {
        if parity == "odd" { &self.odd_data } else { &self.even_data }
    }

    /// `common` merged with the given weekday's section.
    pub fn schedule_for_day(&self, parity: &str, weekday: &str) -> Dict {
        let data = self.get_schedule_for_parity(parity);
        let mut merged = section(data, "common");
        for (k, v) in section(data, weekday) {
            merged.insert(k, v);
        }
        merged
    }

    /// Today's merged schedule, or empty on a skip day.
    pub fn get_today_schedule(&self, config: &ScheduleConfig) -> Dict {
        if config.should_skip_today() {
            return Dict::new();
        }
        let today = crate::clock::today();
        self.schedule_for_day(&crate::time_utils::get_week_parity(), py::weekday_name(today))
    }
}
