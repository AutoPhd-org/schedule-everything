"""
Daily plan command (`rmd sync`) for today's focus blocks.

Planning itself happens in the user's AI agent (the Schedule Everything plugin
for Claude Code, Codex, or Antigravity). This module provides the deterministic
parts the agent and the browser use: today's assignable focus slots and open
tasks as JSON, validating and saving a plan as the synced overlay, and clearing
it. The overlay changes only today; the weekly routine files stay untouched.
"""

from __future__ import annotations

import json
import sys
from datetime import date
from pathlib import Path
from typing import Any

from schedule_management.i18n import _t

try:
    from rich import box
    from rich.console import Console
    from rich.table import Table
except ImportError:
    print(_t("Please install the 'rich' library: pip install rich"))
    sys.exit(1)

from schedule_management import EVEN_PATH, ODD_PATH, SETTINGS_PATH
from schedule_management.commands.deadlines import refresh_deadlines
from schedule_management.config import ScheduleConfig, WeeklySchedule
from schedule_management.data import (
    get_procrastinate_age_days,
    load_deadlines,
    load_mode,
    load_procrastinate_records,
    load_tasks,
)
from schedule_management.synced_schedule import (
    SyncedDaySchedule,
    apply_synced_schedule,
    format_event_label,
    get_event_block_name,
    iter_syncable_slots,
    load_synced_schedule,
    resolve_synced_schedule_path,
    save_synced_schedule,
    synced_schedule_matches_today,
)
from schedule_management.time_utils import get_week_parity

CONSOLE = Console()
MAX_TITLE_LENGTH = 120
P_MODE_MESSAGE = "Currently in p mode. Switch back to j mode to execute this command."


class SyncPlanError(ValueError):
    """A daily plan that cannot be applied, with a user-facing message."""


def _today() -> date:
    """Return today's date for sync operations."""
    return date.today()


def _get_base_today_schedule() -> tuple[dict[str, Any], str, bool, ScheduleConfig]:
    """Load today's base schedule without applying an existing sync overlay."""
    config = ScheduleConfig(SETTINGS_PATH)
    weekly = WeeklySchedule(ODD_PATH, EVEN_PATH)
    is_skipped = config.should_skip_today()
    parity = get_week_parity()
    schedule = {} if is_skipped else weekly.get_today_schedule(config)
    return schedule, parity, is_skipped, config


def _as_int(value: Any) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return 0


def _open_tasks(today: date, task_types: dict[str, str]) -> list[dict[str, Any]]:
    """Open tasks for planning, most important first."""
    records = load_procrastinate_records()
    rows: list[dict[str, Any]] = []
    for task in load_tasks():
        if not isinstance(task, dict):
            continue
        description = task.get("description")
        if not isinstance(description, str) or not description.strip():
            continue
        alarm_from = task.get("alarm_from") if isinstance(task.get("alarm_from"), str) else None
        type_id = str(task.get("type")) if task.get("type") is not None else None
        procrastinated = description in records
        rows.append(
            {
                "description": description.strip(),
                "priority": _as_int(task.get("priority")),
                "type": type_id,
                "typeName": task_types.get(type_id) if type_id else None,
                "alarmFrom": alarm_from,
                "postponed": bool(alarm_from and alarm_from > today.isoformat()),
                "procrastinated": procrastinated,
                "procrastinateDays": get_procrastinate_age_days(records[description].get("since"), today=today) if procrastinated else None,
            }
        )
    return sorted(rows, key=lambda row: row["priority"], reverse=True)


def _upcoming_deadlines(today: date) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for item in refresh_deadlines(load_deadlines(), today)[0]:
        try:
            due = date.fromisoformat(str(item.get("deadline", "")))
        except ValueError:
            continue
        days_left = (due - today).days
        if days_left >= 0:
            rows.append({"event": str(item.get("event", "")), "deadline": due.isoformat(), "daysLeft": days_left})
    return sorted(rows, key=lambda row: row["daysLeft"])


def _todays_plan(today: date, parity: str, weekday: str) -> SyncedDaySchedule | None:
    synced = load_synced_schedule()
    return synced if synced_schedule_matches_today(synced, target_date=today, parity=parity, weekday=weekday) else None


def sync_context() -> dict[str, Any]:
    """Return today's focus slots, schedule, tasks, deadlines, and current plan."""
    today = _today()
    weekday = today.strftime("%A").lower()
    schedule, parity, is_skipped, config = _get_base_today_schedule()
    plan = _todays_plan(today, parity, weekday)
    planned = plan.assignments if plan else {}
    blocks = config.time_blocks if isinstance(getattr(config, "time_blocks", None), dict) else {}
    merged = apply_synced_schedule(schedule, target_date=today, parity=parity, weekday=weekday, synced=plan) if plan else schedule
    return {
        "date": today.isoformat(),
        "weekday": weekday,
        "parity": parity,
        "mode": load_mode(),
        "skipped": is_skipped,
        "slots": [
            {"time": time_str, "block": block, "minutes": blocks.get(block), "title": planned.get(time_str, {}).get("title")}
            for time_str, block in iter_syncable_slots(schedule)
        ],
        "events": [
            {"time": time_str, "label": format_event_label(merged[time_str]), "block": get_event_block_name(merged[time_str])}
            for time_str in sorted(merged)
        ],
        "tasks": _open_tasks(today, config.task_types if isinstance(getattr(config, "task_types", None), dict) else {}),
        "deadlines": _upcoming_deadlines(today),
        "plan": {"date": plan.target_date, "assignments": {time_str: item["title"] for time_str, item in sorted(planned.items())}} if plan else None,
    }


def _normalize_assignments(raw: Any, slots: dict[str, str]) -> dict[str, dict[str, str]]:
    """Validate agent-provided assignments against today's focus slots."""
    if isinstance(raw, list):
        pairs = []
        for item in raw:
            if not isinstance(item, dict):
                raise SyncPlanError("Each assignment in a list must be an object with time and title.")
            pairs.append((item.get("time"), item.get("title")))
    elif isinstance(raw, dict):
        pairs = [(time_str, value.get("title") if isinstance(value, dict) else value) for time_str, value in raw.items()]
    else:
        raise SyncPlanError("The plan needs an 'assignments' object mapping slot times to task titles.")

    assignments: dict[str, dict[str, str]] = {}
    for time_str, title in pairs:
        if not isinstance(time_str, str) or time_str not in slots:
            available = ", ".join(slots) or "none"
            raise SyncPlanError(f"{time_str!r} is not one of today's focus slots ({available}).")
        if not isinstance(title, str) or not title.strip():
            raise SyncPlanError(f"The assignment for {time_str} needs a title.")
        title = " ".join(title.split())
        if len(title) > MAX_TITLE_LENGTH:
            raise SyncPlanError(f"The title for {time_str} is longer than {MAX_TITLE_LENGTH} characters.")
        assignments[time_str] = {"block": slots[time_str], "title": title}
    if not assignments:
        raise SyncPlanError("The plan has no assignments. Use `rmd sync --clear` to remove today's plan.")
    return assignments


def apply_sync_plan(payload: dict[str, Any]) -> dict[str, Any]:
    """Validate a plan for today and save it as the synced overlay."""
    if not isinstance(payload, dict):
        raise SyncPlanError("The plan must be a JSON object.")
    if load_mode() == "p":
        raise SyncPlanError(P_MODE_MESSAGE)
    today = _today()
    plan_date = payload.get("date", today.isoformat())
    if plan_date != today.isoformat():
        raise SyncPlanError(f"The plan is for {plan_date}, but today is {today.isoformat()}. Plans apply to today only.")
    schedule, parity, is_skipped, _config = _get_base_today_schedule()
    if is_skipped:
        raise SyncPlanError("Today is a skip day, so there is nothing to plan.")
    assignments = _normalize_assignments(payload.get("assignments"), dict(iter_syncable_slots(schedule)))
    plan = SyncedDaySchedule(target_date=today.isoformat(), parity=parity, weekday=today.strftime("%A").lower(), assignments=assignments)
    saved_path = save_synced_schedule(plan)
    return {"savedPath": str(saved_path), "assigned": len(assignments)}


def clear_sync_plan() -> dict[str, Any]:
    """Remove the saved daily plan, if any."""
    path = resolve_synced_schedule_path()
    existed = path.exists()
    if existed:
        path.unlink()
    return {"cleared": existed}


def _read_plan(source: str) -> dict[str, Any]:
    text = sys.stdin.read() if source == "-" else Path(source).expanduser().read_text(encoding="utf-8")
    try:
        payload = json.loads(text)
    except json.JSONDecodeError as exc:
        raise SyncPlanError(f"The plan is not valid JSON: {exc}") from exc
    return payload


def _print_plan(context: dict[str, Any]) -> None:
    if context["skipped"]:
        CONSOLE.print("[bold yellow]" + _t("Today is a skipped day. Nothing to sync.") + "[/]")
        return
    if not context["slots"]:
        CONSOLE.print("[bold yellow]" + _t("No untitled pomodoro/potato blocks need syncing today.") + "[/]")
        return
    table = Table(title="[bold]" + _t("Today's focus blocks") + "[/bold]", box=box.ROUNDED, header_style="bold cyan")
    table.add_column(_t("Time"), justify="right", style="cyan", no_wrap=True)
    table.add_column(_t("Block"))
    table.add_column(_t("Planned task"))
    for slot in context["slots"]:
        table.add_row(slot["time"], slot["block"], slot["title"] or "[dim]—[/dim]")
    CONSOLE.print(table)
    if not context["plan"]:
        CONSOLE.print(_t("No plan yet. Ask your AI agent to plan today with the Schedule Everything plugin, or apply a plan with `rmd sync --apply plan.json`."))


def sync_command(args) -> int:
    """Handle `rmd sync [--json | --apply FILE | --clear]`."""
    apply_source = getattr(args, "apply", None)
    apply_source = apply_source if isinstance(apply_source, str) else None
    try:
        if getattr(args, "clear", False) is True:
            result = clear_sync_plan()
            print(_t("Cleared today's plan.") if result["cleared"] else _t("There was no saved plan."))
            return 0
        if getattr(args, "json", False) is True:
            print(json.dumps(sync_context(), ensure_ascii=False, indent=2))
            return 0
        if load_mode() == "p":
            print("❌ " + _t(P_MODE_MESSAGE))
            return 1
        if apply_source:
            result = apply_sync_plan(_read_plan(apply_source))
            print(_t("Saved today's plan with {count} focus blocks to {path}.").format(count=result["assigned"], path=result["savedPath"]))
            return 0
        _print_plan(sync_context())
        return 0
    except SyncPlanError as exc:
        print("❌ " + str(exc), file=sys.stderr)
        return 1
    except (OSError, ValueError) as exc:
        print("❌ " + _t("Failed to load today's schedule:") + f" {exc}", file=sys.stderr)
        return 1
