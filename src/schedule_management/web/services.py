"""
Structured service operations for the Schedule Everything browser interface.

This module wraps existing schedule-management storage and schedule helpers
without printing CLI tables or prompts. Functions accept JSON-like payloads and
return JSON-serializable dictionaries for the local HTTP API.
"""

from __future__ import annotations

from datetime import date, datetime, timedelta, timezone
from typing import Any

from schedule_management.commands import status as status_commands
from schedule_management.commands.deadlines import prune_expired_deadlines
from schedule_management.commands.history import _format_duration, _pair_task_activities
from schedule_management.commands.settings import DEFAULT_TASK_TYPES
from schedule_management.commands.status import (
    get_current_and_next_events,
    get_today_schedule_for_status,
)
from schedule_management.config import ScheduleConfig
from schedule_management.config_layout import (
    RuntimePaths,
    preview_active_config_dir,
    resolve_config_root_dir,
)
from schedule_management.data import (
    load_deadlines,
    load_habit_records,
    load_habits,
    load_procrastinate_records,
    load_task_log,
    load_tasks,
    log_task_action,
    save_deadlines,
    save_habit_records,
    save_tasks,
    get_procrastinate_age_days,
)
from schedule_management.synced_schedule import (
    format_event_label,
    get_event_block_name,
    iter_syncable_slots,
    load_synced_schedule,
    synced_schedule_matches_today,
)


class WebError(Exception):
    """Structured error for browser API responses."""

    def __init__(
        self,
        code: str,
        message: str,
        details: dict[str, Any] | None = None,
    ) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details or {}

    def to_dict(self) -> dict[str, Any]:
        return {
            "code": self.code,
            "message": self.message,
            "details": self.details,
        }


def _today() -> date:
    return date.today()


def _snapshot_runtime_paths() -> RuntimePaths:
    root_dir = resolve_config_root_dir()
    active_config_dir = preview_active_config_dir(root_dir)
    active_id = 0
    if active_config_dir.name.startswith("user_config_"):
        try:
            active_id = int(active_config_dir.name.removeprefix("user_config_"))
        except ValueError:
            active_id = 0

    if not (active_config_dir / "settings.toml").exists() and (
        root_dir / "settings.toml"
    ).exists():
        active_config_dir = root_dir

    tasks_dir = root_dir / "tasks"
    return RuntimePaths(
        root_dir=root_dir,
        active_id=active_id,
        active_config_dir=active_config_dir,
        settings_path=active_config_dir / "settings.toml",
        odd_path=active_config_dir / "odd_weeks.toml",
        even_path=active_config_dir / "even_weeks.toml",
        ddl_path=active_config_dir / "ddl.json",
        habit_path=active_config_dir / "habits.toml",
        profile_path=active_config_dir / "profile.md",
        tasks_path=tasks_dir / "tasks.json",
        task_log_path=tasks_dir / "tasks.log",
        record_path=tasks_dir / "record.json",
        procrastinate_path=tasks_dir / "procrastinate.json",
        mode_path=tasks_dir / "mode.txt",
    )


def _require_text(payload: dict[str, Any], key: str) -> str:
    value = payload.get(key)
    if not isinstance(value, str) or not value.strip():
        raise WebError("invalid_input", f"{key} is required.")
    return value.strip()


def _require_priority(payload: dict[str, Any]) -> int:
    value = payload.get("priority")
    try:
        priority = int(value)
    except (TypeError, ValueError):
        raise WebError("invalid_input", "priority must be an integer.") from None
    if priority < 1 or priority > 10:
        raise WebError("invalid_input", "priority must be between 1 and 10.")
    return priority


def _save_tasks_or_error(tasks: list[dict[str, Any]]) -> None:
    try:
        save_tasks(tasks)
    except Exception as exc:
        raise WebError("storage_error", f"failed to save tasks: {exc}") from exc


def _log_task_or_error(
    action: str,
    task: dict[str, Any],
    metadata: dict[str, Any] | None = None,
) -> None:
    try:
        log_task_action(action, task, metadata)
    except Exception as exc:
        raise WebError("storage_error", f"failed to log task action: {exc}") from exc


def _save_deadlines_or_error(deadlines: list[dict[str, Any]]) -> None:
    try:
        save_deadlines(deadlines)
    except Exception as exc:
        raise WebError("storage_error", f"failed to save deadlines: {exc}") from exc


def _save_habit_records_or_error(records: list[dict[str, Any]]) -> None:
    try:
        save_habit_records(records)
    except Exception as exc:
        raise WebError("storage_error", f"failed to save habit records: {exc}") from exc


def _parse_deadline_date(raw_date: str) -> str:
    text = raw_date.strip()
    try:
        if "-" in text:
            parsed = datetime.strptime(text, "%Y-%m-%d").date()
            return parsed.isoformat()

        parts = text.split(".")
        if len(parts) != 2:
            raise ValueError("date must be YYYY-MM-DD or M.D")
        month = int(parts[0])
        day = int(parts[1])
        current = _today()
        parsed = date(current.year, month, day)
        if parsed < current:
            parsed = date(current.year + 1, month, day)
        return parsed.isoformat()
    except ValueError as exc:
        raise WebError("invalid_input", f"invalid deadline date: {exc}") from exc


def _load_task_types() -> dict[str, str]:
    try:
        runtime_paths = _snapshot_runtime_paths()
        config = ScheduleConfig(str(runtime_paths.settings_path))
        task_types = config.task_types
    except Exception:
        task_types = {}
    if not task_types:
        task_types = dict(DEFAULT_TASK_TYPES)
    return task_types


def _sorted_tasks() -> list[dict[str, Any]]:
    today = _today()
    task_types = _load_task_types()
    sorted_type_ids = sorted(task_types.keys(), key=lambda x: int(x) if x.isdigit() else 999)
    default_type = sorted_type_ids[0] if sorted_type_ids else "1"

    procrastinate_records = load_procrastinate_records()
    procrastinate_list = set(procrastinate_records)

    normalized: list[dict[str, Any]] = []
    for task in load_tasks():
        if not isinstance(task, dict):
            continue
        description = task.get("description")
        if not isinstance(description, str) or not description.strip():
            continue
        try:
            priority = int(task.get("priority", 0))
        except (TypeError, ValueError):
            priority = 0

        task_type = str(task.get("type", default_type))
        type_name = task_types.get(task_type, task_types.get(default_type, "other"))

        alarm_from = task.get("alarm_from")
        alarm_from_str: str | None = None
        days_left = 0
        if isinstance(alarm_from, str) and alarm_from.strip():
            alarm_from_str = alarm_from.strip()
            try:
                alarm_date = datetime.strptime(alarm_from_str, "%Y-%m-%d").date()
                days_left = (alarm_date - today).days
            except ValueError:
                days_left = 0

        is_postponed_future = days_left > 0
        is_procrastinated = (not is_postponed_future) and (description in procrastinate_list)
        procrastinate_days: int | None = None
        if is_procrastinated:
            record = procrastinate_records.get(description, {})
            procrastinate_days = get_procrastinate_age_days(
                record.get("since"), today=today
            )

        if is_procrastinated:
            section = 0
        elif not is_postponed_future:
            section = 1
        else:
            section = 2

        normalized.append(
            {
                "description": description.strip(),
                "priority": priority,
                "type": task_type,
                "typeName": type_name,
                "alarmFrom": alarm_from_str,
                "procrastinated": is_procrastinated,
                "procrastinateDays": procrastinate_days,
                "_section": section,
            }
        )

    normalized.sort(key=lambda item: (item["_section"], -item["priority"]))
    for item in normalized:
        del item["_section"]
    return normalized


def _deadline_status(deadline_date: date, current_date: date) -> str:
    days_left = (deadline_date - current_date).days
    if days_left < 0:
        return "overdue"
    if days_left == 0:
        return "today"
    if days_left <= 3:
        return "urgent"
    if days_left <= 7:
        return "soon"
    return "ok"


def _deadline_rows(current_date: date) -> list[dict[str, Any]]:
    deadlines, removed = prune_expired_deadlines(load_deadlines(), today=current_date)
    if removed:
        _save_deadlines_or_error(deadlines)

    rows: list[dict[str, Any]] = []
    for item in sorted(deadlines, key=lambda value: str(value.get("deadline", ""))):
        event = str(item.get("event", "")).strip()
        raw_deadline = str(item.get("deadline", "")).strip()
        if not event or not raw_deadline:
            continue
        try:
            deadline_date = datetime.strptime(raw_deadline, "%Y-%m-%d").date()
        except ValueError:
            rows.append(
                {
                    "event": event,
                    "deadline": raw_deadline,
                    "daysLeft": None,
                    "status": "invalid",
                }
            )
            continue
        rows.append(
            {
                "event": event,
                "deadline": deadline_date.isoformat(),
                "daysLeft": (deadline_date - current_date).days,
                "status": _deadline_status(deadline_date, current_date),
            }
        )
    return rows


def _habit_rows(current_date: date) -> list[dict[str, Any]]:
    habits = load_habits()
    records = load_habit_records()
    completed: dict[str, str] = {}
    for record in records:
        if record.get("date") == current_date.isoformat():
            raw_completed = record.get("completed", {})
            if isinstance(raw_completed, dict):
                completed = {str(key): str(value) for key, value in raw_completed.items()}
            break

    return [
        {
            "id": habit_id,
            "description": description,
            "completed": habit_id in completed,
        }
        for habit_id, description in sorted(habits.items())
    ]


def _schedule_rows(schedule: dict[str, Any]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for time_str in sorted(schedule):
        event = schedule[time_str]
        rows.append(
            {
                "time": time_str,
                "label": format_event_label(event),
                "block": get_event_block_name(event),
                "syncable": bool(time_str in dict(iter_syncable_slots({time_str: event}))),
            }
        )
    return rows


def status_snapshot(payload: dict[str, Any]) -> dict[str, Any]:
    del payload
    current_date = _today()
    runtime_paths = _snapshot_runtime_paths()
    status_commands.SETTINGS_PATH = str(runtime_paths.settings_path)
    status_commands.ODD_PATH = str(runtime_paths.odd_path)
    status_commands.EVEN_PATH = str(runtime_paths.even_path)

    configuration_error = None
    try:
        schedule, parity, is_skipped, config = get_today_schedule_for_status()
    except Exception as exc:
        # A fresh install must still be able to build or repair its schedule
        # entirely in the browser.
        from schedule_management.time_utils import get_week_parity
        schedule, parity, is_skipped, config = {}, get_week_parity(), False, None
        configuration_error = str(exc)

    current_event, next_event, time_to_next = get_current_and_next_events(
        schedule,
        config,
    )
    synced = load_synced_schedule()
    weekday = current_date.strftime("%A").lower()

    return {
        "config": {
            "rootDir": str(runtime_paths.root_dir),
            "activeId": runtime_paths.active_id,
            "activeConfigDir": str(runtime_paths.active_config_dir),
            "tasksPath": str(runtime_paths.tasks_path),
            "deadlinesPath": str(runtime_paths.ddl_path),
            "habitsPath": str(runtime_paths.habit_path),
            "recordsPath": str(runtime_paths.record_path),
            "error": configuration_error,
        },
        "today": {
            "date": current_date.isoformat(),
            "weekday": weekday,
            "parity": parity,
        },
        "schedule": {
            "isSkipped": is_skipped,
            "current": current_event,
            "next": next_event,
            "timeToNext": time_to_next,
            "events": _schedule_rows(schedule),
            "hasSyncedOverlay": synced_schedule_matches_today(
                synced,
                target_date=current_date,
                parity=parity,
                weekday=weekday,
            ),
        },
        "tasks": _sorted_tasks(),
        "deadlines": _deadline_rows(current_date),
        "habits": _habit_rows(current_date),
        "taskTypes": _load_task_types(),
        "history": _history_rows(),
    }


def task_add(payload: dict[str, Any]) -> dict[str, Any]:
    description = _require_text(payload, "description")
    priority = _require_priority(payload)

    task_types = _load_task_types()
    raw_type = payload.get("type")
    task_type = str(raw_type).strip() if raw_type is not None else ""
    if not task_type:
        sorted_type_ids = sorted(task_types.keys(), key=lambda x: int(x) if x.isdigit() else 999)
        task_type = sorted_type_ids[0] if sorted_type_ids else "1"
    elif task_type not in task_types:
        raise WebError("invalid_input", f"unknown task type: {task_type}")

    postpone = payload.get("postpone")
    alarm_from: str | None = None
    if postpone is not None:
        try:
            postpone_days = int(postpone)
        except (TypeError, ValueError):
            raise WebError("invalid_input", "postpone must be an integer.") from None
        if postpone_days < 0:
            raise WebError("invalid_input", "postpone days must be non-negative.")
        if postpone_days > 0:
            from datetime import timedelta
            alarm_from = (_today() + timedelta(days=postpone_days)).isoformat()

    tasks = load_tasks()
    new_task: dict[str, Any] = {
        "description": description,
        "priority": priority,
        "type": task_type,
    }
    if alarm_from:
        new_task["alarm_from"] = alarm_from

    existing_index = None
    for index, task in enumerate(tasks):
        if task.get("description") == description:
            existing_index = index
            break

    if existing_index is None:
        tasks.append(new_task)
        log_action = "added"
        metadata = None
    else:
        old_priority = tasks[existing_index].get("priority")
        tasks[existing_index] = new_task
        log_action = "updated"
        metadata = {"old_priority": old_priority}

    _save_tasks_or_error(tasks)
    _log_task_or_error(log_action, new_task, metadata)
    return new_task


def task_update(payload: dict[str, Any]) -> dict[str, Any]:
    original_description = _require_text(payload, "originalDescription")
    description = _require_text(payload, "description")
    priority = _require_priority(payload)
    tasks = load_tasks()

    for task in tasks:
        if (
            task.get("description") == description
            and task.get("description") != original_description
        ):
            raise WebError("duplicate", f"task already exists: {description}")

    for index, task in enumerate(tasks):
        if task.get("description") == original_description:
            updated = {**task, "description": description, "priority": priority}
            if "type" in payload:
                task_type = str(payload["type"])
                if task_type not in _load_task_types():
                    raise WebError("invalid_input", f"unknown task type: {task_type}")
                updated["type"] = task_type
            if "postpone" in payload:
                try:
                    days = int(payload["postpone"])
                    if days < 0:
                        raise ValueError
                    if days:
                        updated["alarm_from"] = (_today() + timedelta(days=days)).isoformat()
                    else:
                        updated.pop("alarm_from", None)
                except (TypeError, ValueError, OverflowError):
                    raise WebError("invalid_input", "postpone must be a non-negative integer.") from None
            old_task = dict(task)
            tasks[index] = updated
            _save_tasks_or_error(tasks)
            _log_task_or_error("updated", updated, {"old_task": old_task})
            if original_description != description:
                from schedule_management.data.loaders import PROCRASTINATE_PATH
                from pathlib import Path
                import json
                records = load_procrastinate_records()
                if original_description in records:
                    record = records.pop(original_description)
                    records[description] = {**record, "description": description}
                    Path(PROCRASTINATE_PATH).write_text(
                        json.dumps(list(records.values()), ensure_ascii=False, indent=2),
                        encoding="utf-8",
                    )
            return updated

    raise WebError("not_found", f"task not found: {original_description}")


def task_delete(payload: dict[str, Any]) -> dict[str, Any]:
    description = _require_text(payload, "description")
    action = payload.get("action", "deleted")
    if action not in {"deleted", "cancelled", "dropped"}:
        raise WebError("invalid_input", "invalid task action.")
    tasks = load_tasks()
    deleted = [task for task in tasks if task.get("description") == description]
    if not deleted:
        raise WebError("not_found", f"task not found: {description}")

    remaining = [task for task in tasks if task.get("description") != description]
    _save_tasks_or_error(remaining)
    for task in deleted:
        _log_task_or_error(action, dict(task))
    from schedule_management.data import save_procrastinate_list
    records = load_procrastinate_records()
    if description in records:
        save_procrastinate_list(set(records) - {description})
    return {"description": description, "deleted": len(deleted)}


def deadline_add(payload: dict[str, Any]) -> dict[str, Any]:
    event = _require_text(payload, "event")
    deadline = _parse_deadline_date(_require_text(payload, "date"))
    deadlines = load_deadlines()
    new_deadline = {
        "event": event,
        "deadline": deadline,
        "added": datetime.now(timezone.utc).isoformat(),
    }

    existing_index = None
    for index, item in enumerate(deadlines):
        if item.get("event") == event:
            existing_index = index
            break
    if existing_index is None:
        deadlines.append(new_deadline)
    else:
        deadlines[existing_index] = new_deadline
    _save_deadlines_or_error(deadlines)
    return new_deadline


def deadline_update(payload: dict[str, Any]) -> dict[str, Any]:
    original_event = _require_text(payload, "originalEvent")
    event = _require_text(payload, "event")
    deadline = _parse_deadline_date(_require_text(payload, "date"))
    deadlines = load_deadlines()

    for item in deadlines:
        if item.get("event") == event and item.get("event") != original_event:
            raise WebError("duplicate", f"deadline already exists: {event}")

    for index, item in enumerate(deadlines):
        if item.get("event") == original_event:
            updated = {
                "event": event,
                "deadline": deadline,
                "added": str(item.get("added") or datetime.now(timezone.utc).isoformat()),
            }
            deadlines[index] = updated
            _save_deadlines_or_error(deadlines)
            return updated

    raise WebError("not_found", f"deadline not found: {original_event}")


def deadline_delete(payload: dict[str, Any]) -> dict[str, Any]:
    event = _require_text(payload, "event")
    deadlines = load_deadlines()
    deleted = [item for item in deadlines if item.get("event") == event]
    if not deleted:
        raise WebError("not_found", f"deadline not found: {event}")
    remaining = [item for item in deadlines if item.get("event") != event]
    _save_deadlines_or_error(remaining)
    return {"event": event, "deleted": len(deleted)}


def habit_mark(payload: dict[str, Any]) -> dict[str, Any]:
    raw_ids = payload.get("habitIds", [])
    if not isinstance(raw_ids, list):
        raise WebError("invalid_input", "habitIds must be a list.")

    habits = load_habits()
    valid_ids: list[str] = []
    invalid_ids: list[str] = []
    for raw_id in raw_ids:
        habit_id = str(raw_id)
        if habit_id in habits:
            valid_ids.append(habit_id)
        else:
            invalid_ids.append(habit_id)

    if invalid_ids:
        raise WebError(
            "invalid_input",
            "one or more habit IDs are invalid.",
            {"invalidIds": invalid_ids},
        )

    current_date = _today().isoformat()
    completed = {habit_id: habits[habit_id] for habit_id in valid_ids}
    records = load_habit_records()
    new_record = {
        "date": current_date,
        "completed": completed,
        "timestamp": datetime.now(timezone.utc).isoformat(),
    }

    for index, record in enumerate(records):
        if record.get("date") == current_date:
            records[index] = new_record
            break
    else:
        records.append(new_record)

    _save_habit_records_or_error(records)
    return new_record


def _history_rows(count: int | None = 5) -> list[dict[str, Any]]:
    try:
        log_entries = load_task_log()
    except Exception:
        return []
    if not log_entries:
        return []
    activities = _pair_task_activities(log_entries)
    if not activities:
        return []
    recent = activities[-count:] if count is not None else activities[:]
    recent.reverse()
    return [
        {
            "description": a["description"],
            "priority": a["priority"],
            "startedAt": a["started_at"].isoformat(),
            "endedAt": a["ended_at"].isoformat(),
            "duration": _format_duration(a["started_at"], a["ended_at"]),
            "status": a["status"],
        }
        for a in recent
    ]


def task_history(payload: dict[str, Any]) -> dict[str, Any]:
    if payload.get("all") is True:
        return {"activities": _history_rows(None)}
    count = payload.get("count", 10)
    try:
        count = int(count)
    except (TypeError, ValueError):
        count = 10
    if count < 1:
        count = 1
    if count > 50:
        count = 50
    return {"activities": _history_rows(count)}


def settings_get_task_types(payload: dict[str, Any]) -> dict[str, Any]:
    del payload
    return {"taskTypes": _load_task_types()}


def settings_set_task_types(payload: dict[str, Any]) -> dict[str, Any]:
    raw_types = payload.get("taskTypes")
    if not isinstance(raw_types, dict):
        raise WebError("invalid_input", "taskTypes must be an object.")

    validated: dict[str, str] = {}
    for key, value in raw_types.items():
        key_str = str(key).strip()
        value_str = str(value).strip() if value is not None else ""
        if not key_str:
            raise WebError("invalid_input", "task type keys must be non-empty.")
        if not value_str:
            raise WebError("invalid_input", f"task type '{key_str}' name must be non-empty.")
        validated[key_str] = value_str

    runtime_paths = _snapshot_runtime_paths()
    from schedule_management.commands.settings import SettingsModel

    try:
        model = SettingsModel(runtime_paths.settings_path)
    except Exception as exc:
        raise WebError("config_error", f"failed to load settings: {exc}") from exc

    model.data["task_types"] = validated
    model.dirty = True
    try:
        model.save()
    except Exception as exc:
        raise WebError("storage_error", f"failed to save settings: {exc}") from exc

    return {"taskTypes": validated}
