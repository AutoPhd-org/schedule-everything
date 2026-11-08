//! First-run config wizard used by the installer: creates missing config
//! files from templates and fills in settings keys the template has but
//! `settings.toml` lacks.
//!
//! The installer runs `rmd __setup-config --config-dir DIR [--template-dir DIR] [--yes]`.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::anyhow;
use clap::Parser;
use toml::{Table, Value};

use crate::i18n::{t, tf};
use crate::py;

/// Required config files and the templates they are created from.
pub const REQUIRED_CONFIG_FILES: [(&str, &str); 4] = [
    ("settings.toml", "settings_template.toml"),
    ("odd_weeks.toml", "week_schedule_template.toml"),
    ("even_weeks.toml", "week_schedule_template.toml"),
    ("habits.toml", "habits_template.toml"),
];

/// Sections whose keys you choose yourself: time block names, reminder names,
/// and task type IDs. The template's entries there are only examples, so an
/// entry you removed is not missing; the examples are offered only when the
/// whole section is.
pub const USER_DEFINED_SECTIONS: [&str; 3] = ["time_blocks", "time_points", "task_types"];

/// One `[section] key = value` setting.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingEntry {
    pub section: String,
    pub key: String,
    pub value: Value,
}

/// Reads one answer for a prompt; `None` at end of input (Python's `EOFError`).
pub type InputFunc<'a> = dyn FnMut(&str) -> Option<String> + 'a;

/// Why a settings file could not be loaded.
enum LoadError {
    Io(io::Error),
    Decode(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(error) => write!(f, "{}", crate::commands::service::os_error_text(error, None)),
            LoadError::Decode(message) => write!(f, "{message}"),
        }
    }
}

fn load_toml_file(path: &Path) -> Result<Table, LoadError> {
    let bytes = std::fs::read(path).map_err(LoadError::Io)?;
    let text = String::from_utf8(bytes).map_err(|error| LoadError::Decode(error.to_string()))?;
    text.parse::<Table>().map_err(|error| LoadError::Decode(crate::config::clean_toml_error(&error)))
}

fn ask_yes_no(prompt: &str, default: bool, auto_yes: bool, input: &mut InputFunc) -> bool {
    if auto_yes {
        println!("{prompt} [auto-yes]");
        return true;
    }
    let suffix = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        let Some(answer) = input(&format!("{prompt} {suffix}: ")) else {
            println!("{}", t("Input stream closed; using default answer."));
            return default;
        };
        match answer.trim().to_lowercase().as_str() {
            "" => return default,
            "y" | "yes" => return true,
            "n" | "no" => return false,
            _ => println!("{}", t("Please answer with 'y' or 'n'.")),
        }
    }
}

/// Python's type name for a TOML value (for error messages).
fn type_name(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "str",
        Value::Integer(_) => "int",
        Value::Float(_) => "float",
        Value::Boolean(_) => "bool",
        Value::Array(_) => "list",
        Value::Table(_) => "dict",
        Value::Datetime(datetime) => match (datetime.date, datetime.time) {
            (Some(_), Some(_)) => "datetime",
            (Some(_), None) => "date",
            _ => "time",
        },
    }
}

/// A value as a TOML literal (strings JSON-quoted like the Python version).
pub fn format_toml_value(value: &Value) -> anyhow::Result<String> {
    Ok(match value {
        Value::String(text) => py::dumps(&serde_json::Value::String(text.clone())),
        Value::Boolean(flag) => if *flag { "true" } else { "false" }.to_string(),
        Value::Integer(number) => number.to_string(),
        Value::Float(number) => py::float_repr(*number),
        Value::Array(items) => {
            let parts = items.iter().map(format_toml_value).collect::<anyhow::Result<Vec<_>>>()?;
            format!("[{}]", parts.join(", "))
        }
        other => return Err(anyhow!("Unsupported config value type: {}", type_name(other))),
    })
}

/// Python `float(text)`: underscores are allowed only between digits.
fn parse_py_float(text: &str) -> Option<f64> {
    let chars: Vec<char> = text.chars().collect();
    let underscores_ok = chars.iter().enumerate().all(|(i, c)| {
        *c != '_'
            || (i > 0 && chars[i - 1].is_ascii_digit() && chars.get(i + 1).is_some_and(|next| next.is_ascii_digit()))
    });
    if !underscores_ok {
        return None;
    }
    text.replace('_', "").parse().ok()
}

/// Parse an answer into the type of the template's default value.
pub fn parse_user_value(raw_value: &str, default_value: &Value) -> Result<Value, String> {
    match default_value {
        Value::Boolean(_) => match raw_value.trim().to_lowercase().as_str() {
            "true" | "t" | "yes" | "y" | "1" => Ok(Value::Boolean(true)),
            "false" | "f" | "no" | "n" | "0" => Ok(Value::Boolean(false)),
            _ => Err("expected true/false".to_string()),
        },
        Value::Integer(_) => {
            py::parse_int_str(raw_value.trim()).map(Value::Integer).ok_or_else(|| "expected an integer".to_string())
        }
        Value::Float(_) => parse_py_float(raw_value.trim()).map(Value::Float).ok_or_else(|| "expected a number".into()),
        Value::Array(defaults) => {
            let stripped = raw_value.trim();
            if stripped.starts_with('[') && stripped.ends_with(']') {
                let parsed = format!("value = {stripped}")
                    .parse::<Table>()
                    .map_err(|_| "expected a valid TOML list".to_string())?
                    .remove("value");
                return match parsed {
                    Some(list @ Value::Array(_)) => Ok(list),
                    _ => Err("expected a list".to_string()),
                };
            }
            let items: Vec<&str> = stripped.split(',').map(str::trim).filter(|item| !item.is_empty()).collect();
            if items.is_empty() {
                return Ok(Value::Array(Vec::new()));
            }
            // Python's `isinstance(item, int)` is also true for booleans.
            let integer_list = !defaults.is_empty()
                && defaults.iter().all(|item| matches!(item, Value::Integer(_) | Value::Boolean(_)));
            if integer_list {
                return items
                    .iter()
                    .map(|item| py::parse_int_str(item).map(Value::Integer))
                    .collect::<Option<Vec<_>>>()
                    .map(Value::Array)
                    .ok_or_else(|| "expected comma-separated integers".to_string());
            }
            Ok(Value::Array(items.into_iter().map(|item| Value::String(item.to_string())).collect()))
        }
        _ => Ok(Value::String(raw_value.to_string())),
    }
}

/// Template settings missing from the current settings, in template order.
pub fn collect_missing_settings(current_data: &Table, template_data: &Table) -> Vec<SettingEntry> {
    let empty = Table::new();
    let mut missing = Vec::new();
    for (section, template_section) in template_data {
        let Value::Table(template_section) = template_section else {
            continue;
        };
        let current_section = match current_data.get(section) {
            Some(Value::Table(table)) => table,
            _ => &empty,
        };
        if USER_DEFINED_SECTIONS.contains(&section.as_str()) && !current_section.is_empty() {
            continue;
        }
        for (key, default_value) in template_section {
            if !current_section.contains_key(key) {
                missing.push(SettingEntry { section: section.clone(), key: key.clone(), value: default_value.clone() });
            }
        }
    }
    missing
}

/// Python `str.splitlines()`.
fn splitlines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                lines.push(std::mem::take(&mut current));
            }
            '\n' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
                lines.push(std::mem::take(&mut current));
            }
            other => current.push(other),
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// `{section: (start, end)}` line ranges of the `[section]` headers in `lines`.
fn find_section_bounds(lines: &[String]) -> Vec<(String, (usize, usize))> {
    let headers: Vec<(String, usize)> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            let stripped = line.trim();
            if stripped.len() >= 2 && stripped.starts_with('[') && stripped.ends_with(']') && !stripped.contains('=') {
                let name = stripped[1..stripped.len() - 1].trim();
                (!name.is_empty()).then(|| (name.to_string(), index))
            } else {
                None
            }
        })
        .collect();
    let mut bounds: Vec<(String, (usize, usize))> = Vec::new();
    for (position, (name, start)) in headers.iter().enumerate() {
        let end = headers.get(position + 1).map_or(lines.len(), |(_, next)| *next);
        // A repeated header replaces the earlier one, like a dict assignment.
        match bounds.iter_mut().find(|(known, _)| known == name) {
            Some(entry) => entry.1 = (*start, end),
            None => bounds.push((name.clone(), (*start, end))),
        }
    }
    bounds
}

/// Insert `updates` into `settings_path`, at the end of their sections
/// (appending sections that do not exist), keeping everything else as written.
pub fn write_missing_settings(settings_path: &Path, updates: &[SettingEntry]) -> anyhow::Result<()> {
    if !settings_path.exists() {
        if let Some(parent) = settings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(settings_path, "")?;
    }
    let mut lines = splitlines(&std::fs::read_to_string(settings_path)?);

    let mut grouped: Vec<(&str, Vec<&SettingEntry>)> = Vec::new();
    for entry in updates {
        match grouped.iter_mut().find(|(section, _)| *section == entry.section) {
            Some((_, entries)) => entries.push(entry),
            None => grouped.push((&entry.section, vec![entry])),
        }
    }
    for (section, entries) in grouped {
        let insertion = entries
            .iter()
            .map(|entry| Ok(format!("{} = {}", entry.key, format_toml_value(&entry.value)?)))
            .collect::<anyhow::Result<Vec<_>>>()?;
        if let Some((_, (_, end))) = find_section_bounds(&lines).into_iter().find(|(name, _)| name == section) {
            lines.splice(end..end, insertion);
            continue;
        }
        if lines.last().is_some_and(|last| !last.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("[{section}]"));
        lines.extend(insertion);
    }

    let mut content = lines.join("\n").trim_end().to_string();
    if !content.is_empty() {
        content.push('\n');
    }
    std::fs::write(settings_path, content)?;
    Ok(())
}

fn ensure_required_files(
    config_dir: &Path,
    template_dir: &Path,
    auto_yes: bool,
    input: &mut InputFunc,
) -> anyhow::Result<bool> {
    for (config_name, template_name) in REQUIRED_CONFIG_FILES {
        let config_path = config_dir.join(config_name);
        if config_path.exists() {
            continue;
        }
        let template_path = template_dir.join(template_name);
        if !template_path.exists() {
            println!("{}", tf("Missing required template: {path}", &[("path", &template_path.display())]));
            return Ok(false);
        }
        let prompt = tf(
            "{config_name} is missing. Create it from {template_name}?",
            &[("config_name", &config_name), ("template_name", &template_name)],
        );
        if !ask_yes_no(&prompt, true, auto_yes, input) {
            println!("{}", tf("Cannot continue without {config_name}.", &[("config_name", &config_name)]));
            return Ok(false);
        }
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&template_path, &config_path)?;
        println!("{}", tf("Created {config_path}", &[("config_path", &config_path.display())]));
    }
    Ok(true)
}

/// The current settings; `None` when they are invalid and not replaced.
fn load_current_settings(
    settings_path: &Path,
    settings_template_path: &Path,
    auto_yes: bool,
    input: &mut InputFunc,
) -> anyhow::Result<Option<Table>> {
    if !settings_path.exists() {
        return Ok(Some(Table::new()));
    }
    match load_toml_file(settings_path) {
        Ok(table) => Ok(Some(table)),
        Err(LoadError::Io(error)) => Err(error.into()),
        Err(LoadError::Decode(message)) => {
            println!(
                "{}",
                tf(
                    "Invalid TOML in {settings_path}: {exc}",
                    &[("settings_path", &settings_path.display()), ("exc", &message)]
                )
            );
            if !settings_template_path.exists() {
                println!("{}", t("settings_template.toml is unavailable; cannot auto-repair settings.toml"));
                return Ok(None);
            }
            let prompt = t("Replace settings.toml with settings_template.toml and continue?");
            if !ask_yes_no(&prompt, false, auto_yes, input) {
                return Ok(None);
            }
            std::fs::copy(settings_template_path, settings_path)?;
            load_toml_file(settings_path).map(Some).map_err(|error| anyhow!("{error}"))
        }
    }
}

/// Create missing config files and fill in missing settings. `Ok(false)` when
/// the user declined a required step or a file could not be used.
pub fn run_wizard(
    config_dir: &Path,
    template_dir: &Path,
    auto_yes: bool,
    input: &mut InputFunc,
) -> anyhow::Result<bool> {
    std::fs::create_dir_all(config_dir)?;
    if !ensure_required_files(config_dir, template_dir, auto_yes, input)? {
        return Ok(false);
    }

    let settings_template_path = template_dir.join("settings_template.toml");
    if !settings_template_path.exists() {
        println!("{}", t("settings_template.toml not found; skipping missing-key checks."));
        return Ok(true);
    }
    let template_settings = match load_toml_file(&settings_template_path) {
        Ok(table) => table,
        Err(error) => {
            println!("{}", tf("Failed to load settings template: {exc}", &[("exc", &error)]));
            return Ok(false);
        }
    };

    let settings_path = config_dir.join("settings.toml");
    let Some(current_settings) = load_current_settings(&settings_path, &settings_template_path, auto_yes, input)?
    else {
        return Ok(false);
    };

    let missing = collect_missing_settings(&current_settings, &template_settings);
    if missing.is_empty() {
        println!("{}", t("Configuration check complete. No missing keys found."));
        return Ok(true);
    }
    println!("{}", tf("Found {count} missing configuration value(s).", &[("count", &missing.len())]));

    let mut resolved: Vec<SettingEntry> = Vec::new();
    for entry in missing {
        let default_display = format_toml_value(&entry.value)?;
        let names: [(&str, &dyn std::fmt::Display); 3] =
            [("section", &entry.section), ("key", &entry.key), ("default_display", &default_display)];
        if auto_yes {
            println!("{}", tf("Missing [{section}].{key}; using default {default_display}", &names));
            resolved.push(entry);
            continue;
        }
        let prompt = tf("Enter value for [{section}].{key} (press Enter for {default_display}): ", &names);
        loop {
            let response = input(&prompt).ok_or_else(|| anyhow!("EOF when reading a line"))?;
            let response = response.trim();
            if response.is_empty() {
                resolved.push(entry.clone());
                break;
            }
            match parse_user_value(response, &entry.value) {
                Ok(value) => {
                    resolved.push(SettingEntry { value, ..entry.clone() });
                    break;
                }
                Err(error) => println!("{}", tf("Invalid value: {exc}", &[("exc", &error)])),
            }
        }
    }

    write_missing_settings(&settings_path, &resolved)?;
    println!(
        "{}",
        tf(
            "Updated {settings_path} with {count} missing value(s).",
            &[("settings_path", &settings_path.display()), ("count", &resolved.len())]
        )
    );
    Ok(true)
}

/// `input(prompt)`: print the prompt, read one line from stdin.
fn read_stdin_line(prompt: &str) -> Option<String> {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    match io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            Some(line)
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "rmd __setup-config",
    about = "Validate and complete required reminder configuration files.",
    infer_long_args = true,
    args_override_self = true
)]
struct Args {
    /// Target configuration directory (e.g. ~/.schedule_management/config)
    #[arg(long, value_name = "CONFIG_DIR")]
    config_dir: String,
    /// Directory containing settings_template.toml and other templates.
    #[arg(long, value_name = "TEMPLATE_DIR")]
    template_dir: Option<String>,
    /// Use template defaults without interactive prompts.
    #[arg(long)]
    yes: bool,
}

fn run_with(argv: Vec<String>, stdin_is_tty: bool, input: &mut InputFunc) -> i32 {
    let args = match Args::try_parse_from(std::iter::once("rmd __setup-config".to_string()).chain(argv)) {
        Ok(args) => args,
        Err(error) => {
            let _ = error.print();
            return error.exit_code();
        }
    };
    let config_dir = crate::paths::expand_user(&args.config_dir);
    let template_dir: PathBuf =
        args.template_dir.as_deref().map(crate::paths::expand_user).unwrap_or_else(|| config_dir.clone());

    if !args.yes && !stdin_is_tty {
        println!("{}", t("Interactive prompts require a TTY. Re-run with --yes to accept defaults."));
        return 1;
    }
    match run_wizard(&config_dir, &template_dir, args.yes, input) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

/// Run the wizard with the arguments after `rmd __setup-config`; returns the exit code.
pub fn run(argv: Vec<String>) -> i32 {
    run_with(argv, io::stdin().is_terminal(), &mut read_stdin_line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn load(path: &Path) -> Table {
        std::fs::read_to_string(path).unwrap().parse().unwrap()
    }

    /// Prompts the fake input has been asked, shared with the test.
    type Prompts = std::rc::Rc<std::cell::RefCell<Vec<String>>>;

    /// Input that answers with `responses` in order, then reports end of input.
    fn answers(responses: &[&str]) -> (impl FnMut(&str) -> Option<String>, Prompts) {
        let prompts = Prompts::default();
        let seen = prompts.clone();
        let mut queue: std::collections::VecDeque<String> = responses.iter().map(|r| r.to_string()).collect();
        (
            move |prompt: &str| {
                seen.borrow_mut().push(prompt.to_string());
                queue.pop_front()
            },
            prompts,
        )
    }

    const SETTINGS_TEMPLATE: &str = "[settings]\nsound_file = \"/tmp/ping.aiff\"\nalarm_interval = 5\n\n[paths]\nreports_path = \"~/Desktop/reports\"\n";

    fn write_templates(template_dir: &Path, settings_template: &str) {
        write(&template_dir.join("settings_template.toml"), settings_template);
        write(&template_dir.join("week_schedule_template.toml"), "[monday]\n\"09:00\" = \"pomodoro\"\n");
        write(&template_dir.join("habits_template.toml"), "[habits]\n1 = \"Read\"\n");
    }

    #[test]
    fn run_wizard_creates_required_files() {
        let env = TestEnv::new();
        let (config_dir, template_dir) = (env.root().join("config"), env.root().join("templates"));
        write_templates(&template_dir, SETTINGS_TEMPLATE);
        let (mut input, prompts) = answers(&[]);

        assert!(run_wizard(&config_dir, &template_dir, true, &mut input).unwrap());
        for name in ["settings.toml", "odd_weeks.toml", "even_weeks.toml", "habits.toml"] {
            assert!(config_dir.join(name).exists(), "{name}");
        }
        assert!(prompts.borrow().is_empty());
    }

    #[test]
    fn run_wizard_prompts_for_missing_settings() {
        let env = TestEnv::new();
        let (config_dir, template_dir) = (env.root().join("config"), env.root().join("templates"));
        write(&config_dir.join("settings.toml"), "[settings]\nsound_file = \"/tmp/ping.aiff\"\n");
        write(&config_dir.join("odd_weeks.toml"), "[monday]\n\"09:00\" = \"pomodoro\"\n");
        write(&config_dir.join("even_weeks.toml"), "[monday]\n\"09:00\" = \"pomodoro\"\n");
        write(&config_dir.join("habits.toml"), "[habits]\n1 = \"Read\"\n");
        write_templates(&template_dir, SETTINGS_TEMPLATE);
        let (mut input, prompts) = answers(&["7", "~/Desktop/custom-reports"]);

        assert!(run_wizard(&config_dir, &template_dir, false, &mut input).unwrap());
        let data = load(&config_dir.join("settings.toml"));
        assert_eq!(data["settings"]["alarm_interval"].as_integer(), Some(7));
        assert_eq!(data["paths"]["reports_path"].as_str(), Some("~/Desktop/custom-reports"));
        assert_eq!(
            *prompts.borrow(),
            vec![
                "Enter value for [settings].alarm_interval (press Enter for 5): ".to_string(),
                "Enter value for [paths].reports_path (press Enter for \"~/Desktop/reports\"): ".to_string(),
            ]
        );
        assert_eq!(
            std::fs::read_to_string(config_dir.join("settings.toml")).unwrap(),
            "[settings]\nsound_file = \"/tmp/ping.aiff\"\nalarm_interval = 7\n\n[paths]\nreports_path = \"~/Desktop/custom-reports\"\n"
        );
    }

    fn write_config_and_templates(config_dir: &Path, template_dir: &Path, settings: &str, template: &str) {
        write(&config_dir.join("settings.toml"), &(settings.trim().to_string() + "\n"));
        write(&config_dir.join("odd_weeks.toml"), "[monday]\n\"09:00\" = \"potato\"\n");
        write(&config_dir.join("even_weeks.toml"), "[monday]\n\"09:00\" = \"potato\"\n");
        write(&config_dir.join("habits.toml"), "[habits]\n1 = \"Read\"\n");
        write_templates(template_dir, &(template.trim().to_string() + "\n"));
    }

    const EXAMPLE_TEMPLATE: &str = r#"
[settings]
sound_file = "/tmp/ping.aiff"
alarm_interval = 5

[time_blocks]
pomodoro = 25
potato = 50

[time_points]
go_to_bed = "Time for bed"

[task_types]
1 = "read papers"
2 = "gym work"
3 = "coding"
4 = "other"
"#;

    #[test]
    fn run_wizard_does_not_bring_back_removed_user_defined_entries() {
        let env = TestEnv::new();
        let (config_dir, template_dir) = (env.root().join("config"), env.root().join("templates"));
        // Task types 2 and 4, the pomodoro block, and the bedtime reminder were all removed on purpose.
        write_config_and_templates(
            &config_dir,
            &template_dir,
            r#"
[settings]
sound_file = "/tmp/ping.aiff"

[time_blocks]
potato = 50

[time_points]
lunch = "Lunch time"

[task_types]
1 = "TA"
3 = "diary"
18 = "Intern"
"#,
            EXAMPLE_TEMPLATE,
        );
        let (mut input, prompts) = answers(&["7"]);

        assert!(run_wizard(&config_dir, &template_dir, false, &mut input).unwrap());
        // Only the missing fixed setting is asked for.
        assert_eq!(prompts.borrow().len(), 1);
        assert!(prompts.borrow()[0].contains("[settings].alarm_interval"));
        let data = load(&config_dir.join("settings.toml"));
        assert_eq!(data["settings"]["alarm_interval"].as_integer(), Some(7));
        assert_eq!(data["task_types"], "1 = \"TA\"\n3 = \"diary\"\n18 = \"Intern\"\n".parse::<Table>().unwrap().into());
        assert_eq!(data["time_blocks"], "potato = 50".parse::<Table>().unwrap().into());
        assert_eq!(data["time_points"], "lunch = \"Lunch time\"".parse::<Table>().unwrap().into());
    }

    #[test]
    fn run_wizard_offers_examples_for_a_missing_user_defined_section() {
        let env = TestEnv::new();
        let (config_dir, template_dir) = (env.root().join("config"), env.root().join("templates"));
        write_config_and_templates(
            &config_dir,
            &template_dir,
            r#"
[settings]
sound_file = "/tmp/ping.aiff"
alarm_interval = 5

[time_blocks]
potato = 50

[time_points]
lunch = "Lunch time"
"#,
            EXAMPLE_TEMPLATE,
        );
        let (mut input, _) = answers(&[]);

        assert!(run_wizard(&config_dir, &template_dir, true, &mut input).unwrap());
        let data = load(&config_dir.join("settings.toml"));
        let expected: Table = "1 = \"read papers\"\n2 = \"gym work\"\n3 = \"coding\"\n4 = \"other\"".parse().unwrap();
        assert_eq!(data["task_types"], expected.into());
        assert_eq!(data["time_blocks"], "potato = 50".parse::<Table>().unwrap().into());
    }

    #[test]
    fn run_wizard_aborts_when_required_file_creation_declined() {
        let env = TestEnv::new();
        let (config_dir, template_dir) = (env.root().join("config"), env.root().join("templates"));
        write(&template_dir.join("settings_template.toml"), "[settings]\nsound_file = \"/tmp/ping.aiff\"\n");
        write(&template_dir.join("week_schedule_template.toml"), "[monday]\n");
        write(&template_dir.join("habits_template.toml"), "[habits]\n1 = \"Read\"\n");
        let (mut input, prompts) = answers(&["n"]);

        assert!(!run_wizard(&config_dir, &template_dir, false, &mut input).unwrap());
        assert!(!config_dir.join("settings.toml").exists());
        assert_eq!(
            *prompts.borrow(),
            vec!["settings.toml is missing. Create it from settings_template.toml? [Y/n]: ".to_string()]
        );
    }

    #[test]
    fn invalid_settings_can_be_replaced_with_the_template() {
        let env = TestEnv::new();
        let (config_dir, template_dir) = (env.root().join("config"), env.root().join("templates"));
        write_config_and_templates(&config_dir, &template_dir, "[settings", SETTINGS_TEMPLATE);
        // Declining keeps the broken file and fails.
        let (mut input, _) = answers(&["maybe", ""]);
        assert!(!run_wizard(&config_dir, &template_dir, false, &mut input).unwrap());
        assert_eq!(std::fs::read_to_string(config_dir.join("settings.toml")).unwrap(), "[settings\n");
        // End of input takes the default (no) as well.
        let (mut input, _) = answers(&[]);
        assert!(!run_wizard(&config_dir, &template_dir, false, &mut input).unwrap());

        let (mut input, _) = answers(&["yes"]);
        assert!(run_wizard(&config_dir, &template_dir, false, &mut input).unwrap());
        assert_eq!(std::fs::read_to_string(config_dir.join("settings.toml")).unwrap(), SETTINGS_TEMPLATE);
    }

    #[test]
    fn answers_are_parsed_by_the_default_type() {
        let int = Value::Integer(5);
        assert_eq!(parse_user_value(" 1_0 ", &int), Ok(Value::Integer(10)));
        assert_eq!(parse_user_value("x", &int), Err("expected an integer".into()));
        assert_eq!(parse_user_value("Yes", &Value::Boolean(false)), Ok(Value::Boolean(true)));
        assert_eq!(parse_user_value("0", &Value::Boolean(true)), Ok(Value::Boolean(false)));
        assert_eq!(parse_user_value("maybe", &Value::Boolean(true)), Err("expected true/false".into()));
        assert_eq!(parse_user_value("2.5", &Value::Float(1.0)), Ok(Value::Float(2.5)));
        assert_eq!(parse_user_value("1_000.5", &Value::Float(1.0)), Ok(Value::Float(1000.5)));
        assert_eq!(parse_user_value("1__0", &Value::Float(1.0)), Err("expected a number".into()));

        let times = Value::Array(vec![Value::String("10:00".into())]);
        assert_eq!(
            parse_user_value("09:00, 18:00,", &times),
            Ok(Value::Array(vec![Value::String("09:00".into()), Value::String("18:00".into())]))
        );
        assert_eq!(parse_user_value(r#"["a", 1]"#, &times), Ok(Value::Array(vec!["a".into(), 1.into()])));
        assert_eq!(parse_user_value("[a]", &times), Err("expected a valid TOML list".into()));
        assert_eq!(parse_user_value("  ", &times), Ok(Value::Array(Vec::new())));
        let numbers = Value::Array(vec![Value::Integer(1)]);
        assert_eq!(parse_user_value("1, 2", &numbers), Ok(Value::Array(vec![1.into(), 2.into()])));
        assert_eq!(parse_user_value("1, x", &numbers), Err("expected comma-separated integers".into()));
        assert_eq!(parse_user_value("free text", &Value::String(String::new())), Ok("free text".into()));
    }

    #[test]
    fn values_are_formatted_as_toml_literals() {
        let value: Value = "v = [\"é \\\"q\\\"\", 1, 2.0, true, 1e-5]".parse::<Table>().unwrap().remove("v").unwrap();
        assert_eq!(format_toml_value(&value).unwrap(), r#"["é \"q\"", 1, 2.0, true, 1e-05]"#);
        let table = Value::Table(Table::new());
        assert_eq!(format_toml_value(&table).unwrap_err().to_string(), "Unsupported config value type: dict");
    }

    #[test]
    fn missing_settings_go_to_the_end_of_their_section() {
        let env = TestEnv::new();
        let path = env.write("settings.toml", "# top\r\n[settings]\r\na = 1\r\n\r\n[paths]\r\nx = \"y\"");
        let entry = |section: &str, key: &str, value: Value| SettingEntry {
            section: section.to_string(),
            key: key.to_string(),
            value,
        };
        write_missing_settings(
            &path,
            &[
                entry("settings", "b", Value::Integer(2)),
                entry("tasks", "daily_urgent", Value::Array(vec!["10:00".into()])),
                entry("settings", "c", Value::Boolean(true)),
            ],
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# top\n[settings]\na = 1\n\nb = 2\nc = true\n[paths]\nx = \"y\"\n\n[tasks]\ndaily_urgent = [\"10:00\"]\n"
        );
    }

    #[test]
    fn command_line_requires_a_tty_unless_yes() {
        let env = TestEnv::new();
        let config_dir = env.root().join("config").display().to_string();
        let (mut input, _) = answers(&[]);
        assert_eq!(run_with(vec!["--config-dir".into(), config_dir.clone()], false, &mut input), 1);
        assert!(!env.root().join("config").exists());

        let template_dir = env.root().join("templates");
        write_templates(&template_dir, SETTINGS_TEMPLATE);
        let argv = vec![
            "--config-dir".to_string(),
            config_dir,
            "--template-dir".into(),
            template_dir.display().to_string(),
            "--yes".into(),
        ];
        assert_eq!(run_with(argv, false, &mut input), 0);
        assert!(env.root().join("config/settings.toml").exists());
        // Missing --config-dir is a usage error.
        assert_eq!(run_with(vec!["--yes".into()], true, &mut input), 2);
    }

    #[test]
    fn splitlines_matches_python() {
        assert_eq!(splitlines("a\nb\r\nc\rd\n"), vec!["a", "b", "c", "d"]);
        assert_eq!(splitlines("a\n\nb"), vec!["a", "", "b"]);
        assert!(splitlines("").is_empty());
    }
}
