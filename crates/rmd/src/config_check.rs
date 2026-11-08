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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    const SETTINGS: &str = "\n[settings]\nlanguage = \"en\"\n\n[time_blocks]\npomodoro = 25\ndeep_work = 90\n\n[time_points]\nlunch_time = \"Lunch\"\n\n[tasks]\ndaily_summary = \"23:35\"\n\n[paths]\nconfig_dir = \"config\"\n";
    const WEEK: &str = "\n[monday]\n\"09:00\" = \"deep_work\"\n\"12:30\" = \"lunch_time\"\n\"14:00\" = { block = \"pomodoro\", title = \"Review\" }\n\n[common]\n\"20:00\" = \"pomodoro\"\n";

    fn config_root() -> TestEnv {
        let env = TestEnv::new();
        env.write(".active_config", "0\n");
        env.write("user_config_0/settings.toml", SETTINGS);
        env.write("user_config_0/odd_weeks.toml", WEEK);
        env.write("user_config_0/even_weeks.toml", WEEK);
        env.write("user_config_0/habits.toml", "[habits]\n1 = \"Read\"\n");
        env
    }

    fn report(env: &TestEnv) -> Value {
        let _ = env;
        check_config(None)
    }

    #[test]
    fn valid_active_config_reports_paths_for_agents() {
        let env = config_root();
        let root = crate::paths::resolve(env.root()).display().to_string();
        let report = report(&env);
        assert_eq!(report["valid"], true);
        assert_eq!(report["errors"], json!([]));
        assert_eq!(report["configRoot"], root);
        assert_eq!(report["activeId"], 0);
        assert_eq!(report["configIds"], json!([0]));
        assert_eq!(report["nextConfigId"], 1);
        assert_eq!(report["nextConfigDir"], format!("{root}/user_config_1"));
        assert_eq!(report["directory"], report["activeConfigDir"]);
        assert_eq!(report["directory"], format!("{root}/user_config_0"));
        assert_eq!(
            report["files"]["profile"],
            json!({"path": format!("{root}/user_config_0/profile.md"), "exists": false})
        );
        let keys: Vec<&String> = report.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            [
                "configRoot",
                "activeId",
                "activeConfigDir",
                "configIds",
                "nextConfigId",
                "nextConfigDir",
                "directory",
                "files",
                "valid",
                "errors",
                "warnings"
            ]
        );
    }

    #[test]
    fn unknown_blocks_fail_and_undefined_labels_warn() {
        let env = config_root();
        env.write(
            "user_config_0/odd_weeks.toml",
            "[tuesday]\n\"09:00\" = \"gym\"\n\"10:00\" = { block = \"lunch_time\", title = \"Lunch talk\" }\n",
        );
        let report = report(&env);
        assert_eq!(report["valid"], false);
        assert_eq!(
            report["errors"],
            json!([{"file": "odd_weeks.toml", "message": "[tuesday] 10:00: block 'lunch_time' is not a [time_blocks] name in settings.toml"}])
        );
        assert_eq!(
            report["warnings"],
            json!([{"file": "odd_weeks.toml", "message": "[tuesday] 09:00: 'gym' is not a [time_blocks] or [time_points] name, so it is shown as a plain reminder message"}])
        );
        env.write("user_config_0/odd_weeks.toml", "[tuesday]\n\"09:00\" = \"Call mom\"\n");
        let report = check_config(None);
        assert_eq!(report["valid"], true);
        assert_eq!(report["warnings"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn syntax_and_structure_errors() {
        let cases = [
            ("settings.toml", "[settings]\nlanguage = 'en'\n", "settings require a [time_blocks] table"),
            ("settings.toml", "[settings\n", "(at line 1, column"),
            ("even_weeks.toml", "[funday]\n\"09:00\" = \"pomodoro\"\n", "weekday or common tables"),
            ("even_weeks.toml", "[monday]\n\"9am\" = \"pomodoro\"\n", "valid HH:MM time"),
            ("habits.toml", "[habits\n", "(at line 1, column"),
            ("ddl.json", "[{\"event\": \"Launch\", \"deadline\": \"soon\"}]", "Invalid isoformat string: 'soon'"),
            (
                "ddl.json",
                "[{\"event\": \"Rent\", \"deadline\": \"2026-11-30\", \"repeat\": \"daily\"}]",
                "repeat must be",
            ),
        ];
        for (file, content, message) in cases {
            let env = config_root();
            env.write(&format!("user_config_0/{file}"), content);
            let report = check_config(None);
            assert_eq!(report["valid"], false, "{file}: {content}");
            let errors = report["errors"].as_array().unwrap();
            assert!(
                errors.iter().any(|e| e["file"] == file && e["message"].as_str().unwrap().contains(message)),
                "{file}: expected {message:?} in {errors:?}"
            );
        }
    }

    #[test]
    fn missing_tasks_and_paths_tables_are_warnings() {
        let env = config_root();
        env.write(
            "user_config_0/settings.toml",
            "[settings]\n[time_blocks]\npomodoro = 25\ndeep_work = 90\n[time_points]\nlunch_time = \"Lunch\"\n",
        );
        let report = check_config(None);
        assert_eq!(report["valid"], true);
        let messages: Vec<&str> =
            report["warnings"].as_array().unwrap().iter().map(|w| w["message"].as_str().unwrap()).collect();
        assert_eq!(
            messages,
            ["no [tasks] table; built-in defaults apply", "no [paths] table; built-in defaults apply"]
        );
    }

    #[test]
    fn other_directories_and_missing_files() {
        let env = config_root();
        let draft = env.root().join("draft");
        std::fs::create_dir_all(&draft).unwrap();
        std::fs::write(draft.join("settings.toml"), SETTINGS).unwrap();
        let report = check_config(Some(&draft));
        assert_eq!(report["valid"], false);
        assert_eq!(report["directory"], crate::paths::resolve(&draft).display().to_string());
        let errors = &report["errors"];
        assert!(errors.as_array().unwrap().contains(&json!({"file": "odd_weeks.toml", "message": "missing"})));
        assert!(errors.as_array().unwrap().contains(&json!({"file": "even_weeks.toml", "message": "missing"})));
        std::fs::write(draft.join("odd_weeks.toml"), WEEK).unwrap();
        std::fs::write(draft.join("even_weeks.toml"), WEEK).unwrap();
        assert_eq!(check_config(Some(&draft))["valid"], true);
    }

    #[test]
    fn validate_content_matches_the_browser_editor_rules() {
        assert_eq!(
            validate_content("settings", "[settings]\n[time_blocks]\n[time_points]\n[tasks]\n").unwrap_err().0,
            "settings require a [paths] table"
        );
        assert!(validate_content("profile", "# Anything goes in Markdown").is_ok());
        assert!(
            validate_content(
                "deadlines",
                "[{\"event\": \"Rent\", \"deadline\": \"2026-11-30\", \"repeat\": \"monthly\"}]"
            )
            .is_ok()
        );
        assert!(validate_content("deadlines", "{}").is_err());
        assert!(validate_content("habits", "[habits]\n1 = \"Read\"\n").is_ok());
    }
}
