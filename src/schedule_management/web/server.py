"""Authenticated loopback HTTP server for the bundled browser interface.

A server owns one config root. SSE connections identify open browser tabs and
carry activation requests; API mutations are serialized within this process.
"""
from __future__ import annotations

import argparse
import json
import mimetypes
import os
import queue
import secrets
import threading
from http.cookies import SimpleCookie
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlsplit

from schedule_management.web.api import dispatch
from schedule_management.web.workspace import EXPORTS

STATIC_DIR = Path(__file__).with_name("static")
MAX_BODY = 8 * 1024 * 1024


class BrowserServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, port: int = 0, token: str | None = None):
        self.tabs_lock = threading.Lock()
        self.tabs: set[queue.Queue] = set()
        self.closing = threading.Event()
        super().__init__(("127.0.0.1", port), BrowserHandler)
        self.token = token or secrets.token_urlsafe(32)
        self.origin = f"http://127.0.0.1:{self.server_port}"
        self.cookie_name = f"rmd_{self.server_port}"
        self.operation_lock = threading.RLock()

    def activate(self) -> bool:
        with self.tabs_lock:
            for tab in self.tabs:
                try:
                    tab.put_nowait("activate")
                except queue.Full:
                    pass
            return bool(self.tabs)

    def server_close(self):
        self.closing.set()
        with self.tabs_lock:
            for tab in self.tabs:
                try:
                    tab.put_nowait("close")
                except queue.Full:
                    pass
        super().server_close()


class BrowserHandler(BaseHTTPRequestHandler):
    server: BrowserServer
    protocol_version = "HTTP/1.1"

    def log_message(self, format, *args):
        # Requests can include local filenames; do not log personal data.
        pass

    def _send(self, status: int, content: bytes, content_type: str, headers: dict | None = None):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(content)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'")
        for key, value in (headers or {}).items():
            self.send_header(key, value)
        self.end_headers()
        self.wfile.write(content)

    def _json(self, status: int, value: dict, headers: dict | None = None):
        self._send(status, json.dumps(value, ensure_ascii=False).encode(), "application/json; charset=utf-8", headers)

    def _host_ok(self) -> bool:
        return self.headers.get("Host") == f"127.0.0.1:{self.server.server_port}"

    def _authorized(self) -> bool:
        if not self._host_ok():
            return False
        origin = self.headers.get("Origin")
        if origin and origin != self.server.origin:
            return False
        token = self.headers.get("Authorization", "").removeprefix("Bearer ")
        if not token:
            try:
                cookie = SimpleCookie(self.headers.get("Cookie", ""))
                token = cookie[self.server.cookie_name].value if self.server.cookie_name in cookie else ""
            except Exception:
                return False
        return secrets.compare_digest(token, self.server.token)

    def do_GET(self):
        if not self._host_ok():
            self._json(403, {"error": "Invalid host."})
            return
        path = unquote(urlsplit(self.path).path)
        if path == "/health":
            if not self._authorized():
                self._json(403, {"error": "Unauthorized."})
                return
            self._json(200, {"app": "schedule-everything", "pid": os.getpid()})
        elif path == "/events":
            if not self._authorized():
                self._json(403, {"error": "Unauthorized."})
                return
            tab: queue.Queue = queue.Queue(maxsize=8)
            with self.server.tabs_lock:
                self.server.tabs.add(tab)
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Connection", "close")
            self.end_headers()
            self.close_connection = True
            try:
                self.wfile.write(b"event: ready\ndata: {}\n\n")
                self.wfile.flush()
                while not self.server.closing.is_set():
                    try:
                        event = tab.get(timeout=2)
                    except queue.Empty:
                        event = "heartbeat"
                    if event == "close":
                        break
                    self.wfile.write(f"event: {event}\ndata: {{}}\n\n".encode())
                    self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                pass
            finally:
                with self.server.tabs_lock:
                    self.server.tabs.discard(tab)
        elif path.startswith("/downloads/"):
            if not self._authorized():
                self._json(403, {"error": "Unauthorized."})
                return
            output = EXPORTS.get(path.removeprefix("/downloads/"))
            if not output or not output.is_file():
                self._json(404, {"error": "Export expired."})
                return
            self._send(200, output.read_bytes(), "application/pdf")
        else:
            target = (STATIC_DIR / path.lstrip("/")).resolve() if path != "/" else STATIC_DIR / "index.html"
            if not target.is_relative_to(STATIC_DIR.resolve()) or not target.is_file():
                self._json(404, {"error": "Not found."})
                return
            self._send(200, target.read_bytes(), mimetypes.guess_type(target.name)[0] or "application/octet-stream")

    def do_POST(self):
        if not self._authorized():
            self.close_connection = True
            self._json(403, {"error": "Unauthorized request."})
            return
        path = urlsplit(self.path).path
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= MAX_BODY:
                self.close_connection = True
                self._json(413, {"error": "Invalid request size."})
                return
            payload = json.loads(self.rfile.read(length))
        except (ValueError, UnicodeDecodeError):
            self._json(400, {"error": "Invalid JSON."})
            return
        if path == "/session":
            self._json(200, {"ok": True}, {"Set-Cookie": f"{self.server.cookie_name}={self.server.token}; HttpOnly; SameSite=Strict; Path=/"})
        elif path == "/activate":
            self._json(200, {"hasTab": self.server.activate()})
        elif path == "/api":
            if isinstance(payload, dict) and payload.get("command") == "server_shutdown":
                self._json(200, {"ok": True, "data": {"message": "Browser service stopped."}})
                threading.Thread(target=self.server.shutdown, daemon=True).start()
                return
            with self.server.operation_lock:
                response = dispatch(payload)
            self._json(200, response)
        else:
            self._json(404, {"error": "Not found."})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True)
    parser.add_argument("--port", type=int, default=0)
    args = parser.parse_args()
    root = Path(args.root).expanduser().resolve()
    os.environ["REMINDER_CONFIG_DIR"] = str(root)
    root.mkdir(parents=True, exist_ok=True)
    server = BrowserServer(args.port)
    registry = root / ".browser.json"
    temporary = registry.with_suffix(".tmp")
    temporary.write_text(json.dumps({"port": server.server_port, "token": server.token, "pid": os.getpid()}))
    temporary.chmod(0o600)
    temporary.replace(registry)
    try:
        server.serve_forever()
    finally:
        server.server_close()
        try:
            if json.loads(registry.read_text()).get("pid") == os.getpid():
                registry.unlink()
        except (OSError, ValueError):
            pass


if __name__ == "__main__":
    main()
