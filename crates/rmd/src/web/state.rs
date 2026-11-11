//! The workspace's open state: the state document (`GET /api/state`,
//! `rmd state`) and the health report (`GET /api/health`) of the LaunchX
//! contract.
//!
//! Both are computed from the files on every request, like everything the
//! page shows, so a change made with the CLI shows up without a restart. The
//! shape is documented in `documentation/docs/advanced/launchx.md`; fields are only
//! ever added.

use serde_json::{Map, Value, json};

use crate::py::Dict;
use crate::web::services::{self, WebResult};
use crate::web::workspace;

/// Read commands of the `POST /api` channel behind the views the state
/// document leaves out: they take a parameter (a day, a year, a file) or are
/// large.
pub const VIEW_COMMANDS: [&str; 5] =
    ["daily_overview", "weekly_overview", "task_history", "conference_calendar", "config_read"];

/// Everything the workspace page shows when it loads: `workspace` (config
/// sets, mode, language), today's `schedule`, `tasks`, `deadlines`,
/// `habits`, `suggestions` and `plugins` (the `status_snapshot` command's
/// fields), and `views`, which names the commands for the rest.
pub fn document() -> WebResult {
    let mut state = Map::new();
    state.insert("workspace".into(), workspace::workspace_info(&Dict::new())?);
    match services::status_snapshot(&Dict::new())? {
        Value::Object(snapshot) => state.extend(snapshot),
        other => {
            state.insert("snapshot".into(), other);
        }
    }
    state.insert("views".into(), json!({"href": "/api", "method": "POST", "commands": VIEW_COMMANDS}));
    Ok(Value::Object(state))
}

/// One short line for the launcher's card: open tasks, suggestions waiting,
/// and the current or next event.
fn summary(snapshot: &Value) -> String {
    let count = |key: &str| snapshot.get(key).and_then(Value::as_array).map_or(0, Vec::len);
    let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut parts = vec![plural(count("tasks"), "open task", "open tasks")];
    let suggestions = count("suggestions");
    if suggestions > 0 {
        parts.push(plural(suggestions, "suggestion to review", "suggestions to review"));
    }
    let schedule = snapshot.get("schedule");
    let event = |key: &str| schedule.and_then(|s| s.get(key)).and_then(Value::as_str).filter(|s| !s.is_empty());
    if let Some(current) = event("current") {
        parts.push(format!("now: {current}"));
    } else if let Some(next) = event("next") {
        parts.push(format!("next: {next}"));
    }
    parts.join(" · ")
}

/// The health report (contract §6.3). `degraded` while the schedule
/// configuration does not load: the workspace still works (it is where the
/// configuration gets repaired), but reminders and today's schedule do not.
pub fn health(id: &str) -> Value {
    let mut health = json!({"id": id, "version": crate::launchx::VERSION, "status": "ok"});
    let degraded = |health: &mut Value, detail: String| {
        health["status"] = json!("degraded");
        health["detail"] = json!(detail);
    };
    match services::status_snapshot(&Dict::new()) {
        Ok(snapshot) => {
            health["summary"] = json!(summary(&snapshot));
            let error = snapshot.get("config").and_then(|config| config.get("error")).and_then(Value::as_str);
            if let Some(error) = error {
                degraded(&mut health, format!("The schedule configuration does not load: {error}"));
            }
        }
        Err(error) => degraded(&mut health, error.message),
    }
    health
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    #[test]
    fn the_state_document_holds_what_the_page_loads() {
        let mut env = TestEnv::with_fixture();
        env.set_now("2026-11-10T10:00:00");
        let state = document().unwrap();
        let keys: Vec<&str> = state.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "workspace",
                "config",
                "today",
                "schedule",
                "tasks",
                "deadlines",
                "habits",
                "taskTypes",
                "taskTypeColors",
                "history",
                "timedDone",
                "suggestions",
                "plugins",
                "views"
            ]
        );
        assert_eq!(state["workspace"], workspace::workspace_info(&Dict::new()).unwrap());
        assert_eq!(state["today"]["date"], json!("2026-11-10"));
        assert_eq!(state["views"]["commands"][0], json!("daily_overview"));
        for command in VIEW_COMMANDS {
            assert!(crate::web::api::COMMANDS.iter().any(|(name, _)| *name == command), "{command}");
        }
    }

    #[test]
    fn health_reports_tasks_and_a_broken_configuration() {
        let mut env = TestEnv::with_fixture();
        env.set_now("2026-11-10T10:00:00");
        let health = health("rmd");
        assert_eq!((&health["id"], &health["status"]), (&json!("rmd"), &json!("ok")));
        assert_eq!(health["version"], json!(env!("CARGO_PKG_VERSION")));
        assert!(health["summary"].as_str().unwrap().contains("open task"), "{health}");
        assert!(health.get("detail").is_none());

        env.write("user_config_0/settings.toml", "[settings\nbroken");
        let health = super::health("rmd-dev");
        assert_eq!((&health["id"], &health["status"]), (&json!("rmd-dev"), &json!("degraded")));
        assert!(health["detail"].as_str().unwrap().starts_with("The schedule configuration does not load: "));
    }

    #[test]
    fn summary_names_tasks_suggestions_and_the_current_event() {
        let snapshot =
            json!({"tasks": [1], "suggestions": [], "schedule": {"current": null, "next": "Lunch at 12:00"}});
        assert_eq!(summary(&snapshot), "1 open task · next: Lunch at 12:00");
        let snapshot = json!({"tasks": [1, 2], "suggestions": [1, 2, 3], "schedule": {"current": "Deep work at 09:00", "next": "Lunch at 12:00"}});
        assert_eq!(summary(&snapshot), "2 open tasks · 3 suggestions to review · now: Deep work at 09:00");
        assert_eq!(summary(&json!({})), "0 open tasks");
    }
}
