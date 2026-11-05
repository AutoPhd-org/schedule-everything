"""
Repeat rules for recurring deadlines (weekly, every two weeks, monthly).

A recurring deadline keeps its next occurrence in `deadline` and its rule in
`repeat`. Monthly deadlines also keep `repeat_day`, the day of the month the
user picked, so a deadline on the 31st returns to the 31st after a shorter
month. These helpers compute occurrences and roll a passed deadline forward.
"""

from __future__ import annotations

import calendar
from collections.abc import Iterator
from datetime import date, timedelta
from typing import Any

REPEAT_RULES = ("weekly", "biweekly", "monthly")
REPEAT_LABELS = {"weekly": "Weekly", "biweekly": "Every 2 weeks", "monthly": "Monthly"}
_MAX_OCCURRENCES = 5000


def repeat_rule(item: dict[str, Any]) -> str | None:
    """Return the deadline's repeat rule, or None for one-off or invalid values."""
    rule = str(item.get("repeat") or "").strip().lower()
    return rule if rule in REPEAT_RULES else None


def anchor_day(item: dict[str, Any], current: date) -> int:
    """Day of the month a monthly deadline returns to (defaults to its current day)."""
    try:
        day = int(item.get("repeat_day") or current.day)
    except (TypeError, ValueError):
        day = current.day
    return min(max(day, 1), 31)


def _add_months(value: date, months: int, day: int) -> date:
    month_index = value.month - 1 + months
    year, month = value.year + month_index // 12, month_index % 12 + 1
    return date(year, month, min(day, calendar.monthrange(year, month)[1]))


def next_occurrence(current: date, rule: str, day: int | None = None) -> date:
    """The occurrence after `current` for a repeat rule."""
    if rule == "weekly":
        return current + timedelta(days=7)
    if rule == "biweekly":
        return current + timedelta(days=14)
    if rule == "monthly":
        return _add_months(current, 1, day or current.day)
    raise ValueError(f"unknown repeat rule: {rule}")


def occurrences(start: date, rule: str, until: date, day: int | None = None) -> Iterator[date]:
    """Occurrences from `start` (inclusive) through `until` (inclusive)."""
    current = start
    for _ in range(_MAX_OCCURRENCES):
        if current > until:
            return
        yield current
        current = next_occurrence(current, rule, day)


def roll_forward(current: date, rule: str, earliest: date, day: int | None = None) -> date:
    """Advance `current` by whole repeats until it is on or after `earliest`."""
    for _ in range(_MAX_OCCURRENCES):
        if current >= earliest:
            return current
        current = next_occurrence(current, rule, day)
    return current
