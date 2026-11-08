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
    rows.sort_by_key(|row| std::cmp::Reverse(row.0));
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

use crate::i18n::{t, tf};
use crate::ui::{BoxStyle, Column, Console, Justify, Table, Text};

/// An `OSError` message the way Python words it (`[Errno 2] No such file or
/// directory: 'plan.json'`).
fn os_error_message(error: &std::io::Error, path: &str) -> String {
    match error.raw_os_error() {
        Some(code) => {
            let text = error.to_string();
            let reason = text.split(" (os error").next().unwrap_or(&text).to_string();
            format!("[Errno {code}] {reason}: {}", py::repr_str(path))
        }
        None => error.to_string(),
    }
}

fn print_plan(console: &mut Console, context: &Value) {
    if context["skipped"].as_bool() == Some(true) {
        console.print(&format!("[bold yellow]{}[/]", t("Today is a skipped day. Nothing to sync.")));
        return;
    }
    let slots = context["slots"].as_array().cloned().unwrap_or_default();
    if slots.is_empty() {
        console.print(&format!("[bold yellow]{}[/]", t("No untitled pomodoro/potato blocks need syncing today.")));
        return;
    }
    let mut table = Table::new()
        .title(&format!("[bold]{}[/bold]", t("Today's focus blocks")))
        .box_style(BoxStyle::Rounded)
        .header_style("bold cyan");
    table.add_column(Column::new(&t("Time")).justify(Justify::Right).style("cyan").no_wrap());
    table.add_column(Column::new(&t("Block")));
    table.add_column(Column::new(&t("Planned task")));
    for slot in &slots {
        let title = match &slot["title"] {
            Value::String(title) if !title.is_empty() => title.clone(),
            _ => "[dim]—[/dim]".to_string(),
        };
        table.add_row([
            Text::from(py::py_str(&slot["time"])),
            Text::from(py::py_str(&slot["block"])),
            Text::from(title),
        ]);
    }
    console.render(&table);
    if !py::truthy(&context["plan"]) {
        console.print(&t(
            "No plan yet. Ask your AI agent to plan today with the Schedule Everything plugin, or apply a plan with `rmd sync --apply plan.json`.",
        ));
    }
}

/// `rmd sync [--json | --apply FILE | --clear]`.
pub fn sync_command(json: bool, apply: Option<&str>, clear: bool) -> i32 {
    sync_command_with(&mut Console::stdout(), json, apply, clear)
}

/// [`sync_command`] writing to a given console.
pub fn sync_command_with(console: &mut Console, json: bool, apply: Option<&str>, clear: bool) -> i32 {
    let load_failed = |console: &mut Console, error: &dyn std::fmt::Display| {
        console.eprintln(&format!("❌ {} {error}", t("Failed to load today's schedule:")));
        1
    };
    if clear {
        return match clear_sync_plan() {
            Ok(result) => {
                let cleared = result["cleared"].as_bool() == Some(true);
                console.println(&if cleared { t("Cleared today's plan.") } else { t("There was no saved plan.") });
                0
            }
            Err(e) => load_failed(console, &e),
        };
    }
    if json {
        return match sync_context() {
            Ok(context) => {
                console.println(&py::dumps_pretty(&context));
                0
            }
            Err(e) => load_failed(console, &e),
        };
    }
    if load_mode() == "p" {
        console.println(&format!("❌ {}", t(P_MODE_MESSAGE)));
        return 1;
    }
    if let Some(source) = apply.filter(|s| !s.is_empty()) {
        let result = read_plan(source).and_then(|plan| apply_sync_plan(&plan));
        return match result {
            Ok(result) => {
                console.println(&tf(
                    "Saved today's plan with {count} focus blocks to {path}.",
                    &[("count", &py::py_str(&result["assigned"])), ("path", &py::py_str(&result["savedPath"]))],
                ));
                0
            }
            Err(SyncError::Plan(e)) => {
                console.eprintln(&format!("❌ {e}"));
                1
            }
            Err(SyncError::Io(e)) if source != "-" => load_failed(console, &os_error_message(&e, source)),
            Err(e) => load_failed(console, &e),
        };
    }
    match sync_context() {
        Ok(context) => {
            print_plan(console, &context);
            0
        }
        Err(e) => load_failed(console, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    const SETTINGS: &str = "[settings]\nlanguage = \"en\"\nskip_days = [\"sunday\"]\n\n[time_blocks]\npomodoro = 25\npotato = 50\n\n[task_types]\n1 = \"Work\"\n";
    const WEEK: &str = "[wednesday]\n\"09:00\" = \"pomodoro\"\n\"10:00\" = \"potato\"\n\"11:00\" = { block = \"pomodoro\", title = \"Fixed review\" }\n\"12:00\" = \"lunch\"\n";

    /// Wednesday 2026-11-04 (ISO week 45, odd), like the Python fixture's date.
    fn env() -> TestEnv {
        let mut env = TestEnv::new();
        env.set_now("2026-11-04T08:00:00");
        env.write("user_config_0/settings.toml", SETTINGS);
        env.write("user_config_0/odd_weeks.toml", WEEK);
        env.write("user_config_0/even_weeks.toml", WEEK);
        env.write(
            "tasks/tasks.json",
            r#"[{"description": "Release notes", "priority": 9, "type": "1"}, {"description": "Later idea", "priority": 4, "alarm_from": "2026-11-20"}]"#,
        );
        env.write(
            "user_config_0/ddl.json",
            r#"[{"event": "Launch", "deadline": "2026-11-06"}, {"event": "Past", "deadline": "2026-11-01"}]"#,
        );
        let overlay = env.root().join("synced_schedule.toml").display().to_string();
        env.set_var("REMINDER_SYNCED_SCHEDULE_PATH", &overlay);
        env
    }

    fn json_output(console: &Console) -> Value {
        serde_json::from_str(console.output()).expect("JSON output")
    }

    #[test]
    fn json_lists_slots_tasks_and_deadlines() {
        let _env = env();
        let mut console = Console::capture(80);
        assert_eq!(sync_command_with(&mut console, true, None, false), 0);
        let context = json_output(&console);
        let slots: Vec<(String, String, Value)> = context["slots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (py::py_str(&s["time"]), py::py_str(&s["block"]), s["minutes"].clone()))
            .collect();
        assert_eq!(
            slots,
            [
                ("09:00".to_string(), "pomodoro".to_string(), json!(25)),
                ("10:00".to_string(), "potato".to_string(), json!(50))
            ]
        );
        assert_eq!(
            context["tasks"][0],
            json!({"description": "Release notes", "priority": 9, "type": "1", "typeName": "Work",
                   "alarmFrom": null, "postponed": false, "procrastinated": false, "procrastinateDays": null})
        );
        assert_eq!(context["tasks"][1]["postponed"], true);
        assert_eq!(context["deadlines"], json!([{"event": "Launch", "deadline": "2026-11-06", "daysLeft": 2}]));
        assert_eq!(context["plan"], Value::Null);
    }

    #[test]
    fn apply_saves_a_plan_that_json_then_reports() {
        let env = env();
        let plan = env.write("plan.json", r#"{"date": "2026-11-04", "assignments": {"09:00": "  Release   notes "}}"#);
        let mut console = Console::capture(80);
        assert_eq!(sync_command_with(&mut console, false, Some(plan.to_str().unwrap()), false), 0);
        let overlay = env.root().join("synced_schedule.toml");
        assert_eq!(
            console.output(),
            format!("Saved today's plan with 1 focus blocks to {}.\n", crate::paths::resolve(&overlay).display())
        );
        let saved = load_synced_schedule(Some(&overlay)).unwrap();
        assert_eq!(
            (saved.target_date.as_str(), saved.parity.as_str(), saved.weekday.as_str()),
            ("2026-11-04", "odd", "wednesday")
        );
        assert_eq!(saved.assignments["09:00"].title, "Release notes");

        let mut console = Console::capture(80);
        assert_eq!(sync_command_with(&mut console, true, None, false), 0);
        let context = json_output(&console);
        assert_eq!(context["plan"], json!({"date": "2026-11-04", "assignments": {"09:00": "Release notes"}}));
        assert_eq!(context["slots"][0]["title"], "Release notes");
        assert!(
            context["events"]
                .as_array()
                .unwrap()
                .contains(&json!({"time": "09:00", "label": "pomodoro: Release notes", "block": "pomodoro"}))
        );
    }

    #[test]
    fn apply_rejects_invalid_plans_without_writing() {
        let env = env();
        for (payload, message) in [
            (json!({"date": "2026-11-05", "assignments": {"09:00": "A"}}), "today is 2026-11-04"),
            (json!({"assignments": {"11:00": "Fixed slot"}}), "not one of today's focus slots"),
            (json!({"assignments": {"12:00": "Lunch"}}), "not one of today's focus slots"),
            (json!({"assignments": {"09:00": "   "}}), "needs a title"),
            (json!({"assignments": {"09:00": "x".repeat(121)}}), "longer than 120"),
            (json!({"assignments": {}}), "no assignments"),
            (json!({"plan": []}), "assignments"),
        ] {
            let error = apply_sync_plan(&payload).expect_err("rejected");
            assert!(error.to_string().contains(message), "{error} vs {message}");
        }
        assert!(!env.root().join("synced_schedule.toml").exists());
        let plan = env.write("plan.json", "[1, 2]");
        let mut console = Console::capture(80);
        assert_eq!(sync_command_with(&mut console, false, Some(plan.to_str().unwrap()), false), 1);
        assert_eq!(console.error_output(), "❌ The plan must be a JSON object.\n");
        assert_eq!(console.output(), "");
    }

    #[test]
    fn p_mode_refuses_apply_but_allows_json_and_clear() {
        let env = env();
        env.write("tasks/mode.txt", "p");
        let mut console = Console::capture(80);
        assert_eq!(sync_command_with(&mut console, false, Some("-"), false), 1);
        assert_eq!(console.output(), "❌ Currently in p mode. Switch back to j mode to execute this command.\n");
        let mut console = Console::capture(80);
        assert_eq!(sync_command_with(&mut console, false, None, false), 1);
        let mut console = Console::capture(80);
        assert_eq!(sync_command_with(&mut console, true, None, false), 0);
        assert_eq!(json_output(&console)["mode"], "p");
        let mut console = Console::capture(80);
        assert_eq!(sync_command_with(&mut console, false, None, true), 0);
        assert_eq!(console.output(), "There was no saved plan.\n");
    }

    #[test]
    fn clear_removes_todays_plan() {
        let env = env();
        apply_sync_plan(&json!({"assignments": {"09:00": "Release notes"}})).unwrap();
        assert!(env.root().join("synced_schedule.toml").exists());
        let mut console = Console::capture(80);
        assert_eq!(sync_command_with(&mut console, false, None, true), 0);
        assert_eq!(console.output(), "Cleared today's plan.\n");
        assert!(!env.root().join("synced_schedule.toml").exists());
        assert_eq!(clear_sync_plan().unwrap(), json!({"cleared": false}));
    }

    #[test]
    fn bare_sync_shows_blocks_and_bad_input_fails() {
        let env = env();
        let mut console = Console::capture(80);
        assert_eq!(sync_command_with(&mut console, false, None, false), 0);
        let expected = [
            "       Today's focus blocks        ",
            "╭───────┬──────────┬──────────────╮",
            "│  Time │ Block    │ Planned task │",
            "├───────┼──────────┼──────────────┤",
            "│ 09:00 │ pomodoro │ —            │",
            "│ 10:00 │ potato   │ —            │",
            "╰───────┴──────────┴──────────────╯",
            "No plan yet. Ask your AI agent to plan today with the Schedule Everything ",
            "plugin, or apply a plan with `rmd sync --apply plan.json`.",
            "",
        ];
        assert_eq!(console.output(), expected.join("\n"));

        let bad = env.write("bad.json", "{not json");
        let mut console = Console::capture(80);
        assert_eq!(sync_command_with(&mut console, false, Some(bad.to_str().unwrap()), false), 1);
        assert!(console.error_output().contains("not valid JSON"));

        let missing = env.root().join("missing.json");
        let mut console = Console::capture(80);
        assert_eq!(sync_command_with(&mut console, false, Some(missing.to_str().unwrap()), false), 1);
        assert_eq!(
            console.error_output(),
            format!(
                "❌ Failed to load today's schedule: [Errno 2] No such file or directory: '{}'\n",
                missing.display()
            )
        );
    }

    #[test]
    fn skip_day_and_no_slots_messages() {
        let mut env = env();
        env.set_now("2026-11-08T08:00:00");
        let mut console = Console::capture(80);
        assert_eq!(sync_command_with(&mut console, false, None, false), 0);
        assert_eq!(console.output(), "Today is a skipped day. Nothing to sync.\n");
        env.set_now("2026-11-05T08:00:00");
        let mut console = Console::capture(80);
        assert_eq!(sync_command_with(&mut console, false, None, false), 0);
        assert_eq!(console.output(), "No untitled pomodoro/potato blocks need syncing today.\n");
    }
}
