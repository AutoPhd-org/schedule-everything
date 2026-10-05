from __future__ import annotations

import json
from datetime import date, datetime

import pytest

from conftest import TEST_CONFIG_DIR


def test_status_snapshot_returns_daily_panels(monkeypatch):
    from schedule_management.config import ScheduleConfig
    from schedule_management.web.services import status_snapshot

    monkeypatch.setattr(ScheduleConfig, "should_skip_today", lambda self: False)

    snapshot = status_snapshot({})

    json.dumps(snapshot)

    assert snapshot["config"]["rootDir"] == str(TEST_CONFIG_DIR)
    assert snapshot["config"]["activeId"] == 0
    assert snapshot["today"]["date"] == date.today().isoformat()
    assert snapshot["today"]["weekday"] == datetime.now().strftime("%A").lower()
    assert snapshot["schedule"]["isSkipped"] is False
    assert isinstance(snapshot["schedule"]["events"], list)
    assert isinstance(snapshot["tasks"], list)
    assert isinstance(snapshot["deadlines"], list)
    assert isinstance(snapshot["habits"], list)
    assert isinstance(snapshot["taskTypes"], dict)
    assert isinstance(snapshot["history"], list)
    if snapshot["schedule"]["events"]:
        event = snapshot["schedule"]["events"][0]
        assert {"time", "label"}.issubset(event)
    if snapshot["tasks"]:
        task = snapshot["tasks"][0]
        assert {"description", "priority", "type", "typeName"}.issubset(task)


def test_task_add_updates_existing_task_and_logs(monkeypatch, tmp_path):
    from schedule_management.web.services import task_add

    tasks_path = tmp_path / "tasks.json"
    task_log_path = tmp_path / "tasks.log"
    tasks_path.write_text(
        json.dumps([{"description": "Draft proposal", "priority": 5}]),
        encoding="utf-8",
    )

    import schedule_management.data.loaders as loaders

    monkeypatch.setattr(loaders, "TASKS_PATH", str(tasks_path))
    monkeypatch.setattr(loaders, "TASK_LOG_PATH", str(task_log_path))

    result = task_add({"description": "Draft proposal", "priority": 9})

    assert result["description"] == "Draft proposal"
    assert result["priority"] == 9
    assert json.loads(tasks_path.read_text(encoding="utf-8")) == [
        {"description": "Draft proposal", "priority": 9, "type": "1"}
    ]
    log = json.loads(task_log_path.read_text(encoding="utf-8"))
    assert log[-1]["action"] == "updated"


def test_task_delete_removes_by_description(tmp_path, monkeypatch):
    from schedule_management.web.services import task_delete

    tasks_path = tmp_path / "tasks.json"
    task_log_path = tmp_path / "tasks.log"
    tasks_path.write_text(
        json.dumps(
            [
                {"description": "Draft proposal", "priority": 9},
                {"description": "Read notes", "priority": 4},
            ]
        ),
        encoding="utf-8",
    )

    import schedule_management.data.loaders as loaders

    monkeypatch.setattr(loaders, "TASKS_PATH", str(tasks_path))
    monkeypatch.setattr(loaders, "TASK_LOG_PATH", str(task_log_path))

    result = task_delete({"description": "Read notes"})

    assert result["deleted"] == 1
    assert json.loads(tasks_path.read_text(encoding="utf-8")) == [
        {"description": "Draft proposal", "priority": 9}
    ]


def test_deadline_add_accepts_iso_date(tmp_path, monkeypatch):
    from schedule_management.web.services import deadline_add

    deadlines_path = tmp_path / "ddl.json"
    deadlines_path.write_text("[]", encoding="utf-8")

    import schedule_management.data.loaders as loaders

    monkeypatch.setattr(loaders, "DDL_PATH", str(deadlines_path))

    result = deadline_add({"event": "Submit paper", "date": "2026-05-10"})

    assert result["event"] == "Submit paper"
    assert result["deadline"] == "2026-05-10"
    assert json.loads(deadlines_path.read_text(encoding="utf-8"))[0]["event"] == "Submit paper"


def test_deadline_delete_reports_missing_event(tmp_path, monkeypatch):
    from schedule_management.web.services import WebError, deadline_delete

    deadlines_path = tmp_path / "ddl.json"
    deadlines_path.write_text("[]", encoding="utf-8")

    import schedule_management.data.loaders as loaders

    monkeypatch.setattr(loaders, "DDL_PATH", str(deadlines_path))

    with pytest.raises(WebError) as exc_info:
        deadline_delete({"event": "Missing"})

    assert exc_info.value.code == "not_found"


def test_habit_mark_writes_today_record(monkeypatch, tmp_path):
    from schedule_management.web.services import habit_mark

    record_path = tmp_path / "record.json"
    habit_path = tmp_path / "habits.toml"
    habit_path.write_text("[habits]\n1 = \"Read\"\n2 = \"Exercise\"\n", encoding="utf-8")

    import schedule_management.data.loaders as loaders

    monkeypatch.setattr(loaders, "HABIT_PATH", str(habit_path))
    monkeypatch.setattr(loaders, "RECORD_PATH", str(record_path))
    monkeypatch.setattr(
        "schedule_management.web.services._today",
        lambda: date(2026, 4, 28),
    )

    result = habit_mark({"habitIds": ["1"]})

    assert result["date"] == "2026-04-28"
    assert result["completed"] == {"1": "Read"}
    records = json.loads(record_path.read_text(encoding="utf-8"))
    assert records[0]["completed"] == {"1": "Read"}


def test_task_update_rejects_duplicate_description(tmp_path, monkeypatch):
    from schedule_management.web.services import WebError, task_update

    tasks_path = tmp_path / "tasks.json"
    tasks_path.write_text(
        json.dumps(
            [
                {"description": "Draft proposal", "priority": 9},
                {"description": "Read notes", "priority": 4},
            ]
        ),
        encoding="utf-8",
    )

    import schedule_management.data.loaders as loaders

    monkeypatch.setattr(loaders, "TASKS_PATH", str(tasks_path))

    with pytest.raises(WebError) as exc_info:
        task_update(
            {
                "originalDescription": "Read notes",
                "description": "Draft proposal",
                "priority": 6,
            }
        )

    assert exc_info.value.code == "duplicate"
    assert json.loads(tasks_path.read_text(encoding="utf-8")) == [
        {"description": "Draft proposal", "priority": 9},
        {"description": "Read notes", "priority": 4},
    ]


def test_deadline_update_rejects_duplicate_event(tmp_path, monkeypatch):
    from schedule_management.web.services import WebError, deadline_update

    deadlines_path = tmp_path / "ddl.json"
    deadlines_path.write_text(
        json.dumps(
            [
                {"event": "Submit paper", "deadline": "2026-05-10"},
                {"event": "Pay invoice", "deadline": "2026-05-11"},
            ]
        ),
        encoding="utf-8",
    )

    import schedule_management.data.loaders as loaders

    monkeypatch.setattr(loaders, "DDL_PATH", str(deadlines_path))

    with pytest.raises(WebError) as exc_info:
        deadline_update(
            {
                "originalEvent": "Pay invoice",
                "event": "Submit paper",
                "date": "2026-05-12",
            }
        )

    assert exc_info.value.code == "duplicate"
    assert json.loads(deadlines_path.read_text(encoding="utf-8")) == [
        {"event": "Submit paper", "deadline": "2026-05-10"},
        {"event": "Pay invoice", "deadline": "2026-05-11"},
    ]


def test_task_add_wraps_save_failure(monkeypatch):
    from schedule_management.web import services

    monkeypatch.setattr(services, "load_tasks", lambda: [])

    def fail_save(_tasks):
        raise OSError("disk full")

    monkeypatch.setattr(services, "save_tasks", fail_save)

    with pytest.raises(services.WebError) as exc_info:
        services.task_add({"description": "Draft proposal", "priority": 9})

    assert exc_info.value.code == "storage_error"
    assert "failed to save tasks" in exc_info.value.message
