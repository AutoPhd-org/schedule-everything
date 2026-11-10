//! Browser operations for local configuration, the reminder service, and PDF
//! exports.
//!
//! Configuration writes are validated with the same rules as `rmd check` and
//! guarded against stale browser edits.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::commands::settings::SettingsModel;
use crate::config::{ScheduleConfig, WeeklySchedule};
use crate::config_check::{config_file_name, validate_content};
use crate::data::load_mode;
use crate::py::{self, Dict};
use crate::web::services::{WebError, WebResult, invalid, os_error, require_text, utf8_error};
use crate::{i18n, paths};

/// Seconds a service command (`rmd update`, ...) may run.
const SERVICE_TIMEOUT_SECONDS: u64 = 120;
const SERVICE_ACTIONS: [&str; 4] = ["update", "stop", "switch", "mode"];

fn exports() -> &'static Mutex<HashMap<String, PathBuf>> {
    static EXPORTS: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    EXPORTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The PDF registered under a `/downloads/<key>` URL by [`export_pdf`].
pub fn export_path(key: &str) -> Option<PathBuf> {
    exports().lock().unwrap_or_else(|p| p.into_inner()).get(key).cloned()
}

/// Register a file for download under `key`.
pub fn register_export(key: &str, path: PathBuf) {
    exports().lock().unwrap_or_else(|p| p.into_inner()).insert(key.to_string(), path);
}

/// `secrets.token_urlsafe(n)`: `n` random bytes as unpadded URL-safe base64.
pub fn token_urlsafe(bytes: usize) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut random = vec![0u8; bytes];
    rand::fill(&mut random[..]);
    let mut out = String::with_capacity(bytes.div_ceil(3) * 4);
    for chunk in random.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

fn file_path(key: &str) -> WebResult<PathBuf> {
    let name = config_file_name(key).ok_or_else(|| invalid("unknown configuration file."))?;
    Ok(paths::runtime().active_config_dir.join(name))
}

fn revision(content: &str) -> String {
    hex::encode(Sha256::digest(content.as_bytes()))
}

/// `path.read_text()` (universal newlines), or `""` for a missing file.
fn read(path: &Path) -> WebResult<String> {
    if !path.exists() {
        return Ok(String::new());
    }
    let bytes = std::fs::read(path).map_err(|e| WebError::internal(os_error(&e, path)))?;
    let text = std::str::from_utf8(&bytes).map_err(|e| WebError::internal(utf8_error(&bytes, &e)))?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// `value == number` with Python's numeric equality (`0 == 0.0 == False`).
fn equals_int(value: Option<&Value>, number: i64) -> bool {
    match value {
        Some(Value::Number(n)) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => i == number,
            (None, Some(u), _) => i64::try_from(u).is_ok_and(|u| u == number),
            (None, None, Some(f)) => f == number as f64,
            _ => false,
        },
        Some(Value::Bool(b)) => i64::from(*b) == number,
        _ => false,
    }
}

/// `{"file", "content", "revision", "activeId"}` for one configuration file.
pub fn config_read(payload: &Dict) -> WebResult {
    let key = payload.get("file").map(py::py_str).unwrap_or_else(|| "settings".to_string());
    let path = file_path(&key)?;
    let content = read(&path)?;
    Ok(json!({
        "file": key,
        "content": content,
        "revision": revision(&content),
        "activeId": paths::runtime().active_id,
    }))
}

/// Restart the reminder service after a configuration change.
fn reload() -> Dict {
    let (ok, message) = crate::commands::service::restart_reminder_service();
    let message = if ok { "Saved and reminders reloaded.".to_string() } else { format!("Saved. {message}") };
    let mut result = Dict::new();
    result.insert("reloaded".into(), json!(ok));
    result.insert("message".into(), json!(message));
    result
}

/// Validate and save a configuration file the browser edited, refusing edits
/// made to an older revision, then reload the reminders.
pub fn config_save(payload: &Dict) -> WebResult {
    config_save_with(payload, reload)
}

fn config_save_with(payload: &Dict, reload: impl FnOnce() -> Dict) -> WebResult {
    let key = require_text(payload, "file")?;
    let path = file_path(&key)?;
    let Some(Value::String(content)) = payload.get("content") else {
        return Err(invalid("content must be text."));
    };
    if !equals_int(payload.get("activeId"), paths::runtime().active_id)
        || payload.get("revision").and_then(Value::as_str) != Some(revision(&read(&path)?).as_str())
    {
        return Err(WebError::new("conflict", "This file changed. Reload it before saving."));
    }
    validate_content(&key, content).map_err(|e| invalid(e.0))?;
    let write = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut temporary = path.clone().into_os_string();
        temporary.push(".tmp");
        std::fs::write(&temporary, content)?;
        std::fs::rename(&temporary, &path)
    };
    write().map_err(|e| WebError::internal(os_error(&e, &path)))?;
    let mut request = Dict::new();
    request.insert("file".into(), json!(key));
    let mut result = config_read(&request)?.as_object().cloned().unwrap_or_default();
    result.extend(reload());
    Ok(Value::Object(result))
}

/// Config sets, the active one, the root directory, the j/p mode, and the
/// interface language.
pub fn workspace_info(_payload: &Dict) -> WebResult {
    let runtime_paths = paths::runtime();
    Ok(json!({
        "configIds": paths::list_config_ids(Some(&runtime_paths.root_dir)),
        "activeId": runtime_paths.active_id,
        "rootDir": runtime_paths.root_dir.display().to_string(),
        "mode": load_mode(),
        "language": i18n::get_language(),
    }))
}

/// Save `[settings] language`, which the browser, the CLI, and the reminder
/// popups all follow. Returns the language now in effect (`REMINDER_LANG`
/// still wins over the setting).
pub fn language_set(payload: &Dict) -> WebResult {
    let requested = require_text(payload, "language")?;
    let Some(language) = i18n::normalize_language(&requested) else {
        return Err(invalid(format!("unsupported language: {requested}. Use one of {}.", i18n::LANGUAGES.join(", "))));
    };
    let settings_path = crate::web::services::snapshot_runtime_paths().settings_path;
    if let Some(edited) = with_language(&read(&settings_path)?, language) {
        std::fs::write(&settings_path, edited).map_err(|e| {
            WebError::new("storage_error", format!("failed to save settings: {}", os_error(&e, &settings_path)))
        })?;
    } else {
        let mut model = SettingsModel::new(&settings_path)
            .map_err(|e| WebError::new("config_error", format!("failed to load settings: {e}")))?;
        model.set("settings", "language", json!(language));
        model.save().map_err(|e| WebError::new("storage_error", format!("failed to save settings: {e}")))?;
    }
    Ok(json!({"language": i18n::get_language()}))
}

/// `content` (a settings.toml) with `[settings] language` set to `language`,
/// changing only that line so the file's comments and layout stay as they
/// are. `None` when it cannot be done that way and still mean the same as the
/// file with only the language changed; the caller then rewrites the file.
fn with_language(content: &str, language: &str) -> Option<String> {
    let assignment = format!("language = \"{language}\"");
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let mut section = String::new();
    let mut header = None;
    let mut replaced = false;
    for (index, line) in lines.iter_mut().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            section = trimmed.trim_start_matches('[').split(']').next().unwrap_or_default().trim().to_string();
            if section == "settings" {
                header = Some(index);
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        if section != "settings" || key.trim() != "language" {
            continue;
        }
        // Keep whatever follows the quoted value, such as a comment.
        let value = value.trim_start();
        let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let rest = &value[value[1..].find(quote)? + 2..];
        *line = format!("{}{assignment}{rest}", &line[..line.len() - trimmed.len()]);
        replaced = true;
        break;
    }
    if !replaced {
        match header {
            Some(index) => lines.insert(index + 1, assignment),
            None => lines.splice(0..0, ["[settings]".to_string(), assignment, String::new()]).for_each(drop),
        }
    }
    let edited = lines.join("\n") + "\n";
    let mut expected = crate::config::parse_toml_str(content).ok()?;
    let settings = expected.entry("settings").or_insert_with(|| json!({})).as_object_mut()?;
    settings.insert("language".into(), json!(language));
    (crate::config::parse_toml_str(&edited).ok()? == expected).then_some(edited)
}

/// Run this `rmd` binary with `args`; returns the exit code, stdout, and
/// stderr.
fn run_cli(args: &[String]) -> WebResult<(i32, String, String)> {
    let executable = std::env::current_exe().map_err(|e| WebError::internal(e.to_string()))?;
    let mut child = Command::new(&executable)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| WebError::internal(os_error(&e, &executable)))?;
    let capture = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            String::from_utf8_lossy(&bytes).into_owned()
        })
    };
    let stdout = capture(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr = capture(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = Instant::now() + Duration::from_secs(SERVICE_TIMEOUT_SECONDS);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let mut command = vec![json!(executable.display().to_string())];
                command.extend(args.iter().map(|a| json!(a)));
                return Err(WebError::internal(format!(
                    "Command {} timed out after {SERVICE_TIMEOUT_SECONDS} seconds",
                    py::py_repr(&Value::Array(command))
                )));
            }
            Err(e) => return Err(WebError::internal(e.to_string())),
        }
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    // A process killed by a signal has no code; Python reports it as negative.
    let code = status.code().unwrap_or(-1);
    Ok((code, stdout, stderr))
}

/// `update`, `stop`, `switch` (with `configId`), or `mode` (with `mode`).
pub fn service_action(payload: &Dict) -> WebResult {
    let action = require_text(payload, "action")?;
    if !SERVICE_ACTIONS.contains(&action.as_str()) {
        return Err(invalid("unknown service action."));
    }
    let mut args = vec![action.clone()];
    if action == "switch" {
        let config_id =
            payload.get("configId").and_then(py::py_int).ok_or_else(|| invalid("configId must be an integer."))?;
        if !paths::list_config_ids(None).contains(&config_id) {
            return Err(WebError::new("not_found", "configuration does not exist."));
        }
        args.push(config_id.to_string());
    } else if action == "mode" {
        let mode = py::py_str(payload.get("mode").unwrap_or(&Value::Null));
        if mode != "j" && mode != "p" {
            return Err(invalid("mode must be j or p."));
        }
        args.push(mode);
    }
    let (code, stdout, stderr) = run_cli(&args)?;
    let stdout = crate::web::services::py_strip(&stdout).to_string();
    if code != 0 {
        let stderr = crate::web::services::py_strip(&stderr).to_string();
        return Err(WebError::new("service_error", if stdout.is_empty() { stderr } else { stdout }));
    }
    let mut result = Dict::new();
    result.insert("message".into(), json!(stdout));
    if let Value::Object(info) = workspace_info(&Dict::new())? {
        result.extend(info);
    }
    Ok(Value::Object(result))
}

/// Generate a weekly or monthly report or the schedule PDF and register it
/// for download.
pub fn export_pdf(payload: &Dict) -> WebResult {
    let kind = require_text(payload, "kind")?;
    let runtime_paths = paths::runtime();
    let output: Option<PathBuf> = match kind.as_str() {
        "weekly" | "monthly" => {
            let target: Option<NaiveDate> = match payload.get("date") {
                Some(Value::String(text)) if !text.is_empty() => {
                    Some(py::parse_iso_date(text).ok_or_else(|| {
                        WebError::internal(format!("Invalid isoformat string: {}", py::repr_str(text)))
                    })?)
                }
                Some(value) if py::truthy(value) => {
                    return Err(WebError::internal("fromisoformat: argument must be str"));
                }
                _ => None,
            };
            crate::report::generate_manual_report(&kind, target, Some(&runtime_paths.settings_path))
                .map_err(|e| WebError::internal(e.to_string()))?
        }
        "schedule" => {
            let config =
                ScheduleConfig::load(&runtime_paths.settings_path).map_err(|e| WebError::internal(e.to_string()))?;
            let weekly = WeeklySchedule::load(&runtime_paths.odd_path, &runtime_paths.even_path)
                .map_err(|e| WebError::internal(e.to_string()))?;
            let output = runtime_paths.root_dir.join("exports").join("schedule.pdf");
            crate::visualizer::visualize(&config, &weekly.odd_data, &weekly.even_data, Some(&output))
                .map_err(|e| WebError::internal(e.to_string()))?;
            Some(output)
        }
        _ => return Err(invalid("Unknown PDF type.")),
    };
    let Some(output) = output.filter(|path| path.is_file()) else {
        return Err(WebError::new("export_error", "Could not generate PDF."));
    };
    let key = token_urlsafe(24);
    register_export(&key, output.clone());
    let filename = output.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(json!({"url": format!("/downloads/{key}"), "filename": filename}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    fn payload(value: Value) -> Dict {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn tokens_are_url_safe_and_sized_like_python() {
        let token = token_urlsafe(32);
        assert_eq!(token.len(), 43);
        assert!(token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_eq!(token_urlsafe(24).len(), 32);
        assert_ne!(token_urlsafe(24), token_urlsafe(24));
    }

    #[test]
    fn configuration_save_validates_and_rejects_stale_edits() {
        let env = TestEnv::new();
        env.write("user_config_0/habits.toml", "[habits]\n1 = \"Read\"\n");
        let reloaded = || {
            let mut result = Dict::new();
            result.insert("reloaded".into(), json!(true));
            result
        };
        let original = config_read(&payload(json!({"file": "habits"}))).unwrap();
        assert_eq!(original["activeId"], json!(0));
        let mut broken = original.as_object().cloned().unwrap();
        broken.insert("content".into(), json!("[habits"));
        let error = config_save_with(&broken, reloaded).unwrap_err();
        assert_eq!(error.code, "invalid_input");
        assert_eq!(env.read("user_config_0/habits.toml"), "[habits]\n1 = \"Read\"\n");

        env.write("user_config_0/habits.toml", "[habits]\n1 = \"Exercise\"\n");
        let mut stale = original.as_object().cloned().unwrap();
        stale.insert("content".into(), json!("[habits]\n1 = \"Sleep\"\n"));
        let error = config_save_with(&stale, reloaded).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.message.as_str()),
            ("conflict", "This file changed. Reload it before saving.")
        );

        let mut fresh = config_read(&payload(json!({"file": "habits"}))).unwrap().as_object().cloned().unwrap();
        fresh.insert("content".into(), json!("[habits]\n1 = \"Sleep\"\n"));
        let result = config_save_with(&fresh, reloaded).unwrap();
        assert_eq!(result["reloaded"], json!(true));
        assert_eq!(result["content"], json!("[habits]\n1 = \"Sleep\"\n"));
        assert!(env.read("user_config_0/habits.toml").contains("\"Sleep\""));
        assert!(!env.root().join("user_config_0/habits.toml.tmp").exists());
    }

    #[test]
    fn configuration_file_selection_cannot_escape_root() {
        let _env = TestEnv::new();
        for key in ["../credentials", "model", "passwords"] {
            let error = config_read(&payload(json!({"file": key}))).unwrap_err();
            assert_eq!((error.code.as_str(), error.message.as_str()), ("invalid_input", "unknown configuration file."));
        }
    }

    #[test]
    fn reading_uses_universal_newlines() {
        let env = TestEnv::new();
        env.write("user_config_0/profile.md", "a\r\nb\rc\n");
        let result = config_read(&payload(json!({"file": "profile"}))).unwrap();
        assert_eq!(result["content"], json!("a\nb\nc\n"));
        assert_eq!(result["revision"], json!(revision("a\nb\nc\n")));
        let missing = config_read(&payload(json!({"file": "deadlines"}))).unwrap();
        assert_eq!(missing["content"], json!(""));
    }

    #[test]
    fn service_and_export_requests_are_validated_before_running_anything() {
        let env = TestEnv::new();
        env.config_dir();
        let error = |value: Value, f: fn(&Dict) -> WebResult| f(&payload(value)).unwrap_err();
        assert_eq!(error(json!({"action": "explode"}), service_action).message, "unknown service action.");
        assert_eq!(
            error(json!({"action": "switch", "configId": null}), service_action).message,
            "configId must be an integer."
        );
        assert_eq!(error(json!({"action": "switch", "configId": 7}), service_action).code, "not_found");
        assert_eq!(error(json!({"action": "mode", "mode": "x"}), service_action).message, "mode must be j or p.");
        assert_eq!(error(json!({"kind": "poster"}), export_pdf).message, "Unknown PDF type.");
        assert_eq!(
            error(json!({"kind": "weekly", "date": "soon"}), export_pdf).message,
            "Invalid isoformat string: 'soon'"
        );
        assert_eq!(error(json!({"kind": "schedule"}), export_pdf).code, "internal_error");
    }

    #[test]
    fn workspace_info_lists_config_sets_and_mode() {
        let env = TestEnv::with_fixture();
        let info = workspace_info(&Dict::new()).unwrap();
        assert_eq!(info["configIds"], json!([0]));
        assert_eq!(info["activeId"], json!(0));
        assert_eq!(info["mode"], json!("j"));
        assert_eq!(info["rootDir"], json!(paths::resolve(env.root()).display().to_string()));
        assert_eq!(info["language"], json!("en"));
    }

    #[test]
    fn language_edits_only_its_own_line() {
        let template = "# Settings\n[settings]\nalarm_interval = 5  # seconds\nlanguage = \"en\"  # \"en\", \"zh\", \"es\", or \"ko\"\n\n[task_types]\n1 = \"Work\"\n";
        assert_eq!(
            with_language(template, "ko").unwrap(),
            template.replace("language = \"en\"  #", "language = \"ko\"  #")
        );
        assert_eq!(with_language("[settings]\nlanguage='zh'\n", "es").unwrap(), "[settings]\nlanguage = \"es\"\n");
        // A language key in another table is not the setting; the setting goes under [settings].
        assert_eq!(
            with_language("[tasks]\nlanguage = \"x\"\n\n[settings]\nsound = 1 # loud\n", "zh").unwrap(),
            "[tasks]\nlanguage = \"x\"\n\n[settings]\nlanguage = \"zh\"\nsound = 1 # loud\n"
        );
        assert_eq!(with_language("", "ko").unwrap(), "[settings]\nlanguage = \"ko\"\n\n");
        assert_eq!(
            with_language("# mine\n[task_types]\n1 = \"Work\"\n", "es").unwrap(),
            "[settings]\nlanguage = \"es\"\n\n# mine\n[task_types]\n1 = \"Work\"\n"
        );
        // Shapes it cannot edit in place are left to the full rewrite.
        assert_eq!(with_language("[settings]\nlanguage = [\"en\"]\n", "zh"), None);
        assert_eq!(with_language("[settings]\n\"language\" = \"en\"\n", "zh"), None);
        assert_eq!(with_language("not toml [", "zh"), None);
    }

    #[test]
    fn language_set_saves_the_setting_everyone_follows() {
        let env = TestEnv::with_fixture();
        for (requested, saved) in [("ko", "ko"), ("Español", "es"), ("zh-CN", "zh"), ("en", "en")] {
            let result = language_set(&payload(json!({"language": requested}))).unwrap();
            assert_eq!(result, json!({"language": saved}), "{requested}");
            assert_eq!(workspace_info(&Dict::new()).unwrap()["language"], json!(saved));
            let settings = std::fs::read_to_string(env.root().join("user_config_0/settings.toml")).unwrap();
            assert!(settings.contains(&format!("language = \"{saved}\"")), "{settings}");
        }
        let error = language_set(&payload(json!({"language": "fr"}))).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.message.as_str()),
            ("invalid_input", "unsupported language: fr. Use one of en, zh, es, ko.")
        );
        assert_eq!(language_set(&Dict::new()).unwrap_err().code, "invalid_input");
        let settings = std::fs::read_to_string(env.root().join("user_config_0/settings.toml")).unwrap();
        assert!(settings.contains("alarm_interval = 5  # seconds between alarm repeats"), "{settings}");
        // A file the line edit cannot handle is rewritten whole, and still gets the language.
        env.write("user_config_0/settings.toml", "[settings]\nlanguage = [\"en\"]\n");
        assert_eq!(language_set(&payload(json!({"language": "ko"}))).unwrap(), json!({"language": "ko"}));
    }

    #[test]
    fn downloads_are_registered_by_key() {
        register_export("test-export", PathBuf::from("/tmp/report.pdf"));
        assert_eq!(export_path("test-export"), Some(PathBuf::from("/tmp/report.pdf")));
        assert_eq!(export_path("missing"), None);
    }
}
