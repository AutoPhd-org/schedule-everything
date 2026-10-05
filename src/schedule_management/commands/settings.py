"""
Settings TUI - Interactive terminal editor for settings.toml.

Provides a keyboard-driven interface for viewing and editing settings.
Users navigate with arrow keys and modify values through selection
pickers, multi-select checkboxes, toggles, or inline text input.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Any

from rich.console import Console
from rich.live import Live
from rich.panel import Panel
from rich.text import Text

from schedule_management.config_layout import resolve_runtime_paths
from schedule_management.toml_writer import dump_toml, load_toml_raw
from schedule_management.commands.setup_agent.configuration import (
    LLMConfig,
    _resolve_llm_config_path,
    load_llm_config,
    save_llm_config,
)


# =============================================================================
# CONSTANTS
# =============================================================================

WEEKDAYS = (
    "monday", "tuesday", "wednesday", "thursday",
    "friday", "saturday", "sunday",
)

SECTION_LABELS: dict[str, str] = {
    "settings": "⚙️  General Settings",
    "time_blocks": "⏱️  Time Blocks",
    "time_points": "🔔  Notifications",
    "tasks": "📋  Task Scheduling",
    "paths": "📁  File Paths",
    "task_types": "🏷️  Task Types",
    "llm": "🤖  Model Settings",
}

# Synthetic section key for the global llm.toml model-override page.
# It is never present in settings.toml, so it cannot collide with real sections.
LLM_SECTION = "llm"
LLM_CLEAR_KEY = "__clear__"

_TIME_RE = re.compile(r"^(\d{1,2}):(\d{2})$")


def _valid_time(value: str) -> bool:
    m = _TIME_RE.match(value.strip())
    if not m:
        return False
    return 0 <= int(m.group(1)) <= 23 and 0 <= int(m.group(2)) <= 59


def _load_llm_model() -> str | None:
    """Read the optional pi ``--model`` override from the global llm.toml.

    Returns ``None`` when the file is missing, unreadable, or the model is
    unset (meaning: let pi choose). Credentials are owned by pi itself.
    """
    path = _resolve_llm_config_path()
    config = load_llm_config(path)
    if config is None:
        return None
    return config.model


def _save_llm_model(model: str | None) -> None:
    """Persist the optional model override to the global llm.toml."""
    path = _resolve_llm_config_path()
    save_llm_config(path, LLMConfig(model=model))


# =============================================================================
# EDITOR TYPES & FIELD METADATA
# =============================================================================


class EditorType(Enum):
    TOGGLE = "toggle"
    PICKER = "picker"
    MULTI_SELECT = "multi_select"
    NUMBER = "number"
    TEXT = "text"
    TIME = "time"
    TIME_LIST = "time_list"
    WEEKDAY_TIME = "weekday_time"
    DAY_TIME = "day_time"


@dataclass(frozen=True)
class FieldMeta:
    editor: EditorType
    help_text: str = ""
    choices: tuple[str, ...] = ()
    min_val: int | None = None
    max_val: int | None = None


# Explicit metadata for known setting keys.
FIELD_REGISTRY: dict[tuple[str, str], FieldMeta] = {
    ("settings", "sound_file"): FieldMeta(
        EditorType.TEXT, "Path to notification sound file"),
    ("settings", "alarm_interval"): FieldMeta(
        EditorType.NUMBER, "Seconds between alarm repeats", min_val=1, max_val=120),
    ("settings", "max_alarm_duration"): FieldMeta(
        EditorType.NUMBER, "Max alarm duration in seconds", min_val=10, max_val=3600),
    ("settings", "skip_days"): FieldMeta(
        EditorType.MULTI_SELECT, "Days to skip all reminders", choices=WEEKDAYS),
    ("settings", "language"): FieldMeta(
        EditorType.PICKER, "UI language", choices=("en", "zh")),
    ("settings", "show_tasks_after_change"): FieldMeta(
        EditorType.TOGGLE, "Print task list after rmd add/rm"),
    ("tasks", "daily_urgent"): FieldMeta(
        EditorType.TIME_LIST, "Times for urgent task reminders (HH:MM)"),
    ("tasks", "ddl_urgent"): FieldMeta(
        EditorType.TIME_LIST, "Times for deadline reminders (HH:MM)"),
    ("tasks", "daily_summary"): FieldMeta(
        EditorType.TIME, "Time for daily summary (HH:MM)"),
    ("tasks", "habit_prompt"): FieldMeta(
        EditorType.TIME, "Time for habit tracking prompt (HH:MM)"),
    ("tasks", "weekly_review"): FieldMeta(
        EditorType.WEEKDAY_TIME, "Weekday and time for weekly review"),
    ("tasks", "monthly_review"): FieldMeta(
        EditorType.DAY_TIME, "Day-of-month and time for monthly review"),
}

# Fallback metadata for sections with user-defined keys.
SECTION_FALLBACKS: dict[str, FieldMeta] = {
    "time_blocks": FieldMeta(EditorType.NUMBER, "Duration in minutes", min_val=1, max_val=480),
    "time_points": FieldMeta(EditorType.TEXT, "Notification message"),
    "paths": FieldMeta(EditorType.TEXT, "File or directory path"),
    "task_types": FieldMeta(EditorType.TEXT, "Task type name"),
}

DEFAULT_TASK_TYPES: dict[str, str] = {
    "1": "read papers",
    "2": "gym work",
    "3": "coding",
    "4": "other",
}


def _get_meta(section: str, key: str) -> FieldMeta:
    meta = FIELD_REGISTRY.get((section, key))
    if meta is not None:
        return meta
    return SECTION_FALLBACKS.get(section, FieldMeta(EditorType.TEXT))


# =============================================================================
# ROW MODEL
# =============================================================================


@dataclass(frozen=True)
class Row:
    """A single UI row — either a section header or a key-value entry."""
    section: str
    key: str | None = None

    @property
    def is_header(self) -> bool:
        return self.key is None


# =============================================================================
# DATA MODEL
# =============================================================================


class SettingsModel:
    """Loads, mutates, and persists settings.toml data."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self.data: dict[str, dict[str, Any]] = {}
        self.dirty = False
        self.load()

    def load(self) -> None:
        self.data = load_toml_raw(self.path)
        if "task_types" not in self.data:
            self.data["task_types"] = dict(DEFAULT_TASK_TYPES)
        self.dirty = False

    def save(self) -> None:
        dump_toml(self.data, self.path)
        self.dirty = False

    def get(self, section: str, key: str) -> Any:
        return self.data.get(section, {}).get(key)

    def set(self, section: str, key: str, value: Any) -> None:
        if section not in self.data:
            self.data[section] = {}
        if self.data[section].get(key) != value:
            self.data[section][key] = value
            self.dirty = True

    def delete(self, section: str, key: str) -> bool:
        if section in self.data and key in self.data[section]:
            del self.data[section][key]
            self.dirty = True
            return True
        return False

    def add_key(self, section: str, key: str) -> bool:
        """Add a key with a sensible default.  Returns False if it exists."""
        if section in self.data and key in self.data[section]:
            return False
        if section not in self.data:
            self.data[section] = {}
        meta = _get_meta(section, key)
        default: Any
        if meta.editor == EditorType.NUMBER:
            default = meta.min_val or 0
        elif meta.editor == EditorType.TOGGLE:
            default = False
        elif meta.editor in (EditorType.TIME_LIST, EditorType.MULTI_SELECT):
            default = []
        else:
            default = ""
        self.data[section][key] = default
        self.dirty = True
        return True

    def sections(self) -> list[str]:
        return list(self.data.keys())

    def keys_in(self, section: str) -> list[str]:
        return list(self.data.get(section, {}).keys())


# =============================================================================
# TUI STATE
# =============================================================================


class _Mode(Enum):
    BROWSE = "browse"
    PICKER = "picker"
    MULTI_SELECT = "multi_select"
    INLINE = "inline"
    TIME_LIST = "time_list"
    CONFIRM_QUIT = "confirm_quit"
    ADD_KEY = "add_key"
    LLM_EDIT = "llm_edit"


# =============================================================================
# TUI
# =============================================================================


class SettingsTUI:
    """Interactive terminal UI for editing settings."""

    def __init__(self, model: SettingsModel) -> None:
        self.model = model
        self.console = Console()

        # Navigation
        self.rows: list[Row] = []
        self.cursor = 0
        self.scroll_offset = 0
        self.mode = _Mode.BROWSE
        self._quit_return_mode = _Mode.BROWSE
        self.message = ""

        # Drill-down browse state: level 0 = sections, level 1 = keys
        self.browse_level = 0
        self.browse_section = ""
        self.section_cursor = 0

        # Picker / multi-select state
        self.picker_choices: list[str] = []
        self.picker_cursor = 0
        self.picker_selected: set[int] = set()

        # Inline editor state
        self.edit_buffer = ""

        # Time-list editor state
        self.time_list_values: list[str] = []
        self.time_list_cursor = 0
        self.time_list_editing = False  # True when editing an item inline
        self.time_list_adding = False

        # Compound editor state (weekday_time / day_time)
        self.compound_type: str | None = None   # "weekday_time" or "day_time"
        self.compound_step = 0
        self.compound_partial = ""
        self._compound_time_default = "20:00"

        # Row being edited
        self.editing_row: Row = Row("")

        # Add-key target section
        self._add_section = ""

        # Model Settings page state (backed by the global llm.toml, not
        # SettingsModel). llm_model is the optional pi --model override;
        # None means "let pi choose". llm_dirty tracks unsaved changes.
        self.llm_model: str | None = _load_llm_model()
        self._saved_llm_model = self.llm_model
        self.llm_dirty = False

        self._build_rows()

    # --------------------------------------------------------------------- #
    # Row management
    # --------------------------------------------------------------------- #

    def _build_rows(self) -> None:
        if self.browse_level == 0:
            self.rows = []
            return
        self.rows = []
        section = self.browse_section
        for key in self.model.keys_in(section):
            self.rows.append(Row(section, key))
        self._clamp_cursor()

    def _sections_list(self) -> list[str]:
        real = self.model.sections()
        # Always surface the synthetic Model Settings page at the top.
        if LLM_SECTION in real:
            return real
        return [LLM_SECTION, *real]

    def _move_section(self, delta: int) -> None:
        sections = self._sections_list()
        if not sections:
            return
        self.section_cursor = max(0, min(len(sections) - 1, self.section_cursor + delta))

    def _drill_into_section(self) -> None:
        sections = self._sections_list()
        if not sections or self.section_cursor >= len(sections):
            return
        self.browse_section = sections[self.section_cursor]
        self.browse_level = 1
        self.cursor = 0
        self.scroll_offset = 0
        if self.browse_section == LLM_SECTION:
            self._build_llm_rows()
        else:
            self._build_rows()

    def _build_llm_rows(self) -> None:
        """Rows for the synthetic Model Settings page (not from settings.toml)."""
        self.rows = [Row(LLM_SECTION, "model"), Row(LLM_SECTION, LLM_CLEAR_KEY)]
        self._clamp_cursor()

    def _go_back_to_sections(self) -> None:
        sections = self._sections_list()
        self.browse_level = 0
        if self.browse_section in sections:
            self.section_cursor = sections.index(self.browse_section)
        self.browse_section = ""
        self.rows = []

    def _is_dirty(self) -> bool:
        return self.model.dirty or self.llm_dirty

    def _save_all(self) -> None:
        """Persist both settings.toml and the global llm.toml when dirty."""
        if self.model.dirty:
            self.model.save()
        if self.llm_dirty:
            _save_llm_model(self.llm_model)
            self._saved_llm_model = self.llm_model
            self.llm_dirty = False

    def _nav_indices(self) -> list[int]:
        return [i for i, r in enumerate(self.rows) if not r.is_header]

    def _clamp_cursor(self) -> None:
        nav = self._nav_indices()
        if not nav:
            self.cursor = 0
            return
        if self.cursor in nav:
            return
        for idx in nav:
            if idx >= self.cursor:
                self.cursor = idx
                return
        self.cursor = nav[-1]

    def _move(self, delta: int) -> None:
        nav = self._nav_indices()
        if not nav:
            return
        try:
            ci = nav.index(self.cursor)
        except ValueError:
            ci = 0
        ci = max(0, min(len(nav) - 1, ci + delta))
        self.cursor = nav[ci]

    def _current_row(self) -> Row:
        if 0 <= self.cursor < len(self.rows):
            return self.rows[self.cursor]
        return Row("")

    # --------------------------------------------------------------------- #
    # Value formatting for display
    # --------------------------------------------------------------------- #

    @staticmethod
    def _fmt_val(value: Any) -> str:
        if isinstance(value, bool):
            return "✓ yes" if value else "✗ no"
        if isinstance(value, list):
            return ", ".join(str(v) for v in value) if value else "(empty)"
        return str(value)

    # --------------------------------------------------------------------- #
    # Rendering
    # --------------------------------------------------------------------- #

    @staticmethod
    def _append_actions(
        text: Text,
        primary: tuple[tuple[str, str], ...],
        secondary: tuple[tuple[str, str], ...] = (),
    ) -> None:
        """Show each action once, with local actions above navigation."""
        for index, actions in enumerate((primary, secondary)):
            if not actions:
                continue
            text.append("\n\n  " if index == 0 else "\n  ")
            for action_index, (shortcut, label) in enumerate(actions):
                if action_index:
                    text.append("  ")
                text.append(f"[{shortcut}]", style="cyan")
                text.append(f" {label}", style="dim")

    def _render(self) -> Panel:
        match self.mode:
            case _Mode.BROWSE:
                return self._render_browse()
            case _Mode.PICKER:
                return self._render_picker()
            case _Mode.MULTI_SELECT:
                return self._render_multi_select()
            case _Mode.INLINE | _Mode.ADD_KEY | _Mode.LLM_EDIT:
                return self._render_inline()
            case _Mode.TIME_LIST:
                return self._render_time_list()
            case _Mode.CONFIRM_QUIT:
                return self._render_confirm_quit()
        return self._render_browse()  # fallback

    # ---- Browse --------------------------------------------------------- #

    def _render_browse(self) -> Panel:
        if self.browse_level == 0:
            return self._render_sections_view()
        if self.browse_section == LLM_SECTION:
            return self._render_llm_view()
        return self._render_keys_view()

    def _render_sections_view(self) -> Panel:
        sections = self._sections_list()
        th = self.console.size.height
        viewport = max(1, th - 12)

        if self.section_cursor < self.scroll_offset:
            self.scroll_offset = self.section_cursor
        elif self.section_cursor >= self.scroll_offset + viewport:
            self.scroll_offset = self.section_cursor - viewport + 1

        t = Text()
        end = min(len(sections), self.scroll_offset + viewport)
        for idx in range(self.scroll_offset, end):
            section = sections[idx]
            sel = idx == self.section_cursor
            prefix = "  ▸ " if sel else "    "
            label = SECTION_LABELS.get(section, section)
            if section == LLM_SECTION:
                detail = "(global)"
            else:
                detail = f"({len(self.model.keys_in(section))} keys)"
            style = "bold cyan" if sel else "cyan"
            t.append(prefix, style=style)
            t.append(label, style=style)
            t.append(f"  {detail}\n", style="dim")

        if self.scroll_offset > 0:
            t.append("    ↑ more above\n", style="dim italic")
        if end < len(sections):
            t.append("    ↓ more below\n", style="dim italic")

        self._append_actions(
            t,
            (("↑↓", "Move"), ("Enter", "Open")),
            (("s", "Save"), ("q", "Quit")),
        )

        if self.message:
            t.append(f"\n\n  {self.message}", style="green")

        title = "⚙  Settings"
        if self._is_dirty():
            title += "  •  modified"
        return Panel(t, title=title, border_style="bright_blue", padding=(0, 1))

    def _render_keys_view(self) -> Panel:
        th = self.console.size.height
        viewport = max(1, th - 14)

        if self.cursor < self.scroll_offset:
            self.scroll_offset = self.cursor
        elif self.cursor >= self.scroll_offset + viewport:
            self.scroll_offset = self.cursor - viewport + 1

        t = Text()
        section_label = SECTION_LABELS.get(self.browse_section, self.browse_section)
        t.append(f"  ← {section_label}\n\n", style="bold cyan")

        end = min(len(self.rows), self.scroll_offset + viewport)
        for idx in range(self.scroll_offset, end):
            row = self.rows[idx]
            sel = idx == self.cursor
            prefix = "  ▸ " if sel else "    "
            val = self.model.get(row.section, row.key)
            fv = self._fmt_val(val)
            kstyle = "bold white" if sel else "white"
            vstyle = "bold yellow" if sel else "dim"
            assert row.key is not None
            line = Text(prefix, style=kstyle)
            line.append(f"{row.key:<26s}", style=kstyle)
            line.append(fv, style=vstyle)
            # Keep data rows compact while allowing the action footer to wrap.
            line.truncate(max(1, self.console.size.width - 4), overflow="ellipsis")
            t.append_text(line)
            t.append("\n")

        if not self.rows:
            t.append("    (no keys in this section)\n", style="dim")

        if self.scroll_offset > 0:
            t.append("    ↑ more above\n", style="dim italic")
        if end < len(self.rows):
            t.append("    ↓ more below\n", style="dim italic")

        row = self._current_row()
        if not row.is_header and row.key:
            meta = _get_meta(row.section, row.key)
            if meta.help_text:
                t.append(f"\n  ℹ  {meta.help_text}", style="dim italic")

        edit_action = ("Enter", "Edit")
        if row.key and _get_meta(row.section, row.key).editor == EditorType.TOGGLE:
            edit_action = ("Space", "Toggle")
        local_actions = (
            (("↑↓", "Move"), edit_action, ("a", "Add"), ("d", "Delete"))
            if self.rows else (("a", "Add"),)
        )
        self._append_actions(
            t,
            local_actions,
            (("Esc", "Back"), ("s", "Save"), ("q", "Quit")),
        )

        if self.message:
            t.append(f"\n\n  {self.message}", style="green")

        title = f"⚙  Settings › {section_label}"
        if self._is_dirty():
            title += "  •  modified"
        return Panel(t, title=title, border_style="bright_blue", padding=(0, 1))

    # ---- Model Settings (global llm.toml) ------------------------------ #

    def _render_llm_view(self) -> Panel:
        t = Text()
        t.append("  ← Model Settings\n\n", style="bold cyan")
        t.append(
            "  pi owns credentials and model selection. This optional override "
            "is passed straight to\n  `pi --model` (e.g. a provider/model id or "
            "pattern); leave it blank to let pi choose.\n",
            style="dim italic",
        )
        t.append(
            "  Stored in ~/.schedule_management/llm.toml (global, shared across "
            "config sets).\n\n",
            style="dim italic",
        )

        for idx, row in enumerate(self.rows):
            sel = idx == self.cursor
            prefix = "  ▸ " if sel else "    "
            kstyle = "bold white" if sel else "white"
            vstyle = "bold yellow" if sel else "dim"
            t.append(prefix, style=kstyle)
            if row.key == LLM_CLEAR_KEY:
                t.append(f"{'clear':<26s}", style=kstyle)
                t.append("reset to pi default\n", style=vstyle)
            else:
                t.append(f"{row.key:<26s}", style=kstyle)
                value = self.llm_model if self.llm_model else "(pi default)"
                t.append(f"{value}\n", style=vstyle)

        action = "Clear" if self._current_row().key == LLM_CLEAR_KEY else "Edit"
        self._append_actions(
            t,
            (("↑↓", "Move"), ("Enter", action)),
            (("Esc", "Back"), ("s", "Save"), ("q", "Quit")),
        )

        if self.message:
            t.append(f"\n\n  {self.message}", style="green")

        title = "⚙  Settings › Model Settings"
        if self.model.dirty or self.llm_dirty:
            title += "  •  modified"
        return Panel(t, title=title, border_style="bright_blue", padding=(0, 1))

    # ---- Picker --------------------------------------------------------- #

    def _render_picker(self) -> Panel:
        row = self.editing_row
        t = Text()
        for i, choice in enumerate(self.picker_choices):
            sel = i == self.picker_cursor
            prefix = "  ▸ " if sel else "    "
            style = "bold yellow" if sel else ""
            t.append(f"{prefix}{choice}\n", style=style)
        action = "Next" if self.compound_type == "weekday_time" else "Apply"
        self._append_actions(
            t, (("↑↓", "Move"), ("Enter", action)), (("Esc", "Cancel"),)
        )

        label = row.key or ""
        if self.compound_type == "weekday_time" and self.compound_step == 0:
            label += " – select weekday"
        return Panel(t, title=f"  Select: {label}  ", border_style="yellow", padding=(1, 2))

    # ---- Multi-select --------------------------------------------------- #

    def _render_multi_select(self) -> Panel:
        row = self.editing_row
        t = Text()
        for i, choice in enumerate(self.picker_choices):
            sel = i == self.picker_cursor
            check = "✓" if i in self.picker_selected else " "
            prefix = "▸" if sel else " "
            style = "bold yellow" if sel else ""
            t.append(f"  {prefix} [{check}] {choice}\n", style=style)
        self._append_actions(
            t,
            (("↑↓", "Move"), ("Space", "Toggle")),
            (("Enter", "Apply"), ("Esc", "Cancel")),
        )
        return Panel(t, title=f"  Select: {row.key}  ", border_style="yellow", padding=(1, 2))

    # ---- Inline editor -------------------------------------------------- #

    def _render_inline(self) -> Panel:
        row = self.editing_row
        t = Text()

        if self.mode == _Mode.LLM_EDIT:
            current = self.llm_model if self.llm_model else "(pi default)"
            t.append(f"  Current: {current}\n\n", style="dim")
            t.append("  New model (blank = pi default): ", style="white")
            t.append(f"{self.edit_buffer}█\n", style="bold yellow")
            t.append(
                "\n  ℹ  Passed straight to `pi --model` (provider/model id or pattern).",
                style="dim italic",
            )
            if self.message:
                t.append(f"\n  ⚠  {self.message}", style="bold red")
            self._append_actions(t, (("Enter", "Apply"), ("Esc", "Cancel")))
            return Panel(
                t, title="  Edit: model  ", border_style="yellow", padding=(1, 2)
            )

        meta = _get_meta(row.section, row.key or "")

        if self.mode == _Mode.ADD_KEY:
            t.append(f"  Section: [{self._add_section}]\n\n", style="cyan")
            t.append("  Key name: ", style="white")
            t.append(f"{self.edit_buffer}█\n", style="bold yellow")
        else:
            old = self.model.get(row.section, row.key or "")
            t.append(f"  Current: {self._fmt_val(old)}\n\n", style="dim")

            label = "New value"
            if self.compound_type == "weekday_time" and self.compound_step == 1:
                label = f"Time (for {self.compound_partial})"
            elif self.compound_type == "day_time" and self.compound_step == 0:
                label = "Day of month (1-31)"
            elif self.compound_type == "day_time" and self.compound_step == 1:
                label = f"Time (for day {self.compound_partial})"

            t.append(f"  {label}: ", style="white")
            t.append(f"{self.edit_buffer}█\n", style="bold yellow")

            if meta.help_text:
                hint = meta.help_text
                if meta.min_val is not None and meta.max_val is not None:
                    hint += f" ({meta.min_val}–{meta.max_val})"
                t.append(f"\n  ℹ  {hint}", style="dim italic")

        if self.message:
            t.append(f"\n  ⚠  {self.message}", style="bold red")

        action = "Apply"
        if self.mode == _Mode.ADD_KEY:
            action = "Add"
        elif self.compound_type == "day_time" and self.compound_step == 0:
            action = "Next"
        self._append_actions(t, (("Enter", action), ("Esc", "Cancel")))
        title = f"  Edit: {row.key or 'new key'}  " if self.mode != _Mode.ADD_KEY else "  Add Key  "
        return Panel(t, title=title, border_style="yellow", padding=(1, 2))

    # ---- Time-list editor ----------------------------------------------- #

    def _render_time_list(self) -> Panel:
        row = self.editing_row
        t = Text()
        if not self.time_list_values:
            t.append("  (no times configured)\n", style="dim")
        else:
            for i, v in enumerate(self.time_list_values):
                sel = i == self.time_list_cursor
                prefix = "  ▸ " if sel else "    "
                style = "bold yellow" if sel else ""
                t.append(f"{prefix}{v}\n", style=style)
        local_actions = (
            (("↑↓", "Move"), ("Space", "Edit"), ("a", "Add"), ("d", "Delete"))
            if self.time_list_values else (("a", "Add"),)
        )
        self._append_actions(
            t,
            local_actions,
            (("Enter", "Apply"), ("Esc", "Cancel")),
        )
        return Panel(t, title=f"  Edit: {row.key}  ", border_style="yellow", padding=(1, 2))

    # ---- Confirm quit --------------------------------------------------- #

    def _render_confirm_quit(self) -> Panel:
        t = Text()
        t.append("  You have unsaved changes.\n\n", style="bold white")
        t.append("  [s] Save and quit\n", style="green")
        t.append("  [d] Discard and quit\n", style="red")
        t.append("  [Esc] Keep editing\n", style="dim")
        if self.message:
            t.append(f"\n  {self.message}", style="bold red")
        return Panel(t, title="  Unsaved Changes  ", border_style="red", padding=(1, 2))

    # --------------------------------------------------------------------- #
    # Key handling
    # --------------------------------------------------------------------- #

    def _handle_key(self, key: str) -> str | None:
        """Dispatch key to the active mode handler.  Return 'quit' to exit."""
        import readchar as rc

        self.message = ""
        match self.mode:
            case _Mode.BROWSE:
                return self._on_browse(key, rc)
            case _Mode.PICKER:
                return self._on_picker(key, rc)
            case _Mode.MULTI_SELECT:
                return self._on_multi_select(key, rc)
            case _Mode.INLINE | _Mode.LLM_EDIT:
                return self._on_inline(key, rc)
            case _Mode.ADD_KEY:
                return self._on_add_key(key, rc)
            case _Mode.TIME_LIST:
                return self._on_time_list(key, rc)
            case _Mode.CONFIRM_QUIT:
                return self._on_confirm_quit(key, rc)
        return None

    # ---- Browse --------------------------------------------------------- #

    def _on_browse(self, key: str, rc: Any) -> str | None:
        if key == "s":
            try:
                self._save_all()
                self.message = "✅ Settings saved"
            except Exception as exc:
                self.message = f"❌ Save failed: {exc}"
            return None
        # Keep the old exit shortcuts as unadvertised aliases for Quit.
        if key in ("q", "e", "x"):
            return self._request_quit()
        if key in (rc.key.ESC, rc.key.BACKSPACE, "\x7f", "\x08", rc.key.LEFT):
            if self.browse_level == 1:
                self._go_back_to_sections()
            return None
        if self.browse_level == 0:
            return self._on_sections_view(key, rc)
        if self.browse_section == LLM_SECTION:
            return self._on_llm_view(key, rc)
        return self._on_keys_view(key, rc)

    def _request_quit(self) -> str | None:
        if not self._is_dirty():
            return "quit"
        if self.mode != _Mode.CONFIRM_QUIT:
            self._quit_return_mode = self.mode
        self.mode = _Mode.CONFIRM_QUIT
        return None

    def _on_sections_view(self, key: str, rc: Any) -> str | None:
        if key == rc.key.UP:
            self._move_section(-1)
        elif key == rc.key.DOWN:
            self._move_section(1)
        elif key in (rc.key.ENTER, "\r", "\n"):
            self._drill_into_section()
        return None

    def _on_llm_view(self, key: str, rc: Any) -> str | None:
        if key == rc.key.UP:
            self._move(-1)
        elif key == rc.key.DOWN:
            self._move(1)
        elif key in (rc.key.ENTER, "\r", "\n"):
            row = self._current_row()
            if row.key == LLM_CLEAR_KEY:
                self.llm_model = None
                self.llm_dirty = self.llm_model != self._saved_llm_model
                self.message = "Model cleared (pi will use its default)"
            else:
                self.editing_row = row
                self.edit_buffer = self.llm_model or ""
                self.mode = _Mode.LLM_EDIT
        return None

    def _on_keys_view(self, key: str, rc: Any) -> str | None:
        if key == rc.key.UP:
            self._move(-1)
        elif key == rc.key.DOWN:
            self._move(1)
        elif key in (rc.key.ENTER, "\r", "\n"):
            self._start_edit()
        elif key == " ":
            row = self._current_row()
            if not row.is_header and row.key:
                meta = _get_meta(row.section, row.key)
                if meta.editor == EditorType.TOGGLE:
                    cur = self.model.get(row.section, row.key)
                    self.model.set(row.section, row.key, not bool(cur))
                    self.message = f"Toggled {row.key}"
                else:
                    self._start_edit()
        elif key == "d":
            row = self._current_row()
            if not row.is_header and row.key:
                self.model.delete(row.section, row.key)
                self._build_rows()
                self.message = f"Deleted {row.key}"
        elif key == "a":
            row = self._current_row()
            self._add_section = row.section if row.key else self.browse_section
            if self._add_section == "task_types":
                existing = self.model.keys_in("task_types")
                max_num = 0
                for k in existing:
                    try:
                        max_num = max(max_num, int(k))
                    except ValueError:
                        pass
                self.edit_buffer = str(max_num + 1)
            else:
                self.edit_buffer = ""
            self.mode = _Mode.ADD_KEY
        return None

    # ---- Picker --------------------------------------------------------- #

    def _on_picker(self, key: str, rc: Any) -> str | None:
        if key == rc.key.UP:
            self.picker_cursor = max(0, self.picker_cursor - 1)
        elif key == rc.key.DOWN:
            self.picker_cursor = min(len(self.picker_choices) - 1, self.picker_cursor + 1)
        elif key in (rc.key.ENTER, "\r", "\n"):
            selected = self.picker_choices[self.picker_cursor]
            if self.compound_type == "weekday_time":
                # Step 1 done → move to time input
                self.compound_partial = selected
                self.compound_step = 1
                self.mode = _Mode.INLINE
                self.edit_buffer = self._compound_time_default
            else:
                row = self.editing_row
                self.model.set(row.section, row.key or "", selected)
                self.mode = _Mode.BROWSE
                self.message = f"Set {row.key} = {selected}"
        elif key in ("\x1b", rc.key.BACKSPACE, "\x7f", "\x08", rc.key.LEFT):
            self.mode = _Mode.BROWSE
            self.compound_type = None
        return None

    # ---- Multi-select --------------------------------------------------- #

    def _on_multi_select(self, key: str, rc: Any) -> str | None:
        if key == rc.key.UP:
            self.picker_cursor = max(0, self.picker_cursor - 1)
        elif key == rc.key.DOWN:
            self.picker_cursor = min(len(self.picker_choices) - 1, self.picker_cursor + 1)
        elif key == " ":
            if self.picker_cursor in self.picker_selected:
                self.picker_selected.discard(self.picker_cursor)
            else:
                self.picker_selected.add(self.picker_cursor)
        elif key in (rc.key.ENTER, "\r", "\n"):
            chosen = [self.picker_choices[i] for i in sorted(self.picker_selected)]
            row = self.editing_row
            self.model.set(row.section, row.key or "", chosen)
            self.mode = _Mode.BROWSE
            self.message = f"Set {row.key} = {chosen}"
        elif key in ("\x1b", rc.key.BACKSPACE, "\x7f", "\x08", rc.key.LEFT):
            self.mode = _Mode.BROWSE
        return None

    # ---- Inline --------------------------------------------------------- #

    def _on_inline(self, key: str, rc: Any) -> str | None:
        if key in (rc.key.ENTER, "\r", "\n"):
            if self.mode == _Mode.LLM_EDIT:
                return self._confirm_llm_edit()
            return self._confirm_inline()
        elif key == "\x1b":
            if self.time_list_editing:
                self.mode = _Mode.TIME_LIST
                self.time_list_editing = False
                self.time_list_adding = False
                self.time_list_cursor = min(
                    self.time_list_cursor, max(0, len(self.time_list_values) - 1)
                )
            else:
                if self.mode == _Mode.LLM_EDIT:
                    self.browse_section = LLM_SECTION
                    self.browse_level = 1
                    self._build_llm_rows()
                self.mode = _Mode.BROWSE
                self.compound_type = None
            return None
        elif key in (rc.key.BACKSPACE, "\x7f", "\x08"):
            if self.edit_buffer:
                self.edit_buffer = self.edit_buffer[:-1]
        elif len(key) == 1 and key.isprintable():
            self.edit_buffer += key
        return None

    def _confirm_llm_edit(self) -> str | None:
        raw = self.edit_buffer.strip()
        self.llm_model = raw or None
        self.llm_dirty = self.llm_model != self._saved_llm_model
        self.mode = _Mode.BROWSE
        self.browse_section = LLM_SECTION
        self.browse_level = 1
        self._build_llm_rows()
        if raw:
            self.message = f"Set model = {raw}"
        else:
            self.message = "Model cleared (pi will use its default)"
        return None

    def _confirm_inline(self) -> str | None:
        raw = self.edit_buffer.strip()
        row = self.editing_row

        # Compound: weekday_time step 2
        if self.compound_type == "weekday_time" and self.compound_step == 1:
            if not _valid_time(raw):
                self.message = "Invalid time (use HH:MM)"
                return None
            combined = f"{self.compound_partial} {raw}"
            self.model.set(row.section, row.key or "", combined)
            self.mode = _Mode.BROWSE
            self.compound_type = None
            self.message = f"Set {row.key} = {combined}"
            return None

        # Compound: day_time step 1 → day number
        if self.compound_type == "day_time" and self.compound_step == 0:
            try:
                day = int(raw)
                if not 1 <= day <= 31:
                    raise ValueError
            except ValueError:
                self.message = "Day must be between 1 and 31"
                return None
            self.compound_partial = str(day)
            self.compound_step = 1
            self.edit_buffer = self._compound_time_default
            return None

        # Compound: day_time step 2 → time
        if self.compound_type == "day_time" and self.compound_step == 1:
            if not _valid_time(raw):
                self.message = "Invalid time (use HH:MM)"
                return None
            combined = f"{self.compound_partial} {raw}"
            self.model.set(row.section, row.key or "", combined)
            self.mode = _Mode.BROWSE
            self.compound_type = None
            self.message = f"Set {row.key} = {combined}"
            return None

        # Time-list item edit
        if self.time_list_editing:
            if not _valid_time(raw):
                self.message = "Invalid time (use HH:MM)"
                return None
            if self.time_list_adding:
                self.time_list_values.append(raw)
            else:
                self.time_list_values[self.time_list_cursor] = raw
            self.mode = _Mode.TIME_LIST
            self.time_list_editing = False
            self.time_list_adding = False
            return None

        # Normal field
        meta = _get_meta(row.section, row.key or "")
        if row.section == "task_types" and not raw:
            self.message = "Task type name cannot be empty"
            return None
        validated = self._validate(meta, raw)
        if validated is None:
            return None
        self.model.set(row.section, row.key or "", validated)
        self.mode = _Mode.BROWSE
        self.message = f"Set {row.key} = {validated}"
        return None

    def _validate(self, meta: FieldMeta, raw: str) -> Any | None:
        if meta.editor == EditorType.NUMBER:
            try:
                v = int(raw)
            except ValueError:
                self.message = "Enter a valid integer"
                return None
            if meta.min_val is not None and v < meta.min_val:
                self.message = f"Minimum is {meta.min_val}"
                return None
            if meta.max_val is not None and v > meta.max_val:
                self.message = f"Maximum is {meta.max_val}"
                return None
            return v
        if meta.editor == EditorType.TIME:
            if not _valid_time(raw):
                self.message = "Invalid time (use HH:MM)"
                return None
            return raw
        return raw  # text / path

    # ---- Add key -------------------------------------------------------- #

    def _on_add_key(self, key: str, rc: Any) -> str | None:
        if key in (rc.key.ENTER, "\r", "\n"):
            name = self.edit_buffer.strip()
            if not name:
                self.message = "Key name cannot be empty"
                return None
            if not self.model.add_key(self._add_section, name):
                self.message = f"Key '{name}' already exists"
                self.mode = _Mode.BROWSE
                return None
            self._build_rows()
            # Move cursor to the new key
            for i, r in enumerate(self.rows):
                if r.section == self._add_section and r.key == name:
                    self.cursor = i
                    break
            if self._add_section == "task_types":
                self._start_edit()
            else:
                self.mode = _Mode.BROWSE
                self.message = f"Added {name} — press Enter to set its value"
            return None
        elif key == "\x1b":
            self.mode = _Mode.BROWSE
            return None
        elif key in (rc.key.BACKSPACE, "\x7f", "\x08"):
            if self.edit_buffer:
                self.edit_buffer = self.edit_buffer[:-1]
        elif len(key) == 1 and key.isprintable():
            self.edit_buffer += key
        return None

    # ---- Time-list ------------------------------------------------------ #

    def _on_time_list(self, key: str, rc: Any) -> str | None:
        if key == rc.key.UP:
            if self.time_list_values:
                self.time_list_cursor = max(0, self.time_list_cursor - 1)
        elif key == rc.key.DOWN:
            if self.time_list_values:
                self.time_list_cursor = min(
                    len(self.time_list_values) - 1, self.time_list_cursor + 1
                )
        elif key == " ":
            if self.time_list_values:
                self.edit_buffer = self.time_list_values[self.time_list_cursor]
                self.time_list_editing = True
                self.time_list_adding = False
                self.mode = _Mode.INLINE
        elif key == "a":
            self.time_list_cursor = len(self.time_list_values)
            self.edit_buffer = "00:00"
            self.time_list_editing = True
            self.time_list_adding = True
            self.mode = _Mode.INLINE
        elif key == "d":
            if self.time_list_values:
                self.time_list_values.pop(self.time_list_cursor)
                if self.time_list_cursor >= len(self.time_list_values):
                    self.time_list_cursor = max(0, len(self.time_list_values) - 1)
        elif key in (rc.key.ENTER, "\r", "\n"):
            row = self.editing_row
            self.model.set(row.section, row.key or "", list(self.time_list_values))
            self.mode = _Mode.BROWSE
            self.message = f"Updated {row.key}"
        elif key in (rc.key.ESC, rc.key.BACKSPACE, "\x7f", "\x08", rc.key.LEFT):
            self.mode = _Mode.BROWSE
        return None

    # ---- Confirm quit --------------------------------------------------- #

    def _on_confirm_quit(self, key: str, rc: Any) -> str | None:
        if key == "s":
            try:
                self._save_all()
            except Exception as exc:
                self.message = f"❌ Save failed: {exc}"
                return None
            return "quit"
        elif key == "d":
            return "quit"
        elif key == rc.key.ESC:
            self.mode = self._quit_return_mode
        return None

    # --------------------------------------------------------------------- #
    # Start editing the current row
    # --------------------------------------------------------------------- #

    def _start_edit(self) -> None:
        row = self._current_row()
        if row.is_header or not row.key:
            return
        meta = _get_meta(row.section, row.key)
        value = self.model.get(row.section, row.key)
        self.editing_row = row
        self.compound_type = None
        self.time_list_editing = False
        self.time_list_adding = False

        match meta.editor:
            case EditorType.TOGGLE:
                self.model.set(row.section, row.key, not bool(value))
                self.message = f"Toggled {row.key}"

            case EditorType.PICKER:
                self.mode = _Mode.PICKER
                self.picker_choices = list(meta.choices)
                try:
                    self.picker_cursor = self.picker_choices.index(str(value))
                except ValueError:
                    self.picker_cursor = 0

            case EditorType.MULTI_SELECT:
                self.mode = _Mode.MULTI_SELECT
                self.picker_choices = list(meta.choices)
                cur = value if isinstance(value, list) else []
                self.picker_selected = {
                    i for i, c in enumerate(self.picker_choices) if c in cur
                }
                self.picker_cursor = 0

            case EditorType.NUMBER | EditorType.TEXT | EditorType.TIME:
                self.mode = _Mode.INLINE
                self.edit_buffer = str(value) if value is not None else ""
                self.time_list_editing = False

            case EditorType.TIME_LIST:
                self.mode = _Mode.TIME_LIST
                self.time_list_values = list(value) if isinstance(value, list) else []
                self.time_list_cursor = 0

            case EditorType.WEEKDAY_TIME:
                parts = str(value).split() if value else ["monday", "20:00"]
                self.mode = _Mode.PICKER
                self.picker_choices = list(WEEKDAYS)
                try:
                    self.picker_cursor = WEEKDAYS.index(parts[0].lower())
                except (ValueError, IndexError):
                    self.picker_cursor = 0
                self.compound_type = "weekday_time"
                self.compound_step = 0
                self._compound_time_default = parts[1] if len(parts) > 1 else "20:00"

            case EditorType.DAY_TIME:
                parts = str(value).split() if value else ["1", "20:00"]
                self.mode = _Mode.INLINE
                self.edit_buffer = parts[0] if parts else "1"
                self.compound_type = "day_time"
                self.compound_step = 0
                self._compound_time_default = parts[1] if len(parts) > 1 else "20:00"
                self.time_list_editing = False

    # --------------------------------------------------------------------- #
    # Main loop
    # --------------------------------------------------------------------- #

    def run(self) -> int:
        import readchar

        with Live(
            self._render(),
            console=self.console,
            screen=True,
            auto_refresh=False,
        ) as live:
            while True:
                try:
                    key = readchar.readkey()
                except KeyboardInterrupt:
                    if self._request_quit() == "quit":
                        break
                    self.message = ""
                    live.update(self._render(), refresh=True)
                    continue
                result = self._handle_key(key)
                if result == "quit":
                    break
                live.update(self._render(), refresh=True)
        return 0


# =============================================================================
# CLI ENTRY POINT
# =============================================================================


def settings_command(args: Any) -> int:
    """CLI handler for ``rmd settings``."""
    try:
        import readchar  # noqa: F401 – verify availability
    except ImportError:
        print("❌ The 'readchar' package is required for the settings editor.")
        print("   Install with: uv pip install readchar")
        return 1

    if not sys.stdin.isatty():
        print("❌ Settings editor requires an interactive terminal.")
        return 1

    paths = resolve_runtime_paths()
    settings_path = paths.settings_path

    if not settings_path.exists():
        print(f"❌ Settings file not found: {settings_path}")
        print("   Run 'rmd setup' to create a configuration first.")
        return 1

    try:
        model = SettingsModel(settings_path)
    except Exception as exc:
        print(f"❌ Failed to load settings: {exc}")
        return 1

    tui = SettingsTUI(model)
    return tui.run()
