//! LaunchX integration: the browser workspace as a level 2 LaunchX service.
//!
//! LaunchX is a launcher for self-hosted local services (its contract is
//! `SPEC.md` in <https://github.com/AutoPhd-org/launchx>). It starts `rmd serve` with its address and
//! bearer token in the environment ([`Managed`]), puts the page behind its
//! gateway at `http://rmd.localhost:7600/`, and records every running service
//! in a run file ([`run_file`]). `rmd web` hands over to `lx open rmd` while a
//! launcher runs ([`hand_over`]).
//!
//! The service's interface (`/api/health`, `/api/state`, the `POST /api`
//! command channel, `/events`) is described in
//! `documentation/docs/advanced/launchx.md`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The service id `launchx.toml` gives the workspace.
pub const SERVICE_ID: &str = "rmd";

/// The version the health endpoint and `rmd --version` report.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What LaunchX hands a service it starts (contract §5.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Managed {
    /// `LAUNCHX_SERVICE`: the id the health endpoint reports.
    pub id: String,
    /// `LAUNCHX_HOST`: the address to bind.
    pub host: String,
    /// `LAUNCHX_PORT`.
    pub port: u16,
    /// `LAUNCHX_TOKEN`: the bearer token every request must carry.
    pub token: String,
}

impl Managed {
    /// The launcher's settings when `LAUNCHX` is set (managed mode), `None`
    /// otherwise; an error names what LaunchX left out.
    pub fn from_env() -> Result<Option<Self>, String> {
        if std::env::var_os("LAUNCHX").is_none() {
            return Ok(None);
        }
        let var = |name: &str| std::env::var(name).ok().map(|value| value.trim().to_string()).filter(|v| !v.is_empty());
        let (port, token) = (var("LAUNCHX_PORT"), var("LAUNCHX_TOKEN"));
        let missing: Vec<&str> = [("LAUNCHX_PORT", port.is_none()), ("LAUNCHX_TOKEN", token.is_none())]
            .into_iter()
            .filter_map(|(name, absent)| absent.then_some(name))
            .collect();
        if !missing.is_empty() {
            return Err(format!("LaunchX did not provide {}.", missing.join(" or ")));
        }
        let port = port.unwrap_or_default();
        let port = port
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
            .ok_or_else(|| format!("LAUNCHX_PORT is not a port: {port}"))?;
        Ok(Some(Self {
            id: var("LAUNCHX_SERVICE").unwrap_or_else(|| SERVICE_ID.to_string()),
            host: var("LAUNCHX_HOST").unwrap_or_else(|| "127.0.0.1".to_string()),
            port,
            token: token.unwrap_or_default(),
        }))
    }
}

/// `$LAUNCHX_HOME`, by default `~/.launchx`.
pub fn home() -> PathBuf {
    match std::env::var("LAUNCHX_HOME") {
        Ok(home) if !home.trim().is_empty() => crate::paths::expand_user(home.trim()),
        _ => crate::paths::home_dir().join(".launchx"),
    }
}

fn pid_alive(pid: i64) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        if pid <= 0 {
            return false;
        }
        // SAFETY: kill(2) with signal 0 only checks that the process exists.
        unsafe { libc::kill(pid, 0) == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM) }
    }
    #[cfg(not(unix))]
    {
        pid > 0
    }
}

/// The run file of a running LaunchX service (contract §10.2): its `pid`,
/// direct `url`, `token` and browser `ui`. `None` when there is none or its
/// process has exited.
pub fn run_file(id: &str) -> Option<Value> {
    let text = std::fs::read_to_string(home().join("run").join(format!("{id}.json"))).ok()?;
    let data: Value = serde_json::from_str(&text).ok()?;
    let pid = data.get("pid")?.as_i64()?;
    (data.is_object() && pid_alive(pid)).then_some(data)
}

/// Whether a LaunchX launcher is running.
pub fn launcher_running() -> bool {
    run_file("launchx").is_some()
}

/// Let a running launcher open the workspace: `lx open <id>` starts it if
/// needed and opens a signed-in tab. Returns the exit code when it did;
/// `None` when no launcher runs, `lx` is not on `PATH`, or `lx` failed (it
/// has said why), so the caller opens the workspace on its own.
pub fn hand_over(id: &str) -> Option<i32> {
    if !launcher_running() {
        return None;
    }
    hand_over_with(&crate::platform::which("lx")?, id)
}

fn hand_over_with(lx: &Path, id: &str) -> Option<i32> {
    match Command::new(lx).arg("open").arg(id).status() {
        Ok(status) if status.success() => Some(0),
        Ok(_) => {
            eprintln!("LaunchX could not open the workspace; opening it without LaunchX.");
            None
        }
        Err(error) => {
            eprintln!("Could not run {}: {error}; opening the workspace without LaunchX.", lx.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    fn clear_launchx(env: &mut TestEnv) {
        for name in ["LAUNCHX", "LAUNCHX_SERVICE", "LAUNCHX_HOST", "LAUNCHX_PORT", "LAUNCHX_TOKEN", "LAUNCHX_HOME"] {
            env.remove_var(name);
        }
    }

    #[test]
    fn managed_mode_reads_the_launcher_environment() {
        let mut env = TestEnv::new();
        clear_launchx(&mut env);
        assert_eq!(Managed::from_env(), Ok(None));
        env.set_var("LAUNCHX", "0.1");
        assert_eq!(Managed::from_env(), Err("LaunchX did not provide LAUNCHX_PORT or LAUNCHX_TOKEN.".into()));
        env.set_var("LAUNCHX_PORT", "53001");
        assert_eq!(Managed::from_env(), Err("LaunchX did not provide LAUNCHX_TOKEN.".into()));
        env.set_var("LAUNCHX_TOKEN", "secret");
        assert_eq!(
            Managed::from_env(),
            Ok(Some(Managed { id: "rmd".into(), host: "127.0.0.1".into(), port: 53001, token: "secret".into() }))
        );
        env.set_var("LAUNCHX_SERVICE", "rmd-dev");
        env.set_var("LAUNCHX_HOST", "::1");
        let managed = Managed::from_env().unwrap().unwrap();
        assert_eq!((managed.id.as_str(), managed.host.as_str()), ("rmd-dev", "::1"));
        env.set_var("LAUNCHX_PORT", "http");
        assert_eq!(Managed::from_env(), Err("LAUNCHX_PORT is not a port: http".into()));
    }

    #[test]
    fn run_files_count_only_while_their_process_lives() {
        let mut env = TestEnv::new();
        let home = env.root().join("launchx");
        env.set_var("LAUNCHX_HOME", home.to_str().unwrap());
        assert_eq!(super::home(), home);
        assert!(run_file("launchx").is_none() && !launcher_running());
        std::fs::create_dir_all(home.join("run")).unwrap();
        let ours = serde_json::json!({"id": "launchx", "pid": std::process::id(), "url": "http://127.0.0.1:7600"});
        std::fs::write(home.join("run/launchx.json"), ours.to_string()).unwrap();
        assert_eq!(run_file("launchx"), Some(ours));
        assert!(launcher_running());
        // A run file left behind by a process that has exited is no launcher.
        let mut child = Command::new("true").spawn().unwrap();
        let gone = child.id();
        child.wait().unwrap();
        std::fs::write(home.join("run/launchx.json"), format!(r#"{{"id": "launchx", "pid": {gone}}}"#)).unwrap();
        assert!(!launcher_running());
        std::fs::write(home.join("run/launchx.json"), "not json").unwrap();
        assert!(!launcher_running());
    }

    #[cfg(unix)]
    #[test]
    fn hand_over_runs_lx_open_and_falls_back_when_it_fails() {
        use std::os::unix::fs::PermissionsExt;
        let env = TestEnv::new();
        let script = |name: &str, body: &str| {
            let path = env.root().join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let log = env.root().join("lx.log");
        let ok = script("lx-ok", &format!("echo \"$@\" > '{}'", log.display()));
        assert_eq!(hand_over_with(&ok, "rmd"), Some(0));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "open rmd\n");
        let fails = script("lx-fails", "echo \"lx: Unknown service 'rmd'\" >&2; exit 1");
        assert_eq!(hand_over_with(&fails, "rmd"), None);
        assert_eq!(hand_over_with(&env.root().join("missing-lx"), "rmd"), None);
    }
}
