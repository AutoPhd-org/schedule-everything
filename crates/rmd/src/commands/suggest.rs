//! `rmd suggest`: tasks an AI agent suggests for the user to review.
//!
//! Agents read what is on the list and what the user decided lately
//! (`rmd suggest ls --json`), add suggestions (`rmd suggest add`), and withdraw
//! their own (`rmd suggest rm`). Only the user accepts or rejects a
//! suggestion, on the browser task board (see `crate::suggestions`).

use serde_json::Value;

use crate::i18n::{t, tf};
use crate::py;
use crate::suggestions::{
    NewSuggestion, SuggestionError, add_suggestion, pending_rows, suggest_context, withdraw_suggestions,
};
use crate::ui::{BoxStyle, Column, Console, Justify, Table, Text};
use crate::web::services::load_task_types;

/// `rmd suggest [ls] [--json]`.
pub fn list_command(json: bool) -> i32 {
    list_command_with(&mut Console::stdout(), json)
}

/// [`list_command`] writing to a given console.
pub fn list_command_with(console: &mut Console, json: bool) -> i32 {
    if json {
        console.println(&py::dumps_pretty(&suggest_context()));
        return 0;
    }
    let task_types = load_task_types();
    let rows = pending_rows(&crate::suggestions::load_suggestions(), &task_types);
    if rows.is_empty() {
        console.print(&format!("[bold yellow]{}[/bold yellow]", t("✨ No suggestions are waiting for review")));
        return 0;
    }
    let mut table = Table::new()
        .title(&format!("[bold]{}[/bold]", t("Suggested tasks")))
        .box_style(BoxStyle::Rounded)
        .header_style("bold cyan")
        .expand(true);
    table.add_column(Column::new(&t("ID")).justify(Justify::Right).style("dim").width(4).no_wrap());
    table.add_column(Column::new(&t("Priority")).justify(Justify::Right).no_wrap());
    table.add_column(Column::new(&t("Suggestion")));
    table.add_column(Column::new(&t("From")).style("dim"));
    let text = |row: &Value, key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    for row in &rows {
        let type_name = match &row["typeName"] {
            Value::Null => text(row, "type"),
            name => py::py_str(name),
        };
        let mut suggestion = format!("{} · {type_name}", text(row, "description"));
        if let Some(reason) = row["reason"].as_str() {
            suggestion.push_str(&format!("\n{reason}"));
        }
        let from = [text(row, "agent"), text(row, "source")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>();
        table.add_row([
            Text::raw(&text(row, "id")),
            Text::raw(&py::py_str(&row["priority"])),
            Text::raw(&suggestion),
            Text::raw(&from.join(" · ")),
        ]);
    }
    console.render(&table);
    console.print_justified(
        &format!(
            "[dim]{}[/dim]",
            tf("{count} waiting · accept or reject them on the task board (rmd web)", &[("count", &rows.len())])
        ),
        Justify::Right,
    );
    0
}

/// `rmd suggest add TASK PRIORITY [options]`.
pub fn add_command(new: &NewSuggestion) -> i32 {
    add_command_with(&mut Console::stdout(), new)
}

/// [`add_command`] writing to a given console.
pub fn add_command_with(console: &mut Console, new: &NewSuggestion) -> i32 {
    match add_suggestion(new) {
        Ok((item, replaced)) => {
            let description = item.get("description").map(py::py_str).unwrap_or_default();
            let id = item.get("id").map(py::py_str).unwrap_or_default();
            let args: [(&str, &dyn std::fmt::Display); 2] = [("description", &description), ("id", &id)];
            console.println(&if replaced {
                tf("✨ Updated the suggestion “{description}” (#{id}).", &args)
            } else {
                tf(
                    "✨ Suggested “{description}” (#{id}). It waits on the task board until the user accepts or rejects it.",
                    &args,
                )
            });
            0
        }
        Err(SuggestionError::Rejected { description, date }) => {
            console.eprintln(&format!(
                "❌ {} {}",
                tf("The user rejected “{description}” on {date}.", &[("description", &description), ("date", &date)]),
                t("Pass --again to suggest it anyway.")
            ));
            1
        }
        Err(error) => {
            console.eprintln(&format!("❌ {error}"));
            1
        }
    }
}

/// `rmd suggest rm SUGGESTION...`: withdraw pending suggestions.
pub fn withdraw_command(identifiers: &[String]) -> i32 {
    withdraw_command_with(&mut Console::stdout(), identifiers)
}

/// [`withdraw_command`] writing to a given console.
pub fn withdraw_command_with(console: &mut Console, identifiers: &[String]) -> i32 {
    match withdraw_suggestions(identifiers) {
        Ok((withdrawn, missing)) => {
            for identifier in &missing {
                console.eprintln(&tf(
                    "❌ No suggestion waiting for review matches '{identifier}'",
                    &[("identifier", identifier)],
                ));
            }
            for description in &withdrawn {
                console.println(&tf("↩️  Withdrew the suggestion “{description}”.", &[("description", description)]));
            }
            if missing.is_empty() { 0 } else { 1 }
        }
        Err(error) => {
            console.eprintln(&format!("❌ {error}"));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    fn env() -> TestEnv {
        let mut env = TestEnv::new();
        env.set_now("2026-11-09T08:00:00");
        env.write(
            "user_config_0/settings.toml",
            "[settings]\nlanguage = \"en\"\n\n[task_types]\n1 = \"Work\"\n2 = \"Errands\"\n",
        );
        env.write("tasks/tasks.json", "[]");
        env
    }

    fn quiet() -> Console {
        Console::capture(100).with_interactive(false)
    }

    fn new(description: &str, priority: i64) -> NewSuggestion {
        NewSuggestion { description: description.into(), priority, ..Default::default() }
    }

    #[test]
    fn add_list_and_withdraw() {
        let _env = env();
        let mut console = quiet();
        assert_eq!(list_command_with(&mut console, false), 0);
        assert_eq!(console.output(), "✨ No suggestions are waiting for review\n");

        let mut console = quiet();
        let reply = NewSuggestion {
            task_type: Some("2".into()),
            reason: Some("Asked for comments by Thursday".into()),
            source: Some("Gmail".into()),
            agent: Some("Claude".into()),
            ..new("Reply to Lee", 8)
        };
        assert_eq!(add_command_with(&mut console, &reply), 0);
        assert_eq!(
            console.output(),
            "✨ Suggested “Reply to Lee” (#1). It waits on the task board until the user accepts or rejects it.\n"
        );
        let mut console = quiet();
        assert_eq!(add_command_with(&mut console, &reply), 0);
        assert_eq!(console.output(), "✨ Updated the suggestion “Reply to Lee” (#1).\n");

        let mut console = quiet();
        assert_eq!(list_command_with(&mut console, false), 0);
        let output = console.output();
        assert!(output.contains("Suggested tasks"));
        assert!(output.contains("Reply to Lee · Errands"));
        assert!(output.contains("Asked for comments by Thursday"));
        assert!(output.contains("Claude · Gmail"));
        assert!(output.ends_with("1 waiting · accept or reject them on the task board (rmd web)\n"));

        let mut console = quiet();
        assert_eq!(list_command_with(&mut console, true), 0);
        let context: Value = serde_json::from_str(console.output()).unwrap();
        assert_eq!(context["pending"][0]["description"], "Reply to Lee");

        let mut console = quiet();
        assert_eq!(withdraw_command_with(&mut console, &["1".to_string(), "Nope".to_string()]), 1);
        assert_eq!(console.output(), "↩️  Withdrew the suggestion “Reply to Lee”.\n");
        assert_eq!(console.error_output(), "❌ No suggestion waiting for review matches 'Nope'\n");
    }

    #[test]
    fn errors_go_to_stderr_with_a_hint_for_rejected_tasks() {
        let env = env();
        let mut console = quiet();
        assert_eq!(add_command_with(&mut console, &NewSuggestion { task_type: Some("gym".into()), ..new("x", 5) }), 1);
        assert_eq!(console.error_output(), "❌ unknown task type: gym. Choose from: 1=Work, 2=Errands\n");
        env.write(
            "tasks/suggestions.json",
            r#"[{"id": "1", "description": "Renew gym", "status": "rejected", "decided_at": "2026-11-03T18:00:00+00:00"}]"#,
        );
        let mut console = quiet();
        assert_eq!(add_command_with(&mut console, &new("Renew gym", 5)), 1);
        assert_eq!(
            console.error_output(),
            "❌ The user rejected “Renew gym” on 2026-11-03. Pass --again to suggest it anyway.\n"
        );
    }
}
