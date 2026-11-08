"""Golden parity cases: the original Python implementation is the oracle.

Each case runs one `rmd` command line (or one browser API request through
`rmd __api JSON`) against a fresh copy of `fixtures/standard` with the clock
frozen, and records the exit code, stdout, stderr and every file under the
config root afterwards. `crates/rmd/tests/golden.rs` replays the same cases
against the Rust binary and compares.

    uv run --with time-machine python tests/golden/golden.py fixture  # rebuild the fixture's data files
    uv run --with time-machine python tests/golden/golden.py record   # re-record cases.json + expected/
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

GOLDEN = Path(__file__).resolve().parent
REPO = GOLDEN.parents[1]
FIXTURE = GOLDEN / "fixtures" / "standard"
EXPECTED = GOLDEN / "expected"
CASES_FILE = GOLDEN / "cases.json"
TZ = "America/Los_Angeles"

N1 = "2026-11-09T10:30:00"  # Monday, even ISO week 46, inside the 10:00 potato block
N2 = "2026-11-12T14:10:00"  # Thursday, even week, inside the 14:00 pomodoro
N3 = "2026-11-15T09:00:00"  # Sunday, a skip day
N4 = "2026-11-17T08:40:00"  # Tuesday, odd ISO week 47
N5 = "2026-12-01T09:00:00"  # first of the month

# =============================================================================
# Fixture data (generated through the Python API so record shapes are real)
# =============================================================================

NEURIPS_YAML = """
- title: NeurIPS
  year: 2026
  id: neurips26
  full_name: Conference on Neural Information Processing Systems
  link: https://neurips.cc/Conferences/2026
  deadlines:
    - type: abstract
      label: Abstract deadline
      date: '2026-05-11 23:59:59'
      timezone: AoE
    - type: paper
      label: Paper deadline
      date: '2026-05-15 23:59:59'
      timezone: AoE
    - type: rebuttal_start
      label: Author rebuttal period start
      date: '2026-07-24 12:00:00'
      timezone: UTC
    - type: rebuttal_end
      label: Author rebuttal period end
      date: '2026-07-30 23:59:59'
      timezone: UTC
  city: San Diego
  country: USA
  date: December 6-12, 2026
  start: '2026-12-06'
  end: '2026-12-12'
- title: NeurIPS
  year: 2027
  full_name: Conference on Neural Information Processing Systems
  link: https://neurips.cc
  deadlines:
    - type: paper
      label: Paper deadline
      date: '2027-05-14 23:59:59'
      timezone: AoE
  date: December 5-11, 2027
"""

ICLR_YAML = """
- title: ICLR
  year: 2027
  full_name: International Conference on Learning Representations
  link: https://iclr.cc/Conferences/2027
  abstract_deadline: '2026-09-19 23:59:59'
  deadline: '2026-09-26 23:59:59'
  timezone: UTC-12
  rebuttal_period_start: '2026-11-12 00:00:00'
  rebuttal_period_end: '2026-12-03 23:59:59'
  city: Rio de Janeiro
  country: Brazil
  date: April 23-27, 2027
  start: 2027-04-23
  end: 2027-04-27
"""


def build_fixture() -> None:
    """Regenerate the fixture's tasks, log, deadlines, habit records and caches."""
    import time_machine
    from zoneinfo import ZoneInfo

    for generated in ("tasks", "cache", "user_config_0/ddl.json", "synced_schedule.toml"):
        path = FIXTURE / generated
        if path.is_dir():
            shutil.rmtree(path)
        elif path.exists():
            path.unlink()
    os.environ["REMINDER_CONFIG_DIR"] = str(FIXTURE)
    os.environ["TZ"] = TZ
    time.tzset()
    sys.path.insert(0, str(REPO / "src"))
    from schedule_management.web.api import dispatch

    zone = ZoneInfo(TZ)

    def at(when: str, command: str, payload: dict) -> dict:
        with time_machine.travel(dt.datetime.fromisoformat(when).replace(tzinfo=zone), tick=False):
            response = dispatch({"command": command, "payload": payload})
        if not response.get("ok"):
            raise SystemExit(f"{when} {command} {payload} failed: {response}")
        return response["data"]

    at("2026-09-01T10:00:00", "deadline_add", {"event": "Rent", "date": "2026-09-30", "repeat": "monthly"})
    at("2026-10-15T10:00:00", "deadline_add", {"event": "Old submission", "date": "2026-11-05"})
    at("2026-10-19T09:00:00", "task_add", {"description": "Write grant report", "priority": 9, "type": "1"})
    at("2026-10-19T09:05:00", "task_add", {"description": "Fix CI pipeline", "priority": 7, "type": "3"})
    at("2026-10-20T10:00:00", "task_add", {"description": "Plan gym routine", "priority": 4, "type": "2"})
    at("2026-10-20T10:05:00", "deadline_add", {"event": "Grant report", "date": "2026-11-20", "type": "1"})
    at("2026-10-20T10:06:00", "deadline_add", {"event": "Yesterday thing", "date": "2026-11-08"})
    at("2026-10-21T15:00:00", "task_add", {"description": "Read attention paper", "priority": 6, "type": "1"})
    at("2026-10-22T18:00:00", "task_delete", {"description": "Read attention paper"})
    at("2026-10-26T09:30:00", "task_add", {"description": "Typo task", "priority": 3})
    at("2026-10-26T09:31:00", "task_delete", {"description": "Typo task", "action": "cancelled"})
    at("2026-10-28T11:00:00", "task_add", {"description": "Learn Rust macros", "priority": 5, "type": "3"})
    at("2026-10-30T10:00:00", "deadline_add", {"event": "Team sync", "date": "2026-11-10", "repeat": "weekly", "type": "3", "color": "blue"})
    at("2026-11-02T10:00:00", "deadline_add", {"event": "论文截止", "date": "2026-12-31"})
    at("2026-11-02T16:00:00", "task_delete", {"description": "Learn Rust macros", "action": "dropped"})
    at("2026-11-03T08:00:00", "task_add", {"description": "整理笔记", "priority": 5, "type": "4"})
    at(
        "2026-11-03T08:01:00",
        "task_subtasks",
        {"description": "整理笔记", "subtasks": [{"text": "Sort notes", "done": True}, {"text": "Tag notes", "done": False}, {"text": "Archive", "done": False}]},
    )
    at("2026-11-04T12:00:00", "task_progress", {"description": "Fix CI pipeline", "progress": 40})
    at("2026-11-05T09:00:00", "task_add", {"description": "Call the bank", "priority": 6})
    at("2026-11-06T09:00:00", "task_add", {"description": "Book flights", "priority": 8, "type": "4", "postpone": 5})
    at("2026-11-06T20:00:00", "task_add", {"description": "Weekly review notes", "priority": 2, "type": "4"})
    at("2026-11-07T17:00:00", "task_delete", {"description": "Weekly review notes"})
    at("2026-11-08T10:00:00", "task_update", {"originalDescription": "Plan gym routine", "description": "Plan gym routine v2", "priority": 5, "type": "2"})
    for day, ids in (
        ("2026-11-02", ["1", "2"]),
        ("2026-11-03", ["1", "3", "5"]),
        ("2026-11-05", ["2"]),
        ("2026-11-07", ["1", "2", "3", "4", "5"]),
        ("2026-11-08", ["3"]),
    ):
        at(f"{day}T23:50:00", "habit_mark", {"habitIds": ids})

    (FIXTURE / "tasks" / "procrastinate.json").write_text(
        json.dumps(["Call the bank", {"description": "Fix CI pipeline", "since": "2026-11-01"}], indent=2, ensure_ascii=False),
        encoding="utf-8",
    )

    from schedule_management.conferences import CACHE_VERSION, SOURCE_URL, normalize_editions

    cache = {
        "version": CACHE_VERSION,
        "source": SOURCE_URL,
        "fetchedAt": "2026-11-08T20:00:00+00:00",
        "conferences": {"neurips": normalize_editions(NEURIPS_YAML), "iclr": normalize_editions(ICLR_YAML)},
        "errors": [],
    }
    cache_path = FIXTURE / "cache" / "ai_conferences.json"
    cache_path.parent.mkdir(parents=True, exist_ok=True)
    cache_path.write_text(json.dumps(cache, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    print(f"fixture rebuilt in {FIXTURE}")


# =============================================================================
# Cases
# =============================================================================

CASES: list[dict] = []


def cli(name: str, now: str, *argv: str, stdout: str = "ignore", stderr: str = "ignore", **extra) -> None:
    """A command-line case. `stdout`/`stderr`: "json", "text" (exact) or "ignore"."""
    CASES.append({"name": name, "now": now, "argv": list(argv), "stdout": stdout, "stderr": stderr, **extra})


def api(name: str, now: str, command: str, payload: dict | None = None, **extra) -> None:
    """A browser API case (`rmd __api JSON`); the JSON response is compared."""
    request = json.dumps({"command": command, "payload": payload if payload is not None else {}}, ensure_ascii=False)
    CASES.append({"name": name, "now": now, "argv": ["__api", request], "stdout": "json", "stderr": "ignore", **extra})


def raw_api(name: str, now: str, request: str, **extra) -> None:
    CASES.append({"name": name, "now": now, "argv": ["__api", request], "stdout": "json", "stderr": "ignore", **extra})


PLAN_N1 = {"date": "2026-11-09", "assignments": {"08:30": "Write grant report", "10:00": "Fix CI pipeline"}}
SYNCED_N1 = (
    '# Accepted task assignments for today\'s focus blocks.\n# This file is written by `rmd sync --apply`.\n'
    'date = "2026-11-09"\nparity = "even"\nweekday = "monday"\n\n[schedule]\n'
    '"09:30" = { block = "pomodoro", title = "Call the bank" }\n'
)


def define_cases() -> None:
    CASES.clear()
    # --- check -----------------------------------------------------------------
    cli("check_json", N1, "check", "--json", stdout="json")
    cli("check_text", N1, "check", stdout="text")
    cli("check_dir_missing", N1, "check", "--dir", "<ROOT>/nowhere", "--json", stdout="json")
    cli(
        "check_bad_week",
        N1,
        "check",
        "--json",
        stdout="json",
        files={"user_config_0/odd_weeks.toml": '[monday]\n"9:00" = "potato"\n'},
    )
    cli(
        "check_label_problems",
        N1,
        "check",
        "--json",
        stdout="json",
        files={
            "user_config_0/even_weeks.toml": '[monday]\n"09:00" = { block = "nap" }\n"10:00" = { block = "potato", title = " " }\n"11:00" = "made_up"\n'
        },
    )
    cli("check_missing_settings_tables", N1, "check", "--json", stdout="json", files={"user_config_0/settings.toml": "[settings]\n[time_blocks]\n"})
    cli(
        "check_bad_deadlines",
        N1,
        "check",
        "--json",
        stdout="json",
        files={"user_config_0/ddl.json": '[{"event": "x", "deadline": "2026-13-01"}]'},
    )
    cli("check_bad_habits", N1, "check", "--json", stdout="json", files={"user_config_0/habits.toml": 'a = "b"\n'})

    # --- sync --------------------------------------------------------------------
    cli("sync_json_monday", N1, "sync", "--json", stdout="json")
    cli("sync_json_thursday", N2, "sync", "--json", stdout="json")
    cli("sync_json_skip_day", N3, "sync", "--json", stdout="json")
    cli("sync_json_odd_week", N4, "sync", "--json", stdout="json")
    cli("sync_json_with_plan", N1, "sync", "--json", stdout="json", files={"synced_schedule.toml": SYNCED_N1})
    cli("sync_json_stale_plan", N2, "sync", "--json", stdout="json", files={"synced_schedule.toml": SYNCED_N1})
    cli("sync_json_p_mode", N1, "sync", "--json", stdout="json", files={"tasks/mode.txt": "p"})
    cli("sync_json_zh", N1, "sync", "--json", stdout="json", env={"REMINDER_LANG": "zh"})
    cli("sync_apply_file", N1, "sync", "--apply", "<ROOT>/plan.json", stdout="text", stderr="text", files={"plan.json": json.dumps(PLAN_N1)})
    cli("sync_apply_stdin", N1, "sync", "--apply", "-", stdout="text", stderr="text", stdin=json.dumps(PLAN_N1))
    cli(
        "sync_apply_list_form",
        N1,
        "sync",
        "--apply",
        "-",
        stdout="text",
        stderr="text",
        stdin=json.dumps({"assignments": [{"time": "09:30", "title": "  Call   the bank  "}, {"time": "16:00", "title": "Book flights"}]}),
    )
    cli("sync_apply_wrong_date", N1, "sync", "--apply", "-", stdout="text", stderr="text", stdin=json.dumps({"date": "2026-11-10", "assignments": {"08:30": "x"}}))
    cli("sync_apply_unknown_slot", N1, "sync", "--apply", "-", stdout="text", stderr="text", stdin=json.dumps({"assignments": {"07:00": "x"}}))
    cli("sync_apply_titled_slot", N1, "sync", "--apply", "-", stdout="text", stderr="text", stdin=json.dumps({"assignments": {"11:00": "x"}}))
    cli("sync_apply_empty_title", N1, "sync", "--apply", "-", stdout="text", stderr="text", stdin=json.dumps({"assignments": {"08:30": "  "}}))
    cli("sync_apply_long_title", N1, "sync", "--apply", "-", stdout="text", stderr="text", stdin=json.dumps({"assignments": {"08:30": "x" * 121}}))
    cli("sync_apply_no_assignments", N1, "sync", "--apply", "-", stdout="text", stderr="text", stdin=json.dumps({"assignments": {}}))
    cli("sync_apply_not_object", N1, "sync", "--apply", "-", stdout="text", stderr="text", stdin="[1, 2]")
    cli("sync_apply_bad_list_item", N1, "sync", "--apply", "-", stdout="text", stderr="text", stdin=json.dumps({"assignments": ["x"]}))
    cli("sync_apply_skip_day", N3, "sync", "--apply", "-", stdout="text", stderr="text", stdin=json.dumps({"assignments": {"08:30": "x"}}))
    cli("sync_apply_p_mode", N1, "sync", "--apply", "-", stdout="text", stderr="text", stdin=json.dumps(PLAN_N1), files={"tasks/mode.txt": "p"})
    cli("sync_clear_existing", N1, "sync", "--clear", stdout="text", files={"synced_schedule.toml": SYNCED_N1})
    cli("sync_clear_none", N1, "sync", "--clear", stdout="text")
    cli("sync_table", N1, "sync")

    # --- tasks -------------------------------------------------------------------
    cli("add_basic", N1, "add", "Write slides", "7")
    cli("add_with_type", N1, "add", "Read diffusion paper", "6", "1")
    cli("add_with_postpone", N1, "add", "Renew passport", "5", "4", "2")
    cli("add_existing", N1, "add", "Call the bank", "9")
    cli("add_bad_priority", N1, "add", "Too important", "11")
    cli("rm_by_index", N1, "rm", "1")
    cli("rm_by_description", N1, "rm", "Call the bank")
    cli("rm_many", N1, "rm", "1", "2")
    cli("rm_missing", N1, "rm", "No such task")
    cli("cancel_task", N1, "cancel", "Book flights")
    cli("drop_task", N1, "drop", "整理笔记")
    cli("ls_all", N1, "ls")
    cli("ls_type", N1, "ls", "-t", "1")
    cli("history_default", N1, "history")
    cli("history_count", N1, "history", "2")

    # --- deadlines, habits, mode, status --------------------------------------------
    cli("ddl_show", N1, "ddl")
    cli("ddl_add", N1, "ddl", "add", "Visa appointment", "12.15")
    cli("ddl_add_past_month_day", N1, "ddl", "add", "New year plan", "1.2")
    cli("ddl_add_repeat", N1, "ddl", "add", "Pay card", "11.20", "--repeat", "monthly")
    cli("ddl_add_existing", N1, "ddl", "add", "Grant report", "11.25")
    cli("ddl_add_bad_date", N1, "ddl", "add", "Broken", "13.40")
    cli("ddl_rm", N1, "ddl", "rm", "Grant report")
    cli("ddl_rm_missing", N1, "ddl", "rm", "Nope")
    cli("track_ids", N1, "track", "1", "3")
    cli("track_invalid", N1, "track", "9")
    cli("mode_show", N1, "mode", stdout="text")
    cli("mode_set_p", N1, "mode", "p", stdout="text")
    cli("mode_set_j", N1, "mode", "j", stdout="text", files={"tasks/mode.txt": "p"})
    cli("status_plain", N1, "status")
    cli("status_verbose", N2, "status", "-v")
    cli("status_skip_day", N3, "status")
    cli("no_command", N1)
    cli("unknown_command", N1, "frobnicate")

    # --- browser API -----------------------------------------------------------------
    raw_api("api_invalid_json", N1, "{not json")
    raw_api("api_not_object", N1, "[1]")
    api("api_unknown_command", N1, "no_such_command")
    raw_api("api_missing_command", N1, json.dumps({"payload": {}}))
    raw_api("api_payload_not_object", N1, json.dumps({"command": "status_snapshot", "payload": []}))

    api("api_status_snapshot_monday", N1, "status_snapshot")
    api("api_status_snapshot_thursday", N2, "status_snapshot")
    api("api_status_snapshot_skip_day", N3, "status_snapshot")
    api("api_status_snapshot_odd_week", N4, "status_snapshot", files={"synced_schedule.toml": SYNCED_N1})
    api("api_status_snapshot_with_plan", N1, "status_snapshot", files={"synced_schedule.toml": SYNCED_N1})
    api("api_status_snapshot_zh", N1, "status_snapshot", env={"REMINDER_LANG": "zh"})

    api("api_task_add", N1, "task_add", {"description": "Prepare talk", "priority": 8, "type": "3"})
    api("api_task_add_postpone", N1, "task_add", {"description": "Dentist", "priority": 4, "postpone": 3})
    api("api_task_add_existing", N1, "task_add", {"description": "Call the bank", "priority": 2})
    api("api_task_add_bad_priority", N1, "task_add", {"description": "x", "priority": 0})
    api("api_task_add_priority_text", N1, "task_add", {"description": "x", "priority": "7"})
    api("api_task_add_unknown_type", N1, "task_add", {"description": "x", "priority": 5, "type": "99"})
    api("api_task_add_negative_postpone", N1, "task_add", {"description": "x", "priority": 5, "postpone": -1})
    api("api_task_add_missing_description", N1, "task_add", {"priority": 5})
    api("api_task_update", N1, "task_update", {"originalDescription": "Call the bank", "description": "Call the bank today", "priority": 9})
    api("api_task_update_type_postpone", N1, "task_update", {"originalDescription": "Book flights", "description": "Book flights", "priority": 8, "type": "1", "postpone": 0})
    api("api_task_update_duplicate", N1, "task_update", {"originalDescription": "Call the bank", "description": "Fix CI pipeline", "priority": 5})
    api("api_task_update_missing", N1, "task_update", {"originalDescription": "Nope", "description": "Nope", "priority": 5})
    api("api_task_progress", N1, "task_progress", {"description": "Fix CI pipeline", "progress": 65})
    api("api_task_progress_clear", N1, "task_progress", {"description": "Fix CI pipeline", "progress": 0})
    api("api_task_progress_subtasks", N1, "task_progress", {"description": "整理笔记", "progress": 10})
    api("api_task_progress_bad", N1, "task_progress", {"description": "Fix CI pipeline", "progress": "lots"})
    api("api_task_subtasks", N1, "task_subtasks", {"description": "Call the bank", "subtasks": [{"text": " Find number ", "done": True}, {"text": "Call", "done": False}]})
    api("api_task_subtasks_clear", N1, "task_subtasks", {"description": "整理笔记", "subtasks": []})
    api("api_task_subtasks_bad", N1, "task_subtasks", {"description": "整理笔记", "subtasks": "nope"})
    api("api_task_delete", N1, "task_delete", {"description": "Write grant report"})
    api("api_task_delete_cancelled", N1, "task_delete", {"description": "Book flights", "action": "cancelled"})
    api("api_task_delete_dropped", N1, "task_delete", {"description": "Call the bank", "action": "dropped"})
    api("api_task_delete_bad_action", N1, "task_delete", {"description": "Call the bank", "action": "eaten"})
    api("api_task_delete_missing", N1, "task_delete", {"description": "Nope"})
    api("api_task_restore_missing_tasks", N1, "task_restore", {"description": "Gone", "action": "deleted"})
    api("api_task_restore", N1, "task_restore", {"description": "Gone", "action": "deleted", "tasks": [{"index": 0, "task": {"description": "Gone", "priority": 3}}]})
    api("api_task_history", N1, "task_history", {"count": 3})
    api("api_task_history_all", N1, "task_history", {"all": True})
    api("api_task_history_default", N1, "task_history")

    api("api_deadline_add", N1, "deadline_add", {"event": "Camera ready", "date": "2026-12-05", "type": "1", "color": "orange"})
    api("api_deadline_add_month_day", N1, "deadline_add", {"event": "Taxes", "date": "4.15"})
    api("api_deadline_add_repeat", N1, "deadline_add", {"event": "Standup notes", "date": "2026-11-13", "repeat": "biweekly"})
    api("api_deadline_add_bad_date", N1, "deadline_add", {"event": "Bad", "date": "2026-02-30"})
    api("api_deadline_add_bad_format", N1, "deadline_add", {"event": "Bad", "date": "tomorrow"})
    api("api_deadline_add_existing", N1, "deadline_add", {"event": "Grant report", "date": "2026-11-27"})
    api("api_deadline_update", N1, "deadline_update", {"originalEvent": "Grant report", "event": "Grant final report", "date": "2026-11-21", "repeat": "weekly"})
    api("api_deadline_update_missing", N1, "deadline_update", {"originalEvent": "Nope", "event": "Nope", "date": "2026-11-21"})
    api("api_deadline_delete", N1, "deadline_delete", {"event": "论文截止"})
    api("api_deadline_delete_missing", N1, "deadline_delete", {"event": "Nope"})

    api("api_habit_mark", N1, "habit_mark", {"habitIds": ["2", 4]})
    api("api_habit_mark_invalid", N1, "habit_mark", {"habitIds": ["2", "42"]})
    api("api_habit_mark_not_list", N1, "habit_mark", {"habitIds": "2"})

    api("api_settings_get_task_types", N1, "settings_get_task_types")
    api("api_settings_set_task_types", N1, "settings_set_task_types", {"taskTypes": {"1": "papers", "7": "errands"}})
    api("api_settings_set_task_types_bad", N1, "settings_set_task_types", {"taskTypes": {"1": " "}})
    api("api_task_type_save_new", N1, "task_type_save", {"name": "writing"})
    api("api_task_type_save_rename", N1, "task_type_save", {"id": "2", "name": "fitness", "color": "green"})
    api("api_task_type_save_duplicate", N1, "task_type_save", {"name": "coding"})
    api("api_task_type_save_too_long", N1, "task_type_save", {"name": "x" * 61})
    api("api_task_type_delete_move", N1, "task_type_delete", {"id": "3", "moveTo": "4"})
    api("api_task_type_delete_missing", N1, "task_type_delete", {"id": "42"})

    api("api_weekly_overview", N1, "weekly_overview")
    api("api_weekly_overview_past", N1, "weekly_overview", {"date": "2026-10-21"})
    api("api_daily_overview", N1, "daily_overview")
    api("api_daily_overview_date", N1, "daily_overview", {"date": "2026-11-07"})
    api("api_daily_overview_bad_date", N1, "daily_overview", {"date": "07/11/2026"})

    api("api_conference_calendar", N1, "conference_calendar", {"year": 2026})
    api("api_conference_calendar_2027", N1, "conference_calendar", {"year": 2027})
    api("api_conference_calendar_bad_year", N1, "conference_calendar", {"year": "soon"})
    api("api_conference_calendar_out_of_range", N1, "conference_calendar", {"year": 1900})

    api("api_config_read_settings", N1, "config_read", {"file": "settings"})
    api("api_config_read_odd", N1, "config_read", {"file": "odd"})
    api("api_config_read_deadlines", N1, "config_read", {"file": "deadlines"})
    api("api_config_read_profile", N1, "config_read", {"file": "profile"})
    api("api_config_read_unknown", N1, "config_read", {"file": "passwords"})
    api("api_config_save_stale", N1, "config_save", {"file": "profile", "content": "# New\n", "activeId": 0, "revision": "stale"})
    api("api_workspace_info", N1, "workspace_info")
    api("api_service_action_unknown", N1, "service_action", {"action": "explode"})
    api("api_export_pdf_unknown_kind", N1, "export_pdf", {"kind": "poster"})
    api("api_sync_clear", N1, "sync_clear", files={"synced_schedule.toml": SYNCED_N1})
    api("api_sync_clear_none", N1, "sync_clear")


# =============================================================================
# Recording
# =============================================================================


def snapshot(root: Path) -> dict[str, str]:
    files: dict[str, str] = {}
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        relative = path.relative_to(root).as_posix()
        data = path.read_bytes()
        try:
            files[relative] = data.decode("utf-8")
        except UnicodeDecodeError:
            files[relative] = "<binary sha256:" + hashlib.sha256(data).hexdigest() + ">"
    return files


def normalize(text: str, root: Path, home: Path) -> str:
    for base, token in ((root, "<ROOT>"), (home, "<HOME>")):
        # Longest form first: the resolved path (/private/var/...) contains the raw one (/var/...).
        for form in sorted({str(base.resolve()), str(base)}, key=len, reverse=True):
            text = text.replace(form, token)
    return text


def run_python(case: dict) -> dict:
    with tempfile.TemporaryDirectory() as tmp:
        root, home = Path(tmp) / "root", Path(tmp) / "home"
        shutil.copytree(FIXTURE, root)
        home.mkdir()
        for relative, content in case.get("files", {}).items():
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content, encoding="utf-8")
        initial = snapshot(root)
        argv = [arg.replace("<ROOT>", str(root)) for arg in case["argv"]]
        env = {
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "TZ": TZ,
            "HOME": str(home),
            "REMINDER_CONFIG_DIR": str(root),
            "NO_COLOR": "1",
            "COLUMNS": "100",
            "LANG": "en_US.UTF-8",
            "TERM": "dumb",
            "PYTHONIOENCODING": "utf-8",
            **case.get("env", {}),
        }
        proc = subprocess.run(
            [sys.executable, str(GOLDEN / "run_python.py"), "--now", case["now"], "--", *argv],
            input=case.get("stdin"),
            capture_output=True,
            text=True,
            cwd=root,
            env=env,
            timeout=120,
        )
        after = snapshot(root)
        # Only what the command changed: new or modified files, and deleted ones.
        files = {k: normalize(v, root, home) for k, v in after.items() if initial.get(k) != v}
        return {
            "exit": proc.returncode,
            "stdout": normalize(proc.stdout, root, home),
            "stderr": normalize(proc.stderr, root, home),
            "files": files,
            "deleted": sorted(k for k in initial if k not in after),
        }


def record(only: list[str]) -> None:
    define_cases()
    EXPECTED.mkdir(exist_ok=True)
    CASES_FILE.write_text(json.dumps({"timezone": TZ, "cases": CASES}, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    names = {case["name"] for case in CASES}
    for stale in EXPECTED.glob("*.json"):
        if stale.stem not in names:
            stale.unlink()
    for case in CASES:
        if only and case["name"] not in only:
            continue
        result = run_python(case)
        (EXPECTED / f"{case['name']}.json").write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        mark = "ok " if result["exit"] == 0 else f"x{result['exit']:<2}"
        print(f"{mark} {case['name']}")
    print(f"recorded {len(only) or len(CASES)} case(s) into {EXPECTED}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="action", required=True)
    sub.add_parser("fixture", help="rebuild the fixture's generated data files")
    rec = sub.add_parser("record", help="record expected outputs from the Python implementation")
    rec.add_argument("names", nargs="*", help="only these cases")
    args = parser.parse_args()
    if args.action == "fixture":
        build_fixture()
    else:
        record(args.names)
    return 0


if __name__ == "__main__":
    sys.exit(main())
