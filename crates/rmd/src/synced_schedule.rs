//! Today's plan overlay (`synced_schedule.toml`), written by `rmd sync --apply`.
//!
//! The overlay maps today's work blocks to task titles on top of the base
//! odd/even schedule, so assignments change daily without touching templates.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use indexmap::IndexMap;
use serde_json::Value;

use crate::py::{self, Dict};

pub const SYNCABLE_BLOCKS: [&str; 2] = ["pomodoro", "potato"];

/// One synced assignment: the block and the task title for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assignment {
    pub block: String,
    pub title: String,
}

/// Accepted task assignments for a single calendar day.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncedDaySchedule {
    pub target_date: String,
    pub parity: String,
    pub weekday: String,
    pub assignments: IndexMap<String, Assignment>,
}

/// Path of the persisted overlay (`REMINDER_SYNCED_SCHEDULE_PATH` overrides).
pub fn resolve_synced_schedule_path() -> PathBuf {
    if let Ok(raw) = std::env::var("REMINDER_SYNCED_SCHEDULE_PATH")
        && !raw.is_empty()
    {
        return crate::paths::resolve(&crate::paths::expand_user(&raw));
    }
    crate::paths::resolve(&crate::paths::resolve_config_root_dir().join("synced_schedule.toml"))
}

fn normalize_assignment(event: &Value) -> Option<Assignment> {
    let map = event.as_object()?;
    let block = map.get("block")?.as_str()?;
    if block.trim().is_empty() {
        return None;
    }
    let title = match map.get("title") {
        Some(Value::String(t)) if !t.trim().is_empty() => t.as_str(),
        None => block,
        _ => block,
    };
    Some(Assignment { block: block.trim().to_string(), title: title.trim().to_string() })
}

/// Load the overlay if it exists and is valid.
pub fn load_synced_schedule(path: Option<&Path>) -> Option<SyncedDaySchedule> {
    let target = path.map(Path::to_path_buf).unwrap_or_else(resolve_synced_schedule_path);
    if !target.exists() {
        return None;
    }
    let raw = crate::config::load_toml_file(&target).ok()?;
    let field = |key: &str| raw.get(key).map(py::py_str).unwrap_or_default().trim().to_string();
    let target_date = field("date");
    let parity = field("parity").to_lowercase();
    let weekday = field("weekday").to_lowercase();
    let schedule = match raw.get("schedule") {
        Some(Value::Object(map)) => map.clone(),
        None => Dict::new(),
        _ => return None,
    };
    if target_date.is_empty() {
        return None;
    }
    let assignments =
        schedule.iter().filter_map(|(time, event)| normalize_assignment(event).map(|a| (time.clone(), a))).collect();
    Some(SyncedDaySchedule { target_date, parity, weekday, assignments })
}

/// Whether an overlay applies to the given day.
pub fn synced_schedule_matches_today(
    synced: Option<&SyncedDaySchedule>,
    target_date: Option<NaiveDate>,
    parity: Option<&str>,
    weekday: Option<&str>,
) -> bool {
    let Some(synced) = synced else {
        return false;
    };
    let actual = target_date.unwrap_or_else(crate::clock::today);
    let expected_weekday = weekday.map(str::to_string).unwrap_or_else(|| py::weekday_name(actual).to_string());
    if synced.target_date != actual.format("%Y-%m-%d").to_string() {
        return false;
    }
    if !synced.weekday.is_empty() && synced.weekday != expected_weekday {
        return false;
    }
    if let Some(parity) = parity.filter(|p| !p.is_empty())
        && !synced.parity.is_empty()
        && synced.parity != parity
    {
        return false;
    }
    true
}

/// Assignment as the schedule event dict `{block, title}`.
pub fn assignment_value(assignment: &Assignment) -> Value {
    serde_json::json!({"block": assignment.block, "title": assignment.title})
}

/// Overlay synced assignments onto a base daily schedule.
pub fn apply_synced_schedule(
    schedule: &Dict,
    target_date: Option<NaiveDate>,
    parity: Option<&str>,
    weekday: Option<&str>,
    synced: Option<&SyncedDaySchedule>,
) -> Dict {
    let loaded;
    let synced = match synced {
        Some(s) => Some(s),
        None => {
            loaded = load_synced_schedule(None);
            loaded.as_ref()
        }
    };
    let mut merged = schedule.clone();
    if !synced_schedule_matches_today(synced, target_date, parity, weekday) {
        return merged;
    }
    if let Some(plan) = synced {
        for (time, assignment) in &plan.assignments {
            merged.insert(time.clone(), assignment_value(assignment));
        }
    }
    merged
}

/// Block name for a schedule event, when it has one.
pub fn get_event_block_name(event: &Value) -> Option<String> {
    match event {
        Value::Object(map) if map.contains_key("block") => {
            map.get("block").and_then(Value::as_str).map(|b| b.trim().to_string())
        }
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// Whether the event has a meaningful custom title.
pub fn has_explicit_title(event: &Value) -> bool {
    let Value::Object(map) = event else {
        return false;
    };
    if !map.contains_key("block") {
        return false;
    }
    let (Some(block), Some(Value::String(title))) = (get_event_block_name(event), map.get("title")) else {
        return false;
    };
    let normalized_block = py::casefold(block.replace('_', " ").trim());
    let normalized_title = py::casefold(title.trim());
    !normalized_title.is_empty() && normalized_title != normalized_block
}

/// Label for status tables and previews.
pub fn format_event_label(event: &Value) -> String {
    if let Value::Object(map) = event
        && map.contains_key("block")
    {
        let block = py::get_str_or_empty(map, "block").trim().to_string();
        let title = py::get_str_or_empty(map, "title").trim().to_string();
        let normalized = block.replace('_', " ").trim().to_string();
        if !title.is_empty() && py::casefold(&title) != py::casefold(&normalized) {
            return format!("{normalized}: {title}");
        }
        return normalized;
    }
    py::py_str(event)
}

/// Untitled pomodoro/potato slots that sync can assign, sorted by time.
pub fn iter_syncable_slots(schedule: &Dict) -> Vec<(String, String)> {
    let mut times: Vec<&String> = schedule.keys().collect();
    times.sort();
    times
        .into_iter()
        .filter_map(|time| {
            let event = &schedule[time.as_str()];
            let block = get_event_block_name(event)?;
            if !SYNCABLE_BLOCKS.contains(&block.as_str()) || has_explicit_title(event) {
                return None;
            }
            Some((time.clone(), block))
        })
        .collect()
}

fn toml_string(value: &str) -> String {
    py::dumps(&Value::String(value.to_string()))
}

/// Serialize an overlay as TOML (same layout as the Python writer).
pub fn render_synced_schedule_file(plan: &SyncedDaySchedule) -> String {
    let mut lines = vec![
        "# Accepted task assignments for today's focus blocks.".to_string(),
        "# This file is written by `rmd sync --apply`.".to_string(),
        format!("date = {}", toml_string(&plan.target_date)),
        format!("parity = {}", toml_string(&plan.parity)),
        format!("weekday = {}", toml_string(&plan.weekday)),
        String::new(),
        "[schedule]".to_string(),
    ];
    let mut times: Vec<&String> = plan.assignments.keys().collect();
    times.sort();
    for time in times {
        let a = &plan.assignments[time.as_str()];
        lines.push(format!(
            "{} = {{ block = {}, title = {} }}",
            toml_string(time),
            toml_string(&a.block),
            toml_string(&a.title)
        ));
    }
    lines.join("\n") + "\n"
}

/// Persist the overlay and return its path.
pub fn save_synced_schedule(plan: &SyncedDaySchedule, path: Option<&Path>) -> std::io::Result<PathBuf> {
    let target = path.map(Path::to_path_buf).unwrap_or_else(resolve_synced_schedule_path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target, render_synced_schedule_file(plan))?;
    Ok(target)
}
