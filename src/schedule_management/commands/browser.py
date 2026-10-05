"""Launch one local browser workspace per config root and reuse open tabs."""
from __future__ import annotations

import json
import os
import subprocess
import sys
import time
import webbrowser
from contextlib import contextmanager
from pathlib import Path
from urllib.error import URLError
from urllib.request import Request, build_opener, ProxyHandler

from schedule_management.config_layout import resolve_config_root_dir


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


def _request(state: dict, route: str, post: bool = False) -> dict:
    port = int(state["port"])
    if not 1 <= port <= 65535:
        raise ValueError("Invalid port")
    request = Request(f"http://127.0.0.1:{port}{route}", data=b"{}" if post else None, headers={"Authorization": f"Bearer {state['token']}", "Content-Type": "application/json"})
    with build_opener(ProxyHandler({})).open(request, timeout=2) as response:
        return json.load(response)


def _live_state(registry: Path) -> dict | None:
    try:
        state = json.loads(registry.read_text())
        health = _request(state, "/health")
        if isinstance(health, dict) and health.get("app") == "schedule-everything" and health.get("pid") == state.get("pid"):
            return state
    except (OSError, URLError, ValueError, KeyError, TypeError):
        pass
    return None


def launch_browser(*, open_browser: bool = True, port: int = 0) -> tuple[str, bool]:
    if not 0 <= port <= 65535:
        raise ValueError("Port must be between 0 and 65535.")
    root = resolve_config_root_dir()
    root.mkdir(parents=True, exist_ok=True)
    registry = root / ".browser.json"
    with _launch_lock(root):
        state = _live_state(registry)
        if state is None:
            env = {**os.environ, "REMINDER_CONFIG_DIR": str(root)}
            with (root / ".browser.log").open("ab") as log:
                kwargs = {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP | subprocess.DETACHED_PROCESS} if os.name == "nt" else {"start_new_session": True}
                process = subprocess.Popen([sys.executable, "-m", "schedule_management.web.server", "--root", str(root), "--port", str(port)], env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log, **kwargs)
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                state = _live_state(registry)
                if state:
                    break
                if process.poll() is not None:
                    raise RuntimeError(f"Browser server failed to start. See {root / '.browser.log'}")
                time.sleep(0.1)
            if state is None:
                raise RuntimeError(f"Browser server startup timed out. See {root / '.browser.log'}")
        url = f"http://127.0.0.1:{state['port']}/"
        has_tab = _request(state, "/activate", post=True).get("hasTab", False)
        if open_browser and not has_tab:
            webbrowser.open(url + "#token=" + state["token"], new=0, autoraise=True)
        return (url if open_browser else url + "#token=" + state["token"]), has_tab


def browser_command(args) -> int:
    url, reused = launch_browser(open_browser=not args.no_open, port=args.port)
    print(f"{'Reusing open browser workspace' if reused else 'Browser workspace ready'}: {url}")
    return 0
