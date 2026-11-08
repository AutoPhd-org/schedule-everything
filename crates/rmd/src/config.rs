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
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| TomlLoadError::Parse(clean_toml_error(&e)))?;
    Ok(table_to_dict(table))
}

/// One-line rendering of a TOML parse error.
pub fn clean_toml_error(error: &toml::de::Error) -> String {
    let message = error.message().trim().to_string();
    match error.span() {
        Some(span) => format!("{message} (at byte {})", span.start),
        None => message,
    }
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

/// Main configuration from `settings.toml`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScheduleConfig {
    pub settings: Dict,
    pub time_blocks: Dict,
    pub time_points: Dict,
    pub tasks: Dict,
    pub paths: Dict,
    pub task_types: Dict,
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
        }
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
        let weekday = py::weekday_name(date);
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
