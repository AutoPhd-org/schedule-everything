"""
Configuration validation shared by `rmd check` and the browser editor.

`validate_content` checks one file's syntax and required structure.
`check_config` checks a whole config directory, including the week schedules'
labels against settings.toml, and reports the paths an AI agent needs to build
or version a schedule. Problems the reminder service tolerates are warnings.
"""

from __future__ import annotations

import json
import tomllib
from datetime import date
from pathlib import Path
from typing import Any

from schedule_management.config_layout import get_next_config_id, list_config_ids, resolve_runtime_paths

CONFIG_FILES = {
    "settings": "settings.toml",
    "odd": "odd_weeks.toml",
    "even": "even_weeks.toml",
    "habits": "habits.toml",
    "deadlines": "ddl.json",
    "profile": "profile.md",
}
REQUIRED_FILES = ("settings", "odd", "even")
WEEK_SECTIONS = {"monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "common"}
SETTINGS_TABLES = ("settings", "time_blocks", "time_points", "tasks", "paths")
# The runtime falls back to defaults when these settings tables are missing, so
# `rmd check` reports them as warnings rather than errors.
OPTIONAL_SETTINGS_TABLES = ("tasks", "paths")


class ConfigCheckError(ValueError):
    """A configuration file that fails validation."""


def _valid_time(value: str) -> bool:
    parts = value.split(":")
    return len(parts) == 2 and all(part.isdigit() and len(part) == 2 for part in parts) and int(parts[0]) < 24 and int(parts[1]) < 60


def validate_content(key: str, content: str) -> None:
    """Validate the syntax and structure of one configuration file."""
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
                for section, events in data.items():
                    if section not in WEEK_SECTIONS or not isinstance(events, dict):
                        raise ValueError("weekly schedules must contain weekday or common tables")
                    for time, event in events.items():
                        if not _valid_time(time) or not isinstance(event, (str, dict)):
                            raise ValueError("events require a valid HH:MM time and label or block/title object")
            if key == "settings":
                for required in SETTINGS_TABLES:
                    if not isinstance(data.get(required), dict):
                        raise ValueError(f"settings require a [{required}] table")
            if key == "habits" and not isinstance(data.get("habits"), dict):
                raise ValueError("habits require a [habits] table")
    except (ValueError, TypeError) as exc:
        raise ConfigCheckError(str(exc)) from exc


def _label_problems(week: dict[str, Any], blocks: set[str], points: set[str]) -> tuple[list[str], list[str]]:
    """Return (errors, warnings) for week entries that settings.toml does not define.

    An unknown `block` in an inline table is ignored by the reminder service, so
    it is an error. An undefined plain label is still shown as a reminder message
    (with no duration), so it is only a warning; it is usually a typo.
    """
    errors: list[str] = []
    warnings: list[str] = []
    for section, events in week.items():
        for time, event in events.items():
            where = f"[{section}] {time}"
            if isinstance(event, str):
                if event not in blocks and event not in points:
                    warnings.append(f"{where}: '{event}' is not a [time_blocks] or [time_points] name, so it is shown as a plain reminder message")
            elif isinstance(event, dict):
                block = event.get("block")
                if not isinstance(block, str) or block not in blocks:
                    errors.append(f"{where}: block '{block}' is not a [time_blocks] name in settings.toml")
                if "title" in event and (not isinstance(event["title"], str) or not event["title"].strip()):
                    errors.append(f"{where}: title must be non-empty text")
    return errors, warnings


def _check_settings(text: str, name: str, errors: list[dict[str, str]], warnings: list[dict[str, str]]) -> str | None:
    """Validate settings.toml; return its text when it can be used for label checks."""
    try:
        data = tomllib.loads(text)
    except tomllib.TOMLDecodeError as exc:
        errors.append({"file": name, "message": str(exc)})
        return None
    missing = [table for table in SETTINGS_TABLES if not isinstance(data.get(table), dict)]
    required = [table for table in missing if table not in OPTIONAL_SETTINGS_TABLES]
    for table in required:
        errors.append({"file": name, "message": f"settings require a [{table}] table"})
    for table in missing:
        if table in OPTIONAL_SETTINGS_TABLES:
            warnings.append({"file": name, "message": f"no [{table}] table; built-in defaults apply"})
    return None if required else text


def check_config(directory: Path | None = None) -> dict[str, Any]:
    """Validate a config directory (the active one by default) and report its paths."""
    paths = resolve_runtime_paths()
    target = (directory.expanduser().resolve() if directory else paths.active_config_dir)
    errors: list[dict[str, str]] = []
    warnings: list[dict[str, str]] = []
    parsed: dict[str, Any] = {}
    files: dict[str, dict[str, Any]] = {}
    for key, name in CONFIG_FILES.items():
        path = target / name
        files[key] = {"path": str(path), "exists": path.is_file()}
        if not path.is_file():
            if key in REQUIRED_FILES:
                errors.append({"file": name, "message": "missing"})
            continue
        text = path.read_text(encoding="utf-8")
        if key == "settings":
            text = _check_settings(text, name, errors, warnings)
            if text is None:
                continue
        else:
            try:
                validate_content(key, text)
            except ConfigCheckError as exc:
                errors.append({"file": name, "message": str(exc)})
                continue
        if key in {"settings", "odd", "even"}:
            parsed[key] = tomllib.loads(text)

    if "settings" in parsed:
        blocks = {str(name) for name in parsed["settings"]["time_blocks"]}
        points = {str(name) for name in parsed["settings"]["time_points"]}
        for key in ("odd", "even"):
            if key in parsed:
                label_errors, label_warnings = _label_problems(parsed[key], blocks, points)
                errors += [{"file": CONFIG_FILES[key], "message": message} for message in label_errors]
                warnings += [{"file": CONFIG_FILES[key], "message": message} for message in label_warnings]

    next_id = get_next_config_id(paths.root_dir)
    return {
        "configRoot": str(paths.root_dir),
        "activeId": paths.active_id,
        "activeConfigDir": str(paths.active_config_dir),
        "configIds": list_config_ids(paths.root_dir),
        "nextConfigId": next_id,
        "nextConfigDir": str(paths.root_dir / f"user_config_{next_id}"),
        "directory": str(target),
        "files": files,
        "valid": not errors,
        "errors": errors,
        "warnings": warnings,
    }


def check_command(args) -> int:
    """Handle `rmd check [--dir DIR] [--json]`."""
    directory = Path(args.dir) if getattr(args, "dir", None) else None
    report = check_config(directory)
    if getattr(args, "json", False):
        print(json.dumps(report, ensure_ascii=False, indent=2))
        return 0 if report["valid"] else 1
    print(f"Config root: {report['configRoot']}")
    print(f"Checked:     {report['directory']}")
    for key, info in report["files"].items():
        mark = "✓" if info["exists"] else ("✗" if key in REQUIRED_FILES else "·")
        print(f"  {mark} {CONFIG_FILES[key]}{'' if info['exists'] else ' (missing)'}")
    for warning in report["warnings"]:
        print(f"⚠️  {warning['file']}: {warning['message']}")
    if report["valid"]:
        print("✅ Configuration is valid.")
        return 0
    for error in report["errors"]:
        print(f"❌ {error['file']}: {error['message']}")
    return 1
