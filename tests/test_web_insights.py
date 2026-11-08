"""Deterministic tests for the browser weekly overview statistics."""

from __future__ import annotations

import json
from datetime import date, datetime, timezone

import pytest


def _log(action: str, when: str, description: str, priority: int = 5, task_type: str | None = "1") -> dict:
    task = {"description": description, "priority": priority}
    if task_type is not None:
        task["type"] = task_type
    return {"action": action, "timestamp": when, "task": task}


@pytest.fixture
def overview(monkeypatch):
    from schedule_management.web import insights, services

    # Wednesday of the week starting Monday 2026-10-26.
    monkeypatch.setattr(services, "_today", lambda: date(2026, 10, 28))
    monkeypatch.setattr(services, "_load_task_types", lambda: {"1": "Work", "2": "Study"})
    monkeypatch.setattr(insights, "load_habits", lambda: {"1": "Exercise", "2": "Read"})
    monkeypatch.setattr(insights, "load_habit_records", lambda: [
        {"date": "2026-10-26", "completed": {"1": "Exercise", "2": "Read"}},
        {"date": "2026-10-27", "completed": {"1": "Exercise", "retired": "Old habit"}},
        {"date": "2026-10-20", "completed": {"2": "Read"}},
    ])
    monkeypatch.setattr(insights, "load_task_log", lambda: [
        # Two weeks ago: one completion.
        _log("added", "2026-10-12T08:00:00", "Old report", 4),
        _log("deleted", "2026-10-13T08:00:00", "Old report", 4),
        # Last week: one completion and one drop.
        _log("added", "2026-10-19T09:00:00", "Plan", 6, "2"),
        _log("deleted", "2026-10-21T09:00:00", "Plan", 6, "2"),
        _log("added", "2026-10-19T09:00:00", "Idea", 3),
        _log("dropped", "2026-10-22T09:00:00", "Idea", 3),
        # This week: three completions (one legacy without a type) and a cancel.
        _log("added", "2026-10-25T10:00:00", "Draft", 9),
        _log("deleted", "2026-10-26T10:00:00", "Draft", 9),
        _log("added", "2026-10-26T12:00:00", "Notes", 5, "2"),
        _log("deleted", "2026-10-27T12:00:00", "Notes", 5, "2"),
        _log("added", "2026-10-27T08:00:00", "Legacy", 7, None),
        _log("deleted", "2026-10-27T10:00:00", "Legacy", 7, None),
        _log("added", "2026-10-27T08:00:00", "Mistake", 2),
        _log("cancelled", "2026-10-27T08:30:00", "Mistake", 2),
    ])
    return insights


def test_weekly_overview_summarizes_current_week(overview):
    data = overview.weekly_overview({})
    json.dumps(data)

    assert (data["weekStart"], data["weekEnd"], data["isCurrentWeek"]) == ("2026-10-26", "2026-11-01", True)
    current = data["current"]
    assert (current["completed"], current["cancelled"], current["dropped"]) == (3, 1, 0)
    assert current["added"] == 3  # Draft was added the previous Sunday.
    assert current["byType"] == {"1": 2, "2": 1}  # The untyped legacy task uses the default type.
    assert current["avgPriority"] == 7.0
    assert current["medianLeadHours"] == 24.0
    # Monday to Wednesday elapsed: 3 completed habit slots out of 2 habits x 3 days.
    assert current["habitRate"] == 0.5

    assert [day["completed"] for day in data["days"]] == [1, 2, 0, 0, 0, 0, 0]
    assert [day["future"] for day in data["days"]] == [False, False, False, True, True, True, True]
    assert data["days"][1]["byType"] == {"1": 1, "2": 1}
    assert [day["completed"] for day in data["previousDays"]] == [0, 0, 1, 0, 0, 0, 0]

    exercise = data["habits"][0]
    assert exercise["days"][:4] == [True, True, False, None]
    assert exercise["rate"] == round(2 / 3, 4)
    assert exercise["previousRate"] == 0.0
    assert data["habits"][1]["previousRate"] == round(1 / 7, 4)

    assert [item["description"] for item in data["highlights"]] == ["Draft", "Legacy", "Notes"]
    assert data["streak"] == 2  # Today has no completion yet, so the streak ends yesterday.


def test_weekly_overview_compares_with_history(overview):
    data = overview.weekly_overview({})

    assert len(data["trend"]) == overview.TREND_WEEKS
    assert [week["completed"] for week in data["trend"][-3:]] == [1, 1, 3]
    assert data["previous"]["dropped"] == 1
    assert data["firstActivity"] == "2026-10-12"
    # Only the two weeks since history began count toward the baseline.
    assert data["baseline"]["weeks"] == 2
    assert data["baseline"]["completed"] == 1.0
    assert data["baseline"]["byType"] == {"1": 0.5, "2": 0.5}
    assert not data["trend"][0]["active"]


def test_weekly_overview_navigates_to_past_weeks(overview):
    data = overview.weekly_overview({"date": "2026-10-22"})

    assert data["weekStart"] == "2026-10-19"
    assert data["isCurrentWeek"] is False
    assert data["current"]["completed"] == 1
    assert all(not day["future"] for day in data["days"])
    assert data["streak"] == 0


def test_weekly_overview_rejects_invalid_dates(overview):
    from schedule_management.web.services import WebError

    with pytest.raises(WebError) as exc_info:
        overview.weekly_overview({"date": "next week"})
    assert exc_info.value.code == "invalid_input"


def test_weekly_overview_without_history(monkeypatch):
    from schedule_management.web import insights, services

    monkeypatch.setattr(services, "_today", lambda: date(2026, 10, 28))
    monkeypatch.setattr(services, "_load_task_types", lambda: {"1": "Work"})
    monkeypatch.setattr(insights, "load_habits", lambda: {})
    monkeypatch.setattr(insights, "load_habit_records", lambda: [])
    monkeypatch.setattr(insights, "load_task_log", lambda: [])

    data = insights.weekly_overview({})

    assert data["current"]["completed"] == 0
    assert data["current"]["habitRate"] is None
    assert data["baseline"]["weeks"] == 0
    assert data["baseline"]["completed"] is None
    assert data["firstActivity"] is None


def test_daily_overview_summarizes_one_day(overview):
    data = overview.daily_overview({"date": "2026-10-27"})

    json.dumps(data)
    assert data["date"] == "2026-10-27"
    assert data["isToday"] is False
    assert (data["completed"], data["cancelled"], data["dropped"], data["added"]) == (2, 1, 0, 2)
    # The legacy completion without a type counts toward the default type.
    assert data["byType"] == {"2": 1, "1": 1}
    assert data["hours"][10] == {"1": 1}
    assert data["hours"][12] == {"2": 1}
    assert sum(len(hour) for hour in data["hours"]) == 2
    # Yesterday had one completion; the usual day averages the 15 days since the history began (3 completions).
    assert data["yesterday"] == 1
    assert (data["usual"], data["usualDays"]) == (0.2, 15)
    # Only configured habits count.
    assert (data["habitsDone"], data["habitsTotal"]) == (1, 2)
    assert data["streak"] == 2
    assert data["medianLeadHours"] == 13.0
    assert [item["description"] for item in data["highlights"]] == ["Legacy", "Notes"]
    assert data["highlights"][0] == {"description": "Legacy", "priority": 7, "type": "1", "endedAt": "2026-10-27T10:00:00"}


def test_daily_overview_defaults_to_today(overview):
    data = overview.daily_overview({})

    assert data["date"] == data["today"] == "2026-10-28"
    assert data["isToday"] is True
    assert data["completed"] == 0
    assert data["hours"] == [{}] * 24
    assert data["yesterday"] == 2
    assert data["medianLeadHours"] is None
    assert data["highlights"] == []
    # Nothing finished yet today, so the streak still counts up to yesterday.
    assert data["streak"] == 2
    with pytest.raises(overview.services.WebError) as exc_info:
        overview.daily_overview({"date": "tomorrow"})
    assert exc_info.value.code == "invalid_input"


def test_daily_overview_without_history(monkeypatch):
    from schedule_management.web import insights, services

    monkeypatch.setattr(services, "_today", lambda: date(2026, 10, 28))
    monkeypatch.setattr(services, "_load_task_types", lambda: {"1": "Work"})
    monkeypatch.setattr(insights, "load_habits", lambda: {})
    monkeypatch.setattr(insights, "load_habit_records", lambda: [])
    monkeypatch.setattr(insights, "load_task_log", lambda: [])

    data = insights.daily_overview({})

    assert data["completed"] == 0
    assert data["yesterday"] is None
    assert data["usual"] is None
    assert data["usualDays"] == 0
    assert data["firstActivity"] is None
    assert data["habitsTotal"] == 0


def test_history_uses_local_time_and_task_types():
    from schedule_management.commands.history import _pair_task_activities

    activities = _pair_task_activities([
        {"action": "added", "timestamp": "2026-10-05T16:00:00+00:00", "task": {"description": "Draft", "priority": 8, "type": "2"}},
        {"action": "updated", "timestamp": "2026-10-05T17:00:00+00:00", "task": {"description": "Draft", "priority": 8, "type": "3"}, "metadata": {"old_priority": 8}},
        {"action": "deleted", "timestamp": "2026-10-05T18:00:00+00:00", "task": {"description": "Draft", "priority": 8, "type": "3"}},
    ])

    expected = datetime(2026, 10, 5, 16, tzinfo=timezone.utc).astimezone().replace(tzinfo=None)
    assert activities[0]["started_at"] == expected
    assert activities[0]["type"] == "3"
