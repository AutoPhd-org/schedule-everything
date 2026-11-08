//! Config layout: versioned `user_config_n` sets, the active-set marker, and
//! the runtime paths every other module reads and writes.

use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_SET_PREFIX: &str = "user_config_";
pub const ACTIVE_CONFIG_MARKER: &str = ".active_config";
pub const MANAGED_CONFIG_FILE_NAMES: [&str; 9] = [
    "ddl.json",
    "even_weeks.toml",
    "habits.toml",
    "habits_template.toml",
    "odd_weeks.toml",
    "profile.md",
    "settings.toml",
    "settings_template.toml",
    "week_schedule_template.toml",
];

/// Resolved filesystem paths for the currently active schedule set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePaths {
    pub root_dir: PathBuf,
    pub active_id: i64,
    pub active_config_dir: PathBuf,
    pub settings_path: PathBuf,
    pub odd_path: PathBuf,
    pub even_path: PathBuf,
    pub ddl_path: PathBuf,
    pub habit_path: PathBuf,
    pub profile_path: PathBuf,
    pub tasks_path: PathBuf,
    pub task_log_path: PathBuf,
    pub record_path: PathBuf,
    pub procrastinate_path: PathBuf,
    pub mode_path: PathBuf,
}

/// Expand a leading `~` the way `Path.expanduser()` does.
pub fn expand_user(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    PathBuf::from(path)
}

/// The user's home directory.
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."))
}

/// `Path.resolve()`: absolute, symlinks resolved where the path exists.
pub fn resolve(path: &Path) -> PathBuf {
    if let Ok(canonical) = fs::canonicalize(path) {
        return canonical;
    }
    let absolute =
        if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(path) };
    // Resolve the longest existing prefix, then append the rest normalised.
    let mut existing = absolute.clone();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    while !existing.exists() {
        match (existing.file_name().map(|n| n.to_os_string()), existing.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name);
                existing = parent.to_path_buf();
            }
            _ => break,
        }
    }
    let mut result = fs::canonicalize(&existing).unwrap_or(existing);
    for part in tail.into_iter().rev() {
        if part == ".." {
            result.pop();
        } else if part != "." {
            result.push(part);
        }
    }
    result
}

/// Root config directory from `REMINDER_CONFIG_DIR`, the installed location,
/// or `./config`.
pub fn resolve_config_root_dir() -> PathBuf {
    if let Ok(raw) = std::env::var("REMINDER_CONFIG_DIR")
        && !raw.is_empty()
    {
        return resolve(&expand_user(&raw));
    }
    let installed = home_dir().join(".schedule_management").join("config");
    if installed.exists() {
        return resolve(&installed);
    }
    resolve(Path::new("config"))
}

fn parse_config_set_id(path: &Path) -> Option<i64> {
    let name = path.file_name()?.to_str()?;
    let digits = name.strip_prefix(CONFIG_SET_PREFIX)?;
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn discover_config_dirs(root_dir: &Path) -> std::collections::BTreeMap<i64, PathBuf> {
    let mut found = std::collections::BTreeMap::new();
    let Ok(entries) = fs::read_dir(root_dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(id) = parse_config_set_id(&path) {
            found.insert(id, path);
        }
    }
    found
}

fn root_or_default(root_dir: Option<&Path>) -> PathBuf {
    root_dir.map(Path::to_path_buf).unwrap_or_else(resolve_config_root_dir)
}

/// Sorted config ids under the root config directory.
pub fn list_config_ids(root_dir: Option<&Path>) -> Vec<i64> {
    discover_config_dirs(&root_or_default(root_dir)).into_keys().collect()
}

/// Directory for a specific config id.
pub fn get_config_dir(root_dir: Option<&Path>, config_id: i64) -> PathBuf {
    root_or_default(root_dir).join(format!("{CONFIG_SET_PREFIX}{config_id}"))
}

/// Next unused config id.
pub fn get_next_config_id(root_dir: Option<&Path>) -> i64 {
    list_config_ids(root_dir).into_iter().max().unwrap_or(-1) + 1
}

/// Whether the root still contains the legacy flat files.
pub fn has_legacy_config_files(root_dir: Option<&Path>) -> bool {
    let root = root_or_default(root_dir);
    MANAGED_CONFIG_FILE_NAMES.iter().any(|name| root.join(name).exists())
}

fn read_active_config_marker(root_dir: &Path) -> Option<i64> {
    let raw = fs::read_to_string(root_dir.join(ACTIVE_CONFIG_MARKER)).ok()?;
    crate::py::parse_int_str(raw.trim())
}

/// Persist the active config id marker.
pub fn write_active_config_id(root_dir: Option<&Path>, config_id: i64) -> std::io::Result<PathBuf> {
    let root = root_or_default(root_dir);
    fs::create_dir_all(&root)?;
    let marker = root.join(ACTIVE_CONFIG_MARKER);
    fs::write(&marker, format!("{config_id}\n"))?;
    Ok(marker)
}

/// Move the legacy flat config files into `user_config_0` once.
pub fn migrate_legacy_config_layout(root_dir: Option<&Path>) -> bool {
    let root = root_or_default(root_dir);
    if !root.exists() || !list_config_ids(Some(&root)).is_empty() {
        return false;
    }
    let legacy: Vec<PathBuf> =
        MANAGED_CONFIG_FILE_NAMES.iter().map(|name| root.join(name)).filter(|p| p.exists()).collect();
    if legacy.is_empty() {
        return false;
    }
    let target = get_config_dir(Some(&root), 0);
    if fs::create_dir_all(&target).is_err() {
        return false;
    }
    for source in legacy {
        if let Some(name) = source.file_name() {
            let _ = fs::rename(&source, target.join(name));
        }
    }
    let _ = write_active_config_id(Some(&root), 0);
    true
}

/// Active config directory without touching the on-disk layout.
pub fn preview_active_config_dir(root_dir: Option<&Path>) -> PathBuf {
    let root = root_or_default(root_dir);
    let dirs = discover_config_dirs(&root);
    if dirs.is_empty() {
        return get_config_dir(Some(&root), 0);
    }
    if let Some(active) = read_active_config_marker(&root)
        && let Some(dir) = dirs.get(&active)
    {
        return dir.clone();
    }
    dirs.values().next().cloned().unwrap_or_else(|| get_config_dir(Some(&root), 0))
}

/// Active config id after applying the legacy migration if needed.
pub fn resolve_active_config_id(root_dir: Option<&Path>) -> Option<i64> {
    let root = root_or_default(root_dir);
    migrate_legacy_config_layout(Some(&root));
    let dirs = discover_config_dirs(&root);
    if dirs.is_empty() {
        return None;
    }
    if let Some(active) = read_active_config_marker(&root)
        && dirs.contains_key(&active)
    {
        return Some(active);
    }
    let first = *dirs.keys().next()?;
    let _ = write_active_config_id(Some(&root), first);
    Some(first)
}

/// Active config directory, defaulting to `user_config_0`.
pub fn resolve_active_config_dir(root_dir: Option<&Path>, create: bool) -> PathBuf {
    let root = root_or_default(root_dir);
    if let Some(active) = resolve_active_config_id(Some(&root)) {
        return get_config_dir(Some(&root), active);
    }
    let default_dir = get_config_dir(Some(&root), 0);
    if create {
        let _ = fs::create_dir_all(&default_dir);
        let _ = write_active_config_id(Some(&root), 0);
    }
    default_dir
}

/// Recursively copy a directory tree.
pub fn copy_dir_all(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Clone the active config set into the next `user_config_n` directory.
pub fn clone_active_config_dir(root_dir: Option<&Path>, source_dir: Option<&Path>) -> std::io::Result<(i64, PathBuf)> {
    let root = root_or_default(root_dir);
    fs::create_dir_all(&root)?;
    let source = source_dir.map(Path::to_path_buf).unwrap_or_else(|| resolve_active_config_dir(Some(&root), false));
    let next_id = get_next_config_id(Some(&root));
    let target = get_config_dir(Some(&root), next_id);
    fs::create_dir(&target)?;
    if source.exists() {
        for entry in fs::read_dir(&source)? {
            let entry = entry?;
            let destination = target.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_dir_all(&entry.path(), &destination)?;
            } else {
                fs::copy(entry.path(), &destination)?;
            }
        }
    }
    Ok((next_id, target))
}

/// Resolve all runtime paths for the active config set and shared task data.
pub fn resolve_runtime_paths(root_dir: Option<&Path>) -> RuntimePaths {
    let root = root_or_default(root_dir);
    let active_id = resolve_active_config_id(Some(&root));
    let active_config_dir = resolve_active_config_dir(Some(&root), false);
    let tasks_dir = root.join("tasks");
    RuntimePaths {
        active_id: active_id.unwrap_or(0),
        settings_path: active_config_dir.join("settings.toml"),
        odd_path: active_config_dir.join("odd_weeks.toml"),
        even_path: active_config_dir.join("even_weeks.toml"),
        ddl_path: active_config_dir.join("ddl.json"),
        habit_path: active_config_dir.join("habits.toml"),
        profile_path: active_config_dir.join("profile.md"),
        tasks_path: tasks_dir.join("tasks.json"),
        task_log_path: tasks_dir.join("tasks.log"),
        record_path: tasks_dir.join("record.json"),
        procrastinate_path: tasks_dir.join("procrastinate.json"),
        mode_path: tasks_dir.join("mode.txt"),
        active_config_dir,
        root_dir: root,
    }
}

/// Runtime paths for the current environment (`resolve_runtime_paths()`).
pub fn runtime() -> RuntimePaths {
    resolve_runtime_paths(None)
}
