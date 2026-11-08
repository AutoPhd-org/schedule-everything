//! settings.toml data model shared by the settings TUI, the task commands and
//! the browser API: field metadata, default task types, and load/save.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::py::Dict;
use crate::toml_writer::{dump_toml, load_toml_raw};

pub const WEEKDAYS: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

/// Section headings shown in the settings editor.
pub const SECTION_LABELS: [(&str, &str); 7] = [
    ("settings", "⚙️  General Settings"),
    ("time_blocks", "⏱️  Time Blocks"),
    ("time_points", "🔔  Notifications"),
    ("tasks", "📋  Task Scheduling"),
    ("paths", "📁  File Paths"),
    ("task_types", "🏷️  Task Types"),
    ("task_type_colors", "🎨  Task Type Colors"),
];

/// Label for a section heading (the section name when unknown).
pub fn section_label(section: &str) -> String {
    SECTION_LABELS
        .iter()
        .find(|(s, _)| *s == section)
        .map(|(_, label)| label.to_string())
        .unwrap_or_else(|| section.to_string())
}

/// Default task types used when settings.toml has no `[task_types]`.
pub const DEFAULT_TASK_TYPES: [(&str, &str); 4] =
    [("1", "read papers"), ("2", "gym work"), ("3", "coding"), ("4", "other")];

/// [`DEFAULT_TASK_TYPES`] as an ordered dict.
pub fn default_task_types() -> Dict {
    DEFAULT_TASK_TYPES.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

/// Whether `value` is `H:MM`/`HH:MM` with a valid hour and minute.
pub fn valid_time(value: &str) -> bool {
    let value = value.trim();
    let Some((h, m)) = value.split_once(':') else {
        return false;
    };
    if h.is_empty()
        || h.len() > 2
        || m.len() != 2
        || !h.chars().all(|c| c.is_ascii_digit())
        || !m.chars().all(|c| c.is_ascii_digit())
    {
        return false;
    }
    h.parse::<u32>().is_ok_and(|h| h <= 23) && m.parse::<u32>().is_ok_and(|m| m <= 59)
}

/// How a setting is edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorType {
    Toggle,
    Picker,
    MultiSelect,
    Number,
    Text,
    Time,
    TimeList,
    WeekdayTime,
    DayTime,
}

/// Editor metadata for one setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldMeta {
    pub editor: EditorType,
    pub help_text: &'static str,
    pub choices: &'static [&'static str],
    pub min_val: Option<i64>,
    pub max_val: Option<i64>,
}

impl FieldMeta {
    const fn new(editor: EditorType, help_text: &'static str) -> Self {
        Self { editor, help_text, choices: &[], min_val: None, max_val: None }
    }

    const fn range(editor: EditorType, help_text: &'static str, min: i64, max: i64) -> Self {
        Self { editor, help_text, choices: &[], min_val: Some(min), max_val: Some(max) }
    }

    const fn choices(editor: EditorType, help_text: &'static str, choices: &'static [&'static str]) -> Self {
        Self { editor, help_text, choices, min_val: None, max_val: None }
    }
}

/// Explicit metadata for known setting keys.
pub const FIELD_REGISTRY: [((&str, &str), FieldMeta); 12] = [
    (("settings", "sound_file"), FieldMeta::new(EditorType::Text, "Path to notification sound file")),
    (("settings", "alarm_interval"), FieldMeta::range(EditorType::Number, "Seconds between alarm repeats", 1, 120)),
    (
        ("settings", "max_alarm_duration"),
        FieldMeta::range(EditorType::Number, "Max alarm duration in seconds", 10, 3600),
    ),
    (("settings", "skip_days"), FieldMeta::choices(EditorType::MultiSelect, "Days to skip all reminders", &WEEKDAYS)),
    (("settings", "language"), FieldMeta::choices(EditorType::Picker, "UI language", &["en", "zh"])),
    (("settings", "show_tasks_after_change"), FieldMeta::new(EditorType::Toggle, "Print task list after rmd add/rm")),
    (("tasks", "daily_urgent"), FieldMeta::new(EditorType::TimeList, "Times for urgent task reminders (HH:MM)")),
    (("tasks", "ddl_urgent"), FieldMeta::new(EditorType::TimeList, "Times for deadline reminders (HH:MM)")),
    (("tasks", "daily_summary"), FieldMeta::new(EditorType::Time, "Time for daily summary (HH:MM)")),
    (("tasks", "habit_prompt"), FieldMeta::new(EditorType::Time, "Time for habit tracking prompt (HH:MM)")),
    (("tasks", "weekly_review"), FieldMeta::new(EditorType::WeekdayTime, "Weekday and time for weekly review")),
    (("tasks", "monthly_review"), FieldMeta::new(EditorType::DayTime, "Day-of-month and time for monthly review")),
];

/// Fallback metadata for sections with user-defined keys.
pub const SECTION_FALLBACKS: [(&str, FieldMeta); 5] = [
    ("time_blocks", FieldMeta::range(EditorType::Number, "Duration in minutes", 1, 480)),
    ("time_points", FieldMeta::new(EditorType::Text, "Notification message")),
    ("paths", FieldMeta::new(EditorType::Text, "File or directory path")),
    ("task_types", FieldMeta::new(EditorType::Text, "Task type name")),
    (
        "task_type_colors",
        FieldMeta::new(EditorType::Text, "Color: blue, orange, aqua, yellow, magenta, green, violet, or red"),
    ),
];

/// Metadata for `[section] key`.
pub fn get_meta(section: &str, key: &str) -> FieldMeta {
    if let Some((_, meta)) = FIELD_REGISTRY.iter().find(|((s, k), _)| *s == section && *k == key) {
        return *meta;
    }
    SECTION_FALLBACKS
        .iter()
        .find(|(s, _)| *s == section)
        .map(|(_, meta)| *meta)
        .unwrap_or(FieldMeta::new(EditorType::Text, ""))
}

/// A UI row: a section header (`key == None`) or a key/value entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub section: String,
    pub key: Option<String>,
}

impl Row {
    pub fn is_header(&self) -> bool {
        self.key.is_none()
    }
}

/// Loads, mutates, and persists settings.toml.
#[derive(Clone, Debug)]
pub struct SettingsModel {
    pub path: PathBuf,
    pub data: Dict,
    pub dirty: bool,
}

impl SettingsModel {
    /// Load a settings file (adding default task types when missing).
    pub fn new(path: &Path) -> anyhow::Result<Self> {
        let mut model = Self { path: path.to_path_buf(), data: Dict::new(), dirty: false };
        model.load()?;
        Ok(model)
    }

    /// The active config set's settings.
    pub fn active() -> anyhow::Result<Self> {
        Self::new(&crate::paths::runtime().settings_path)
    }

    pub fn load(&mut self) -> anyhow::Result<()> {
        self.data = load_toml_raw(&self.path)?;
        if !self.data.contains_key("task_types") {
            self.data.insert("task_types".into(), Value::Object(default_task_types()));
        }
        self.dirty = false;
        Ok(())
    }

    pub fn save(&mut self) -> anyhow::Result<()> {
        dump_toml(&self.data, &self.path)?;
        self.dirty = false;
        Ok(())
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&Value> {
        self.data.get(section)?.as_object()?.get(key)
    }

    fn section_mut(&mut self, section: &str) -> &mut Dict {
        let entry = self.data.entry(section.to_string()).or_insert_with(|| Value::Object(Dict::new()));
        if !entry.is_object() {
            *entry = Value::Object(Dict::new());
        }
        entry.as_object_mut().expect("section is an object")
    }

    pub fn set(&mut self, section: &str, key: &str, value: Value) {
        let table = self.section_mut(section);
        if table.get(key) != Some(&value) {
            table.insert(key.to_string(), value);
            self.dirty = true;
        }
    }

    pub fn delete(&mut self, section: &str, key: &str) -> bool {
        let removed = self
            .data
            .get_mut(section)
            .and_then(Value::as_object_mut)
            .and_then(|table| table.shift_remove(key))
            .is_some();
        if removed {
            self.dirty = true;
        }
        removed
    }

    /// Add a key with a sensible default; `false` if it already exists.
    pub fn add_key(&mut self, section: &str, key: &str) -> bool {
        if self.get(section, key).is_some() {
            return false;
        }
        let meta = get_meta(section, key);
        let default = match meta.editor {
            EditorType::Number => Value::from(meta.min_val.filter(|v| *v != 0).unwrap_or(0)),
            EditorType::Toggle => Value::Bool(false),
            EditorType::TimeList | EditorType::MultiSelect => Value::Array(Vec::new()),
            _ => Value::String(String::new()),
        };
        self.section_mut(section).insert(key.to_string(), default);
        self.dirty = true;
        true
    }

    pub fn sections(&self) -> Vec<String> {
        self.data.keys().cloned().collect()
    }

    pub fn keys_in(&self, section: &str) -> Vec<String> {
        self.data.get(section).and_then(Value::as_object).map(|t| t.keys().cloned().collect()).unwrap_or_default()
    }
}
