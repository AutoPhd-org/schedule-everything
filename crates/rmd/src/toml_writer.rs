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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_sections() {
        let data = json!({"settings": {"a": 1, "b": [true, "x"], "c d": 2.5}, "empty": {}});
        let text = render_toml(data.as_object().unwrap()).unwrap();
        assert_eq!(text, "[settings]\na = 1\nb = [true, \"x\"]\n\"c d\" = 2.5\n\n[empty]\n");
    }
}
