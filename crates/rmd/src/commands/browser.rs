//! `rmd web`: start or reuse the local browser workspace server and open a
//! tab (port of `commands/browser.py`).
//!
//! One server runs per config root; `.browser.json` in the root records its
//! port, session token and pid. A running server is reused only while it runs
//! the current code (its `/health` build equals [`code_fingerprint`]). An
//! outdated one is replaced on the same port with the same token, so open tabs
//! reconnect to the new server instead of pairing a new frontend with old APIs.
//!
//! [`code_fingerprint`]: crate::web::fingerprint::code_fingerprint

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail};
use serde_json::{Value, json};

use crate::i18n::tf;
use crate::{paths, py};

/// Carries an existing session token to a replacement server (by environment,
/// to keep it out of the process list).
const TOKEN_ENV: &str = "REMINDER_BROWSER_TOKEN";
const START_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const RECONNECT_TIMEOUT: Duration = Duration::from_secs(6);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

/// How the launcher reaches servers and the desktop; tests substitute a fake.
trait Backend {
    /// GET `route` from the server `state` describes, or POST `payload` to it.
    fn request(&self, state: &Value, route: &str, payload: Option<&Value>) -> Result<Value, String>;
    /// Start a detached `__web-server` process for `root`.
    fn spawn_server(&self, root: &Path, port: u16, token: Option<&str>) -> io::Result<Box<dyn ServerProcess>>;
    fn terminate(&self, pid: i64) -> io::Result<()>;
    fn open_url(&self, url: &str);
    /// Fingerprint of the code a freshly started server would run.
    fn fingerprint(&self) -> String;
}

/// A started server process.
trait ServerProcess {
    fn has_exited(&mut self) -> bool;
}

impl ServerProcess for Child {
    fn has_exited(&mut self) -> bool {
        matches!(self.try_wait(), Ok(Some(_)))
    }
}

/// The real servers and browser.
struct Desktop;

impl Backend for Desktop {
    fn request(&self, state: &Value, route: &str, payload: Option<&Value>) -> Result<Value, String> {
        http_request(state, route, payload)
    }

    fn spawn_server(&self, root: &Path, port: u16, token: Option<&str>) -> io::Result<Box<dyn ServerProcess>> {
        let log = OpenOptions::new().create(true).append(true).open(root.join(".browser.log"))?;
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("__web-server")
            .arg("--root")
            .arg(root)
            .arg("--port")
            .arg(port.to_string())
            .env("REMINDER_CONFIG_DIR", root)
            .env_remove(TOKEN_ENV)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        if let Some(token) = token {
            command.env(TOKEN_ENV, token);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: setsid(2) is async-signal-safe and only detaches the child
            // from this terminal's session (Python's `start_new_session=True`).
            unsafe {
                command.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        Ok(Box::new(command.spawn()?))
    }

    fn terminate(&self, pid: i64) -> io::Result<()> {
        crate::commands::service::send_sigterm(pid)
    }

    fn open_url(&self, url: &str) {
        let _ = open::that_detached(url);
    }

    fn fingerprint(&self) -> String {
        crate::web::fingerprint::code_fingerprint()
    }
}

/// `state["port"]` as Python's `int(...)` would read it.
fn state_port(state: &Value) -> Result<i64, String> {
    let port = state.get("port").ok_or_else(|| "'port'".to_string())?;
    py::py_int(port).ok_or_else(|| format!("invalid literal for int() with base 10: {}", py::py_repr(port)))
}

/// One JSON request to a local server, bypassing proxies.
fn http_request(state: &Value, route: &str, payload: Option<&Value>) -> Result<Value, String> {
    let port = state_port(state)?;
    if !(1..=65535).contains(&port) {
        return Err("Invalid port".to_string());
    }
    let token = state.get("token").map(py::py_str).ok_or_else(|| "'token'".to_string())?;
    let agent: ureq::Agent =
        ureq::Agent::config_builder().proxy(None).timeout_global(Some(REQUEST_TIMEOUT)).build().into();
    let url = format!("http://127.0.0.1:{port}{route}");
    let authorization = format!("Bearer {token}");
    let response = match payload {
        None => {
            agent.get(&url).header("Authorization", &authorization).header("Content-Type", "application/json").call()
        }
        Some(payload) => agent
            .post(&url)
            .header("Authorization", &authorization)
            .header("Content-Type", "application/json")
            .send(py::dumps(payload)),
    }
    .map_err(|error| error.to_string())?;
    let body = response.into_body().read_to_string().map_err(|error| error.to_string())?;
    serde_json::from_str(&body).map_err(|error| error.to_string())
}

/// Hold the launch lock for a config root (an OS lock, released even if a
/// launcher crashes).
fn launch_lock(root: &Path) -> io::Result<File> {
    let file = OpenOptions::new().create(true).append(true).read(true).open(root.join(".browser.lock"))?;
    file.lock()?;
    Ok(file)
}

/// The registered server with its health details, while it answers as itself.
fn live_state(backend: &dyn Backend, registry: &Path) -> Option<Value> {
    let state: Value = serde_json::from_str(&fs::read_to_string(registry).ok()?).ok()?;
    let health = backend.request(&state, "/health", None).ok()?;
    let health = health.as_object()?;
    if health.get("app") != Some(&json!("schedule-everything")) || health.get("pid") != state.get("pid") {
        return None;
    }
    let mut live = state.as_object()?.clone();
    live.insert("build".into(), health.get("build").cloned().unwrap_or(Value::Null));
    live.insert("tabs".into(), health.get("tabs").cloned().unwrap_or(Value::Null));
    Some(Value::Object(live))
}

/// Poll `condition` every `interval` until it holds or `timeout` passes.
fn wait(mut condition: impl FnMut() -> bool, timeout: Duration, interval: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while !condition() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(interval);
    }
    true
}

fn answers(backend: &dyn Backend, state: &Value) -> bool {
    backend.request(state, "/health", None).is_ok()
}

/// Whether the registry still names `pid`.
fn registered(registry: &Path, pid: &Value) -> bool {
    let Some(entry) = fs::read_to_string(registry).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok())
    else {
        return false;
    };
    entry.as_object().is_some_and(|entry| entry.get("pid").unwrap_or(&Value::Null) == pid)
}

/// Shut a server down and wait until it has released its port.
fn stop(backend: &dyn Backend, state: &Value, registry: &Path) -> anyhow::Result<()> {
    let _ = backend.request(state, "/api", Some(&json!({"command": "server_shutdown"})));
    let pid = state.get("pid").cloned().unwrap_or(Value::Null);
    if !wait(|| !answers(backend, state), STOP_TIMEOUT, POLL_INTERVAL) {
        // Still answering as the process its health check named, so stop it.
        if let Some(pid) = py::py_int(&pid) {
            let _ = backend.terminate(pid);
        }
        if !wait(|| !answers(backend, state), STOP_TIMEOUT, POLL_INTERVAL) {
            bail!("Could not stop the outdated browser service (process {}).", py::py_str(&pid));
        }
    }
    // Let it clear its registry entry so it cannot remove the replacement's.
    wait(|| !registered(registry, &pid), Duration::from_secs(1), POLL_INTERVAL);
    Ok(())
}

/// Start a server and wait for it to register; `None` if it exits first.
fn start(
    backend: &dyn Backend,
    root: &Path,
    registry: &Path,
    port: u16,
    token: Option<&str>,
) -> anyhow::Result<Option<Value>> {
    let mut process = backend.spawn_server(root, port, token.filter(|token| !token.is_empty()))?;
    let deadline = Instant::now() + START_TIMEOUT;
    while Instant::now() < deadline {
        // Any outdated server has stopped by now, so a live entry is this one.
        if let Some(state) = live_state(backend, registry) {
            return Ok(Some(state));
        }
        if process.has_exited() {
            return Ok(None);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    bail!("Browser server startup timed out. See {}", root.join(".browser.log").display())
}

/// The live server, replaced first if its code is outdated, and the replaced
/// server's state. A server is started only when `start_one` and none runs.
fn current_server(
    backend: &dyn Backend,
    root: &Path,
    registry: &Path,
    port: u16,
    start_one: bool,
) -> anyhow::Result<(Option<Value>, Option<Value>)> {
    let failed = || anyhow!("Browser server failed to start. See {}", root.join(".browser.log").display());
    let Some(state) = live_state(backend, registry) else {
        if !start_one {
            return Ok((None, None));
        }
        let state = start(backend, root, registry, port, None)?.ok_or_else(failed)?;
        return Ok((Some(state), None));
    };
    if state.get("build") == Some(&Value::String(backend.fingerprint())) {
        return Ok((Some(state), None));
    }
    stop(backend, &state, registry)?;
    // Reusing the port and token lets open tabs reconnect to the replacement.
    let old_port = u16::try_from(state_port(&state).map_err(|error| anyhow!(error))?)?;
    let token = state.get("token").map(py::py_str);
    let replacement = match start(backend, root, registry, old_port, token.as_deref())? {
        Some(replacement) => replacement,
        None => start(backend, root, registry, port, None)?.ok_or_else(failed)?,
    };
    Ok((Some(replacement), Some(state)))
}

/// Focus an open tab, giving tabs of a replaced server time to reconnect.
fn activate(backend: &dyn Backend, state: &Value, replaced: Option<&Value>) -> anyhow::Result<bool> {
    let had_no_tabs = |old: &Value| old.get("tabs").and_then(Value::as_f64) == Some(0.0);
    let reconnecting = replaced.is_some_and(|old| !had_no_tabs(old) && old.get("port") == state.get("port"));
    let mut found = false;
    let mut failure = None;
    let timeout = if reconnecting { RECONNECT_TIMEOUT } else { Duration::ZERO };
    wait(
        || match backend.request(state, "/activate", Some(&json!({}))) {
            Ok(Value::Object(response)) => {
                found = py::opt_truthy(response.get("hasTab"));
                found
            }
            Ok(other) => {
                failure = Some(format!("unexpected /activate response: {}", py::dumps(&other)));
                true
            }
            Err(error) => {
                failure = Some(error);
                true
            }
        },
        timeout,
        Duration::from_millis(200),
    );
    match failure {
        Some(error) => Err(anyhow!(error)),
        None => Ok(found),
    }
}

fn server_url(state: &Value) -> String {
    format!("http://127.0.0.1:{}/", state.get("port").map(py::py_str).unwrap_or_default())
}

/// Open the workspace. Returns its URL, whether an open tab was reused, and
/// whether an outdated server was replaced.
fn launch_browser(backend: &dyn Backend, open_browser: bool, port: u16) -> anyhow::Result<(String, bool, bool)> {
    let root = paths::resolve_config_root_dir();
    fs::create_dir_all(&root)?;
    let registry = root.join(".browser.json");
    let _lock = launch_lock(&root)?;
    let (state, replaced) = current_server(backend, &root, &registry, port, true)?;
    let state = state.ok_or_else(|| anyhow!("Browser server failed to start."))?;
    let url = server_url(&state);
    let has_tab = activate(backend, &state, replaced.as_ref())?;
    let private_url = format!("{url}#token={}", state.get("token").map(py::py_str).unwrap_or_default());
    if open_browser && !has_tab {
        backend.open_url(&private_url);
    }
    Ok((if open_browser { url } else { private_url }, has_tab, replaced.is_some()))
}

/// Replace a running workspace server whose code is outdated, without starting
/// one or touching tabs. Returns its URL (`None` when none runs) and whether it
/// was replaced.
fn refresh_browser(backend: &dyn Backend) -> anyhow::Result<(Option<String>, bool)> {
    let root = paths::resolve_config_root_dir();
    if !root.is_dir() {
        return Ok((None, false));
    }
    let registry = root.join(".browser.json");
    let (state, replaced) = {
        let _lock = launch_lock(&root)?;
        current_server(backend, &root, &registry, 0, false)?
    };
    Ok((state.as_ref().map(server_url), replaced.is_some()))
}

fn browser_command_with(backend: &dyn Backend, out: &mut dyn Write, no_open: bool, port: u16, if_running: bool) -> i32 {
    const RESTARTED: &str = "Browser workspace restarted with the updated code";
    let line = if if_running {
        refresh_browser(backend).map(|(url, replaced)| match url {
            None => "No browser workspace is running.".to_string(),
            Some(url) => format!("{}: {url}", if replaced { RESTARTED } else { "Browser workspace is up to date" }),
        })
    } else {
        launch_browser(backend, !no_open, port).map(|(url, reused, replaced)| {
            let status = if replaced {
                RESTARTED
            } else if reused {
                "Reusing open browser workspace"
            } else {
                "Browser workspace ready"
            };
            format!("{status}: {url}")
        })
    };
    match line {
        Ok(line) => {
            let _ = writeln!(out, "{line}");
            0
        }
        Err(error) => {
            let _ = writeln!(out, "{}", tf("❌ Unexpected error: {e}", &[("e", &error)]));
            1
        }
    }
}

/// `rmd web [--no-open] [--port N] [--if-running]`.
pub fn browser_command(no_open: bool, port: u16, if_running: bool) -> i32 {
    browser_command_with(&Desktop, &mut io::stdout(), no_open, port, if_running)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;
    use std::path::PathBuf;

    use super::*;
    use crate::test_support::TestEnv;

    /// A simulated server process, answering the Python server's protocol.
    struct SimServer {
        pid: i64,
        port: i64,
        token: String,
        build: String,
        tabs: i64,
        running: bool,
    }

    /// Simulated servers; spawning one registers it like `__web-server` does.
    struct Sim {
        registry: PathBuf,
        build: RefCell<String>,
        has_tab: Cell<bool>,
        servers: RefCell<Vec<SimServer>>,
        spawned: RefCell<Vec<(u16, Option<String>)>>,
        opened: RefCell<Vec<String>>,
        terminated: RefCell<Vec<i64>>,
    }

    impl Sim {
        fn new(root: &Path) -> Self {
            Self {
                registry: root.join(".browser.json"),
                build: RefCell::new("v1".to_string()),
                has_tab: Cell::new(false),
                servers: RefCell::new(Vec::new()),
                spawned: RefCell::new(Vec::new()),
                opened: RefCell::new(Vec::new()),
                terminated: RefCell::new(Vec::new()),
            }
        }

        fn add_server(&self, pid: i64, port: i64, token: &str, build: &str) {
            self.servers.borrow_mut().push(SimServer {
                pid,
                port,
                token: token.to_string(),
                build: build.to_string(),
                tabs: 0,
                running: true,
            });
        }

        fn registry(&self) -> Value {
            serde_json::from_str(&fs::read_to_string(&self.registry).unwrap()).unwrap()
        }

        fn is_running(&self, pid: i64) -> bool {
            self.servers.borrow().iter().any(|server| server.pid == pid && server.running)
        }
    }

    struct NeverExits;

    impl ServerProcess for NeverExits {
        fn has_exited(&mut self) -> bool {
            false
        }
    }

    impl Backend for Sim {
        fn request(&self, state: &Value, route: &str, payload: Option<&Value>) -> Result<Value, String> {
            let port = state_port(state)?;
            let token = state.get("token").map(py::py_str).ok_or("'token'")?;
            let mut servers = self.servers.borrow_mut();
            let server = servers
                .iter_mut()
                .find(|server| server.running && server.port == port)
                .ok_or("<urlopen error [Errno 61] Connection refused>")?;
            if server.token != token {
                return Err("HTTP Error 403: Forbidden".into());
            }
            match (route, payload) {
                ("/health", None) => Ok(json!({
                    "app": "schedule-everything", "pid": server.pid, "build": server.build, "tabs": server.tabs
                })),
                ("/activate", Some(_)) => Ok(json!({"hasTab": self.has_tab.get()})),
                ("/api", Some(payload)) if payload["command"] == "server_shutdown" => {
                    server.running = false;
                    if registered(&self.registry, &json!(server.pid)) {
                        fs::remove_file(&self.registry).unwrap();
                    }
                    Ok(json!({"ok": true, "data": {"message": "Browser service stopped."}}))
                }
                _ => Err("HTTP Error 404: Not Found".into()),
            }
        }

        fn spawn_server(&self, _root: &Path, port: u16, token: Option<&str>) -> io::Result<Box<dyn ServerProcess>> {
            self.spawned.borrow_mut().push((port, token.map(str::to_string)));
            let count = self.spawned.borrow().len() as i64;
            let pid = 1000 + count;
            let port = if port == 0 { 40000 + count } else { i64::from(port) };
            let token = token.map(str::to_string).unwrap_or_else(|| format!("token-{pid}"));
            self.add_server(pid, port, &token, &self.build.borrow());
            fs::write(&self.registry, json!({"port": port, "token": token, "pid": pid}).to_string())?;
            Ok(Box::new(NeverExits))
        }

        fn terminate(&self, pid: i64) -> io::Result<()> {
            self.terminated.borrow_mut().push(pid);
            Ok(())
        }

        fn open_url(&self, url: &str) {
            self.opened.borrow_mut().push(url.to_string());
        }

        fn fingerprint(&self) -> String {
            self.build.borrow().clone()
        }
    }

    #[test]
    fn launcher_reuses_tab_and_opens_again_when_no_tab() {
        let env = TestEnv::new();
        let sim = Sim::new(&paths::resolve_config_root_dir());
        sim.add_server(1, 12345, "t", "v1");
        env.write(".browser.json", r#"{"port": 12345, "token": "t", "pid": 1}"#);
        sim.has_tab.set(true);

        let (url, reused, replaced) = launch_browser(&sim, true, 0).unwrap();
        assert!(reused && !replaced && sim.opened.borrow().is_empty());
        assert_eq!(url, "http://127.0.0.1:12345/");

        sim.has_tab.set(false);
        launch_browser(&sim, true, 0).unwrap();
        assert_eq!(*sim.opened.borrow(), vec!["http://127.0.0.1:12345/#token=t".to_string()]);
        assert!(launch_browser(&sim, false, 0).unwrap().0.ends_with("#token=t"));
        assert_eq!(sim.opened.borrow().len(), 1);
        assert!(sim.spawned.borrow().is_empty());
    }

    #[test]
    fn stale_registry_does_not_attach_to_another_process() {
        let env = TestEnv::new();
        let sim = Sim::new(env.root());
        let registry = env.write("state.json", r#"{"port":12345,"token":"t","pid":42}"#);
        sim.add_server(43, 12345, "t", "v1");
        assert!(live_state(&sim, &registry).is_none());
        fs::write(&registry, r#"{"port":12345,"token":"t","pid":43}"#).unwrap();
        assert_eq!(live_state(&sim, &registry).unwrap()["build"], "v1");
        // A wrong token or a malformed registry is not a live server either.
        fs::write(&registry, r#"{"port":12345,"token":"x","pid":43}"#).unwrap();
        assert!(live_state(&sim, &registry).is_none());
        fs::write(&registry, "[1, 2]").unwrap();
        assert!(live_state(&sim, &registry).is_none());
    }

    #[test]
    fn refresh_does_not_start_a_server() {
        let env = TestEnv::new();
        let sim = Sim::new(env.root());
        assert_eq!(refresh_browser(&sim).unwrap(), (None, false));
        assert!(sim.spawned.borrow().is_empty());
        let (code, output) = {
            let mut out = Vec::new();
            (browser_command_with(&sim, &mut out, false, 0, true), String::from_utf8(out).unwrap())
        };
        assert_eq!((code, output.as_str()), (0, "No browser workspace is running.\n"));
    }

    #[test]
    fn outdated_server_is_replaced_on_the_same_port_and_token() {
        let _env = TestEnv::new();
        let root = paths::resolve_config_root_dir();
        let sim = Sim::new(&root);
        let (url, _, replaced) = launch_browser(&sim, false, 0).unwrap();
        let first = sim.registry();
        assert!(!replaced);
        assert_eq!(url, format!("http://127.0.0.1:{}/#token={}", first["port"], first["token"].as_str().unwrap()));
        let first_url = format!("http://127.0.0.1:{}/", first["port"]);
        assert_eq!(refresh_browser(&sim).unwrap(), (Some(first_url.clone()), false));

        // Code on disk changes after the server started.
        *sim.build.borrow_mut() = "updated".to_string();
        let (url, replaced) = refresh_browser(&sim).unwrap();
        let second = sim.registry();
        assert!(replaced);
        assert_eq!(url, Some(first_url));
        assert_ne!(second["pid"], first["pid"]);
        assert_eq!((&second["port"], &second["token"]), (&first["port"], &first["token"]));
        assert_eq!(live_state(&sim, &sim.registry).unwrap()["pid"], second["pid"]);
        assert!(!sim.is_running(first["pid"].as_i64().unwrap()));
        assert!(sim.terminated.borrow().is_empty());
        let spawned = sim.spawned.borrow();
        assert_eq!(spawned[1], (40001, Some(first["token"].as_str().unwrap().to_string())));
    }

    #[test]
    fn launch_reports_replacement_and_new_servers() {
        let env = TestEnv::new();
        let sim = Sim::new(env.root());
        let mut out = Vec::new();
        assert_eq!(browser_command_with(&sim, &mut out, true, 8765, false), 0);
        let registry = sim.registry();
        assert_eq!(registry["port"], 8765);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!("Browser workspace ready: http://127.0.0.1:8765/#token={}\n", registry["token"].as_str().unwrap())
        );
        assert!(sim.opened.borrow().is_empty());

        *sim.build.borrow_mut() = "v2".to_string();
        let mut out = Vec::new();
        assert_eq!(browser_command_with(&sim, &mut out, false, 0, false), 0);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "Browser workspace restarted with the updated code: http://127.0.0.1:8765/\n"
        );
        assert_eq!(sim.opened.borrow().len(), 1);
    }

    #[test]
    fn startup_failure_is_reported() {
        struct Fails;
        impl Backend for Fails {
            fn request(&self, _: &Value, _: &str, _: Option<&Value>) -> Result<Value, String> {
                Err("refused".into())
            }
            fn spawn_server(&self, _: &Path, _: u16, _: Option<&str>) -> io::Result<Box<dyn ServerProcess>> {
                struct Exited;
                impl ServerProcess for Exited {
                    fn has_exited(&mut self) -> bool {
                        true
                    }
                }
                Ok(Box::new(Exited))
            }
            fn terminate(&self, _: i64) -> io::Result<()> {
                Ok(())
            }
            fn open_url(&self, _: &str) {}
            fn fingerprint(&self) -> String {
                String::new()
            }
        }
        let _env = TestEnv::new();
        let mut out = Vec::new();
        assert_eq!(browser_command_with(&Fails, &mut out, false, 0, false), 1);
        let output = String::from_utf8(out).unwrap();
        assert!(output.starts_with("❌ Unexpected error: Browser server failed to start. See "), "{output}");
        assert!(output.trim_end().ends_with(".browser.log"));
    }

    /// Serve one canned JSON response; hands back the raw request.
    fn one_shot_server(body: &'static str) -> (u16, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut request = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse().unwrap();
                }
                request.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut payload = vec![0; length];
            reader.read_exact(&mut payload).unwrap();
            request.push_str(&String::from_utf8(payload).unwrap());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            reader.get_mut().write_all(response.as_bytes()).unwrap();
            request
        });
        (port, handle)
    }

    #[test]
    fn http_requests_carry_token_and_host_and_parse_json() {
        let (port, server) = one_shot_server(r#"{"app": "schedule-everything", "pid": 7}"#);
        let state = json!({"port": port, "token": "secret", "pid": 7});
        let health = http_request(&state, "/health", None).unwrap();
        assert_eq!(health, json!({"app": "schedule-everything", "pid": 7}));
        let request = server.join().unwrap().to_lowercase();
        assert!(request.starts_with("get /health http/1.1\r\n"), "{request}");
        assert!(request.contains("authorization: bearer secret\r\n"));
        assert!(request.contains(&format!("host: 127.0.0.1:{port}\r\n")));

        let (port, server) = one_shot_server(r#"{"hasTab": true}"#);
        let state = json!({"port": port, "token": "secret"});
        assert_eq!(http_request(&state, "/activate", Some(&json!({}))).unwrap(), json!({"hasTab": true}));
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /activate HTTP/1.1\r\n"));
        assert!(request.ends_with("\r\n\r\n{}"));
    }

    #[test]
    fn http_requests_validate_the_state() {
        assert_eq!(http_request(&json!({"port": 0, "token": "t"}), "/health", None), Err("Invalid port".into()));
        assert_eq!(http_request(&json!({"port": 70000, "token": "t"}), "/health", None), Err("Invalid port".into()));
        assert_eq!(http_request(&json!({"token": "t"}), "/health", None), Err("'port'".into()));
        assert_eq!(http_request(&json!({"port": 1}), "/health", None), Err("'token'".into()));
        // Nothing listens on a just-closed port.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert!(http_request(&json!({"port": port, "token": "t"}), "/health", None).is_err());
    }
}
