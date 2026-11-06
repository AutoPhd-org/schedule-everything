"""
Structured service operations for the Schedule Everything browser interface.

This module wraps existing schedule-management storage and schedule helpers
without printing CLI tables or prompts. Functions accept JSON-like payloads and
return JSON-serializable dictionaries for the local HTTP API.
"""

from __future__ import annotations

import tomllib
from datetime import date, datetime, timedelta, timezone
from typing import Any

from schedule_management.commands import status as status_commands
from schedule_management.commands.deadlines import refresh_deadlines
from schedule_management.commands.history import (
    _format_duration,
    _pair_task_activities,
    parse_log_timestamp,
)
from schedule_management.commands.settings import DEFAULT_TASK_TYPES
from schedule_management.commands.status import (
    get_current_and_next_events,
    get_today_schedule_for_status,
)
from schedule_management.config import ScheduleConfig
from schedule_management.recurrence import REPEAT_RULES, anchor_day, repeat_rule
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
    save_task_log,
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


# Named marker colors for deadlines; the browser maps each name to a themed hue.
DEADLINE_COLORS = ("blue", "orange", "aqua", "yellow", "magenta", "green", "violet", "red")


def _today() -> date:
    return date.today()


def _default_task_type(task_types: dict[str, str]) -> str:
    sorted_type_ids = sorted(task_types, key=lambda x: int(x) if x.isdigit() else 999)
    return sorted_type_ids[0] if sorted_type_ids else "1"


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


_DUE_FORMATS = ("%Y-%m-%dT%H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M")


def _parse_due(value: Any) -> datetime | None:
    """A timed task's finish-by time as a naive local datetime, or None when it has none or it is unreadable."""
    if not isinstance(value, str) or not value.strip():
        return None
    for fmt in _DUE_FORMATS:
        try:
            return datetime.strptime(value.strip(), fmt)
        except ValueError:
            continue
    return None


def _require_due(payload: dict[str, Any]) -> str | None:
    """Read ``due`` from a payload: a local ``YYYY-MM-DDTHH:MM`` makes the task timed; empty or null clears it."""
    value = payload.get("due")
    if value is None or (isinstance(value, str) and not value.strip()):
        return None
    due = _parse_due(value)
    if due is None:
        raise WebError("invalid_input", "due must be a local date and time like 2026-11-05T18:00.")
    return due.strftime("%Y-%m-%dT%H:%M")


def _task_progress(task: dict[str, Any]) -> int:
    try:
        progress = int(task.get("progress", 0))
    except (TypeError, ValueError):
        return 0
    return max(0, min(100, progress))


def _save_tasks_or_error(tasks: list[dict[str, Any]]) -> None:
    try:
        save_tasks(tasks)
    except Exception as exc:
        raise WebError("storage_error", f"failed to save tasks: {exc}") from exc


def _log_task_or_error(
    action: str,
    task: dict[str, Any],
    metadata: dict[str, Any] | None = None,
) -> dict[str, Any] | None:
    try:
        return log_task_action(action, task, metadata)
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
    default_type = _default_task_type(task_types)

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

        due = _parse_due(task.get("due"))
        normalized.append(
            {
                "description": description.strip(),
                "priority": priority,
                "type": task_type,
                "typeName": type_name,
                "alarmFrom": alarm_from_str,
                "procrastinated": is_procrastinated,
                "procrastinateDays": procrastinate_days,
                "due": due.strftime("%Y-%m-%dT%H:%M") if due else None,
                "progress": _task_progress(task),
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
    deadlines, _removed, changed = refresh_deadlines(load_deadlines(), today=current_date)
    if changed:
        _save_deadlines_or_error(deadlines)

    task_types = _load_task_types()
    rows: list[dict[str, Any]] = []
    for item in sorted(deadlines, key=lambda value: str(value.get("deadline", ""))):
        event = str(item.get("event", "")).strip()
        raw_deadline = str(item.get("deadline", "")).strip()
        if not event or not raw_deadline:
            continue
        raw_type = item.get("type")
        type_id = (str(raw_type).strip() if raw_type is not None else "") or None
        raw_color = str(item.get("color") or "").strip().lower()
        rule = repeat_rule(item)
        type_fields = {
            "type": type_id,
            "typeName": task_types.get(type_id, type_id) if type_id else None,
            "color": raw_color if raw_color in DEADLINE_COLORS else None,
            "repeat": rule,
            "repeatDay": None,
        }
        try:
            deadline_date = datetime.strptime(raw_deadline, "%Y-%m-%d").date()
        except ValueError:
            rows.append(
                {
                    "event": event,
                    "deadline": raw_deadline,
                    "daysLeft": None,
                    "status": "invalid",
                    **type_fields,
                }
            )
            continue
        if rule == "monthly":
            type_fields["repeatDay"] = anchor_day(item, deadline_date)
        rows.append(
            {
                "event": event,
                "deadline": deadline_date.isoformat(),
                "daysLeft": (deadline_date - current_date).days,
                "status": _deadline_status(deadline_date, current_date),
                **type_fields,
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
        "taskTypeColors": _load_task_type_colors(),
        "history": _history_rows(),
        "timedDone": _timed_done_today(current_date),
    }


def _timed_done_today(current_date: date) -> list[dict[str, Any]]:
    """Timed tasks completed today that were due by tonight.

    They still count, as finished work, toward today's progress, so completing a
    timed task never lowers it. A task that is open again is left out.
    """
    try:
        log_entries = load_task_log()
    except Exception:
        return []
    open_descriptions = {
        task.get("description") for task in load_tasks() if isinstance(task, dict)
    }
    done: dict[str, dict[str, Any]] = {}
    for entry in log_entries:
        if not isinstance(entry, dict) or entry.get("action") != "deleted":
            continue
        task = entry.get("task")
        if not isinstance(task, dict):
            continue
        description = task.get("description")
        due = _parse_due(task.get("due"))
        ended = parse_log_timestamp(entry.get("timestamp", ""))
        if not isinstance(description, str) or not description.strip() or due is None or ended is None:
            continue
        if ended.date() != current_date or due.date() > current_date or description in open_descriptions:
            continue
        done[description] = {
            "description": description,
            "type": str(task.get("type")) if task.get("type") is not None else None,
            "due": due.strftime("%Y-%m-%dT%H:%M"),
            "endedAt": ended.isoformat(),
        }
    return sorted(done.values(), key=lambda item: item["endedAt"])


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
    due = _require_due(payload)

    tasks = load_tasks()
    new_task: dict[str, Any] = {
        "description": description,
        "priority": priority,
        "type": task_type,
    }
    if alarm_from:
        new_task["alarm_from"] = alarm_from
    if due:
        new_task["due"] = due

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
            if "due" in payload:
                due = _require_due(payload)
                if due:
                    updated["due"] = due
                else:
                    updated.pop("due", None)
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


def task_progress(payload: dict[str, Any]) -> dict[str, Any]:
    """Set how far along a task is, from 0 to 100 percent.

    Progress moves often, so it is saved on the task without a history entry.
    """
    description = _require_text(payload, "description")
    value = payload.get("progress")
    invalid = WebError("invalid_input", "progress must be a whole number from 0 to 100.")
    if isinstance(value, bool) or not isinstance(value, (int, float, str)):
        raise invalid
    try:
        number = float(value)
    except ValueError:
        raise invalid from None
    if not number.is_integer() or not 0 <= number <= 100:
        raise invalid
    progress = int(number)
    tasks = load_tasks()
    for task in tasks:
        if task.get("description") == description:
            if progress:
                task["progress"] = progress
            else:
                task.pop("progress", None)
            _save_tasks_or_error(tasks)
            return {"description": description, "progress": progress}
    raise WebError("not_found", f"task not found: {description}")


_CLOSING_ACTIONS = {"deleted", "cancelled", "dropped"}


def task_delete(payload: dict[str, Any]) -> dict[str, Any]:
    """Complete, cancel, or drop a task.

    The result carries an ``undo`` token: everything ``task_restore`` needs to
    put the task back as it was, in its place on the list, still deferred if it
    was, and without the history entry this removal wrote.
    """
    description = _require_text(payload, "description")
    action = payload.get("action", "deleted")
    if action not in _CLOSING_ACTIONS:
        raise WebError("invalid_input", "invalid task action.")
    tasks = load_tasks()
    deleted = [
        {"index": index, "task": dict(task)}
        for index, task in enumerate(tasks)
        if task.get("description") == description
    ]
    if not deleted:
        raise WebError("not_found", f"task not found: {description}")

    remaining = [task for task in tasks if task.get("description") != description]
    _save_tasks_or_error(remaining)
    logged: list[str] = []
    for item in deleted:
        entry = _log_task_or_error(action, dict(item["task"]))
        if entry and isinstance(entry.get("timestamp"), str):
            logged.append(entry["timestamp"])
    from schedule_management.data import save_procrastinate_list
    records = load_procrastinate_records()
    procrastinated = records.get(description)
    if procrastinated is not None:
        save_procrastinate_list(set(records) - {description})
    return {
        "description": description,
        "deleted": len(deleted),
        "undo": {
            "description": description,
            "action": action,
            "tasks": deleted,
            "procrastinated": None if procrastinated is None else {"since": procrastinated.get("since")},
            "logged": logged,
        },
    }


def _restore_procrastinated(description: str, since: str | None) -> None:
    """Mark a task as deferred again, keeping the day it was first deferred."""
    from pathlib import Path
    import json
    from schedule_management.data.loaders import PROCRASTINATE_PATH

    records = load_procrastinate_records()
    record: dict[str, str] = {"description": description}
    if since:
        record["since"] = since
    records[description] = record
    path = Path(PROCRASTINATE_PATH)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps([records[key] for key in sorted(records)], ensure_ascii=False, indent=2),
        encoding="utf-8",
    )


def task_restore(payload: dict[str, Any]) -> dict[str, Any]:
    """Undo ``task_delete`` with the ``undo`` token it returned."""
    description = _require_text(payload, "description")
    action = payload.get("action")
    if action not in _CLOSING_ACTIONS:
        raise WebError("invalid_input", "invalid task action.")
    items = payload.get("tasks")
    if not isinstance(items, list) or not items:
        raise WebError("invalid_input", "tasks are required.")
    restored: list[tuple[int, dict[str, Any]]] = []
    for item in items:
        task = item.get("task") if isinstance(item, dict) else None
        index = item.get("index") if isinstance(item, dict) else None
        if not isinstance(task, dict) or task.get("description") != description:
            raise WebError("invalid_input", "every task must match the description.")
        if not isinstance(index, int) or isinstance(index, bool) or index < 0:
            raise WebError("invalid_input", "task index must be a non-negative integer.")
        restored.append((index, task))

    tasks = load_tasks()
    if any(task.get("description") == description for task in tasks):
        raise WebError("duplicate", f"“{description}” is already on your list.")
    # Indexes were taken from the list before the removal, so inserting in order puts each task back in place.
    for index, task in sorted(restored, key=lambda pair: pair[0]):
        tasks.insert(min(index, len(tasks)), task)
    _save_tasks_or_error(tasks)

    # Take back the history entries the removal wrote, so it never counts as completed, cancelled, or dropped.
    logged = {stamp for stamp in payload.get("logged") or [] if isinstance(stamp, str)}
    if logged:
        log_entries = load_task_log()
        kept = [
            entry for entry in log_entries
            if not (
                isinstance(entry, dict)
                and entry.get("timestamp") in logged
                and entry.get("action") == action
                and isinstance(entry.get("task"), dict)
                and entry["task"].get("description") == description
            )
        ]
        if len(kept) != len(log_entries):
            try:
                save_task_log(kept)
            except Exception as exc:
                raise WebError("storage_error", f"failed to save the task log: {exc}") from exc

    procrastinated = payload.get("procrastinated")
    if isinstance(procrastinated, dict):
        since = procrastinated.get("since")
        _restore_procrastinated(description, since if isinstance(since, str) else None)
    return {"description": description, "restored": len(restored)}


def _apply_deadline_type(item: dict[str, Any], payload: dict[str, Any]) -> None:
    """Keep legacy deadlines untyped and preserve types omitted by older clients."""
    if "type" not in payload:
        return
    raw_type = payload.get("type")
    type_id = str(raw_type).strip() if raw_type is not None else ""
    if type_id and type_id not in _load_task_types() and type_id != str(item.get("type")):
        raise WebError("invalid_input", f"unknown deadline type: {type_id}")
    if type_id:
        item["type"] = type_id
    else:
        item.pop("type", None)


def _apply_deadline_color(item: dict[str, Any], payload: dict[str, Any]) -> None:
    """Set or clear a deadline marker color; omitted colors stay unchanged."""
    if "color" not in payload:
        return
    raw_color = payload.get("color")
    color = str(raw_color).strip().lower() if raw_color is not None else ""
    if color and color not in DEADLINE_COLORS:
        raise WebError("invalid_input", f"unknown deadline color: {color}")
    if color:
        item["color"] = color
    else:
        item.pop("color", None)


def _apply_deadline_repeat(item: dict[str, Any], payload: dict[str, Any], previous_deadline: Any) -> None:
    """Set, clear, or keep a deadline's repeat rule; omitted rules stay unchanged.

    A monthly deadline remembers the day of the month the user picked
    (`repeat_day`) and updates it only when the user chooses a new date, so
    rolling past a short month does not drift the day.
    """
    if "repeat" in payload:
        raw_rule = payload.get("repeat")
        rule = str(raw_rule).strip().lower() if raw_rule is not None else ""
        if rule and rule not in REPEAT_RULES:
            raise WebError("invalid_input", f"unknown repeat rule: {rule}")
        if rule:
            if item.get("repeat") != rule:
                item.pop("repeat_day", None)
            item["repeat"] = rule
        else:
            item.pop("repeat", None)
    if item.get("repeat") == "monthly":
        if item["deadline"] != previous_deadline or "repeat_day" not in item:
            item["repeat_day"] = date.fromisoformat(item["deadline"]).day
    else:
        item.pop("repeat_day", None)


def deadline_add(payload: dict[str, Any]) -> dict[str, Any]:
    event = _require_text(payload, "event")
    deadline = _parse_deadline_date(_require_text(payload, "date"))
    deadlines = load_deadlines()
    existing_index = None
    for index, item in enumerate(deadlines):
        if item.get("event") == event:
            existing_index = index
            break
    new_deadline = {
        **(deadlines[existing_index] if existing_index is not None else {}),
        "event": event,
        "deadline": deadline,
        "added": datetime.now(timezone.utc).isoformat(),
    }
    _apply_deadline_type(new_deadline, payload)
    _apply_deadline_color(new_deadline, payload)
    _apply_deadline_repeat(new_deadline, payload, deadlines[existing_index].get("deadline") if existing_index is not None else None)
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
                **item,
                "event": event,
                "deadline": deadline,
                "added": str(item.get("added") or datetime.now(timezone.utc).isoformat()),
            }
            _apply_deadline_type(updated, payload)
            _apply_deadline_color(updated, payload)
            _apply_deadline_repeat(updated, payload, item.get("deadline"))
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
    task_types = _load_task_types()
    default_type = _default_task_type(task_types)
    rows: list[dict[str, Any]] = []
    for a in recent:
        type_id = str(a.get("type") or default_type)
        rows.append(
            {
                "description": a["description"],
                "priority": a["priority"],
                "startedAt": a["started_at"].isoformat(),
                "endedAt": a["ended_at"].isoformat(),
                "duration": _format_duration(a["started_at"], a["ended_at"]),
                "status": a["status"],
                "type": type_id,
                "typeName": task_types.get(type_id, type_id),
            }
        )
    return rows


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

    _save_task_types(validated)
    return {"taskTypes": validated}


TASK_TYPE_NAME_LIMIT = 60
# Saved type colors, so a type keeps its color when the types before it change.
TASK_TYPE_COLORS = "task_type_colors"


def _type_order(type_id: str) -> tuple[int, int, str]:
    return (0, int(type_id), "") if type_id.isdigit() else (1, 0, type_id)


def _load_task_type_colors() -> dict[str, str]:
    """Saved type colors; only the palette's color names count."""
    try:
        with open(_snapshot_runtime_paths().settings_path, "rb") as handle:
            saved = tomllib.load(handle).get(TASK_TYPE_COLORS)
    except Exception:
        return {}
    if not isinstance(saved, dict):
        return {}
    return {str(key): value for key, value in saved.items() if value in DEADLINE_COLORS}


def _shown_type_colors(task_types: dict[str, str]) -> dict[str, str]:
    """Each type's color as the browser shows it now: its saved color, or else
    the palette color of its place among the types (the browser's fallback)."""
    saved = _load_task_type_colors()
    ordered = sorted(task_types, key=_type_order)
    return {
        type_id: saved.get(type_id) or DEADLINE_COLORS[index % len(DEADLINE_COLORS)]
        for index, type_id in enumerate(ordered)
    }


def _hashed_type_color(type_id: str) -> str:
    """The color the browser gives a type ID that is not in the settings."""
    return DEADLINE_COLORS[sum(ord(char) for char in type_id) % len(DEADLINE_COLORS)]


def _least_used_color(colors: dict[str, str]) -> str:
    counts = {color: 0 for color in DEADLINE_COLORS}
    for color in colors.values():
        counts[color] = counts.get(color, 0) + 1
    return min(DEADLINE_COLORS, key=lambda color: counts[color])


def _save_task_types(task_types: dict[str, str], colors: dict[str, str] | None = None) -> None:
    runtime_paths = _snapshot_runtime_paths()
    from schedule_management.commands.settings import SettingsModel

    try:
        model = SettingsModel(runtime_paths.settings_path)
    except Exception as exc:
        raise WebError("config_error", f"failed to load settings: {exc}") from exc

    model.data["task_types"] = task_types
    if colors is not None:
        model.data[TASK_TYPE_COLORS] = {type_id: colors[type_id] for type_id in sorted(colors, key=_type_order) if type_id in task_types}
    model.dirty = True
    try:
        model.save()
    except Exception as exc:
        raise WebError("storage_error", f"failed to save settings: {exc}") from exc


def _next_task_type_id(task_types: dict[str, str]) -> str:
    """One past the highest numeric type ID ever used, by a type, a task, a
    deadline, or the task history, so a new type never inherits another's past."""
    used = set(task_types)
    for item in [*load_tasks(), *load_deadlines()]:
        if isinstance(item, dict) and item.get("type") is not None:
            used.add(str(item["type"]))
    try:
        log_entries = load_task_log()
    except Exception:
        log_entries = []
    for entry in log_entries:
        task = entry.get("task") if isinstance(entry, dict) else None
        if isinstance(task, dict) and task.get("type") is not None:
            used.add(str(task["type"]))
    return str(max((int(key) for key in used if key.isdigit()), default=0) + 1)


def task_type_save(payload: dict[str, Any]) -> dict[str, Any]:
    """Add a task type, or rename the type with `id`.

    Naming an ID that only existing tasks still use (its type was removed from
    the settings) brings that type back under the new name.
    """
    name = _require_text(payload, "name")
    if len(name) > TASK_TYPE_NAME_LIMIT:
        raise WebError("invalid_input", f"task type names can be at most {TASK_TYPE_NAME_LIMIT} characters.")
    task_types = dict(_load_task_types())
    raw_id = payload.get("id")
    type_id = str(raw_id).strip() if raw_id is not None else ""
    if not type_id:
        type_id = _next_task_type_id(task_types)
    elif type_id not in task_types and not any(
        isinstance(task, dict) and str(task.get("type")) == type_id for task in load_tasks()
    ):
        raise WebError("not_found", f"task type not found: {type_id}")
    if any(key != type_id and value.casefold() == name.casefold() for key, value in task_types.items()):
        raise WebError("duplicate", f"a task type named “{name}” already exists.")
    # Every color is saved as it shows now, so later changes to the list never shift it. A new type gets the
    # least-used color; a type named back from its tasks keeps the color it already shows.
    colors = _shown_type_colors(task_types)
    if type_id not in colors:
        colors[type_id] = _hashed_type_color(type_id) if payload.get("id") else _least_used_color(colors)
    task_types[type_id] = name
    _save_task_types(task_types, colors)
    return {"id": type_id, "name": name, "taskTypes": task_types, "color": colors[type_id]}


def task_type_delete(payload: dict[str, Any]) -> dict[str, Any]:
    """Delete a task type and move what uses it.

    Open tasks of the type move to `moveTo`, which is required when there are
    any. Deadlines of the type move there too, or become uncategorized when no
    `moveTo` is given. Finished tasks in the history keep the old type ID.
    """
    type_id = _require_text(payload, "id")
    task_types = dict(_load_task_types())
    default_type = _default_task_type(task_types)
    tasks = load_tasks()
    # Tasks saved without a type belong to the default type.
    owned = [task for task in tasks if isinstance(task, dict) and str(task.get("type", default_type)) == type_id]
    if type_id not in task_types and not owned:
        raise WebError("not_found", f"task type not found: {type_id}")
    remaining = {key: value for key, value in task_types.items() if key != type_id}
    if not remaining:
        raise WebError("invalid_input", "keep at least one task type.")
    raw_target = payload.get("moveTo")
    move_to = str(raw_target).strip() if raw_target is not None else ""
    if move_to and move_to not in remaining:
        raise WebError("invalid_input", f"unknown task type: {move_to}")
    if owned and not move_to:
        raise WebError("invalid_input", "choose a type for this type's tasks.")

    for task in owned:
        task["type"] = move_to
    if owned:
        _save_tasks_or_error(tasks)
    deadlines = load_deadlines()
    moved_deadlines = 0
    for item in deadlines:
        if isinstance(item, dict) and item.get("type") is not None and str(item["type"]) == type_id:
            if move_to:
                item["type"] = move_to
            else:
                item.pop("type", None)
            moved_deadlines += 1
    if moved_deadlines:
        _save_deadlines_or_error(deadlines)
    if type_id in task_types:
        # The other types keep the colors they show now instead of moving up the palette.
        _save_task_types(remaining, _shown_type_colors(task_types))
    return {"taskTypes": remaining, "movedTasks": len(owned), "movedDeadlines": moved_deadlines}
