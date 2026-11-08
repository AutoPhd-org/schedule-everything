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


def _task_storage(tmp_path, monkeypatch, tasks, procrastinated=None):
    import schedule_management.data.loaders as loaders

    paths = {
        "tasks": tmp_path / "tasks.json",
        "log": tmp_path / "tasks.log",
        "procrastinate": tmp_path / "procrastinate.json",
    }
    paths["tasks"].write_text(json.dumps(tasks), encoding="utf-8")
    paths["procrastinate"].write_text(json.dumps(procrastinated or []), encoding="utf-8")
    monkeypatch.setattr(loaders, "TASKS_PATH", str(paths["tasks"]))
    monkeypatch.setattr(loaders, "TASK_LOG_PATH", str(paths["log"]))
    monkeypatch.setattr(loaders, "PROCRASTINATE_PATH", str(paths["procrastinate"]))
    return {key: (lambda path=path: json.loads(path.read_text(encoding="utf-8"))) for key, path in paths.items()}


@pytest.mark.parametrize("action", ["deleted", "cancelled", "dropped"])
def test_task_restore_undoes_a_removal(tmp_path, monkeypatch, action):
    from schedule_management.commands.history import _pair_task_activities
    from schedule_management.data import log_task_action
    from schedule_management.web.services import task_delete, task_restore

    tasks = [
        {"description": "Draft proposal", "priority": 9, "type": "2"},
        {"description": "Read notes", "priority": 4, "type": "3", "alarm_from": "2026-01-02"},
        {"description": "Email Sam", "priority": 6},
    ]
    read = _task_storage(
        tmp_path,
        monkeypatch,
        tasks,
        [{"description": "Read notes", "since": "2026-01-01"}, {"description": "Email Sam", "since": "2026-01-03"}],
    )
    log_task_action("added", tasks[1])
    log_before = read["log"]()

    result = task_delete({"description": "Read notes", "action": action})
    json.dumps(result)
    assert [task["description"] for task in read["tasks"]()] == ["Draft proposal", "Email Sam"]
    assert _pair_task_activities(read["log"]())[0]["status"] == ("completed" if action == "deleted" else action)

    restored = task_restore(result["undo"])

    assert restored == {"description": "Read notes", "restored": 1}
    # Back in its old place with every field, still deferred since the same day, and gone from the history.
    assert read["tasks"]() == tasks
    assert read["procrastinate"]() == [
        {"description": "Email Sam", "since": "2026-01-03"},
        {"description": "Read notes", "since": "2026-01-01"},
    ]
    assert read["log"]() == log_before
    assert _pair_task_activities(read["log"]()) == []


def test_task_restore_leaves_deferrals_alone_for_tasks_that_were_not_deferred(tmp_path, monkeypatch):
    from schedule_management.web.services import task_delete, task_restore

    read = _task_storage(tmp_path, monkeypatch, [{"description": "Read notes", "priority": 4}])
    result = task_delete({"description": "Read notes"})
    assert result["undo"]["procrastinated"] is None

    task_restore(result["undo"])

    assert read["tasks"]() == [{"description": "Read notes", "priority": 4}]
    assert read["procrastinate"]() == []


def test_task_restore_refuses_a_task_that_is_already_back(tmp_path, monkeypatch):
    from schedule_management.web.services import WebError, task_add, task_delete, task_restore

    read = _task_storage(tmp_path, monkeypatch, [{"description": "Read notes", "priority": 4, "type": "1"}])
    result = task_delete({"description": "Read notes"})
    task_add({"description": "Read notes", "priority": 7, "type": "1"})

    with pytest.raises(WebError) as error:
        task_restore(result["undo"])

    assert error.value.code == "duplicate"
    assert read["tasks"]() == [{"description": "Read notes", "priority": 7, "type": "1"}]


@pytest.mark.parametrize(
    "payload",
    [
        {"action": "deleted", "tasks": []},
        {"action": "finished", "tasks": [{"index": 0, "task": {"description": "Read notes"}}]},
        {"action": "deleted", "tasks": [{"index": 0, "task": {"description": "Other"}}]},
        {"action": "deleted", "tasks": [{"index": -1, "task": {"description": "Read notes"}}]},
    ],
)
def test_task_restore_rejects_malformed_tokens(tmp_path, monkeypatch, payload):
    from schedule_management.web.services import WebError, task_restore

    _task_storage(tmp_path, monkeypatch, [])
    with pytest.raises(WebError) as error:
        task_restore({"description": "Read notes", **payload})
    assert error.value.code == "invalid_input"


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


def test_deadline_type_round_trip_and_legacy_updates(tmp_path, monkeypatch):
    from schedule_management.web import services

    deadlines_path = tmp_path / "ddl.json"
    deadlines_path.write_text("[]", encoding="utf-8")
    monkeypatch.setattr("schedule_management.data.loaders.DDL_PATH", str(deadlines_path))
    monkeypatch.setattr(services, "_load_task_types", lambda: {"1": "Work", "2": "Personal"})

    created = services.deadline_add({"event": "Submit paper", "date": "2026-10-27", "type": "1"})
    assert created["type"] == "1"
    assert json.loads(deadlines_path.read_text())[0]["type"] == "1"

    updated = services.deadline_update(
        {"originalEvent": "Submit paper", "event": "Submit draft", "date": "2026-10-28", "type": "2"}
    )
    assert updated["type"] == "2"
    assert updated["added"] == created["added"]

    # Existing callers can still change dates without supplying a type.
    services.deadline_update(
        {"originalEvent": "Submit draft", "event": "Submit draft", "date": "2026-10-29"}
    )
    services.deadline_add({"event": "Submit draft", "date": "2026-10-30"})
    assert json.loads(deadlines_path.read_text())[0]["type"] == "2"

    services.deadline_update(
        {"originalEvent": "Submit draft", "event": "Submit draft", "date": "2026-10-30", "type": ""}
    )
    assert "type" not in json.loads(deadlines_path.read_text())[0]


@pytest.mark.parametrize("operation", ["add", "update"])
def test_deadline_rejects_unknown_type_without_writing(tmp_path, monkeypatch, operation):
    from schedule_management.web import services

    deadlines_path = tmp_path / "ddl.json"
    original = json.dumps([{"event": "Submit paper", "deadline": "2026-10-27", "type": "1"}])
    deadlines_path.write_text(original, encoding="utf-8")
    monkeypatch.setattr("schedule_management.data.loaders.DDL_PATH", str(deadlines_path))
    monkeypatch.setattr(services, "_load_task_types", lambda: {"1": "Work"})
    handler = services.deadline_add if operation == "add" else services.deadline_update
    with pytest.raises(services.WebError, match="unknown deadline type") as exc_info:
        handler({"originalEvent": "Submit paper", "event": "Submit paper", "date": "2026-10-28", "type": "99"})
    assert exc_info.value.code == "invalid_input"
    assert deadlines_path.read_text() == original


def test_deadline_update_preserves_removed_type(tmp_path, monkeypatch):
    from schedule_management.web import services

    deadlines_path = tmp_path / "ddl.json"
    deadlines_path.write_text(
        json.dumps([{"event": "Legacy deadline", "deadline": "2026-10-27", "type": "99"}]), encoding="utf-8"
    )
    monkeypatch.setattr("schedule_management.data.loaders.DDL_PATH", str(deadlines_path))
    monkeypatch.setattr(services, "_load_task_types", lambda: {"1": "Work"})
    updated = services.deadline_update(
        {"originalEvent": "Legacy deadline", "event": "Renamed deadline", "date": "2026-10-28", "type": "99"}
    )
    assert updated["type"] == "99"


def test_deadline_snapshot_types_and_urgency_are_deterministic(monkeypatch):
    from schedule_management.web import services

    monkeypatch.setattr(services, "_load_task_types", lambda: {"1": "Work"})
    monkeypatch.setattr(services, "load_deadlines", lambda: [
        {"event": "Typed", "deadline": "2026-10-27", "type": "1"},
        {"event": "Legacy", "deadline": "2026-10-26"},
        {"event": "Removed type", "deadline": "2026-10-25", "type": "99"},
        {"event": "Bad date", "deadline": "invalid", "type": "1"},
    ])
    rows = {row["event"]: row for row in services._deadline_rows(date(2026, 10, 26))}
    assert (rows["Typed"]["type"], rows["Typed"]["typeName"], rows["Typed"]["daysLeft"], rows["Typed"]["status"]) == ("1", "Work", 1, "urgent")
    assert rows["Legacy"]["type"] is None
    assert rows["Legacy"]["typeName"] is None
    assert rows["Legacy"]["status"] == "today"
    assert (rows["Removed type"]["typeName"], rows["Removed type"]["status"]) == ("99", "overdue")
    assert (rows["Bad date"]["typeName"], rows["Bad date"]["status"]) == ("Work", "invalid")


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


def test_timed_tasks_keep_their_finish_time_and_progress(tmp_path, monkeypatch):
    from schedule_management.web.services import WebError, _sorted_tasks, task_add, task_progress, task_update

    read = _task_storage(tmp_path, monkeypatch, [{"description": "Read notes", "priority": 4, "type": "1"}])

    task_add({"description": "Ship slides", "priority": 7, "type": "1", "due": "2026-11-05T18:30"})
    assert read["tasks"]()[-1] == {"description": "Ship slides", "priority": 7, "type": "1", "due": "2026-11-05T18:30"}
    assert task_progress({"description": "Ship slides", "progress": 40}) == {"description": "Ship slides", "progress": 40}
    # Progress moves often, so it is saved without a history entry.
    assert [entry["action"] for entry in read["log"]()] == ["added"]

    rows = {row["description"]: row for row in _sorted_tasks()}
    assert rows["Ship slides"]["due"] == "2026-11-05T18:30"
    assert rows["Ship slides"]["progress"] == 40
    assert rows["Read notes"]["due"] is None
    assert rows["Read notes"]["progress"] == 0

    # Editing keeps the progress; an empty finish time makes it a normal task again.
    task_update({"originalDescription": "Ship slides", "description": "Ship slides", "priority": 7, "due": "2026-11-06T09:00"})
    assert read["tasks"]()[-1]["due"] == "2026-11-06T09:00"
    task_update({"originalDescription": "Ship slides", "description": "Ship slides", "priority": 7, "due": ""})
    assert read["tasks"]()[-1] == {"description": "Ship slides", "priority": 7, "type": "1", "progress": 40}
    # Editing without a due field leaves the timer alone.
    task_update({"originalDescription": "Read notes", "description": "Read notes", "priority": 4, "due": "2026-11-05T08:00"})
    task_update({"originalDescription": "Read notes", "description": "Read notes", "priority": 5})
    assert read["tasks"]()[0]["due"] == "2026-11-05T08:00"

    task_progress({"description": "Ship slides", "progress": "0"})
    assert "progress" not in read["tasks"]()[-1]
    task_progress({"description": "Ship slides", "progress": 100.0})
    assert read["tasks"]()[-1]["progress"] == 100

    for payload in ({"progress": -1}, {"progress": 101}, {"progress": 12.5}, {"progress": True}, {"progress": "half"}, {"progress": None}):
        with pytest.raises(WebError) as exc_info:
            task_progress({"description": "Ship slides", **payload})
        assert exc_info.value.code == "invalid_input"
    with pytest.raises(WebError) as exc_info:
        task_progress({"description": "Missing", "progress": 10})
    assert exc_info.value.code == "not_found"
    with pytest.raises(WebError) as exc_info:
        task_add({"description": "Bad time", "priority": 5, "due": "tonight"})
    assert exc_info.value.code == "invalid_input"


def test_subtasks_drive_a_tasks_progress(tmp_path, monkeypatch):
    from schedule_management.web.services import WebError, _sorted_tasks, task_delete, task_progress, task_restore, task_subtasks, task_update

    read = _task_storage(tmp_path, monkeypatch, [
        {"description": "Write paper", "priority": 8, "type": "1", "progress": 30},
        {"description": "Read notes", "priority": 4, "type": "1"},
    ])

    result = task_subtasks({"description": "Write paper", "subtasks": [
        {"text": "  Outline ", "done": True},
        {"text": "Draft", "done": False},
        {"text": "Proofread"},
    ]})
    assert result == {"description": "Write paper", "progress": 33, "subtasks": [
        {"text": "Outline", "done": True}, {"text": "Draft", "done": False}, {"text": "Proofread", "done": False},
    ]}
    # The derived progress is saved too, so everything that reads progress agrees; no history entry is written.
    saved = read["tasks"]()[0]
    assert saved["progress"] == 33
    assert saved["subtasks"][0] == {"text": "Outline", "done": True}
    assert not (tmp_path / "tasks.log").exists() or read["log"]() == []

    rows = {row["description"]: row for row in _sorted_tasks()}
    assert rows["Write paper"]["progress"] == 33
    assert [item["text"] for item in rows["Write paper"]["subtasks"]] == ["Outline", "Draft", "Proofread"]
    assert rows["Read notes"]["subtasks"] == []

    # Halves round up, as in the browser: one of eight done is 13%.
    task_subtasks({"description": "Read notes", "subtasks": [{"text": f"Page {i}", "done": i == 0} for i in range(8)]})
    assert read["tasks"]()[1]["progress"] == 13

    # While subtasks set the pace, the progress slider has no say.
    with pytest.raises(WebError) as exc_info:
        task_progress({"description": "Write paper", "progress": 90})
    assert exc_info.value.code == "invalid_input"

    # Renaming, completing, and restoring a task keep its subtasks.
    task_update({"originalDescription": "Write paper", "description": "Write the paper", "priority": 8})
    undo = task_delete({"description": "Write the paper", "action": "deleted"})["undo"]
    task_restore(undo)
    assert read["tasks"]()[0]["subtasks"][2] == {"text": "Proofread", "done": False}

    # Every subtask done is 100%; clearing them keeps the last progress as the task's own.
    task_subtasks({"description": "Write the paper", "subtasks": [{"text": "Outline", "done": True}, {"text": "Draft", "done": True}]})
    assert read["tasks"]()[0]["progress"] == 100
    assert task_subtasks({"description": "Write the paper", "subtasks": []})["progress"] == 100
    assert "subtasks" not in read["tasks"]()[0]
    assert task_progress({"description": "Write the paper", "progress": 40})["progress"] == 40

    # None done drops the saved progress, like a slider at zero.
    task_subtasks({"description": "Write the paper", "subtasks": [{"text": "Again", "done": False}]})
    assert "progress" not in read["tasks"]()[0]

    for subtasks in ("Outline", [{"text": ""}], [{"done": True}], ["Outline"], [{"text": "x" * 201}], [{"text": "Fine", "done": "yes"}], [{"text": f"{i}"} for i in range(51)]):
        with pytest.raises(WebError) as exc_info:
            task_subtasks({"description": "Write the paper", "subtasks": subtasks})
        assert exc_info.value.code == "invalid_input"
    with pytest.raises(WebError) as exc_info:
        task_subtasks({"description": "Missing", "subtasks": []})
    assert exc_info.value.code == "not_found"


def test_timed_tasks_completed_today_still_count_toward_today(tmp_path, monkeypatch):
    from datetime import UTC, timedelta

    from schedule_management.web import services

    def stamp(moment):
        return moment.astimezone(UTC).isoformat()

    today = date(2026, 11, 5)
    monkeypatch.setattr(services, "_today", lambda: today)
    read = _task_storage(tmp_path, monkeypatch, [{"description": "Open again", "priority": 5}])
    now = datetime(2026, 11, 5, 15, 0).astimezone()
    log = [
        {"timestamp": stamp(now), "action": "deleted", "task": {"description": "Ship slides", "priority": 7, "type": "2", "due": "2026-11-05T18:00"}},
        {"timestamp": stamp(now), "action": "deleted", "task": {"description": "Overdue essay", "priority": 6, "due": "2026-11-03T12:00"}},
        # Not today's: due tomorrow, finished yesterday, cancelled, not timed, or open again.
        {"timestamp": stamp(now), "action": "deleted", "task": {"description": "Tomorrow", "priority": 5, "due": "2026-11-06T09:00"}},
        {"timestamp": stamp(now - timedelta(days=1)), "action": "deleted", "task": {"description": "Yesterday", "priority": 5, "due": "2026-11-04T09:00"}},
        {"timestamp": stamp(now), "action": "cancelled", "task": {"description": "Cancelled", "priority": 5, "due": "2026-11-05T09:00"}},
        {"timestamp": stamp(now), "action": "deleted", "task": {"description": "Untimed", "priority": 5}},
        {"timestamp": stamp(now), "action": "deleted", "task": {"description": "Open again", "priority": 5, "due": "2026-11-05T10:00"}},
    ]
    (tmp_path / "tasks.log").write_text(json.dumps(log), encoding="utf-8")
    assert read["tasks"]()

    done = services._timed_done_today(today)

    assert [item["description"] for item in done] == ["Ship slides", "Overdue essay"]
    assert done[0] == {"description": "Ship slides", "priority": 7, "type": "2", "due": "2026-11-05T18:00", "endedAt": "2026-11-05T15:00:00"}
    assert done[1]["type"] is None


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


def test_deadline_colors_round_trip_and_validate(tmp_path, monkeypatch):
    from schedule_management.web import services

    deadlines_path = tmp_path / "ddl.json"
    deadlines_path.write_text("[]", encoding="utf-8")
    monkeypatch.setattr("schedule_management.data.loaders.DDL_PATH", str(deadlines_path))
    monkeypatch.setattr(services, "_load_task_types", lambda: {"1": "Work"})

    created = services.deadline_add({"event": "Launch", "date": "2026-11-20", "color": "Orange"})
    assert created["color"] == "orange"
    # Moving a deadline (as the calendar drag does) keeps its color.
    services.deadline_update({"originalEvent": "Launch", "event": "Launch", "date": "2026-11-21"})
    assert json.loads(deadlines_path.read_text())[0]["color"] == "orange"

    before = deadlines_path.read_text()
    with pytest.raises(services.WebError, match="unknown deadline color"):
        services.deadline_update({"originalEvent": "Launch", "event": "Launch", "date": "2026-11-21", "color": "url(x)"})
    assert deadlines_path.read_text() == before

    services.deadline_update({"originalEvent": "Launch", "event": "Launch", "date": "2026-11-21", "color": ""})
    assert "color" not in json.loads(deadlines_path.read_text())[0]

    monkeypatch.setattr(services, "load_deadlines", lambda: [
        {"event": "Marked", "deadline": "2026-11-21", "color": "violet"},
        {"event": "Tampered", "deadline": "2026-11-22", "color": "<b>"},
    ])
    rows = {row["event"]: row for row in services._deadline_rows(date(2026, 11, 4))}
    assert rows["Marked"]["color"] == "violet"
    assert rows["Tampered"]["color"] is None


def test_history_rows_include_task_types(monkeypatch):
    from schedule_management.web import services

    monkeypatch.setattr(services, "_load_task_types", lambda: {"1": "Work", "2": "Study"})
    monkeypatch.setattr(services, "load_task_log", lambda: [
        {"action": "added", "timestamp": "2026-10-05T09:00:00", "task": {"description": "Typed", "priority": 6, "type": "2"}},
        {"action": "deleted", "timestamp": "2026-10-05T10:00:00", "task": {"description": "Typed", "priority": 6, "type": "2"}},
        {"action": "added", "timestamp": "2026-10-05T09:00:00", "task": {"description": "Legacy", "priority": 3}},
        {"action": "dropped", "timestamp": "2026-10-05T11:00:00", "task": {"description": "Legacy", "priority": 3}},
    ])

    rows = services.task_history({"all": True})["activities"]

    assert [(row["description"], row["type"], row["typeName"], row["status"]) for row in rows] == [
        ("Legacy", "1", "Work", "dropped"),
        ("Typed", "2", "Study", "completed"),
    ]


def _task_type_store(tmp_path, monkeypatch, types, tasks=(), deadlines=(), log=(), colors=()):
    """Point the type, task, deadline, and history storage at temporary files."""
    from schedule_management.web import services
    import schedule_management.data.loaders as loaders

    store = {"types": dict(types), "colors": dict(colors)}
    paths = {name: tmp_path / name for name in ("tasks.json", "ddl.json", "tasks.log")}
    paths["tasks.json"].write_text(json.dumps(list(tasks)), encoding="utf-8")
    paths["ddl.json"].write_text(json.dumps(list(deadlines)), encoding="utf-8")
    paths["tasks.log"].write_text(json.dumps(list(log)), encoding="utf-8")
    monkeypatch.setattr(loaders, "TASKS_PATH", str(paths["tasks.json"]))
    monkeypatch.setattr(loaders, "DDL_PATH", str(paths["ddl.json"]))
    monkeypatch.setattr(loaders, "TASK_LOG_PATH", str(paths["tasks.log"]))
    monkeypatch.setattr(services, "_load_task_types", lambda: dict(store["types"]))
    monkeypatch.setattr(services, "_load_task_type_colors", lambda: dict(store["colors"]))

    def save(task_types, colors=None):
        store["types"] = dict(task_types)
        if colors is not None:
            store["colors"] = {key: value for key, value in colors.items() if key in task_types}

    monkeypatch.setattr(services, "_save_task_types", save)
    read = lambda name: json.loads(paths[name].read_text(encoding="utf-8"))
    return services, store, read


def test_task_type_save_adds_types_with_fresh_ids_and_renames(tmp_path, monkeypatch):
    services, store, _ = _task_type_store(
        tmp_path,
        monkeypatch,
        {"1": "Work", "3": "Study"},
        tasks=[{"description": "Orphan", "priority": 5, "type": "7"}],
        deadlines=[{"event": "Old", "deadline": "2026-10-10", "type": "5"}],
        log=[{"action": "deleted", "timestamp": "2026-10-01T09:00:00", "task": {"description": "Done", "priority": 3, "type": "9"}}],
    )

    # IDs 5, 7, and 9 are still used by a deadline, a task, and the history, so the new type gets 10.
    created = services.task_type_save({"name": "  Reading  "})
    assert created["id"] == "10"
    assert store["types"] == {"1": "Work", "3": "Study", "10": "Reading"}

    services.task_type_save({"id": "1", "name": "Job"})
    assert store["types"]["1"] == "Job"
    # A type only open tasks still use can be named again.
    services.task_type_save({"id": "7", "name": "Errands"})
    assert store["types"]["7"] == "Errands"

    with pytest.raises(services.WebError) as duplicate:
        services.task_type_save({"name": "study"})
    assert duplicate.value.code == "duplicate"
    with pytest.raises(services.WebError) as missing:
        services.task_type_save({"id": "42", "name": "Nowhere"})
    assert missing.value.code == "not_found"
    with pytest.raises(services.WebError, match="at most 60"):
        services.task_type_save({"name": "x" * 61})


def test_task_type_delete_moves_tasks_and_deadlines(tmp_path, monkeypatch):
    services, store, read = _task_type_store(
        tmp_path,
        monkeypatch,
        {"1": "Work", "2": "Study", "3": "Home"},
        tasks=[
            {"description": "Read chapter", "priority": 5, "type": "2"},
            {"description": "Untyped", "priority": 4},
            {"description": "Clean", "priority": 3, "type": "3"},
        ],
        deadlines=[
            {"event": "Exam", "deadline": "2026-10-10", "type": "2"},
            {"event": "Report", "deadline": "2026-10-12", "type": "1"},
            {"event": "Plain", "deadline": "2026-10-13"},
        ],
    )

    for payload in ({"id": "2"}, {"id": "2", "moveTo": "2"}, {"id": "2", "moveTo": "9"}):
        with pytest.raises(services.WebError):
            services.task_type_delete(payload)
    assert store["types"] == {"1": "Work", "2": "Study", "3": "Home"}
    assert read("tasks.json")[0]["type"] == "2"

    result = services.task_type_delete({"id": "2", "moveTo": "3"})
    assert result == {"taskTypes": {"1": "Work", "3": "Home"}, "movedTasks": 1, "movedDeadlines": 1}
    assert store["types"] == {"1": "Work", "3": "Home"}
    assert [task.get("type") for task in read("tasks.json")] == ["3", None, "3"]
    assert [item.get("type") for item in read("ddl.json")] == ["3", "1", None]

    # Tasks saved without a type belong to the default (lowest) type and move with it.
    services.task_type_delete({"id": "1", "moveTo": "3"})
    assert [task.get("type") for task in read("tasks.json")] == ["3", "3", "3"]
    assert [item.get("type") for item in read("ddl.json")] == ["3", "3", None]

    with pytest.raises(services.WebError, match="keep at least one"):
        services.task_type_delete({"id": "3", "moveTo": "3"})


def test_task_type_delete_without_tasks_leaves_deadlines_uncategorized(tmp_path, monkeypatch):
    services, store, read = _task_type_store(
        tmp_path,
        monkeypatch,
        {"1": "Work", "2": "Study"},
        tasks=[{"description": "Write", "priority": 5, "type": "1"}],
        deadlines=[{"event": "Exam", "deadline": "2026-10-10", "type": "2"}],
    )

    result = services.task_type_delete({"id": "2"})

    assert result["movedTasks"] == 0 and result["movedDeadlines"] == 1
    assert store["types"] == {"1": "Work"}
    assert "type" not in read("ddl.json")[0]
    assert read("tasks.json")[0]["type"] == "1"


def test_task_types_keep_their_colors_when_the_list_changes(tmp_path, monkeypatch):
    services, store, _ = _task_type_store(
        tmp_path,
        monkeypatch,
        {"1": "Work", "2": "Study", "3": "Home", "5": "Garden"},
        tasks=[{"description": "Orphan", "priority": 5, "type": "7"}],
    )

    # Without saved colors, types show the palette color of their place: blue, orange, aqua, yellow.
    services.task_type_delete({"id": "2"})
    # Deleting Study saves the others as they showed, instead of moving Home and Garden up the palette.
    assert store["colors"] == {"1": "blue", "3": "aqua", "5": "yellow"}

    # A new type takes the least-used color, in palette order.
    created = services.task_type_save({"name": "Errands"})
    assert created["color"] == "orange"
    assert store["colors"][created["id"]] == "orange"

    services.task_type_save({"id": "1", "name": "Job"})
    assert store["colors"]["1"] == "blue"
    # A type named back from its tasks keeps the hashed color it already showed ("7" -> 55 % 8 -> red).
    services.task_type_save({"id": "7", "name": "Errands 2"})
    assert store["colors"]["7"] == "red"


def test_snapshot_reports_saved_type_colors(tmp_path, monkeypatch):
    from schedule_management.web import services

    settings = tmp_path / "settings.toml"
    settings.write_text('[task_types]\n1 = "Work"\n\n[task_type_colors]\n1 = "violet"\n2 = "plaid"\n', encoding="utf-8")

    class Paths:
        settings_path = settings

    monkeypatch.setattr(services, "_snapshot_runtime_paths", lambda: Paths)
    assert services._load_task_type_colors() == {"1": "violet"}
