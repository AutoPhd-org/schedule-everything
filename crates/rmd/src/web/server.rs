//! Authenticated loopback HTTP server for the bundled browser interface (port
//! of `web/server.py`).
//!
//! A server owns one config root. SSE connections identify open browser tabs
//! and carry activation requests; API mutations are serialized within this
//! process. The server reports a fingerprint of its build so the launcher can
//! replace it after an update; open tabs reconnect to the replacement on the
//! same port with the same token.
//!
//! The server listens on `127.0.0.1` only, unless phone access is on (see
//! [`phone`]): then it listens on every interface and also accepts requests
//! addressed to this computer's IPv4 address with the paired phones' token.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::Ipv4Addr;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use axum::Router;
use axum::body::{Body, Bytes, HttpBody};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::Response;
use http_body::Frame;
use rust_embed::RustEmbed;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};

use crate::paths;
use crate::py;
use crate::web::fingerprint::code_fingerprint;
use crate::web::{TOKEN_ENV, api, phone, pyjson, workspace};

/// The built browser app (`web/src`, bundled by Vite).
#[derive(RustEmbed)]
#[folder = "web-static"]
pub(crate) struct Assets;

pub const MAX_BODY: usize = 8 * 1024 * 1024;
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'";
/// Seconds between heartbeats on an idle event stream.
const HEARTBEAT_SECONDS: u64 = 2;
/// How long open requests may finish after a shutdown request.
const SHUTDOWN_GRACE_SECONDS: u64 = 2;

/// Which token a request carried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    /// The session token of tabs on this computer.
    Desktop,
    /// The token of paired phones.
    Phone,
}

/// An open browser tab's event channel.
struct Tab {
    /// Whether the tab runs on this computer (it reached the server at `127.0.0.1`).
    local: bool,
    events: mpsc::Sender<&'static str>,
}

/// State shared by every connection of one server.
pub struct BrowserServer {
    pub port: u16,
    pub token: String,
    pub build: String,
    /// The paired phones' token, while phone access is on.
    phone_token: Option<String>,
    cookie_name: String,
    tabs: Mutex<HashMap<u64, Tab>>,
    next_tab: AtomicU64,
    closing: AtomicBool,
    operation_lock: Arc<Mutex<()>>,
    shutdown: watch::Sender<bool>,
}

impl BrowserServer {
    pub fn new(port: u16, token: String, build: String) -> Self {
        Self {
            port,
            token,
            build,
            phone_token: None,
            cookie_name: format!("rmd_{port}"),
            tabs: Mutex::new(HashMap::new()),
            next_tab: AtomicU64::new(0),
            closing: AtomicBool::new(false),
            operation_lock: Arc::new(Mutex::new(())),
            shutdown: watch::Sender::new(false),
        }
    }

    /// Also serve paired phones, which authenticate with `token`.
    pub fn with_phone_access(mut self, token: String) -> Self {
        self.phone_token = Some(token);
        self
    }

    fn tabs(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Tab>> {
        self.tabs.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Register an open browser tab, `local` when it runs on this computer;
    /// its events arrive on the receiver.
    pub fn add_tab(&self, local: bool) -> (u64, mpsc::Receiver<&'static str>) {
        let (events, receiver) = mpsc::channel(8);
        let id = self.next_tab.fetch_add(1, Ordering::SeqCst);
        self.tabs().insert(id, Tab { local, events });
        (id, receiver)
    }

    pub fn remove_tab(&self, id: u64) {
        self.tabs().remove(&id);
    }

    pub fn tab_count(&self) -> usize {
        self.tabs().len()
    }

    /// Open tabs on this computer (a phone's tab is no reason not to open one here).
    pub fn local_tab_count(&self) -> usize {
        self.tabs().values().filter(|tab| tab.local).count()
    }

    /// Ask every open tab on this computer to come to the front; whether there was one.
    pub fn activate(&self) -> bool {
        let mut found = false;
        for tab in self.tabs().values().filter(|tab| tab.local) {
            let _ = tab.events.try_send("activate");
            found = true;
        }
        found
    }

    /// Stop serving (`server_shutdown`, or Ctrl-C).
    pub fn shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    /// End every event stream (Python's `server_close`).
    fn close_tabs(&self) {
        self.closing.store(true, Ordering::SeqCst);
        for tab in self.tabs().values() {
            let _ = tab.events.try_send("close");
        }
    }

    /// Whether the request reached this server at `127.0.0.1`.
    fn is_local(&self, headers: &HeaderMap) -> bool {
        headers.get(header::HOST).is_some_and(|host| host.as_bytes() == format!("127.0.0.1:{}", self.port).as_bytes())
    }

    /// `127.0.0.1`, or with phone access any IPv4 address of this computer.
    /// A host name is never accepted, so a page that rebinds its own name to
    /// this computer cannot reach the server.
    fn host_ok(&self, headers: &HeaderMap) -> bool {
        self.is_local(headers)
            || (self.phone_token.is_some()
                && headers.get(header::HOST).is_some_and(|host| phone::is_address_host(host.as_bytes(), self.port)))
    }

    fn cookie_token<'a>(&self, headers: &'a HeaderMap) -> &'a str {
        let cookies = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()).unwrap_or("");
        cookies
            .split(';')
            .filter_map(|part| part.split_once('='))
            .filter(|(name, _)| name.trim() == self.cookie_name)
            .map(|(_, value)| value.trim().trim_matches('"'))
            .next_back()
            .unwrap_or("")
    }

    /// An accepted host, an origin that is this server at that host, and the
    /// session or phone token as a bearer token or cookie.
    fn access(&self, headers: &HeaderMap) -> Option<Access> {
        if !self.host_ok(headers) {
            return None;
        }
        let host = headers.get(header::HOST)?.as_bytes();
        if let Some(origin) = headers.get(header::ORIGIN)
            && !origin.is_empty()
            && origin.as_bytes() != [b"http://", host].concat()
        {
            return None;
        }
        let authorization = headers.get(header::AUTHORIZATION).map_or(Ok(""), HeaderValue::to_str).ok()?;
        // `Bearer ` with an empty token arrives as `Bearer` (trailing space trimmed); like the
        // Python server, it falls back to the session cookie.
        let mut token = match authorization {
            "Bearer" => "",
            other => other.strip_prefix("Bearer ").unwrap_or(other),
        };
        if token.is_empty() {
            token = self.cookie_token(headers);
        }
        if constant_time_eq(token.as_bytes(), self.token.as_bytes()) {
            Some(Access::Desktop)
        } else if self.phone_token.as_ref().is_some_and(|phone| constant_time_eq(token.as_bytes(), phone.as_bytes())) {
            Some(Access::Phone)
        } else {
            None
        }
    }

    fn authorized(&self, headers: &HeaderMap) -> bool {
        self.access(headers).is_some()
    }

    /// The cookie that keeps a tab signed in: for this browser session on this
    /// computer, for as long as browsers allow on a paired phone.
    fn session_cookie(&self, access: Access) -> String {
        let name = &self.cookie_name;
        match (access, &self.phone_token) {
            (Access::Phone, Some(token)) => {
                format!("{name}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}", phone::COOKIE_MAX_AGE)
            }
            _ => format!("{name}={}; HttpOnly; SameSite=Strict; Path=/", self.token),
        }
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A response with the security headers every regular response carries.
fn send(status: StatusCode, content: Vec<u8>, content_type: &str, extra: &[(header::HeaderName, String)]) -> Response {
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, content.len())
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::REFERRER_POLICY, "no-referrer")
        .header(header::CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY);
    for (name, value) in extra {
        builder = builder.header(name, value);
    }
    builder.body(Body::from(content)).expect("valid response")
}

fn json_response(status: StatusCode, value: &Value) -> Response {
    send(status, py::dumps(value).into_bytes(), "application/json; charset=utf-8", &[])
}

fn error(status: StatusCode, message: &str) -> Response {
    json_response(status, &json!({"error": message}))
}

/// The response, and the connection closed after it.
fn closing(mut response: Response) -> Response {
    response.headers_mut().insert(header::CONNECTION, HeaderValue::from_static("close"));
    response
}

/// `urllib.parse.unquote`.
fn unquote(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && let (Some(Some(high)), Some(Some(low))) =
                (bytes.get(i + 1).map(|b| hex(*b)), bytes.get(i + 2).map(|b| hex(*b)))
        {
            out.push((high * 16 + low) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The bundled file a request path names, resolved like a path under the
/// static directory (`None` when it would leave it).
fn static_file(path: &str) -> Option<String> {
    if path == "/" {
        return Some("index.html".to_string());
    }
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.trim_start_matches('/').split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            name if name.contains('\\') => return None,
            name => parts.push(name),
        }
    }
    Some(parts.join("/"))
}

/// An open tab's server-sent events: `ready`, then `activate` requests and a
/// heartbeat every two idle seconds, until the tab or the server goes away.
struct EventStream {
    server: Arc<BrowserServer>,
    tab: u64,
    events: mpsc::Receiver<&'static str>,
    first: Option<Bytes>,
    heartbeat: Pin<Box<tokio::time::Sleep>>,
    done: bool,
}

impl EventStream {
    fn new(server: Arc<BrowserServer>, local: bool) -> Self {
        let (tab, events) = server.add_tab(local);
        Self {
            server,
            tab,
            events,
            // A short retry lets tabs find a replacement server quickly.
            first: Some(Bytes::from_static(b"event: ready\nretry: 1000\ndata: {}\n\n")),
            heartbeat: Box::pin(tokio::time::sleep(std::time::Duration::from_secs(HEARTBEAT_SECONDS))),
            done: false,
        }
    }

    fn event(&mut self, name: &str) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(HEARTBEAT_SECONDS);
        self.heartbeat.as_mut().reset(deadline);
        Poll::Ready(Some(Ok(Frame::data(Bytes::from(format!("event: {name}\ndata: {{}}\n\n"))))))
    }
}

impl HttpBody for EventStream {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        let this = self.get_mut();
        if let Some(first) = this.first.take() {
            return Poll::Ready(Some(Ok(Frame::data(first))));
        }
        if this.done || this.server.closing.load(Ordering::SeqCst) {
            this.done = true;
            return Poll::Ready(None);
        }
        match this.events.poll_recv(cx) {
            Poll::Ready(Some("close")) | Poll::Ready(None) => {
                this.done = true;
                Poll::Ready(None)
            }
            Poll::Ready(Some(name)) => this.event(name),
            Poll::Pending => match this.heartbeat.as_mut().poll(cx) {
                Poll::Ready(()) => this.event("heartbeat"),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

impl Drop for EventStream {
    fn drop(&mut self) {
        self.server.remove_tab(self.tab);
    }
}

fn events_response(server: Arc<BrowserServer>, local: bool) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONNECTION, "close")
        .body(Body::new(EventStream::new(server, local)))
        .expect("valid response")
}

async fn get(server: Arc<BrowserServer>, request: Request) -> Response {
    let headers = request.headers();
    if !server.host_ok(headers) {
        return error(StatusCode::FORBIDDEN, "Invalid host.");
    }
    let path = unquote(request.uri().path());
    if path == "/health" {
        if !server.authorized(headers) {
            return error(StatusCode::FORBIDDEN, "Unauthorized.");
        }
        let health = json!({
            "app": "schedule-everything",
            "pid": std::process::id(),
            "build": server.build,
            "tabs": server.local_tab_count(),
            "phone": server.phone_token.is_some(),
        });
        return json_response(StatusCode::OK, &health);
    }
    if path == "/events" {
        if !server.authorized(headers) {
            return error(StatusCode::FORBIDDEN, "Unauthorized.");
        }
        let local = server.is_local(headers);
        return events_response(server, local);
    }
    if let Some(key) = path.strip_prefix("/downloads/") {
        if !server.authorized(headers) {
            return error(StatusCode::FORBIDDEN, "Unauthorized.");
        }
        let content = workspace::export_path(key).filter(|p| p.is_file()).and_then(|p| std::fs::read(p).ok());
        return match content {
            Some(content) => send(StatusCode::OK, content, "application/pdf", &[]),
            None => error(StatusCode::NOT_FOUND, "Export expired."),
        };
    }
    match static_file(&path).and_then(|name| Assets::get(&name).map(|file| (name, file))) {
        Some((name, file)) => {
            let content_type = mime_guess::from_path(&name).first_raw().unwrap_or("application/octet-stream");
            send(StatusCode::OK, file.data.into_owned(), content_type, &[])
        }
        None => error(StatusCode::NOT_FOUND, "Not found."),
    }
}

async fn post(server: Arc<BrowserServer>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let Some(access) = server.access(&parts.headers) else {
        return closing(error(StatusCode::FORBIDDEN, "Unauthorized request."));
    };
    let length = match parts.headers.get(header::CONTENT_LENGTH) {
        None => Some(0),
        Some(value) => value.to_str().ok().and_then(py::parse_int_str),
    };
    let Some(length) = length else {
        return error(StatusCode::BAD_REQUEST, "Invalid JSON.");
    };
    if !(1..=MAX_BODY as i64).contains(&length) {
        return closing(error(StatusCode::PAYLOAD_TOO_LARGE, "Invalid request size."));
    }
    let Ok(bytes) = axum::body::to_bytes(body, length as usize).await else {
        return error(StatusCode::BAD_REQUEST, "Invalid JSON.");
    };
    let Ok(payload) = pyjson::loads_bytes(&bytes) else {
        return error(StatusCode::BAD_REQUEST, "Invalid JSON.");
    };
    match parts.uri.path() {
        "/session" => {
            let cookie = server.session_cookie(access);
            send(
                StatusCode::OK,
                py::dumps(&json!({"ok": true})).into_bytes(),
                "application/json; charset=utf-8",
                &[(header::SET_COOKIE, cookie)],
            )
        }
        "/activate" => json_response(StatusCode::OK, &json!({"hasTab": server.activate()})),
        "/api" => {
            if payload.get("command") == Some(&json!("server_shutdown")) && payload.is_object() {
                server.shutdown();
                return json_response(
                    StatusCode::OK,
                    &json!({"ok": true, "data": {"message": "Browser service stopped."}}),
                );
            }
            let lock = server.operation_lock.clone();
            let response = tokio::task::spawn_blocking(move || {
                let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
                api::dispatch(&payload)
            })
            .await
            .unwrap_or_else(
                |e| json!({"ok": false, "error": {"code": "internal_error", "message": e.to_string(), "details": {}}}),
            );
            json_response(StatusCode::OK, &response)
        }
        _ => error(StatusCode::NOT_FOUND, "Not found."),
    }
}

async fn handle(State(server): State<Arc<BrowserServer>>, request: Request) -> Response {
    match *request.method() {
        Method::GET => get(server, request).await,
        Method::POST => post(server, request).await,
        ref method => closing(send(
            StatusCode::NOT_IMPLEMENTED,
            format!("Unsupported method ('{method}')").into_bytes(),
            "text/plain; charset=utf-8",
            &[],
        )),
    }
}

/// Serve until [`BrowserServer::shutdown`]; open requests get a moment to finish.
pub async fn serve(server: Arc<BrowserServer>, listener: TcpListener) -> std::io::Result<()> {
    let app = Router::new().fallback(handle).with_state(server.clone());
    let mut stop = server.shutdown.subscribe();
    let closer = server.clone();
    let graceful = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = stop.wait_for(|stop| *stop).await;
        closer.close_tabs();
    });
    let mut stop = server.shutdown.subscribe();
    let deadline = async move {
        let _ = stop.wait_for(|stop| *stop).await;
        tokio::time::sleep(std::time::Duration::from_secs(SHUTDOWN_GRACE_SECONDS)).await;
    };
    tokio::select! {
        result = graceful => result,
        () = deadline => Ok(()),
    }
}

/// Write `.browser.json` (`{"port", "token", "pid"}`, owner-only) atomically.
fn write_registry(registry: &Path, port: u16, token: &str) -> std::io::Result<()> {
    crate::web::write_private(registry, &py::dumps(&json!({"port": port, "token": token, "pid": std::process::id()})))
}

/// Remove `.browser.json` if it still names this process.
fn clear_registry(registry: &Path) {
    let ours = std::fs::read_to_string(registry)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|state| state.get("pid") == Some(&json!(std::process::id())));
    if ours {
        let _ = std::fs::remove_file(registry);
    }
}

/// `rmd __web-server --root R --port P`: serve the workspace for config root
/// `root` on `port` (0 picks a free port) until shut down. A replacement
/// server keeps its predecessor's token, passed in `REMINDER_BROWSER_TOKEN`.
/// Phone access is on when `root` holds `.phone.json` at startup.
pub fn run(root: &Path, port: u16) -> i32 {
    // Fingerprint first, so an update that lands mid-startup makes this server
    // look stale rather than current.
    let build = code_fingerprint();
    let root = paths::resolve(&paths::expand_user(&root.to_string_lossy()));
    // SAFETY: this runs at startup, before the runtime or any other thread exists.
    unsafe { std::env::set_var("REMINDER_CONFIG_DIR", &root) };
    if let Err(e) = std::fs::create_dir_all(&root) {
        eprintln!("Could not create {}: {e}", root.display());
        return 1;
    }
    // The token arrives by environment to stay out of the process list; keep
    // it away from the commands this server runs.
    let token = std::env::var(TOKEN_ENV).ok().filter(|token| !token.is_empty());
    // SAFETY: as above, no other thread exists yet.
    unsafe { std::env::remove_var(TOKEN_ENV) };
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("Could not start the browser server: {e}");
            return 1;
        }
    };
    let phone = phone::load(&root);
    let address = if phone.is_some() { Ipv4Addr::UNSPECIFIED } else { Ipv4Addr::LOCALHOST };
    let code = runtime.block_on(async move {
        let listener = match TcpListener::bind((address, port)).await {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("Could not listen on {address}:{port}: {e}");
                return 1;
            }
        };
        let port = match listener.local_addr() {
            Ok(address) => address.port(),
            Err(e) => {
                eprintln!("Could not read the server address: {e}");
                return 1;
            }
        };
        let token = token.unwrap_or_else(|| workspace::token_urlsafe(32));
        let mut server = BrowserServer::new(port, token, build);
        if let Some(phone) = phone {
            server = server.with_phone_access(phone.token);
        }
        let server = Arc::new(server);
        let registry = root.join(".browser.json");
        if let Err(e) = write_registry(&registry, port, &server.token) {
            eprintln!("Could not write {}: {e}", registry.display());
            return 1;
        }
        // Ctrl-C, or SIGTERM from a launcher replacing this server: shut down
        // cleanly so open tabs are told and `.browser.json` is removed.
        let interrupt = server.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                match signal(SignalKind::terminate()) {
                    Ok(mut terminate) => {
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => {}
                            _ = terminate.recv() => {}
                        }
                    }
                    Err(_) => {
                        let _ = tokio::signal::ctrl_c().await;
                    }
                }
            }
            #[cfg(not(unix))]
            let _ = tokio::signal::ctrl_c().await;
            interrupt.shutdown();
        });
        let result = serve(server.clone(), listener).await;
        server.close_tabs();
        clear_registry(&registry);
        match result {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("Browser server stopped: {e}");
                1
            }
        }
    });
    // Like the Python server's daemon threads, a request still running (a
    // long `rmd update`, say) does not keep the stopped server alive.
    runtime.shutdown_background();
    code
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::test_support::TestEnv;

    const TOKEN: &str = "test-token";

    /// A server on a free port, served from a background runtime.
    struct TestServer {
        server: Arc<BrowserServer>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl TestServer {
        fn start() -> Self {
            Self::start_with(None)
        }

        /// A server with phone access when `phone_token` is given.
        fn start_with(phone_token: Option<&str>) -> Self {
            let phone_token = phone_token.map(str::to_string);
            let (sender, receiver) = std::sync::mpsc::channel();
            let thread = std::thread::spawn(move || {
                let runtime =
                    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
                runtime.block_on(async move {
                    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
                    let port = listener.local_addr().unwrap().port();
                    let mut server = BrowserServer::new(port, TOKEN.to_string(), "test-build".to_string());
                    if let Some(token) = phone_token {
                        server = server.with_phone_access(token);
                    }
                    let server = Arc::new(server);
                    sender.send(server.clone()).unwrap();
                    serve(server, listener).await.unwrap();
                });
            });
            Self { server: receiver.recv().unwrap(), thread: Some(thread) }
        }

        fn port(&self) -> u16 {
            self.server.port
        }

        fn stop(&mut self) {
            self.server.shutdown();
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.stop();
        }
    }

    struct Reply {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    impl Reply {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
        }

        fn json(&self) -> Value {
            serde_json::from_slice(&self.body).unwrap()
        }
    }

    fn send_request(port: u16, method: &str, path: &str, body: Option<&str>, headers: &[(&str, &str)]) -> TcpStream {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut text = format!("{method} {path} HTTP/1.1\r\n");
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
            text += &format!("Host: 127.0.0.1:{port}\r\n");
        }
        for (key, value) in headers {
            text += &format!("{key}: {value}\r\n");
        }
        if let Some(body) = body
            && !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        {
            text += &format!("Content-Length: {}\r\n", body.len());
        }
        text += "Connection: close\r\n\r\n";
        text += body.unwrap_or("");
        stream.write_all(text.as_bytes()).unwrap();
        stream
    }

    fn parse_head(raw: &[u8]) -> (u16, Vec<(String, String)>, usize) {
        let end = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("complete head") + 4;
        let head = String::from_utf8_lossy(&raw[..end]).to_string();
        let mut lines = head.split("\r\n");
        let status = lines.next().unwrap().split(' ').nth(1).unwrap().parse().unwrap();
        let headers =
            lines.filter_map(|line| line.split_once(": ")).map(|(k, v)| (k.to_string(), v.to_string())).collect();
        (status, headers, end)
    }

    fn request(port: u16, method: &str, path: &str, body: Option<&str>, headers: &[(&str, &str)]) -> Reply {
        let mut stream = send_request(port, method, path, body, headers);
        let mut raw = Vec::new();
        let _ = stream.read_to_end(&mut raw);
        let (status, headers, end) = parse_head(&raw);
        Reply { status, headers, body: raw[end..].to_vec() }
    }

    fn auth() -> (&'static str, &'static str) {
        ("Authorization", "Bearer test-token")
    }

    #[test]
    fn static_app_is_bundled_and_paths_are_restricted() {
        let server = TestServer::start();
        let reply = request(server.port(), "GET", "/", None, &[]);
        assert_eq!(reply.status, 200);
        assert!(String::from_utf8_lossy(&reply.body).contains("<main id=\"app\">"));
        assert_eq!(reply.header("Content-Type"), Some("text/html"));
        assert!(reply.header("Content-Security-Policy").unwrap().contains("frame-ancestors 'none'"));
        assert_eq!(reply.header("Cache-Control"), Some("no-store"));
        assert_eq!(reply.header("X-Content-Type-Options"), Some("nosniff"));
        assert_eq!(reply.header("Referrer-Policy"), Some("no-referrer"));
        for path in ["/../server.py", "/%2e%2e/server.py", "/.browser.json", "/assets", "/missing.js"] {
            let reply = request(server.port(), "GET", path, None, &[]);
            assert_eq!((reply.status, reply.json()), (404, json!({"error": "Not found."})), "{path}");
        }
        let asset = Assets::iter().find(|name| name.ends_with(".js")).expect("bundled script");
        let reply = request(server.port(), "GET", &format!("/{asset}?v=1"), None, &[]);
        assert_eq!((reply.status, reply.header("Content-Type")), (200, Some("text/javascript")));
        let reply = request(server.port(), "GET", "/", None, &[("Host", "evil.example")]);
        assert_eq!((reply.status, reply.json()), (403, json!({"error": "Invalid host."})));
    }

    #[test]
    fn api_rejects_unauthenticated_cross_origin_and_rebound_requests() {
        let server = TestServer::start();
        let body = r#"{"command": "workspace_info"}"#;
        for headers in [
            vec![],
            vec![("Authorization", "Bearer wrong")],
            vec![auth(), ("Origin", "https://example.com")],
            vec![auth(), ("Host", "evil.example")],
        ] {
            let reply = request(server.port(), "POST", "/api", Some(body), &headers);
            assert_eq!((reply.status, reply.json()), (403, json!({"error": "Unauthorized request."})));
        }
        for path in ["/health", "/events", "/downloads/x"] {
            let reply = request(server.port(), "GET", path, None, &[]);
            assert_eq!((reply.status, reply.json()), (403, json!({"error": "Unauthorized."})), "{path}");
        }
    }

    #[test]
    fn session_cookie_authenticates_same_origin_requests() {
        let _env = TestEnv::new();
        let server = TestServer::start();
        let origin = format!("http://127.0.0.1:{}", server.port());
        let reply = request(server.port(), "POST", "/session", Some("{}"), &[auth(), ("Origin", &origin)]);
        assert_eq!((reply.status, reply.json()), (200, json!({"ok": true})));
        let cookie = reply.header("Set-Cookie").unwrap().to_string();
        assert_eq!(cookie, format!("rmd_{}=test-token; HttpOnly; SameSite=Strict; Path=/", server.port()));
        let session = cookie.split(';').next().unwrap();
        let body = r#"{"command": "settings_get_task_types"}"#;
        let reply = request(server.port(), "POST", "/api", Some(body), &[("Cookie", session), ("Origin", &origin)]);
        assert_eq!(reply.status, 200);
        assert_eq!(reply.json()["ok"], json!(true));
        let cookies = format!("a=b; {session}; c=d");
        let reply =
            request(server.port(), "POST", "/api", Some(body), &[("Authorization", "Bearer "), ("Cookie", &cookies)]);
        assert_eq!(reply.status, 200, "an empty bearer token falls back to the cookie");
        let reply = request(server.port(), "POST", "/api", Some(body), &[("Authorization", "test-token")]);
        assert_eq!(reply.status, 200, "the Bearer prefix is optional, as in the Python server");
        let wrong = format!("rmd_{}=wrong", server.port());
        assert_eq!(request(server.port(), "POST", "/api", Some(body), &[("Cookie", &wrong)]).status, 403);
        assert_eq!(reply.header("Content-Type"), Some("application/json; charset=utf-8"));
    }

    #[test]
    fn api_validates_json_shape_and_size() {
        let server = TestServer::start();
        let reply = request(server.port(), "POST", "/api", Some("[]"), &[auth()]);
        assert_eq!(reply.status, 200);
        assert_eq!(reply.json()["error"]["code"], json!("invalid_request"));
        let reply = request(server.port(), "POST", "/api", Some("{}"), &[auth(), ("Content-Length", "8388609")]);
        assert_eq!((reply.status, reply.json()), (413, json!({"error": "Invalid request size."})));
        let reply = request(server.port(), "POST", "/api", None, &[auth()]);
        assert_eq!(reply.status, 413, "a missing body counts as length 0");
        let reply = request(server.port(), "POST", "/api", Some("{nope"), &[auth()]);
        assert_eq!((reply.status, reply.json()), (400, json!({"error": "Invalid JSON."})));
        let reply = request(server.port(), "POST", "/elsewhere", Some("{}"), &[auth()]);
        assert_eq!((reply.status, reply.json()), (404, json!({"error": "Not found."})));
        let reply = request(server.port(), "PUT", "/api", Some("{}"), &[auth()]);
        assert_eq!(reply.status, 501);
    }

    #[test]
    fn activation_notifies_existing_tabs() {
        let server = TestServer::start();
        let (tab, mut events) = server.server.add_tab(true);
        let reply = request(server.port(), "POST", "/activate", Some("{}"), &[auth()]);
        assert_eq!((reply.status, reply.json()), (200, json!({"hasTab": true})));
        assert_eq!(events.try_recv().unwrap(), "activate");
        server.server.remove_tab(tab);
        assert!(!server.server.activate());
    }

    /// Read decoded chunked-body text until it contains `count` lines.
    fn read_lines(stream: &mut TcpStream, buffer: &mut Vec<u8>, count: usize) -> Vec<String> {
        let decoded = |raw: &[u8]| -> String {
            let mut out = Vec::new();
            let mut rest = raw;
            while let Some(line_end) = rest.windows(2).position(|w| w == b"\r\n") {
                let size = usize::from_str_radix(std::str::from_utf8(&rest[..line_end]).unwrap().trim(), 16).unwrap();
                if rest.len() < line_end + 2 + size + 2 {
                    break;
                }
                out.extend_from_slice(&rest[line_end + 2..line_end + 2 + size]);
                rest = &rest[line_end + 2 + size + 2..];
            }
            String::from_utf8(out).unwrap()
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let text = decoded(buffer);
            let lines: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
            if lines.len() >= count || Instant::now() > deadline {
                return lines;
            }
            let mut chunk = [0u8; 1024];
            let read = stream.read(&mut chunk).unwrap();
            buffer.extend_from_slice(&chunk[..read]);
        }
    }

    #[test]
    fn sse_registers_an_open_browser_tab() {
        let server = TestServer::start();
        let mut stream = send_request(server.port(), "GET", "/events", None, &[auth()]);
        let mut raw = Vec::new();
        while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
            let mut chunk = [0u8; 256];
            let read = stream.read(&mut chunk).unwrap();
            raw.extend_from_slice(&chunk[..read]);
        }
        let (status, headers, end) = parse_head(&raw);
        assert_eq!(status, 200);
        assert!(headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v == "text/event-stream"));
        let mut body = raw[end..].to_vec();
        let lines = read_lines(&mut stream, &mut body, 4);
        assert_eq!(lines[..4], ["event: ready\n", "retry: 1000\n", "data: {}\n", "\n"]);
        assert_eq!(server.server.tab_count(), 1);
        assert!(server.server.activate());
        let lines = read_lines(&mut stream, &mut body, 6);
        assert_eq!(lines[4], "event: activate\n");
        let health = request(server.port(), "GET", "/health", None, &[auth()]);
        assert_eq!(health.json()["tabs"], json!(1));
        // Closing the tab unregisters it (at the latest when the next heartbeat fails).
        drop(stream);
        let deadline = Instant::now() + Duration::from_secs(8);
        while server.server.tab_count() > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(server.server.tab_count(), 0);
    }

    #[test]
    fn downloads_require_authentication_and_known_export() {
        let env = TestEnv::new();
        let output = env.write("report.pdf", "%PDF-test");
        workspace::register_export("test-export", output);
        let server = TestServer::start();
        assert_eq!(request(server.port(), "GET", "/downloads/test-export", None, &[]).status, 403);
        let reply = request(server.port(), "GET", "/downloads/test-export", None, &[auth()]);
        assert_eq!((reply.status, reply.body.as_slice()), (200, b"%PDF-test".as_slice()));
        assert_eq!(reply.header("Content-Type"), Some("application/pdf"));
        let missing = request(server.port(), "GET", "/downloads/missing", None, &[auth()]);
        assert_eq!((missing.status, missing.json()), (404, json!({"error": "Export expired."})));
    }

    #[test]
    fn health_reports_code_build_and_open_tabs() {
        let server = TestServer::start();
        let health = request(server.port(), "GET", "/health", None, &[auth()]);
        assert_eq!(health.status, 200);
        let body = health.json();
        assert_eq!(body["app"], json!("schedule-everything"));
        assert_eq!(body["pid"], json!(std::process::id()));
        assert_eq!((&body["build"], &body["tabs"]), (&json!("test-build"), &json!(0)));
        let _tab = server.server.add_tab(true);
        assert_eq!(request(server.port(), "GET", "/health", None, &[auth()]).json()["tabs"], json!(1));
        assert_eq!(body["phone"], json!(false));
    }

    #[test]
    fn without_phone_access_only_loopback_requests_are_served() {
        let server = TestServer::start();
        let host = format!("192.168.1.20:{}", server.port());
        let reply = request(server.port(), "GET", "/", None, &[("Host", &host)]);
        assert_eq!((reply.status, reply.json()), (403, json!({"error": "Invalid host."})));
        let body = r#"{"command": "workspace_info"}"#;
        let reply = request(server.port(), "POST", "/api", Some(body), &[auth(), ("Host", &host)]);
        assert_eq!(reply.status, 403);
    }

    #[test]
    fn phone_access_serves_ipv4_hosts_with_either_token() {
        let _env = TestEnv::new();
        let server = TestServer::start_with(Some("phone-token"));
        let port = server.port();
        let host = format!("192.168.1.20:{port}");
        let origin = format!("http://{host}");
        let phone = ("Authorization", "Bearer phone-token");
        let body = r#"{"command": "settings_get_task_types"}"#;

        let reply = request(port, "GET", "/", None, &[("Host", &host)]);
        assert_eq!(reply.status, 200, "the app loads at the computer's address");
        for headers in [vec![phone, ("Host", &host), ("Origin", &origin)], vec![auth(), ("Host", &host)], vec![phone]] {
            let reply = request(port, "POST", "/api", Some(body), &headers);
            assert_eq!((reply.status, reply.json()["ok"].clone()), (200, json!(true)), "{headers:?}");
        }
        let elsewhere = format!("http://127.0.0.1:{port}");
        let tailscale = format!("100.101.102.103:{port}");
        let wrong_port = format!("192.168.1.20:{}", port.wrapping_add(1));
        for headers in [
            vec![phone, ("Host", &host), ("Origin", &elsewhere)],
            vec![phone, ("Host", &tailscale), ("Origin", &origin)],
            vec![phone, ("Host", "evil.example")],
            vec![phone, ("Host", &format!("evil.example:{port}"))],
            vec![phone, ("Host", &wrong_port)],
            vec![("Authorization", "Bearer wrong"), ("Host", &host)],
        ] {
            assert_eq!(request(port, "POST", "/api", Some(body), &headers).status, 403, "{headers:?}");
        }
        let health = request(port, "GET", "/health", None, &[phone, ("Host", &host)]);
        assert_eq!((health.status, health.json()["phone"].clone()), (200, json!(true)));
    }

    #[test]
    fn a_paired_phone_stays_signed_in() {
        let _env = TestEnv::new();
        let server = TestServer::start_with(Some("phone-token"));
        let port = server.port();
        let host = format!("192.168.1.20:{port}");
        let origin = format!("http://{host}");
        let headers = [("Authorization", "Bearer phone-token"), ("Host", &host), ("Origin", &origin)];
        let reply = request(port, "POST", "/session", Some("{}"), &headers);
        assert_eq!(
            reply.header("Set-Cookie"),
            Some(format!("rmd_{port}=phone-token; HttpOnly; SameSite=Strict; Path=/; Max-Age=34560000").as_str())
        );
        let cookie = format!("rmd_{port}=phone-token");
        let body = r#"{"command": "settings_get_task_types"}"#;
        let reply =
            request(port, "POST", "/api", Some(body), &[("Cookie", &cookie), ("Host", &host), ("Origin", &origin)]);
        assert_eq!(reply.status, 200);
        // The desktop session still ends with the browser session, wherever it is used.
        let reply = request(port, "POST", "/session", Some("{}"), &[auth(), ("Host", &host)]);
        assert_eq!(
            reply.header("Set-Cookie"),
            Some(format!("rmd_{port}=test-token; HttpOnly; SameSite=Strict; Path=/").as_str())
        );
    }

    #[test]
    fn phone_tabs_neither_count_nor_come_to_the_front() {
        let server = TestServer::start_with(Some("phone-token"));
        let host = format!("192.168.1.20:{}", server.port());
        let stream = send_request(
            server.port(),
            "GET",
            "/events",
            None,
            &[("Authorization", "Bearer phone-token"), ("Host", &host)],
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while server.server.tab_count() == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!((server.server.tab_count(), server.server.local_tab_count()), (1, 0));
        assert!(!server.server.activate(), "a phone's tab is no tab on this computer");
        let health = request(server.port(), "GET", "/health", None, &[auth()]);
        assert_eq!(health.json()["tabs"], json!(0));
        let (_local, mut events) = server.server.add_tab(true);
        assert!(server.server.activate());
        assert_eq!(events.try_recv().unwrap(), "activate");
        drop(stream);
    }

    #[test]
    fn shutdown_request_stops_the_server() {
        let mut server = TestServer::start();
        let reply = request(server.port(), "POST", "/api", Some(r#"{"command": "server_shutdown"}"#), &[auth()]);
        assert_eq!(
            (reply.status, reply.json()),
            (200, json!({"ok": true, "data": {"message": "Browser service stopped."}}))
        );
        let port = server.port();
        server.thread.take().unwrap().join().unwrap();
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    }

    #[test]
    fn registry_is_private_and_removed_only_by_its_server() {
        let env = TestEnv::new();
        let registry = env.root().join(".browser.json");
        write_registry(&registry, 8765, "secret").unwrap();
        let state: Value = serde_json::from_str(&env.read(".browser.json")).unwrap();
        assert_eq!(state, json!({"port": 8765, "token": "secret", "pid": std::process::id()}));
        assert!(env.read(".browser.json").starts_with("{\"port\": 8765, \"token\": "));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&registry).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert!(!env.root().join(".browser.tmp").exists());
        env.write(".browser.json", r#"{"port": 1, "token": "t", "pid": 1}"#);
        clear_registry(&registry);
        assert!(registry.exists(), "another server's entry stays");
        write_registry(&registry, 8765, "secret").unwrap();
        clear_registry(&registry);
        assert!(!registry.exists());
    }

    #[test]
    fn request_paths_resolve_like_files_under_the_bundle() {
        assert_eq!(unquote("/%2e%2e/a%20b"), "/../a b");
        assert_eq!(unquote("/100%"), "/100%");
        assert_eq!(static_file("/"), Some("index.html".to_string()));
        assert_eq!(static_file("//assets/./x.js"), Some("assets/x.js".to_string()));
        assert_eq!(static_file("/assets/../index.html/"), Some("index.html".to_string()));
        assert_eq!(static_file("/../server.py"), None);
        assert_eq!(static_file("/a\\b"), None);
    }
}
