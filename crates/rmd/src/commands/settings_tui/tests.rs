//! Navigation, draft cancellation, and visible-action tests for the settings
//! editor.

use std::path::Path;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serde_json::{Value, json};

use super::*;
use crate::test_support::TestEnv;

const FIXTURE: &str = "[settings]\nlanguage = \"en\"\nalarm_interval = 5\n\
skip_days = [\"monday\"]\nshow_tasks_after_change = true\n\
\n[tasks]\ndaily_urgent = [\"10:00\", \"20:00\"]\n\
weekly_review = \"monday 20:00\"\nmonthly_review = \"1 20:00\"\n\
\n[paths]\n";

/// An editor over a temporary settings file (kept alive by the returned dir).
fn editor(content: &str) -> (tempfile::TempDir, SettingsTui) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, content).unwrap();
    let tui = SettingsTui::new(SettingsModel::new(&path).unwrap());
    (dir, tui)
}

fn reload(path: &Path) -> SettingsModel {
    SettingsModel::new(path).unwrap()
}

fn open_section(tui: &mut SettingsTui, section: &str) {
    tui.section_cursor = tui.sections_list().iter().position(|name| name == section).unwrap();
    tui.handle_key(Key::Enter);
}

fn open_field(tui: &mut SettingsTui, key: &str) {
    tui.cursor = tui.rows.iter().position(|row| row.key.as_deref() == Some(key)).unwrap();
    tui.handle_key(Key::Enter);
}

fn row_keys(tui: &SettingsTui) -> Vec<String> {
    tui.rows.iter().map(|row| row.key.clone().unwrap()).collect()
}

fn get(tui: &SettingsTui, section: &str, key: &str) -> Option<Value> {
    tui.model.get(section, key).cloned()
}

fn set(tui: &mut SettingsTui, section: &str, key: &str, value: Value) {
    tui.model.set(section, key, value);
}

/// The plain text of the current panel (Python `_render().renderable.plain`).
fn plain(tui: &mut SettingsTui) -> String {
    tui.render(80, 25).text.plain()
}

/// The screen after drawing the current panel on a `width` x `height` terminal.
fn screen(tui: &mut SettingsTui, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| tui.render(width, height).draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..height {
        let mut line = String::new();
        let mut x = 0;
        while x < width {
            let symbol = buffer[(x, y)].symbol();
            line.push_str(symbol);
            // Skip the cells a wide character covers.
            x += u16::try_from(unicode_width::UnicodeWidthStr::width(symbol)).unwrap_or(1).max(1);
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

/// Run the main loop over `inputs`, recording every panel drawn.
fn run_with_inputs(tui: &mut SettingsTui, inputs: Vec<Input>) -> (i32, Vec<Panel>, usize) {
    let mut inputs = inputs.into_iter();
    let mut read = 0;
    let mut panels = Vec::new();
    let code = tui
        .run_loop(
            || {
                read += 1;
                Ok(inputs.next().expect("the editor asked for more input than the test gave"))
            },
            |tui| {
                panels.push(tui.render(80, 25));
                Ok(())
            },
        )
        .unwrap();
    (code, panels, read)
}

// ---- test_cli_commands.py: TestSettingsCommand ----------------------------

#[test]
fn row_building_drills_into_sections_and_back() {
    let (_dir, mut tui) = editor("[settings]\na = 1\nb = 2\n\n[paths]\nc = 3\n");
    // Level 0 lists sections; rows are only built inside a section.
    assert_eq!(tui.browse_level, 0);
    assert!(tui.rows.is_empty());
    let sections = tui.sections_list();
    assert_eq!(sections, ["settings", "paths", "task_types"]);

    tui.section_cursor = 0;
    tui.drill_into_section();
    assert_eq!((tui.browse_level, tui.browse_section.as_str()), (1, "settings"));
    assert_eq!(row_keys(&tui), ["a", "b"]);
    assert_eq!(tui.cursor, 0);

    tui.go_back_to_sections();
    assert_eq!(tui.browse_level, 0);
    assert_eq!(tui.section_cursor, 0);
    assert!(tui.rows.is_empty());

    tui.section_cursor = 1;
    tui.drill_into_section();
    assert_eq!(tui.browse_section, "paths");
    assert_eq!(row_keys(&tui), ["c"]);
}

#[test]
fn settings_command_needs_a_terminal_and_a_settings_file() {
    let env = TestEnv::new();
    env.config_dir();
    env.write(".active_config", "0\n");
    let mut out = Vec::new();
    assert_eq!(settings_command_with(&mut out, false, |_| panic!("no terminal")), 1);
    assert_eq!(String::from_utf8(out).unwrap(), "❌ Settings editor requires an interactive terminal.\n");

    let mut out = Vec::new();
    assert_eq!(settings_command_with(&mut out, true, |_| panic!("no settings file")), 1);
    let output = String::from_utf8(out).unwrap();
    assert!(output.starts_with("❌ Settings file not found: "));
    assert!(output.contains("not found"));

    env.write("user_config_0/settings.toml", "[settings\n");
    let mut out = Vec::new();
    assert_eq!(settings_command_with(&mut out, true, |_| panic!("invalid settings")), 1);
    assert!(String::from_utf8(out).unwrap().starts_with("❌ Failed to load settings: "));

    env.write("user_config_0/settings.toml", "[settings]\nlanguage = \"en\"\n");
    let mut out = Vec::new();
    assert_eq!(settings_command_with(&mut out, true, |tui| Ok(tui.sections_list().len() as i32)), 2);
}

#[test]
fn run_loop_exits_on_quit_keys() {
    for exit_key in ['q', 'e', 'x'] {
        let (_dir, mut tui) = editor("[settings]\nlanguage = \"en\"\n");
        let (code, panels, read) = run_with_inputs(&mut tui, vec![Input::Key(Key::Char(exit_key))]);
        assert_eq!((code, panels.len(), read), (0, 1, 1));
    }
}

#[test]
fn time_list_navigation_clamps_and_back_keys_cancel() {
    let (_dir, mut tui) = editor("[tasks]\ndaily_urgent = [\"10:00\", \"20:00\"]\n");
    tui.editing_row = Row { section: "tasks".into(), key: Some("daily_urgent".into()) };
    tui.mode = Mode::TimeList;
    tui.time_list_values = vec![json!("10:00"), json!("20:00")];
    tui.time_list_cursor = 0;

    tui.handle_key(Key::Down);
    assert_eq!(tui.time_list_cursor, 1);
    tui.handle_key(Key::Down);
    assert_eq!(tui.time_list_cursor, 1);
    tui.handle_key(Key::Up);
    assert_eq!(tui.time_list_cursor, 0);
    tui.handle_key(Key::Up);
    assert_eq!(tui.time_list_cursor, 0);

    tui.handle_key(Key::Left);
    assert_eq!(tui.mode, Mode::Browse);
    assert_eq!(get(&tui, "tasks", "daily_urgent"), Some(json!(["10:00", "20:00"])));

    tui.mode = Mode::Picker;
    tui.handle_key(Key::Backspace);
    assert_eq!(tui.mode, Mode::Browse);
}

#[test]
fn task_types_rows_prefill_and_auto_edit() {
    let (_dir, mut tui) = editor("[settings]\na = 1\n\n[task_types]\n1 = \"read papers\"\n2 = \"coding\"\n");
    open_section(&mut tui, "task_types");
    assert_eq!((tui.browse_level, tui.browse_section.as_str()), (1, "task_types"));
    assert_eq!(row_keys(&tui), ["1", "2"]);

    let (_dir, mut tui) = editor("[task_types]\n1 = \"read papers\"\n3 = \"coding\"\n");
    open_section(&mut tui, "task_types");
    tui.handle_key(Key::Char('a'));
    assert_eq!(tui.mode, Mode::AddKey);
    assert_eq!(tui.edit_buffer, "4");

    let (_dir, mut tui) = editor("[task_types]\n1 = \"read papers\"\n");
    open_section(&mut tui, "task_types");
    tui.handle_key(Key::Char('a'));
    assert_eq!(tui.edit_buffer, "2");
    tui.handle_key(Key::Enter);
    // Adding a task type goes straight on to naming it.
    assert_eq!(tui.mode, Mode::Inline);
    assert_eq!(tui.editing_row.key.as_deref(), Some("2"));
    assert_eq!(tui.rows[tui.cursor].key.as_deref(), Some("2"));
}

#[test]
fn task_type_names_cannot_be_empty() {
    let (_dir, mut tui) = editor("[task_types]\n1 = \"read papers\"\n");
    tui.editing_row = Row { section: "task_types".into(), key: Some("1".into()) };
    tui.mode = Mode::Inline;
    tui.edit_buffer.clear();
    tui.handle_key(Key::Enter);
    assert_eq!(tui.mode, Mode::Inline);
    assert!(tui.message.contains("cannot be empty"));
}

#[test]
fn sections_list_has_no_model_page() {
    let (_dir, tui) = editor("[settings]\nfoo = 1\n\n[time_blocks]\npomodoro = 25\n");
    let sections = tui.sections_list();
    assert_eq!(sections[..2], ["settings", "time_blocks"]);
    assert!(!sections.iter().any(|section| section == "llm"));
}

#[test]
fn ctrl_c_with_changes_opens_quit_confirmation() {
    let (_dir, mut tui) = editor("[settings]\nfoo = 1\n");
    set(&mut tui, "settings", "foo", json!(2));
    let (code, panels, read) = run_with_inputs(&mut tui, vec![Input::Interrupt, Input::Key(Key::Char('d'))]);
    assert_eq!(code, 0);
    assert!(panels.last().unwrap().title.contains("Unsaved Changes"));
    assert_eq!(read, 2);
}

// ---- test_settings_navigation.py --------------------------------------------

#[test]
fn back_keeps_changes_and_section_selection() {
    for section in ["settings", "tasks"] {
        for back_key in [Key::Esc, Key::Backspace, Key::Left] {
            let (_dir, mut tui) = editor(FIXTURE);
            open_section(&mut tui, section);
            set(&mut tui, "settings", "alarm_interval", json!(10));

            assert!(!tui.handle_key(back_key));
            assert_eq!(tui.mode, Mode::Browse);
            assert_eq!(tui.browse_level, 0);
            assert_eq!(tui.section_cursor, tui.sections_list().iter().position(|s| s == section).unwrap());
            assert_eq!(get(&tui, "settings", "alarm_interval"), Some(json!(10)));
            assert!(tui.model.dirty);
        }
    }
}

#[test]
fn escape_at_root_stays_in_settings() {
    for dirty in [false, true] {
        let (_dir, mut tui) = editor(FIXTURE);
        if dirty {
            set(&mut tui, "settings", "alarm_interval", json!(10));
        }
        assert!(!tui.handle_key(Key::Esc));
        assert_eq!(tui.mode, Mode::Browse);
        assert_eq!(tui.browse_level, 0);
    }
}

#[test]
fn quit_and_legacy_aliases_share_unsaved_guard() {
    for section in [None, Some("settings"), Some("tasks")] {
        for quit_key in ['q', 'e', 'x'] {
            let (_dir, mut tui) = editor(FIXTURE);
            if let Some(section) = section {
                open_section(&mut tui, section);
            }
            set(&mut tui, "settings", "alarm_interval", json!(10));
            assert!(!tui.handle_key(Key::Char(quit_key)));
            assert_eq!(tui.mode, Mode::ConfirmQuit);

            // Repeating Quit cannot discard changes; that is a separate action.
            assert!(!tui.handle_key(Key::Char(quit_key)));
            assert_eq!(tui.mode, Mode::ConfirmQuit);
            tui.handle_key(Key::Esc);
            assert_eq!(tui.mode, Mode::Browse);
            assert_eq!(tui.browse_level, u8::from(section.is_some()));
            assert!(tui.model.dirty);
        }
    }
}

#[test]
fn save_stays_on_page_and_persists_settings() {
    let (_dir, mut tui) = editor(FIXTURE);
    open_section(&mut tui, "settings");
    set(&mut tui, "settings", "alarm_interval", json!(10));

    assert!(!tui.handle_key(Key::Char('s')));
    assert_eq!(tui.mode, Mode::Browse);
    assert_eq!(tui.browse_section, "settings");
    assert!(!tui.model.dirty);
    assert_eq!(tui.message, "✅ Settings saved");
    assert_eq!(reload(&tui.model.path).get("settings", "alarm_interval"), Some(&json!(10)));
}

#[test]
fn quit_prompt_save_or_discard() {
    for action in ['s', 'd'] {
        let (_dir, mut tui) = editor(FIXTURE);
        set(&mut tui, "settings", "alarm_interval", json!(10));
        tui.handle_key(Key::Char('q'));

        assert!(tui.handle_key(Key::Char(action)));
        let expected = if action == 's' { 10 } else { 5 };
        assert_eq!(reload(&tui.model.path).get("settings", "alarm_interval"), Some(&json!(expected)));
    }
}

#[test]
fn failed_save_keeps_quit_prompt_with_error() {
    let (dir, mut tui) = editor(FIXTURE);
    set(&mut tui, "settings", "alarm_interval", json!(10));
    tui.handle_key(Key::Char('q'));

    // A settings path inside a missing directory cannot be written.
    tui.model.path = dir.path().join("missing").join("settings.toml");
    assert!(!tui.handle_key(Key::Char('s')));
    assert_eq!(tui.mode, Mode::ConfirmQuit);
    assert!(tui.model.dirty);
    assert!(plain(&mut tui).contains("❌ Save failed: "));
}

#[test]
fn escape_cancels_field_draft() {
    for key in ["language", "alarm_interval", "skip_days"] {
        let (_dir, mut tui) = editor(FIXTURE);
        open_section(&mut tui, "settings");
        let old_value = get(&tui, "settings", key);
        open_field(&mut tui, key);
        match key {
            "language" => tui.handle_key(Key::Down),
            "skip_days" => tui.handle_key(Key::Char(' ')),
            _ => {
                tui.edit_buffer = "10".into();
                false
            }
        };
        tui.handle_key(Key::Esc);

        assert_eq!(tui.mode, Mode::Browse, "{key}");
        assert_eq!(tui.browse_section, "settings");
        assert_eq!(get(&tui, "settings", key), old_value);
        assert!(!tui.model.dirty);
    }
}

#[test]
fn compound_editor_next_then_cancel() {
    for key in ["weekly_review", "monthly_review"] {
        let (_dir, mut tui) = editor(FIXTURE);
        open_section(&mut tui, "tasks");
        let old_value = get(&tui, "tasks", key);
        open_field(&mut tui, key);
        assert!(plain(&mut tui).contains("[Enter] Next"), "{key}");
        tui.handle_key(Key::Enter);
        assert!(plain(&mut tui).contains("[Enter] Apply"), "{key}");
        tui.edit_buffer = "22:00".into();
        tui.handle_key(Key::Esc);

        assert_eq!(tui.mode, Mode::Browse);
        assert_eq!(tui.compound_type, None);
        assert_eq!(get(&tui, "tasks", key), old_value);
        assert!(!tui.model.dirty);
    }
}

#[test]
fn time_list_apply_or_cancel() {
    for apply in [false, true] {
        let (_dir, mut tui) = editor(FIXTURE);
        open_section(&mut tui, "tasks");
        open_field(&mut tui, "daily_urgent");
        tui.handle_key(Key::Char(' '));
        assert_eq!(tui.mode, Mode::Inline);
        tui.edit_buffer = "11:00".into();
        tui.handle_key(Key::Enter);
        tui.handle_key(Key::Down);
        tui.handle_key(Key::Char('d'));
        tui.handle_key(Key::Char('a'));
        tui.edit_buffer = "22:00".into();
        tui.handle_key(Key::Enter);
        assert_eq!(tui.time_list_values, [json!("11:00"), json!("22:00")]);
        assert_eq!(get(&tui, "tasks", "daily_urgent"), Some(json!(["10:00", "20:00"])));

        tui.handle_key(if apply { Key::Enter } else { Key::Esc });
        assert_eq!(tui.mode, Mode::Browse);
        assert_eq!(tui.browse_section, "tasks");
        let expected = if apply { json!(["11:00", "22:00"]) } else { json!(["10:00", "20:00"]) };
        assert_eq!(get(&tui, "tasks", "daily_urgent"), Some(expected));
        assert_eq!(tui.model.dirty, apply);
    }
}

#[test]
fn cancel_added_time_leaves_no_placeholder() {
    for empty in [false, true] {
        let (_dir, mut tui) = editor(FIXTURE);
        if empty {
            set(&mut tui, "tasks", "daily_urgent", json!([]));
        }
        open_section(&mut tui, "tasks");
        open_field(&mut tui, "daily_urgent");
        let old_values = tui.time_list_values.clone();
        tui.handle_key(Key::Char('a'));
        tui.handle_key(Key::Esc);

        assert_eq!(tui.mode, Mode::TimeList);
        assert_eq!(tui.time_list_values, old_values);
        assert_eq!(tui.time_list_cursor, old_values.len().saturating_sub(1));
        tui.handle_key(Key::Enter);
        assert_eq!(get(&tui, "tasks", "daily_urgent"), Some(Value::Array(old_values)));
    }
}

#[test]
fn browse_actions_are_visible_and_contextual() {
    for section in [None, Some("settings"), Some("tasks"), Some("paths")] {
        for width in [50, 80] {
            let (_dir, mut tui) = editor(FIXTURE);
            if let Some(section) = section {
                open_section(&mut tui, section);
            }
            let rendered = screen(&mut tui, width, 24);
            assert!(rendered.contains("[q] Quit"), "{rendered}");
            assert!(rendered.contains("[s] Save"));
            assert!(!rendered.contains("Exit"));
            assert!(!rendered.contains("e/x"));
            if section.is_some() {
                assert!(rendered.contains("[Esc] Back"));
            }
            if section == Some("paths") {
                assert!(rendered.contains("[a] Add"));
                assert!(!rendered.contains("[Enter] Edit"));
                assert!(!rendered.contains("[d] Delete"));
            }
        }
    }
}

#[test]
fn toggle_and_clear_have_one_visible_action() {
    let (_dir, mut tui) = editor(FIXTURE);
    open_section(&mut tui, "settings");
    tui.cursor = tui.rows.iter().position(|row| row.key.as_deref() == Some("show_tasks_after_change")).unwrap();
    let footer = plain(&mut tui);
    assert!(footer.contains("[Space] Toggle"));
    assert!(!footer.contains("[Enter] Edit"));
}

#[test]
fn long_values_leave_room_for_navigation() {
    for width in [50, 80] {
        let (_dir, mut tui) = editor(FIXTURE);
        let value = "/very/long/path".repeat(30);
        for index in 0..15 {
            set(&mut tui, "paths", &format!("file_{index}"), json!(value));
        }
        open_section(&mut tui, "paths");
        tui.cursor = 14;
        let panel = tui.render(width, 24);
        assert!(panel.height(width) <= 24, "{width}: {}", panel.height(width));

        let rendered = screen(&mut tui, width, 24);
        assert!(rendered.contains("[Esc] Back"), "{rendered}");
        assert!(rendered.contains("[q] Quit"));
        assert!(rendered.contains("file_14"));
        assert!(rendered.contains('…'));
        assert_eq!(get(&tui, "paths", "file_14"), Some(json!(value)));
    }
}

#[test]
fn ctrl_c_prompt_keeps_editor_on_cancel() {
    let (_dir, mut tui) = editor(FIXTURE);
    set(&mut tui, "settings", "alarm_interval", json!(10));
    open_section(&mut tui, "settings");
    open_field(&mut tui, "alarm_interval");
    tui.edit_buffer = "15".into();
    let inputs =
        vec![Input::Interrupt, Input::Interrupt, Input::Key(Key::Esc), Input::Interrupt, Input::Key(Key::Char('d'))];

    let (code, panels, _) = run_with_inputs(&mut tui, inputs);
    assert_eq!(code, 0);
    // Panels: initial editor, prompt, prompt, editor again, prompt.
    assert!(panels[1].title.contains("Unsaved Changes"));
    assert!(panels[3].title.contains("Edit: alarm_interval"));
    assert!(panels[3].text.plain().contains("15"));
    assert_eq!(tui.quit_return_mode, Mode::Inline);
}

// ---- editors -----------------------------------------------------------------

#[test]
fn compound_editors_apply_weekday_and_day_values() {
    let (_dir, mut tui) = editor(FIXTURE);
    open_section(&mut tui, "tasks");
    open_field(&mut tui, "weekly_review");
    assert_eq!(tui.render(80, 25).title, "  Select: weekly_review – select weekday  ");
    assert_eq!(tui.picker_cursor, 0);
    tui.handle_key(Key::Down);
    tui.handle_key(Key::Enter);
    assert_eq!(tui.edit_buffer, "20:00");
    assert!(plain(&mut tui).contains("  Time (for tuesday): 20:00█"));
    tui.edit_buffer = "25:00".into();
    tui.handle_key(Key::Enter);
    assert_eq!(tui.message, "Invalid time (use HH:MM)");
    tui.edit_buffer = "9:30".into();
    tui.handle_key(Key::Enter);
    assert_eq!(get(&tui, "tasks", "weekly_review"), Some(json!("tuesday 9:30")));
    assert_eq!(tui.message, "Set weekly_review = tuesday 9:30");

    open_field(&mut tui, "monthly_review");
    assert_eq!(tui.edit_buffer, "1");
    assert!(plain(&mut tui).contains("  Day of month (1-31): 1█"));
    tui.edit_buffer = "32".into();
    tui.handle_key(Key::Enter);
    assert_eq!(tui.message, "Day must be between 1 and 31");
    tui.edit_buffer = "015".into();
    tui.handle_key(Key::Enter);
    assert!(plain(&mut tui).contains("  Time (for day 15): 20:00█"));
    tui.handle_key(Key::Backspace);
    tui.handle_key(Key::Char('5'));
    tui.handle_key(Key::Enter);
    assert_eq!(get(&tui, "tasks", "monthly_review"), Some(json!("15 20:05")));
}

#[test]
fn inline_editor_validates_numbers_and_times() {
    let (_dir, mut tui) = editor("[settings]\nalarm_interval = 5\n\n[tasks]\ndaily_summary = \"22:00\"\n");
    open_section(&mut tui, "settings");
    open_field(&mut tui, "alarm_interval");
    assert_eq!(tui.render(80, 25).title, "  Edit: alarm_interval  ");
    assert!(plain(&mut tui).contains("  ℹ  Seconds between alarm repeats (1–120)"));
    for (input, message) in [("abc", "Enter a valid integer"), ("0", "Minimum is 1"), ("121", "Maximum is 120")] {
        tui.edit_buffer = input.into();
        tui.handle_key(Key::Enter);
        assert_eq!((tui.mode, tui.message.as_str()), (Mode::Inline, message));
        assert!(plain(&mut tui).contains(&format!("  ⚠  {message}")));
    }
    tui.edit_buffer = " 30 ".into();
    tui.handle_key(Key::Enter);
    assert_eq!(get(&tui, "settings", "alarm_interval"), Some(json!(30)));
    assert_eq!(tui.message, "Set alarm_interval = 30");

    tui.handle_key(Key::Esc);
    open_section(&mut tui, "tasks");
    open_field(&mut tui, "daily_summary");
    tui.edit_buffer = "7".into();
    tui.handle_key(Key::Enter);
    assert_eq!(tui.message, "Invalid time (use HH:MM)");
    tui.edit_buffer = "07:15".into();
    tui.handle_key(Key::Enter);
    assert_eq!(get(&tui, "tasks", "daily_summary"), Some(json!("07:15")));
}

#[test]
fn picker_multi_select_and_toggle_apply_values() {
    let (_dir, mut tui) = editor(FIXTURE);
    open_section(&mut tui, "settings");
    open_field(&mut tui, "language");
    assert_eq!(tui.mode, Mode::Picker);
    tui.handle_key(Key::Down);
    tui.handle_key(Key::Down);
    tui.handle_key(Key::Enter);
    assert_eq!(get(&tui, "settings", "language"), Some(json!("zh")));
    assert_eq!(tui.message, "Set language = zh");

    open_field(&mut tui, "skip_days");
    assert_eq!(tui.picker_selected, BTreeSet::from([0]));
    tui.handle_key(Key::Down);
    tui.handle_key(Key::Down);
    tui.handle_key(Key::Char(' '));
    tui.handle_key(Key::Up);
    tui.handle_key(Key::Up);
    tui.handle_key(Key::Char(' '));
    assert!(plain(&mut tui).contains("  ▸ [ ] monday\n    [ ] tuesday\n    [✓] wednesday"));
    tui.handle_key(Key::Enter);
    assert_eq!(get(&tui, "settings", "skip_days"), Some(json!(["wednesday"])));
    assert_eq!(tui.message, "Set skip_days = ['wednesday']");

    tui.cursor = tui.rows.iter().position(|row| row.key.as_deref() == Some("show_tasks_after_change")).unwrap();
    tui.handle_key(Key::Char(' '));
    assert_eq!(get(&tui, "settings", "show_tasks_after_change"), Some(json!(false)));
    assert_eq!(tui.message, "Toggled show_tasks_after_change");
    tui.handle_key(Key::Enter);
    assert_eq!(get(&tui, "settings", "show_tasks_after_change"), Some(json!(true)));
}

#[test]
fn keys_can_be_added_and_deleted() {
    let (_dir, mut tui) = editor(FIXTURE);
    open_section(&mut tui, "paths");
    assert!(plain(&mut tui).contains("    (no keys in this section)"));
    tui.handle_key(Key::Char('a'));
    assert_eq!(tui.render(80, 25).title, "  Add Key  ");
    tui.handle_key(Key::Enter);
    assert_eq!(tui.message, "Key name cannot be empty");
    for c in "reports_path".chars() {
        tui.handle_key(Key::Char(c));
    }
    assert!(plain(&mut tui).contains("  Section: [paths]\n\n  Key name: reports_path█"));
    tui.handle_key(Key::Enter);
    assert_eq!(tui.mode, Mode::Browse);
    assert_eq!(tui.message, "Added reports_path — press Enter to set its value");
    assert_eq!(get(&tui, "paths", "reports_path"), Some(json!("")));

    tui.handle_key(Key::Char('a'));
    for c in "reports_path".chars() {
        tui.handle_key(Key::Char(c));
    }
    tui.handle_key(Key::Enter);
    assert_eq!(tui.message, "Key 'reports_path' already exists");
    assert_eq!(tui.mode, Mode::Browse);

    tui.handle_key(Key::Char('d'));
    assert_eq!(tui.message, "Deleted reports_path");
    assert!(tui.rows.is_empty());
    assert!(tui.model.dirty);
}

// ---- rendering ----------------------------------------------------------------

#[test]
fn sections_view_matches_the_python_layout() {
    let (_dir, mut tui) = editor(FIXTURE);
    let rendered = screen(&mut tui, 60, 24);
    let lines: Vec<&str> = rendered.lines().collect();
    assert!(lines[0].starts_with("╭") && lines[0].contains(" ⚙  Settings ") && lines[0].ends_with("╮"));
    assert!(lines[1].starts_with("│   ▸ ⚙"));
    assert!(lines[1].contains("General Settings  (4 keys)"));
    assert!(lines[2].contains("    📋  Task Scheduling  (3 keys)"));
    assert!(lines[4].contains("Task Types  (4 keys)"));
    assert_eq!(lines[5], "│                                                          │");
    assert_eq!(lines[7], "│   [↑↓] Move  [Enter] Open                                │");
    assert_eq!(lines[8], "│   [s] Save  [q] Quit                                     │");
    assert!(lines[9].starts_with("╰") && lines[9].ends_with("╯"));
    assert!(lines[10..].iter().all(|line| line.is_empty()));

    set(&mut tui, "settings", "alarm_interval", json!(10));
    assert_eq!(tui.render(60, 24).title, "⚙  Settings  •  modified");
}

#[test]
fn edit_panels_match_the_python_text() {
    let (_dir, mut tui) = editor(FIXTURE);
    open_section(&mut tui, "settings");
    open_field(&mut tui, "language");
    assert_eq!(plain(&mut tui), "  ▸ en\n    zh\n\n\n  [↑↓] Move  [Enter] Apply\n  [Esc] Cancel");
    tui.handle_key(Key::Esc);
    set(&mut tui, "settings", "alarm_interval", json!(10));
    tui.handle_key(Key::Char('q'));
    let panel = tui.render(80, 25);
    assert_eq!(panel.title, "  Unsaved Changes  ");
    assert_eq!(
        panel.text.plain(),
        "  You have unsaved changes.\n\n  [s] Save and quit\n  [d] Discard and quit\n  [Esc] Keep editing\n"
    );
    let rendered = screen(&mut tui, 60, 24);
    assert!(rendered.lines().next().unwrap().contains("   Unsaved Changes   "));
}

#[test]
fn keys_map_like_readchar() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let press = |code, modifiers| map_key(KeyEvent::new(code, modifiers));
    assert_eq!(press(KeyCode::Char('c'), KeyModifiers::CONTROL), Input::Interrupt);
    assert_eq!(press(KeyCode::Char('h'), KeyModifiers::CONTROL), Input::Key(Key::Backspace));
    assert_eq!(press(KeyCode::Char('m'), KeyModifiers::CONTROL), Input::Key(Key::Enter));
    assert_eq!(press(KeyCode::Char('a'), KeyModifiers::CONTROL), Input::Key(Key::Other));
    assert_eq!(press(KeyCode::Char('Q'), KeyModifiers::SHIFT), Input::Key(Key::Char('Q')));
    assert_eq!(press(KeyCode::Char(' '), KeyModifiers::NONE), Input::Key(Key::Char(' ')));
    assert_eq!(press(KeyCode::Esc, KeyModifiers::NONE), Input::Key(Key::Esc));
    assert_eq!(press(KeyCode::Backspace, KeyModifiers::NONE), Input::Key(Key::Backspace));
    assert_eq!(press(KeyCode::Tab, KeyModifiers::NONE), Input::Key(Key::Other));
    assert_eq!(press(KeyCode::Left, KeyModifiers::NONE), Input::Key(Key::Left));
}
