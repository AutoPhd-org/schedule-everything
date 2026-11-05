"""Tests for `rmd check`, which AI agent plugins use to validate schedule files."""

from __future__ import annotations

import json

import pytest

SETTINGS = """
[settings]
language = "en"

[time_blocks]
pomodoro = 25
deep_work = 90

[time_points]
lunch_time = "Lunch"

[tasks]
daily_summary = "23:35"

[paths]
config_dir = "config"
"""
WEEK = """
[monday]
"09:00" = "deep_work"
"12:30" = "lunch_time"
"14:00" = { block = "pomodoro", title = "Review" }

[common]
"20:00" = "pomodoro"
"""


@pytest.fixture
def config_root(tmp_path, monkeypatch):
    root = tmp_path / "config"
    active = root / "user_config_0"
    active.mkdir(parents=True)
    (root / ".active_config").write_text("0\n", encoding="utf-8")
    (active / "settings.toml").write_text(SETTINGS, encoding="utf-8")
    (active / "odd_weeks.toml").write_text(WEEK, encoding="utf-8")
    (active / "even_weeks.toml").write_text(WEEK, encoding="utf-8")
    (active / "habits.toml").write_text('[habits]\n1 = "Read"\n', encoding="utf-8")
    monkeypatch.setenv("REMINDER_CONFIG_DIR", str(root))
    return root


def run(args: list[str]) -> int:
    from schedule_management.cli import create_parser

    parsed = create_parser().parse_args(["check", *args])
    return parsed.func(parsed)


def test_valid_active_config_reports_paths_for_agents(config_root, capsys):
    assert run(["--json"]) == 0
    report = json.loads(capsys.readouterr().out)
    assert report["valid"] is True
    assert report["errors"] == []
    assert report["configRoot"] == str(config_root)
    assert report["activeId"] == 0
    assert report["configIds"] == [0]
    assert report["nextConfigId"] == 1
    assert report["nextConfigDir"] == str(config_root / "user_config_1")
    assert report["directory"] == report["activeConfigDir"] == str(config_root / "user_config_0")
    assert report["files"]["profile"] == {"path": str(config_root / "user_config_0" / "profile.md"), "exists": False}

    assert run([]) == 0
    assert "Configuration is valid" in capsys.readouterr().out


def test_unknown_blocks_fail_and_undefined_labels_warn(config_root, capsys):
    (config_root / "user_config_0" / "odd_weeks.toml").write_text(
        '[tuesday]\n"09:00" = "gym"\n"10:00" = { block = "lunch_time", title = "Lunch talk" }\n', encoding="utf-8"
    )
    assert run(["--json"]) == 1
    report = json.loads(capsys.readouterr().out)
    assert report["errors"] == [{"file": "odd_weeks.toml", "message": "[tuesday] 10:00: block 'lunch_time' is not a [time_blocks] name in settings.toml"}]
    assert report["warnings"] == [{"file": "odd_weeks.toml", "message": "[tuesday] 09:00: 'gym' is not a [time_blocks] or [time_points] name, so it is shown as a plain reminder message"}]

    (config_root / "user_config_0" / "odd_weeks.toml").write_text('[tuesday]\n"09:00" = "Call mom"\n', encoding="utf-8")
    assert run(["--json"]) == 0
    assert len(json.loads(capsys.readouterr().out)["warnings"]) == 1


@pytest.mark.parametrize(
    ("file_name", "content", "message"),
    [
        ("settings.toml", "[settings]\nlanguage = 'en'\n", "settings require a [time_blocks] table"),
        ("settings.toml", "[settings\n", "Expected"),
        ("even_weeks.toml", '[funday]\n"09:00" = "pomodoro"\n', "weekday or common tables"),
        ("even_weeks.toml", '[monday]\n"9am" = "pomodoro"\n', "valid HH:MM time"),
        ("habits.toml", "[habits\n", "Expected"),
        ("ddl.json", '[{"event": "Launch", "deadline": "soon"}]', "Invalid isoformat"),
    ],
)
def test_syntax_and_structure_errors(config_root, capsys, file_name, content, message):
    (config_root / "user_config_0" / file_name).write_text(content, encoding="utf-8")
    assert run(["--json"]) == 1
    errors = json.loads(capsys.readouterr().out)["errors"]
    assert any(error["file"] == file_name and message in error["message"] for error in errors)


def test_missing_tasks_and_paths_tables_are_warnings(config_root, capsys):
    (config_root / "user_config_0" / "settings.toml").write_text(
        '[settings]\n[time_blocks]\npomodoro = 25\ndeep_work = 90\n[time_points]\nlunch_time = "Lunch"\n', encoding="utf-8"
    )
    assert run(["--json"]) == 0
    report = json.loads(capsys.readouterr().out)
    assert report["valid"] is True
    assert [warning["message"] for warning in report["warnings"]] == [
        "no [tasks] table; built-in defaults apply",
        "no [paths] table; built-in defaults apply",
    ]
    assert run([]) == 0
    assert "⚠️  settings.toml: no [tasks] table" in capsys.readouterr().out


def test_missing_required_files_and_other_directories(config_root, tmp_path, capsys):
    draft = tmp_path / "draft"
    draft.mkdir()
    (draft / "settings.toml").write_text(SETTINGS, encoding="utf-8")
    assert run(["--dir", str(draft)]) == 1
    output = capsys.readouterr().out
    assert f"Checked:     {draft}" in output
    assert "odd_weeks.toml: missing" in output and "even_weeks.toml: missing" in output

    (draft / "odd_weeks.toml").write_text(WEEK, encoding="utf-8")
    (draft / "even_weeks.toml").write_text(WEEK, encoding="utf-8")
    assert run(["--dir", str(draft), "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["directory"] == str(draft.resolve())


def test_browser_editor_uses_the_same_validation(config_root):
    from schedule_management.web.services import WebError
    from schedule_management.web.workspace import validate_content

    with pytest.raises(WebError, match=r"settings require a \[paths\] table") as exc_info:
        validate_content("settings", "[settings]\n[time_blocks]\n[time_points]\n[tasks]\n")
    assert exc_info.value.code == "invalid_input"
    validate_content("profile", "# Anything goes in Markdown")
