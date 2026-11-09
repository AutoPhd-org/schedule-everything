//! Tasks an AI agent suggests, waiting for the user's review
//! (`tasks/suggestions.json`).
//!
//! An agent, such as one the user runs on a schedule to look through their
//! mail and calendar, proposes tasks with `rmd suggest add`. A suggestion is
//! not a task yet: reminders, `rmd ls`, `rmd sync`, the workload and the
//! statistics never count it. The browser board shows it in its type's group,
//! styled apart from the user's own tasks, until the user accepts it (it
//! becomes a normal task, see `web::suggestions`) or rejects it. Agents can
//! withdraw what they suggested, but only the user decides.
//!
//! Decided suggestions stay in the file for [`KEEP_DECIDED_DAYS`] days, so an
//! agent can see what was turned down and not suggest it again.

use std::path::PathBuf;

use chrono::{Duration, NaiveDateTime};
use serde_json::{Value, json};

use crate::commands::history::parse_log_timestamp;
use crate::data::load_tasks;
use crate::py::{self, Dict};
use crate::web::services::{default_task_type, load_task_types};

pub const PENDING: &str = "pending";
pub const ACCEPTED: &str = "accepted";
pub const REJECTED: &str = "rejected";
/// The most suggestions that can wait for review at once, so a runaway agent
/// cannot bury the board.
pub const MAX_PENDING: usize = 20;
/// How long accepted and rejected suggestions are remembered.
pub const KEEP_DECIDED_DAYS: i64 = 60;
pub const DESCRIPTION_LIMIT: usize = 500;
pub const REASON_LIMIT: usize = 300;
/// Limit for the short labels: where a suggestion came from, and who made it.
pub const LABEL_LIMIT: usize = 40;
/// Who made a suggestion that does not say.
pub const DEFAULT_AGENT: &str = "AI agent";
const DUE_FORMATS: [&str; 3] = ["%Y-%m-%dT%H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M"];

/// Why a suggestion could not be saved or changed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SuggestionError {
    /// The request itself is wrong: a bad priority, an unknown type, a missing field.
    #[error("{0}")]
    Invalid(String),
    /// It clashes with what is there: the task is already on the list, or too many wait.
    #[error("{0}")]
    Conflict(String),
    /// The user turned the same task down on this date.
    #[error("The user rejected “{description}” on {date}.")]
    Rejected { description: String, date: String },
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Storage(String),
}

pub type SuggestionResult<T> = Result<T, SuggestionError>;

// =============================================================================
// Storage
// =============================================================================

/// `tasks/suggestions.json`, next to the task list.
pub fn suggestions_path() -> PathBuf {
    crate::paths::runtime().tasks_path.with_file_name("suggestions.json")
}

/// Every suggestion on file, pending and decided. A missing or unreadable file
/// gives none; entries without an ID or a description are skipped.
pub fn load_suggestions() -> Vec<Dict> {
    let Ok(text) = std::fs::read_to_string(suggestions_path()) else {
        return Vec::new();
    };
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter_map(|item| match item {
            Value::Object(item) if !id_of(&item).is_empty() && !description_of(&item).is_empty() => Some(item),
            _ => None,
        })
        .collect()
}

/// Save suggestions, forgetting decisions older than [`KEEP_DECIDED_DAYS`].
pub fn save_suggestions(items: &[Dict]) -> SuggestionResult<()> {
    let cutoff = crate::clock::now() - Duration::days(KEEP_DECIDED_DAYS);
    let kept: Vec<Value> = items
        .iter()
        .filter(|item| status_of(item) == PENDING || decided_at(item).is_none_or(|at| at >= cutoff))
        .map(|item| Value::Object(item.clone()))
        .collect();
    let path = suggestions_path();
    py::write_json_pretty(&path, &kept)
        .map_err(|e| SuggestionError::Storage(format!("failed to save suggestions to {}: {e}", path.display())))
}

pub fn id_of(item: &Dict) -> String {
    item.get("id").filter(|id| !id.is_null()).map(py::py_str).unwrap_or_default()
}

pub fn description_of(item: &Dict) -> String {
    item.get("description").and_then(Value::as_str).map(|d| d.trim().to_string()).unwrap_or_default()
}

/// `pending` (also for an entry without a status), `accepted`, or `rejected`.
pub fn status_of(item: &Dict) -> &str {
    item.get("status").and_then(Value::as_str).unwrap_or(PENDING)
}

fn decided_at(item: &Dict) -> Option<NaiveDateTime> {
    item.get("decided_at").and_then(parse_log_timestamp)
}

/// One past the highest numeric ID on file.
fn next_id(items: &[Dict]) -> String {
    let highest = items.iter().filter_map(|item| id_of(item).parse::<u64>().ok()).max().unwrap_or(0);
    (highest + 1).to_string()
}

/// Where the pending suggestion with this ID is.
pub fn find_pending(items: &[Dict], id: &str) -> Option<usize> {
    items.iter().position(|item| status_of(item) == PENDING && id_of(item) == id)
}

// =============================================================================
// Field validation
// =============================================================================

/// Trimmed text, with runs of whitespace (newlines too) made single spaces;
/// `None` when blank.
fn one_line(value: Option<&str>, limit: usize, what: &str) -> SuggestionResult<Option<String>> {
    let Some(text) = value.map(|v| v.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    if text.chars().count() > limit {
        return Err(SuggestionError::Invalid(format!("{what} can be at most {limit} characters.")));
    }
    Ok(Some(text))
}

/// A task type by ID, or by name ignoring case.
pub fn resolve_type(task_types: &Dict, value: &str) -> Option<String> {
    let value = value.trim();
    if task_types.contains_key(value) {
        return Some(value.to_string());
    }
    let lowered = value.to_lowercase();
    task_types.iter().find(|(_, name)| py::py_str(name).to_lowercase() == lowered).map(|(id, _)| id.clone())
}

/// A local finish-by time as `YYYY-MM-DDTHH:MM`.
pub fn parse_due(text: &str) -> SuggestionResult<String> {
    DUE_FORMATS
        .iter()
        .find_map(|format| py::strptime_datetime(text.trim(), format))
        .map(|due| due.format("%Y-%m-%dT%H:%M").to_string())
        .ok_or_else(|| SuggestionError::Invalid("due must be a local date and time like 2026-11-12T17:00.".into()))
}

// =============================================================================
// Suggesting and withdrawing (agents)
// =============================================================================

/// A task an agent proposes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewSuggestion {
    pub description: String,
    pub priority: i64,
    /// A task type ID or name; the default type when absent.
    pub task_type: Option<String>,
    /// Finish by this local time (`YYYY-MM-DDTHH:MM`): a timed task.
    pub due: Option<String>,
    /// Hold the daily urgent alarm for this many days.
    pub postpone: Option<i64>,
    /// Why the agent suggests it, in a sentence the user will read.
    pub reason: Option<String>,
    /// Where the agent found it, such as "Gmail" or "Calendar".
    pub source: Option<String>,
    /// Who suggests it, such as "Claude".
    pub agent: Option<String>,
    /// Suggest it even though the user rejected the same task before.
    pub again: bool,
}

/// Save a suggestion for the user to review. A pending suggestion with the
/// same description is replaced (keeping its ID), so an agent that runs again
/// does not suggest a task twice. Returns the saved suggestion and whether it
/// replaced one.
pub fn add_suggestion(new: &NewSuggestion) -> SuggestionResult<(Dict, bool)> {
    let description = new.description.trim().to_string();
    if description.is_empty() {
        return Err(SuggestionError::Invalid("a suggestion needs a description.".into()));
    }
    if description.chars().count() > DESCRIPTION_LIMIT {
        return Err(SuggestionError::Invalid(format!(
            "the description can be at most {DESCRIPTION_LIMIT} characters."
        )));
    }
    if !(1..=10).contains(&new.priority) {
        return Err(SuggestionError::Invalid("priority must be between 1 and 10.".into()));
    }
    let task_types = load_task_types();
    let type_id = match new.task_type.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        None => default_task_type(&task_types),
        Some(value) => resolve_type(&task_types, value).ok_or_else(|| {
            let choices =
                task_types.iter().map(|(id, name)| format!("{id}={}", py::py_str(name))).collect::<Vec<_>>().join(", ");
            SuggestionError::Invalid(format!("unknown task type: {value}. Choose from: {choices}"))
        })?,
    };
    let due = new.due.as_deref().filter(|d| !d.trim().is_empty()).map(parse_due).transpose()?;
    let alarm_from = match new.postpone {
        None | Some(0) => None,
        Some(days) if days < 0 => {
            return Err(SuggestionError::Invalid("postpone days must be a non-negative integer.".into()));
        }
        Some(days) => {
            let date = crate::clock::today()
                .checked_add_signed(Duration::days(days))
                .ok_or_else(|| SuggestionError::Invalid("postpone days is too large.".into()))?;
            Some(date.format("%Y-%m-%d").to_string())
        }
    };
    let reason = one_line(new.reason.as_deref(), REASON_LIMIT, "the reason")?;
    let source = one_line(new.source.as_deref(), LABEL_LIMIT, "the source")?;
    let agent = one_line(new.agent.as_deref(), LABEL_LIMIT, "the agent name")?;

    if load_tasks().iter().any(|task| task.get("description").and_then(Value::as_str) == Some(description.as_str())) {
        return Err(SuggestionError::Conflict(format!("“{description}” is already on the task list.")));
    }
    let mut items = load_suggestions();
    let existing = items.iter().position(|item| status_of(item) == PENDING && description_of(item) == description);
    if existing.is_none() {
        if !new.again
            && let Some(rejected) = items
                .iter()
                .filter(|item| status_of(item) == REJECTED && description_of(item) == description)
                .filter_map(decided_at)
                .max()
        {
            return Err(SuggestionError::Rejected { description, date: rejected.format("%Y-%m-%d").to_string() });
        }
        if items.iter().filter(|item| status_of(item) == PENDING).count() >= MAX_PENDING {
            return Err(SuggestionError::Conflict(format!(
                "{MAX_PENDING} suggestions are already waiting for review. Withdraw some, or wait until the user has reviewed them."
            )));
        }
    }

    let id = existing.map(|index| id_of(&items[index])).unwrap_or_else(|| next_id(&items));
    let mut item = Dict::new();
    item.insert("id".into(), json!(id));
    item.insert("description".into(), json!(description));
    item.insert("priority".into(), json!(new.priority));
    item.insert("type".into(), json!(type_id));
    if let Some(due) = due {
        item.insert("due".into(), json!(due));
    }
    if let Some(alarm_from) = alarm_from {
        item.insert("alarm_from".into(), json!(alarm_from));
    }
    if let Some(reason) = reason {
        item.insert("reason".into(), json!(reason));
    }
    if let Some(source) = source {
        item.insert("source".into(), json!(source));
    }
    item.insert("agent".into(), json!(agent.unwrap_or_else(|| DEFAULT_AGENT.to_string())));
    item.insert("status".into(), json!(PENDING));
    item.insert("suggested_at".into(), json!(py::utc_now_isoformat()));
    match existing {
        Some(index) => items[index] = item.clone(),
        None => items.push(item.clone()),
    }
    save_suggestions(&items)?;
    Ok((item, existing.is_some()))
}

/// Withdraw pending suggestions by ID or description; the user never sees
/// them. Returns the descriptions withdrawn and the identifiers that matched
/// no pending suggestion.
pub fn withdraw_suggestions(identifiers: &[String]) -> SuggestionResult<(Vec<String>, Vec<String>)> {
    let mut items = load_suggestions();
    let mut withdrawn = Vec::new();
    let mut missing = Vec::new();
    for identifier in identifiers {
        let wanted = identifier.trim();
        let matches =
            |item: &Dict| status_of(item) == PENDING && (id_of(item) == wanted || description_of(item) == wanted);
        let before = items.len();
        let mut taken = Vec::new();
        items.retain(|item| {
            let hit = matches(item);
            if hit {
                taken.push(description_of(item));
            }
            !hit
        });
        if items.len() == before {
            missing.push(identifier.clone());
        }
        withdrawn.extend(taken);
    }
    if !withdrawn.is_empty() {
        save_suggestions(&items)?;
    }
    Ok((withdrawn, missing))
}

// =============================================================================
// Rows for the browser and the agent
// =============================================================================

/// A suggestion as the browser and `rmd suggest ls --json` show it.
pub fn suggestion_row(item: &Dict, task_types: &Dict) -> Value {
    let text = |key: &str| item.get(key).and_then(Value::as_str).map(str::to_string);
    let type_id =
        item.get("type").filter(|t| !t.is_null()).map(py::py_str).unwrap_or_else(|| default_task_type(task_types));
    json!({
        "id": id_of(item),
        "description": description_of(item),
        "priority": item.get("priority").and_then(py::py_int).unwrap_or(5),
        "type": type_id,
        "typeName": task_types.get(&type_id).cloned().unwrap_or(Value::Null),
        "due": text("due"),
        "alarmFrom": text("alarm_from"),
        "reason": text("reason"),
        "source": text("source"),
        "agent": text("agent").unwrap_or_else(|| DEFAULT_AGENT.to_string()),
        "suggestedAt": text("suggested_at"),
        "status": status_of(item),
        "decidedAt": text("decided_at"),
    })
}

/// Pending suggestions, most important first, then in the order they came.
pub fn pending_rows(items: &[Dict], task_types: &Dict) -> Vec<Value> {
    let mut pending: Vec<&Dict> = items.iter().filter(|item| status_of(item) == PENDING).collect();
    pending.sort_by_key(|item| {
        let priority = item.get("priority").and_then(py::py_int).unwrap_or(5);
        (std::cmp::Reverse(priority), id_of(item).parse::<u64>().unwrap_or(u64::MAX))
    });
    pending.into_iter().map(|item| suggestion_row(item, task_types)).collect()
}

/// Everything an agent needs before suggesting: the user's profile, the task
/// types, the open tasks, what waits for review, and what the user decided lately.
pub fn suggest_context() -> Value {
    let task_types = load_task_types();
    let items = load_suggestions();
    let open_tasks: Vec<Value> = load_tasks()
        .iter()
        .filter_map(|task| {
            let task = task.as_object()?;
            let description = task.get("description")?.as_str()?.trim();
            (!description.is_empty()).then(|| {
                json!({
                    "description": description,
                    "priority": task.get("priority").and_then(py::py_int),
                    "type": task.get("type").filter(|t| !t.is_null()).map(py::py_str),
                })
            })
        })
        .collect();
    let mut decided: Vec<&Dict> = items.iter().filter(|item| status_of(item) != PENDING).collect();
    decided.sort_by_key(|item| std::cmp::Reverse(decided_at(item)));
    let profile = crate::paths::runtime().profile_path;
    json!({
        "profilePath": profile.exists().then(|| profile.display().to_string()),
        "taskTypes": task_types,
        "defaultType": default_task_type(&task_types),
        "openTasks": open_tasks,
        "pending": pending_rows(&items, &task_types),
        "decided": decided.into_iter().map(|item| suggestion_row(item, &task_types)).collect::<Vec<_>>(),
        "limits": {"maxPending": MAX_PENDING, "keepDecidedDays": KEEP_DECIDED_DAYS},
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    const SETTINGS: &str = "[settings]\nlanguage = \"en\"\n\n[task_types]\n1 = \"Work\"\n2 = \"Errands\"\n";

    fn env() -> TestEnv {
        let mut env = TestEnv::new();
        env.set_now("2026-11-09T08:00:00");
        env.write("user_config_0/settings.toml", SETTINGS);
        env.write("tasks/tasks.json", r#"[{"description": "Write report", "priority": 7, "type": "1"}]"#);
        env
    }

    fn suggestion(description: &str, priority: i64) -> NewSuggestion {
        NewSuggestion { description: description.into(), priority, ..Default::default() }
    }

    fn saved(env: &TestEnv) -> Vec<Value> {
        serde_json::from_str(&env.read("tasks/suggestions.json")).unwrap()
    }

    #[test]
    fn add_saves_a_pending_suggestion_with_defaults() {
        let env = env();
        let new = NewSuggestion {
            task_type: Some("errands".into()),
            due: Some("2026-11-12 17:00".into()),
            postpone: Some(1),
            reason: Some("  Prof. Lee asked\nfor comments by Thursday. ".into()),
            source: Some("Gmail".into()),
            agent: Some("Claude".into()),
            ..suggestion("Reply to Prof. Lee", 8)
        };
        let (item, replaced) = add_suggestion(&new).unwrap();
        assert!(!replaced);
        assert_eq!(item["id"], "1");
        let file = saved(&env);
        assert_eq!(file.len(), 1);
        assert_eq!(file[0]["type"], "2");
        assert_eq!(file[0]["due"], "2026-11-12T17:00");
        assert_eq!(file[0]["alarm_from"], "2026-11-10");
        assert_eq!(file[0]["reason"], "Prof. Lee asked for comments by Thursday.");
        assert_eq!(file[0]["source"], "Gmail");
        assert_eq!(file[0]["agent"], "Claude");
        assert_eq!(file[0]["status"], "pending");
        assert!(file[0]["suggested_at"].as_str().unwrap().ends_with("+00:00"));

        let (plain, _) = add_suggestion(&suggestion("Buy stamps", 3)).unwrap();
        assert_eq!(plain["id"], "2");
        assert_eq!(plain["type"], "1");
        assert_eq!(plain["agent"], DEFAULT_AGENT);
        assert!(plain.get("due").is_none() && plain.get("reason").is_none());
        // The task list itself is never touched.
        assert_eq!(env.read("tasks/tasks.json"), r#"[{"description": "Write report", "priority": 7, "type": "1"}]"#);
    }

    #[test]
    fn suggesting_again_replaces_the_pending_one() {
        let env = env();
        add_suggestion(&suggestion("Buy stamps", 3)).unwrap();
        let (item, replaced) = add_suggestion(&NewSuggestion {
            reason: Some("Cards go out Friday".into()),
            ..suggestion("Buy stamps", 6)
        })
        .unwrap();
        assert!(replaced);
        assert_eq!(item["id"], "1");
        let file = saved(&env);
        assert_eq!(file.len(), 1);
        assert_eq!(file[0]["priority"], 6);
        assert_eq!(file[0]["reason"], "Cards go out Friday");
    }

    #[test]
    fn add_refuses_bad_input_and_open_tasks() {
        let env = env();
        let invalid = |new: NewSuggestion| match add_suggestion(&new) {
            Err(SuggestionError::Invalid(message)) => message,
            other => panic!("expected invalid, got {other:?}"),
        };
        assert_eq!(invalid(suggestion("  ", 5)), "a suggestion needs a description.");
        assert_eq!(invalid(suggestion("x", 11)), "priority must be between 1 and 10.");
        assert_eq!(
            invalid(NewSuggestion { task_type: Some("gym".into()), ..suggestion("x", 5) }),
            "unknown task type: gym. Choose from: 1=Work, 2=Errands"
        );
        assert!(invalid(NewSuggestion { due: Some("Friday".into()), ..suggestion("x", 5) }).starts_with("due must be"));
        assert!(invalid(NewSuggestion { postpone: Some(-1), ..suggestion("x", 5) }).starts_with("postpone days"));
        assert!(invalid(NewSuggestion { reason: Some("r".repeat(301)), ..suggestion("x", 5) }).contains("300"));
        assert_eq!(
            add_suggestion(&suggestion("Write report", 5)),
            Err(SuggestionError::Conflict("“Write report” is already on the task list.".into()))
        );
        assert!(!env.root().join("tasks/suggestions.json").exists());
    }

    #[test]
    fn rejected_suggestions_are_not_suggested_again_unless_asked() {
        let env = env();
        env.write(
            "tasks/suggestions.json",
            r#"[{"id": "4", "description": "Renew gym", "priority": 4, "type": "1", "status": "rejected", "decided_at": "2026-11-03T18:00:00+00:00"}]"#,
        );
        assert_eq!(
            add_suggestion(&suggestion("Renew gym", 5)),
            Err(SuggestionError::Rejected { description: "Renew gym".into(), date: "2026-11-03".into() })
        );
        let (item, _) = add_suggestion(&NewSuggestion { again: true, ..suggestion("Renew gym", 5) }).unwrap();
        assert_eq!(item["id"], "5");
        let statuses: Vec<Value> = saved(&env).iter().map(|item| item["status"].clone()).collect();
        assert_eq!(statuses, [json!("rejected"), json!("pending")]);
    }

    #[test]
    fn pending_suggestions_are_capped() {
        let _env = env();
        for n in 0..MAX_PENDING {
            add_suggestion(&suggestion(&format!("Idea {n}"), 5)).unwrap();
        }
        assert!(matches!(add_suggestion(&suggestion("One more", 5)), Err(SuggestionError::Conflict(_))));
        // Updating one that already waits is still fine.
        add_suggestion(&suggestion("Idea 3", 9)).unwrap();
    }

    #[test]
    fn old_decisions_are_forgotten_on_save() {
        let env = env();
        env.write(
            "tasks/suggestions.json",
            r#"[
              {"id": "1", "description": "Old", "priority": 4, "status": "rejected", "decided_at": "2026-08-01T10:00:00+00:00"},
              {"id": "2", "description": "Recent", "priority": 4, "status": "accepted", "decided_at": "2026-11-01T10:00:00+00:00"},
              {"id": "3", "description": "Waiting", "priority": 4, "status": "pending", "suggested_at": "2026-06-01T10:00:00+00:00"},
              {"description": "No id"},
              "junk"
            ]"#,
        );
        add_suggestion(&suggestion("New", 5)).unwrap();
        let ids: Vec<Value> = saved(&env).iter().map(|item| item["id"].clone()).collect();
        assert_eq!(ids, [json!("2"), json!("3"), json!("4")]);
    }

    #[test]
    fn withdraw_by_id_or_description_only_touches_pending() {
        let env = env();
        add_suggestion(&suggestion("Buy stamps", 3)).unwrap();
        add_suggestion(&suggestion("Call plumber", 6)).unwrap();
        let mut items = load_suggestions();
        items[1].insert("status".into(), json!(REJECTED));
        save_suggestions(&items).unwrap();
        let (withdrawn, missing) =
            withdraw_suggestions(&["1".to_string(), "Call plumber".to_string(), "9".to_string()]).unwrap();
        assert_eq!(withdrawn, ["Buy stamps"]);
        assert_eq!(missing, ["Call plumber", "9"]);
        assert_eq!(saved(&env).len(), 1);
    }

    #[test]
    fn context_lists_types_tasks_pending_and_decided() {
        let env = env();
        env.write(
            "tasks/suggestions.json",
            r#"[
              {"id": "1", "description": "Low", "priority": 3, "type": "2", "status": "pending", "agent": "Claude"},
              {"id": "2", "description": "High", "priority": 9, "type": "7", "status": "pending"},
              {"id": "3", "description": "Done deal", "priority": 5, "type": "1", "status": "accepted", "decided_at": "2026-11-08T10:00:00+00:00"}
            ]"#,
        );
        let context = suggest_context();
        assert_eq!(context["profilePath"], Value::Null);
        assert_eq!(context["taskTypes"], json!({"1": "Work", "2": "Errands"}));
        assert_eq!(context["defaultType"], "1");
        assert_eq!(context["openTasks"], json!([{"description": "Write report", "priority": 7, "type": "1"}]));
        let pending: Vec<&str> =
            context["pending"].as_array().unwrap().iter().map(|row| row["description"].as_str().unwrap()).collect();
        assert_eq!(pending, ["High", "Low"]);
        assert_eq!(context["pending"][0]["typeName"], Value::Null);
        assert_eq!(context["pending"][0]["agent"], DEFAULT_AGENT);
        assert_eq!(context["pending"][1]["typeName"], "Errands");
        assert_eq!(context["decided"][0]["status"], "accepted");
        assert_eq!(context["limits"]["maxPending"], MAX_PENDING);
    }
}
