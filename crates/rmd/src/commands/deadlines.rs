//! Deadline management (`rmd ddl ...`).
//!
//! Deadlines live in `ddl.json` as `{"event", "deadline", "added", ...}`. A
//! recurring deadline also has `repeat` (weekly, biweekly, or monthly); once it
//! passes it rolls forward to its next occurrence instead of being removed.

use chrono::{Duration, NaiveDate};
use serde_json::Value;

use crate::py;
use crate::recurrence::{anchor_day, repeat_rule, roll_forward};

/// One-off deadlines more than this many days overdue are removed.
pub const AUTO_REMOVE_OVERDUE_DAYS: i64 = -2;

/// Result of [`refresh_deadlines`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RefreshedDeadlines {
    pub kept: Vec<Value>,
    pub removed: Vec<Value>,
    /// Whether `kept` differs from the input and should be saved.
    pub changed: bool,
}

/// Remove long-overdue one-off deadlines and roll recurring ones forward.
pub fn refresh_deadlines(deadlines: &[Value], today: Option<NaiveDate>) -> RefreshedDeadlines {
    let current = today.unwrap_or_else(crate::clock::today);
    let earliest_kept = current + Duration::days(AUTO_REMOVE_OVERDUE_DAYS + 1);
    let mut out = RefreshedDeadlines::default();
    let mut rolled = false;
    for deadline in deadlines {
        let parsed = deadline.get("deadline").and_then(Value::as_str).and_then(|s| py::strptime_date(s, "%Y-%m-%d"));
        let Some(deadline_date) = parsed else {
            out.kept.push(deadline.clone());
            continue;
        };
        if deadline_date >= earliest_kept {
            out.kept.push(deadline.clone());
            continue;
        }
        let empty = py::Dict::new();
        let item = deadline.as_object().unwrap_or(&empty);
        let Some(rule) = repeat_rule(item) else {
            out.removed.push(deadline.clone());
            continue;
        };
        let next = roll_forward(deadline_date, rule, earliest_kept, Some(anchor_day(item, deadline_date)));
        let mut updated = item.clone();
        updated.insert("deadline".into(), Value::String(next.format("%Y-%m-%d").to_string()));
        out.kept.push(Value::Object(updated));
        rolled = true;
    }
    out.changed = rolled || !out.removed.is_empty();
    out
}

/// Split deadlines into retained and auto-removed entries (recurring ones rolled).
pub fn prune_expired_deadlines(deadlines: &[Value], today: Option<NaiveDate>) -> (Vec<Value>, Vec<Value>) {
    let refreshed = refresh_deadlines(deadlines, today);
    (refreshed.kept, refreshed.removed)
}

// --- CLI commands (`rmd ddl`, `rmd ddl add`, `rmd ddl rm`) are added below. ---

use crate::data::{load_deadlines, save_deadlines};
use crate::i18n::{t, tf};
use crate::recurrence::repeat_label;
use crate::ui::{BoxStyle, Column, Console, Justify, Table, Text};

/// Python's `int(text)` error message.
fn int_error(text: &str) -> String {
    format!("invalid literal for int() with base 10: {}", py::repr_str(text))
}

/// Parse `M.D` / `MM.DD` into the next date it falls on (this year, or next
/// year once it has passed). `Err` holds the message to print.
fn parse_month_day(date_text: &str, today: NaiveDate) -> Result<NaiveDate, String> {
    let parts: Vec<&str> = date_text.split('.').collect();
    if parts.len() != 2 {
        return Err(t("❌ Error: Date must be in format M.D or MM.DD (e.g., 7.4 or 07.04)"));
    }
    let invalid = |e: String| tf("❌ Error: Invalid date format - {e}", &[("e", &e)]);
    let month = py::parse_int_str(parts[0]).ok_or_else(|| invalid(int_error(parts[0])))?;
    let day = py::parse_int_str(parts[1]).ok_or_else(|| invalid(int_error(parts[1])))?;
    if !(1..=12).contains(&month) {
        return Err(t("❌ Error: Month must be between 1 and 12"));
    }
    if !(1..=31).contains(&day) {
        return Err(t("❌ Error: Day must be between 1 and 31"));
    }
    let on = |year: i32| {
        NaiveDate::from_ymd_opt(year, month as u32, day as u32)
            .ok_or_else(|| invalid("day is out of range for month".to_string()))
    };
    use chrono::Datelike;
    let this_year = on(today.year())?;
    if this_year < today { on(today.year() + 1) } else { Ok(this_year) }
}

/// `rmd ddl add EVENT M.D [--repeat RULE]`.
pub fn add_deadline(event: &str, date: &str, repeat: Option<&str>) -> i32 {
    add_deadline_with(&mut Console::stdout(), event, date, repeat)
}

/// [`add_deadline`] writing to a given console.
pub fn add_deadline_with(console: &mut Console, event: &str, date: &str, repeat: Option<&str>) -> i32 {
    let deadline_date = match parse_month_day(date, crate::clock::today()) {
        Ok(d) => d,
        Err(message) => {
            console.println(&message);
            return 1;
        }
    };
    let deadline_str = deadline_date.format("%Y-%m-%d").to_string();
    let mut deadlines = load_deadlines();
    let existing = deadlines.iter().position(|d| d.get("event").and_then(Value::as_str) == Some(event));
    let mut new_deadline = py::Dict::new();
    new_deadline.insert("event".into(), Value::String(event.to_string()));
    new_deadline.insert("deadline".into(), Value::String(deadline_str.clone()));
    new_deadline.insert("added".into(), Value::String(py::utc_now_isoformat()));
    if let Some(rule) = repeat.filter(|r| !r.is_empty()) {
        new_deadline.insert("repeat".into(), Value::String(rule.to_string()));
        if rule == "monthly" {
            use chrono::Datelike;
            new_deadline.insert("repeat_day".into(), Value::from(deadline_date.day()));
        }
    }
    let message = match existing {
        Some(index) => {
            let old_date = deadlines[index].get("deadline").map(py::py_str).unwrap_or_default();
            deadlines[index] = Value::Object(new_deadline);
            tf(
                "✅ Deadline for '{event_name}' updated from {old_date} to {deadline_str}",
                &[("event_name", &event), ("old_date", &old_date), ("deadline_str", &deadline_str)],
            )
        }
        None => {
            deadlines.push(Value::Object(new_deadline));
            tf("✅ Deadline '{event}' added successfully for {date}!", &[("event", &event), ("date", &deadline_str)])
        }
    };
    match save_deadlines(&deadlines) {
        Ok(()) => {
            console.println(&message);
            0
        }
        Err(e) => {
            console.println(&tf("❌ Error saving deadline: {e}", &[("e", &e)]));
            1
        }
    }
}

/// `rmd ddl`: all deadlines in a table (overdue one-offs are pruned and
/// recurring ones rolled forward first).
pub fn show_deadlines() -> i32 {
    show_deadlines_with(&mut Console::stdout())
}

/// [`show_deadlines`] writing to a given console.
pub fn show_deadlines_with(console: &mut Console) -> i32 {
    let today = crate::clock::today();
    let refreshed = refresh_deadlines(&load_deadlines(), Some(today));
    if refreshed.changed
        && let Err(e) = save_deadlines(&refreshed.kept)
    {
        console.print(&tf("[bold red]❌ Error saving pruned deadlines: {e}[/bold red]", &[("e", &e)]));
        return 1;
    }
    let deadlines = refreshed.kept;
    if deadlines.is_empty() {
        console.print(&t("[bold yellow]📅 No deadlines found[/bold yellow]"));
        return 0;
    }
    let mut sorted: Vec<&Value> = deadlines.iter().collect();
    sorted.sort_by(|a, b| {
        let key = |v: &Value| v.get("deadline").map(py::py_str).unwrap_or_default();
        key(a).cmp(&key(b))
    });

    let mut table = Table::new()
        .title(&format!("[bold]{}[/bold]", t("Current Deadlines")))
        .box_style(BoxStyle::Rounded)
        .header_style("bold cyan")
        .expand(true);
    table.add_column(Column::new(&t("Event")).style("bold"));
    table.add_column(Column::new(&t("Deadline")).width(15));
    table.add_column(Column::new(&t("Days Left")).justify(Justify::Right).width(12));
    table.add_column(Column::new(&t("Status")).justify(Justify::Center).width(10));

    for deadline in sorted {
        let event = deadline.get("event").map(py::py_str).unwrap_or_default();
        let deadline_text = deadline.get("deadline").map(py::py_str).unwrap_or_default();
        let Some(date) = py::strptime_date(&deadline_text, "%Y-%m-%d") else {
            let error = format!("time data {} does not match format '%Y-%m-%d'", py::repr_str(&deadline_text));
            console.println(&tf("❌ Unexpected error: {e}", &[("e", &error)]));
            return 1;
        };
        let days_left = (date - today).num_days();
        let days = |color: &str| {
            let text = if days_left == 0 { t("Today!") } else { tf("{days} days left", &[("days", &days_left)]) };
            format!("[{color}]{text}[/{color}]")
        };
        let (status, days_text, row_style) = match days_left {
            d if d < 0 => (t("⚠️ OVERDUE"), String::new(), "dim"),
            0 => (t("🔴 TODAY"), days("red"), ""),
            d if d <= 3 => (t("🔴 URGENT"), days("red"), ""),
            d if d <= 7 => (t("🟡 SOON"), days("yellow"), ""),
            _ => (t("🟢 OK"), days("green"), ""),
        };
        let mut event_markup = event;
        if let Some(rule) = deadline.as_object().and_then(crate::recurrence::repeat_rule) {
            let label = repeat_label(rule).map(t).unwrap_or_default();
            event_markup = format!("{event_markup} [dim]↻ {label}[/dim]");
        }
        table.add_row_styled(
            [
                Text::from(event_markup),
                Text::from(date.format("%b %d, %Y").to_string()),
                Text::from(days_text),
                Text::from(status),
            ],
            row_style,
        );
    }
    console.render(&table);
    console.print_justified(
        &format!("[dim]{}[/dim]", tf("Total deadlines: {count}", &[("count", &deadlines.len())])),
        Justify::Right,
    );
    0
}

/// `rmd ddl rm EVENT...`.
pub fn delete_deadline(events: &[String]) -> i32 {
    delete_deadline_with(&mut Console::stdout(), events)
}

/// [`delete_deadline`] writing to a given console.
pub fn delete_deadline_with(console: &mut Console, events: &[String]) -> i32 {
    let mut deadlines = load_deadlines();
    if deadlines.is_empty() {
        console.println(&t("⚠️  No deadlines found to delete"));
        return 1;
    }
    let mut errors = Vec::new();
    let mut deletions = Vec::new();
    for event in events {
        let original = deadlines.len();
        deadlines.retain(|d| d.get("event").and_then(Value::as_str) != Some(event.as_str()));
        let deleted = original - deadlines.len();
        match deleted {
            0 => errors.push(tf("❌ Deadline '{event}' not found", &[("event", event)])),
            1 => deletions.push(tf("Deadline '{event}'", &[("event", event)])),
            n => deletions
                .push(tf("{deleted_count} deadlines with name '{event}'", &[("deleted_count", &n), ("event", event)])),
        }
    }
    for error in &errors {
        console.println(error);
    }
    if deletions.is_empty() {
        return 1;
    }
    if let Err(e) = save_deadlines(&deadlines) {
        console.println(&tf("❌ Error saving deadlines: {e}", &[("e", &e)]));
        return 1;
    }
    if deletions.len() == 1 {
        console.println(&tf("✅ {deletion} deleted successfully!", &[("deletion", &deletions[0])]));
    } else {
        console.println(&tf("✅ {count} deadlines deleted successfully:", &[("count", &deletions.len())]));
        for deletion in &deletions {
            console.println(&format!("   - {deletion}"));
        }
    }
    if errors.is_empty() { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;
    use serde_json::json;

    fn env_with(deadlines: Value, now: &str) -> TestEnv {
        let mut env = TestEnv::new();
        env.set_now(now);
        env.write("user_config_0/settings.toml", "[settings]\nlanguage = \"en\"\n");
        env.write("user_config_0/ddl.json", &py::dumps_pretty(&deadlines));
        env
    }

    fn saved(env: &TestEnv) -> Vec<Value> {
        serde_json::from_str(&env.read("user_config_0/ddl.json")).unwrap()
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn add_uses_next_occurrence_of_month_day() {
        let env = env_with(json!([]), "2025-11-22T10:00:00");
        let mut console = Console::capture(80);
        assert_eq!(add_deadline_with(&mut console, "homework2", "7.4", None), 0);
        assert_eq!(console.output(), "✅ Deadline 'homework2' added successfully for 2026-07-04!\n");
        let mut console = Console::capture(80);
        assert_eq!(add_deadline_with(&mut console, "exam", "12.15", None), 0);
        assert_eq!(add_deadline_with(&mut Console::capture(80), "leading", "07.04", None), 0);
        assert_eq!(add_deadline_with(&mut Console::capture(80), "today", "11.22", None), 0);
        let deadlines = saved(&env);
        let dates: Vec<&str> = deadlines.iter().map(|d| d["deadline"].as_str().unwrap()).collect();
        assert_eq!(dates, ["2026-07-04", "2025-12-15", "2026-07-04", "2025-11-22"]);
        assert_eq!(deadlines[0].as_object().unwrap().keys().collect::<Vec<_>>(), ["event", "deadline", "added"]);
        assert!(deadlines[0]["added"].as_str().unwrap().ends_with("+00:00"));
    }

    #[test]
    fn add_updates_existing_and_records_repeat() {
        let env = env_with(
            json!([{"event": "homework2", "deadline": "2026-07-04", "added": "2025-11-20T10:00:00Z"}]),
            "2025-11-22T10:00:00",
        );
        let mut console = Console::capture(80);
        assert_eq!(add_deadline_with(&mut console, "homework2", "7.10", None), 0);
        assert_eq!(console.output(), "✅ Deadline for 'homework2' updated from 2026-07-04 to 2026-07-10\n");
        assert_eq!(add_deadline_with(&mut Console::capture(80), "rent", "12.31", Some("monthly")), 0);
        assert_eq!(add_deadline_with(&mut Console::capture(80), "sync", "12.1", Some("weekly")), 0);
        let deadlines = saved(&env);
        assert_eq!(deadlines.len(), 3);
        assert_eq!(deadlines[0]["deadline"], "2026-07-10");
        assert_eq!(deadlines[1]["repeat"], "monthly");
        assert_eq!(deadlines[1]["repeat_day"], 31);
        assert_eq!(deadlines[2]["repeat"], "weekly");
        assert!(deadlines[2].get("repeat_day").is_none());
    }

    #[test]
    fn add_rejects_bad_dates() {
        let env = env_with(json!([]), "2026-03-01T10:00:00");
        for (date, message) in [
            ("2026-07-04", "❌ Error: Date must be in format M.D or MM.DD (e.g., 7.4 or 07.04)"),
            ("1.2.3", "❌ Error: Date must be in format M.D or MM.DD (e.g., 7.4 or 07.04)"),
            ("13.4", "❌ Error: Month must be between 1 and 12"),
            ("0.5", "❌ Error: Month must be between 1 and 12"),
            ("7.32", "❌ Error: Day must be between 1 and 31"),
            ("5.0", "❌ Error: Day must be between 1 and 31"),
            ("2.30", "❌ Error: Invalid date format - day is out of range for month"),
            ("a.b", "❌ Error: Invalid date format - invalid literal for int() with base 10: 'a'"),
            ("7.", "❌ Error: Invalid date format - invalid literal for int() with base 10: ''"),
        ] {
            let mut console = Console::capture(80);
            assert_eq!(add_deadline_with(&mut console, "x", date, None), 1, "{date}");
            assert_eq!(console.output(), format!("{message}\n"));
        }
        assert!(saved(&env).is_empty());
        // Python's int() accepts surrounding spaces.
        assert_eq!(add_deadline_with(&mut Console::capture(80), "x", " 5. 3", None), 0);
        assert_eq!(saved(&env)[0]["deadline"], "2026-05-03");
    }

    #[test]
    fn delete_one_or_many() {
        let env = env_with(
            json!([
                {"event": "homework2", "deadline": "2026-07-04", "added": "x"},
                {"event": "project", "deadline": "2025-12-25", "added": "x"},
                {"event": "exam", "deadline": "2026-03-15", "added": "x"}
            ]),
            "2025-11-22T10:00:00",
        );
        let mut console = Console::capture(80);
        assert_eq!(delete_deadline_with(&mut console, &strings(&["homework2"])), 0);
        assert_eq!(console.output(), "✅ Deadline 'homework2' deleted successfully!\n");
        let mut console = Console::capture(80);
        assert_eq!(delete_deadline_with(&mut console, &strings(&["project", "nonexistent", "exam"])), 1);
        assert_eq!(
            console.output(),
            "❌ Deadline 'nonexistent' not found\n✅ 2 deadlines deleted successfully:\n   - Deadline 'project'\n   - Deadline 'exam'\n"
        );
        assert!(saved(&env).is_empty());
        let mut console = Console::capture(80);
        assert_eq!(delete_deadline_with(&mut console, &strings(&["homework2"])), 1);
        assert_eq!(console.output(), "⚠️  No deadlines found to delete\n");
    }

    #[test]
    fn delete_missing_event_leaves_file_alone() {
        let env =
            env_with(json!([{"event": "homework2", "deadline": "2026-07-04", "added": "x"}]), "2025-11-22T10:00:00");
        let before = env.read("user_config_0/ddl.json");
        let mut console = Console::capture(80);
        assert_eq!(delete_deadline_with(&mut console, &strings(&["nonexistent"])), 1);
        assert_eq!(console.output(), "❌ Deadline 'nonexistent' not found\n");
        assert_eq!(env.read("user_config_0/ddl.json"), before);
    }

    #[test]
    fn show_lists_by_date_with_status() {
        let _env = env_with(
            json!([
                {"event": "ok", "deadline": "2025-12-15", "added": "x"},
                {"event": "urgent", "deadline": "2025-11-24", "added": "x"},
                {"event": "soon", "deadline": "2025-11-27", "added": "x", "repeat": "weekly"},
                {"event": "overdue", "deadline": "2025-11-21", "added": "x"},
                {"event": "today", "deadline": "2025-11-22", "added": "x"}
            ]),
            "2025-11-22T10:00:00",
        );
        let mut console = Console::capture(100);
        assert_eq!(show_deadlines_with(&mut console), 0);
        let rows: Vec<Vec<String>> = console
            .output()
            .lines()
            .filter(|l| l.starts_with('│') && !l.contains("Event"))
            .map(|l| l.split('│').skip(1).take(4).map(|c| c.trim().to_string()).collect())
            .collect();
        assert_eq!(
            rows,
            [
                ["overdue", "Nov 21, 2025", "", "⚠️ OVERDUE"],
                ["today", "Nov 22, 2025", "Today!", "🔴 TODAY"],
                ["urgent", "Nov 24, 2025", "2 days left", "🔴 URGENT"],
                ["soon ↻ Weekly", "Nov 27, 2025", "5 days left", "🟡 SOON"],
                ["ok", "Dec 15, 2025", "23 days left", "🟢 OK"],
            ]
        );
        assert!(console.output().contains("Current Deadlines"));
        assert!(console.output().ends_with("Total deadlines: 5\n"));
    }

    #[test]
    fn show_prunes_long_overdue_and_rolls_recurring() {
        let env = env_with(
            json!([
                {"event": "remove", "deadline": "2026-04-21", "added": "x"},
                {"event": "keep-overdue", "deadline": "2026-04-22", "added": "x"},
                {"event": "keep-future", "deadline": "2026-04-25", "added": "x"},
                {"event": "weekly", "deadline": "2026-04-14", "added": "x", "repeat": "weekly"}
            ]),
            "2026-04-23T08:00:00",
        );
        assert_eq!(show_deadlines_with(&mut Console::capture(100)), 0);
        let events: Vec<(String, String)> = saved(&env)
            .iter()
            .map(|d| (d["event"].as_str().unwrap().to_string(), d["deadline"].as_str().unwrap().to_string()))
            .collect();
        assert_eq!(
            events,
            [
                ("keep-overdue".to_string(), "2026-04-22".to_string()),
                ("keep-future".to_string(), "2026-04-25".to_string()),
                ("weekly".to_string(), "2026-04-28".to_string()),
            ]
        );
    }

    #[test]
    fn show_empty() {
        let _env = env_with(json!([]), "2026-04-23T08:00:00");
        let mut console = Console::capture(80);
        assert_eq!(show_deadlines_with(&mut console), 0);
        assert_eq!(console.output(), "📅 No deadlines found\n");
    }
}
