//! Minimal serializer for settings.toml round-trips.
//!
//! Handles the flat structure settings.toml uses: each section holds scalar
//! values (string, integer, float, boolean) or arrays of scalars.

use std::path::Path;

use serde_json::Value;

use crate::py::{self, Dict};

/// Load a TOML file as an ordered dict of sections.
pub fn load_toml_raw(path: &Path) -> Result<Dict, crate::config::TomlLoadError> {
    crate::config::load_toml_file(path)
}

/// Format a value as a TOML literal.
pub fn format_value(value: &Value) -> Result<String, String> {
    match value {
        Value::Bool(b) => Ok(if *b { "true" } else { "false" }.to_string()),
        Value::Number(n) => Ok(if n.is_f64() { py::float_repr(n.as_f64().unwrap_or(0.0)) } else { n.to_string() }),
        Value::String(s) => Ok(py::dumps(&Value::String(s.clone()))),
        Value::Array(items) => {
            let parts: Result<Vec<String>, String> = items.iter().map(format_value).collect();
            Ok(format!("[{}]", parts?.join(", ")))
        }
        Value::Null => Err("Unsupported TOML value type: NoneType".to_string()),
        Value::Object(_) => Err("Unsupported TOML value type: dict".to_string()),
    }
}

fn is_bare_key(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Render `{section: {key: value}}` as TOML text.
pub fn render_toml(data: &Dict) -> Result<String, String> {
    let mut lines: Vec<String> = Vec::new();
    for (i, (section, values)) in data.iter().enumerate() {
        if i > 0 {
            lines.push(String::new());
        }
        lines.push(format!("[{section}]"));
        let Value::Object(values) = values else {
            continue;
        };
        for (key, value) in values {
            let quoted = if is_bare_key(key) { key.clone() } else { py::dumps(&Value::String(key.clone())) };
            lines.push(format!("{quoted} = {}", format_value(value)?));
        }
    }
    Ok(lines.join("\n") + "\n")
}

/// Write a dict of sections to a TOML file.
pub fn dump_toml(data: &Dict, path: &Path) -> anyhow::Result<()> {
    let text = render_toml(data).map_err(anyhow::Error::msg)?;
    std::fs::write(path, text)?;
    Ok(())
}

/// Where the value starting `text` (the part of a line after `key =`) ends, in
/// bytes: a quoted string, a one-line array, or a bare word such as a number
/// or boolean. `None` for a value that goes on past the line.
fn value_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    match bytes.first()? {
        quote @ (b'"' | b'\'') => {
            let mut index = 1;
            while index < bytes.len() {
                match bytes[index] {
                    b'\\' if *quote == b'"' => index += 2,
                    byte if byte == *quote => return Some(index + 1),
                    _ => index += 1,
                }
            }
            None
        }
        b'[' => {
            let (mut depth, mut index, mut quote) = (0usize, 0, None::<u8>);
            while index < bytes.len() {
                let byte = bytes[index];
                match quote {
                    Some(open) if byte == b'\\' && open == b'"' => index += 1,
                    Some(open) if byte == open => quote = None,
                    Some(_) => {}
                    None if byte == b'"' || byte == b'\'' => quote = Some(byte),
                    None if byte == b'[' => depth += 1,
                    None if byte == b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(index + 1);
                        }
                    }
                    None if byte == b'#' => return None,
                    None => {}
                }
                index += 1;
            }
            None
        }
        _ => Some(text.find('#').map_or(text.len(), |hash| text[..hash].trim_end().len())),
    }
}

/// The table a header line opens (`[tasks]` gives `tasks`), or `None` for a
/// line that is not a header.
fn header_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let inner = trimmed.strip_prefix('[')?;
    // Arrays of tables (`[[x]]`) are tables of their own that nothing here edits.
    if inner.starts_with('[') {
        return Some(format!("[{}", inner.split(']').next().unwrap_or_default().trim()));
    }
    Some(inner.split(']').next().unwrap_or_default().trim().to_string())
}

/// `content` (a TOML file of plain tables) with `[section] key` set to `value`
/// for each edit, changing only those lines so the file's comments and layout
/// stay as they are. A key the table lacks goes after the table's last
/// setting (right under its header when it has none); a table the file lacks
/// is added at its end. `None` when it cannot be done
/// that way and still mean the same as the file with only those values
/// changed (a value spread over several lines, a quoted or dotted key, a file
/// that is not valid TOML); the caller then rewrites the file.
pub fn edit_toml(content: &str, edits: &[(&str, &str, Value)]) -> Option<String> {
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    for (section_name, key_name, value) in edits {
        let assignment = format!("{key_name} = {}", format_value(value).ok()?);
        let mut section = String::new();
        // Where a new key goes: after the table's header, then after each of its settings.
        let mut place = None;
        let mut replaced = false;
        for (index, line) in lines.iter_mut().enumerate() {
            if let Some(name) = header_name(line) {
                if name == *section_name {
                    place = Some(index);
                }
                section = name;
                continue;
            }
            if section != *section_name || line.trim_start().starts_with('#') {
                continue;
            }
            let Some((key, rest)) = line.split_once('=') else { continue };
            place = Some(index);
            if key.trim() != *key_name {
                continue;
            }
            let old_value = rest.trim_start();
            let after = &old_value[value_end(old_value)?..];
            let indent = &line[..line.len() - line.trim_start().len()];
            *line = format!("{indent}{assignment}{after}");
            replaced = true;
            break;
        }
        if replaced {
            continue;
        }
        match place {
            Some(index) => lines.insert(index + 1, assignment),
            None => {
                while lines.last().is_some_and(|line| line.trim().is_empty()) {
                    lines.pop();
                }
                if !lines.is_empty() {
                    lines.push(String::new());
                }
                lines.push(format!("[{section_name}]"));
                lines.push(assignment);
            }
        }
    }
    let edited = lines.join("\n") + "\n";

    let mut expected = crate::config::parse_toml_str(content).ok()?;
    for (section, key, value) in edits {
        let table = expected.entry(section.to_string()).or_insert_with(|| Value::Object(Dict::new()));
        table.as_object_mut()?.insert(key.to_string(), value.clone());
    }
    (crate::config::parse_toml_str(&edited).ok()? == expected).then_some(edited)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn edit(content: &str, edits: &[(&str, &str, Value)]) -> Option<String> {
        edit_toml(content, edits)
    }

    #[test]
    fn edits_values_in_place_and_keeps_comments() {
        let content = "# mine\n[settings]\nskip_days = [\"sunday\"]  # quiet days\n\n[tasks]\ndaily_urgent = [\"08:31\", \"14:01\"] # ask\nhabit_prompt = \"23:50\"\nflag = true # x\n";
        let edited = edit(
            content,
            &[
                ("tasks", "daily_urgent", json!(["09:00"])),
                ("settings", "skip_days", json!([])),
                ("tasks", "flag", json!(false)),
                ("tasks", "habit_prompt", json!("21:30")),
            ],
        )
        .unwrap();
        assert_eq!(
            edited,
            "# mine\n[settings]\nskip_days = []  # quiet days\n\n[tasks]\ndaily_urgent = [\"09:00\"] # ask\nhabit_prompt = \"21:30\"\nflag = false # x\n"
        );
    }

    #[test]
    fn adds_keys_under_their_header_and_new_tables_at_the_end() {
        let content = "[settings]\nlanguage = \"en\"\n\n[tasks]\n# when\n\n";
        let edited = edit(
            content,
            &[
                ("tasks", "habit_prompt", json!("21:30")),
                ("reminders", "block_end", json!(false)),
                ("reminders", "muted_points", json!(["napping", "该喝水了"])),
            ],
        )
        .unwrap();
        assert_eq!(
            edited,
            "[settings]\nlanguage = \"en\"\n\n[tasks]\nhabit_prompt = \"21:30\"\n# when\n\n[reminders]\nblock_end = false\nmuted_points = [\"napping\", \"该喝水了\"]\n"
        );
        assert_eq!(edit("", &[("reminders", "deadline_days", json!(5))]).unwrap(), "[reminders]\ndeadline_days = 5\n");
    }

    #[test]
    fn new_keys_follow_the_table_s_last_setting() {
        let content = "[reminders]\nblock_end = false # quiet\n\n# Times\n[tasks]\n";
        assert_eq!(
            edit(content, &[("reminders", "deadline_days", json!(7))]).unwrap(),
            "[reminders]\nblock_end = false # quiet\ndeadline_days = 7\n\n# Times\n[tasks]\n"
        );
    }

    #[test]
    fn a_key_in_another_table_or_a_comment_is_left_alone() {
        let content = "[tasks]\n# habit_prompt = \"x\"\n[settings]\nhabit_prompt = \"y\"\n";
        assert_eq!(
            edit(content, &[("tasks", "habit_prompt", json!("21:30"))]).unwrap(),
            "[tasks]\nhabit_prompt = \"21:30\"\n# habit_prompt = \"x\"\n[settings]\nhabit_prompt = \"y\"\n"
        );
    }

    #[test]
    fn strings_with_brackets_hashes_and_escapes_are_read_whole() {
        let content =
            "[tasks]\nweekly_review = \"sunday # 20:00\" # c\nlist = [\"a]\", 'b#'] # d\nq = \"say \\\"hi\\\"\"\n";
        assert_eq!(
            edit(
                content,
                &[
                    ("tasks", "weekly_review", json!("monday 09:00")),
                    ("tasks", "list", json!([])),
                    ("tasks", "q", json!("ok"))
                ]
            )
            .unwrap(),
            "[tasks]\nweekly_review = \"monday 09:00\" # c\nlist = [] # d\nq = \"ok\"\n"
        );
    }

    #[test]
    fn shapes_it_cannot_edit_in_place_are_left_to_a_rewrite() {
        let multi_line = "[tasks]\ndaily_urgent = [\n  \"08:31\",\n]\n";
        assert_eq!(edit(multi_line, &[("tasks", "daily_urgent", json!(["09:00"]))]), None);
        assert_eq!(edit("[tasks]\n\"habit_prompt\" = \"x\"\n", &[("tasks", "habit_prompt", json!("y"))]), None);
        assert_eq!(edit("not toml [", &[("tasks", "habit_prompt", json!("y"))]), None);
        assert_eq!(edit("[tasks]\n", &[("tasks", "x", Value::Null)]), None);
    }

    #[test]
    fn renders_sections() {
        let data = json!({"settings": {"a": 1, "b": [true, "x"], "c d": 2.5}, "empty": {}});
        let text = render_toml(data.as_object().unwrap()).unwrap();
        assert_eq!(text, "[settings]\na = 1\nb = [true, \"x\"]\n\"c d\" = 2.5\n\n[empty]\n");
    }
}
