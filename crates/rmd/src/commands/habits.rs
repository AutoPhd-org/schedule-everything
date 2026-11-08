//! Habit tracking (`rmd track [ID...]`).
//!
//! Without IDs, `rmd track` asks about each habit in a Yes/No/Stop dialog and
//! falls back to a terminal prompt when no dialog can be shown.

use indexmap::IndexMap;
use serde_json::Value;

use crate::data::{load_habit_records, load_habits, save_habit_records};
use crate::i18n::{get_language, t, tf};
use crate::py::{self, Dict};
use crate::ui::{Console, PromptError};

/// Numeric IDs first (in numeric order), then the rest alphabetically.
pub fn habit_sort_key(habit_id: &str) -> (u8, String) {
    if !habit_id.is_empty() && habit_id.chars().all(|c| c.is_ascii_digit()) {
        let trimmed = habit_id.trim_start_matches('0');
        (0, format!("{:0>9}", if trimmed.is_empty() { "0" } else { trimmed }))
    } else {
        (1, habit_id.to_string())
    }
}

/// Turn a habit description into a yes/no question.
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
        Some(first) if first.is_alphabetic() => first.to_lowercase().collect::<String>() + chars.as_str(),
        _ => text.to_string(),
    };
    format!("Did you {text} today?")
}

fn sorted_habits(habits: &IndexMap<String, String>) -> Vec<(&String, &String)> {
    let mut sorted: Vec<(&String, &String)> = habits.iter().collect();
    sorted.sort_by_key(|(id, _)| habit_sort_key(id));
    sorted
}

/// Ask about each habit with `ask` (question, title) → yes/no/stop. `None`
/// when stopped before anything was marked done.
fn prompt_completed_habits(
    habits: &IndexMap<String, String>,
    ask: &mut dyn FnMut(&str, &str) -> Option<bool>,
) -> Option<Vec<String>> {
    let sorted = sorted_habits(habits);
    let total = sorted.len();
    let mut completed = Vec::new();
    let mut cancelled = false;
    for (index, (id, description)) in sorted.into_iter().enumerate() {
        let title = tf("Habit Tracker ({i}/{total_habits})", &[("i", &(index + 1)), ("total_habits", &total)]);
        match ask(&habit_question(description), &title) {
            None => {
                cancelled = true;
                break;
            }
            Some(true) => completed.push(id.clone()),
            Some(false) => {}
        }
    }
    if cancelled && completed.is_empty() { None } else { Some(completed) }
}

/// Terminal fallback: list the habits and read IDs from one line. `Ok(None)`
/// when standard input is not a terminal.
fn prompt_completed_habits_cli(
    console: &mut Console,
    habits: &IndexMap<String, String>,
) -> Result<Option<Vec<String>>, PromptError> {
    if !console.is_interactive() {
        return Ok(None);
    }
    console.println(&t("Habits for today:"));
    for (id, description) in sorted_habits(habits) {
        console.println(&format!("  [{id}] {}", habit_question(description)));
    }
    let raw = console.input_plain(&t("Enter completed habit IDs (space-separated), or press Enter for none: "))?;
    Ok(Some(raw.split_whitespace().map(str::to_string).collect()))
}

/// `rmd track [ID...]`: record today's completed habits.
pub fn track_habits(habit_ids: &[String]) -> i32 {
    track_habits_with(&mut Console::stdout(), habit_ids, &mut crate::platform::ask_yes_no)
}

/// [`track_habits`] with a given console and Yes/No dialog.
pub fn track_habits_with(
    console: &mut Console,
    habit_ids: &[String],
    ask: &mut dyn FnMut(&str, &str) -> Option<bool>,
) -> i32 {
    let habits = load_habits();
    if habits.is_empty() {
        console.println(&t("❌ Error: No habits configured. Please create config/habits.toml"));
        return 1;
    }
    let mut ids: Vec<String> = habit_ids.to_vec();
    if ids.is_empty() {
        let mut chosen = prompt_completed_habits(&habits, ask);
        if chosen.is_none() {
            chosen = match prompt_completed_habits_cli(console, &habits) {
                Ok(chosen) => chosen,
                Err(PromptError::Interrupted) => {
                    console.println(&format!("\n{}", t("❌ Operation cancelled by user")));
                    return 1;
                }
                Err(PromptError::Eof) => {
                    console.println(&tf("❌ Unexpected error: {e}", &[("e", &"EOF when reading a line")]));
                    return 1;
                }
            };
        }
        match chosen {
            Some(chosen) => ids = chosen,
            None => {
                console
                    .println(&t("❌ Could not open a habit prompt window. Provide habit IDs, e.g. `rmd track 1 2`."));
                return 1;
            }
        }
    }

    let (valid, invalid): (Vec<String>, Vec<String>) = ids.into_iter().partition(|id| habits.contains_key(id));
    if !invalid.is_empty() {
        console.println(&tf("⚠️  Warning: Invalid habit IDs: {ids}", &[("ids", &invalid.join(", "))]));
        let mut available: Vec<&String> = habits.keys().collect();
        available.sort();
        let available = available.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
        console.println(&tf("Available habits: {habits}", &[("habits", &available)]));
        if valid.is_empty() {
            return 1;
        }
    }

    let today = crate::clock::today().format("%Y-%m-%d").to_string();
    let mut records = load_habit_records();
    let existing = records.iter().position(|r| r.get("date").and_then(Value::as_str) == Some(today.as_str()));
    let mut completed = Dict::new();
    for id in &valid {
        completed.insert(id.clone(), Value::String(habits[id].clone()));
    }
    let completed_count = completed.len();
    let mut record = Dict::new();
    record.insert("date".into(), Value::String(today.clone()));
    record.insert("completed".into(), Value::Object(completed));
    record.insert("timestamp".into(), Value::String(py::utc_now_isoformat()));

    match existing {
        Some(index) => {
            let previous = match records[index].get("completed") {
                Some(Value::Object(map)) => map.len(),
                Some(Value::Array(items)) => items.len(),
                Some(Value::String(text)) => text.chars().count(),
                _ => 0,
            };
            records[index] = Value::Object(record);
            console.println(&tf("✅ Updated habit record for {today}", &[("today", &today)]));
            console.println(&tf("Previously completed: {count} habits", &[("count", &previous)]));
        }
        None => {
            records.push(Value::Object(record));
            console.println(&tf("✅ Recorded habit tracking for {today}", &[("today", &today)]));
        }
    }
    console.println(&tf("Completed habits today: {count}", &[("count", &completed_count)]));
    let mut shown = valid.clone();
    shown.sort_by_key(|id| habit_sort_key(id));
    for id in &shown {
        console.println(&format!("  [{id}] {}", habits[id]));
    }
    match save_habit_records(&records) {
        Ok(()) => 0,
        Err(e) => {
            console.println(&tf("❌ Error saving habit records: {e}", &[("e", &e)]));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;
    use serde_json::json;

    fn env() -> TestEnv {
        let mut env = TestEnv::new();
        env.set_now("2026-11-09T10:30:00");
        env.write("user_config_0/settings.toml", "[settings]\nlanguage = \"en\"\n");
        env.write(
            "user_config_0/habits.toml",
            "[habits]\n1 = \"Go to sleep on time\"\n2 = \"Exercise\"\n3 = \"Did you read?\"\n10 = \"冥想\"\n",
        );
        env
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn no_dialog(_: &str, _: &str) -> Option<bool> {
        None
    }

    fn records(env: &TestEnv) -> Vec<Value> {
        serde_json::from_str(&env.read("tasks/record.json")).unwrap()
    }

    #[test]
    fn questions_read_naturally() {
        assert_eq!(habit_question("Exercise"), "Did you exercise today?");
        assert_eq!(habit_question("Did you read?"), "Did you read?");
        assert_eq!(habit_question("did you stretch"), "did you stretch?");
        assert_eq!(habit_question("  "), "Did you complete this habit today?");
        assert_eq!(habit_question("8 glasses of water"), "Did you 8 glasses of water today?");
    }

    #[test]
    fn sort_keys_put_numbers_first() {
        let mut ids = strings(&["b", "10", "2", "a", "1"]);
        ids.sort_by_key(|id| habit_sort_key(id));
        assert_eq!(ids, ["1", "2", "10", "a", "b"]);
    }

    #[test]
    fn records_ids_given_on_the_command_line() {
        let env = env();
        let mut console = Console::capture(80);
        assert_eq!(track_habits_with(&mut console, &strings(&["10", "1"]), &mut no_dialog), 0);
        assert_eq!(
            console.output(),
            "✅ Recorded habit tracking for 2026-11-09\nCompleted habits today: 2\n  [1] Go to sleep on time\n  [10] 冥想\n"
        );
        let saved = records(&env);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0]["date"], "2026-11-09");
        assert_eq!(saved[0]["completed"], json!({"10": "冥想", "1": "Go to sleep on time"}));
        assert!(saved[0]["timestamp"].as_str().unwrap().ends_with("+00:00"));

        let mut console = Console::capture(80);
        assert_eq!(track_habits_with(&mut console, &strings(&["2", "9"]), &mut no_dialog), 0);
        assert_eq!(
            console.output(),
            "⚠️  Warning: Invalid habit IDs: 9\nAvailable habits: 1, 10, 2, 3\n✅ Updated habit record for 2026-11-09\nPreviously completed: 2 habits\nCompleted habits today: 1\n  [2] Exercise\n"
        );
        assert_eq!(records(&env).len(), 1);
    }

    #[test]
    fn only_invalid_ids_fail() {
        let env = env();
        let mut console = Console::capture(80);
        assert_eq!(track_habits_with(&mut console, &strings(&["9"]), &mut no_dialog), 1);
        assert_eq!(console.output(), "⚠️  Warning: Invalid habit IDs: 9\nAvailable habits: 1, 10, 2, 3\n");
        assert!(!env.root().join("tasks/record.json").exists());
    }

    #[test]
    fn no_habits_configured() {
        let env = env();
        env.write("user_config_0/habits.toml", "[habits]\n");
        let mut console = Console::capture(80);
        assert_eq!(track_habits_with(&mut console, &strings(&["1"]), &mut no_dialog), 1);
        assert_eq!(console.output(), "❌ Error: No habits configured. Please create config/habits.toml\n");
    }

    #[test]
    fn dialogs_ask_about_each_habit_in_order() {
        let env = env();
        let mut asked = Vec::new();
        let mut ask = |question: &str, title: &str| {
            asked.push(format!("{title}: {question}"));
            Some(asked.len() % 2 == 1)
        };
        let mut console = Console::capture(80).with_interactive(false);
        assert_eq!(track_habits_with(&mut console, &[], &mut ask), 0);
        assert_eq!(
            asked,
            [
                "Habit Tracker (1/4): Did you go to sleep on time today?",
                "Habit Tracker (2/4): Did you exercise today?",
                "Habit Tracker (3/4): Did you read?",
                "Habit Tracker (4/4): Did you 冥想 today?",
            ]
        );
        assert_eq!(records(&env)[0]["completed"], json!({"1": "Go to sleep on time", "3": "Did you read?"}));
    }

    #[test]
    fn stopping_dialogs_falls_back_to_the_terminal() {
        let env = env();
        let mut console = Console::capture(80).with_inputs([Ok(" 2  10 ".to_string())]);
        assert_eq!(track_habits_with(&mut console, &[], &mut no_dialog), 0);
        assert!(console.output().starts_with(
            "Habits for today:\n  [1] Did you go to sleep on time today?\n  [2] Did you exercise today?\n  [3] Did you read?\n  [10] Did you 冥想 today?\nEnter completed habit IDs (space-separated), or press Enter for none:  2  10 \n"
        ));
        assert_eq!(records(&env)[0]["completed"], json!({"2": "Exercise", "10": "冥想"}));

        // Stopping after a "yes" keeps what was answered.
        let mut answers = vec![Some(true), None].into_iter();
        let mut ask = |_: &str, _: &str| answers.next().flatten();
        let mut console = Console::capture(80).with_interactive(false);
        assert_eq!(track_habits_with(&mut console, &[], &mut ask), 0);
        assert_eq!(records(&env)[0]["completed"], json!({"1": "Go to sleep on time"}));
    }

    #[cfg(unix)]
    #[test]
    fn save_errors_are_reported() {
        use std::os::unix::fs::PermissionsExt;
        let env = env();
        let path = env.write("tasks/record.json", "[]");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        if std::fs::OpenOptions::new().write(true).open(&path).is_ok() {
            return; // running as root
        }
        let mut console = Console::capture(80);
        assert_eq!(track_habits_with(&mut console, &strings(&["1"]), &mut no_dialog), 1);
        assert!(
            console.output().ends_with(
                "  [1] Go to sleep on time\n❌ Error saving habit records: Permission denied (os error 13)\n"
            )
        );
    }

    #[test]
    fn no_dialog_and_no_terminal_fails() {
        let _env = env();
        let mut console = Console::capture(80).with_interactive(false);
        assert_eq!(track_habits_with(&mut console, &[], &mut no_dialog), 1);
        assert_eq!(
            console.output(),
            "❌ Could not open a habit prompt window. Provide habit IDs, e.g. `rmd track 1 2`.\n"
        );
        let mut console = Console::capture(80).with_inputs([Err(PromptError::Eof)]);
        assert_eq!(track_habits_with(&mut console, &[], &mut no_dialog), 1);
        assert!(console.output().ends_with("❌ Unexpected error: EOF when reading a line\n"));
    }
}
