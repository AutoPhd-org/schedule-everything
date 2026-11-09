//! The board's side of agent suggestions (see `crate::suggestions`): show the
//! pending ones, and let the user accept one (it becomes a normal task, as
//! suggested or as they adjusted it), reject it, or take either back.

use std::collections::HashSet;

use serde_json::{Value, json};

use crate::data::{load_task_log, load_tasks, save_task_log};
use crate::py::{self, Dict};
use crate::suggestions::{
    ACCEPTED, PENDING, SuggestionError, description_of, find_pending, id_of, load_suggestions, pending_rows,
    save_suggestions, status_of,
};
use crate::web::services::{
    WebError, WebResult, default_task_type, invalid, iso_date, load_task_types, log_task_or_error, os_error,
    require_due, require_priority, require_text, save_tasks_or_error, today,
};

fn web_error(error: SuggestionError) -> WebError {
    let message = error.to_string();
    let code = match error {
        SuggestionError::Invalid(_) => "invalid_input",
        SuggestionError::Conflict(_) | SuggestionError::Rejected { .. } => "duplicate",
        SuggestionError::NotFound(_) => "not_found",
        SuggestionError::Storage(_) => "storage_error",
    };
    WebError::new(code, message)
}

fn is_task(task: &Value, description: &str) -> bool {
    task.get("description").and_then(Value::as_str) == Some(description)
}

/// The suggestions waiting for review, for the status snapshot.
pub fn snapshot_rows() -> Vec<Value> {
    pending_rows(&load_suggestions(), &load_task_types())
}

/// The pending suggestion that `payload["id"]` names.
fn pending_index(items: &[Dict], payload: &Dict) -> WebResult<usize> {
    let id = require_text(payload, "id")?;
    find_pending(items, &id)
        .ok_or_else(|| WebError::new("not_found", "This suggestion is no longer waiting for review."))
}

fn decide(item: &mut Dict, status: &str) {
    item.insert("status".into(), json!(status));
    item.insert("decided_at".into(), json!(py::utc_now_isoformat()));
}

/// Accept a suggestion: it joins the task list as a normal task. The payload
/// may adjust its `description`, `priority`, `type`, or `due` first. The
/// result carries an `undo` token for `suggestion_undo`.
pub fn suggestion_accept(payload: &Dict) -> WebResult {
    let mut items = load_suggestions();
    let index = pending_index(&items, payload)?;
    let suggestion = items[index].clone();
    let task_types = load_task_types();

    let description = if payload.contains_key("description") {
        require_text(payload, "description")?
    } else {
        description_of(&suggestion)
    };
    let priority = if payload.contains_key("priority") {
        require_priority(payload)?
    } else {
        suggestion.get("priority").and_then(py::py_int).unwrap_or(5).clamp(1, 10)
    };
    let task_type = match payload.get("type").filter(|value| !value.is_null()) {
        Some(raw) => {
            let task_type = py::py_str(raw).trim().to_string();
            if !task_types.contains_key(&task_type) {
                return Err(invalid(format!("unknown task type: {task_type}")));
            }
            task_type
        }
        // A type deleted since the suggestion was made falls back to the default.
        None => suggestion
            .get("type")
            .map(py::py_str)
            .filter(|task_type| task_types.contains_key(task_type))
            .unwrap_or_else(|| default_task_type(&task_types)),
    };
    let due = if payload.contains_key("due") {
        require_due(payload)?
    } else {
        suggestion.get("due").and_then(Value::as_str).map(str::to_string)
    };
    let alarm_from =
        suggestion.get("alarm_from").and_then(Value::as_str).filter(|date| *date > iso_date(today()).as_str());

    let mut tasks = load_tasks();
    if tasks.iter().any(|task| is_task(task, &description)) {
        return Err(WebError::new("duplicate", format!("“{description}” is already on your list.")));
    }
    let mut task = Dict::new();
    task.insert("description".into(), json!(description));
    task.insert("priority".into(), json!(priority));
    task.insert("type".into(), json!(task_type));
    if let Some(alarm_from) = alarm_from {
        task.insert("alarm_from".into(), json!(alarm_from));
    }
    if let Some(due) = due {
        task.insert("due".into(), json!(due));
    }
    let task = Value::Object(task);
    tasks.push(task.clone());
    save_tasks_or_error(&tasks)?;

    let id = id_of(&suggestion);
    let origin = json!({"id": id, "agent": suggestion.get("agent"), "source": suggestion.get("source")});
    let entry = log_task_or_error("added", &task, Some(&json!({"suggestion": origin})))?;
    decide(&mut items[index], ACCEPTED);
    if description != description_of(&suggestion) {
        items[index].insert("accepted_as".into(), json!(description));
    }
    save_suggestions(&items).map_err(web_error)?;
    Ok(json!({
        "task": task,
        "undo": {"id": id, "logged": entry.get("timestamp").into_iter().collect::<Vec<_>>()},
    }))
}

/// Reject a suggestion: it leaves the board, and agents see it was turned down.
pub fn suggestion_reject(payload: &Dict) -> WebResult {
    let mut items = load_suggestions();
    let index = pending_index(&items, payload)?;
    decide(&mut items[index], crate::suggestions::REJECTED);
    save_suggestions(&items).map_err(web_error)?;
    let id = id_of(&items[index]);
    Ok(json!({"id": id, "description": description_of(&items[index]), "undo": {"id": id}}))
}

/// Take back an accept or a reject with the `undo` token it returned: the
/// suggestion waits for review again, and an accepted one leaves the task list
/// along with the history entry it wrote.
pub fn suggestion_undo(payload: &Dict) -> WebResult {
    let id = require_text(payload, "id")?;
    let mut items = load_suggestions();
    let Some(index) = items.iter().position(|item| id_of(item) == id && status_of(item) != PENDING) else {
        return Err(WebError::new("not_found", "There is no decision on this suggestion to take back."));
    };
    if status_of(&items[index]) == ACCEPTED {
        let description = items[index]
            .get("accepted_as")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| description_of(&items[index]));
        let mut tasks = load_tasks();
        let Some(at) = tasks.iter().rposition(|task| is_task(task, &description)) else {
            return Err(WebError::new("not_found", format!("“{description}” is no longer on your list.")));
        };
        tasks.remove(at);
        save_tasks_or_error(&tasks)?;
        let logged: HashSet<&str> = match payload.get("logged") {
            Some(Value::Array(stamps)) => stamps.iter().filter_map(Value::as_str).collect(),
            _ => HashSet::new(),
        };
        if !logged.is_empty() {
            let entries = load_task_log();
            let kept: Vec<Value> = entries
                .iter()
                .filter(|entry| {
                    let stamped = entry.get("timestamp").and_then(Value::as_str).is_some_and(|s| logged.contains(s));
                    let added = entry.get("action").and_then(Value::as_str) == Some("added");
                    let same = entry.get("task").is_some_and(|task| is_task(task, &description));
                    !(stamped && added && same)
                })
                .cloned()
                .collect();
            if kept.len() != entries.len() {
                save_task_log(&kept).map_err(|e| {
                    let path = crate::paths::runtime().task_log_path;
                    WebError::new("storage_error", format!("failed to save the task log: {}", os_error(&e, &path)))
                })?;
            }
        }
    }
    let item = &mut items[index];
    item.insert("status".into(), json!(PENDING));
    item.shift_remove("decided_at");
    item.shift_remove("accepted_as");
    let description = description_of(item);
    save_suggestions(&items).map_err(web_error)?;
    Ok(json!({"id": id, "description": description}))
}

/// Move pending suggestions from a deleted type to another one.
pub(crate) fn retype_pending(from: &str, to: &str) -> WebResult<usize> {
    let mut items = load_suggestions();
    let mut moved = 0;
    for item in items.iter_mut().filter(|item| status_of(item) == PENDING) {
        if item.get("type").is_some_and(|t| py::py_str(t) == from) {
            item.insert("type".into(), json!(to));
            moved += 1;
        }
    }
    if moved > 0 {
        save_suggestions(&items).map_err(web_error)?;
    }
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggestions::{NewSuggestion, add_suggestion};
    use crate::test_support::TestEnv;

    const SETTINGS: &str = "[settings]\nlanguage = \"en\"\n\n[task_types]\n1 = \"Work\"\n2 = \"Errands\"\n";

    fn env() -> TestEnv {
        let mut env = TestEnv::new();
        env.set_now("2026-11-09T08:00:00");
        env.write("user_config_0/settings.toml", SETTINGS);
        env.write("tasks/tasks.json", r#"[{"description": "Write report", "priority": 7, "type": "1"}]"#);
        env.write("tasks/tasks.log", "[]");
        env
    }

    fn payload(value: Value) -> Dict {
        value.as_object().cloned().unwrap()
    }

    fn suggest(description: &str, extra: NewSuggestion) -> String {
        let new = NewSuggestion { description: description.into(), priority: 6, ..extra };
        py::py_str(&add_suggestion(&new).unwrap().0["id"])
    }

    fn file(env: &TestEnv, relative: &str) -> Value {
        serde_json::from_str(&env.read(relative)).unwrap()
    }

    #[test]
    fn accepting_makes_a_normal_task_and_undo_takes_it_back() {
        let env = env();
        let id = suggest(
            "Reply to Prof. Lee",
            NewSuggestion {
                task_type: Some("2".into()),
                due: Some("2026-11-12T17:00".into()),
                postpone: Some(2),
                source: Some("Gmail".into()),
                agent: Some("Claude".into()),
                ..Default::default()
            },
        );
        assert_eq!(snapshot_rows().len(), 1);
        let result = suggestion_accept(&payload(json!({"id": id}))).unwrap();
        let task = json!({"description": "Reply to Prof. Lee", "priority": 6, "type": "2", "alarm_from": "2026-11-11", "due": "2026-11-12T17:00"});
        assert_eq!(result["task"], task);
        assert_eq!(file(&env, "tasks/tasks.json")[1], task);
        let log = file(&env, "tasks/tasks.log");
        assert_eq!(log[0]["action"], "added");
        assert_eq!(log[0]["metadata"], json!({"suggestion": {"id": id, "agent": "Claude", "source": "Gmail"}}));
        assert_eq!(file(&env, "tasks/suggestions.json")[0]["status"], "accepted");
        assert!(snapshot_rows().is_empty());
        assert_eq!(code(suggestion_accept(&payload(json!({"id": id})))), "not_found");

        suggestion_undo(&payload(result["undo"].clone())).unwrap();
        assert_eq!(file(&env, "tasks/tasks.json").as_array().unwrap().len(), 1);
        assert_eq!(file(&env, "tasks/tasks.log"), json!([]));
        let saved = &file(&env, "tasks/suggestions.json")[0];
        assert_eq!(saved["status"], "pending");
        assert!(saved.get("decided_at").is_none());
        assert_eq!(snapshot_rows().len(), 1);
        assert_eq!(code(suggestion_undo(&payload(json!({"id": id})))), "not_found");
    }

    #[test]
    fn accepting_with_adjustments() {
        let env = env();
        let id = suggest("Buy stamps", NewSuggestion::default());
        let adjusted = json!({"id": id, "description": "Buy stamps and cards", "priority": 3, "type": "2", "due": ""});
        let result = suggestion_accept(&payload(adjusted)).unwrap();
        assert_eq!(result["task"], json!({"description": "Buy stamps and cards", "priority": 3, "type": "2"}));
        assert_eq!(file(&env, "tasks/suggestions.json")[0]["accepted_as"], "Buy stamps and cards");
        // Undo finds the task under the name it was accepted as.
        suggestion_undo(&payload(result["undo"].clone())).unwrap();
        assert_eq!(file(&env, "tasks/tasks.json").as_array().unwrap().len(), 1);
        assert!(file(&env, "tasks/suggestions.json")[0].get("accepted_as").is_none());
    }

    #[test]
    fn accept_refuses_bad_adjustments_and_duplicates() {
        let env = env();
        let id = suggest("Buy stamps", NewSuggestion::default());
        for (adjusted, expected) in [
            (json!({"id": id, "priority": 11}), "invalid_input"),
            (json!({"id": id, "type": "9"}), "invalid_input"),
            (json!({"id": id, "due": "soon"}), "invalid_input"),
            (json!({"id": id, "description": "Write report"}), "duplicate"),
            (json!({"id": "99"}), "not_found"),
            (json!({}), "invalid_input"),
        ] {
            assert_eq!(code(suggestion_accept(&payload(adjusted))), expected);
        }
        assert_eq!(file(&env, "tasks/tasks.json").as_array().unwrap().len(), 1);
        assert_eq!(file(&env, "tasks/suggestions.json")[0]["status"], "pending");
    }

    #[test]
    fn a_deleted_type_falls_back_to_the_default() {
        let env = env();
        env.write(
            "tasks/suggestions.json",
            r#"[{"id": "3", "description": "Old type", "priority": 4, "type": "8", "status": "pending", "alarm_from": "2026-11-01"}]"#,
        );
        let result = suggestion_accept(&payload(json!({"id": "3"}))).unwrap();
        // The type is gone, and a hold that already ended is not kept.
        assert_eq!(result["task"], json!({"description": "Old type", "priority": 4, "type": "1"}));
        drop(env);
    }

    #[test]
    fn rejecting_and_undoing_it() {
        let env = env();
        let id = suggest("Renew gym", NewSuggestion::default());
        let result = suggestion_reject(&payload(json!({"id": id}))).unwrap();
        assert_eq!(result["description"], "Renew gym");
        let saved = &file(&env, "tasks/suggestions.json")[0];
        assert_eq!(saved["status"], "rejected");
        assert!(saved["decided_at"].as_str().is_some());
        assert!(snapshot_rows().is_empty());
        assert_eq!(file(&env, "tasks/tasks.json").as_array().unwrap().len(), 1);
        // Agents are told not to suggest it again.
        let again =
            add_suggestion(&NewSuggestion { description: "Renew gym".into(), priority: 5, ..Default::default() });
        assert!(matches!(again, Err(SuggestionError::Rejected { .. })));

        suggestion_undo(&payload(result["undo"].clone())).unwrap();
        assert_eq!(snapshot_rows().len(), 1);
    }

    #[test]
    fn deleting_a_type_moves_its_suggestions() {
        let env = env();
        suggest("Buy stamps", NewSuggestion { task_type: Some("2".into()), ..Default::default() });
        assert_eq!(retype_pending("2", "1").unwrap(), 1);
        assert_eq!(file(&env, "tasks/suggestions.json")[0]["type"], "1");
        assert_eq!(retype_pending("2", "1").unwrap(), 0);
    }

    fn code(result: WebResult) -> String {
        result.unwrap_err().code
    }
}
