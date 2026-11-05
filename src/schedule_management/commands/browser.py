"""Launch one local browser workspace per config root and reuse open tabs.

A running server is reused only while it runs the code installed on disk. An
outdated one is replaced on the same port with the same token, so open tabs
reconnect to the new server instead of pairing a new frontend with old APIs.
"""
from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
import time
import webbrowser
from contextlib import contextmanager
from pathlib import Path
from urllib.error import URLError
from urllib.request import Request, build_opener, ProxyHandler

from schedule_management.config_layout import resolve_config_root_dir
from schedule_management.web import TOKEN_ENV
from schedule_management.web.fingerprint import code_fingerprint

START_TIMEOUT = 10.0
STOP_TIMEOUT = 5.0
RECONNECT_TIMEOUT = 6.0


@contextmanager
def _launch_lock(root: Path):
    """OS lock is released automatically even if a launcher crashes."""
    with (root / ".browser.lock").open("a+b") as handle:
        if os.name == "nt":
            import msvcrt
            handle.write(b"0")
            handle.flush()
            handle.seek(0)
            msvcrt.locking(handle.fileno(), msvcrt.LK_LOCK, 1)
        else:
            import fcntl
            fcntl.flock(handle, fcntl.LOCK_EX)
        try:
            yield
        finally:
            if os.name == "nt":
                handle.seek(0)
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(handle, fcntl.LOCK_UN)


def _request(state: dict, route: str, payload: dict | None = None) -> dict:
    """GET ``route`` from a server, or POST ``payload`` to it when given."""
    port = int(state["port"])
    if not 1 <= port <= 65535:
        raise ValueError("Invalid port")
    data = None if payload is None else json.dumps(payload).encode()
    request = Request(f"http://127.0.0.1:{port}{route}", data=data, headers={"Authorization": f"Bearer {state['token']}", "Content-Type": "application/json"})
    with build_opener(ProxyHandler({})).open(request, timeout=2) as response:
        return json.load(response)


def _live_state(registry: Path) -> dict | None:
    """Return the registered server with its health details while it answers."""
    try:
        state = json.loads(registry.read_text())
        health = _request(state, "/health")
        if isinstance(health, dict) and health.get("app") == "schedule-everything" and health.get("pid") == state.get("pid"):
            return {**state, "build": health.get("build"), "tabs": health.get("tabs")}
    except (OSError, URLError, ValueError, KeyError, TypeError):
        pass
    return None


def _wait(condition, timeout: float, interval: float = 0.1) -> bool:
    deadline = time.monotonic() + timeout
    while not condition():
        if time.monotonic() >= deadline:
            return False
        time.sleep(interval)
    return True


def _answers(state: dict) -> bool:
    try:
        _request(state, "/health")
        return True
    except (OSError, ValueError):
        return False


def _registered(registry: Path, pid) -> bool:
    try:
        return json.loads(registry.read_text()).get("pid") == pid
    except (OSError, ValueError, AttributeError):
        return False


def _stop(state: dict, registry: Path) -> None:
    """Shut a server down and wait until it has released its port."""
    try:
        _request(state, "/api", {"command": "server_shutdown"})
    except (OSError, ValueError):
        pass
    if not _wait(lambda: not _answers(state), STOP_TIMEOUT):
        # Still answering as the process its health check named, so stop it.
        try:
            os.kill(int(state["pid"]), signal.SIGTERM)
        except (OSError, ValueError):
            pass
        if not _wait(lambda: not _answers(state), STOP_TIMEOUT):
            raise RuntimeError(f"Could not stop the outdated browser service (process {state['pid']}).")
    # Let it clear its registry entry so it cannot remove the replacement's.
    _wait(lambda: not _registered(registry, state["pid"]), 1.0)


def _start(root: Path, registry: Path, port: int, token: str | None = None) -> dict | None:
    """Start a server and wait for it to register; None if it exits first."""
    env = {**os.environ, "REMINDER_CONFIG_DIR": str(root)}
    env.pop(TOKEN_ENV, None)
    if token:
        env[TOKEN_ENV] = token
    with (root / ".browser.log").open("ab") as log:
        kwargs = {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP | subprocess.DETACHED_PROCESS} if os.name == "nt" else {"start_new_session": True}
        process = subprocess.Popen([sys.executable, "-m", "schedule_management.web.server", "--root", str(root), "--port", str(port)], env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log, **kwargs)
    deadline = time.monotonic() + START_TIMEOUT
    while time.monotonic() < deadline:
        # Any outdated server has stopped by now, so a live entry is this one.
        state = _live_state(registry)
        if state:
            return state
        if process.poll() is not None:
            return None
        time.sleep(0.1)
    raise RuntimeError(f"Browser server startup timed out. See {root / '.browser.log'}")


def _current_server(root: Path, registry: Path, port: int, *, start: bool) -> tuple[dict | None, dict | None]:
    """Return the live server, replacing it first if its code is outdated.

    The second value is the replaced server's state, or None when nothing was
    replaced. A server is started only when ``start`` is true and none runs.
    """
    failed = RuntimeError(f"Browser server failed to start. See {root / '.browser.log'}")
    state = _live_state(registry)
    if state is None:
        if not start:
            return None, None
        state = _start(root, registry, port)
        if state is None:
            raise failed
        return state, None
    if state.get("build") == code_fingerprint():
        return state, None
    _stop(state, registry)
    # Reusing the port and token lets open tabs reconnect to the replacement.
    replacement = _start(root, registry, int(state["port"]), state["token"]) or _start(root, registry, port)
    if replacement is None:
        raise failed
    return replacement, state


def _activate(state: dict, replaced: dict | None) -> bool:
    """Focus an open tab, giving tabs of a replaced server time to reconnect."""
    reconnecting = replaced is not None and replaced.get("tabs") != 0 and replaced.get("port") == state.get("port")
    found = False

    def activated() -> bool:
        nonlocal found
        found = bool(_request(state, "/activate", {}).get("hasTab", False))
        return found

    _wait(activated, RECONNECT_TIMEOUT if reconnecting else 0, interval=0.2)
    return found


def launch_browser(*, open_browser: bool = True, port: int = 0) -> tuple[str, bool, bool]:
    """Open the workspace. Returns its URL, whether an open tab was reused,
    and whether an outdated server was replaced."""
    if not 0 <= port <= 65535:
        raise ValueError("Port must be between 0 and 65535.")
    root = resolve_config_root_dir()
    root.mkdir(parents=True, exist_ok=True)
    registry = root / ".browser.json"
    with _launch_lock(root):
        state, replaced = _current_server(root, registry, port, start=True)
        url = f"http://127.0.0.1:{state['port']}/"
        has_tab = _activate(state, replaced)
        if open_browser and not has_tab:
            webbrowser.open(url + "#token=" + state["token"], new=0, autoraise=True)
        return (url if open_browser else url + "#token=" + state["token"]), has_tab, replaced is not None


def refresh_browser() -> tuple[str | None, bool]:
    """Replace a running workspace server whose code is outdated, without
    starting one or touching tabs. Returns its URL (None when no server is
    running) and whether it was replaced."""
    root = resolve_config_root_dir()
    if not root.is_dir():
        return None, False
    registry = root / ".browser.json"
    with _launch_lock(root):
        state, replaced = _current_server(root, registry, 0, start=False)
    if state is None:
        return None, False
    return f"http://127.0.0.1:{state['port']}/", replaced is not None


def browser_command(args) -> int:
    if args.if_running:
        url, replaced = refresh_browser()
        if url is None:
            print("No browser workspace is running.")
        else:
            print(f"{'Browser workspace restarted with the updated code' if replaced else 'Browser workspace is up to date'}: {url}")
        return 0
    url, reused, replaced = launch_browser(open_browser=not args.no_open, port=args.port)
    if replaced:
        status = "Browser workspace restarted with the updated code"
    else:
        status = "Reusing open browser workspace" if reused else "Browser workspace ready"
    print(f"{status}: {url}")
    return 0
