//! Daily summary and habit tracking popups (port of `popups.py`).
//!
//! The reminder service shows these at the configured `[tasks]` times: the
//! daily summary lists the tasks completed (removed with `rmd rm`) today, and
//! the habit prompt asks about each habit in `habits.toml` and records the
//! answers in `tasks/record.json`.

use std::cmp::Ordering;

use chrono::NaiveDateTime;
use indexmap::IndexMap;
use serde_json::{Value, json};

use crate::config::ScheduleConfig;
use crate::i18n::{get_language, t, tf};
use crate::py::{self, Dict};
use crate::{clock, data, paths, platform};

// =============================================================================
// Task log
// =============================================================================

/// Task action log entries (missing or invalid file gives an empty list).
pub fn load_task_log() -> Vec<Value> {
    data::load_task_log()
}

/// Tasks completed (logged as `deleted`) today, as the logged task objects.
///
/// Log timestamps are UTC ISO strings; their date part is compared with the
/// local date, exactly like the Python version.
pub fn get_today_completed_tasks() -> Vec<Value> {
    let today = clock::today().format("%Y-%m-%d").to_string();
    let mut completed = Vec::new();
    for entry in load_task_log() {
        if entry.get("action").and_then(Value::as_str) != Some("deleted") {
            continue;
        }
        let Some(Value::String(timestamp)) = entry.get("timestamp") else {
            continue;
        };
        let entry_date = if timestamp.contains('T') { timestamp.split('T').next().unwrap_or("") } else { "" };
        if entry_date == today {
            completed.push(entry.get("task").cloned().unwrap_or_else(|| json!({})));
        }
    }
    completed
}

// =============================================================================
// Daily summary
// =============================================================================

/// Order two `priority` values like Python's `<`: numbers (and booleans)
/// numerically, strings by code point. Values Python cannot compare keep
/// their relative order (numbers before strings before anything else).
fn compare_priorities(a: &Value, b: &Value) -> Ordering {
    fn rank(value: &Value) -> u8 {
        match value {
            Value::Bool(_) | Value::Number(_) => 0,
            Value::String(_) => 1,
            _ => 2,
        }
    }
    fn exact_int(value: &Value) -> Option<i128> {
        match value {
            Value::Bool(b) => Some(i128::from(*b)),
            Value::Number(n) => n.as_i64().map(i128::from).or_else(|| n.as_u64().map(i128::from)),
            _ => None,
        }
    }
    match (a, b) {
        (Value::String(x), Value::String(y)) => x.cmp(y),
        _ if rank(a) == 0 && rank(b) == 0 => match (exact_int(a), exact_int(b)) {
            (Some(x), Some(y)) => x.cmp(&y),
            _ => py::py_float(a).partial_cmp(&py::py_float(b)).unwrap_or(Ordering::Equal),
        },
        _ => rank(a).cmp(&rank(b)),
    }
}

/// The daily summary text for the given completed tasks.
pub fn daily_summary_message(completed_tasks: &[Value]) -> String {
    if completed_tasks.is_empty() {
        return t("📋 Daily Completed Tasks\n\n✨ No tasks completed today. Keep it up tomorrow!");
    }
    let zero = json!(0);
    let priority_of = |task: &Value| task.get("priority").cloned().unwrap_or_else(|| zero.clone());
    // Highest priority first; ties keep their log order (stable, like `sorted(..., reverse=True)`).
    let mut sorted: Vec<&Value> = completed_tasks.iter().collect();
    sorted.sort_by(|a, b| compare_priorities(&priority_of(b), &priority_of(a)));

    let task_lines: Vec<String> = sorted
        .iter()
        .enumerate()
        .map(|(i, task)| {
            let description = task.get("description").map(py::py_str).unwrap_or_else(|| t("Unknown Task"));
            let priority = py::py_str(&priority_of(task));
            tf(
                "{index}. {description} (Priority: {priority})",
                &[("index", &(i + 1)), ("description", &description), ("priority", &priority)],
            )
        })
        .collect();
    tf("📋 Daily Completed Tasks Summary\n\n🎉 Completed {count} tasks today:\n\n", &[("count", &sorted.len())])
        + &task_lines.join("\n")
}

/// Show today's completed tasks (highest priority first) with a sound.
pub fn show_daily_summary_popup() {
    let summary_message = daily_summary_message(&get_today_completed_tasks());

    // Play the notification sound; a missing or broken settings file only
    // silences it.
    if let Ok(config) = ScheduleConfig::load(&paths::runtime().settings_path) {
        platform::play_sound(&config.sound_file());
    }

    platform::show_dialog(&summary_message);
}

// =============================================================================
// Habit tracking
// =============================================================================

/// Sort key for habit ids: numeric ids first (numerically), then the rest.
fn habit_sort_key(habit_id: &str) -> (u8, String) {
    if !habit_id.is_empty() && habit_id.bytes().all(|b| b.is_ascii_digit()) {
        let digits = habit_id.trim_start_matches('0');
        return (0, format!("{:0>9}", if digits.is_empty() { "0" } else { digits }));
    }
    (1, habit_id.to_string())
}

/// Turn a habit description into a yes/no question:
/// `"Exercise"` -> `"Did you exercise today?"`.
pub fn habit_question(description: &str) -> String {
    let text = description.trim();
    if text.is_empty() {
        return t("Did you complete this habit today?");
    }
    if text.ends_with('?') {
        return text.to_string();
    }
    if text.to_lowercase().starts_with("did you ") {
        return format!("{text}?");
    }
    if get_language() == "zh" {
        return format!("今天你 {text} 了吗？");
    }
    let mut chars = text.chars();
    let text = match chars.next() {
        Some(first) if first.is_alphabetic() => first.to_lowercase().chain(chars).collect(),
        _ => text.to_string(),
    };
    format!("Did you {text} today?")
}

/// Habits as `{id: description}`; an unreadable file means no habits.
fn load_habits() -> IndexMap<String, String> {
    match crate::config::load_toml_file(&paths::runtime().habit_path) {
        Ok(document) => data::habits_from_dict(&document),
        Err(_) => IndexMap::new(),
    }
}

/// Ask about each habit and save today's record. Returns `true` when a record
/// was saved, `false` when there are no habits or the first answer was Stop.
pub fn show_habit_tracking_popup(now: Option<NaiveDateTime>) -> bool {
    show_habit_tracking_popup_with(now, &mut platform::ask_yes_no)
}

/// [`show_habit_tracking_popup`] with an injectable yes/no prompt
/// (`Some(true)` yes, `Some(false)` no, `None` stop).
pub fn show_habit_tracking_popup_with(
    now: Option<NaiveDateTime>,
    ask_yes_no: &mut dyn FnMut(&str, &str) -> Option<bool>,
) -> bool {
    let habits = load_habits();
    if habits.is_empty() {
        return false;
    }

    let now_dt = now.unwrap_or_else(clock::now);
    let today = now_dt.format("%Y-%m-%d").to_string();

    // Sort habits for a consistent order.
    let mut sorted_habits: Vec<(&String, &String)> = habits.iter().collect();
    sorted_habits.sort_by_key(|(habit_id, _)| habit_sort_key(habit_id));

    let mut completed_ids: Vec<&String> = Vec::new();
    let total_habits = sorted_habits.len();
    let mut cancelled = false;

    for (i, (habit_id, description)) in sorted_habits.iter().enumerate() {
        let question = habit_question(description);
        let title = tf("Habit Tracker ({i}/{total_habits})", &[("i", &(i + 1)), ("total_habits", &total_habits)]);
        match ask_yes_no(&question, &title) {
            // The user clicked Stop or closed the dialog.
            None => {
                cancelled = true;
                break;
            }
            Some(true) => completed_ids.push(habit_id),
            Some(false) => {}
        }
    }

    // Stopped before answering yes to anything: nothing to save.
    if cancelled && completed_ids.is_empty() {
        return false;
    }

    let completed: Dict =
        completed_ids.into_iter().map(|habit_id| (habit_id.clone(), Value::String(habits[habit_id].clone()))).collect();
    let new_record = json!({
        "date": today,
        "completed": completed,
        "timestamp": py::isoformat_naive(&now_dt),
    });

    let mut records = data::load_habit_records();
    match records.iter().position(|record| record.get("date").and_then(Value::as_str) == Some(today.as_str())) {
        Some(index) => records[index] = new_record,
        None => records.push(new_record),
    }

    if let Err(error) = data::save_habit_records(&records) {
        eprintln!("Error saving habit records: {error}");
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;
    use chrono::NaiveDate;

    fn noon_jan_2() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2025, 1, 2).unwrap().and_hms_opt(12, 0, 0).unwrap()
    }

    fn read_records(env: &TestEnv) -> Value {
        serde_json::from_str(&env.read("tasks/record.json")).unwrap()
    }

    #[test]
    fn habit_tracking_saves_record() {
        let env = TestEnv::new();
        env.write("user_config_0/habits.toml", "[habits]\n1 = \"Get up early\"\n2 = \"Exercise\"\n");

        let mut calls = 0;
        let mut first_yes = |_: &str, _: &str| {
            calls += 1;
            Some(calls == 1)
        };
        assert!(show_habit_tracking_popup_with(Some(noon_jan_2()), &mut first_yes));

        let data = read_records(&env);
        assert_eq!(data[0]["date"], "2025-01-02");
        assert_eq!(data[0]["completed"], json!({"1": "Get up early"}));
        assert_eq!(data[0]["timestamp"], "2025-01-02T12:00:00");

        // Answering "no" to everything replaces today's record with an empty one.
        assert!(show_habit_tracking_popup_with(Some(noon_jan_2()), &mut |_, _| Some(false)));
        let data = read_records(&env);
        assert_eq!(data.as_array().unwrap().len(), 1);
        assert_eq!(data[0]["completed"], json!({}));
        assert_eq!(
            env.read("tasks/record.json"),
            "[\n  {\n    \"date\": \"2025-01-02\",\n    \"completed\": {},\n    \"timestamp\": \"2025-01-02T12:00:00\"\n  }\n]"
        );
    }

    #[test]
    fn habit_tracking_cancel_does_not_save() {
        let env = TestEnv::new();
        env.write("user_config_0/habits.toml", "[habits]\n1 = \"Get up early\"\n");

        assert!(!show_habit_tracking_popup_with(Some(noon_jan_2()), &mut |_, _| None));
        assert!(!env.root().join("tasks/record.json").exists());
    }

    #[test]
    fn habit_tracking_keeps_answers_given_before_stop() {
        let env = TestEnv::new();
        env.write("user_config_0/habits.toml", "[habits]\n2 = \"Read\"\n10 = \"Stretch\"\n1 = \"Walk\"\n");
        env.write("tasks/record.json", r#"[{"date": "2025-01-01", "completed": {}, "timestamp": "x"}]"#);

        let mut asked = Vec::new();
        let mut answers = [Some(true), Some(false), None].into_iter();
        let mut ask = |question: &str, title: &str| {
            asked.push((question.to_string(), title.to_string()));
            answers.next().flatten()
        };
        assert!(show_habit_tracking_popup_with(Some(noon_jan_2()), &mut ask));

        // Numeric ids are asked in numeric order.
        let questions: Vec<&str> = asked.iter().map(|(q, _)| q.as_str()).collect();
        assert_eq!(questions, ["Did you walk today?", "Did you read today?", "Did you stretch today?"]);
        assert_eq!(asked[0].1, "Habit Tracker (1/3)");
        let data = read_records(&env);
        assert_eq!(data.as_array().unwrap().len(), 2);
        assert_eq!(data[1]["completed"], json!({"1": "Walk"}));
    }

    #[test]
    fn habit_tracking_without_habits_does_nothing() {
        let env = TestEnv::new();
        assert!(!show_habit_tracking_popup_with(Some(noon_jan_2()), &mut |_, _| panic!("no habits to ask about")));
        assert!(!env.root().join("tasks/record.json").exists());
    }

    #[test]
    fn habit_ids_from_name_to_number_tables() {
        let env = TestEnv::new();
        env.write("user_config_0/habits.toml", "[habits]\n\"Drink water\" = 2\n\"Sleep early\" = 1\n");
        let mut asked = Vec::new();
        assert!(show_habit_tracking_popup_with(Some(noon_jan_2()), &mut |question, _| {
            asked.push(question.to_string());
            Some(true)
        }));
        assert_eq!(asked, ["Did you sleep early today?", "Did you drink water today?"]);
        assert_eq!(read_records(&env)[0]["completed"], json!({"1": "Sleep early", "2": "Drink water"}));
    }

    #[test]
    fn habit_questions() {
        let _env = TestEnv::new();
        assert_eq!(habit_question("Exercise"), "Did you exercise today?");
        assert_eq!(habit_question("Did you read?"), "Did you read?");
        assert_eq!(habit_question("did you meditate"), "did you meditate?");
        assert_eq!(habit_question("  Meditate for 10 mins "), "Did you meditate for 10 mins today?");
        assert_eq!(habit_question("10k steps"), "Did you 10k steps today?");
        assert_eq!(habit_question("   "), "Did you complete this habit today?");
    }

    #[test]
    fn habit_questions_in_chinese() {
        let mut env = TestEnv::new();
        env.set_var("REMINDER_LANG", "zh");
        assert_eq!(habit_question("早起"), "今天你 早起 了吗？");
        assert_eq!(habit_question("Did you read?"), "Did you read?");
    }

    #[test]
    fn habit_sort_keys() {
        assert_eq!(habit_sort_key("7"), (0, "000000007".to_string()));
        assert_eq!(habit_sort_key("0010"), (0, "000000010".to_string()));
        assert_eq!(habit_sort_key("x1"), (1, "x1".to_string()));
        assert_eq!(habit_sort_key(""), (1, String::new()));
    }

    #[test]
    fn today_completed_tasks_come_from_deleted_log_entries() {
        let mut env = TestEnv::new();
        env.set_now("2026-04-08T21:00:00");
        env.write(
            "tasks/tasks.log",
            &py::dumps_pretty(&json!([
                {"timestamp": "2026-04-08T09:00:00+00:00", "action": "deleted", "task": {"description": "A", "priority": 3}},
                {"timestamp": "2026-04-08T10:00:00+00:00", "action": "added", "task": {"description": "B", "priority": 9}},
                {"timestamp": "2026-04-07T10:00:00+00:00", "action": "deleted", "task": {"description": "C", "priority": 9}},
                {"timestamp": "2026-04-08", "action": "deleted", "task": {"description": "D", "priority": 9}},
                {"timestamp": null, "action": "deleted", "task": {"description": "E", "priority": 9}},
                {"timestamp": "2026-04-08T11:00:00+00:00", "action": "deleted", "task": {"description": "F", "priority": 8}},
                {"timestamp": "2026-04-08T12:00:00+00:00", "action": "deleted"},
            ])),
        );
        let completed = get_today_completed_tasks();
        assert_eq!(
            completed,
            vec![json!({"description": "A", "priority": 3}), json!({"description": "F", "priority": 8}), json!({})]
        );
    }

    #[test]
    fn daily_summary_lists_tasks_by_priority() {
        let _env = TestEnv::new();
        let message = daily_summary_message(&[
            json!({"description": "Code review", "priority": 3}),
            json!({"description": "Write documentation", "priority": 8}),
            json!({"description": "Fix bug", "priority": 5}),
            json!({"description": "Tie", "priority": 5}),
            json!({"priority": 1}),
        ]);
        assert_eq!(
            message,
            "📋 Daily Completed Tasks Summary\n\n🎉 Completed 5 tasks today:\n\n\
             1. Write documentation (Priority: 8)\n\
             2. Fix bug (Priority: 5)\n\
             3. Tie (Priority: 5)\n\
             4. Code review (Priority: 3)\n\
             5. Unknown Task (Priority: 1)"
        );
        assert_eq!(
            daily_summary_message(&[]),
            "📋 Daily Completed Tasks\n\n✨ No tasks completed today. Keep it up tomorrow!"
        );
        assert_eq!(
            daily_summary_message(&[json!({"description": "x"}), json!({"description": "y", "priority": 2.5})]),
            "📋 Daily Completed Tasks Summary\n\n🎉 Completed 2 tasks today:\n\n\
             1. y (Priority: 2.5)\n\
             2. x (Priority: 0)"
        );
    }

    #[test]
    fn daily_summary_in_chinese() {
        let mut env = TestEnv::new();
        env.set_var("REMINDER_LANG", "zh");
        let message = daily_summary_message(&[json!({"description": "写代码", "priority": 8})]);
        assert_eq!(message, "📋 今日完成任务总结\n\n🎉 今天完成了 1 个任务：\n\n1. 写代码 (优先级: 8)");
    }

    #[test]
    fn priorities_compare_like_python() {
        assert_eq!(compare_priorities(&json!(2), &json!(10)), Ordering::Less);
        assert_eq!(compare_priorities(&json!(2.5), &json!(2)), Ordering::Greater);
        assert_eq!(compare_priorities(&json!(true), &json!(1)), Ordering::Equal);
        assert_eq!(compare_priorities(&json!("10"), &json!("9")), Ordering::Less);
        assert_eq!(compare_priorities(&json!(9), &json!("1")), Ordering::Less);
    }
}
