//! Interactive settings.toml editor (`rmd settings`).
//!
//! Users browse sections, then keys, and change values with a picker,
//! multi-select checkboxes, a toggle, a time-list editor, or inline text
//! input. State transitions ([`SettingsTui::handle_key`]) are separate from
//! drawing ([`SettingsTui::render`] builds a [`Panel`] that is then painted
//! with ratatui), so navigation is tested without a terminal.

#[cfg(test)]
mod tests;
mod text;

use std::collections::BTreeSet;
use std::io::{self, IsTerminal, Write};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use serde_json::Value;

use crate::commands::settings::{
    EditorType, FieldMeta, Row, SettingsModel, WEEKDAYS, get_meta, section_label, valid_time,
};
use crate::i18n::tf;
use crate::py;
pub use text::{Panel, Text};

// Rich styles used by the Python editor.
const PLAIN: Style = Style::new();
const CYAN: Style = Style::new().fg(Color::Cyan);
const BOLD_CYAN: Style = CYAN.add_modifier(Modifier::BOLD);
const DIM: Style = Style::new().add_modifier(Modifier::DIM);
const DIM_ITALIC: Style = Style::new().add_modifier(Modifier::DIM.union(Modifier::ITALIC));
const GREEN: Style = Style::new().fg(Color::Green);
const WHITE: Style = Style::new().fg(Color::White);
const BOLD_WHITE: Style = WHITE.add_modifier(Modifier::BOLD);
const BOLD_YELLOW: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
const RED: Style = Style::new().fg(Color::Red);
const BOLD_RED: Style = RED.add_modifier(Modifier::BOLD);

const INVALID_TIME: &str = "Invalid time (use HH:MM)";

/// A key press, as the editor tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    /// Enter / Return (also Ctrl+J, Ctrl+M).
    Enter,
    /// Backspace (also Ctrl+H).
    Backspace,
    Esc,
    /// A printable character, including space.
    Char(char),
    /// Anything else (function keys, control characters, ...).
    Other,
}

/// One terminal event for the main loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Key(Key),
    /// Ctrl+C (the Python editor's `KeyboardInterrupt`).
    Interrupt,
    /// The terminal was resized.
    Redraw,
}

/// What the editor is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Browse,
    Picker,
    MultiSelect,
    Inline,
    TimeList,
    ConfirmQuit,
    AddKey,
}

/// Two-step editors for `weekday HH:MM` and `day HH:MM` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compound {
    WeekdayTime,
    DayTime,
}

/// A value as the editor shows it.
fn fmt_val(value: Option<&Value>) -> String {
    match value {
        None => "None".to_string(),
        Some(Value::Bool(flag)) => if *flag { "✓ yes" } else { "✗ no" }.to_string(),
        Some(Value::Array(items)) if items.is_empty() => "(empty)".to_string(),
        Some(Value::Array(items)) => items.iter().map(py::py_str).collect::<Vec<_>>().join(", "),
        Some(other) => py::py_str(other),
    }
}

/// `str(value).split() if value else default`.
fn compound_parts(value: Option<&Value>, default: [&str; 2]) -> Vec<String> {
    match value {
        Some(value) if py::truthy(value) => py::py_str(value).split_whitespace().map(str::to_string).collect(),
        _ => default.iter().map(|part| part.to_string()).collect(),
    }
}

/// Show each action once, with local actions above navigation.
fn append_actions(text: &mut Text, primary: &[(&str, &str)], secondary: &[(&str, &str)]) {
    for (index, actions) in [primary, secondary].into_iter().enumerate() {
        if actions.is_empty() {
            continue;
        }
        text.append(if index == 0 { "\n\n  " } else { "\n  " }, PLAIN);
        for (position, (shortcut, label)) in actions.iter().enumerate() {
            if position > 0 {
                text.append("  ", PLAIN);
            }
            text.append(&format!("[{shortcut}]"), CYAN);
            text.append(&format!(" {label}"), DIM);
        }
    }
}

/// The interactive settings editor.
pub struct SettingsTui {
    model: SettingsModel,

    // Navigation.
    rows: Vec<Row>,
    cursor: usize,
    scroll_offset: usize,
    mode: Mode,
    quit_return_mode: Mode,
    message: String,

    // Drill-down browsing: level 0 = sections, level 1 = keys of `browse_section`.
    browse_level: u8,
    browse_section: String,
    section_cursor: usize,

    // Picker / multi-select.
    picker_choices: Vec<String>,
    picker_cursor: usize,
    picker_selected: BTreeSet<usize>,

    // Inline editor.
    edit_buffer: String,

    // Time-list editor.
    time_list_values: Vec<Value>,
    time_list_cursor: usize,
    /// Editing one list item in the inline editor.
    time_list_editing: bool,
    time_list_adding: bool,

    // Compound editors (weekday_time / day_time).
    compound_type: Option<Compound>,
    compound_step: u8,
    compound_partial: String,
    compound_time_default: String,

    /// The row being edited.
    editing_row: Row,
    /// Section receiving a new key.
    add_section: String,
}

fn no_row() -> Row {
    Row { section: String::new(), key: None }
}

impl SettingsTui {
    pub fn new(model: SettingsModel) -> Self {
        let mut tui = Self {
            model,
            rows: Vec::new(),
            cursor: 0,
            scroll_offset: 0,
            mode: Mode::Browse,
            quit_return_mode: Mode::Browse,
            message: String::new(),
            browse_level: 0,
            browse_section: String::new(),
            section_cursor: 0,
            picker_choices: Vec::new(),
            picker_cursor: 0,
            picker_selected: BTreeSet::new(),
            edit_buffer: String::new(),
            time_list_values: Vec::new(),
            time_list_cursor: 0,
            time_list_editing: false,
            time_list_adding: false,
            compound_type: None,
            compound_step: 0,
            compound_partial: String::new(),
            compound_time_default: "20:00".to_string(),
            editing_row: no_row(),
            add_section: String::new(),
        };
        tui.build_rows();
        tui
    }

    // --------------------------------------------------------------------- //
    // Row management
    // --------------------------------------------------------------------- //

    fn build_rows(&mut self) {
        self.rows.clear();
        if self.browse_level == 0 {
            return;
        }
        let section = self.browse_section.clone();
        self.rows = self
            .model
            .keys_in(&section)
            .into_iter()
            .map(|key| Row { section: section.clone(), key: Some(key) })
            .collect();
        self.clamp_cursor();
    }

    fn sections_list(&self) -> Vec<String> {
        self.model.sections()
    }

    fn move_section(&mut self, delta: isize) {
        let count = self.sections_list().len();
        if count == 0 {
            return;
        }
        self.section_cursor = self.section_cursor.saturating_add_signed(delta).min(count - 1);
    }

    fn drill_into_section(&mut self) {
        let sections = self.sections_list();
        let Some(section) = sections.get(self.section_cursor) else {
            return;
        };
        self.browse_section = section.clone();
        self.browse_level = 1;
        self.cursor = 0;
        self.scroll_offset = 0;
        self.build_rows();
    }

    fn go_back_to_sections(&mut self) {
        self.browse_level = 0;
        if let Some(index) = self.sections_list().iter().position(|section| *section == self.browse_section) {
            self.section_cursor = index;
        }
        self.browse_section.clear();
        self.rows.clear();
    }

    /// Persist settings.toml when it has unsaved changes.
    fn save_all(&mut self) -> anyhow::Result<()> {
        if self.model.dirty {
            self.model.save()?;
        }
        Ok(())
    }

    /// Indexes of rows the cursor can rest on (every row; headers are not listed).
    fn nav_indices(&self) -> Vec<usize> {
        self.rows.iter().enumerate().filter(|(_, row)| !row.is_header()).map(|(index, _)| index).collect()
    }

    fn clamp_cursor(&mut self) {
        let nav = self.nav_indices();
        let Some(last) = nav.last().copied() else {
            self.cursor = 0;
            return;
        };
        if !nav.contains(&self.cursor) {
            self.cursor = nav.iter().copied().find(|index| *index >= self.cursor).unwrap_or(last);
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        let nav = self.nav_indices();
        if nav.is_empty() {
            return;
        }
        let current = nav.iter().position(|index| *index == self.cursor).unwrap_or(0);
        self.cursor = nav[current.saturating_add_signed(delta).min(nav.len() - 1)];
    }

    fn current_row(&self) -> Row {
        self.rows.get(self.cursor).cloned().unwrap_or_else(no_row)
    }

    /// The current row's key when it has a non-empty one.
    fn current_key(&self) -> Option<(String, String)> {
        let row = self.current_row();
        row.key.filter(|key| !key.is_empty()).map(|key| (row.section, key))
    }

    /// `row.key` as Python formatted it into messages.
    fn editing_key(&self) -> String {
        self.editing_row.key.clone().unwrap_or_else(|| "None".to_string())
    }

    // --------------------------------------------------------------------- //
    // Rendering
    // --------------------------------------------------------------------- //

    /// The panel for the current mode on a `width` x `height` terminal.
    pub fn render(&mut self, width: u16, height: u16) -> Panel {
        match self.mode {
            Mode::Browse if self.browse_level == 0 => self.render_sections_view(height),
            Mode::Browse => self.render_keys_view(width, height),
            Mode::Picker => self.render_picker(),
            Mode::MultiSelect => self.render_multi_select(),
            Mode::Inline | Mode::AddKey => self.render_inline(),
            Mode::TimeList => self.render_time_list(),
            Mode::ConfirmQuit => self.render_confirm_quit(),
        }
    }

    fn title_with_marker(&self, title: String) -> String {
        if self.model.dirty { format!("{title}  •  modified") } else { title }
    }

    /// Keep `cursor` inside a window of `viewport` rows.
    fn scroll_to(&mut self, cursor: usize, viewport: usize) {
        if cursor < self.scroll_offset {
            self.scroll_offset = cursor;
        } else if cursor >= self.scroll_offset + viewport {
            self.scroll_offset = cursor + 1 - viewport;
        }
    }

    fn render_sections_view(&mut self, height: u16) -> Panel {
        let sections = self.sections_list();
        let viewport = usize::from(height).saturating_sub(12).max(1);
        self.scroll_to(self.section_cursor, viewport);

        let mut t = Text::new();
        let end = sections.len().min(self.scroll_offset + viewport);
        for (index, section) in sections.iter().enumerate().take(end).skip(self.scroll_offset) {
            let selected = index == self.section_cursor;
            let style = if selected { BOLD_CYAN } else { CYAN };
            t.append(if selected { "  ▸ " } else { "    " }, style);
            t.append(&section_label(section), style);
            t.append(&format!("  ({} keys)\n", self.model.keys_in(section).len()), DIM);
        }
        if self.scroll_offset > 0 {
            t.append("    ↑ more above\n", DIM_ITALIC);
        }
        if end < sections.len() {
            t.append("    ↓ more below\n", DIM_ITALIC);
        }
        append_actions(&mut t, &[("↑↓", "Move"), ("Enter", "Open")], &[("s", "Save"), ("q", "Quit")]);
        if !self.message.is_empty() {
            t.append(&format!("\n\n  {}", self.message), GREEN);
        }
        Panel {
            title: self.title_with_marker("⚙  Settings".to_string()),
            text: t,
            border: Color::LightBlue,
            padding: (0, 1),
        }
    }

    fn render_keys_view(&mut self, width: u16, height: u16) -> Panel {
        let viewport = usize::from(height).saturating_sub(14).max(1);
        self.scroll_to(self.cursor, viewport);

        let mut t = Text::new();
        let label = section_label(&self.browse_section);
        t.append(&format!("  ← {label}\n\n"), BOLD_CYAN);

        let end = self.rows.len().min(self.scroll_offset + viewport);
        let row_width = usize::from(width).saturating_sub(4).max(1);
        for (index, row) in self.rows.iter().enumerate().take(end).skip(self.scroll_offset) {
            let selected = index == self.cursor;
            let key = row.key.clone().unwrap_or_default();
            let value = fmt_val(self.model.get(&row.section, &key));
            let (key_style, value_style) = if selected { (BOLD_WHITE, BOLD_YELLOW) } else { (WHITE, DIM) };
            let line = vec![
                ((if selected { "  ▸ " } else { "    " }).to_string(), key_style),
                (format!("{key:<26}"), key_style),
                (value, key_style.patch(value_style)),
            ];
            // Keep data rows compact while allowing the action footer to wrap.
            t.append_segments(text::truncate_ellipsis(line, row_width));
            t.append("\n", PLAIN);
        }
        if self.rows.is_empty() {
            t.append("    (no keys in this section)\n", DIM);
        }
        if self.scroll_offset > 0 {
            t.append("    ↑ more above\n", DIM_ITALIC);
        }
        if end < self.rows.len() {
            t.append("    ↓ more below\n", DIM_ITALIC);
        }

        let current = self.current_key();
        let meta = current.as_ref().map(|(section, key)| get_meta(section, key));
        if let Some(meta) = meta.filter(|meta| !meta.help_text.is_empty()) {
            t.append(&format!("\n  ℹ  {}", meta.help_text), DIM_ITALIC);
        }
        let edit_action = if meta.is_some_and(|meta| meta.editor == EditorType::Toggle) {
            ("Space", "Toggle")
        } else {
            ("Enter", "Edit")
        };
        let local: Vec<(&str, &str)> = if self.rows.is_empty() {
            vec![("a", "Add")]
        } else {
            vec![("↑↓", "Move"), edit_action, ("a", "Add"), ("d", "Delete")]
        };
        append_actions(&mut t, &local, &[("Esc", "Back"), ("s", "Save"), ("q", "Quit")]);
        if !self.message.is_empty() {
            t.append(&format!("\n\n  {}", self.message), GREEN);
        }
        Panel {
            title: self.title_with_marker(format!("⚙  Settings › {label}")),
            text: t,
            border: Color::LightBlue,
            padding: (0, 1),
        }
    }

    fn render_picker(&self) -> Panel {
        let mut t = Text::new();
        for (index, choice) in self.picker_choices.iter().enumerate() {
            let selected = index == self.picker_cursor;
            let prefix = if selected { "  ▸ " } else { "    " };
            t.append(&format!("{prefix}{choice}\n"), if selected { BOLD_YELLOW } else { PLAIN });
        }
        let weekday = self.compound_type == Some(Compound::WeekdayTime);
        let action = if weekday { "Next" } else { "Apply" };
        append_actions(&mut t, &[("↑↓", "Move"), ("Enter", action)], &[("Esc", "Cancel")]);

        let mut label = self.editing_row.key.clone().unwrap_or_default();
        if weekday && self.compound_step == 0 {
            label.push_str(" – select weekday");
        }
        Panel { title: format!("  Select: {label}  "), text: t, border: Color::Yellow, padding: (1, 2) }
    }

    fn render_multi_select(&self) -> Panel {
        let mut t = Text::new();
        for (index, choice) in self.picker_choices.iter().enumerate() {
            let selected = index == self.picker_cursor;
            let check = if self.picker_selected.contains(&index) { "✓" } else { " " };
            let prefix = if selected { "▸" } else { " " };
            t.append(&format!("  {prefix} [{check}] {choice}\n"), if selected { BOLD_YELLOW } else { PLAIN });
        }
        append_actions(&mut t, &[("↑↓", "Move"), ("Space", "Toggle")], &[("Enter", "Apply"), ("Esc", "Cancel")]);
        Panel { title: format!("  Select: {}  ", self.editing_key()), text: t, border: Color::Yellow, padding: (1, 2) }
    }

    fn render_inline(&self) -> Panel {
        let row = &self.editing_row;
        let key = row.key.clone().unwrap_or_default();
        let meta = get_meta(&row.section, &key);
        let mut t = Text::new();

        if self.mode == Mode::AddKey {
            t.append(&format!("  Section: [{}]\n\n", self.add_section), CYAN);
            t.append("  Key name: ", WHITE);
            t.append(&format!("{}█\n", self.edit_buffer), BOLD_YELLOW);
        } else {
            t.append(&format!("  Current: {}\n\n", fmt_val(self.model.get(&row.section, &key))), DIM);
            let label = match (self.compound_type, self.compound_step) {
                (Some(Compound::WeekdayTime), 1) => format!("Time (for {})", self.compound_partial),
                (Some(Compound::DayTime), 0) => "Day of month (1-31)".to_string(),
                (Some(Compound::DayTime), 1) => format!("Time (for day {})", self.compound_partial),
                _ => "New value".to_string(),
            };
            t.append(&format!("  {label}: "), WHITE);
            t.append(&format!("{}█\n", self.edit_buffer), BOLD_YELLOW);
            if !meta.help_text.is_empty() {
                let mut hint = meta.help_text.to_string();
                if let (Some(min), Some(max)) = (meta.min_val, meta.max_val) {
                    hint.push_str(&format!(" ({min}–{max})"));
                }
                t.append(&format!("\n  ℹ  {hint}"), DIM_ITALIC);
            }
        }
        if !self.message.is_empty() {
            t.append(&format!("\n  ⚠  {}", self.message), BOLD_RED);
        }

        let action = if self.mode == Mode::AddKey {
            "Add"
        } else if self.compound_type == Some(Compound::DayTime) && self.compound_step == 0 {
            "Next"
        } else {
            "Apply"
        };
        append_actions(&mut t, &[("Enter", action), ("Esc", "Cancel")], &[]);
        let title = if self.mode == Mode::AddKey {
            "  Add Key  ".to_string()
        } else {
            format!("  Edit: {}  ", row.key.as_deref().filter(|key| !key.is_empty()).unwrap_or("new key"))
        };
        Panel { title, text: t, border: Color::Yellow, padding: (1, 2) }
    }

    fn render_time_list(&self) -> Panel {
        let mut t = Text::new();
        if self.time_list_values.is_empty() {
            t.append("  (no times configured)\n", DIM);
        } else {
            for (index, value) in self.time_list_values.iter().enumerate() {
                let selected = index == self.time_list_cursor;
                let prefix = if selected { "  ▸ " } else { "    " };
                t.append(&format!("{prefix}{}\n", py::py_str(value)), if selected { BOLD_YELLOW } else { PLAIN });
            }
        }
        let local: Vec<(&str, &str)> = if self.time_list_values.is_empty() {
            vec![("a", "Add")]
        } else {
            vec![("↑↓", "Move"), ("Space", "Edit"), ("a", "Add"), ("d", "Delete")]
        };
        append_actions(&mut t, &local, &[("Enter", "Apply"), ("Esc", "Cancel")]);
        Panel { title: format!("  Edit: {}  ", self.editing_key()), text: t, border: Color::Yellow, padding: (1, 2) }
    }

    fn render_confirm_quit(&self) -> Panel {
        let mut t = Text::new();
        t.append("  You have unsaved changes.\n\n", BOLD_WHITE);
        t.append("  [s] Save and quit\n", GREEN);
        t.append("  [d] Discard and quit\n", RED);
        t.append("  [Esc] Keep editing\n", DIM);
        if !self.message.is_empty() {
            t.append(&format!("\n  {}", self.message), BOLD_RED);
        }
        Panel { title: "  Unsaved Changes  ".to_string(), text: t, border: Color::Red, padding: (1, 2) }
    }

    // --------------------------------------------------------------------- //
    // Key handling
    // --------------------------------------------------------------------- //

    /// Handle one key press; `true` means quit.
    pub fn handle_key(&mut self, key: Key) -> bool {
        self.message.clear();
        match self.mode {
            Mode::Browse => return self.on_browse(key),
            Mode::Picker => self.on_picker(key),
            Mode::MultiSelect => self.on_multi_select(key),
            Mode::Inline => self.on_inline(key),
            Mode::AddKey => self.on_add_key(key),
            Mode::TimeList => self.on_time_list(key),
            Mode::ConfirmQuit => return self.on_confirm_quit(key),
        }
        false
    }

    /// Quit now (`true`) when nothing is unsaved; otherwise ask first.
    fn request_quit(&mut self) -> bool {
        if !self.model.dirty {
            return true;
        }
        if self.mode != Mode::ConfirmQuit {
            self.quit_return_mode = self.mode;
        }
        self.mode = Mode::ConfirmQuit;
        false
    }

    fn on_browse(&mut self, key: Key) -> bool {
        match key {
            Key::Char('s') => {
                self.message = match self.save_all() {
                    Ok(()) => "✅ Settings saved".to_string(),
                    Err(error) => format!("❌ Save failed: {error}"),
                };
            }
            // `e` and `x` are the old, unadvertised quit shortcuts.
            Key::Char('q' | 'e' | 'x') => return self.request_quit(),
            Key::Esc | Key::Backspace | Key::Left => {
                if self.browse_level == 1 {
                    self.go_back_to_sections();
                }
            }
            _ if self.browse_level == 0 => self.on_sections_view(key),
            _ => self.on_keys_view(key),
        }
        false
    }

    fn on_sections_view(&mut self, key: Key) {
        match key {
            Key::Up => self.move_section(-1),
            Key::Down => self.move_section(1),
            Key::Enter => self.drill_into_section(),
            _ => {}
        }
    }

    fn on_keys_view(&mut self, key: Key) {
        match key {
            Key::Up => self.move_cursor(-1),
            Key::Down => self.move_cursor(1),
            Key::Enter => self.start_edit(),
            Key::Char(' ') => {
                if let Some((section, key)) = self.current_key() {
                    if get_meta(&section, &key).editor == EditorType::Toggle {
                        let current = py::opt_truthy(self.model.get(&section, &key));
                        self.model.set(&section, &key, Value::Bool(!current));
                        self.message = format!("Toggled {key}");
                    } else {
                        self.start_edit();
                    }
                }
            }
            Key::Char('d') => {
                if let Some((section, key)) = self.current_key() {
                    self.model.delete(&section, &key);
                    self.build_rows();
                    self.message = format!("Deleted {key}");
                }
            }
            Key::Char('a') => {
                let row = self.current_row();
                self.add_section =
                    if row.key.is_some_and(|key| !key.is_empty()) { row.section } else { self.browse_section.clone() };
                self.edit_buffer = if self.add_section == "task_types" {
                    // Pre-fill the next free task type number.
                    let highest = self
                        .model
                        .keys_in("task_types")
                        .iter()
                        .filter_map(|key| py::parse_int_str(key))
                        .fold(0, i64::max);
                    (highest + 1).to_string()
                } else {
                    String::new()
                };
                self.mode = Mode::AddKey;
            }
            _ => {}
        }
    }

    fn on_picker(&mut self, key: Key) {
        match key {
            Key::Up => self.picker_cursor = self.picker_cursor.saturating_sub(1),
            Key::Down => self.picker_cursor = (self.picker_cursor + 1).min(self.picker_choices.len().saturating_sub(1)),
            Key::Enter => {
                let Some(selected) = self.picker_choices.get(self.picker_cursor).cloned() else {
                    return;
                };
                if self.compound_type == Some(Compound::WeekdayTime) {
                    // The weekday is chosen; ask for the time next.
                    self.compound_partial = selected;
                    self.compound_step = 1;
                    self.mode = Mode::Inline;
                    self.edit_buffer = self.compound_time_default.clone();
                } else {
                    let row = self.editing_row.clone();
                    self.model.set(
                        &row.section,
                        row.key.as_deref().unwrap_or_default(),
                        Value::String(selected.clone()),
                    );
                    self.mode = Mode::Browse;
                    self.message = format!("Set {} = {selected}", self.editing_key());
                }
            }
            Key::Esc | Key::Backspace | Key::Left => {
                self.mode = Mode::Browse;
                self.compound_type = None;
            }
            _ => {}
        }
    }

    fn on_multi_select(&mut self, key: Key) {
        match key {
            Key::Up => self.picker_cursor = self.picker_cursor.saturating_sub(1),
            Key::Down => self.picker_cursor = (self.picker_cursor + 1).min(self.picker_choices.len().saturating_sub(1)),
            Key::Char(' ') => {
                if !self.picker_selected.remove(&self.picker_cursor) {
                    self.picker_selected.insert(self.picker_cursor);
                }
            }
            Key::Enter => {
                let chosen: Vec<Value> = self
                    .picker_selected
                    .iter()
                    .filter_map(|index| self.picker_choices.get(*index))
                    .map(|choice| Value::String(choice.clone()))
                    .collect();
                let chosen = Value::Array(chosen);
                let row = self.editing_row.clone();
                self.model.set(&row.section, row.key.as_deref().unwrap_or_default(), chosen.clone());
                self.mode = Mode::Browse;
                self.message = format!("Set {} = {}", self.editing_key(), py::py_repr(&chosen));
            }
            Key::Esc | Key::Backspace | Key::Left => self.mode = Mode::Browse,
            _ => {}
        }
    }

    fn on_inline(&mut self, key: Key) {
        match key {
            Key::Enter => self.confirm_inline(),
            Key::Esc => {
                if self.time_list_editing {
                    // Back to the list; a cancelled new item leaves nothing behind.
                    self.mode = Mode::TimeList;
                    self.time_list_editing = false;
                    self.time_list_adding = false;
                    self.time_list_cursor = self.time_list_cursor.min(self.time_list_values.len().saturating_sub(1));
                } else {
                    self.mode = Mode::Browse;
                    self.compound_type = None;
                }
            }
            Key::Backspace => {
                self.edit_buffer.pop();
            }
            Key::Char(c) => self.edit_buffer.push(c),
            _ => {}
        }
    }

    fn set_compound_value(&mut self, raw: &str) {
        if !valid_time(raw) {
            self.message = INVALID_TIME.to_string();
            return;
        }
        let combined = format!("{} {raw}", self.compound_partial);
        let row = self.editing_row.clone();
        self.model.set(&row.section, row.key.as_deref().unwrap_or_default(), Value::String(combined.clone()));
        self.mode = Mode::Browse;
        self.compound_type = None;
        self.message = format!("Set {} = {combined}", self.editing_key());
    }

    fn confirm_inline(&mut self) {
        let raw = self.edit_buffer.trim().to_string();
        match (self.compound_type, self.compound_step) {
            // Second step of a weekday or day-of-month editor: the time.
            (Some(Compound::WeekdayTime | Compound::DayTime), 1) => return self.set_compound_value(&raw),
            // First step of the day-of-month editor: the day.
            (Some(Compound::DayTime), 0) => {
                match py::parse_int_str(&raw).filter(|day| (1..=31).contains(day)) {
                    Some(day) => {
                        self.compound_partial = day.to_string();
                        self.compound_step = 1;
                        self.edit_buffer = self.compound_time_default.clone();
                    }
                    None => self.message = "Day must be between 1 and 31".to_string(),
                }
                return;
            }
            _ => {}
        }

        if self.time_list_editing {
            if !valid_time(&raw) {
                self.message = INVALID_TIME.to_string();
                return;
            }
            if self.time_list_adding {
                self.time_list_values.push(Value::String(raw));
            } else if let Some(item) = self.time_list_values.get_mut(self.time_list_cursor) {
                *item = Value::String(raw);
            }
            self.mode = Mode::TimeList;
            self.time_list_editing = false;
            self.time_list_adding = false;
            return;
        }

        let row = self.editing_row.clone();
        let key = row.key.clone().unwrap_or_default();
        if row.section == "task_types" && raw.is_empty() {
            self.message = "Task type name cannot be empty".to_string();
            return;
        }
        let Some(validated) = self.validate(get_meta(&row.section, &key), &raw) else {
            return;
        };
        let shown = py::py_str(&validated);
        self.model.set(&row.section, &key, validated);
        self.mode = Mode::Browse;
        self.message = format!("Set {} = {shown}", self.editing_key());
    }

    fn validate(&mut self, meta: FieldMeta, raw: &str) -> Option<Value> {
        match meta.editor {
            EditorType::Number => {
                let Some(value) = py::parse_int_str(raw) else {
                    self.message = "Enter a valid integer".to_string();
                    return None;
                };
                if let Some(min) = meta.min_val.filter(|min| value < *min) {
                    self.message = format!("Minimum is {min}");
                    return None;
                }
                if let Some(max) = meta.max_val.filter(|max| value > *max) {
                    self.message = format!("Maximum is {max}");
                    return None;
                }
                Some(Value::from(value))
            }
            EditorType::Time if !valid_time(raw) => {
                self.message = INVALID_TIME.to_string();
                None
            }
            _ => Some(Value::String(raw.to_string())),
        }
    }

    fn on_add_key(&mut self, key: Key) {
        match key {
            Key::Enter => {
                let name = self.edit_buffer.trim().to_string();
                if name.is_empty() {
                    self.message = "Key name cannot be empty".to_string();
                    return;
                }
                let section = self.add_section.clone();
                if !self.model.add_key(&section, &name) {
                    self.message = format!("Key '{name}' already exists");
                    self.mode = Mode::Browse;
                    return;
                }
                self.build_rows();
                // Move the cursor to the new key.
                if let Some(index) =
                    self.rows.iter().position(|row| row.section == section && row.key.as_deref() == Some(name.as_str()))
                {
                    self.cursor = index;
                }
                if section == "task_types" {
                    self.start_edit();
                } else {
                    self.mode = Mode::Browse;
                    self.message = format!("Added {name} — press Enter to set its value");
                }
            }
            Key::Esc => self.mode = Mode::Browse,
            Key::Backspace => {
                self.edit_buffer.pop();
            }
            Key::Char(c) => self.edit_buffer.push(c),
            _ => {}
        }
    }

    fn on_time_list(&mut self, key: Key) {
        let count = self.time_list_values.len();
        match key {
            Key::Up if count > 0 => self.time_list_cursor = self.time_list_cursor.saturating_sub(1),
            Key::Down if count > 0 => self.time_list_cursor = (self.time_list_cursor + 1).min(count - 1),
            Key::Char(' ') if count > 0 => {
                self.edit_buffer = self.time_list_values.get(self.time_list_cursor).map(py::py_str).unwrap_or_default();
                self.time_list_editing = true;
                self.time_list_adding = false;
                self.mode = Mode::Inline;
            }
            Key::Char('a') => {
                self.time_list_cursor = count;
                self.edit_buffer = "00:00".to_string();
                self.time_list_editing = true;
                self.time_list_adding = true;
                self.mode = Mode::Inline;
            }
            Key::Char('d') if self.time_list_cursor < count => {
                self.time_list_values.remove(self.time_list_cursor);
                if self.time_list_cursor >= self.time_list_values.len() {
                    self.time_list_cursor = self.time_list_values.len().saturating_sub(1);
                }
            }
            Key::Enter => {
                let row = self.editing_row.clone();
                let values = Value::Array(self.time_list_values.clone());
                self.model.set(&row.section, row.key.as_deref().unwrap_or_default(), values);
                self.mode = Mode::Browse;
                self.message = format!("Updated {}", self.editing_key());
            }
            Key::Esc | Key::Backspace | Key::Left => self.mode = Mode::Browse,
            _ => {}
        }
    }

    fn on_confirm_quit(&mut self, key: Key) -> bool {
        match key {
            Key::Char('s') => match self.save_all() {
                Ok(()) => true,
                Err(error) => {
                    self.message = format!("❌ Save failed: {error}");
                    false
                }
            },
            Key::Char('d') => true,
            Key::Esc => {
                self.mode = self.quit_return_mode;
                false
            }
            _ => false,
        }
    }

    /// Open the editor that fits the current row.
    fn start_edit(&mut self) {
        let row = self.current_row();
        let Some(key) = row.key.clone().filter(|key| !key.is_empty()) else {
            return;
        };
        let meta = get_meta(&row.section, &key);
        let value = self.model.get(&row.section, &key).cloned();
        self.editing_row = row.clone();
        self.compound_type = None;
        self.time_list_editing = false;
        self.time_list_adding = false;

        match meta.editor {
            EditorType::Toggle => {
                self.model.set(&row.section, &key, Value::Bool(!py::opt_truthy(value.as_ref())));
                self.message = format!("Toggled {key}");
            }
            EditorType::Picker => {
                self.mode = Mode::Picker;
                self.picker_choices = meta.choices.iter().map(|choice| choice.to_string()).collect();
                let current = value.as_ref().map(py::py_str).unwrap_or_else(|| "None".to_string());
                self.picker_cursor = self.picker_choices.iter().position(|choice| *choice == current).unwrap_or(0);
            }
            EditorType::MultiSelect => {
                self.mode = Mode::MultiSelect;
                self.picker_choices = meta.choices.iter().map(|choice| choice.to_string()).collect();
                let current = match &value {
                    Some(Value::Array(items)) => items.clone(),
                    _ => Vec::new(),
                };
                self.picker_selected = self
                    .picker_choices
                    .iter()
                    .enumerate()
                    .filter(|(_, choice)| current.contains(&Value::String(choice.to_string())))
                    .map(|(index, _)| index)
                    .collect();
                self.picker_cursor = 0;
            }
            EditorType::Number | EditorType::Text | EditorType::Time => {
                self.mode = Mode::Inline;
                self.edit_buffer = value.as_ref().map(py::py_str).unwrap_or_default();
            }
            EditorType::TimeList => {
                self.mode = Mode::TimeList;
                self.time_list_values = match value {
                    Some(Value::Array(items)) => items,
                    _ => Vec::new(),
                };
                self.time_list_cursor = 0;
            }
            EditorType::WeekdayTime => {
                let parts = compound_parts(value.as_ref(), ["monday", "20:00"]);
                self.mode = Mode::Picker;
                self.picker_choices = WEEKDAYS.iter().map(|day| day.to_string()).collect();
                self.picker_cursor = parts
                    .first()
                    .and_then(|day| WEEKDAYS.iter().position(|weekday| *weekday == day.to_lowercase()))
                    .unwrap_or(0);
                self.compound_type = Some(Compound::WeekdayTime);
                self.compound_step = 0;
                self.compound_time_default = parts.get(1).cloned().unwrap_or_else(|| "20:00".to_string());
            }
            EditorType::DayTime => {
                let parts = compound_parts(value.as_ref(), ["1", "20:00"]);
                self.mode = Mode::Inline;
                self.edit_buffer = parts.first().cloned().unwrap_or_else(|| "1".to_string());
                self.compound_type = Some(Compound::DayTime);
                self.compound_step = 0;
                self.compound_time_default = parts.get(1).cloned().unwrap_or_else(|| "20:00".to_string());
            }
        }
    }

    // --------------------------------------------------------------------- //
    // Main loop
    // --------------------------------------------------------------------- //

    /// Draw, read a key, react; until the user quits. Returns the exit code.
    fn run_loop(
        &mut self,
        mut next_input: impl FnMut() -> io::Result<Input>,
        mut draw: impl FnMut(&mut Self) -> io::Result<()>,
    ) -> io::Result<i32> {
        loop {
            draw(self)?;
            match next_input()? {
                Input::Interrupt => {
                    if self.request_quit() {
                        break;
                    }
                    self.message.clear();
                }
                Input::Key(key) => {
                    if self.handle_key(key) {
                        break;
                    }
                }
                Input::Redraw => {}
            }
        }
        Ok(0)
    }

    /// Run the editor full-screen in the terminal.
    pub fn run(&mut self) -> io::Result<i32> {
        let mut terminal = match ratatui::try_init() {
            Ok(terminal) => terminal,
            Err(error) => {
                ratatui::restore();
                return Err(error);
            }
        };
        let result = self.run_loop(read_input, |tui| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    tui.render(area.width, area.height).draw(frame);
                })
                .map(|_| ())
        });
        ratatui::restore();
        result
    }
}

/// Wait for the next key press or resize.
fn read_input() -> io::Result<Input> {
    loop {
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => return Ok(map_key(key)),
            Event::Resize(..) => return Ok(Input::Redraw),
            _ => {}
        }
    }
}

/// Translate a crossterm key event the way `readchar` keys were compared.
fn map_key(event: KeyEvent) -> Input {
    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
    let key = match event.code {
        KeyCode::Char('c') if ctrl => return Input::Interrupt,
        KeyCode::Char('h') if ctrl => Key::Backspace,
        KeyCode::Char('j' | 'm') if ctrl => Key::Enter,
        KeyCode::Char('[') if ctrl => Key::Esc,
        KeyCode::Char(_) if ctrl || event.modifiers.contains(KeyModifiers::ALT) => Key::Other,
        KeyCode::Char(c) if !c.is_control() => Key::Char(c),
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Enter => Key::Enter,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Esc => Key::Esc,
        _ => Key::Other,
    };
    Input::Key(key)
}

/// Check the terminal and settings file, then run `run_editor`.
fn settings_command_with(
    out: &mut dyn Write,
    is_tty: bool,
    run_editor: impl FnOnce(SettingsTui) -> io::Result<i32>,
) -> i32 {
    if !is_tty {
        let _ = writeln!(out, "❌ Settings editor requires an interactive terminal.");
        return 1;
    }
    let settings_path = crate::paths::runtime().settings_path;
    if !settings_path.exists() {
        let _ = writeln!(out, "❌ Settings file not found: {}", settings_path.display());
        let _ = writeln!(
            out,
            "   Ask your AI agent to build one with the Schedule Everything plugin, then run 'rmd check'."
        );
        return 1;
    }
    let model = match SettingsModel::new(&settings_path) {
        Ok(model) => model,
        Err(error) => {
            let _ = writeln!(out, "❌ Failed to load settings: {error}");
            return 1;
        }
    };
    match run_editor(SettingsTui::new(model)) {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(out, "{}", tf("❌ Unexpected error: {e}", &[("e", &error)]));
            1
        }
    }
}

/// Run the settings editor; returns the exit code.
pub fn settings_command() -> i32 {
    settings_command_with(&mut io::stdout(), io::stdin().is_terminal(), |mut tui| tui.run())
}
