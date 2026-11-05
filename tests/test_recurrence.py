"""Recurring deadlines: repeat rules, roll-forward, CLI, browser API, and validation."""

from __future__ import annotations

import json
from datetime import date

import pytest

from schedule_management.recurrence import next_occurrence, occurrences, roll_forward


def test_repeat_rules_and_month_end_clamping():
    assert next_occurrence(date(2026, 11, 4), "weekly") == date(2026, 11, 11)
    assert next_occurrence(date(2026, 11, 4), "biweekly") == date(2026, 11, 18)
    assert next_occurrence(date(2026, 1, 31), "monthly", 31) == date(2026, 2, 28)
    # The 31st comes back after a shorter month.
    assert next_occurrence(date(2026, 2, 28), "monthly", 31) == date(2026, 3, 31)
    assert next_occurrence(date(2026, 12, 15), "monthly") == date(2027, 1, 15)
    with pytest.raises(ValueError):
        next_occurrence(date(2026, 1, 1), "daily")


def test_occurrences_and_roll_forward():
    assert list(occurrences(date(2026, 11, 20), "biweekly", date(2026, 12, 31))) == [
        date(2026, 11, 20), date(2026, 12, 4), date(2026, 12, 18),
    ]
    assert list(occurrences(date(2026, 10, 31), "monthly", date(2027, 3, 1), 31))[:4] == [
        date(2026, 10, 31), date(2026, 11, 30), date(2026, 12, 31), date(2027, 1, 31),
    ]
    assert roll_forward(date(2026, 10, 1), "weekly", date(2026, 11, 3)) == date(2026, 11, 5)
    assert roll_forward(date(2026, 11, 10), "weekly", date(2026, 11, 3)) == date(2026, 11, 10)


def test_refresh_rolls_recurring_deadlines_and_prunes_one_offs():
    from schedule_management.commands.deadlines import prune_expired_deadlines, refresh_deadlines

    today = date(2026, 11, 4)
    items = [
        {"event": "Standup notes", "deadline": "2026-10-01", "repeat": "weekly"},
        {"event": "Rent", "deadline": "2026-09-30", "repeat": "monthly", "repeat_day": 31},
        {"event": "Old report", "deadline": "2026-10-01"},
        {"event": "Yesterday", "deadline": "2026-11-03", "repeat": "weekly"},
        {"event": "Unknown rule", "deadline": "2026-10-01", "repeat": "hourly"},
    ]
    kept, removed, changed = refresh_deadlines(items, today)
    by_event = {item["event"]: item for item in kept}
    assert changed is True
    assert by_event["Standup notes"]["deadline"] == "2026-11-05"
    assert by_event["Rent"] == {"event": "Rent", "deadline": "2026-11-30", "repeat": "monthly", "repeat_day": 31}
    # One day overdue stays visible, like a one-off deadline.
    assert by_event["Yesterday"]["deadline"] == "2026-11-03"
    assert [item["event"] for item in removed] == ["Old report", "Unknown rule"]
    assert items[0]["deadline"] == "2026-10-01", "input is not mutated"

    assert refresh_deadlines([{"event": "Next week", "deadline": "2026-11-11", "repeat": "weekly"}], today)[2] is False
    assert [item["event"] for item in prune_expired_deadlines(items, today)[1]] == ["Old report", "Unknown rule"]


@pytest.fixture
def deadline_file(tmp_path, monkeypatch):
    from schedule_management.web import services

    path = tmp_path / "ddl.json"
    path.write_text("[]", encoding="utf-8")
    monkeypatch.setattr("schedule_management.data.loaders.DDL_PATH", str(path))
    monkeypatch.setattr(services, "_load_task_types", lambda: {"1": "Work"})
    monkeypatch.setattr(services, "_today", lambda: date(2026, 11, 4))
    return path


def test_browser_api_sets_keeps_and_clears_repeat(deadline_file):
    from schedule_management.web import services

    created = services.deadline_add({"event": "Rent", "date": "2026-11-30", "repeat": "monthly", "color": "red"})
    assert (created["repeat"], created["repeat_day"]) == ("monthly", 30)

    # Editing without changing the date keeps the chosen day of the month.
    stored = json.loads(deadline_file.read_text())
    stored[0]["deadline"], stored[0]["repeat_day"] = "2027-02-28", 31
    deadline_file.write_text(json.dumps(stored))
    renamed = services.deadline_update({"originalEvent": "Rent", "event": "Rent due", "date": "2027-02-28"})
    assert (renamed["repeat"], renamed["repeat_day"], renamed["color"]) == ("monthly", 31, "red")

    # Choosing a new date (for example by dragging in the calendar) moves the day.
    moved = services.deadline_update({"originalEvent": "Rent due", "event": "Rent due", "date": "2027-03-05"})
    assert moved["repeat_day"] == 5

    weekly = services.deadline_update({"originalEvent": "Rent due", "event": "Rent due", "date": "2027-03-05", "repeat": "weekly"})
    assert weekly["repeat"] == "weekly" and "repeat_day" not in weekly
    cleared = services.deadline_update({"originalEvent": "Rent due", "event": "Rent due", "date": "2027-03-05", "repeat": ""})
    assert "repeat" not in cleared

    before = deadline_file.read_text()
    with pytest.raises(services.WebError, match="unknown repeat rule"):
        services.deadline_add({"event": "Bad", "date": "2026-12-01", "repeat": "daily"})
    assert deadline_file.read_text() == before


def test_deadline_rows_report_repeat_and_roll_forward(deadline_file):
    from schedule_management.web import services

    deadline_file.write_text(json.dumps([
        {"event": "Team report", "deadline": "2026-10-23", "repeat": "biweekly"},
        {"event": "Rent", "deadline": "2026-11-30", "repeat": "monthly", "repeat_day": 31},
        {"event": "One-off", "deadline": "2026-11-20"},
    ]))
    rows = {row["event"]: row for row in services._deadline_rows(date(2026, 11, 4))}
    assert (rows["Team report"]["deadline"], rows["Team report"]["repeat"], rows["Team report"]["repeatDay"]) == ("2026-11-06", "biweekly", None)
    assert (rows["Rent"]["repeat"], rows["Rent"]["repeatDay"]) == ("monthly", 31)
    assert rows["One-off"]["repeat"] is None
    assert json.loads(deadline_file.read_text())[0]["deadline"] == "2026-11-06", "rolled deadlines are saved"


def test_cli_ddl_add_repeat(deadline_file, monkeypatch, capsys):
    from schedule_management.cli import create_parser

    args = create_parser().parse_args(["ddl", "add", "rent", "1.31", "--repeat", "monthly"])
    assert args.func(args) == 0
    stored = json.loads(deadline_file.read_text())[0]
    assert (stored["event"], stored["repeat"], stored["repeat_day"]) == ("rent", "monthly", 31)

    show = create_parser().parse_args(["ddl"])
    assert show.func(show) == 0
    assert "↻ Monthly" in capsys.readouterr().out

    with pytest.raises(SystemExit):
        create_parser().parse_args(["ddl", "add", "x", "1.1", "--repeat", "daily"])


def test_config_check_rejects_unknown_repeat_rules():
    from schedule_management.config_check import ConfigCheckError, validate_content

    validate_content("deadlines", json.dumps([{"event": "Rent", "deadline": "2026-11-30", "repeat": "monthly"}]))
    with pytest.raises(ConfigCheckError, match="repeat must be"):
        validate_content("deadlines", json.dumps([{"event": "Rent", "deadline": "2026-11-30", "repeat": "daily"}]))
