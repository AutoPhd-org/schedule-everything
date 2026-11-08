//! Configuration validation shared by `rmd check` and the browser editor.
//!
//! [`validate_content`] checks one file's syntax and required structure.
//! [`check_config`] checks a whole config directory, including the week
//! schedules' labels against settings.toml, and reports the paths an AI agent
//! needs to build or version a schedule. Problems the reminder service
//! tolerates are warnings.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value, json};

use crate::config::parse_toml_str;
use crate::paths;
use crate::py::{self, Dict};
use crate::recurrence::REPEAT_RULES;

/// `(key, file name)` for each config file, in report order.
pub const CONFIG_FILES: [(&str, &str); 6] = [
    ("settings", "settings.toml"),
    ("odd", "odd_weeks.toml"),
    ("even", "even_weeks.toml"),
    ("habits", "habits.toml"),
    ("deadlines", "ddl.json"),
    ("profile", "profile.md"),
];
pub const REQUIRED_FILES: [&str; 3] = ["settings", "odd", "even"];
pub const WEEK_SECTIONS: [&str; 8] =
    ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "common"];
pub const SETTINGS_TABLES: [&str; 5] = ["settings", "time_blocks", "time_points", "tasks", "paths"];
/// Tables the runtime can do without (reported as warnings).
pub const OPTIONAL_SETTINGS_TABLES: [&str; 2] = ["tasks", "paths"];

/// File name for a config key such as `"odd"`.
pub fn config_file_name(key: &str) -> Option<&'static str> {
    CONFIG_FILES.iter().find(|(k, _)| *k == key).map(|(_, name)| *name)
}

/// A configuration file that fails validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ConfigCheckError(pub String);

fn valid_time(value: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    parts.len() == 2
        && parts.iter().all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_digit()))
        && parts[0].parse::<u32>().is_ok_and(|h| h < 24)
        && parts[1].parse::<u32>().is_ok_and(|m| m < 60)
}

fn err(message: impl Into<String>) -> ConfigCheckError {
    ConfigCheckError(message.into())
}

/// Validate the syntax and structure of one configuration file.
pub fn validate_content(key: &str, content: &str) -> Result<(), ConfigCheckError> {
    if key == "deadlines" {
        let values: Value = serde_json::from_str(content).map_err(|e| err(e.to_string()))?;
        let Value::Array(items) = &values else {
            return Err(err("deadlines must be a list of event/deadline objects"));
        };
        let well_formed = items.iter().all(|item| {
            item.as_object().is_some_and(|m| {
                matches!(m.get("event"), Some(Value::String(_))) && matches!(m.get("deadline"), Some(Value::String(_)))
            })
        });
        if !well_formed {
            return Err(err("deadlines must be a list of event/deadline objects"));
        }
        for item in items {
            let deadline = item["deadline"].as_str().unwrap_or_default();
            if py::parse_iso_date(deadline).is_none() {
                return Err(err(format!("Invalid isoformat string: {}", py::repr_str(deadline))));
            }
            match item.get("repeat") {
                None | Some(Value::Null) => {}
                Some(Value::String(s)) if s.is_empty() || REPEAT_RULES.contains(&s.as_str()) => {}
                Some(_) => return Err(err("deadline repeat must be weekly, biweekly, or monthly")),
            }
        }
        return Ok(());
    }
    if key == "profile" {
        return Ok(());
    }
    let data = parse_toml_str(content).map_err(|e| err(e.to_string()))?;
    if key == "odd" || key == "even" {
        for (section, events) in &data {
            let Value::Object(events) = events else {
                return Err(err("weekly schedules must contain weekday or common tables"));
            };
            if !WEEK_SECTIONS.contains(&section.as_str()) {
                return Err(err("weekly schedules must contain weekday or common tables"));
            }
            for (time, event) in events {
                if !valid_time(time) || !matches!(event, Value::String(_) | Value::Object(_)) {
                    return Err(err("events require a valid HH:MM time and label or block/title object"));
                }
            }
        }
    }
    if key == "settings" {
        for required in SETTINGS_TABLES {
            if !matches!(data.get(required), Some(Value::Object(_))) {
                return Err(err(format!("settings require a [{required}] table")));
            }
        }
    }
    if key == "habits" && !matches!(data.get("habits"), Some(Value::Object(_))) {
        return Err(err("habits require a [habits] table"));
    }
    Ok(())
}

fn label_problems(week: &Dict, blocks: &BTreeSet<String>, points: &BTreeSet<String>) -> (Vec<String>, Vec<String>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for (section, events) in week {
        let Value::Object(events) = events else { continue };
        for (time, event) in events {
            let place = format!("[{section}] {time}");
            match event {
                Value::String(label) => {
                    if !blocks.contains(label) && !points.contains(label) {
                        warnings.push(format!(
                            "{place}: '{label}' is not a [time_blocks] or [time_points] name, so it is shown as a plain reminder message"
                        ));
                    }
                }
                Value::Object(map) => {
                    let block = map.get("block");
                    let known = matches!(block, Some(Value::String(b)) if blocks.contains(b));
                    if !known {
                        let shown = block.map(py::py_str).unwrap_or_else(|| "None".to_string());
                        errors.push(format!("{place}: block '{shown}' is not a [time_blocks] name in settings.toml"));
                    }
                    if let Some(title) = map.get("title") {
                        let ok = matches!(title, Value::String(t) if !t.trim().is_empty());
                        if !ok {
                            errors.push(format!("{place}: title must be non-empty text"));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    (errors, warnings)
}

fn issue(file: &str, message: impl Into<String>) -> Value {
    json!({"file": file, "message": message.into()})
}

/// Validate settings.toml; `Some(parsed)` when it can be used for label checks.
fn check_settings(text: &str, name: &str, errors: &mut Vec<Value>, warnings: &mut Vec<Value>) -> Option<Dict> {
    let data = match parse_toml_str(text) {
        Ok(d) => d,
        Err(e) => {
            errors.push(issue(name, e.to_string()));
            return None;
        }
    };
    let missing: Vec<&str> =
        SETTINGS_TABLES.iter().copied().filter(|t| !matches!(data.get(*t), Some(Value::Object(_)))).collect();
    let required: Vec<&str> = missing.iter().copied().filter(|t| !OPTIONAL_SETTINGS_TABLES.contains(t)).collect();
    for table in &required {
        errors.push(issue(name, format!("settings require a [{table}] table")));
    }
    for table in &missing {
        if OPTIONAL_SETTINGS_TABLES.contains(table) {
            warnings.push(issue(name, format!("no [{table}] table; built-in defaults apply")));
        }
    }
    if required.is_empty() { Some(data) } else { None }
}

fn key_names(settings: &Dict, table: &str) -> BTreeSet<String> {
    match settings.get(table) {
        Some(Value::Object(map)) => map.keys().cloned().collect(),
        _ => BTreeSet::new(),
    }
}

/// Validate a config directory (the active one by default) and report its paths.
pub fn check_config(directory: Option<&Path>) -> Value {
    let rp = paths::runtime();
    let target = match directory {
        Some(dir) => paths::resolve(&paths::expand_user(&dir.to_string_lossy())),
        None => rp.active_config_dir.clone(),
    };
    let mut errors: Vec<Value> = Vec::new();
    let mut warnings: Vec<Value> = Vec::new();
    let mut parsed: indexmap::IndexMap<&str, Dict> = indexmap::IndexMap::new();
    let mut files = Dict::new();
    for (key, name) in CONFIG_FILES {
        let path = target.join(name);
        let exists = path.is_file();
        files.insert(key.into(), json!({"path": path.display().to_string(), "exists": exists}));
        if !exists {
            if REQUIRED_FILES.contains(&key) {
                errors.push(issue(name, "missing"));
            }
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                errors.push(issue(name, e.to_string()));
                continue;
            }
        };
        if key == "settings" {
            match check_settings(&text, name, &mut errors, &mut warnings) {
                Some(data) => {
                    parsed.insert(key, data);
                }
                None => continue,
            }
        } else {
            if let Err(e) = validate_content(key, &text) {
                errors.push(issue(name, e.0));
                continue;
            }
            if (key == "odd" || key == "even")
                && let Ok(data) = parse_toml_str(&text)
            {
                parsed.insert(key, data);
            }
        }
    }

    if let Some(settings) = parsed.get("settings") {
        let blocks = key_names(settings, "time_blocks");
        let points = key_names(settings, "time_points");
        for key in ["odd", "even"] {
            if let Some(week) = parsed.get(key) {
                let (label_errors, label_warnings) = label_problems(week, &blocks, &points);
                let file = config_file_name(key).unwrap_or(key);
                errors.extend(label_errors.into_iter().map(|m| issue(file, m)));
                warnings.extend(label_warnings.into_iter().map(|m| issue(file, m)));
            }
        }
    }

    let next_id = paths::get_next_config_id(Some(&rp.root_dir));
    json!({
        "configRoot": rp.root_dir.display().to_string(),
        "activeId": rp.active_id,
        "activeConfigDir": rp.active_config_dir.display().to_string(),
        "configIds": paths::list_config_ids(Some(&rp.root_dir)),
        "nextConfigId": next_id,
        "nextConfigDir": rp.root_dir.join(format!("user_config_{next_id}")).display().to_string(),
        "directory": target.display().to_string(),
        "files": files,
        "valid": errors.is_empty(),
        "errors": errors,
        "warnings": warnings,
    })
}

/// `rmd check [--dir DIR] [--json]`.
pub fn check_command(dir: Option<&str>, as_json: bool) -> i32 {
    let directory = dir.filter(|d| !d.is_empty()).map(Path::new);
    let report = check_config(directory);
    let valid = report["valid"].as_bool().unwrap_or(false);
    if as_json {
        println!("{}", py::dumps_pretty(&report));
        return if valid { 0 } else { 1 };
    }
    println!("Config root: {}", report["configRoot"].as_str().unwrap_or_default());
    println!("Checked:     {}", report["directory"].as_str().unwrap_or_default());
    if let Some(files) = report["files"].as_object() {
        for (key, info) in files {
            let exists = info["exists"].as_bool().unwrap_or(false);
            let mark = if exists {
                "✓"
            } else if REQUIRED_FILES.contains(&key.as_str()) {
                "✗"
            } else {
                "·"
            };
            let name = config_file_name(key).unwrap_or(key);
            println!("  {mark} {name}{}", if exists { "" } else { " (missing)" });
        }
    }
    for warning in report["warnings"].as_array().into_iter().flatten() {
        println!(
            "⚠️  {}: {}",
            warning["file"].as_str().unwrap_or_default(),
            warning["message"].as_str().unwrap_or_default()
        );
    }
    if valid {
        println!("✅ Configuration is valid.");
        return 0;
    }
    for error in report["errors"].as_array().into_iter().flatten() {
        println!(
            "❌ {}: {}",
            error["file"].as_str().unwrap_or_default(),
            error["message"].as_str().unwrap_or_default()
        );
    }
    1
}
