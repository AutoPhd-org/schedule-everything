from __future__ import annotations

from datetime import date, datetime


def test_generate_sync_proposal_uses_feedback(monkeypatch):
    from schedule_management.commands import sync as sync_module

    schedule = {
        "09:00": "pomodoro",
        "09:25": "short_break",
        "10:00": "potato",
    }

    monkeypatch.setattr(
        sync_module,
        "_get_base_today_schedule",
        lambda: (schedule, "even", False, _FakeConfig()),
    )
    monkeypatch.setattr(
        sync_module,
        "_load_ranked_tasks",
        lambda: [{"description": "Draft proposal", "priority": 9}],
    )
    monkeypatch.setattr(sync_module, "ensure_llm_config", lambda: {"provider": "fake"})
    monkeypatch.setattr(sync_module, "LLMClient", _FakeClient)
    monkeypatch.setattr(sync_module, "_today", lambda: date(2026, 4, 28))
    monkeypatch.setattr(
        sync_module,
        "_now",
        lambda: datetime(2026, 4, 28, 8, 30),
    )

    proposal = sync_module.generate_sync_proposal(["make the first block specific"])

    assert proposal["summary"] == "Use the strongest task first."
    assert proposal["plan"]["target_date"] == "2026-04-28"
    assert proposal["plan"]["assignments"]["09:00"]["title"] == "Draft proposal"
    assert proposal["preview"][0]["time"] == "09:00"
    assert "make the first block specific" in _FakeClient.last_prompt


def test_accept_sync_plan_persists_payload(monkeypatch, tmp_path):
    from schedule_management.commands import sync as sync_module

    saved_paths = []

    def fake_save(plan):
        saved_paths.append(plan)
        return tmp_path / "synced_schedule.toml"

    monkeypatch.setattr(sync_module, "save_synced_schedule", fake_save)

    result = sync_module.accept_sync_plan(
        {
            "target_date": "2026-04-28",
            "parity": "even",
            "weekday": "tuesday",
            "assignments": {
                "09:00": {"block": "pomodoro", "title": "Draft proposal"},
            },
        }
    )

    assert result["savedPath"].endswith("synced_schedule.toml")
    assert saved_paths[0].assignments["09:00"]["title"] == "Draft proposal"


class _FakeConfig:
    time_blocks = {"pomodoro": 25, "potato": 30}


class _FakeClient:
    last_prompt = ""

    def __init__(self, config):
        self.config = config

    def generate(self, system_prompt, user_prompt):
        type(self).last_prompt = user_prompt
        return (
            '{"summary": "Use the strongest task first.", '
            '"assignments": {"09:00": "Draft proposal", "10:00": "Draft proposal"}}'
        )
