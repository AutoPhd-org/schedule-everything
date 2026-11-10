//! The browser's side of plugins (see `crate::plugins`): add, change, and
//! remove plugins, save their data, and log what was done. A logged session
//! can also count as a finished task of the plugin's type, so it shows in the
//! activity history like every other task.

use std::collections::HashSet;

use chrono::{NaiveDateTime, Utc};
use serde_json::{Value, json};

use crate::data::{load_task_log, save_task_log};
use crate::plugins::{
    self, DESCRIPTION_LIMIT, ENTRY_LIMIT, ICON_LIMIT, KINDS, NAME_LIMIT, SETTINGS, STATE_LIMIT, find, id_of, json_size,
    load_plugins, plugin_for_type, plugin_row, save_plugins, type_of,
};
use crate::py::{self, Dict};
use crate::web::services::{
    WebError, WebResult, invalid, load_task_types, os_error, py_strip, require_priority, require_text, task_type_save,
};

fn save_or_error(items: &[Dict]) -> WebResult<()> {
    save_plugins(items).map_err(|e| {
        WebError::new("storage_error", format!("failed to save plugins: {}", os_error(&e, &plugins::plugins_path())))
    })
}

/// The plugin `payload["id"]` names.
fn plugin_index(items: &[Dict], payload: &Dict) -> WebResult<usize> {
    let id = require_text(payload, "id")?;
    find(items, &id).ok_or_else(|| WebError::new("not_found", format!("plugin not found: {id}")))
}

/// Optional one-line text, at most `limit` characters.
fn optional_text(payload: &Dict, key: &str, limit: usize) -> WebResult<Option<String>> {
    match payload.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => {
            let text = py_strip(text).to_string();
            if text.chars().count() > limit {
                return Err(invalid(format!("{key} can be at most {limit} characters.")));
            }
            Ok(Some(text))
        }
        Some(_) => Err(invalid(format!("{key} must be text."))),
    }
}

/// The settings a payload changes, each true or false.
fn require_settings(payload: &Dict) -> WebResult<Dict> {
    let Some(raw) = payload.get("settings") else { return Ok(Dict::new()) };
    let Value::Object(raw) = raw else { return Err(invalid("settings must be an object.")) };
    for (key, value) in raw {
        if !SETTINGS.contains(&key.as_str()) {
            return Err(invalid(format!("unknown plugin setting: {key}")));
        }
        if !value.is_boolean() {
            return Err(invalid(format!("the setting {key} is either on or off: use true or false.")));
        }
    }
    Ok(raw.clone())
}

/// Change the settings `changes` names; the others keep their value.
fn apply_settings(plugin: &mut Dict, changes: Dict) {
    let mut settings = plugin.get("settings").and_then(Value::as_object).cloned().unwrap_or_default();
    settings.extend(changes);
    plugin.insert("settings".into(), Value::Object(settings));
}

/// A state object from a payload, within the size limit.
fn require_state(value: &Value) -> WebResult<Dict> {
    let Value::Object(state) = value else { return Err(invalid("state must be an object.")) };
    if json_size(value) > STATE_LIMIT {
        return Err(invalid(format!("a plugin's data can be at most {} KB.", STATE_LIMIT / 1000)));
    }
    Ok(state.clone())
}

/// The type a plugin owns: the type `wanted` names when no other plugin owns
/// it, else a type with the plugin's name, which is added when there is none.
fn claim_type(items: &[Dict], skip: Option<usize>, wanted: Option<&str>, name: &str) -> WebResult<String> {
    let task_types = load_task_types();
    let free = |type_id: &str| match plugin_for_type(items, type_id) {
        None => true,
        Some(owner) => Some(owner) == skip,
    };
    if let Some(type_id) = wanted.filter(|id| !id.is_empty()) {
        if !task_types.contains_key(type_id) {
            return Err(invalid(format!("unknown task type: {type_id}")));
        }
        if !free(type_id) {
            return Err(WebError::new("duplicate", "Another plugin already uses this type."));
        }
        return Ok(type_id.to_string());
    }
    let folded = py::casefold(name);
    if let Some((type_id, _)) =
        task_types.iter().find(|(id, value)| py::casefold(&py::py_str(value)) == folded && free(id))
    {
        return Ok(type_id.clone());
    }
    let mut request = Dict::new();
    let mut candidate = name.to_string();
    for n in 2.. {
        request.insert("name".into(), json!(candidate));
        match task_type_save(&request) {
            Ok(created) => return Ok(py::py_str(&created["id"])),
            Err(error) if error.code == "duplicate" && n < 50 => candidate = format!("{name} {n}"),
            Err(error) => return Err(error),
        }
    }
    unreachable!("the loop returns")
}

/// Add a plugin (`kind`, `name`, and optionally `description`, `icon`,
/// `settings`, a starting `state`, and an existing `type` to take over), or
/// change the plugin with `id`: its name (its type's name follows), its
/// description, icon, and settings.
pub fn plugin_save(payload: &Dict) -> WebResult {
    let name = require_text(payload, "name")?;
    if name.chars().count() > NAME_LIMIT {
        return Err(invalid(format!("plugin names can be at most {NAME_LIMIT} characters.")));
    }
    let description = optional_text(payload, "description", DESCRIPTION_LIMIT)?;
    let icon = optional_text(payload, "icon", ICON_LIMIT)?;
    let settings = require_settings(payload)?;
    let mut items = load_plugins();
    let existing = match payload.get("id").and_then(Value::as_str).map(py_strip).filter(|id| !id.is_empty()) {
        Some(id) => {
            Some(find(&items, id).ok_or_else(|| WebError::new("not_found", format!("plugin not found: {id}")))?)
        }
        None => None,
    };
    let index = match existing {
        Some(index) => {
            let task_types = load_task_types();
            let owned = type_of(&items[index]).filter(|type_id| task_types.contains_key(type_id));
            let type_id = match owned {
                // A renamed plugin renames its type, so the board shows the same name.
                Some(type_id) => {
                    if task_types.get(&type_id).map(py::py_str).as_deref() != Some(name.as_str()) {
                        let mut request = Dict::new();
                        request.insert("id".into(), json!(type_id));
                        request.insert("name".into(), json!(name));
                        task_type_save(&request)?;
                    }
                    type_id
                }
                // Its type was removed outside the board: it gets one again.
                None => claim_type(&items, Some(index), None, &name)?,
            };
            items[index].insert("type".into(), json!(type_id));
            index
        }
        None => {
            let kind = require_text(payload, "kind")?;
            if !KINDS.contains(&kind.as_str()) {
                return Err(invalid(format!("unknown plugin kind: {kind}")));
            }
            let state = match payload.get("state") {
                None | Some(Value::Null) => Dict::new(),
                Some(value) => require_state(value)?,
            };
            let wanted = payload.get("type").filter(|t| !t.is_null()).map(py::py_str);
            let type_id = claim_type(&items, None, wanted.as_deref().map(py_strip), &name)?;
            // A type taken over goes by the plugin's name from now on, as a renamed plugin's type does.
            if load_task_types().get(&type_id).map(py::py_str).as_deref() != Some(name.as_str()) {
                let mut request = Dict::new();
                request.insert("id".into(), json!(type_id));
                request.insert("name".into(), json!(name));
                task_type_save(&request)?;
            }
            let taken: Vec<String> = items.iter().map(id_of).collect();
            let mut plugin = Dict::new();
            plugin.insert("id".into(), json!(plugins::new_id(&name, &kind, &taken)));
            plugin.insert("kind".into(), json!(kind));
            plugin.insert("type".into(), json!(type_id));
            plugin.insert("created_at".into(), json!(py::utc_now_isoformat()));
            plugin.insert("state".into(), Value::Object(state));
            plugin.insert("log".into(), json!([]));
            items.push(plugin);
            items.len() - 1
        }
    };
    let plugin = &mut items[index];
    plugin.insert("name".into(), json!(name));
    if existing.is_none() || payload.contains_key("description") {
        plugin.insert("description".into(), json!(description.unwrap_or_default()));
    }
    if let Some(icon) = icon {
        plugin.insert("icon".into(), json!(icon));
    }
    apply_settings(plugin, settings);
    save_or_error(&items)?;
    Ok(json!({"plugin": plugin_row(&items[index], &load_task_types()), "created": existing.is_none()}))
}

/// Remove a plugin and its data. Its type and tasks stay, as a plain type.
/// The result carries an `undo` token for `plugin_restore`.
pub fn plugin_delete(payload: &Dict) -> WebResult {
    let mut items = load_plugins();
    let index = plugin_index(&items, payload)?;
    let removed = items.remove(index);
    save_or_error(&items)?;
    Ok(json!({"id": id_of(&removed), "undo": {"plugin": removed, "index": index}}))
}

/// Undo `plugin_delete` with the `undo` token it returned.
pub fn plugin_restore(payload: &Dict) -> WebResult {
    let Some(Value::Object(plugin)) = payload.get("plugin") else {
        return Err(invalid("plugin is required."));
    };
    let id = id_of(plugin);
    if id.is_empty() || !KINDS.contains(&plugins::kind_of(plugin)) {
        return Err(invalid("this is not a plugin."));
    }
    let mut items = load_plugins();
    if find(&items, &id).is_some() {
        return Err(WebError::new("duplicate", format!("the plugin {id} is already there.")));
    }
    if let Some(type_id) = type_of(plugin)
        && plugin_for_type(&items, &type_id).is_some()
    {
        return Err(WebError::new("duplicate", "Another plugin already uses this type."));
    }
    let index = payload.get("index").and_then(Value::as_u64).map(|i| i as usize).unwrap_or(items.len());
    items.insert(index.min(items.len()), plugin.clone());
    save_or_error(&items)?;
    Ok(json!({"plugin": plugin_row(plugin, &load_task_types())}))
}

/// Change parts of a plugin's data: each key in `set` replaces that part of
/// its state, and a null removes it.
pub fn plugin_state(payload: &Dict) -> WebResult {
    let mut items = load_plugins();
    let index = plugin_index(&items, payload)?;
    let Some(Value::Object(changes)) = payload.get("set") else {
        return Err(invalid("set must be an object."));
    };
    let mut state = items[index].get("state").and_then(Value::as_object).cloned().unwrap_or_default();
    for (key, value) in changes {
        if value.is_null() {
            state.shift_remove(key);
        } else {
            state.insert(key.clone(), value.clone());
        }
    }
    let state = Value::Object(require_state(&Value::Object(state))?);
    items[index].insert("state".into(), state.clone());
    save_or_error(&items)?;
    Ok(json!({"id": id_of(&items[index]), "state": state}))
}

/// A local time `YYYY-MM-DDTHH:MM[:SS]` from a payload.
fn local_time(payload: &Dict, key: &str) -> WebResult<Option<NaiveDateTime>> {
    match payload.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if py_strip(text).is_empty() => Ok(None),
        Some(Value::String(text)) => ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"]
            .iter()
            .find_map(|format| py::strptime_datetime(py_strip(text), format))
            .map(Some)
            .ok_or_else(|| invalid(format!("{key} must be a local date and time like 2026-11-09T18:30."))),
        Some(_) => Err(invalid(format!("{key} must be a local date and time like 2026-11-09T18:30."))),
    }
}

fn utc_stamp(local: NaiveDateTime) -> String {
    py::isoformat_aware(&crate::clock::local_from_naive(local).with_timezone(&Utc))
}

/// Write a finished task to the task history: added at `started`, completed at `ended`.
fn log_finished_task(task: &Value, started: NaiveDateTime, ended: NaiveDateTime) -> WebResult<Vec<String>> {
    let mut entries = load_task_log();
    let stamps = [utc_stamp(started), utc_stamp(ended)];
    for (action, stamp) in ["added", "deleted"].iter().zip(&stamps) {
        entries.push(json!({"timestamp": stamp, "action": action, "task": task}));
    }
    save_task_log(&entries).map_err(|e| {
        let path = crate::paths::runtime().task_log_path;
        WebError::new("storage_error", format!("failed to save the task log: {}", os_error(&e, &path)))
    })?;
    Ok(stamps.to_vec())
}

/// Log what was done: an entry of `kind` with its `data`, at `at` (local time,
/// now by default). With `asTask`, it also counts as a finished task of the
/// plugin's type, named `description` (importance `priority`, 5 by default),
/// running from `startedAt` to `at`, so it shows in the activity history.
/// The result carries an `undo` token for `plugin_log_delete`.
pub fn plugin_log(payload: &Dict) -> WebResult {
    let mut items = load_plugins();
    let index = plugin_index(&items, payload)?;
    let kind = require_text(payload, "kind")?;
    if kind.chars().count() > 30 {
        return Err(invalid("kind can be at most 30 characters."));
    }
    let data = payload.get("data").cloned().unwrap_or(Value::Null);
    let ended = local_time(payload, "at")?.unwrap_or_else(crate::clock::now);
    if ended > crate::clock::now() + chrono::Duration::minutes(5) {
        return Err(invalid("you can only log what was already done."));
    }
    let as_task = payload.get("asTask") == Some(&Value::Bool(true));
    let description = if as_task { Some(require_text(payload, "description")?) } else { None };
    let at = ended.format("%Y-%m-%dT%H:%M:%S").to_string();
    let entry =
        plugins::append_entry(&mut items, index, &kind, description.as_deref(), data, Some(at)).map_err(invalid)?;
    let entry_id = py::py_str(&entry["id"]);
    let mut logged = Vec::new();
    if let Some(description) = description {
        let priority = if payload.contains_key("priority") { require_priority(payload)? } else { 5 };
        let started = local_time(payload, "startedAt")?.filter(|start| *start <= ended).unwrap_or(ended);
        let Some(type_id) = type_of(&items[index]) else {
            return Err(invalid("this plugin has no task type."));
        };
        let task = json!({
            "description": description,
            "priority": priority,
            "type": type_id,
            "plugin": {"id": id_of(&items[index]), "entry": entry_id},
        });
        save_or_error(&items)?;
        logged = log_finished_task(&task, started, ended)?;
    } else {
        save_or_error(&items)?;
    }
    let id = id_of(&items[index]);
    Ok(json!({"entry": entry, "undo": {"id": id, "entry": entry_id, "logged": logged}}))
}

/// Remove a log entry (`id`, `entry`), and the history entries a logged task
/// wrote (`logged`, the stamps `plugin_log` returned).
pub fn plugin_log_delete(payload: &Dict) -> WebResult {
    let mut items = load_plugins();
    let index = plugin_index(&items, payload)?;
    let entry_id = require_text(payload, "entry")?;
    let plugin_id = id_of(&items[index]);
    if !plugins::remove_entry(&mut items[index], &entry_id) {
        return Err(WebError::new("not_found", "This entry is no longer in the log."));
    }
    save_or_error(&items)?;
    let logged: HashSet<&str> = match payload.get("logged") {
        Some(Value::Array(stamps)) => stamps.iter().filter_map(Value::as_str).collect(),
        _ => HashSet::new(),
    };
    // Also the history of a task finished from this entry, found by the entry it names.
    let entries = load_task_log();
    let kept: Vec<Value> = entries
        .iter()
        .filter(|entry| {
            let link = entry.get("task").and_then(|task| task.get("plugin"));
            let linked = link.and_then(|link| link.get("id")).and_then(Value::as_str) == Some(plugin_id.as_str())
                && link.and_then(|link| link.get("entry")).and_then(Value::as_str) == Some(entry_id.as_str());
            let stamped = entry.get("timestamp").and_then(Value::as_str).is_some_and(|s| logged.contains(s));
            !(linked && (stamped || logged.is_empty()))
        })
        .cloned()
        .collect();
    if kept.len() != entries.len() {
        save_task_log(&kept).map_err(|e| {
            let path = crate::paths::runtime().task_log_path;
            WebError::new("storage_error", format!("failed to save the task log: {}", os_error(&e, &path)))
        })?;
    }
    Ok(json!({"id": plugin_id, "entry": entry_id, "removedHistory": entries.len() - kept.len()}))
}

// =============================================================================
// Tasks of a plugin's type
// =============================================================================

/// A task's plugin payload: an object within the size limit, or `None`.
pub(crate) fn require_task_payload(value: Option<&Value>) -> WebResult<Option<Value>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value @ Value::Object(_)) => {
            if json_size(value) > ENTRY_LIMIT {
                return Err(invalid(format!("a task's plugin data can be at most {} KB.", ENTRY_LIMIT / 1000)));
            }
            Ok(Some(value.clone()))
        }
        Some(_) => Err(invalid("plugin must be an object.")),
    }
}

/// When a task of a plugin's type is completed, log it with the plugin:
/// `log` (`{"kind", "data"}`, such as the session the user just finished)
/// or else a `task` entry with the task's plugin payload and subtasks. Returns
/// `{"plugin", "entry"}` for the undo token, or `None` when no plugin owns the type.
pub(crate) fn log_completed_task(task: &Value, log: Option<&Value>) -> WebResult<Option<Value>> {
    let Some(type_id) = task.get("type").filter(|t| !t.is_null()).map(py::py_str) else {
        return Ok(None);
    };
    let mut items = load_plugins();
    let Some(index) = plugin_for_type(&items, &type_id) else {
        return Ok(None);
    };
    let description = task.get("description").and_then(Value::as_str);
    let (kind, data) = match log {
        Some(Value::Object(log)) => {
            let kind = log.get("kind").and_then(Value::as_str).map(py_strip).filter(|k| !k.is_empty());
            (
                kind.unwrap_or("task").chars().take(30).collect::<String>(),
                log.get("data").cloned().unwrap_or(Value::Null),
            )
        }
        Some(Value::Null) | None => {
            let mut data = Dict::new();
            if let Some(payload) = task.get("plugin").filter(|p| p.is_object()) {
                data.insert("plugin".into(), payload.clone());
            }
            if let Some(steps) = task.get("subtasks").filter(|s| s.as_array().is_some_and(|s| !s.is_empty())) {
                data.insert("subtasks".into(), steps.clone());
            }
            data.insert("priority".into(), task.get("priority").cloned().unwrap_or(Value::Null));
            ("task".to_string(), Value::Object(data))
        }
        Some(_) => return Err(invalid("pluginLog must be an object.")),
    };
    let entry = plugins::append_entry(&mut items, index, &kind, description, data, None).map_err(invalid)?;
    save_or_error(&items)?;
    Ok(Some(json!({"plugin": id_of(&items[index]), "entry": entry["id"]})))
}

/// Take back the log entry `log_completed_task` wrote, when the task is restored.
pub(crate) fn unlog_completed_task(link: Option<&Value>) -> WebResult<()> {
    let Some(Value::Object(link)) = link else { return Ok(()) };
    let (Some(plugin_id), Some(entry_id)) =
        (link.get("plugin").and_then(Value::as_str), link.get("entry").and_then(Value::as_str))
    else {
        return Ok(());
    };
    let mut items = load_plugins();
    if let Some(index) = find(&items, plugin_id)
        && plugins::remove_entry(&mut items[index], entry_id)
    {
        save_or_error(&items)?;
    }
    Ok(())
}

/// The plugin that owns a type, if any, by name (for messages).
pub(crate) fn owner_name(type_id: &str) -> Option<String> {
    let items = load_plugins();
    plugin_for_type(&items, type_id).map(|index| plugins::name_of(&items[index], &load_task_types()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::load_tasks;
    use crate::test_support::TestEnv;
    use crate::web::services::{status_snapshot, task_add, task_delete, task_restore, task_type_delete};

    const SETTINGS: &str = "[settings]\nlanguage = \"en\"\n\n[task_types]\n1 = \"Work\"\n2 = \"Gym\"\n";

    fn env() -> TestEnv {
        let mut env = TestEnv::new();
        env.set_now("2026-11-09T18:30:00");
        env.write("user_config_0/settings.toml", SETTINGS);
        env.write("tasks/tasks.json", "[]");
        env.write("tasks/tasks.log", "[]");
        env
    }

    fn payload(value: Value) -> Dict {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn a_new_plugin_takes_a_type_named_like_it_or_adds_one() {
        let _env = env();
        let gym = plugin_save(&payload(json!({"kind": "fitness", "name": "gym"}))).unwrap();
        assert_eq!(gym["plugin"]["type"], "2");
        // The type it takes over goes by the plugin's name.
        assert_eq!(gym["plugin"]["name"], "gym");
        assert_eq!(load_task_types()["2"], "gym");
        assert_eq!(gym["plugin"]["settings"], json!({"page": true, "activity": true, "timer": false, "agent": false}));
        assert_eq!(gym["created"], true);
        let guitar = plugin_save(&payload(json!({"kind": "tracker", "name": "Guitar", "description": "Scales and songs", "icon": "guitar", "state": {"metrics": []}, "settings": {"agent": true}}))).unwrap();
        assert_eq!(guitar["plugin"]["id"], "guitar");
        assert_eq!(guitar["plugin"]["type"], "3");
        assert_eq!(load_task_types()["3"], "Guitar");
        // A type another plugin owns cannot be taken.
        let error = plugin_save(&payload(json!({"kind": "tracker", "name": "Weights", "type": "2"}))).unwrap_err();
        assert_eq!(error.code, "duplicate");
        let error = plugin_save(&payload(json!({"kind": "chess", "name": "Chess"}))).unwrap_err();
        assert_eq!(error.message, "unknown plugin kind: chess");
    }

    #[test]
    fn renaming_a_plugin_renames_its_type_and_settings_change_one_by_one() {
        let _env = env();
        plugin_save(&payload(json!({"kind": "fitness", "name": "Gym"}))).unwrap();
        let saved =
            plugin_save(&payload(json!({"id": "gym", "name": "Training", "settings": {"timer": true}}))).unwrap();
        assert_eq!(saved["plugin"]["name"], "Training");
        assert_eq!(load_task_types()["2"], "Training");
        assert_eq!(saved["plugin"]["settings"]["timer"], true);
        assert_eq!(saved["plugin"]["settings"]["page"], true);
        let error =
            plugin_save(&payload(json!({"id": "gym", "name": "Gym", "settings": {"timer": "yes"}}))).unwrap_err();
        assert_eq!(error.code, "invalid_input");
        // The type cannot be deleted from the board while the plugin owns it.
        let error = task_type_delete(&payload(json!({"id": "2", "moveTo": "1"}))).unwrap_err();
        assert!(error.message.contains("Training"), "{}", error.message);
    }

    #[test]
    fn state_changes_part_by_part_and_delete_can_be_undone() {
        let _env = env();
        plugin_save(&payload(json!({"kind": "fitness", "name": "Gym", "state": {"goal": 3, "units": "kg"}}))).unwrap();
        let state =
            plugin_state(&payload(json!({"id": "gym", "set": {"goal": 4, "units": null, "routines": []}}))).unwrap();
        assert_eq!(state["state"], json!({"goal": 4, "routines": []}));
        let deleted = plugin_delete(&payload(json!({"id": "gym"}))).unwrap();
        assert!(load_plugins().is_empty());
        // The type stays as a plain type.
        assert_eq!(load_task_types()["2"], "Gym");
        plugin_restore(deleted["undo"].as_object().unwrap()).unwrap();
        assert_eq!(load_plugins()[0]["state"], json!({"goal": 4, "routines": []}));
        assert_eq!(plugin_restore(deleted["undo"].as_object().unwrap()).unwrap_err().code, "duplicate");
    }

    #[test]
    fn a_logged_session_can_count_as_a_finished_task_and_be_taken_back() {
        let _env = env();
        plugin_save(&payload(json!({"kind": "fitness", "name": "Gym"}))).unwrap();
        let logged = plugin_log(&payload(json!({
            "id": "gym", "kind": "session", "data": {"minutes": 45}, "asTask": true,
            "description": "Leg day", "priority": 6, "at": "2026-11-09T18:00", "startedAt": "2026-11-09T17:15",
        })))
        .unwrap();
        assert_eq!(logged["entry"]["at"], "2026-11-09T18:00:00");
        assert_eq!(logged["entry"]["task"], "Leg day");
        let history = crate::web::services::task_history(&payload(json!({"all": true}))).unwrap();
        let first = &history["activities"][0];
        assert_eq!(first["description"], "Leg day");
        assert_eq!(first["type"], "2");
        assert_eq!(first["startedAt"], "2026-11-09T17:15:00");
        assert_eq!(first["endedAt"], "2026-11-09T18:00:00");
        // A body measurement is not a task.
        plugin_log(&payload(json!({"id": "gym", "kind": "body", "data": {"weight": 72.4}}))).unwrap();
        assert_eq!(load_plugins()[0]["log"].as_array().unwrap().len(), 2);
        assert_eq!(
            plugin_log(&payload(json!({"id": "gym", "kind": "body", "at": "2026-11-10T09:00"}))).unwrap_err().code,
            "invalid_input"
        );

        plugin_log_delete(logged["undo"].as_object().unwrap()).unwrap();
        assert_eq!(load_plugins()[0]["log"].as_array().unwrap().len(), 1);
        let history = crate::web::services::task_history(&payload(json!({"all": true}))).unwrap();
        assert_eq!(history["activities"], json!([]));
    }

    #[test]
    fn agents_see_shared_plugins_and_suggest_for_them_with_plugin_data() {
        use crate::suggestions::{NewSuggestion, add_suggestion, suggest_context};
        let _env = env();
        plugin_save(&payload(json!({"kind": "tracker", "name": "Guitar practice", "description": "Fingerstyle", "settings": {"agent": true}}))).unwrap();
        plugin_save(&payload(json!({"kind": "tracker", "name": "Diary"}))).unwrap();
        let context = suggest_context();
        let shared = context["plugins"].as_array().unwrap();
        assert_eq!(shared.len(), 1, "a plugin is private from agents until shared");
        assert_eq!(shared[0]["description"], "Fingerstyle");
        assert_eq!(shared[0]["typeName"], "Guitar practice");

        let data = json!({"piece": "Blackbird"});
        let new = NewSuggestion {
            description: "Learn the bridge".into(),
            priority: 6,
            task_type: Some("guitar-practice".into()),
            plugin_data: Some(data.clone()),
            ..Default::default()
        };
        let (item, _) = add_suggestion(&new).unwrap();
        assert_eq!(item["type"], "3", "a plugin's ID names its type");
        let bad = NewSuggestion {
            description: "Bad".into(),
            priority: 5,
            plugin_data: Some(json!([1])),
            ..Default::default()
        };
        assert!(add_suggestion(&bad).unwrap_err().to_string().contains("JSON object"));
        assert_eq!(suggest_context()["pending"][0]["plugin"], data);

        let id = py::py_str(&item["id"]);
        crate::web::suggestions::suggestion_accept(&payload(
            json!({"id": id, "subtasks": [{"text": "Bars 9–12", "done": false}]}),
        ))
        .unwrap();
        let task = &load_tasks()[0];
        assert_eq!(task["plugin"], data);
        assert_eq!(task["subtasks"][0]["text"], "Bars 9–12");
    }

    #[test]
    fn completing_a_plugin_task_logs_it_with_the_plugin_until_it_is_restored() {
        let _env = env();
        plugin_save(&payload(json!({"kind": "fitness", "name": "Gym"}))).unwrap();
        let workout =
            json!({"workout": {"name": "Push", "units": [{"exercise": "bench", "sets": 3, "reps": 8, "weight": 60}]}});
        task_add(&payload(json!({"description": "Push day", "priority": 6, "type": "2", "plugin": workout, "subtasks": [{"text": "Bench press 3×8 @ 60 kg", "done": false}]}))).unwrap();
        let task = &load_tasks()[0];
        assert_eq!(task["plugin"], workout);
        assert_eq!(task["subtasks"][0]["text"], "Bench press 3×8 @ 60 kg");
        let snapshot = status_snapshot(&Dict::new()).unwrap();
        assert_eq!(snapshot["tasks"][0]["plugin"], workout);
        assert_eq!(snapshot["plugins"][0]["id"], "gym");

        let session = json!({"kind": "session", "data": {"minutes": 38, "units": []}});
        let done = task_delete(&payload(json!({"description": "Push day", "action": "deleted", "pluginLog": session})))
            .unwrap();
        let log = load_plugins()[0]["log"].as_array().unwrap().clone();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0]["kind"], "session");
        assert_eq!(log[0]["task"], "Push day");
        assert_eq!(done["undo"]["pluginEntry"], json!({"plugin": "gym", "entry": "1"}));
        task_restore(done["undo"].as_object().unwrap()).unwrap();
        assert!(load_plugins()[0]["log"].as_array().unwrap().is_empty());

        // Completed from the board, it is logged as a task with its plan; cancelled, it is not logged.
        task_delete(&payload(json!({"description": "Push day", "action": "deleted"}))).unwrap();
        let log = load_plugins()[0]["log"].as_array().unwrap().clone();
        assert_eq!(log[0]["kind"], "task");
        assert_eq!(log[0]["data"]["plugin"], workout);
        task_add(&payload(json!({"description": "Pull day", "priority": 5, "type": "2"}))).unwrap();
        task_delete(&payload(json!({"description": "Pull day", "action": "cancelled"}))).unwrap();
        assert_eq!(load_plugins()[0]["log"].as_array().unwrap().len(), 1);
    }
}
