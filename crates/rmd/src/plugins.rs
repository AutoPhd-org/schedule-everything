//! Plugins (`tasks/plugins.json`): task categories that need more than a list.
//!
//! A plugin owns one task type. Its tasks are ordinary tasks of that type, so
//! they sit in the type's group on the board, run in the timers, and land in
//! the activity history like any other. What a plugin adds is its own data
//! (`state`, such as workout plans or the metrics a tracker records) and a log
//! of what was done (`log`, one entry per finished task, logged session, or
//! measurement). The browser draws each kind of plugin (`fitness`, or a
//! user-built `tracker`) from that data.
//!
//! Four settings say where a plugin shows up: as its own `page`, with its own
//! metrics in the `activity` page, in the `timer` row above the board, and to
//! AI `agent`s, which then see its description and data in
//! `rmd suggest ls --json` so they can suggest tasks for it.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::py::{self, Dict};

/// Kinds of plugin the browser can draw.
pub const KINDS: [&str; 2] = ["fitness", "tracker"];
/// Where a plugin shows up, each on or off.
pub const SETTINGS: [&str; 4] = ["page", "activity", "timer", "agent"];
pub const NAME_LIMIT: usize = 60;
pub const DESCRIPTION_LIMIT: usize = 600;
pub const ICON_LIMIT: usize = 40;
/// Most log entries a plugin keeps.
pub const LOG_LIMIT: usize = 10_000;
/// Largest log entry, or task payload, in bytes of JSON.
pub const ENTRY_LIMIT: usize = 20_000;
/// Largest plugin state, in bytes of JSON.
pub const STATE_LIMIT: usize = 1_000_000;
/// How many of the latest log entries an agent sees.
const AGENT_LOG: usize = 20;

/// `tasks/plugins.json`, next to the task list.
pub fn plugins_path() -> PathBuf {
    crate::paths::runtime().tasks_path.with_file_name("plugins.json")
}

/// Every plugin on file, in the order the user added them. A missing or
/// unreadable file gives none; entries without an ID or a known kind are skipped.
pub fn load_plugins() -> Vec<Dict> {
    let Ok(text) = std::fs::read_to_string(plugins_path()) else {
        return Vec::new();
    };
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter_map(|item| match item {
            Value::Object(item) if !id_of(&item).is_empty() && KINDS.contains(&kind_of(&item)) => Some(item),
            _ => None,
        })
        .collect()
}

pub fn save_plugins(items: &[Dict]) -> std::io::Result<()> {
    let path = plugins_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    py::write_json_pretty(&path, &items)
}

pub fn id_of(plugin: &Dict) -> String {
    plugin.get("id").and_then(Value::as_str).map(str::to_string).unwrap_or_default()
}

pub fn kind_of(plugin: &Dict) -> &str {
    plugin.get("kind").and_then(Value::as_str).unwrap_or("")
}

/// The task type the plugin owns.
pub fn type_of(plugin: &Dict) -> Option<String> {
    plugin.get("type").filter(|t| !t.is_null()).map(py::py_str).filter(|t| !t.is_empty())
}

/// Where the plugin that owns `type_id` is.
pub fn plugin_for_type(plugins: &[Dict], type_id: &str) -> Option<usize> {
    plugins.iter().position(|plugin| type_of(plugin).as_deref() == Some(type_id))
}

pub fn find(plugins: &[Dict], id: &str) -> Option<usize> {
    plugins.iter().position(|plugin| id_of(plugin) == id)
}

/// A setting's default: a plugin has its page and its activity metrics until
/// turned off; the timer and agents see it only once turned on.
pub fn default_setting(key: &str) -> bool {
    matches!(key, "page" | "activity")
}

pub fn setting(plugin: &Dict, key: &str) -> bool {
    plugin
        .get("settings")
        .and_then(Value::as_object)
        .and_then(|settings| settings.get(key))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| default_setting(key))
}

/// Every setting, defaults filled in.
pub fn settings_of(plugin: &Dict) -> Value {
    Value::Object(SETTINGS.iter().map(|key| (key.to_string(), json!(setting(plugin, key)))).collect())
}

fn log_of(plugin: &Dict) -> Vec<Value> {
    plugin.get("log").and_then(Value::as_array).cloned().unwrap_or_default()
}

fn state_of(plugin: &Dict) -> Dict {
    plugin.get("state").and_then(Value::as_object).cloned().unwrap_or_default()
}

/// The name the user knows the plugin by: its type's name, which the board can
/// rename, or else the name it was given.
pub fn name_of(plugin: &Dict, task_types: &Dict) -> String {
    type_of(plugin)
        .and_then(|type_id| task_types.get(&type_id).map(py::py_str))
        .or_else(|| plugin.get("name").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| id_of(plugin))
}

/// A plugin as the browser sees it.
pub fn plugin_row(plugin: &Dict, task_types: &Dict) -> Value {
    let text = |key: &str| plugin.get(key).and_then(Value::as_str).map(str::to_string);
    json!({
        "id": id_of(plugin),
        "kind": kind_of(plugin),
        "name": name_of(plugin, task_types),
        "description": text("description").unwrap_or_default(),
        "icon": text("icon"),
        "type": type_of(plugin),
        "settings": settings_of(plugin),
        "state": Value::Object(state_of(plugin)),
        "log": log_of(plugin),
        "createdAt": text("created_at"),
    })
}

/// Every plugin, for the status snapshot.
pub fn snapshot_rows(task_types: &Dict) -> Vec<Value> {
    load_plugins().iter().map(|plugin| plugin_row(plugin, task_types)).collect()
}

/// What agents may see: only the plugins the user shared with them, each with
/// its description, its data (without what is running right now: a workout in
/// progress, a practice timer), and its latest log entries, newest first.
pub fn agent_rows(task_types: &Dict) -> Vec<Value> {
    load_plugins()
        .iter()
        .filter(|plugin| setting(plugin, "agent"))
        .map(|plugin| {
            let mut state = state_of(plugin);
            state.shift_remove("active");
            state.shift_remove("timer");
            let log = log_of(plugin);
            let recent: Vec<Value> = log.iter().rev().take(AGENT_LOG).cloned().collect();
            let type_id = type_of(plugin);
            json!({
                "id": id_of(plugin),
                "kind": kind_of(plugin),
                "name": name_of(plugin, task_types),
                "description": plugin.get("description").and_then(Value::as_str).unwrap_or_default(),
                "type": type_id,
                "typeName": type_id.and_then(|id| task_types.get(&id).cloned()),
                "state": state,
                "logged": log.len(),
                "recentLog": recent,
            })
        })
        .collect()
}

/// JSON size in bytes.
pub fn json_size(value: &Value) -> usize {
    serde_json::to_string(value).map(|text| text.len()).unwrap_or(usize::MAX)
}

/// One past the highest numeric log ID.
fn next_entry_id(log: &[Value]) -> String {
    let highest = log
        .iter()
        .filter_map(|entry| entry.get("id").and_then(Value::as_str).and_then(|id| id.parse::<u64>().ok()))
        .max()
        .unwrap_or(0);
    (highest + 1).to_string()
}

/// Add an entry to a plugin's log: what was done (`kind`, such as `task`,
/// `session`, `entry`, or `body`), when (local time, now by default), the
/// task it finished if any, and its data. Returns the saved entry.
pub fn append_entry(
    plugins: &mut [Dict],
    index: usize,
    kind: &str,
    task: Option<&str>,
    data: Value,
    at: Option<String>,
) -> Result<Value, String> {
    if json_size(&data) > ENTRY_LIMIT {
        return Err(format!("a log entry can be at most {} KB.", ENTRY_LIMIT / 1000));
    }
    let plugin = &mut plugins[index];
    let mut log = log_of(plugin);
    if log.len() >= LOG_LIMIT {
        return Err(format!("this plugin's log is full ({LOG_LIMIT} entries). Delete old entries first."));
    }
    let mut entry = Dict::new();
    entry.insert("id".into(), json!(next_entry_id(&log)));
    entry.insert("at".into(), json!(at.unwrap_or_else(|| crate::clock::now().format("%Y-%m-%dT%H:%M:%S").to_string())));
    entry.insert("kind".into(), json!(kind));
    if let Some(task) = task {
        entry.insert("task".into(), json!(task));
    }
    entry.insert("data".into(), data);
    let entry = Value::Object(entry);
    log.push(entry.clone());
    plugin.insert("log".into(), Value::Array(log));
    Ok(entry)
}

/// Remove a log entry; whether there was one.
pub fn remove_entry(plugin: &mut Dict, entry_id: &str) -> bool {
    let mut log = log_of(plugin);
    let before = log.len();
    log.retain(|entry| entry.get("id").and_then(Value::as_str) != Some(entry_id));
    let removed = log.len() != before;
    if removed {
        plugin.insert("log".into(), Value::Array(log));
    }
    removed
}

/// A plugin ID from its name: lowercase ASCII letters, digits, and dashes,
/// made unique among `taken` (`fitness`, `fitness-2`, ...). A name without
/// ASCII letters falls back to the kind.
pub fn new_id(name: &str, kind: &str, taken: &[String]) -> String {
    let mut slug = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let mut base: String = slug.trim_matches('-').chars().take(32).collect();
    base = base.trim_end_matches('-').to_string();
    if base.is_empty() {
        base = kind.to_string();
    }
    if !taken.contains(&base) {
        return base;
    }
    (2..).map(|n| format!("{base}-{n}")).find(|id| !taken.contains(id)).expect("some suffix is free")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    #[test]
    fn ids_are_slugs_made_unique() {
        let taken = vec!["fitness".to_string(), "fitness-2".to_string()];
        assert_eq!(new_id("Guitar practice!", "tracker", &taken), "guitar-practice");
        assert_eq!(new_id("Fitness", "fitness", &taken), "fitness-3");
        assert_eq!(new_id("健身", "fitness", &[]), "fitness");
        assert_eq!(new_id("  Spanish — B1 ", "tracker", &[]), "spanish-b1");
    }

    #[test]
    fn settings_have_defaults() {
        let plugin: Dict =
            serde_json::from_value(json!({"id": "x", "kind": "tracker", "settings": {"timer": true}})).unwrap();
        assert_eq!(settings_of(&plugin), json!({"page": true, "activity": true, "timer": true, "agent": false}));
    }

    #[test]
    fn log_entries_get_ids_and_times_and_can_be_removed() {
        let mut env = TestEnv::new();
        env.set_now("2026-11-09T18:30:00");
        let mut plugins: Vec<Dict> =
            vec![serde_json::from_value(json!({"id": "fitness", "kind": "fitness", "type": "4"})).unwrap()];
        let first = append_entry(&mut plugins, 0, "session", Some("Leg day"), json!({"minutes": 40}), None).unwrap();
        assert_eq!(
            first,
            json!({"id": "1", "at": "2026-11-09T18:30:00", "kind": "session", "task": "Leg day", "data": {"minutes": 40}})
        );
        let second =
            append_entry(&mut plugins, 0, "body", None, json!({"weight": 72.5}), Some("2026-11-08T07:00:00".into()))
                .unwrap();
        assert_eq!(second["id"], "2");
        assert!(remove_entry(&mut plugins[0], "1"));
        assert!(!remove_entry(&mut plugins[0], "1"));
        assert_eq!(plugins[0]["log"].as_array().unwrap().len(), 1);
        save_plugins(&plugins).unwrap();
        assert_eq!(load_plugins().len(), 1);
        assert_eq!(plugin_for_type(&load_plugins(), "4"), Some(0));
    }

    #[test]
    fn agents_see_only_shared_plugins_without_a_session_in_progress() {
        let _env = TestEnv::new();
        let plugins: Vec<Dict> = vec![
            serde_json::from_value(json!({"id": "fitness", "kind": "fitness", "type": "4", "description": "Get stronger", "settings": {"agent": true}, "state": {"goal": 3, "active": {"startedAt": 1}, "timer": {"startedAt": 2}}, "log": [{"id": "1"}, {"id": "2"}]})).unwrap(),
            serde_json::from_value(json!({"id": "diary", "kind": "tracker", "type": "5"})).unwrap(),
        ];
        save_plugins(&plugins).unwrap();
        let types: Dict = serde_json::from_value(json!({"4": "Fitness", "5": "Diary"})).unwrap();
        let rows = agent_rows(&types);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], "Fitness");
        assert_eq!(rows[0]["state"], json!({"goal": 3}));
        assert_eq!(rows[0]["recentLog"], json!([{"id": "2"}, {"id": "1"}]));
        assert_eq!(rows[0]["logged"], 2);
    }
}
