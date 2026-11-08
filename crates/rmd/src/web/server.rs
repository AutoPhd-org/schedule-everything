//! Authenticated loopback HTTP server for the bundled browser interface (port
//! of `web/server.py`).
//!
//! A server owns one config root. SSE connections identify open browser tabs
//! and carry activation requests; API mutations are serialized within this
//! process. The server reports a fingerprint of its build so the launcher can
//! replace it after an update; open tabs reconnect to the replacement on the
//! same port with the same token.

use std::collections::HashMap;
use std::convert::Infallible;
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
use crate::web::{TOKEN_ENV, api, pyjson, workspace};

/// The built browser app (`web/src`, bundled by Vite).
#[derive(RustEmbed)]
#[folder = "../../src/schedule_management/web/static"]
pub(crate) struct Assets;

pub const MAX_BODY: usize = 8 * 1024 * 1024;
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'";
/// Seconds between heartbeats on an idle event stream.
const HEARTBEAT_SECONDS: u64 = 2;
/// How long open requests may finish after a shutdown request.
const SHUTDOWN_GRACE_SECONDS: u64 = 2;

/// State shared by every connection of one server.
pub struct BrowserServer {
    pub port: u16,
    pub token: String,
    pub build: String,
    origin: String,
    cookie_name: String,
    tabs: Mutex<HashMap<u64, mpsc::Sender<&'static str>>>,
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
            origin: format!("http://127.0.0.1:{port}"),
            cookie_name: format!("rmd_{port}"),
            tabs: Mutex::new(HashMap::new()),
            next_tab: AtomicU64::new(0),
            closing: AtomicBool::new(false),
            operation_lock: Arc::new(Mutex::new(())),
            shutdown: watch::Sender::new(false),
        }
    }

    fn tabs(&self) -> std::sync::MutexGuard<'_, HashMap<u64, mpsc::Sender<&'static str>>> {
        self.tabs.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Register an open browser tab; its events arrive on the receiver.
    pub fn add_tab(&self) -> (u64, mpsc::Receiver<&'static str>) {
        let (sender, receiver) = mpsc::channel(8);
        let id = self.next_tab.fetch_add(1, Ordering::SeqCst);
        self.tabs().insert(id, sender);
        (id, receiver)
    }

    pub fn remove_tab(&self, id: u64) {
        self.tabs().remove(&id);
    }

    pub fn tab_count(&self) -> usize {
        self.tabs().len()
    }

    /// Ask every open tab to come to the front; whether there was one.
    pub fn activate(&self) -> bool {
        let tabs = self.tabs();
        for tab in tabs.values() {
            let _ = tab.try_send("activate");
        }
        !tabs.is_empty()
    }

    /// Stop serving (`server_shutdown`, or Ctrl-C).
    pub fn shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    /// End every event stream (Python's `server_close`).
    fn close_tabs(&self) {
        self.closing.store(true, Ordering::SeqCst);
        for tab in self.tabs().values() {
            let _ = tab.try_send("close");
        }
    }

    fn host_ok(&self, headers: &HeaderMap) -> bool {
        headers.get(header::HOST).is_some_and(|host| host.as_bytes() == format!("127.0.0.1:{}", self.port).as_bytes())
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

    /// Same host and origin, and the session token as a bearer token or cookie.
    fn authorized(&self, headers: &HeaderMap) -> bool {
        if !self.host_ok(headers) {
            return false;
        }
        if let Some(origin) = headers.get(header::ORIGIN)
            && !origin.is_empty()
            && origin.as_bytes() != self.origin.as_bytes()
        {
            return false;
        }
        let Ok(authorization) = headers.get(header::AUTHORIZATION).map_or(Ok(""), HeaderValue::to_str) else {
            return false;
        };
        let mut token = authorization.strip_prefix("Bearer ").unwrap_or(authorization);
        if token.is_empty() {
            token = self.cookie_token(headers);
        }
        constant_time_eq(token.as_bytes(), self.token.as_bytes())
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
    fn new(server: Arc<BrowserServer>) -> Self {
        let (tab, events) = server.add_tab();
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

fn events_response(server: Arc<BrowserServer>) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONNECTION, "close")
        .body(Body::new(EventStream::new(server)))
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
            "tabs": server.tab_count(),
        });
        return json_response(StatusCode::OK, &health);
    }
    if path == "/events" {
        if !server.authorized(headers) {
            return error(StatusCode::FORBIDDEN, "Unauthorized.");
        }
        return events_response(server);
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
    if !server.authorized(&parts.headers) {
        return closing(error(StatusCode::FORBIDDEN, "Unauthorized request."));
    }
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
            let cookie = format!("{}={}; HttpOnly; SameSite=Strict; Path=/", server.cookie_name, server.token);
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
    let temporary = registry.with_extension("tmp");
    let content = py::dumps(&json!({"port": port, "token": token, "pid": std::process::id()}));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    std::io::Write::write_all(&mut options.open(&temporary)?, content.as_bytes())?;
    #[cfg(unix)]
    std::fs::set_permissions(&temporary, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    std::fs::rename(&temporary, registry)
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
    runtime.block_on(async move {
        let listener = match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("Could not listen on 127.0.0.1:{port}: {e}");
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
        let server = Arc::new(BrowserServer::new(port, token, build));
        let registry = root.join(".browser.json");
        if let Err(e) = write_registry(&registry, port, &server.token) {
            eprintln!("Could not write {}: {e}", registry.display());
            return 1;
        }
        let interrupt = server.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                interrupt.shutdown();
            }
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
    })
}
