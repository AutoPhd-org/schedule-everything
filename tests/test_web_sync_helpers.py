"""Browser API coverage for today's plan overlay and the removed in-app AI commands."""

from __future__ import annotations

from datetime import date


def test_sync_clear_removes_todays_plan(monkeypatch, tmp_path):
    from schedule_management.commands import sync as sync_module
    from schedule_management.web import api

    overlay = tmp_path / "synced_schedule.toml"
    monkeypatch.setenv("REMINDER_SYNCED_SCHEDULE_PATH", str(overlay))
    monkeypatch.setattr(sync_module, "_today", lambda: date(2026, 11, 4))
    monkeypatch.setattr(sync_module, "load_mode", lambda: "j")
    monkeypatch.setattr(
        sync_module,
        "_get_base_today_schedule",
        lambda: ({"09:00": "pomodoro"}, "even", False, None),
    )
    sync_module.apply_sync_plan({"assignments": {"09:00": "Write"}})
    assert overlay.exists()

    assert api.dispatch({"command": "sync_clear", "payload": {}}) == {"ok": True, "data": {"cleared": True}}
    assert not overlay.exists()
    assert api.dispatch({"command": "sync_clear", "payload": {}}) == {"ok": True, "data": {"cleared": False}}


def test_in_app_ai_commands_are_gone():
    from schedule_management.web import api

    for command in ("sync_generate", "sync_accept", "setup_turn", "setup_accept"):
        response = api.dispatch({"command": command, "payload": {}})
        assert response["ok"] is False
        assert response["error"]["code"] == "unknown_command"


def test_config_editor_no_longer_exposes_model_settings():
    from schedule_management.web import workspace
    from schedule_management.web.services import WebError

    assert "model" not in workspace.FILES
    try:
        workspace._file_path("model")
    except WebError as exc:
        assert exc.code == "invalid_input"
    else:
        raise AssertionError("model file should be rejected")
