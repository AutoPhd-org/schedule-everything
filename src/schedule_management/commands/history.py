"""
History Commands - CLI commands for viewing recent task activity.

This module provides the 'history' command which displays recently
completed tasks with their lifecycle information (when they were
added and when they were completed/deleted).

Activities are grouped by calendar day and displayed with visual
separators between days.

Example Usage (via CLI):
    $ rmd history          # Show 5 most recent activities
    $ rmd history 10       # Show 10 most recent activities
"""

import sys
from collections import defaultdict
from datetime import datetime
from typing import Any

try:
    from rich import box
    from rich.console import Console
    from rich.panel import Panel
    from rich.text import Text
except ImportError:
    print("Please install the 'rich' library: pip install rich")
    sys.exit(1)

from schedule_management.i18n import _t
from schedule_management.data import load_task_log


# =============================================================================
# HISTORY DATA PROCESSING
# =============================================================================


def _pair_task_activities(log_entries: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """
    Pair 'added' log entries with their closing action into activities.

    A closing action is one of 'deleted' (counts as completion), 'cancelled'
    (added by mistake), or 'dropped' (gave up). Walks the log chronologically,
    tracking open 'added' entries per description, and closes the most recent
    matching open entry when a closing action is found.

    Returns:
        List of activity dicts with keys: description, priority,
        started_at (datetime), ended_at (datetime), and status
        ('completed' | 'cancelled' | 'dropped').
        Sorted by ended_at ascending.
    """
    open_adds: dict[str, list[dict[str, Any]]] = defaultdict(list)
    activities: list[dict[str, Any]] = []

    sorted_entries = sorted(log_entries, key=lambda e: e.get("timestamp", ""))

    for entry in sorted_entries:
        action = entry.get("action")
        task = entry.get("task", {})
        if not isinstance(task, dict):
            continue

        description = task.get("description", "")
        if not description:
            continue

        ts_str = entry.get("timestamp", "")
        try:
            ts = datetime.fromisoformat(ts_str)
            if ts.tzinfo:
                ts = ts.replace(tzinfo=None)
        except (ValueError, TypeError):
            continue

        if action == "added":
            open_adds[description].append({
                "started_at": ts,
                "priority": task.get("priority", 0),
            })

        elif action == "updated":
            metadata = entry.get("metadata", {})
            old_task = metadata.get("old_task", {}) if isinstance(metadata, dict) else {}
            old_description = old_task.get("description") if isinstance(old_task, dict) else None
            if old_description and old_description != description:
                open_adds[description].extend(open_adds.pop(old_description, []))

        elif action in ("deleted", "cancelled", "dropped"):
            if open_adds[description]:
                add_record = open_adds[description].pop(0)
                activities.append({
                    "description": description,
                    "priority": add_record["priority"],
                    "started_at": add_record["started_at"],
                    "ended_at": ts,
                    "status": "completed" if action == "deleted" else action,
                })

    activities.sort(key=lambda a: a["ended_at"])
    return activities


# =============================================================================
# HISTORY COMMAND
# =============================================================================


def _priority_style(priority: int) -> tuple[str, str]:
    """Return (color, icon) for a given priority level."""
    if priority >= 8:
        return "red", "!!"
    if priority >= 5:
        return "yellow", "!"
    return "blue", "~"


def _format_duration(started: datetime, ended: datetime) -> str:
    """Format the duration between two datetimes as a human-readable string."""
    delta = ended - started
    total_minutes = int(delta.total_seconds() // 60)
    if total_minutes < 1:
        return _t("< 1 min")
    hours, minutes = divmod(total_minutes, 60)
    days, hours = divmod(hours, 24)
    parts = []
    if days > 0:
        parts.append(_t("{days}d").format(days=days))
    if hours > 0:
        parts.append(_t("{hours}h").format(hours=hours))
    if minutes > 0:
        parts.append(_t("{minutes}m").format(minutes=minutes))
    return " ".join(parts)


def _activity_status_style(status: str) -> tuple[str, str, str, bool]:
    """
    Return (prefix_icon, status_tag, style, strike) for an activity status.

    Each removal command records a distinct status so the three outcomes
    (completed / cancelled / dropped) render visibly differently in history:
    - completed: plain (priority-driven styling)
    - cancelled: dim/strikethrough 'cancelled' (added by mistake)
    - dropped:   yellow 'dropped' (gave up)
    """
    if status == "cancelled":
        return "🚫", _t("cancelled"), "dim strike", True
    if status == "dropped":
        return "🏳️", _t("dropped"), "yellow", False
    return "", "", "", False


def history_command(args) -> int:
    """
    Handle the 'history' command - display recent completed activities.

    Shows the most recent completed tasks grouped by calendar day,
    with visual separators between days.

    Args:
        args: Namespace with 'count' (int, default 5)

    Returns:
        0 on success, 1 on error
    """
    count = getattr(args, "count", None) or 5

    console = Console()

    try:
        log_entries = load_task_log()
    except Exception as e:
        console.print(_t("[bold red]Error loading task log:[/bold red] {e}").format(e=e))
        return 1

    if not log_entries:
        console.print("[bold yellow]" + _t("No activity history found") + "[/bold yellow]")
        return 0

    activities = _pair_task_activities(log_entries)

    if not activities:
        console.print(
            "[bold yellow]"
            + _t("No completed activities found. Complete some tasks to build history!")
            + "[/bold yellow]"
        )
        return 0

    recent = activities[-count:]
    recent.reverse()

    by_day: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for activity in recent:
        day_key = activity["ended_at"].strftime("%Y-%m-%d")
        by_day[day_key].append(activity)

    sorted_days = sorted(by_day.keys(), reverse=True)

    console.print()
    console.print(
        Text(
            _t("Recent Activity ({count})").format(count=len(recent)),
            style="bold cyan",
        ),
        justify="center",
    )
    console.print()

    for day_idx, day_key in enumerate(sorted_days):
        day_activities = by_day[day_key]

        if day_idx > 0:
            console.print()
            console.rule(style="dim")
            console.print()

        try:
            day_dt = datetime.strptime(day_key, "%Y-%m-%d")
            weekday_name = day_dt.strftime("%A")
            day_header = f"{day_key}  {weekday_name}"
        except ValueError:
            day_header = day_key

        console.print(
            Text(day_header, style="bold underline bright_white"),
        )
        console.print()

        for activity in day_activities:
            description = activity["description"]
            priority = activity["priority"]
            started = activity["started_at"]
            ended = activity["ended_at"]
            duration = _format_duration(started, ended)
            status = activity.get("status", "completed")

            color, icon = _priority_style(priority)
            status_icon, status_tag, status_style, strike = _activity_status_style(status)

            name_text = Text()
            name_prefix = f"  {status_icon} {description}" if status_icon else f"  {description}"
            name_text.append(name_prefix, style="bold")
            if status_tag:
                name_text.append(f"  ({status_tag})", style=status_style)

            priority_text = Text()
            priority_text.append(f"[{icon} {priority}]", style=f"bold {color}")

            time_text = Text()
            time_text.append(
                f"    {started.strftime('%H:%M')}", style="green"
            )
            time_text.append(" → ", style="dim")
            time_text.append(f"{ended.strftime('%H:%M')}", style="red")
            time_text.append(f"  ({duration})", style="dim italic")

            console.print(name_text)
            console.print(priority_text)
            console.print(time_text)
            console.print()

    return 0
