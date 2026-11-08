//! Task activity history (`rmd history`).
//!
//! Pairs each `added` log entry with its closing action (`deleted` counts as
//! completion; `cancelled` and `dropped` do not).

use chrono::NaiveDateTime;
use serde_json::Value;

use crate::i18n::{t, tf};
use crate::py;

/// Parse a task-log timestamp as a naive local datetime (UTC values are
/// converted so day boundaries follow the user's calendar).
pub fn parse_log_timestamp(value: &Value) -> Option<NaiveDateTime> {
    let text = value.as_str()?;
    Some(py::parse_iso_datetime(text)?.to_local_naive())
}

/// A task from `added` to its closing action.
#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    pub description: String,
    /// `task.get("priority", 0)` from the `added` entry, as stored.
    pub priority: Value,
    pub started_at: NaiveDateTime,
    pub ended_at: NaiveDateTime,
    /// `"completed"`, `"cancelled"`, or `"dropped"`.
    pub status: String,
    /// Task type id (or `Value::Null` for legacy entries).
    pub task_type: Value,
}

struct OpenAdd {
    started_at: NaiveDateTime,
    priority: Value,
    task_type: Value,
}

fn description_of(task: &py::Dict) -> String {
    match task.get("description") {
        Some(Value::String(s)) => s.clone(),
        Some(v) if py::truthy(v) => py::py_str(v),
        _ => String::new(),
    }
}

/// Pair `added` entries with their closing actions, sorted by end time.
pub fn pair_task_activities(log_entries: &[Value]) -> Vec<Activity> {
    let mut open_adds: indexmap::IndexMap<String, Vec<OpenAdd>> = indexmap::IndexMap::new();
    let mut activities: Vec<Activity> = Vec::new();

    let mut sorted: Vec<&Value> = log_entries.iter().collect();
    sorted.sort_by(|a, b| {
        let ka = a.get("timestamp").and_then(Value::as_str).unwrap_or("");
        let kb = b.get("timestamp").and_then(Value::as_str).unwrap_or("");
        ka.cmp(kb)
    });

    let empty = py::Dict::new();
    for entry in sorted {
        let action = entry.get("action").and_then(Value::as_str).unwrap_or("");
        let task = match entry.get("task") {
            Some(Value::Object(map)) => map,
            Some(_) => continue,
            None => &empty,
        };
        let description = description_of(task);
        if description.is_empty() {
            continue;
        }
        let Some(ts) = entry.get("timestamp").and_then(parse_log_timestamp) else {
            continue;
        };
        match action {
            "added" => {
                open_adds.entry(description).or_default().push(OpenAdd {
                    started_at: ts,
                    priority: task.get("priority").cloned().unwrap_or(Value::from(0)),
                    task_type: task.get("type").cloned().unwrap_or(Value::Null),
                });
            }
            "updated" => {
                let old_description = entry
                    .get("metadata")
                    .and_then(Value::as_object)
                    .and_then(|m| m.get("old_task"))
                    .and_then(Value::as_object)
                    .and_then(|old| old.get("description"))
                    .filter(|d| py::truthy(d))
                    .map(py::py_str);
                if let Some(old) = old_description
                    && old != description
                {
                    let moved = open_adds.shift_remove(&old).unwrap_or_default();
                    open_adds.entry(description.clone()).or_default().extend(moved);
                }
                if let Some(new_type) = task.get("type").filter(|v| !v.is_null()) {
                    for record in open_adds.entry(description).or_default() {
                        record.task_type = new_type.clone();
                    }
                }
            }
            "deleted" | "cancelled" | "dropped" => {
                let records = open_adds.entry(description.clone()).or_default();
                if !records.is_empty() {
                    let add = records.remove(0);
                    activities.push(Activity {
                        description,
                        priority: add.priority,
                        started_at: add.started_at,
                        ended_at: ts,
                        status: if action == "deleted" { "completed".to_string() } else { action.to_string() },
                        task_type: add.task_type,
                    });
                }
            }
            _ => {}
        }
    }
    activities.sort_by_key(|a| a.ended_at);
    activities
}

/// Human-readable duration such as `"1d 2h 5m"` (or `"< 1 min"`).
pub fn format_duration(started: NaiveDateTime, ended: NaiveDateTime) -> String {
    let total_seconds = (ended - started).num_microseconds().map(|us| us as f64 / 1e6).unwrap_or(0.0);
    let total_minutes = (total_seconds / 60.0).floor() as i64;
    if total_minutes < 1 {
        return t("< 1 min");
    }
    let (hours, minutes) = (total_minutes / 60, total_minutes % 60);
    let (days, hours) = (hours / 24, hours % 24);
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(tf("{days}d", &[("days", &days)]));
    }
    if hours > 0 {
        parts.push(tf("{hours}h", &[("hours", &hours)]));
    }
    if minutes > 0 {
        parts.push(tf("{minutes}m", &[("minutes", &minutes)]));
    }
    parts.join(" ")
}

// --- CLI command (`rmd history [count]`) is added below. ---
