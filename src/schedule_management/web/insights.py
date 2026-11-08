"""
Weekly and daily insight statistics for the browser workspace.

The weekly overview (Activity page) summarizes one Monday-Sunday week of task
outcomes and habit records, and compares it with the previous week and a short
baseline of earlier weeks. The daily overview (Overview page) does the same for
one day: what was finished and when, against yesterday and the usual day.
The browser draws both without generating report files.
"""

from __future__ import annotations

from collections import Counter
from datetime import date, timedelta
from statistics import mean, median
from typing import Any

from schedule_management.commands.history import _pair_task_activities, parse_log_timestamp
from schedule_management.data import load_habit_records, load_habits, load_task_log
from schedule_management.web import services

TREND_WEEKS = 8
BASELINE_WEEKS = 4
HIGHLIGHT_COUNT = 3
# The usual day is the average of this many days before the one shown.
USUAL_DAYS = 28
_NUMERIC_KEYS = ("completed", "cancelled", "dropped", "added", "habitRate", "avgPriority", "medianLeadHours")


def _week_start(day: date) -> date:
    return day - timedelta(days=day.weekday())


def _week_days(start: date) -> list[date]:
    return [start + timedelta(days=offset) for offset in range(7)]


def _as_int(value: Any) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return 0


def _habit_days(habits: dict[str, str], records: list[dict[str, Any]]) -> dict[str, set[str]]:
    """Map ISO dates to the configured habit IDs completed on that date."""
    days: dict[str, set[str]] = {}
    for record in records:
        day = record.get("date")
        completed = record.get("completed", {})
        if isinstance(day, str) and isinstance(completed, dict):
            days[day] = {str(key) for key in completed if str(key) in habits}
    return days


class _History:
    """Indexed task and habit history shared by every computed week."""

    def __init__(self, today: date) -> None:
        self.today = today
        self.task_types = services._load_task_types()
        self.default_type = services._default_task_type(self.task_types)
        log_entries = load_task_log()
        self.activities = [
            {**activity, "priority": _as_int(activity.get("priority")), "type": str(activity.get("type") or self.default_type)}
            for activity in _pair_task_activities(log_entries)
        ]
        self.added_days = [
            parsed.date()
            for entry in log_entries
            if isinstance(entry, dict) and entry.get("action") == "added"
            and (parsed := parse_log_timestamp(entry.get("timestamp"))) is not None
        ]
        self.habits = load_habits()
        self.habit_days = _habit_days(self.habits, load_habit_records())
        known_days = [a["ended_at"].date() for a in self.activities] + self.added_days
        known_days += [date.fromisoformat(day) for day in self.habit_days if _is_iso_date(day)]
        self.first_day = min(known_days) if known_days else None

    def in_range(self, start: date, end: date) -> list[dict[str, Any]]:
        return [a for a in self.activities if start <= a["ended_at"].date() <= end]

    def habit_count(self, day: date) -> int:
        return len(self.habit_days.get(day.isoformat(), ()))

    def is_active(self, start: date) -> bool:
        """Whether a week overlaps the period the user has recorded anything."""
        return self.first_day is not None and start + timedelta(days=6) >= self.first_day and start <= self.today

    def week_stats(self, start: date) -> dict[str, Any]:
        end = start + timedelta(days=6)
        activities = self.in_range(start, end)
        completed = [a for a in activities if a["status"] == "completed"]
        elapsed = [day for day in _week_days(start) if day <= self.today]
        slots = len(self.habits) * len(elapsed)
        habit_done = sum(self.habit_count(day) for day in elapsed)
        leads = [max(0.0, (a["ended_at"] - a["started_at"]).total_seconds() / 3600) for a in completed]
        return {
            "weekStart": start.isoformat(),
            "active": self.is_active(start),
            "completed": len(completed),
            "cancelled": sum(a["status"] == "cancelled" for a in activities),
            "dropped": sum(a["status"] == "dropped" for a in activities),
            "added": sum(start <= day <= end for day in self.added_days),
            "habitRate": round(habit_done / slots, 4) if slots else None,
            "avgPriority": round(mean(a["priority"] for a in completed), 2) if completed else None,
            "medianLeadHours": round(median(leads), 2) if leads else None,
            "byType": dict(Counter(a["type"] for a in completed)),
        }

    def day_rows(self, start: date) -> list[dict[str, Any]]:
        rows: list[dict[str, Any]] = []
        for day in _week_days(start):
            completed = [a for a in self.in_range(day, day) if a["status"] == "completed"]
            rows.append(
                {
                    "date": day.isoformat(),
                    "completed": len(completed),
                    "byType": dict(Counter(a["type"] for a in completed)),
                    "habitsDone": self.habit_count(day),
                    "habitsTotal": len(self.habits),
                    "future": day > self.today,
                }
            )
        return rows

    def habit_rows(self, start: date) -> list[dict[str, Any]]:
        days = _week_days(start)
        previous_days = [day for day in _week_days(start - timedelta(days=7)) if day <= self.today]
        rows: list[dict[str, Any]] = []
        for habit_id, description in sorted(self.habits.items()):
            marks = [None if day > self.today else habit_id in self.habit_days.get(day.isoformat(), ()) for day in days]
            elapsed = [mark for mark in marks if mark is not None]
            previous = [habit_id in self.habit_days.get(day.isoformat(), ()) for day in previous_days]
            rows.append(
                {
                    "id": habit_id,
                    "description": description,
                    "days": marks,
                    "rate": round(sum(elapsed) / len(elapsed), 4) if elapsed else None,
                    "previousRate": round(sum(previous) / len(previous), 4) if previous else None,
                }
            )
        return rows

    def streak(self, end: date) -> int:
        """Consecutive days with a completed task, ending at the week's last elapsed day."""
        completed_days = {a["ended_at"].date() for a in self.activities if a["status"] == "completed"}
        anchor = min(end, self.today)
        if anchor == self.today and anchor not in completed_days:
            anchor -= timedelta(days=1)
        count = 0
        while anchor in completed_days:
            count += 1
            anchor -= timedelta(days=1)
        return count


def _is_iso_date(value: str) -> bool:
    try:
        date.fromisoformat(value)
    except ValueError:
        return False
    return True


def _baseline(weeks: list[dict[str, Any]]) -> dict[str, Any]:
    """Average earlier active weeks; values are None before any history exists."""
    active = [week for week in weeks if week["active"]]
    result: dict[str, Any] = {"weeks": len(active)}
    for key in _NUMERIC_KEYS:
        values = [week[key] for week in active if week[key] is not None]
        result[key] = round(mean(values), 2) if values else None
    type_ids = sorted({type_id for week in active for type_id in week["byType"]})
    result["byType"] = {
        type_id: round(sum(week["byType"].get(type_id, 0) for week in active) / len(active), 2)
        for type_id in type_ids
    }
    return result


def _target_date(payload: dict[str, Any], today: date) -> date:
    raw_date = payload.get("date")
    if raw_date in (None, ""):
        return today
    try:
        return date.fromisoformat(str(raw_date))
    except ValueError:
        raise services.WebError("invalid_input", "date must be YYYY-MM-DD.") from None


def _highlight_rows(completed: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """The most important completions, latest first among equals."""
    best = sorted(completed, key=lambda a: (-a["priority"], -a["ended_at"].timestamp()))[:HIGHLIGHT_COUNT]
    return [
        {"description": a["description"], "priority": a["priority"], "type": a["type"], "endedAt": a["ended_at"].isoformat()}
        for a in best
    ]


def daily_overview(payload: dict[str, Any]) -> dict[str, Any]:
    """Return statistics for the day ``payload['date']`` (default today).

    ``hours`` holds the day's completions per hour, by task type. ``yesterday``
    and ``usual`` (the average of the 28 days before, counting only days since
    the history began) are None while there is no history to compare with.
    """
    today = services._today()
    target = _target_date(payload, today)
    history = _History(today)
    activities = history.in_range(target, target)
    completed = [a for a in activities if a["status"] == "completed"]
    hours: list[Counter[str]] = [Counter() for _ in range(24)]
    for activity in completed:
        hours[activity["ended_at"].hour][activity["type"]] += 1
    done_days = Counter(a["ended_at"].date() for a in history.activities if a["status"] == "completed")
    first = history.first_day
    earlier = [target - timedelta(days=offset) for offset in range(1, USUAL_DAYS + 1)]
    counted = [day for day in earlier if first is not None and day >= first]
    yesterday = target - timedelta(days=1)
    leads = [max(0.0, (a["ended_at"] - a["started_at"]).total_seconds() / 3600) for a in completed]
    return {
        "date": target.isoformat(),
        "today": today.isoformat(),
        "isToday": target == today,
        "firstActivity": first.isoformat() if first else None,
        "taskTypes": history.task_types,
        "completed": len(completed),
        "cancelled": sum(a["status"] == "cancelled" for a in activities),
        "dropped": sum(a["status"] == "dropped" for a in activities),
        "added": sum(day == target for day in history.added_days),
        "byType": dict(Counter(a["type"] for a in completed)),
        "hours": [dict(counts) for counts in hours],
        "yesterday": done_days[yesterday] if first is not None and yesterday >= first else None,
        "usual": round(mean(done_days[day] for day in counted), 2) if counted else None,
        "usualDays": len(counted),
        "habitsDone": history.habit_count(target),
        "habitsTotal": len(history.habits),
        "streak": history.streak(target),
        "medianLeadHours": round(median(leads), 2) if leads else None,
        "highlights": _highlight_rows(completed),
    }


def weekly_overview(payload: dict[str, Any]) -> dict[str, Any]:
    """Return statistics for the week containing ``payload['date']`` (default today)."""
    today = services._today()
    target = _target_date(payload, today)
    history = _History(today)
    start = _week_start(target)
    end = start + timedelta(days=6)
    trend = [history.week_stats(start - timedelta(weeks=offset)) for offset in reversed(range(TREND_WEEKS))]
    current, previous = trend[-1], trend[-2]
    completed = [a for a in history.in_range(start, end) if a["status"] == "completed"]

    return {
        "weekStart": start.isoformat(),
        "weekEnd": end.isoformat(),
        "today": today.isoformat(),
        "isCurrentWeek": start == _week_start(today),
        "firstActivity": history.first_day.isoformat() if history.first_day else None,
        "taskTypes": history.task_types,
        "current": current,
        "previous": previous,
        "baseline": _baseline(trend[-1 - BASELINE_WEEKS:-1]),
        "trend": trend,
        "days": history.day_rows(start),
        "previousDays": history.day_rows(start - timedelta(days=7)),
        "habits": history.habit_rows(start),
        "streak": history.streak(end),
        "highlights": _highlight_rows(completed),
    }

