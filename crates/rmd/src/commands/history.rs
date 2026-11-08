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

use crate::ui::{Console, Justify, Text};

/// `(color, icon)` for a priority level.
fn priority_style(priority: &Value) -> (&'static str, &'static str) {
    let value = py::py_float(priority).unwrap_or(0.0);
    if value >= 8.0 {
        ("red", "!!")
    } else if value >= 5.0 {
        ("yellow", "!")
    } else {
        ("blue", "~")
    }
}

/// `(icon, tag, style, strike)` so completed, cancelled and dropped
/// activities look different.
pub fn activity_status_style(status: &str) -> (&'static str, String, &'static str, bool) {
    match status {
        "cancelled" => ("🚫", t("cancelled"), "dim strike", true),
        "dropped" => ("🏳️", t("dropped"), "yellow", false),
        _ => ("", String::new(), "", false),
    }
}

/// `rmd history [count]`: recent completed activities grouped by day.
pub fn history_command(count: i64) -> i32 {
    history_command_with(&mut Console::stdout(), count)
}

/// [`history_command`] writing to a given console.
pub fn history_command_with(console: &mut Console, count: i64) -> i32 {
    let count = if count == 0 { 5 } else { count };
    let log_entries = crate::data::load_task_log();
    if log_entries.is_empty() {
        console.print(&format!("[bold yellow]{}[/bold yellow]", t("No activity history found")));
        return 0;
    }
    let activities = pair_task_activities(&log_entries);
    if activities.is_empty() {
        console.print(&format!(
            "[bold yellow]{}[/bold yellow]",
            t("No completed activities found. Complete some tasks to build history!")
        ));
        return 0;
    }
    // `activities[-count:]`, newest first.
    let start = if count > 0 {
        activities.len().saturating_sub(count as usize)
    } else {
        (count.unsigned_abs() as usize).min(activities.len())
    };
    let recent: Vec<&Activity> = activities[start..].iter().rev().collect();

    let mut by_day: indexmap::IndexMap<String, Vec<&Activity>> = indexmap::IndexMap::new();
    for activity in &recent {
        by_day.entry(activity.ended_at.format("%Y-%m-%d").to_string()).or_default().push(activity);
    }
    let mut days: Vec<&String> = by_day.keys().collect();
    days.sort_by(|a, b| b.cmp(a));

    console.newline();
    console.print_text_justified(
        &Text::styled(&tf("Recent Activity ({count})", &[("count", &recent.len())]), "bold cyan"),
        Justify::Center,
    );
    console.newline();
    for (index, day) in days.iter().enumerate() {
        if index > 0 {
            console.newline();
            console.rule("dim");
            console.newline();
        }
        let header = match py::strptime_date(day, "%Y-%m-%d") {
            Some(date) => format!("{day}  {}", py::weekday_title(date)),
            None => day.to_string(),
        };
        console.print_text(&Text::styled(&header, "bold underline bright_white"));
        console.newline();
        for activity in &by_day[*day] {
            let (color, icon) = priority_style(&activity.priority);
            let (status_icon, status_tag, status_style, _strike) = activity_status_style(&activity.status);
            let mut name = Text::new();
            let prefix = if status_icon.is_empty() {
                format!("  {}", activity.description)
            } else {
                format!("  {status_icon} {}", activity.description)
            };
            name.append(&prefix, "bold");
            if !status_tag.is_empty() {
                name.append(&format!("  ({status_tag})"), status_style);
            }
            let mut priority = Text::new();
            priority.append(&format!("[{icon} {}]", py::py_str(&activity.priority)), &format!("bold {color}"));
            let mut times = Text::new();
            times.append(&format!("    {}", activity.started_at.format("%H:%M")), "green");
            times.append(" → ", "dim");
            times.append(&activity.ended_at.format("%H:%M").to_string(), "red");
            times.append(&format!("  ({})", format_duration(activity.started_at, activity.ended_at)), "dim italic");
            console.print_text(&name);
            console.print_text(&priority);
            console.print_text(&times);
            console.newline();
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;
    use serde_json::json;

    fn entry(timestamp: &str, action: &str, description: &str, priority: i64) -> Value {
        json!({"timestamp": timestamp, "action": action, "task": {"description": description, "priority": priority}})
    }

    fn dt(text: &str) -> NaiveDateTime {
        py::parse_naive_datetime(text).unwrap()
    }

    #[test]
    fn pairs_added_with_closing_actions() {
        let activities = pair_task_activities(&[
            entry("2026-06-01T09:00:00", "added", "Task A", 8),
            entry("2026-06-01T10:00:00", "deleted", "Task A", 8),
            entry("2026-06-01T11:00:00", "added", "Task B", 3),
        ]);
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].description, "Task A");
        assert_eq!(activities[0].priority, json!(8));
        assert_eq!(activities[0].started_at, dt("2026-06-01T09:00:00"));
        assert_eq!(activities[0].ended_at, dt("2026-06-01T10:00:00"));

        assert!(
            pair_task_activities(&[
                entry("2026-06-01T09:00:00", "added", "Orphan task", 5),
                entry("2026-06-01T10:00:00", "updated", "Orphan task", 6),
            ])
            .is_empty()
        );

        let readded = pair_task_activities(&[
            entry("2026-06-01T09:00:00", "added", "Recycled task", 7),
            entry("2026-06-01T10:00:00", "deleted", "Recycled task", 7),
            entry("2026-06-02T09:00:00", "added", "Recycled task", 9),
            entry("2026-06-02T11:00:00", "deleted", "Recycled task", 9),
        ]);
        assert_eq!(readded.iter().map(|a| a.priority.clone()).collect::<Vec<_>>(), [json!(7), json!(9)]);
    }

    #[test]
    fn statuses_follow_the_closing_action() {
        let activities = pair_task_activities(&[
            entry("2026-06-01T09:00:00", "added", "Done task", 8),
            entry("2026-06-01T10:00:00", "deleted", "Done task", 8),
            entry("2026-06-01T09:00:00", "added", "Mistake task", 5),
            entry("2026-06-01T11:00:00", "cancelled", "Mistake task", 5),
            entry("2026-06-01T09:00:00", "added", "Give up task", 3),
            entry("2026-06-01T12:00:00", "dropped", "Give up task", 3),
        ]);
        let status = |d: &str| activities.iter().find(|a| a.description == d).unwrap().status.clone();
        assert_eq!(status("Done task"), "completed");
        assert_eq!(status("Mistake task"), "cancelled");
        assert_eq!(status("Give up task"), "dropped");

        assert_eq!(activity_status_style("completed"), ("", String::new(), "", false));
        assert_eq!(activity_status_style("cancelled").0, "🚫");
        assert!(activity_status_style("cancelled").3);
        assert_eq!(activity_status_style("dropped").0, "🏳️");
        assert!(!activity_status_style("dropped").3);
    }

    #[test]
    fn durations_are_human_readable() {
        assert_eq!(format_duration(dt("2026-06-01T09:00:00"), dt("2026-06-01T09:30:00")), "30m");
        assert_eq!(format_duration(dt("2026-06-01T09:00:00"), dt("2026-06-01T11:15:00")), "2h 15m");
        assert_eq!(format_duration(dt("2026-06-01T09:00:00"), dt("2026-06-03T09:00:00")), "2d");
        assert_eq!(format_duration(dt("2026-06-01T09:00:00"), dt("2026-06-01T09:00:00")), "< 1 min");
    }

    fn env_with_log(entries: Value) -> TestEnv {
        let env = TestEnv::new();
        env.write("user_config_0/settings.toml", "[settings]\nlanguage = \"en\"\n");
        env.write("tasks/tasks.log", &py::dumps_pretty(&entries));
        env
    }

    #[test]
    fn empty_log_and_unpaired_entries() {
        let env = env_with_log(json!([]));
        let mut console = Console::capture(80);
        assert_eq!(history_command_with(&mut console, 5), 0);
        assert_eq!(console.output(), "No activity history found\n");
        drop(env);

        let _env = env_with_log(json!([entry("2026-06-01T09:00:00", "added", "Pending task", 5)]));
        let mut console = Console::capture(80);
        assert_eq!(history_command_with(&mut console, 5), 0);
        assert_eq!(console.output(), "No completed activities found. Complete some tasks to build history!\n");
    }

    #[test]
    fn shows_recent_activities_grouped_by_day() {
        let _env = env_with_log(json!([
            entry("2026-06-01T09:00:00", "added", "Study math", 8),
            entry("2026-06-01T10:30:00", "deleted", "Study math", 8),
            entry("2026-06-02T14:00:00", "added", "Write report", 6),
            entry("2026-06-02T15:00:00", "cancelled", "Write report", 6),
            entry("2026-06-02T14:00:00", "added", "Old idea", 2),
            entry("2026-06-02T16:00:00", "dropped", "Old idea", 2),
        ]));
        let mut console = Console::capture(40);
        assert_eq!(history_command_with(&mut console, 5), 0);
        let expected = [
            "",
            "          Recent Activity (3)           ",
            "",
            "2026-06-02  Tuesday",
            "",
            "  🏳️ Old idea  (dropped)",
            "[~ 2]",
            "    14:00 → 16:00  (2h)",
            "",
            "  🚫 Write report  (cancelled)",
            "[! 6]",
            "    14:00 → 15:00  (1h)",
            "",
            "",
            "────────────────────────────────────────",
            "",
            "2026-06-01  Monday",
            "",
            "  Study math",
            "[!! 8]",
            "    09:00 → 10:30  (1h 30m)",
            "",
            "",
        ];
        assert_eq!(console.output(), expected.join("\n"));
    }

    #[test]
    fn count_limits_like_python_slicing() {
        let mut entries = Vec::new();
        for i in 0..10 {
            entries.push(entry(&format!("2026-06-01T{:02}:00:00", 9 + i), "added", &format!("Task {i}"), 5));
            entries.push(entry(&format!("2026-06-01T{:02}:30:00", 9 + i), "deleted", &format!("Task {i}"), 5));
        }
        let _env = env_with_log(Value::Array(entries));
        let shown = |count: i64| {
            let mut console = Console::capture(80);
            history_command_with(&mut console, count);
            console.output().lines().filter(|l| l.starts_with("  Task")).map(str::to_string).collect::<Vec<_>>()
        };
        assert_eq!(shown(3), ["  Task 9", "  Task 8", "  Task 7"]);
        assert_eq!(shown(0).len(), 5);
        assert_eq!(shown(-8), ["  Task 9", "  Task 8"]);
        assert_eq!(shown(50).len(), 10);
    }
}
