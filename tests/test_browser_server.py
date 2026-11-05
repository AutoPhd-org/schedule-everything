"""HTTP security, launch reuse, and browser configuration regressions."""
from __future__ import annotations

import http.client
import json
import queue
import threading
from pathlib import Path

import pytest

from schedule_management.web.server import BrowserServer
from schedule_management.web import api, workspace
from schedule_management.web.services import WebError


@pytest.fixture
def server():
    app = BrowserServer(token="test-token")
    thread = threading.Thread(target=app.serve_forever, daemon=True)
    thread.start()
    yield app
    app.shutdown()
    app.server_close()
    thread.join(timeout=3)


def request(server, method, path, payload=None, headers=None):
    connection = http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=5)
    body = json.dumps(payload) if payload is not None else None
    connection.request(method, path, body=body, headers=headers or {})
    response = connection.getresponse()
    result = response.status, response.getheaders(), response.read()
    connection.close()
    return result


def test_static_app_is_bundled_and_paths_are_restricted(server):
    status, headers, body = request(server, "GET", "/")
    assert status == 200 and b'<main id="app">' in body
    assert "frame-ancestors 'none'" in dict(headers)["Content-Security-Policy"]
    for path in ("/../server.py", "/%2e%2e/server.py", "/.browser.json"):
        assert request(server, "GET", path)[0] == 404


@pytest.mark.parametrize("headers", [{}, {"Authorization": "Bearer wrong"}, {"Authorization": "Bearer test-token", "Origin": "https://example.com"}, {"Authorization": "Bearer test-token", "Host": "evil.example"}])
def test_api_rejects_unauthenticated_cross_origin_and_rebound_requests(server, headers):
    assert request(server, "POST", "/api", {"command": "workspace_info"}, headers)[0] == 403


def test_session_cookie_authenticates_same_origin_requests(server):
    status, headers, _ = request(server, "POST", "/session", {}, {"Authorization": "Bearer test-token", "Origin": server.origin})
    assert status == 200
    cookie = dict(headers)["Set-Cookie"]
    assert "HttpOnly" in cookie and "SameSite=Strict" in cookie
    status, _, body = request(server, "POST", "/api", {"command": "settings_get_task_types"}, {"Cookie": cookie.split(";", 1)[0], "Origin": server.origin})
    assert status == 200 and json.loads(body)["ok"]


def test_api_validates_json_shape_and_size(server):
    status, _, body = request(server, "POST", "/api", [], {"Authorization": "Bearer test-token"})
    assert status == 200 and json.loads(body)["error"]["code"] == "invalid_request"
    assert request(server, "POST", "/api", {}, {"Authorization": "Bearer test-token", "Content-Length": "8388609"})[0] == 413


def test_activation_notifies_existing_tabs(server):
    tab = queue.Queue()
    server.tabs.add(tab)
    status, _, body = request(server, "POST", "/activate", {}, {"Authorization": "Bearer test-token"})
    assert status == 200 and json.loads(body)["hasTab"] is True
    assert tab.get_nowait() == "activate"
    server.tabs.remove(tab)
    assert server.activate() is False


def test_sse_registers_an_open_browser_tab(server):
    connection = http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=5)
    connection.request("GET", "/events", headers={"Authorization": "Bearer test-token"})
    response = connection.getresponse()
    assert response.status == 200
    assert response.readline() == b"event: ready\n"
    response.readline(); response.readline()
    assert server.activate() is True
    assert response.readline() == b"event: activate\n"
    connection.close()


def test_downloads_require_authentication_and_known_export(server, tmp_path, monkeypatch):
    output = tmp_path / "report.pdf"
    output.write_bytes(b"%PDF-test")
    monkeypatch.setitem(workspace.EXPORTS, "test-export", output)
    assert request(server, "GET", "/downloads/test-export")[0] == 403
    assert request(server, "GET", "/downloads/test-export", headers={"Authorization": "Bearer test-token"})[2] == b"%PDF-test"
    assert request(server, "GET", "/downloads/missing", headers={"Authorization": "Bearer test-token"})[0] == 404


def test_launcher_reuses_tab_and_opens_again_when_no_tab(monkeypatch, tmp_path):
    from schedule_management.commands import browser
    opened = []
    monkeypatch.setattr(browser, "resolve_config_root_dir", lambda: tmp_path)
    monkeypatch.setattr(browser, "_live_state", lambda path: {"port": 12345, "token": "t", "pid": 1})
    monkeypatch.setattr(browser, "_request", lambda state, route, post=False: {"hasTab": True})
    monkeypatch.setattr(browser.webbrowser, "open", lambda *args, **kwargs: opened.append(args))
    url, reused = browser.launch_browser()
    assert reused is True and not opened
    assert url == "http://127.0.0.1:12345/"
    monkeypatch.setattr(browser, "_request", lambda state, route, post=False: {"hasTab": False})
    browser.launch_browser()
    assert opened == [("http://127.0.0.1:12345/#token=t",)]
    assert browser.launch_browser(open_browser=False)[0].endswith("#token=t")
    assert len(opened) == 1


def test_stale_registry_does_not_attach_to_another_process(monkeypatch, tmp_path):
    from schedule_management.commands import browser
    registry = tmp_path / "state.json"
    registry.write_text('{"port":12345,"token":"t","pid":42}')
    monkeypatch.setattr(browser, "_request", lambda *args: {"app": "schedule-everything", "pid": 43})
    assert browser._live_state(registry) is None


def test_cli_web_options_route_to_browser():
    from schedule_management.cli import create_parser
    from schedule_management.commands.browser import browser_command
    args = create_parser().parse_args(["web", "--port", "8765", "--no-open"])
    assert args.func is browser_command and args.port == 8765 and args.no_open


def test_configuration_save_validates_and_rejects_stale_edits(monkeypatch, tmp_path):
    file = tmp_path / "habits.toml"
    file.write_text('[habits]\n1 = "Read"\n')
    monkeypatch.setattr(workspace, "_file_path", lambda key: file)
    monkeypatch.setattr(workspace, "_reload", lambda: {"reloaded": True})
    original = workspace.config_read({"file": "habits"})
    with pytest.raises(WebError, match="Expected"):
        workspace.config_save({**original, "content": "[habits"})
    assert file.read_text() == original["content"]
    file.write_text('[habits]\n1 = "Exercise"\n')
    with pytest.raises(WebError, match="changed"):
        workspace.config_save({**original, "content": '[habits]\n1 = "Sleep"\n'})
    refreshed = workspace.config_read({"file": "habits"})
    result = workspace.config_save({**refreshed, "content": '[habits]\n1 = "Sleep"\n'})
    assert result["reloaded"] and '"Sleep"' in file.read_text()


def test_configuration_file_selection_cannot_escape_root():
    with pytest.raises(WebError, match="unknown"):
        workspace._file_path("../credentials")


def test_task_edit_retains_metadata_and_removal_status(monkeypatch, tmp_path):
    from schedule_management.web import services
    import schedule_management.data.loaders as loaders
    path = tmp_path / "tasks.json"; log = tmp_path / "tasks.log"
    path.write_text('[{"description":"Write","priority":5,"type":"2","alarm_from":"2026-10-07","added":"saved"}]')
    monkeypatch.setattr(loaders, "TASKS_PATH", path)
    monkeypatch.setattr(loaders, "TASK_LOG_PATH", log)
    monkeypatch.setattr(services, "_load_task_types", lambda: {"1":"Code","2":"Write"})
    task = services.task_update({"originalDescription":"Write","description":"Write better","priority":8})
    assert task["type"] == "2" and task["alarm_from"] == "2026-10-07" and task["added"] == "saved"
    services.task_delete({"description":"Write better","action":"dropped"})
    assert json.loads(log.read_text())[-1]["action"] == "dropped"


def test_missing_schedule_still_opens_browser_setup(monkeypatch, tmp_path):
    from schedule_management.web import services
    import schedule_management.data.loaders as loaders
    monkeypatch.setenv("REMINDER_CONFIG_DIR", str(tmp_path))
    for name in ["TASKS_PATH", "TASK_LOG_PATH", "DDL_PATH", "HABIT_PATH", "RECORD_PATH", "PROCRASTINATE_PATH"]:
        monkeypatch.setattr(loaders, name, tmp_path / name)
    snapshot = services.status_snapshot({})
    assert snapshot["config"]["error"]
    assert snapshot["schedule"]["events"] == []
    assert snapshot["tasks"] == []


def test_renamed_task_keeps_its_history_lifecycle():
    from schedule_management.commands.history import _pair_task_activities
    activities = _pair_task_activities([
        {"action":"added","timestamp":"2026-10-05T09:00:00","task":{"description":"Draft","priority":8}},
        {"action":"updated","timestamp":"2026-10-05T09:30:00","task":{"description":"Final draft","priority":9},"metadata":{"old_task":{"description":"Draft","priority":8}}},
        {"action":"deleted","timestamp":"2026-10-05T10:00:00","task":{"description":"Final draft","priority":9}},
    ])
    assert len(activities) == 1
    assert activities[0]["description"] == "Final draft"
    assert activities[0]["status"] == "completed"
    assert activities[0]["started_at"].hour == 9


def test_type_labels_round_trip_special_characters(tmp_path):
    import tomllib
    from schedule_management.toml_writer import dump_toml
    path = tmp_path / "settings.toml"
    values = {"task_types":{"custom id":"Quote \" and newline\nNext"}}
    dump_toml(values,path)
    assert tomllib.loads(path.read_text()) == values


