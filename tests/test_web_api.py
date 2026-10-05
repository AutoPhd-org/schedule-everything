from __future__ import annotations

import importlib
import json
import sys


def test_dispatch_returns_success(monkeypatch):
    from schedule_management.web import api

    monkeypatch.setitem(
        api.COMMANDS,
        "example",
        lambda payload: {"echo": payload["value"]},
    )

    response = api.dispatch({"command": "example", "payload": {"value": "ok"}})

    assert response == {"ok": True, "data": {"echo": "ok"}}


def test_dispatch_returns_structured_web_error(monkeypatch):
    from schedule_management.web import api
    from schedule_management.web.services import WebError

    def raise_error(payload):
        raise WebError("invalid_input", "bad input", {"field": "name"})

    monkeypatch.setitem(api.COMMANDS, "broken", raise_error)

    response = api.dispatch({"command": "broken", "payload": {}})

    assert response["ok"] is False
    assert response["error"]["code"] == "invalid_input"
    assert response["error"]["message"] == "bad input"
    assert response["error"]["details"] == {"field": "name"}


def test_dispatch_rejects_unknown_command():
    from schedule_management.web import api

    response = api.dispatch({"command": "missing", "payload": {}})

    assert response["ok"] is False
    assert response["error"]["code"] == "unknown_command"


def test_main_reads_json_from_argument(capsys, monkeypatch):
    from schedule_management.web import api

    monkeypatch.setitem(
        api.COMMANDS,
        "example",
        lambda payload: {"echo": payload["value"]},
    )

    exit_code = api.main(
        ['{"command": "example", "payload": {"value": "hello"}}']
    )

    assert exit_code == 0
    output = json.loads(capsys.readouterr().out)
    assert output == {"ok": True, "data": {"echo": "hello"}}


def test_api_import_does_not_load_visualizer():
    sys.modules.pop("schedule_management.web.api", None)
    sys.modules.pop("schedule_management.commands.status", None)
    sys.modules.pop("schedule_management.visualizer", None)

    importlib.import_module("schedule_management.web.api")

    assert "schedule_management.visualizer" not in sys.modules
