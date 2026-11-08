//! Loading and saving the JSON/TOML data files (tasks, task log, deadlines,
//! habits, procrastination records, and the j/p mode).
//!
//! Records stay as ordered JSON values, exactly as the Python code kept them
//! as dicts, so unknown fields survive a load/save round trip.

use std::path::Path;

use chrono::NaiveDate;
use serde_json::Value;

use crate::paths;
use crate::py::{self, Dict};

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Items of a JSON list file, or `None` if missing/invalid/not a list.
fn read_json_list(path: &Path) -> Option<Vec<Value>> {
    match read_json(path)? {
        Value::Array(items) => Some(items),
        _ => None,
    }
}

// =============================================================================
// Tasks
// =============================================================================

/// Load tasks (a list of objects). Missing or invalid files give an empty list.
pub fn load_tasks() -> Vec<Value> {
    load_tasks_from(&paths::runtime().tasks_path)
}

/// Load tasks from an explicit path.
pub fn load_tasks_from(path: &Path) -> Vec<Value> {
    match read_json(path) {
        Some(Value::Array(items)) => items,
        Some(_) => Vec::new(),
        None => {
            eprintln!("Error loading tasks file, starting with an empty task list.");
            Vec::new()
        }
    }
}

/// Save tasks with Python's `indent=2` layout.
pub fn save_tasks(tasks: &[Value]) -> std::io::Result<()> {
    py::write_json_pretty(&paths::runtime().tasks_path, &tasks)
}

// =============================================================================
// Procrastination records
// =============================================================================

/// `{description: {"description": ..., "since"?: ...}}` from procrastinate.json.
pub fn load_procrastinate_records() -> indexmap::IndexMap<String, Dict> {
    let mut records = indexmap::IndexMap::new();
    let Some(items) = read_json_list(&paths::runtime().procrastinate_path) else {
        return records;
    };
    for item in items {
        let mut description = String::new();
        let mut since = None;
        match &item {
            Value::String(s) => description = s.trim().to_string(),
            Value::Object(map) => {
                if let Some(Value::String(d)) = map.get("description") {
                    description = d.trim().to_string();
                }
                if let Some(Value::String(s)) = map.get("since") {
                    let s = s.trim();
                    if !s.is_empty() && py::parse_iso_date(s).is_some() {
                        since = Some(s.to_string());
                    }
                }
            }
            _ => {}
        }
        if description.is_empty() {
            continue;
        }
        let mut record = Dict::new();
        record.insert("description".into(), Value::String(description.clone()));
        if let Some(since) = since {
            record.insert("since".into(), Value::String(since));
        }
        records.insert(description, record);
    }
    records
}

/// Descriptions currently marked as procrastinated.
pub fn load_procrastinate_list() -> std::collections::BTreeSet<String> {
    load_procrastinate_records().into_keys().collect()
}

/// Save procrastinated descriptions, keeping each record's first `since` date.
pub fn save_procrastinate_list<I, S>(descriptions: I, today: Option<NaiveDate>) -> std::io::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let existing = load_procrastinate_records();
    let today_str = today.unwrap_or_else(crate::clock::today).format("%Y-%m-%d").to_string();
    let normalized: std::collections::BTreeSet<String> =
        descriptions.into_iter().map(|d| d.as_ref().trim().to_string()).filter(|d| !d.is_empty()).collect();
    let serialized: Vec<Value> = normalized
        .into_iter()
        .map(|description| {
            let since = existing
                .get(&description)
                .and_then(|r| r.get("since"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| today_str.clone());
            let mut record = Dict::new();
            record.insert("description".into(), Value::String(description));
            if !since.is_empty() {
                record.insert("since".into(), Value::String(since));
            }
            Value::Object(record)
        })
        .collect();
    py::write_json_pretty(&paths::runtime().procrastinate_path, &serialized)
}

/// Whole days since a task was first deferred (`None` if unknown).
pub fn get_procrastinate_age_days(since: Option<&str>, today: Option<NaiveDate>) -> Option<i64> {
    let since = since.filter(|s| !s.is_empty())?;
    let since_date = py::parse_iso_date(since)?;
    let current = today.unwrap_or_else(crate::clock::today);
    Some((current - since_date).num_days().max(0))
}

// =============================================================================
// Task log
// =============================================================================

/// Load the task action log (missing/invalid gives an empty list).
pub fn load_task_log() -> Vec<Value> {
    match read_json(&paths::runtime().task_log_path) {
        Some(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

/// Save the task action log.
pub fn save_task_log(entries: &[Value]) -> std::io::Result<()> {
    py::write_json_pretty(&paths::runtime().task_log_path, &entries)
}

/// Append an action (`added`, `updated`, `deleted`, ...) with a UTC timestamp.
pub fn log_task_action(action: &str, task: &Value, metadata: Option<&Value>) -> std::io::Result<Value> {
    let mut entry = Dict::new();
    entry.insert("timestamp".into(), Value::String(py::utc_now_isoformat()));
    entry.insert("action".into(), Value::String(action.to_string()));
    entry.insert("task".into(), task.clone());
    if let Some(meta) = metadata.filter(|m| py::truthy(m)) {
        entry.insert("metadata".into(), meta.clone());
    }
    let entry = Value::Object(entry);
    let mut entries = load_task_log();
    entries.push(entry.clone());
    save_task_log(&entries)?;
    Ok(entry)
}

// =============================================================================
// Deadlines
// =============================================================================

/// Load deadlines (missing/invalid gives an empty list).
pub fn load_deadlines() -> Vec<Value> {
    match read_json(&paths::runtime().ddl_path) {
        Some(Value::Array(items)) => items,
        Some(_) => Vec::new(),
        None => {
            eprintln!("Error loading deadlines file, starting with an empty deadline list.");
            Vec::new()
        }
    }
}

/// Save deadlines.
pub fn save_deadlines(deadlines: &[Value]) -> std::io::Result<()> {
    py::write_json_pretty(&paths::runtime().ddl_path, &deadlines)
}

// =============================================================================
// Habits
// =============================================================================

/// Habits as `{id: description}`; accepts `{name = id}` and `{id = name}`,
/// optionally nested under `[habits]`.
pub fn load_habits() -> indexmap::IndexMap<String, String> {
    load_habits_from(&paths::runtime().habit_path)
}

/// Habits from an explicit file.
pub fn load_habits_from(path: &Path) -> indexmap::IndexMap<String, String> {
    let data = match crate::config::load_toml_file(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error loading habits file: {e}");
            return indexmap::IndexMap::new();
        }
    };
    habits_from_dict(&data)
}

/// Normalise a parsed habits document.
pub fn habits_from_dict(data: &Dict) -> indexmap::IndexMap<String, String> {
    let habits_data = match data.get("habits") {
        Some(Value::Object(map)) => map,
        Some(_) => {
            eprintln!("Error loading habits file: 'habits' is not a table");
            return indexmap::IndexMap::new();
        }
        None => data,
    };
    let mut habits = indexmap::IndexMap::new();
    for (key, value) in habits_data {
        match value {
            // `isinstance(value, int)` is also true for booleans in Python.
            Value::Number(n) if n.is_i64() || n.is_u64() => {
                habits.insert(py::py_str(value), key.clone());
            }
            Value::Bool(_) => {
                habits.insert(py::py_str(value), key.clone());
            }
            other => {
                habits.insert(key.clone(), py::py_str(other));
            }
        }
    }
    habits
}

/// Load habit records (missing/invalid gives an empty list).
pub fn load_habit_records() -> Vec<Value> {
    read_json_list(&paths::runtime().record_path).unwrap_or_default()
}

/// Save habit records.
pub fn save_habit_records(records: &[Value]) -> std::io::Result<()> {
    py::write_json_pretty(&paths::runtime().record_path, &records)
}

// =============================================================================
// Mode
// =============================================================================

/// Current mode, `"j"` (default) or `"p"`.
pub fn load_mode() -> String {
    match std::fs::read_to_string(paths::runtime().mode_path) {
        Ok(text) if text.trim().to_lowercase() == "p" => "p".to_string(),
        _ => "j".to_string(),
    }
}

/// Persist the mode (`"j"` or `"p"`).
pub fn save_mode(mode: &str) -> anyhow::Result<()> {
    if mode != "j" && mode != "p" {
        anyhow::bail!("Mode must be 'j' or 'p'");
    }
    let path = paths::runtime().mode_path;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, mode)?;
    Ok(())
}
