"""
Synced schedule helpers for date-scoped task assignments.

This module stores the daily plan (saved with `rmd sync --apply`, usually by
an AI agent's plan-day skill) that maps today's work blocks to task titles. The synced file acts as an overlay on top of the base
odd/even weekly schedule so task assignments can change daily without
rewriting the template schedules.
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass
from datetime import date
from pathlib import Path
from typing import Any

import tomllib

from schedule_management.config_layout import resolve_config_root_dir

SYNCABLE_BLOCKS = {"pomodoro", "potato"}


@dataclass
class SyncedDaySchedule:
    """Accepted task assignments for a single calendar day."""

    target_date: str
    parity: str
    weekday: str
    assignments: dict[str, dict[str, str]]


def resolve_synced_schedule_path() -> Path:
    """Return the persisted sync overlay path."""
    override = os.getenv("REMINDER_SYNCED_SCHEDULE_PATH")
    if override:
        return Path(override).expanduser().resolve()

    return (resolve_config_root_dir() / "synced_schedule.toml").resolve()


def _normalize_assignment(event: Any) -> dict[str, str] | None:
    """Validate a synced assignment loaded from TOML."""
    if not isinstance(event, dict):
        return None

    block = event.get("block")
    if not isinstance(block, str) or not block.strip():
        return None

    title = event.get("title", block)
    if not isinstance(title, str) or not title.strip():
        title = block

    return {
        "block": block.strip(),
        "title": title.strip(),
    }


def load_synced_schedule(path: Path | None = None) -> SyncedDaySchedule | None:
    """Load a synced overlay file if it exists and is valid."""
    target_path = path or resolve_synced_schedule_path()
    if not target_path.exists():
        return None

    try:
        with open(target_path, "rb") as handle:
            raw = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError):
        return None

    if not isinstance(raw, dict):
        return None

    target_date = str(raw.get("date", "")).strip()
    parity = str(raw.get("parity", "")).strip().lower()
    weekday = str(raw.get("weekday", "")).strip().lower()
    schedule = raw.get("schedule", {})

    if not target_date or not isinstance(schedule, dict):
        return None

    assignments: dict[str, dict[str, str]] = {}
    for time_str, event in schedule.items():
        normalized = _normalize_assignment(event)
        if normalized is None:
            continue
        assignments[str(time_str)] = normalized

    return SyncedDaySchedule(
        target_date=target_date,
        parity=parity,
        weekday=weekday,
        assignments=assignments,
    )


def synced_schedule_matches_today(
    synced: SyncedDaySchedule | None,
    *,
    target_date: date | None = None,
    parity: str | None = None,
    weekday: str | None = None,
) -> bool:
    """Return True when a synced overlay applies to the requested day."""
    if synced is None:
        return False

    actual_date = target_date or date.today()
    expected_weekday = weekday or actual_date.strftime("%A").lower()

    if synced.target_date != actual_date.isoformat():
        return False
    if synced.weekday and synced.weekday != expected_weekday:
        return False
    if parity and synced.parity and synced.parity != parity:
        return False
    return True


def apply_synced_schedule(
    schedule: dict[str, Any],
    *,
    target_date: date | None = None,
    parity: str | None = None,
    weekday: str | None = None,
    synced: SyncedDaySchedule | None = None,
) -> dict[str, Any]:
    """Overlay synced assignments onto a base daily schedule."""
    loaded = synced if synced is not None else load_synced_schedule()
    if not synced_schedule_matches_today(
        loaded,
        target_date=target_date,
        parity=parity,
        weekday=weekday,
    ):
        return dict(schedule)

    merged = dict(schedule)
    merged.update(loaded.assignments)
    return merged


def get_event_block_name(event: Any) -> str | None:
    """Return the block name for time-block events when available."""
    if isinstance(event, dict) and "block" in event:
        block = event.get("block")
        return str(block).strip() if isinstance(block, str) else None
    if isinstance(event, str):
        return event
    return None


def has_explicit_title(event: Any) -> bool:
    """Return True when the event has a meaningful custom title."""
    if not isinstance(event, dict) or "block" not in event:
        return False

    block = get_event_block_name(event)
    title = event.get("title")
    if not isinstance(block, str) or not isinstance(title, str):
        return False

    normalized_block = block.replace("_", " ").strip().casefold()
    normalized_title = title.strip().casefold()
    return bool(normalized_title) and normalized_title != normalized_block


def format_event_label(event: Any) -> str:
    """Render an event label for status tables and previews."""
    if isinstance(event, dict) and "block" in event:
        block = str(event.get("block", "")).strip()
        title = str(event.get("title", "")).strip()
        normalized_block = block.replace("_", " ").strip()
        if title and title.casefold() != normalized_block.casefold():
            return f"{normalized_block}: {title}"
        return normalized_block
    return str(event)


def iter_syncable_slots(schedule: dict[str, Any]) -> list[tuple[str, str]]:
    """List untitled pomodoro/potato slots that can be assigned by sync."""
    slots: list[tuple[str, str]] = []

    for time_str in sorted(schedule.keys()):
        event = schedule[time_str]
        block = get_event_block_name(event)
        if block not in SYNCABLE_BLOCKS:
            continue
        if has_explicit_title(event):
            continue
        slots.append((time_str, block))

    return slots


def _toml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def render_synced_schedule_file(plan: SyncedDaySchedule) -> str:
    """Serialize a synced overlay file in TOML format."""
    lines = [
        "# Accepted task assignments for today's focus blocks.",
        "# This file is written by `rmd sync --apply`.",
        f"date = {_toml_string(plan.target_date)}",
        f"parity = {_toml_string(plan.parity)}",
        f"weekday = {_toml_string(plan.weekday)}",
        "",
        "[schedule]",
    ]

    for time_str in sorted(plan.assignments.keys()):
        assignment = plan.assignments[time_str]
        lines.append(
            f"{_toml_string(time_str)} = "
            "{ "
            f"block = {_toml_string(assignment['block'])}, "
            f"title = {_toml_string(assignment['title'])} "
            "}"
        )

    return "\n".join(lines) + "\n"


def save_synced_schedule(
    plan: SyncedDaySchedule,
    path: Path | None = None,
) -> Path:
    """Persist the synced overlay file and return its path."""
    target_path = path or resolve_synced_schedule_path()
    target_path.parent.mkdir(parents=True, exist_ok=True)
    target_path.write_text(render_synced_schedule_file(plan), encoding="utf-8")
    return target_path
