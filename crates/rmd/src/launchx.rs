//! LaunchX integration: the browser workspace as a level 2 LaunchX service.
//!
//! LaunchX is a launcher for self-hosted local services (its contract is
//! `SPEC.md` in <https://github.com/AutoPhd-org/launchx>). It starts `rmd serve` with its address and
//! bearer token in the environment ([`Managed`]), puts the page behind its
//! gateway at `http://rmd.localhost:7600/`, and records every running service
//! in a run file ([`run_file`]). While a launcher runs, `rmd web` lets it
//! open the workspace ([`hand_over`]).
//!
//! The service's interface (`/api/health`, `/api/state`, the `POST /api`
//! command channel, `/events`) is described in
//! `documentation/docs/advanced/launchx.md`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use crate::opener::{self, OpenIn};

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

/// Let a running launcher open the workspace, as `how` asks. For a tab,
/// `lx open <id>` starts the service if needed and opens a signed-in tab. For
/// a window, a workspace tab that is already open is asked to come forward;
/// without one, `lx open --print <id>` gives a signed-in link that opens in a
/// window of its own ([`opener`]). Returns the exit code when it opened the
/// workspace; `None` when no launcher runs, `lx` is not on `PATH`, or `lx`
/// failed (it has said why), so the caller opens the workspace on its own.
pub fn hand_over(id: &str, how: OpenIn) -> Option<i32> {
    if !launcher_running() {
        return None;
    }
    let lx = crate::platform::which("lx")?;
    if how != OpenIn::Tab {
        if focus_open_tab(id) {
            opener::bring_forward();
            return Some(0);
        }
        if let Some(link) = sign_in_link(&lx, id) {
            opener::open(&link, how);
            return Some(0);
        }
    }
    hand_over_with(&lx, id)
}

/// Ask the running service to focus its open workspace tab, straight at the
/// address and with the token its run file gives. Whether it has one.
fn focus_open_tab(id: &str) -> bool {
    let Some(run) = run_file(id) else {
        return false;
    };
    let (Some(url), Some(token)) = (run.get("url").and_then(Value::as_str), run.get("token").and_then(Value::as_str))
    else {
        return false;
    };
    let agent: ureq::Agent =
        ureq::Agent::config_builder().proxy(None).timeout_global(Some(Duration::from_secs(2))).build().into();
    agent
        .post(&format!("{}/activate", url.trim_end_matches('/')))
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .send("{}")
        .ok()
        .and_then(|response| response.into_body().read_to_string().ok())
        .and_then(|body| serde_json::from_str::<Value>(&body).ok())
        .is_some_and(|answer| opener::has_tab(&answer))
}

/// The one-time signed-in link `lx open --print <id>` prints; it starts the
/// service first when it is not running.
fn sign_in_link(lx: &Path, id: &str) -> Option<String> {
    let output = Command::new(lx).args(["open", "--print", id]).stderr(Stdio::inherit()).output().ok()?;
    output.status.success().then(|| opener::last_link(&String::from_utf8_lossy(&output.stdout))).flatten()
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

    #[cfg(unix)]
    #[test]
    fn a_window_opens_from_the_link_lx_prints() {
        use std::os::unix::fs::PermissionsExt;
        let env = TestEnv::new();
        let script = |name: &str, body: &str| {
            let path = env.root().join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let log = env.root().join("lx.log");
        let lx = script(
            "lx",
            &format!(
                "echo \"$@\" > '{}'; echo 'http://launchx.localhost:7600/.launchx/login?code=one-time'",
                log.display()
            ),
        );
        assert_eq!(
            sign_in_link(&lx, "rmd").as_deref(),
            Some("http://launchx.localhost:7600/.launchx/login?code=one-time")
        );
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "open --print rmd\n");
        let fails = script("lx-fails", "echo 'lx: LaunchX is not running.' >&2; exit 1");
        assert_eq!(sign_in_link(&fails, "rmd"), None);
    }

    #[test]
    fn an_open_tab_is_found_through_the_run_file() {
        use std::io::{Read, Write};
        let mut env = TestEnv::new();
        let home = env.root().join("launchx");
        env.set_var("LAUNCHX_HOME", home.to_str().unwrap());
        std::fs::create_dir_all(home.join("run")).unwrap();
        assert!(!focus_open_tab("rmd"));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = vec![0u8; 4096];
            let read = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..read]).to_string();
            let body = r#"{"hasTab": true}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            request
        });
        let run = serde_json::json!({"id": "rmd", "pid": std::process::id(), "url": format!("http://127.0.0.1:{port}"), "token": "secret"});
        std::fs::write(home.join("run/rmd.json"), run.to_string()).unwrap();
        assert!(focus_open_tab("rmd"));
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /activate "));
        assert!(request.to_lowercase().contains("authorization: bearer secret"));
    }
}
