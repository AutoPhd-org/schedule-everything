//! Test helpers: an isolated config root with a process-wide lock, because the
//! environment (`REMINDER_CONFIG_DIR`, `RMD_NOW`, ...) is global to the test
//! process and tests run on parallel threads.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Variables every test starts without.
const MANAGED_VARS: [&str; 5] =
    ["REMINDER_CONFIG_DIR", "RMD_NOW", "REMINDER_LANG", "REMINDER_SYNCED_SCHEDULE_PATH", "REMINDER_CONFERENCE_CACHE_PATH"];

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// An isolated config root. Holds the environment lock until dropped and
/// restores every variable it changed.
pub struct TestEnv {
    _guard: MutexGuard<'static, ()>,
    root: tempfile::TempDir,
    saved: Vec<(String, Option<OsString>)>,
}

impl TestEnv {
    /// Empty config root (`REMINDER_CONFIG_DIR` points at it).
    pub fn new() -> Self {
        let guard = env_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = tempfile::tempdir().expect("create temp config root");
        let mut env = Self { _guard: guard, root, saved: Vec::new() };
        for var in MANAGED_VARS {
            env.remove_var(var);
        }
        let root_path = env.root().to_path_buf();
        env.set_var("REMINDER_CONFIG_DIR", root_path.to_str().expect("utf-8 temp path"));
        env
    }

    /// Config root with a copy of `tests/config` (the Python test fixture:
    /// `user_config_0` with settings, odd/even weeks, tasks, ...).
    pub fn with_fixture() -> Self {
        let env = Self::new();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/config");
        crate::paths::copy_dir_all(&fixture, env.root()).expect("copy tests/config fixture");
        env
    }

    /// The temporary config root.
    pub fn root(&self) -> &Path {
        self.root.path()
    }

    /// `root/user_config_0` (created if missing).
    pub fn config_dir(&self) -> PathBuf {
        let dir = self.root().join("user_config_0");
        std::fs::create_dir_all(&dir).expect("create user_config_0");
        dir
    }

    /// Write a file relative to the config root, creating parent directories.
    pub fn write(&self, relative: &str, content: &str) -> PathBuf {
        let path = self.root().join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(&path, content).expect("write test file");
        path
    }

    /// Read a file relative to the config root.
    pub fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.root().join(relative)).expect("read test file")
    }

    /// Freeze the clock at a local time such as `"2026-11-09T10:30:00"`.
    pub fn set_now(&mut self, now: &str) {
        self.set_var(crate::clock::NOW_ENV, now);
    }

    fn remember(&mut self, key: &str) {
        if !self.saved.iter().any(|(k, _)| k == key) {
            self.saved.push((key.to_string(), std::env::var_os(key)));
        }
    }

    /// Set an environment variable until the `TestEnv` is dropped.
    pub fn set_var(&mut self, key: &str, value: &str) {
        self.remember(key);
        // SAFETY: all environment mutation in tests happens while holding the lock.
        unsafe { std::env::set_var(key, value) };
    }

    /// Remove an environment variable until the `TestEnv` is dropped.
    pub fn remove_var(&mut self, key: &str) {
        self.remember(key);
        // SAFETY: all environment mutation in tests happens while holding the lock.
        unsafe { std::env::remove_var(key) };
    }
}

impl Default for TestEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..).rev() {
            // SAFETY: the lock is still held (the guard field drops after this).
            unsafe {
                match value {
                    Some(v) => std::env::set_var(&key, v),
                    None => std::env::remove_var(&key),
                }
            }
        }
    }
}
