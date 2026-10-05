"""Settings TUI navigation, draft cancellation, and visible action coverage."""

from io import StringIO
from unittest.mock import patch

import pytest
import readchar
from rich.console import Console

from schedule_management.commands import settings
from schedule_management.commands.settings import SettingsModel, SettingsTUI, _Mode


@pytest.fixture
def tui(tmp_path, monkeypatch):
    monkeypatch.setattr(settings, "_resolve_llm_config_path", lambda: tmp_path / "llm.toml")
    settings_file = tmp_path / "settings.toml"
    settings_file.write_text(
        '[settings]\nlanguage = "en"\nalarm_interval = 5\n'
        'skip_days = ["monday"]\nshow_tasks_after_change = true\n'
        '\n[tasks]\ndaily_urgent = ["10:00", "20:00"]\n'
        'weekly_review = "monday 20:00"\nmonthly_review = "1 20:00"\n'
        '\n[paths]\n',
        encoding="utf-8",
    )
    return SettingsTUI(SettingsModel(settings_file))


def open_section(tui, section):
    tui.section_cursor = tui._sections_list().index(section)
    tui._handle_key(readchar.key.ENTER)


def open_field(tui, key):
    tui.cursor = next(i for i, row in enumerate(tui.rows) if row.key == key)
    tui._handle_key(readchar.key.ENTER)


@pytest.mark.parametrize("section", ["settings", "llm"])
@pytest.mark.parametrize("back_key", [readchar.key.ESC, readchar.key.BACKSPACE, "\x08", readchar.key.LEFT])
def test_back_keeps_changes_and_section_selection(tui, section, back_key):
    open_section(tui, section)
    tui.model.set("settings", "alarm_interval", 10)

    assert tui._handle_key(back_key) is None
    assert tui.mode == _Mode.BROWSE
    assert tui.browse_level == 0
    assert tui.section_cursor == tui._sections_list().index(section)
    assert tui.model.get("settings", "alarm_interval") == 10
    assert tui.model.dirty


@pytest.mark.parametrize("dirty", [False, True])
def test_escape_at_root_stays_in_settings(tui, dirty):
    if dirty:
        tui.model.set("settings", "alarm_interval", 10)
    assert tui._handle_key(readchar.key.ESC) is None
    assert tui.mode == _Mode.BROWSE
    assert tui.browse_level == 0


@pytest.mark.parametrize("section", [None, "settings", "llm"])
@pytest.mark.parametrize("quit_key", ["q", "e", "x"])
def test_quit_and_legacy_aliases_share_unsaved_guard(tui, section, quit_key):
    if section:
        open_section(tui, section)
    tui.model.set("settings", "alarm_interval", 10)
    assert tui._handle_key(quit_key) is None
    assert tui.mode == _Mode.CONFIRM_QUIT

    # Repeating Quit cannot discard changes; that is a separate action.
    assert tui._handle_key(quit_key) is None
    assert tui.mode == _Mode.CONFIRM_QUIT
    tui._handle_key(readchar.key.ESC)
    assert tui.mode == _Mode.BROWSE
    assert tui.browse_level == int(section is not None)
    assert tui.model.dirty


def test_save_stays_on_page_and_persists_both_files(tui):
    open_section(tui, "settings")
    tui.model.set("settings", "alarm_interval", 10)
    tui.llm_model = "test/model"
    tui.llm_dirty = True

    assert tui._handle_key("s") is None
    assert tui.mode == _Mode.BROWSE
    assert tui.browse_section == "settings"
    assert not tui._is_dirty()
    assert SettingsModel(tui.model.path).get("settings", "alarm_interval") == 10
    assert settings._load_llm_model() == "test/model"


@pytest.mark.parametrize("action", ["s", "d"])
def test_quit_prompt_save_or_discard(tui, action):
    tui.model.set("settings", "alarm_interval", 10)
    tui.llm_model = "test/model"
    tui.llm_dirty = True
    tui._handle_key("q")

    assert tui._handle_key(action) == "quit"
    saved = action == "s"
    assert SettingsModel(tui.model.path).get("settings", "alarm_interval") == (10 if saved else 5)
    assert settings._load_llm_model() == ("test/model" if saved else None)


def test_failed_save_keeps_quit_prompt_with_error(tui, monkeypatch):
    tui.model.set("settings", "alarm_interval", 10)
    tui._handle_key("q")

    def fail_save():
        raise OSError("read-only settings")

    monkeypatch.setattr(tui.model, "save", fail_save)
    assert tui._handle_key("s") is None
    assert tui.mode == _Mode.CONFIRM_QUIT
    assert tui.model.dirty
    assert "Save failed: read-only settings" in tui._render().renderable.plain


@pytest.mark.parametrize("key", ["language", "alarm_interval", "skip_days"])
def test_escape_cancels_field_draft(tui, key):
    open_section(tui, "settings")
    old_value = tui.model.get("settings", key)
    open_field(tui, key)
    if key == "language":
        tui._handle_key(readchar.key.DOWN)
    elif key == "skip_days":
        tui._handle_key(" ")
    else:
        tui.edit_buffer = "10"
    tui._handle_key(readchar.key.ESC)

    assert tui.mode == _Mode.BROWSE
    assert tui.browse_section == "settings"
    assert tui.model.get("settings", key) == old_value
    assert not tui.model.dirty


@pytest.mark.parametrize("key", ["weekly_review", "monthly_review"])
def test_compound_editor_next_then_cancel(tui, key):
    open_section(tui, "tasks")
    old_value = tui.model.get("tasks", key)
    open_field(tui, key)
    assert "[Enter] Next" in tui._render().renderable.plain
    tui._handle_key(readchar.key.ENTER)
    assert "[Enter] Apply" in tui._render().renderable.plain
    tui.edit_buffer = "22:00"
    tui._handle_key(readchar.key.ESC)

    assert tui.mode == _Mode.BROWSE
    assert tui.compound_type is None
    assert tui.model.get("tasks", key) == old_value
    assert not tui.model.dirty


@pytest.mark.parametrize("apply_changes", [False, True])
def test_time_list_apply_or_cancel(tui, apply_changes):
    open_section(tui, "tasks")
    open_field(tui, "daily_urgent")
    tui._handle_key(" ")
    assert tui.mode == _Mode.INLINE
    tui.edit_buffer = "11:00"
    tui._handle_key(readchar.key.ENTER)
    tui._handle_key(readchar.key.DOWN)
    tui._handle_key("d")
    tui._handle_key("a")
    tui.edit_buffer = "22:00"
    tui._handle_key(readchar.key.ENTER)
    assert tui.time_list_values == ["11:00", "22:00"]
    assert tui.model.get("tasks", "daily_urgent") == ["10:00", "20:00"]

    tui._handle_key(readchar.key.ENTER if apply_changes else readchar.key.ESC)
    assert tui.mode == _Mode.BROWSE
    assert tui.browse_section == "tasks"
    assert tui.model.get("tasks", "daily_urgent") == (
        ["11:00", "22:00"] if apply_changes else ["10:00", "20:00"]
    )
    assert tui.model.dirty == apply_changes


@pytest.mark.parametrize("empty", [False, True])
def test_cancel_added_time_leaves_no_placeholder(tui, empty):
    if empty:
        tui.model.set("tasks", "daily_urgent", [])
    open_section(tui, "tasks")
    open_field(tui, "daily_urgent")
    old_values = list(tui.time_list_values)
    tui._handle_key("a")
    tui._handle_key(readchar.key.ESC)

    assert tui.mode == _Mode.TIME_LIST
    assert tui.time_list_values == old_values
    assert tui.time_list_cursor == max(0, len(old_values) - 1)
    tui._handle_key(readchar.key.ENTER)
    assert tui.model.get("tasks", "daily_urgent") == old_values


@pytest.mark.parametrize("section", [None, "settings", "llm", "paths"])
@pytest.mark.parametrize("width", [50, 80])
def test_browse_actions_are_visible_and_contextual(tui, section, width):
    if section:
        open_section(tui, section)
    output = StringIO()
    tui.console = Console(file=output, width=width, height=24)
    tui.console.print(tui._render())
    rendered = output.getvalue()
    assert "[q] Quit" in rendered
    assert "[s] Save" in rendered
    assert "Exit" not in rendered
    assert "e/x" not in rendered
    if section:
        assert "[Esc] Back" in rendered
    if section == "paths":
        assert "[a] Add" in rendered
        assert "[Enter] Edit" not in rendered
        assert "[d] Delete" not in rendered


def test_toggle_and_clear_have_one_visible_action(tui):
    open_section(tui, "settings")
    tui.cursor = next(i for i, row in enumerate(tui.rows) if row.key == "show_tasks_after_change")
    footer = tui._render().renderable.plain
    assert "[Space] Toggle" in footer
    assert "[Enter] Edit" not in footer

    tui._go_back_to_sections()
    open_section(tui, "llm")
    tui.cursor = 1
    footer = tui._render().renderable.plain
    assert "[Enter] Clear" in footer
    assert "Edit/Clear" not in footer


@pytest.mark.parametrize("width", [50, 80])
def test_long_values_leave_room_for_navigation(tui, width):
    value = "/very/long/path" * 30
    for index in range(15):
        tui.model.set("paths", f"file_{index}", value)
    open_section(tui, "paths")
    tui.cursor = 14
    output = StringIO()
    tui.console = Console(file=output, width=width, height=24)
    panel = tui._render()

    assert len(tui.console.render_lines(panel, tui.console.options)) <= 24
    tui.console.print(panel)
    assert "[Esc] Back" in output.getvalue()
    assert "[q] Quit" in output.getvalue()
    assert tui.model.get("paths", "file_14") == value


@patch("schedule_management.commands.settings.Live")
@patch("readchar.readkey")
def test_ctrl_c_prompt_keeps_editor_on_cancel(mock_readkey, mock_live, tui):
    tui.model.set("settings", "alarm_interval", 10)
    open_section(tui, "settings")
    open_field(tui, "alarm_interval")
    tui.edit_buffer = "15"
    mock_readkey.side_effect = [
        KeyboardInterrupt(), KeyboardInterrupt(), readchar.key.ESC, KeyboardInterrupt(), "d"
    ]

    assert tui.run() == 0
    panels = [call.args[0] for call in mock_live.return_value.__enter__.return_value.update.call_args_list]
    assert "Edit: alarm_interval" in panels[2].title
    assert "15" in panels[2].renderable.plain
    assert tui._quit_return_mode == _Mode.INLINE
