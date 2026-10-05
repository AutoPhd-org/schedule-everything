"""Browser operations for local configuration, schedule setup, and PDF exports.

Schedule proposals stay in memory until accepted. Configuration writes are
validated and guarded against stale browser edits.
"""
from __future__ import annotations

import base64
import hashlib
import json
import secrets
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import asdict
from datetime import date
from pathlib import Path
from typing import Any

from schedule_management.config_layout import list_config_ids, resolve_runtime_paths
from schedule_management.web.services import WebError, _require_text

FILES = {"settings": "settings.toml", "odd": "odd_weeks.toml", "even": "even_weeks.toml", "habits": "habits.toml", "deadlines": "ddl.json", "profile": "profile.md", "model": "llm.toml"}
SETUP_SESSIONS: dict[str, dict[str, Any]] = {}
EXPORTS: dict[str, Path] = {}


def _file_path(key: str) -> Path:
    if key not in FILES:
        raise WebError("invalid_input", "unknown configuration file.")
    if key == "model":
        from schedule_management.commands.setup_agent.configuration import _resolve_llm_config_path
        return _resolve_llm_config_path()
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
        if key == "deadlines":
            values = json.loads(content)
            if not isinstance(values, list) or any(not isinstance(item, dict) or not isinstance(item.get("event"), str) or not isinstance(item.get("deadline"), str) for item in values):
                raise ValueError("deadlines must be a list of event/deadline objects")
            for item in values:
                date.fromisoformat(item["deadline"])
        elif key != "profile":
            data = tomllib.loads(content)
            if key in {"odd", "even"}:
                from schedule_management.commands.settings import _valid_time, WEEKDAYS
                for section, events in data.items():
                    if section not in {*WEEKDAYS, "common"} or not isinstance(events, dict):
                        raise ValueError("weekly schedules must contain weekday or common tables")
                    for time, event in events.items():
                        if not _valid_time(time) or not isinstance(event, (str, dict)):
                            raise ValueError("events require a valid HH:MM time and label or block/title object")
            if key == "settings":
                for required in ("settings", "time_blocks", "time_points", "tasks", "paths"):
                    if not isinstance(data.get(required), dict):
                        raise ValueError(f"settings require a [{required}] table")
            if key == "habits" and not isinstance(data.get("habits"), dict):
                raise ValueError("habits require a [habits] table")
    except (ValueError, TypeError) as exc:
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


def setup_turn(payload: dict[str, Any]) -> dict[str, Any]:
    from schedule_management.commands.setup_agent import workflow as flow
    from schedule_management.commands.setup_agent.configuration import has_completed_configuration
    paths = resolve_runtime_paths()
    session_id = str(payload.get("sessionId", ""))
    if session_id:
        session = SETUP_SESSIONS.get(session_id)
        if session is None:
            raise WebError("not_found", "Conversation expired. Start a new conversation.")
        if session["configDir"] != str(paths.active_config_dir):
            raise WebError("conflict", "Active schedule changed. Start a new conversation.")
    else:
        # Limit abandoned sessions without retaining profile data indefinitely.
        if len(SETUP_SESSIONS) >= 32:
            SETUP_SESSIONS.pop(next(iter(SETUP_SESSIONS)))
        session_id = secrets.token_urlsafe(24)
        session = {"configDir": str(paths.active_config_dir), "modify": has_completed_configuration(paths.active_config_dir)[0], "history": "", "profile": flow._load_profile_markdown(paths.active_config_dir), "summary": None, "confirmed": False, "bundle": None}
        SETUP_SESSIONS[session_id] = session
    message = _require_text(payload, "message")
    upload = payload.get("attachment")
    if upload is not None:
        from schedule_management.commands.setup_agent.attachments import TEXT_EXTENSIONS, IMAGE_EXTENSIONS
        if not isinstance(upload, dict) or not isinstance(upload.get("name"), str) or not isinstance(upload.get("data"), str):
            raise WebError("invalid_input", "Invalid attachment.")
        filename = Path(upload["name"].replace("\\", "/")).name
        if Path(filename).suffix.lower() not in TEXT_EXTENSIONS | IMAGE_EXTENSIONS:
            raise WebError("invalid_input", "Use a text file or image for your timetable.")
        try:
            raw = base64.b64decode(upload["data"], validate=True)
        except ValueError:
            raise WebError("invalid_input", "Invalid attachment encoding.") from None
        if len(raw) > 5 * 1024 * 1024:
            raise WebError("invalid_input", "Attachments must be at most 5 MB.")
        session["attachment"] = (filename, raw)
    session["history"] += f"\nUser: {message}"
    if payload.get("confirmSummary") is True and session["summary"]:
        session["confirmed"] = True
    current_files = flow._render_current_files(paths.active_config_dir)
    base_revision = _revision(current_files)
    client = flow.LLMClient(flow.ensure_llm_config())
    if session["modify"]:
        prompt = flow.render_modify_user_prompt(message, current_files, profile_context=session["profile"], conversation_history=session["history"])
        system = flow.MODIFY_SYSTEM_PROMPT
    else:
        prompt = flow.render_build_user_prompt(paths.active_config_dir, description=message, attachment_name=session["attachment"][0] if session.get("attachment") else None, conversation_history=session["history"], profile_context=session["profile"], summary_presented=bool(session["summary"]), summary_confirmed=session["confirmed"], latest_summary=session["summary"])
        system = flow.BUILD_SYSTEM_PROMPT
    def validate_turn(turn):
        if not session["modify"] and turn.phase == "final" and not session["confirmed"]:
            return "The user must explicitly confirm a summary before final TOML is returned."
        if not session["modify"] and turn.phase == "summary" and not turn.profile_markdown:
            return "Summary requires a complete profile draft."
        return None
    with tempfile.TemporaryDirectory(prefix="rmd-attachment-") as temporary:
        attachment = None
        if session.get("attachment"):
            filename, raw = session["attachment"]
            target = Path(temporary) / filename
            target.write_bytes(raw)
            attachment, attachment_error = flow._load_source_attachment(target)
            if attachment is None:
                raise WebError("invalid_input", str(attachment_error))
        turn, error = flow._request_agent_turn(client, system, prompt, attachment=attachment, file_tools=flow._build_local_file_tools(paths.active_config_dir), turn_validator=validate_turn)
    if turn is None:
        raise WebError("ai_error", str(error))
    session["history"] += f"\nAssistant: {turn.conversation}"
    session["profile"] = turn.profile_markdown or session["profile"]
    if turn.phase == "summary":
        session["confirmed"] = False
    session["summary"] = turn.schedule_summary or session["summary"]
    session["bundle"] = turn.bundle
    session["revision"] = base_revision
    response = asdict(turn)
    response.pop("actions", None)
    return {"sessionId": session_id, **response}


def setup_accept(payload: dict[str, Any]) -> dict[str, Any]:
    from schedule_management.commands.setup_agent import workflow as flow
    session_id = _require_text(payload, "sessionId")
    session = SETUP_SESSIONS.get(session_id)
    if not session or not session.get("bundle"):
        raise WebError("invalid_input", "No schedule proposal to accept.")
    paths = resolve_runtime_paths()
    if session["configDir"] != str(paths.active_config_dir) or session["revision"] != _revision(flow._render_current_files(paths.active_config_dir)):
        raise WebError("conflict", "Your schedule changed. Generate a fresh proposal.")
    bundle = dict(session["bundle"])
    bundle.setdefault("habits.toml", flow.DEFAULT_HABITS_TOML)
    for filename, content in bundle.items():
        key = next((key for key, value in FILES.items() if value == filename), None)
        if key not in {"settings", "odd", "even", "habits"}:
            raise WebError("invalid_input", "Unexpected proposal file.")
        validate_content(key, content)
    if session["modify"]:
        flow._apply_versioned_schedule_update(paths.active_config_dir, bundle, profile_markdown=session["profile"])
    else:
        flow._write_bundle(paths.active_config_dir, bundle)
        if session["profile"]:
            flow._write_profile_markdown(paths.active_config_dir, session["profile"])
    del SETUP_SESSIONS[session_id]
    return {**workspace_info({}), **_reload()}


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
