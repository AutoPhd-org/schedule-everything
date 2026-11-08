//! Today's focus-block plan (`rmd sync`).
//!
//! Planning happens in the user's AI agent (the Schedule Everything plugin).
//! This module provides the deterministic parts the agent and the browser use:
//! today's assignable focus slots and open tasks as JSON, validating and saving
//! a plan as the synced overlay, and clearing it. The overlay changes only
//! today; the weekly routine files stay untouched.

use chrono::NaiveDate;
use indexmap::IndexMap;
use serde_json::{Value, json};

use crate::commands::deadlines::refresh_deadlines;
use crate::config::{ScheduleConfig, TomlLoadError, WeeklySchedule};
use crate::data::{get_procrastinate_age_days, load_deadlines, load_mode, load_procrastinate_records, load_tasks};
use crate::py::{self, Dict};
use crate::synced_schedule::{
    Assignment, SyncedDaySchedule, apply_synced_schedule, format_event_label, get_event_block_name,
    iter_syncable_slots, load_synced_schedule, resolve_synced_schedule_path, save_synced_schedule,
    synced_schedule_matches_today,
};
use crate::time_utils::get_week_parity;

pub const MAX_TITLE_LENGTH: usize = 120;
pub const P_MODE_MESSAGE: &str = "Currently in p mode. Switch back to j mode to execute this command.";

/// A plan that cannot be applied, with a user-facing message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct SyncPlanError(pub String);

/// Errors from sync operations.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Plan(#[from] SyncPlanError),
    #[error(transparent)]
    Load(#[from] TomlLoadError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn plan_error(message: impl Into<String>) -> SyncError {
    SyncError::Plan(SyncPlanError(message.into()))
}

/// Base schedule for today (without an existing overlay).
pub struct BaseSchedule {
    pub schedule: Dict,
    pub parity: String,
    pub is_skipped: bool,
    pub config: ScheduleConfig,
}

fn base_today_schedule() -> Result<BaseSchedule, TomlLoadError> {
    let paths = crate::paths::runtime();
    let config = ScheduleConfig::load(&paths.settings_path)?;
    let weekly = WeeklySchedule::load(&paths.odd_path, &paths.even_path)?;
    let is_skipped = config.should_skip_today();
    let parity = get_week_parity();
    let schedule = if is_skipped { Dict::new() } else { weekly.get_today_schedule(&config) };
    Ok(BaseSchedule { schedule, parity, is_skipped, config })
}

fn as_int(value: Option<&Value>) -> i64 {
    value.and_then(py::py_int).unwrap_or(0)
}

fn open_tasks(today: NaiveDate, task_types: &Dict) -> Vec<Value> {
    let records = load_procrastinate_records();
    let today_iso = today.format("%Y-%m-%d").to_string();
    let mut rows: Vec<(i64, Value)> = Vec::new();
    for task in load_tasks() {
        let Value::Object(task) = task else { continue };
        let Some(Value::String(description)) = task.get("description") else { continue };
        if description.trim().is_empty() {
            continue;
        }
        let alarm_from = match task.get("alarm_from") {
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        };
        let type_id = task.get("type").filter(|v| !v.is_null()).map(py::py_str);
        let record = records.get(description.as_str());
        let priority = as_int(task.get("priority"));
        let type_name = match &type_id {
            Some(id) if !id.is_empty() => task_types.get(id).cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        };
        let postponed = alarm_from.as_deref().is_some_and(|a| !a.is_empty() && a > today_iso.as_str());
        let procrastinate_days = record.map(|r| {
            let since = r.get("since").and_then(Value::as_str);
            get_procrastinate_age_days(since, Some(today)).map(Value::from).unwrap_or(Value::Null)
        });
        rows.push((
            priority,
            json!({
                "description": description.trim(),
                "priority": priority,
                "type": type_id,
                "typeName": type_name,
                "alarmFrom": alarm_from,
                "postponed": postponed,
                "procrastinated": record.is_some(),
                "procrastinateDays": procrastinate_days.unwrap_or(Value::Null),
            }),
        ));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    rows.into_iter().map(|(_, row)| row).collect()
}

fn upcoming_deadlines(today: NaiveDate) -> Vec<Value> {
    let mut rows: Vec<(i64, Value)> = Vec::new();
    for item in refresh_deadlines(&load_deadlines(), Some(today)).kept {
        let deadline = item.get("deadline").map(py::py_str).unwrap_or_default();
        let Some(due) = py::parse_iso_date(&deadline) else { continue };
        let days_left = (due - today).num_days();
        if days_left >= 0 {
            let event = item.get("event").map(py::py_str).unwrap_or_default();
            rows.push((
                days_left,
                json!({"event": event, "deadline": due.format("%Y-%m-%d").to_string(), "daysLeft": days_left}),
            ));
        }
    }
    rows.sort_by_key(|(days, _)| *days);
    rows.into_iter().map(|(_, row)| row).collect()
}

fn todays_plan(today: NaiveDate, parity: &str, weekday: &str) -> Option<SyncedDaySchedule> {
    let synced = load_synced_schedule(None);
    if synced_schedule_matches_today(synced.as_ref(), Some(today), Some(parity), Some(weekday)) { synced } else { None }
}

/// Today's focus slots, schedule, open tasks, deadlines, and current plan.
pub fn sync_context() -> Result<Value, TomlLoadError> {
    let today = crate::clock::today();
    let weekday = py::weekday_name(today);
    let base = base_today_schedule()?;
    let plan = todays_plan(today, &base.parity, weekday);
    let empty = IndexMap::new();
    let planned: &IndexMap<String, Assignment> = plan.as_ref().map(|p| &p.assignments).unwrap_or(&empty);
    let merged = match &plan {
        Some(p) => apply_synced_schedule(&base.schedule, Some(today), Some(&base.parity), Some(weekday), Some(p)),
        None => base.schedule.clone(),
    };
    let slots: Vec<Value> = iter_syncable_slots(&base.schedule)
        .into_iter()
        .map(|(time, block)| {
            json!({
                "time": time,
                "block": block,
                "minutes": base.config.time_blocks.get(&block).cloned().unwrap_or(Value::Null),
                "title": planned.get(&time).map(|a| a.title.clone()),
            })
        })
        .collect();
    let mut times: Vec<&String> = merged.keys().collect();
    times.sort();
    let events: Vec<Value> = times
        .into_iter()
        .map(|time| {
            let event = &merged[time.as_str()];
            json!({"time": time, "label": format_event_label(event), "block": get_event_block_name(event)})
        })
        .collect();
    let plan_value = plan.as_ref().map(|p| {
        let mut sorted: Vec<(&String, &Assignment)> = planned.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        let assignments: Dict = sorted.into_iter().map(|(t, a)| (t.clone(), Value::String(a.title.clone()))).collect();
        json!({"date": p.target_date, "assignments": assignments})
    });
    Ok(json!({
        "date": today.format("%Y-%m-%d").to_string(),
        "weekday": weekday,
        "parity": base.parity,
        "mode": load_mode(),
        "skipped": base.is_skipped,
        "slots": slots,
        "events": events,
        "tasks": open_tasks(today, &base.config.task_types),
        "deadlines": upcoming_deadlines(today),
        "plan": plan_value,
    }))
}

fn normalize_assignments(
    raw: Option<&Value>,
    slots: &IndexMap<String, String>,
) -> Result<IndexMap<String, Assignment>, SyncError> {
    let pairs: Vec<(Value, Value)> = match raw {
        Some(Value::Array(items)) => {
            let mut pairs = Vec::new();
            for item in items {
                let Value::Object(map) = item else {
                    return Err(plan_error("Each assignment in a list must be an object with time and title."));
                };
                pairs.push((
                    map.get("time").cloned().unwrap_or(Value::Null),
                    map.get("title").cloned().unwrap_or(Value::Null),
                ));
            }
            pairs
        }
        Some(Value::Object(map)) => map
            .iter()
            .map(|(time, value)| {
                let title = match value {
                    Value::Object(inner) => inner.get("title").cloned().unwrap_or(Value::Null),
                    other => other.clone(),
                };
                (Value::String(time.clone()), title)
            })
            .collect(),
        _ => return Err(plan_error("The plan needs an 'assignments' object mapping slot times to task titles.")),
    };

    let mut assignments: IndexMap<String, Assignment> = IndexMap::new();
    for (time, title) in pairs {
        let block = match &time {
            Value::String(t) => slots.get(t),
            _ => None,
        };
        let Some(block) = block else {
            let available = if slots.is_empty() {
                "none".to_string()
            } else {
                slots.keys().cloned().collect::<Vec<_>>().join(", ")
            };
            return Err(plan_error(format!("{} is not one of today's focus slots ({available}).", py::py_repr(&time))));
        };
        let time = time.as_str().unwrap_or_default().to_string();
        let title = match &title {
            Value::String(s) if !s.trim().is_empty() => py::collapse_whitespace(s),
            _ => return Err(plan_error(format!("The assignment for {time} needs a title."))),
        };
        if title.chars().count() > MAX_TITLE_LENGTH {
            return Err(plan_error(format!("The title for {time} is longer than {MAX_TITLE_LENGTH} characters.")));
        }
        assignments.insert(time, Assignment { block: block.clone(), title });
    }
    if assignments.is_empty() {
        return Err(plan_error("The plan has no assignments. Use `rmd sync --clear` to remove today's plan."));
    }
    Ok(assignments)
}

/// Validate a plan for today and save it as the synced overlay.
pub fn apply_sync_plan(payload: &Value) -> Result<Value, SyncError> {
    let Value::Object(payload) = payload else {
        return Err(plan_error("The plan must be a JSON object."));
    };
    if load_mode() == "p" {
        return Err(plan_error(P_MODE_MESSAGE));
    }
    let today = crate::clock::today();
    let today_iso = today.format("%Y-%m-%d").to_string();
    let plan_date = payload.get("date").cloned().unwrap_or_else(|| Value::String(today_iso.clone()));
    if plan_date.as_str() != Some(today_iso.as_str()) {
        return Err(plan_error(format!(
            "The plan is for {}, but today is {today_iso}. Plans apply to today only.",
            py::py_str(&plan_date)
        )));
    }
    let base = base_today_schedule()?;
    if base.is_skipped {
        return Err(plan_error("Today is a skip day, so there is nothing to plan."));
    }
    let slots: IndexMap<String, String> = iter_syncable_slots(&base.schedule).into_iter().collect();
    let assignments = normalize_assignments(payload.get("assignments"), &slots)?;
    let count = assignments.len();
    let plan = SyncedDaySchedule {
        target_date: today_iso,
        parity: base.parity,
        weekday: py::weekday_name(today).to_string(),
        assignments,
    };
    let saved = save_synced_schedule(&plan, None)?;
    Ok(json!({"savedPath": saved.display().to_string(), "assigned": count}))
}

/// Remove the saved daily plan, if any.
pub fn clear_sync_plan() -> std::io::Result<Value> {
    let path = resolve_synced_schedule_path();
    let existed = path.exists();
    if existed {
        std::fs::remove_file(&path)?;
    }
    Ok(json!({"cleared": existed}))
}

/// Read a plan from a file, or standard input for `"-"`.
pub fn read_plan(source: &str) -> Result<Value, SyncError> {
    let text = if source == "-" {
        let mut buffer = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buffer)?;
        buffer
    } else {
        std::fs::read_to_string(crate::paths::expand_user(source))?
    };
    serde_json::from_str(&text).map_err(|e| plan_error(format!("The plan is not valid JSON: {e}")))
}

// --- CLI command (`rmd sync [--json | --apply FILE | --clear]`) is added below. ---
