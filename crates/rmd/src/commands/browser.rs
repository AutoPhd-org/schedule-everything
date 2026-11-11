//! `rmd web`: start or reuse the local browser workspace server and open a
//! tab.
//!
//! One server runs per config root; `.browser.json` in the root records its
//! port, session token and pid. A running server is reused only while it runs
//! the current code (its `/health` build equals [`code_fingerprint`]). An
//! outdated one is replaced on the same port with the same token, so open tabs
//! reconnect to the new server instead of pairing a new frontend with old APIs.
//! A server is replaced the same way when phone access is turned on or off,
//! since that changes where it listens.
//!
//! While a LaunchX launcher runs, a plain `rmd web` lets it open the
//! workspace instead (`lx open rmd`, see [`crate::launchx`]).
//!
//! [`code_fingerprint`]: crate::web::fingerprint::code_fingerprint

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::net::Ipv4Addr;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail};
use serde_json::{Value, json};

use crate::i18n::tf;
use crate::web::phone::{self, PhoneAccess};
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
    /// Addresses a phone can reach this computer at, best first.
    fn network_addresses(&self) -> Vec<Ipv4Addr>;
    /// Whether another VPN carries the traffic for this Tailscale address.
    fn tailscale_captured(&self, tailscale: Ipv4Addr) -> bool;
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

    fn network_addresses(&self) -> Vec<Ipv4Addr> {
        phone::network_addresses()
    }

    fn tailscale_captured(&self, tailscale: Ipv4Addr) -> bool {
        phone::tailscale_captured(tailscale)
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
    live.insert("phone".into(), Value::Bool(py::opt_truthy(health.get("phone"))));
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

/// The live server, replaced first if its code is outdated or it listens for
/// phones when phone access is off (or the other way round), and the replaced
/// server's state. A server is started only when `start_one` and none runs.
fn current_server(
    backend: &dyn Backend,
    root: &Path,
    registry: &Path,
    port: u16,
    start_one: bool,
) -> anyhow::Result<(Option<Value>, Option<Value>)> {
    let failed = || anyhow!("Browser server failed to start. See {}", root.join(".browser.log").display());
    let access = phone::load(root);
    let Some(state) = live_state(backend, registry) else {
        if !start_one {
            return Ok((None, None));
        }
        // Paired phones know the port phone access last used, so a new server
        // takes it again while it is free.
        let preferred = if port == 0 { access.as_ref().map_or(0, |access| access.port) } else { port };
        let state = match start(backend, root, registry, preferred, None)? {
            Some(state) => state,
            None if preferred != port => start(backend, root, registry, port, None)?.ok_or_else(failed)?,
            None => return Err(failed()),
        };
        remember_phone_port(root, access, &state)?;
        return Ok((Some(state), None));
    };
    let up_to_date = state.get("build") == Some(&Value::String(backend.fingerprint()));
    if up_to_date && state.get("phone") == Some(&Value::Bool(access.is_some())) {
        remember_phone_port(root, access, &state)?;
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
    remember_phone_port(root, access, &replacement)?;
    Ok((Some(replacement), Some(state)))
}

/// With phone access on, record the server's port for the next server to reuse.
fn remember_phone_port(root: &Path, access: Option<PhoneAccess>, state: &Value) -> anyhow::Result<()> {
    let Some(mut access) = access else {
        return Ok(());
    };
    let port = u16::try_from(state_port(state).map_err(|error| anyhow!(error))?)?;
    if access.port != port {
        access.port = port;
        phone::save(root, &access)?;
    }
    Ok(())
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

/// What opening the workspace found and did.
#[derive(Debug)]
struct Launch {
    /// The workspace URL (with the session token when no tab was opened).
    url: String,
    /// Whether an open tab was reused.
    reused: bool,
    /// Why a running server was replaced, if it was.
    replaced: Option<Replaced>,
    /// Phone access, while it is on.
    phone: Option<PhoneAccess>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Replaced {
    /// It ran outdated code.
    Outdated,
    /// Phone access was turned on or off.
    PhoneAccess,
}

/// Open the workspace, turning phone access on or off first when `phone` says so.
fn launch_browser(backend: &dyn Backend, open_browser: bool, port: u16, phone: Option<bool>) -> anyhow::Result<Launch> {
    let root = paths::resolve_config_root_dir();
    fs::create_dir_all(&root)?;
    let registry = root.join(".browser.json");
    let _lock = launch_lock(&root)?;
    match phone {
        Some(true) => {
            phone::enable(&root)?;
        }
        Some(false) => phone::disable(&root)?,
        None => {}
    }
    let (state, replaced) = current_server(backend, &root, &registry, port, true)?;
    let state = state.ok_or_else(|| anyhow!("Browser server failed to start."))?;
    let url = server_url(&state);
    let reused = activate(backend, &state, replaced.as_ref())?;
    let private_url = format!("{url}#token={}", state.get("token").map(py::py_str).unwrap_or_default());
    if open_browser && !reused {
        backend.open_url(&private_url);
    }
    let fingerprint = Value::String(backend.fingerprint());
    let replaced = replaced
        .map(|old| if old.get("build") == Some(&fingerprint) { Replaced::PhoneAccess } else { Replaced::Outdated });
    Ok(Launch { url: if open_browser { url } else { private_url }, reused, replaced, phone: phone::load(&root) })
}

/// Start the workspace server for paired phones when phone access is on and
/// none runs (the reminder service calls this at login), without opening or
/// focusing a tab. Returns a line for the service log.
pub fn serve_phones() -> Option<String> {
    serve_phones_with(&Desktop)
}

fn serve_phones_with(backend: &dyn Backend) -> Option<String> {
    let root = paths::resolve_config_root_dir();
    phone::load(&root)?;
    let registry = root.join(".browser.json");
    let started = launch_lock(&root)
        .map_err(anyhow::Error::from)
        .and_then(|_lock| current_server(backend, &root, &registry, 0, true));
    Some(match started {
        Ok((Some(state), _)) => format!("Workspace serving paired phones on port {}", py::py_str(&state["port"])),
        Ok((None, _)) => "Could not start the workspace for paired phones".to_string(),
        Err(error) => format!("Could not start the workspace for paired phones: {error}"),
    })
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

/// What `rmd web` was asked to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WebRequest {
    /// Start or reuse the server without opening a tab.
    pub no_open: bool,
    /// The port for a new server (0 picks one).
    pub port: u16,
    /// Only replace a running server whose code is outdated.
    pub if_running: bool,
    /// Turn phone access on (`Some(true)`) or off (`Some(false)`).
    pub phone: Option<bool>,
}

/// How a phone pairs: for the local network and for Tailscale, a QR code of
/// the pairing link and the link itself. A phone signed in at one address is
/// not signed in at the other, so each has its own code.
fn pairing_text(backend: &dyn Backend, access: &PhoneAccess, addresses: &[Ipv4Addr], color: bool) -> String {
    if addresses.is_empty() {
        return "Phone access is on, but this computer has no network address. \
                Connect to Wi-Fi, then run `rmd web --phone` again.\n"
            .to_string();
    }
    let (tailscale, local): (Vec<Ipv4Addr>, Vec<Ipv4Addr>) =
        addresses.iter().partition(|&&address| phone::is_tailscale(address));
    let mut text =
        String::from("Phone access is on. Scan a code with your phone's camera, or open its link on the phone.\n");
    for (heading, group) in
        [("On the same Wi-Fi as this computer:", &local), ("Over Tailscale, from anywhere:", &tailscale)]
    {
        let Some(&first) = group.first() else {
            continue;
        };
        text += &format!("\n{heading}\n\n");
        for line in phone::qr_lines(&access.pairing_url(first), color) {
            text += &format!("  {line}\n");
        }
        text += "\n";
        for &address in group {
            text += &format!("  {}\n", access.pairing_url(address));
        }
        if phone::is_tailscale(first) && backend.tailscale_captured(first) {
            text += "\nAnother VPN on this computer is carrying Tailscale's traffic, so a phone's \
                     requests arrive but the replies never reach it. Disconnect that VPN while \
                     you use your phone, then run `rmd web --phone` again.\n";
        }
    }
    text += "\nThis computer must be awake. A link signs a phone in, so keep it private and \
             use phone access only on networks you trust. `rmd web --no-phone` turns phone \
             access off and signs every phone out.\n";
    text
}

/// Where paired phones connect, for `rmd web` while phone access is on.
fn phone_access_line(access: &PhoneAccess, addresses: &[Ipv4Addr]) -> Option<String> {
    let places: Vec<String> = addresses
        .iter()
        .map(|&address| {
            if phone::is_tailscale(address) {
                format!("{} over Tailscale", access.url(address))
            } else {
                access.url(address)
            }
        })
        .collect();
    (!places.is_empty())
        .then(|| format!("Phone access is on: {} (`rmd web --phone` shows the pairing codes)\n", places.join(", ")))
}

fn browser_command_with(backend: &dyn Backend, out: &mut dyn Write, color: bool, request: WebRequest) -> i32 {
    const RESTARTED: &str = "Browser workspace restarted with the updated code";
    let text = if request.if_running {
        refresh_browser(backend).map(|(url, replaced)| match url {
            None => "No browser workspace is running.\n".to_string(),
            Some(url) => format!("{}: {url}\n", if replaced { RESTARTED } else { "Browser workspace is up to date" }),
        })
    } else {
        launch_browser(backend, !request.no_open, request.port, request.phone).map(|launch| {
            let status = match (launch.replaced, &launch.phone) {
                (Some(Replaced::Outdated), _) => RESTARTED,
                (Some(Replaced::PhoneAccess), Some(_)) => "Browser workspace restarted with phone access",
                (Some(Replaced::PhoneAccess), None) => "Browser workspace restarted without phone access",
                (None, _) if launch.reused => "Reusing open browser workspace",
                (None, _) => "Browser workspace ready",
            };
            let mut text = format!("{status}: {}\n", launch.url);
            let addresses = || backend.network_addresses();
            match (&launch.phone, request.phone) {
                (Some(access), Some(true)) => {
                    text += &format!("\n{}", pairing_text(backend, access, &addresses(), color))
                }
                (Some(access), _) => text += &phone_access_line(access, &addresses()).unwrap_or_default(),
                (None, Some(false)) => {
                    text += "Phone access is off. Phones need a new pairing code to connect again.\n"
                }
                (None, _) => {}
            }
            text
        })
    };
    match text {
        Ok(text) => {
            let _ = write!(out, "{text}");
            0
        }
        Err(error) => {
            let _ = writeln!(out, "{}", tf("❌ Unexpected error: {e}", &[("e", &error)]));
            1
        }
    }
}

/// `rmd web [--no-open] [--port N] [--if-running] [--phone | --no-phone]`.
///
/// A plain `rmd web` hands over to `lx open rmd` while a LaunchX launcher
/// runs; the options are about the workspace's own server, so they keep it.
pub fn browser_command(request: WebRequest) -> i32 {
    if request == WebRequest::default()
        && let Some(code) = crate::launchx::hand_over(crate::launchx::SERVICE_ID)
    {
        return code;
    }
    let color = crate::ui::detect_color_system().is_some();
    browser_command_with(&Desktop, &mut io::stdout(), color, request)
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
        phone: bool,
        running: bool,
    }

    /// Simulated servers; spawning one registers it like `__web-server` does.
    struct Sim {
        registry: PathBuf,
        build: RefCell<String>,
        has_tab: Cell<bool>,
        tailscale_captured: Cell<bool>,
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
                tailscale_captured: Cell::new(false),
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
                phone: false,
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
                    "app": "schedule-everything", "pid": server.pid, "build": server.build, "tabs": server.tabs,
                    "phone": server.phone
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

        fn spawn_server(&self, root: &Path, port: u16, token: Option<&str>) -> io::Result<Box<dyn ServerProcess>> {
            self.spawned.borrow_mut().push((port, token.map(str::to_string)));
            let count = self.spawned.borrow().len() as i64;
            let pid = 1000 + count;
            let port = if port == 0 { 40000 + count } else { i64::from(port) };
            // A taken port: the process exits without registering.
            if self.servers.borrow().iter().any(|server| server.running && server.port == port) {
                struct Exited;
                impl ServerProcess for Exited {
                    fn has_exited(&mut self) -> bool {
                        true
                    }
                }
                return Ok(Box::new(Exited));
            }
            let token = token.map(str::to_string).unwrap_or_else(|| format!("token-{pid}"));
            self.add_server(pid, port, &token, &self.build.borrow());
            // Like `__web-server`, the server serves phones when phone access is on at startup.
            self.servers.borrow_mut().last_mut().unwrap().phone = phone::load(root).is_some();
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

        fn network_addresses(&self) -> Vec<Ipv4Addr> {
            vec![Ipv4Addr::new(192, 168, 1, 20), Ipv4Addr::new(100, 101, 102, 103)]
        }

        fn tailscale_captured(&self, _: Ipv4Addr) -> bool {
            self.tailscale_captured.get()
        }
    }

    #[test]
    fn launcher_reuses_tab_and_opens_again_when_no_tab() {
        let env = TestEnv::new();
        let sim = Sim::new(&paths::resolve_config_root_dir());
        sim.add_server(1, 12345, "t", "v1");
        env.write(".browser.json", r#"{"port": 12345, "token": "t", "pid": 1}"#);
        sim.has_tab.set(true);

        let launch = launch_browser(&sim, true, 0, None).unwrap();
        assert!(launch.reused && launch.replaced.is_none() && sim.opened.borrow().is_empty());
        assert_eq!(launch.url, "http://127.0.0.1:12345/");

        sim.has_tab.set(false);
        launch_browser(&sim, true, 0, None).unwrap();
        assert_eq!(*sim.opened.borrow(), vec!["http://127.0.0.1:12345/#token=t".to_string()]);
        assert!(launch_browser(&sim, false, 0, None).unwrap().url.ends_with("#token=t"));
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
            let request = WebRequest { if_running: true, ..WebRequest::default() };
            (browser_command_with(&sim, &mut out, false, request), String::from_utf8(out).unwrap())
        };
        assert_eq!((code, output.as_str()), (0, "No browser workspace is running.\n"));
    }

    #[test]
    fn outdated_server_is_replaced_on_the_same_port_and_token() {
        let _env = TestEnv::new();
        let root = paths::resolve_config_root_dir();
        let sim = Sim::new(&root);
        let launch = launch_browser(&sim, false, 0, None).unwrap();
        let first = sim.registry();
        assert!(launch.replaced.is_none());
        assert_eq!(
            launch.url,
            format!("http://127.0.0.1:{}/#token={}", first["port"], first["token"].as_str().unwrap())
        );
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
        assert_eq!(
            browser_command_with(
                &sim,
                &mut out,
                false,
                WebRequest { no_open: true, port: 8765, ..WebRequest::default() }
            ),
            0
        );
        let registry = sim.registry();
        assert_eq!(registry["port"], 8765);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!("Browser workspace ready: http://127.0.0.1:8765/#token={}\n", registry["token"].as_str().unwrap())
        );
        assert!(sim.opened.borrow().is_empty());

        *sim.build.borrow_mut() = "v2".to_string();
        let mut out = Vec::new();
        assert_eq!(browser_command_with(&sim, &mut out, false, WebRequest::default()), 0);
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
            fn network_addresses(&self) -> Vec<Ipv4Addr> {
                Vec::new()
            }
            fn tailscale_captured(&self, _: Ipv4Addr) -> bool {
                false
            }
        }
        let _env = TestEnv::new();
        let mut out = Vec::new();
        assert_eq!(browser_command_with(&Fails, &mut out, false, WebRequest::default()), 1);
        let output = String::from_utf8(out).unwrap();
        assert!(output.starts_with("❌ Unexpected error: Browser server failed to start. See "), "{output}");
        assert!(output.trim_end().ends_with(".browser.log"));
    }

    fn web(sim: &Sim, request: WebRequest) -> String {
        let mut out = Vec::new();
        assert_eq!(browser_command_with(sim, &mut out, false, request), 0);
        String::from_utf8(out).unwrap()
    }

    fn running(sim: &Sim) -> Vec<(i64, i64, String, bool)> {
        let servers = sim.servers.borrow();
        servers.iter().filter(|s| s.running).map(|s| (s.pid, s.port, s.token.clone(), s.phone)).collect()
    }

    #[test]
    fn phone_access_restarts_the_server_in_place_and_shows_a_pairing_code() {
        let env = TestEnv::new();
        let sim = Sim::new(env.root());
        let local = WebRequest { no_open: true, port: 8765, ..WebRequest::default() };
        web(&sim, local);
        let desktop_token = sim.registry()["token"].as_str().unwrap().to_string();
        assert_eq!(phone::load(env.root()), None);

        let output = web(&sim, WebRequest { phone: Some(true), ..WebRequest::default() });
        // Same port and token, so the tab on this computer reconnects.
        assert_eq!(running(&sim), vec![(1002, 8765, desktop_token.clone(), true)]);
        let access = phone::load(env.root()).unwrap();
        assert_eq!(access.port, 8765);
        assert_ne!(access.token, desktop_token);
        let pairing = format!("http://192.168.1.20:8765/#token={}", access.token);
        let tailscale = format!("http://100.101.102.103:8765/#token={}", access.token);
        assert!(output.starts_with("Browser workspace restarted with phone access: http://127.0.0.1:8765/\n\n"));
        // A code and a link for each network: a phone signed in at one address is not at the other.
        let wifi_code = output.find("On the same Wi-Fi as this computer:\n\n  ").expect("Wi-Fi code");
        let tailscale_code = output.find("Over Tailscale, from anywhere:\n\n  ").expect("Tailscale code");
        assert!(wifi_code < tailscale_code);
        assert!(output[wifi_code..tailscale_code].contains(&format!("\n  {pairing}\n")), "{output}");
        assert!(output[tailscale_code..].contains(&format!("\n  {tailscale}\n")), "{output}");
        assert!(!output.contains("Another VPN"));
        assert!(output.contains("`rmd web --no-phone` turns phone access off"));

        // A plain launch keeps phone access and reminds where phones connect.
        assert_eq!(
            web(&sim, WebRequest::default()),
            "Browser workspace ready: http://127.0.0.1:8765/\n\
             Phone access is on: http://192.168.1.20:8765/, http://100.101.102.103:8765/ over Tailscale \
             (`rmd web --phone` shows the pairing codes)\n"
        );
        // Showing the code again keeps phones paired and the server running.
        let again = web(&sim, WebRequest { phone: Some(true), ..WebRequest::default() });
        assert!(again.starts_with("Browser workspace ready: ") && again.contains(&pairing), "{again}");
        assert_eq!(running(&sim).len(), 1);
        assert_eq!(phone::load(env.root()).unwrap(), access);

        let output = web(&sim, WebRequest { phone: Some(false), ..WebRequest::default() });
        assert_eq!(
            output,
            "Browser workspace restarted without phone access: http://127.0.0.1:8765/\n\
             Phone access is off. Phones need a new pairing code to connect again.\n"
        );
        assert_eq!(running(&sim), vec![(1003, 8765, desktop_token, false)]);
        assert_eq!(phone::load(env.root()), None);
        let output = web(&sim, WebRequest { phone: Some(true), ..WebRequest::default() });
        assert!(!output.contains(&access.token), "turning it on again pairs anew");
    }

    #[test]
    fn a_new_server_takes_the_port_paired_phones_know() {
        let env = TestEnv::new();
        let sim = Sim::new(env.root());
        phone::save(env.root(), &PhoneAccess { token: "phone".into(), port: 8765 }).unwrap();
        let launch = launch_browser(&sim, false, 0, None).unwrap();
        assert_eq!(running(&sim), vec![(1001, 8765, "token-1001".into(), true)]);
        assert_eq!(launch.phone, Some(PhoneAccess { token: "phone".into(), port: 8765 }));

        // When another program holds that port, the server takes a free one and phones follow it.
        sim.servers.borrow_mut()[0].running = false;
        fs::remove_file(&sim.registry).unwrap();
        sim.add_server(7, 8765, "someone-else", "v1");
        launch_browser(&sim, false, 0, None).unwrap();
        assert_eq!(*sim.spawned.borrow(), vec![(8765, None), (8765, None), (0, None)]);
        assert_eq!(phone::load(env.root()).unwrap().port, 40003);
    }

    #[test]
    fn a_port_asked_for_wins_over_the_one_phones_know() {
        let env = TestEnv::new();
        let sim = Sim::new(env.root());
        phone::save(env.root(), &PhoneAccess { token: "phone".into(), port: 8765 }).unwrap();
        launch_browser(&sim, false, 9000, None).unwrap();
        assert_eq!(phone::load(env.root()).unwrap().port, 9000);
    }

    #[test]
    fn the_reminder_service_starts_the_server_only_for_phones() {
        let env = TestEnv::new();
        let sim = Sim::new(env.root());
        assert_eq!(serve_phones_with(&sim), None);
        assert!(sim.spawned.borrow().is_empty());
        phone::enable(env.root()).unwrap();
        assert_eq!(serve_phones_with(&sim).unwrap(), "Workspace serving paired phones on port 40001");
        sim.has_tab.set(true);
        assert_eq!(serve_phones_with(&sim).unwrap(), "Workspace serving paired phones on port 40001");
        assert_eq!(sim.spawned.borrow().len(), 1, "a running server is reused");
        assert!(sim.opened.borrow().is_empty(), "no tab opens at login");
    }

    #[test]
    fn pairing_warns_when_another_vpn_carries_tailscale_traffic() {
        let env = TestEnv::new();
        let sim = Sim::new(env.root());
        let access = PhoneAccess { token: "t".into(), port: 8765 };
        let addresses = sim.network_addresses();
        sim.tailscale_captured.set(true);
        let text = pairing_text(&sim, &access, &addresses, false);
        let warning = text.find("Another VPN on this computer is carrying Tailscale's traffic").expect("warning");
        assert!(warning > text.find("http://100.101.102.103:8765/#token=t").unwrap(), "under the Tailscale code");
        // Without Tailscale there is nothing to warn about.
        let text = pairing_text(&sim, &access, &addresses[..1], false);
        assert!(!text.contains("Over Tailscale") && !text.contains("Another VPN"), "{text}");
    }

    #[test]
    fn pairing_needs_a_network_address() {
        let env = TestEnv::new();
        let sim = Sim::new(env.root());
        let access = PhoneAccess { token: "t".into(), port: 8765 };
        assert_eq!(
            pairing_text(&sim, &access, &[], false),
            "Phone access is on, but this computer has no network address. \
             Connect to Wi-Fi, then run `rmd web --phone` again.\n"
        );
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
