"""Browser operations for local configuration, reminder service, and PDF exports.

Configuration writes are validated (with the same rules as `rmd check`) and
guarded against stale browser edits.
"""
from __future__ import annotations

import hashlib
import secrets
import subprocess
import sys
from datetime import date
from pathlib import Path
from typing import Any

from schedule_management.config_check import CONFIG_FILES as FILES
from schedule_management.config_check import ConfigCheckError
from schedule_management.config_check import validate_content as _validate_file
from schedule_management.config_layout import list_config_ids, resolve_runtime_paths
from schedule_management.web.services import WebError, _require_text

EXPORTS: dict[str, Path] = {}


def _file_path(key: str) -> Path:
    if key not in FILES:
        raise WebError("invalid_input", "unknown configuration file.")
    return resolve_runtime_paths().active_config_dir / FILES[key]


def _revision(content: str) -> str:
    return hashlib.sha256(content.encode()).hexdigest()


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8") if path.exists() else ""


def config_read(payload: dict[str, Any]) -> dict[str, Any]:
    key = str(payload.get("file", "settings"))
    path = _file_path(key)
    content = _read(path)
    return {"file": key, "content": content, "revision": _revision(content), "activeId": resolve_runtime_paths().active_id}


def validate_content(key: str, content: str) -> None:
    try:
        _validate_file(key, content)
    except ConfigCheckError as exc:
        raise WebError("invalid_input", str(exc)) from exc


def config_save(payload: dict[str, Any]) -> dict[str, Any]:
    key = _require_text(payload, "file")
    path = _file_path(key)
    content = payload.get("content")
    if not isinstance(content, str):
        raise WebError("invalid_input", "content must be text.")
    if payload.get("activeId") != resolve_runtime_paths().active_id or payload.get("revision") != _revision(_read(path)):
        raise WebError("conflict", "This file changed. Reload it before saving.")
    validate_content(key, content)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(content, encoding="utf-8")
    temporary.replace(path)
    return {**config_read({"file": key}), **_reload()}


def _reload() -> dict[str, Any]:
    from schedule_management.commands.service import _restart_reminder_service
    ok, message = _restart_reminder_service()
    return {"reloaded": ok, "message": "Saved and reminders reloaded." if ok else f"Saved. {message}"}


def workspace_info(payload: dict[str, Any]) -> dict[str, Any]:
    del payload
    from schedule_management.data import load_mode
    paths = resolve_runtime_paths()
    return {"configIds": list_config_ids(paths.root_dir), "activeId": paths.active_id, "rootDir": str(paths.root_dir), "mode": load_mode()}


def service_action(payload: dict[str, Any]) -> dict[str, Any]:
    action = _require_text(payload, "action")
    if action not in {"update", "stop", "switch", "mode"}:
        raise WebError("invalid_input", "unknown service action.")
    args = [action]
    if action == "switch":
        try:
            config_id = int(payload.get("configId"))
        except (TypeError, ValueError):
            raise WebError("invalid_input", "configId must be an integer.") from None
        if config_id not in list_config_ids():
            raise WebError("not_found", "configuration does not exist.")
        args.append(str(config_id))
    elif action == "mode":
        mode = str(payload.get("mode"))
        if mode not in {"j", "p"}:
            raise WebError("invalid_input", "mode must be j or p.")
        args.append(mode)
    completed = subprocess.run([sys.executable, "-m", "schedule_management.cli", *args], capture_output=True, text=True, timeout=120)
    if completed.returncode:
        raise WebError("service_error", completed.stdout.strip() or completed.stderr.strip())
    return {"message": completed.stdout.strip(), **workspace_info({})}


def export_pdf(payload: dict[str, Any]) -> dict[str, Any]:
    import matplotlib
    matplotlib.use("Agg")
    kind = _require_text(payload, "kind")
    paths = resolve_runtime_paths()
    if kind in {"weekly", "monthly"}:
        from schedule_management.report import generate_manual_report
        target = date.fromisoformat(payload["date"]) if payload.get("date") else None
        output = generate_manual_report(kind, target, str(paths.settings_path))
    elif kind == "schedule":
        from schedule_management.config import ScheduleConfig, WeeklySchedule
        from schedule_management.visualizer import ScheduleVisualizer
        config = ScheduleConfig(str(paths.settings_path))
        weekly = WeeklySchedule(str(paths.odd_path), str(paths.even_path))
        output = paths.root_dir / "exports" / "schedule.pdf"
        ScheduleVisualizer(config, weekly.odd_data, weekly.even_data).visualize(output)
    else:
        raise WebError("invalid_input", "Unknown PDF type.")
    if not output or not Path(output).is_file():
        raise WebError("export_error", "Could not generate PDF.")
    key = secrets.token_urlsafe(24)
    EXPORTS[key] = Path(output)
    return {"url": f"/downloads/{key}", "filename": Path(output).name}
