//! Task commands: `rmd add`, `rmd rm`, `rmd cancel`, `rmd drop`, `rmd ls`
//! (port of `commands/tasks.py`). OWNER: CLI agent.
//!
//! The three removal commands all take the task out of tasks.json, but each
//! records a distinct history action so they are not all treated as done:
//! `rm` logs `deleted` (counts as done), `cancel` logs `cancelled` (added by
//! mistake) and `drop` logs `dropped` (gave up).

use std::collections::BTreeSet;

use chrono::{Duration, NaiveDate};
use indexmap::IndexMap;
use serde_json::{Value, json};

use crate::commands::settings::{DEFAULT_TASK_TYPES, SettingsModel};
use crate::config::ScheduleConfig;
use crate::data::{
    get_procrastinate_age_days, load_procrastinate_list, load_procrastinate_records, load_tasks, log_task_action,
    save_procrastinate_list, save_tasks,
};
use crate::i18n::{t, tf};
use crate::py::{self, Dict};
use crate::ui::{BoxStyle, Column, Console, Justify, Panel, Table, Text};

/// Rich colour names that render distinctly in standard/256-colour terminals.
pub const TASK_TYPE_BASE_COLORS: [&str; 40] = [
    "red",
    "green",
    "blue",
    "yellow",
    "magenta",
    "cyan",
    "bright_blue",
    "bright_green",
    "bright_red",
    "bright_yellow",
    "bright_magenta",
    "bright_cyan",
    "orange3",
    "purple",
    "deep_pink1",
    "gold1",
    "dark_orange",
    "navy_blue",
    "spring_green3",
    "deep_sky_blue1",
    "light_sea_green",
    "hot_pink",
    "violet",
    "light_salmon1",
    "chartreuse3",
    "khaki1",
    "cornflower_blue",
    "aquamarine1",
    "orchid",
    "turquoise2",
    "salmon1",
    "medium_purple",
    "sea_green3",
    "steel_blue1",
    "light_coral",
    "dark_violet",
    "green_yellow",
    "dodger_blue1",
    "orange1",
    "slate_blue1",
];

/// Python's `colorsys.hls_to_rgb`.
fn hls_to_rgb(hue: f64, lightness: f64, saturation: f64) -> (f64, f64, f64) {
    if saturation == 0.0 {
        return (lightness, lightness, lightness);
    }
    let m2 =
        if lightness <= 0.5 { lightness * (1.0 + saturation) } else { lightness + saturation - lightness * saturation };
    let m1 = 2.0 * lightness - m2;
    let channel = |hue: f64| {
        let hue = hue.rem_euclid(1.0);
        if hue < 1.0 / 6.0 {
            m1 + (m2 - m1) * hue * 6.0
        } else if hue < 0.5 {
            m2
        } else if hue < 2.0 / 3.0 {
            m1 + (m2 - m1) * (2.0 / 3.0 - hue) * 6.0
        } else {
            m1
        }
    };
    (channel(hue + 1.0 / 3.0), channel(hue), channel(hue - 1.0 / 3.0))
}

/// A distinct colour per task type id: the curated palette while it lasts,
/// then evenly spaced hues so no two types share a colour.
pub fn task_type_colors(sorted_type_ids: &[String]) -> IndexMap<String, String> {
    let total = sorted_type_ids.len();
    if total <= TASK_TYPE_BASE_COLORS.len() {
        return sorted_type_ids.iter().zip(TASK_TYPE_BASE_COLORS).map(|(id, c)| (id.clone(), c.to_string())).collect();
    }
    sorted_type_ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let (r, g, b) = hls_to_rgb(index as f64 / total as f64, 0.55, 0.85);
            let byte = |v: f64| (v * 255.0) as u8;
            (id.clone(), format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b)))
        })
        .collect()
}

/// Task types from settings.toml, or the defaults when there are none.
fn load_task_types() -> IndexMap<String, String> {
    let types = ScheduleConfig::load_active().map(|c| c.task_type_names()).unwrap_or_default();
    if types.is_empty() {
        DEFAULT_TASK_TYPES.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    } else {
        types
    }
}

/// `int(key) if key.isdigit() else 999` (stable sort by it).
fn type_id_sort_key(id: &str) -> i64 {
    if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) { id.parse().unwrap_or(i64::MAX) } else { 999 }
}

fn sorted_type_ids(task_types: &IndexMap<String, String>) -> Vec<String> {
    let mut ids: Vec<String> = task_types.keys().cloned().collect();
    ids.sort_by_key(|id| type_id_sort_key(id));
    ids
}

/// Whether settings ask for the task list after `rmd add`/`rmd rm`.
fn should_show_tasks_after_change() -> bool {
    ScheduleConfig::load_active().map(|c| c.show_tasks_after_change()).unwrap_or(false)
}

/// Add a task type to settings.toml, keeping the rest of the file; `false`
/// when it could not be written.
pub fn save_new_task_type(settings_path: &std::path::Path, type_id: &str, type_name: &str) -> bool {
    let Ok(mut model) = SettingsModel::new(settings_path) else {
        return false;
    };
    model.set("task_types", type_id, Value::String(type_name.to_string()));
    model.save().is_ok()
}

fn cancelled_message(console: &mut Console) {
    console.println(&format!("\n{}", t("👋 Operation cancelled. Have a great day! ✨")));
}

/// Finish creating task type `type_id`: ask for a name, save it, and return
/// the id (`None` when cancelled).
fn prompt_create_task_type(
    console: &mut Console,
    task_types: &mut IndexMap<String, String>,
    type_id: &str,
    settings_path: &std::path::Path,
) -> Option<String> {
    console.print(&format!(
        "[bold green]{}[/bold green]",
        tf("✨ New task type! Let's create type {type_id}.", &[("type_id", &type_id)])
    ));
    let type_name = loop {
        let prompt = tf("🏷️  Enter a name for task type {type_id}: ", &[("type_id", &type_id)]);
        let Ok(input) = console.input(&format!("[bold cyan]{prompt}[/bold cyan]")) else {
            cancelled_message(console);
            return None;
        };
        let name = input.trim().to_string();
        if name.is_empty() {
            console.print(&format!(
                "[bold yellow]{}[/bold yellow]",
                t("⚠️  Oops! Task type name can't be empty. Let's try that again! 😊")
            ));
            continue;
        }
        if task_types.values().any(|v| *v == name) {
            console.print(&format!(
                "[bold yellow]{}[/bold yellow]",
                t("⚠️  Oops! A task type with that name already exists. Let's try that again! 😊")
            ));
            continue;
        }
        break name;
    };
    let saved = save_new_task_type(settings_path, type_id, &type_name);
    task_types.insert(type_id.to_string(), type_name.clone());
    if saved {
        console.print(&format!(
            "[bold green]{}[/bold green]",
            tf(
                "✅ Task type '{type_name}' (number {type_id}) created and saved!",
                &[("type_name", &type_name), ("type_id", &type_id)]
            )
        ));
    } else {
        console.print(&format!(
            "[bold yellow]{}[/bold yellow]",
            t("⚠️  Could not save the new task type to settings.toml; using it for this task only.")
        ));
    }
    Some(type_id.to_string())
}

/// Values gathered for `rmd add` (from arguments or the wizard).
struct AddInput {
    description: Option<String>,
    priority: Option<i64>,
    task_type: Option<i64>,
    postpone: Option<i64>,
}

/// The interactive "Task Creator Wizard": ask for whatever is missing.
/// Returns `false` when the user cancels.
fn run_add_wizard(console: &mut Console, task_types: &mut IndexMap<String, String>, input: &mut AddInput) -> bool {
    let mut welcome = Text::new();
    match &input.description {
        None => {
            welcome.append(&t("Let's add a new task to your schedule! 🚀\n"), "bold green");
            welcome.append(&t("I'll guide you through a couple of quick questions. 🌟"), "cyan");
        }
        Some(description) => {
            welcome.append(&t("Almost there! Let's complete the details for your task. ✨\n"), "bold green");
            welcome.append(&tf("Task: {task_description}", &[("task_description", description)]), "cyan");
        }
    }
    let panel = Panel::new(welcome)
        .title(&format!("[bold green]✨ {} ✨[/bold green]", t("Task Creator Wizard")))
        .border_style("green")
        .padding(1, 2);
    console.render(&panel);

    if input.description.is_none() {
        loop {
            let prompt = format!("[bold cyan]{}[/bold cyan]", t("✍️  What task would you like to add? "));
            let Ok(answer) = console.input(&prompt) else {
                cancelled_message(console);
                return false;
            };
            let answer = answer.trim();
            if !answer.is_empty() {
                input.description = Some(answer.to_string());
                break;
            }
            console.print(&format!(
                "[bold yellow]{}[/bold yellow]",
                t("⚠️  Oops! The task description can't be empty. Let's try that again! 😊")
            ));
        }
    }

    if input.priority.is_none() {
        loop {
            let prompt =
                format!("[bold cyan]{}[/bold cyan]", t("🔢 What is the priority level for this task (1-10)? "));
            let Ok(answer) = console.input(&prompt) else {
                cancelled_message(console);
                return false;
            };
            match py::parse_int_str(answer.trim()) {
                Some(value) if (1..=10).contains(&value) => {
                    input.priority = Some(value);
                    break;
                }
                Some(_) => console.print(&format!(
                    "[bold yellow]{}[/bold yellow]",
                    t("⚠️  Oops! Priority must be between 1 and 10. Let's try that again! 🌟")
                )),
                None => console.print(&format!(
                    "[bold yellow]{}[/bold yellow]",
                    t("⚠️  Oops! Priority needs to be a valid number. Please enter a number between 1 and 10! 🔢")
                )),
            }
        }
    }

    if input.task_type.is_none() {
        console.print(&format!("\n[bold cyan]{}[/bold cyan]", t("🔢 Select a task type:")));
        let mut sorted: Vec<(&String, &String)> = task_types.iter().collect();
        sorted.sort_by_key(|(k, _)| type_id_sort_key(k));
        for (id, name) in sorted {
            console.print(&format!("  [green]{id}[/green]. {name}"));
        }
        console
            .print(&format!("[dim]{}[/dim]", t("💡 Tip: enter a number not listed above to create a new task type.")));
        let settings_path = crate::paths::runtime().settings_path;
        loop {
            let prompt = format!("[bold cyan]{}[/bold cyan]", t("Enter Task Type Number: "));
            let Ok(answer) = console.input(&prompt) else {
                cancelled_message(console);
                return false;
            };
            let answer = answer.trim().to_string();
            if task_types.contains_key(&answer) {
                match py::parse_int_str(&answer) {
                    Some(value) => {
                        input.task_type = Some(value);
                        break;
                    }
                    None => {
                        let error = format!("invalid literal for int() with base 10: {}", py::repr_str(&answer));
                        console.println(&tf("❌ Unexpected error: {e}", &[("e", &error)]));
                        return false;
                    }
                }
            }
            let is_number = !answer.is_empty() && answer.chars().all(|c| c.is_ascii_digit());
            if is_number && py::parse_int_str(&answer).is_some_and(|v| v > 0) {
                match prompt_create_task_type(console, task_types, &answer, &settings_path) {
                    Some(created) => {
                        input.task_type = py::parse_int_str(&created);
                        break;
                    }
                    None => continue,
                }
            }
            console.print(&format!(
                "[bold yellow]{}[/bold yellow]",
                t("⚠️  Oops! Please enter a positive number (an existing one or a new one to create a type).")
            ));
        }

        if input.postpone.is_none() {
            console.print(&format!(
                "\n[bold cyan]{}[/bold cyan]",
                t("🗓️  Postpone the daily urgent alarm? Enter days from now (0 or empty = start today): ")
            ));
            loop {
                let prompt = format!("[bold cyan]{}[/bold cyan]", t("Postpone days (default 0): "));
                let Ok(answer) = console.input(&prompt) else {
                    cancelled_message(console);
                    return false;
                };
                let answer = answer.trim();
                if answer.is_empty() {
                    input.postpone = Some(0);
                    break;
                }
                match py::parse_int_str(answer) {
                    Some(value) if value < 0 => console.print(&format!(
                        "[bold yellow]{}[/bold yellow]",
                        t("⚠️  Oops! Postpone days must be a non-negative integer. Let's try that again! 🌟")
                    )),
                    Some(value) => {
                        input.postpone = Some(value);
                        break;
                    }
                    None => console.print(&format!(
                        "[bold yellow]{}[/bold yellow]",
                        t("⚠️  Oops! Postpone days needs to be a valid number. Please enter a non-negative integer! 🔢")
                    )),
                }
            }
        }
    }
    true
}

/// `rmd add [task] [PRIORITY] [task_type] [postpone]`.
pub fn add_task(task: Option<&str>, priority: Option<i64>, task_type: Option<i64>, postpone: Option<i64>) -> i32 {
    add_task_with(&mut Console::stdout(), task, priority, task_type, postpone)
}

/// [`add_task`] writing to a given console.
pub fn add_task_with(
    console: &mut Console,
    task: Option<&str>,
    priority: Option<i64>,
    task_type: Option<i64>,
    postpone: Option<i64>,
) -> i32 {
    let mut task_types = load_task_types();
    let mut input = AddInput { description: task.map(str::to_string), priority, task_type, postpone };

    if input.description.is_none() || input.priority.is_none() || input.task_type.is_none() {
        if !console.is_interactive() {
            if input.description.is_none() || input.priority.is_none() {
                let mut missing = Vec::new();
                if input.description.is_none() {
                    missing.push("task");
                }
                if input.priority.is_none() {
                    missing.push("PRIORITY");
                }
                console.println(&tf(
                    "❌ Error: the following arguments are required: {missing}",
                    &[("missing", &missing.join(", "))],
                ));
                return 1;
            }
            input.task_type.get_or_insert(1);
        } else if !run_add_wizard(console, &mut task_types, &mut input) {
            return 1;
        }
    }
    let (Some(description), Some(priority)) = (input.description, input.priority) else {
        return 1;
    };

    if priority <= 0 {
        console.println(&t("❌ Error: Priority must be a positive integer"));
        return 1;
    }
    if let Some(task_type) = input.task_type
        && !task_types.contains_key(&task_type.to_string())
    {
        let choices = task_types.keys().cloned().collect::<Vec<_>>().join(", ");
        console.println(&tf("❌ Error: Invalid task type. Choose from: {choices}", &[("choices", &choices)]));
        return 1;
    }
    let mut alarm_from: Option<String> = None;
    if let Some(postpone) = input.postpone {
        if postpone < 0 {
            console.println(&t("❌ Error: Postpone days must be a non-negative integer"));
            return 1;
        }
        if postpone > 0 {
            let date = crate::clock::today() + Duration::days(postpone);
            alarm_from = Some(date.format("%Y-%m-%d").to_string());
        }
    }

    let mut tasks = load_tasks();
    let existing = tasks.iter().position(|task| task.get("description").and_then(Value::as_str) == Some(&description));
    let type_value = input.task_type.map_or_else(|| "None".to_string(), |t| t.to_string());
    let mut new_task = Dict::new();
    new_task.insert("description".into(), Value::String(description.clone()));
    new_task.insert("priority".into(), Value::from(priority));
    new_task.insert("type".into(), Value::String(type_value));
    if let Some(alarm_from) = &alarm_from {
        new_task.insert("alarm_from".into(), Value::String(alarm_from.clone()));
    }
    let new_task = Value::Object(new_task);
    let suffix = alarm_from
        .as_ref()
        .map(|a| tf(" (Daily urgent alarm postponed until {alarm_from})", &[("alarm_from", a)]))
        .unwrap_or_default();

    let action_message = match existing {
        Some(index) => {
            let old_priority = tasks[index].get("priority").cloned().unwrap_or(Value::Null);
            tasks[index] = new_task.clone();
            if let Err(e) = log_task_action("updated", &new_task, Some(&json!({"old_priority": old_priority}))) {
                console.println(&tf("⚠️  Warning: Could not log task update: {e}", &[("e", &e)]));
            }
            tf(
                "✅ Task '{task_description}' updated! Priority changed from {old_priority} to {priority}",
                &[
                    ("task_description", &description),
                    ("old_priority", &py::py_str(&old_priority)),
                    ("priority", &priority),
                ],
            ) + &suffix
        }
        None => {
            tasks.push(new_task.clone());
            if let Err(e) = log_task_action("added", &new_task, None) {
                console.println(&tf("⚠️  Warning: Could not log task addition: {e}", &[("e", &e)]));
            }
            tf(
                "✅ Task '{task_description}' added successfully with priority {priority}!",
                &[("task_description", &description), ("priority", &priority)],
            ) + &suffix
        }
    };

    if let Err(e) = save_tasks(&tasks) {
        console.println(&tf("❌ Error saving task: {e}", &[("e", &e)]));
        return 1;
    }
    console.println(&action_message);
    if should_show_tasks_after_change() {
        show_tasks_with(console, None);
    }
    0
}

// =============================================================================
// Removal commands
// =============================================================================

/// Days until a task's postponed `alarm_from` date (0 when not postponed).
fn days_left(task: &Value, today: NaiveDate) -> i64 {
    match task.get("alarm_from") {
        Some(Value::String(alarm_from)) if !alarm_from.is_empty() => {
            py::strptime_date(alarm_from, "%Y-%m-%d").map(|date| (date - today).num_days()).unwrap_or(0)
        }
        _ => 0,
    }
}

fn description_of(task: &Value) -> String {
    task.get("description").map(py::py_str).unwrap_or_default()
}

fn priority_number(task: &Value) -> f64 {
    task.get("priority").and_then(py::py_float).unwrap_or(0.0)
}

/// Sort into procrastinated, current, then upcoming (postponed) tasks, each by
/// priority, highest first (the order `rmd ls` shows and IDs refer to).
pub fn sort_tasks_by_section_and_priority(
    tasks: &[Value],
    today: NaiveDate,
    procrastinated: &BTreeSet<String>,
) -> Vec<Value> {
    let section = |task: &Value| {
        let future = days_left(task, today) > 0;
        if !future && procrastinated.contains(&description_of(task)) {
            0
        } else if !future {
            1
        } else {
            2
        }
    };
    let mut sorted: Vec<Value> = tasks.to_vec();
    sorted.sort_by(|a, b| section(a).cmp(&section(b)).then_with(|| priority_number(b).total_cmp(&priority_number(a))));
    sorted
}

/// Shared engine for `rm`, `cancel` and `drop`; only the logged action and
/// the messages differ.
fn remove_tasks(
    console: &mut Console,
    identifiers: &[String],
    log_action: &str,
    success_single: &str,
    success_multi: &str,
) -> i32 {
    let mut tasks = load_tasks();
    let mut procrastinate_list = load_procrastinate_list();
    let mut procrastinate_updated = false;

    if tasks.is_empty() {
        console.println(&t("⚠️  No tasks found to delete"));
        return 1;
    }
    let today = crate::clock::today();
    let sorted = sort_tasks_by_section_and_priority(&tasks, today, &procrastinate_list);

    let mut errors: Vec<String> = Vec::new();
    let mut removals: Vec<String> = Vec::new();
    for identifier in identifiers {
        let description = match py::parse_int_str(identifier) {
            Some(task_id) => {
                if task_id < 1 || task_id > sorted.len() as i64 {
                    errors.push(tf(
                        "❌ Invalid task ID: {task_id}. Please use a number between 1 and {length}",
                        &[("task_id", &task_id), ("length", &sorted.len())],
                    ));
                    continue;
                }
                description_of(&sorted[(task_id - 1) as usize])
            }
            None => identifier.clone(),
        };
        let original_count = tasks.len();
        let (removed, kept): (Vec<Value>, Vec<Value>) =
            tasks.into_iter().partition(|task| task.get("description").and_then(Value::as_str) == Some(&description));
        tasks = kept;
        if removed.is_empty() {
            errors.push(tf("❌ Task '{task_description}' not found", &[("task_description", &description)]));
            continue;
        }
        for task in &removed {
            if let Err(e) = log_task_action(log_action, task, None) {
                console.println(&tf("⚠️  Warning: Could not log task deletion: {e}", &[("e", &e)]));
                break;
            }
        }
        for task in &removed {
            if let Some(Value::String(d)) = task.get("description")
                && procrastinate_list.remove(d)
            {
                procrastinate_updated = true;
            }
        }
        let removed_count = original_count - tasks.len();
        removals.push(if removed_count == 1 {
            tf("Task '{task_description}'", &[("task_description", &description)])
        } else {
            tf(
                "{deleted_count} tasks with description '{task_description}'",
                &[("deleted_count", &removed_count), ("task_description", &description)],
            )
        });
    }

    for error in &errors {
        console.println(error);
    }
    if removals.is_empty() {
        return 1;
    }
    let saved = save_tasks(&tasks)
        .and_then(|()| if procrastinate_updated { save_procrastinate_list(&procrastinate_list, None) } else { Ok(()) });
    if let Err(e) = saved {
        console.println(&tf("❌ Error saving tasks: {e}", &[("e", &e)]));
        return 1;
    }
    if removals.len() == 1 {
        console.println(&crate::i18n::format(success_single, &[("deletion", &removals[0])]));
    } else {
        console.println(&crate::i18n::format(success_multi, &[("count", &removals.len())]));
        for removal in &removals {
            console.println(&format!("   - {removal}"));
        }
    }
    if should_show_tasks_after_change() {
        show_tasks_with(console, None);
    }
    if errors.is_empty() { 0 } else { 1 }
}

/// `rmd rm TASK...`: remove tasks by ID or description (counts as done).
pub fn delete_task(identifiers: &[String]) -> i32 {
    delete_task_with(&mut Console::stdout(), identifiers)
}

/// [`delete_task`] writing to a given console.
pub fn delete_task_with(console: &mut Console, identifiers: &[String]) -> i32 {
    remove_tasks(
        console,
        identifiers,
        "deleted",
        &t("✅ {deletion} deleted successfully!"),
        &t("✅ {count} sets of tasks deleted successfully:"),
    )
}

/// `rmd cancel TASK...`: remove tasks added by mistake (not counted as done).
pub fn cancel_task(identifiers: &[String]) -> i32 {
    cancel_task_with(&mut Console::stdout(), identifiers)
}

/// [`cancel_task`] writing to a given console.
pub fn cancel_task_with(console: &mut Console, identifiers: &[String]) -> i32 {
    remove_tasks(
        console,
        identifiers,
        "cancelled",
        &t("🚫 {deletion} cancelled (not counted as done)."),
        &t("🚫 {count} sets of tasks cancelled (not counted as done):"),
    )
}

/// `rmd drop TASK...`: give up on tasks (not counted as done).
pub fn drop_task(identifiers: &[String]) -> i32 {
    drop_task_with(&mut Console::stdout(), identifiers)
}

/// [`drop_task`] writing to a given console.
pub fn drop_task_with(console: &mut Console, identifiers: &[String]) -> i32 {
    remove_tasks(
        console,
        identifiers,
        "dropped",
        &t("🏳️ {deletion} dropped (gave up)."),
        &t("🏳️ {count} sets of tasks dropped (gave up):"),
    )
}

// =============================================================================
// rmd ls
// =============================================================================

fn procrastination_suffix(age_days: Option<i64>) -> String {
    match age_days {
        None => String::new(),
        Some(0) => t(" (deferred today)"),
        Some(1) => t(" (1 day overdue)"),
        Some(days) => tf(" ({age_days} days overdue)", &[("age_days", &days)]),
    }
}

fn postpone_suffix(days_left: i64) -> String {
    match days_left {
        d if d <= 0 => String::new(),
        1 => t(" (coming tomorrow)"),
        d => tf(" (coming in {days_left} days)", &[("days_left", &d)]),
    }
}

/// Resolve a `--type` filter (an id, or a name matched case-insensitively).
pub fn resolve_task_type_id(value: &str, task_types: &IndexMap<String, String>) -> Option<String> {
    if task_types.contains_key(value) {
        return Some(value.to_string());
    }
    let lowered = value.to_lowercase();
    task_types.iter().find(|(_, name)| name.to_lowercase() == lowered).map(|(id, _)| id.clone())
}

/// `rmd ls [-t TYPE]`: the task list as a table.
pub fn show_tasks(type_filter: Option<&str>) -> i32 {
    show_tasks_with(&mut Console::stdout(), type_filter)
}

/// [`show_tasks`] writing to a given console.
pub fn show_tasks_with(console: &mut Console, type_filter: Option<&str>) -> i32 {
    let tasks = load_tasks();
    let records = load_procrastinate_records();
    let procrastinated: BTreeSet<String> = records.keys().cloned().collect();
    let today = crate::clock::today();

    if tasks.is_empty() {
        console.print(&format!("[bold yellow]{}[/bold yellow]", t("📋 No tasks found")));
        return 0;
    }
    let sorted = sort_tasks_by_section_and_priority(&tasks, today, &procrastinated);
    let task_types = load_task_types();
    let type_ids = sorted_type_ids(&task_types);

    let mut filter_type_id: Option<String> = None;
    if let Some(filter) = type_filter.filter(|f| !f.is_empty()) {
        match resolve_task_type_id(filter, &task_types) {
            Some(id) => filter_type_id = Some(id),
            None => {
                let mut pairs: Vec<(&String, &String)> = task_types.iter().collect();
                pairs.sort_by(|a, b| a.0.cmp(b.0));
                let valid = pairs.iter().map(|(id, name)| format!("{id}={name}")).collect::<Vec<_>>().join(", ");
                console.print(&format!(
                    "[bold red]{}[/bold red]",
                    tf(
                        "❌ Unknown task type '{type_filter}'. Valid types: {valid}",
                        &[("type_filter", &filter), ("valid", &valid)]
                    )
                ));
                return 1;
            }
        }
    }
    let colors = task_type_colors(&type_ids);

    let mut table = Table::new()
        .title(&format!("[bold]{}[/bold]", t("Current Task List")))
        .box_style(BoxStyle::Rounded)
        .header_style("bold cyan")
        .expand(true);
    table.add_column(Column::new(&t("ID")).justify(Justify::Right).style("dim").width(4).no_wrap());
    table.add_column(Column::new(&t("Priority")).no_wrap());
    table.add_column(Column::new(&t("Description")));

    let default_type = type_ids.first().cloned().unwrap_or_else(|| "1".to_string());
    let mut displayed = 0;
    for (index, task) in sorted.iter().enumerate() {
        let description = description_of(task);
        let type_id = task.get("type").map(py::py_str).unwrap_or_else(|| default_type.clone());
        if filter_type_id.as_ref().is_some_and(|f| *f != type_id) {
            continue;
        }
        let left = days_left(task, today);
        let postponed = left > 0;
        let color = colors.get(&type_id).cloned().unwrap_or_else(|| "white".to_string());
        let priority_value = task.get("priority").cloned().unwrap_or(Value::Null);
        let priority = priority_value.as_i64().unwrap_or_else(|| priority_number(task) as i64);
        let filled = "█".repeat(priority.clamp(0, 10) as usize);
        let empty = "░".repeat((10 - priority.min(10)).max(0) as usize);
        let visual = format!("[{color}]{filled}[dim]{empty}[/dim] ({})[/{color}]", py::py_str(&priority_value));
        let description_text = if postponed {
            Text::styled(&format!("💤 {description}{}", postpone_suffix(left)), "italic dim")
        } else if procrastinated.contains(&description) {
            let since = records.get(&description).and_then(|r| r.get("since")).and_then(Value::as_str);
            let age = get_procrastinate_age_days(since, Some(today));
            let overdue = age.is_some_and(|a| a >= 1);
            Text::styled(
                &format!("⏳ {description}{}", procrastination_suffix(age)),
                if overdue { "bold red" } else { "italic dim" },
            )
        } else {
            Text::raw(&description)
        };
        table.add_row([Text::from((index + 1).to_string()), Text::from(visual), description_text]);
        displayed += 1;
    }

    if let Some(filter_id) = &filter_type_id
        && displayed == 0
    {
        let type_name = task_types.get(filter_id).cloned().unwrap_or_else(|| "other".to_string());
        console.print(&format!(
            "[bold yellow]{}[/bold yellow]",
            tf("📋 No tasks found for type '{type_name}'", &[("type_name", &type_name)])
        ));
        return 0;
    }
    let legend = type_ids
        .iter()
        .map(|id| {
            let color = &colors[id];
            format!("[{color}]■ {}[/{color}]", task_types[id])
        })
        .collect::<Vec<_>>()
        .join("  ");
    console.render(&table);
    console.print(&legend);
    let footer = match &filter_type_id {
        Some(id) => tf(
            "Showing {shown} of {total} tasks (type: {type_name})",
            &[
                ("shown", &displayed),
                ("total", &tasks.len()),
                ("type_name", &task_types.get(id).cloned().unwrap_or_else(|| "other".to_string())),
            ],
        ),
        None => tf("Total tasks: {count}", &[("count", &tasks.len())]),
    };
    console.print_justified(&format!("[dim]{footer}[/dim]"), Justify::Right);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;
    use crate::ui::PromptError;

    const SETTINGS: &str = "[settings]\nlanguage = \"en\"\n\n[task_types]\n1 = \"read papers\"\n2 = \"gym work\"\n3 = \"coding\"\n4 = \"other\"\n";

    fn env_with(tasks: Value) -> TestEnv {
        let mut env = TestEnv::new();
        env.set_now("2026-04-08T09:00:00");
        env.write("user_config_0/settings.toml", SETTINGS);
        env.write("tasks/tasks.json", &py::dumps_pretty(&tasks));
        env
    }

    fn saved_tasks(env: &TestEnv) -> Vec<Value> {
        serde_json::from_str::<Vec<Value>>(&env.read("tasks/tasks.json")).unwrap()
    }

    fn logged_actions(env: &TestEnv) -> Vec<String> {
        let log: Vec<Value> = serde_json::from_str(&env.read("tasks/tasks.log")).unwrap_or_default();
        log.iter().map(|e| e["action"].as_str().unwrap_or_default().to_string()).collect()
    }

    fn descriptions(tasks: &[Value]) -> Vec<String> {
        tasks.iter().map(description_of).collect()
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn quiet() -> Console {
        Console::capture(80).with_interactive(false)
    }

    #[test]
    fn add_task_saves_and_logs() {
        let env = env_with(json!([]));
        let mut console = quiet();
        assert_eq!(add_task_with(&mut console, Some("Complete project"), Some(7), None, None), 0);
        assert_eq!(console.output(), "✅ Task 'Complete project' added successfully with priority 7!\n");
        assert_eq!(saved_tasks(&env), vec![json!({"description": "Complete project", "priority": 7, "type": "1"})]);
        assert_eq!(logged_actions(&env), ["added"]);
        let log: Vec<Value> = serde_json::from_str(&env.read("tasks/tasks.log")).unwrap();
        assert!(log[0]["timestamp"].as_str().unwrap().ends_with("+00:00"));
        assert_eq!(log[0]["task"], json!({"description": "Complete project", "priority": 7, "type": "1"}));
    }

    #[test]
    fn add_task_with_type_and_postpone() {
        let env = env_with(json!([]));
        let mut console = quiet();
        assert_eq!(add_task_with(&mut console, Some("Workout"), Some(6), Some(2), Some(2)), 0);
        assert_eq!(
            console.output(),
            "✅ Task 'Workout' added successfully with priority 6! (Daily urgent alarm postponed until 2026-04-10)\n"
        );
        assert_eq!(
            saved_tasks(&env),
            vec![json!({"description": "Workout", "priority": 6, "type": "2", "alarm_from": "2026-04-10"})]
        );
        let mut console = quiet();
        assert_eq!(add_task_with(&mut console, Some("Now"), Some(6), Some(2), Some(0)), 0);
        assert!(saved_tasks(&env)[1].get("alarm_from").is_none());
    }

    #[test]
    fn add_existing_task_updates_priority() {
        let env = env_with(json!([
            {"description": "Complete project", "priority": 5},
            {"description": "Review code", "priority": 3}
        ]));
        let mut console = quiet();
        assert_eq!(add_task_with(&mut console, Some("Complete project"), Some(9), None, None), 0);
        assert_eq!(console.output(), "✅ Task 'Complete project' updated! Priority changed from 5 to 9\n");
        let tasks = saved_tasks(&env);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0], json!({"description": "Complete project", "priority": 9, "type": "1"}));
        let log: Vec<Value> = serde_json::from_str(&env.read("tasks/tasks.log")).unwrap();
        assert_eq!(log[0]["action"], "updated");
        assert_eq!(log[0]["metadata"], json!({"old_priority": 5}));
    }

    #[test]
    fn add_task_rejects_bad_values() {
        let env = env_with(json!([]));
        for (priority, postpone, message) in [
            (-1, None, "❌ Error: Priority must be a positive integer\n"),
            (0, None, "❌ Error: Priority must be a positive integer\n"),
            (5, Some(-1), "❌ Error: Postpone days must be a non-negative integer\n"),
        ] {
            let mut console = quiet();
            assert_eq!(add_task_with(&mut console, Some("x"), Some(priority), None, postpone), 1);
            assert_eq!(console.output(), message);
        }
        let mut console = quiet();
        assert_eq!(add_task_with(&mut console, Some("x"), Some(5), Some(9), None), 1);
        assert_eq!(console.output(), "❌ Error: Invalid task type. Choose from: 1, 2, 3, 4\n");
        assert_eq!(saved_tasks(&env), Vec::<Value>::new());
    }

    #[test]
    fn add_task_missing_arguments_without_terminal() {
        let _env = env_with(json!([]));
        let mut console = quiet();
        assert_eq!(add_task_with(&mut console, None, None, None, None), 1);
        assert_eq!(console.output(), "❌ Error: the following arguments are required: task, PRIORITY\n");
        let mut console = quiet();
        assert_eq!(add_task_with(&mut console, Some("x"), None, None, None), 1);
        assert_eq!(console.output(), "❌ Error: the following arguments are required: PRIORITY\n");
    }

    #[test]
    fn add_task_shows_list_only_when_enabled() {
        let env = env_with(json!([]));
        let mut console = quiet();
        add_task_with(&mut console, Some("Quiet"), Some(7), None, None);
        assert!(!console.output().contains("Current Task List"));
        env.write(
            "user_config_0/settings.toml",
            &SETTINGS.replace("[settings]\n", "[settings]\nshow_tasks_after_change = true\n"),
        );
        let mut console = quiet();
        add_task_with(&mut console, Some("Loud"), Some(7), None, None);
        assert!(console.output().contains("Current Task List"));
        assert!(console.output().contains("Total tasks: 2"));
    }

    fn answers(items: &[&str]) -> Vec<Result<String, PromptError>> {
        items.iter().map(|s| Ok(s.to_string())).collect()
    }

    #[test]
    fn wizard_prompts_for_missing_values() {
        let env = env_with(json!([]));
        let mut console = Console::capture(80).with_inputs(answers(&["Buy groceries", "5", "2", ""]));
        assert_eq!(add_task_with(&mut console, None, None, None, None), 0);
        assert_eq!(saved_tasks(&env), vec![json!({"description": "Buy groceries", "priority": 5, "type": "2"})]);
        let output = console.output();
        assert!(output.contains("✨ Task Creator Wizard ✨"));
        assert!(output.contains("Let's add a new task to your schedule! 🚀"));
        assert!(output.contains("  1. read papers\n"));

        let mut console = Console::capture(80).with_inputs(answers(&["8", "1", ""]));
        assert_eq!(add_task_with(&mut console, Some("Study science"), None, None, None), 0);
        assert_eq!(saved_tasks(&env)[1], json!({"description": "Study science", "priority": 8, "type": "1"}));
        assert!(console.output().contains("Task: Study science"));
    }

    #[test]
    fn wizard_retries_invalid_answers() {
        let env = env_with(json!([]));
        let mut console = Console::capture(80).with_inputs(answers(&[
            "",
            "Valid Task",
            "abc",
            "12",
            "6",
            "x",
            "3",
            "abc",
            "-2",
            "3",
        ]));
        assert_eq!(add_task_with(&mut console, None, None, None, None), 0);
        assert_eq!(
            saved_tasks(&env),
            vec![json!({"description": "Valid Task", "priority": 6, "type": "3", "alarm_from": "2026-04-11"})]
        );
        let output = console.output();
        assert!(output.contains("The task description can't be empty"));
        assert!(output.contains("Priority needs to be a valid number"));
        assert!(output.contains("Priority must be between 1 and 10"));
        assert!(output.contains("Please enter a positive number"));
        assert!(output.contains("Postpone days needs to be a valid number"));
        assert!(output.contains("Postpone days must be a non-negative integer"));
    }

    #[test]
    fn wizard_cancel_returns_1() {
        let env = env_with(json!([]));
        let mut console = Console::capture(80).with_inputs([Err(PromptError::Interrupted)]);
        assert_eq!(add_task_with(&mut console, None, None, None, None), 1);
        assert!(console.output().ends_with("\n👋 Operation cancelled. Have a great day! ✨\n"));
        assert_eq!(env.read("tasks/tasks.json"), "[]");
    }

    #[test]
    fn wizard_creates_a_new_task_type() {
        let env = env_with(json!([]));
        let mut console = Console::capture(80).with_inputs(answers(&["Write novel", "7", "5", "writing", ""]));
        assert_eq!(add_task_with(&mut console, None, None, None, None), 0);
        assert_eq!(saved_tasks(&env)[0]["type"], "5");
        let settings = ScheduleConfig::load_active().unwrap();
        assert_eq!(settings.task_type_names()["5"], "writing");
        assert_eq!(settings.task_type_names()["1"], "read papers");
        assert!(console.output().contains("✅ Task type 'writing' (number 5) created and saved!"));

        // An empty or duplicate name is asked again.
        let mut console =
            Console::capture(80).with_inputs(answers(&["Read book", "5", "6", "", "coding", "reading", ""]));
        assert_eq!(add_task_with(&mut console, None, None, None, None), 0);
        assert_eq!(saved_tasks(&env)[1]["type"], "6");
        assert!(console.output().contains("Task type name can't be empty"));
        assert!(console.output().contains("A task type with that name already exists"));
    }

    #[test]
    fn wizard_new_type_cancel_returns_to_type_prompt() {
        let env = env_with(json!([]));
        let inputs = vec![
            Ok("Plan trip".to_string()),
            Ok("5".to_string()),
            Ok("7".to_string()),
            Err(PromptError::Interrupted),
            Ok("2".to_string()),
            Ok(String::new()),
        ];
        let mut console = Console::capture(80).with_inputs(inputs);
        assert_eq!(add_task_with(&mut console, None, None, None, None), 0);
        assert_eq!(saved_tasks(&env)[0]["type"], "2");
        assert!(!ScheduleConfig::load_active().unwrap().task_type_names().contains_key("7"));
    }

    #[test]
    fn wizard_uses_new_type_even_when_settings_cannot_be_saved() {
        let env = env_with(json!([]));
        std::fs::remove_file(env.root().join("user_config_0/settings.toml")).unwrap();
        let mut console = Console::capture(80).with_inputs(answers(&["Sketch", "5", "9", "drawing", ""]));
        assert_eq!(add_task_with(&mut console, None, None, None, None), 0);
        assert_eq!(saved_tasks(&env)[0]["type"], "9");
        assert!(console.output().contains("Could not save the new task type to settings.toml"));
    }

    #[test]
    fn save_new_task_type_keeps_the_rest_of_settings() {
        let env = env_with(json!([]));
        let path = env.root().join("user_config_0/settings.toml");
        assert!(save_new_task_type(&path, "5", "writing"));
        assert!(save_new_task_type(&path, "2", "yoga"));
        let config = ScheduleConfig::load(&path).unwrap();
        assert_eq!(config.task_type_names()["1"], "read papers");
        assert_eq!(config.task_type_names()["2"], "yoga");
        assert_eq!(config.task_type_names()["5"], "writing");
        assert_eq!(config.settings["language"], "en");
        assert!(!save_new_task_type(&env.root().join("missing.toml"), "1", "x"));
    }

    fn three_tasks() -> Value {
        json!([
            {"description": "High priority task", "priority": 9},
            {"description": "Medium priority task", "priority": 5},
            {"description": "Low priority task", "priority": 2}
        ])
    }

    #[test]
    fn delete_by_description_and_id() {
        let env = env_with(three_tasks());
        let mut console = quiet();
        assert_eq!(delete_task_with(&mut console, &strings(&["Medium priority task"])), 0);
        assert_eq!(console.output(), "✅ Task 'Medium priority task' deleted successfully!\n");
        assert_eq!(descriptions(&saved_tasks(&env)), ["High priority task", "Low priority task"]);
        assert_eq!(logged_actions(&env), ["deleted"]);
        drop(env);

        let env = env_with(three_tasks());
        for (id, gone) in [("2", "Medium priority task"), ("1", "High priority task")] {
            let mut console = quiet();
            assert_eq!(delete_task_with(&mut console, &strings(&[id])), 0);
            assert!(!descriptions(&saved_tasks(&env)).contains(&gone.to_string()));
        }
        assert_eq!(descriptions(&saved_tasks(&env)), ["Low priority task"]);
    }

    #[test]
    fn delete_reports_missing_tasks_and_ids() {
        let env = env_with(json!([{"description": "Task 1", "priority": 5}, {"description": "Task 2", "priority": 3}]));
        let mut console = quiet();
        assert_eq!(delete_task_with(&mut console, &strings(&["Non-existent task"])), 1);
        assert_eq!(console.output(), "❌ Task 'Non-existent task' not found\n");
        for id in ["5", "0", "-1", "123"] {
            let mut console = quiet();
            assert_eq!(delete_task_with(&mut console, &strings(&[id])), 1);
            assert_eq!(console.output(), format!("❌ Invalid task ID: {id}. Please use a number between 1 and 2\n"));
        }
        assert_eq!(saved_tasks(&env).len(), 2);
        drop(env);

        let _env = env_with(json!([]));
        let mut console = quiet();
        assert_eq!(delete_task_with(&mut console, &strings(&["Any task"])), 1);
        assert_eq!(console.output(), "⚠️  No tasks found to delete\n");
    }

    #[test]
    fn delete_many_mixes_ids_descriptions_and_failures() {
        let env = env_with(json!([
            {"description": "High priority task", "priority": 9},
            {"description": "Code review", "priority": 7},
            {"description": "Documentation", "priority": 5},
            {"description": "Testing", "priority": 3}
        ]));
        let mut console = quiet();
        assert_eq!(delete_task_with(&mut console, &strings(&["1", "Documentation", "4"])), 0);
        assert_eq!(descriptions(&saved_tasks(&env)), ["Code review"]);
        assert_eq!(
            console.output(),
            "✅ 3 sets of tasks deleted successfully:\n   - Task 'High priority task'\n   - Task 'Documentation'\n   - Task 'Testing'\n"
        );
        drop(env);

        let env = env_with(json!([{"description": "Task 1", "priority": 5}, {"description": "Task 2", "priority": 3}]));
        let mut console = quiet();
        assert_eq!(delete_task_with(&mut console, &strings(&["1", "5", "-1", "2"])), 1);
        assert!(saved_tasks(&env).is_empty());
        assert_eq!(
            console.output(),
            "❌ Invalid task ID: 5. Please use a number between 1 and 2\n❌ Invalid task ID: -1. Please use a number between 1 and 2\n✅ 2 sets of tasks deleted successfully:\n   - Task 'Task 1'\n   - Task 'Task 2'\n"
        );
    }

    #[test]
    fn delete_removes_all_duplicates() {
        let env = env_with(json!([
            {"description": "Review code", "priority": 7},
            {"description": "Write documentation", "priority": 3},
            {"description": "Review code", "priority": 5}
        ]));
        let mut console = quiet();
        assert_eq!(delete_task_with(&mut console, &strings(&["Review code"])), 0);
        assert_eq!(console.output(), "✅ 2 tasks with description 'Review code' deleted successfully!\n");
        assert_eq!(descriptions(&saved_tasks(&env)), ["Write documentation"]);
        assert_eq!(logged_actions(&env), ["deleted", "deleted"]);
    }

    #[test]
    fn removal_keeps_procrastinate_list_in_sync() {
        let env = env_with(json!([
            {"description": "Review code", "priority": 9},
            {"description": "Write tests", "priority": 5}
        ]));
        env.write(
            "tasks/procrastinate.json",
            "[{\"description\": \"Review code\", \"since\": \"2026-04-01\"}, \"Write tests\"]",
        );
        let mut console = quiet();
        assert_eq!(drop_task_with(&mut console, &strings(&["Review code"])), 0);
        assert_eq!(console.output(), "🏳️ Task 'Review code' dropped (gave up).\n");
        assert_eq!(logged_actions(&env), ["dropped"]);
        let saved: Value = serde_json::from_str(&env.read("tasks/procrastinate.json")).unwrap();
        assert_eq!(saved, json!([{"description": "Write tests", "since": "2026-04-08"}]));
    }

    #[test]
    fn cancel_and_drop_log_their_own_actions() {
        let env = env_with(json!([
            {"description": "Typo task", "priority": 5},
            {"description": "Side project", "priority": 4},
            {"description": "Other", "priority": 9}
        ]));
        let mut console = quiet();
        assert_eq!(cancel_task_with(&mut console, &strings(&["Typo task"])), 0);
        assert_eq!(console.output(), "🚫 Task 'Typo task' cancelled (not counted as done).\n");
        let mut console = quiet();
        assert_eq!(drop_task_with(&mut console, &strings(&["2"])), 0);
        assert_eq!(console.output(), "🏳️ Task 'Side project' dropped (gave up).\n");
        assert_eq!(logged_actions(&env), ["cancelled", "dropped"]);
        assert_eq!(descriptions(&saved_tasks(&env)), ["Other"]);
        let mut console = quiet();
        assert_eq!(cancel_task_with(&mut console, &strings(&["Ghost task"])), 1);
        assert!(console.output().contains("Ghost task"));
    }

    fn table_rows(output: &str) -> Vec<String> {
        output
            .lines()
            .filter(|l| l.starts_with('│') && !l.contains(" ID "))
            .map(|l| {
                let cells: Vec<&str> = l.split('│').collect();
                format!("{}|{}", cells[1].trim(), cells[3].trim())
            })
            .collect()
    }

    #[test]
    fn show_tasks_orders_sections_and_marks_tasks() {
        let env = env_with(json!([
            {"description": "Task 1 (proc 5)", "priority": 5},
            {"description": "Task 2 (curr 9)", "priority": 9},
            {"description": "Task 3 (inc 10)", "priority": 10, "alarm_from": "2026-04-10"},
            {"description": "Task 4 (proc 8)", "priority": 8},
            {"description": "Task 5 (curr 3)", "priority": 3},
            {"description": "Task 6 (inc 2)", "priority": 2, "alarm_from": "2026-04-09"},
            {"description": "Task 7 (today)", "priority": 1, "alarm_from": "2026-04-08"}
        ]));
        env.write(
            "tasks/procrastinate.json",
            "[{\"description\": \"Task 1 (proc 5)\", \"since\": \"2026-04-05\"}, {\"description\": \"Task 4 (proc 8)\", \"since\": \"2026-04-08\"}]",
        );
        let mut console = Console::capture(120);
        assert_eq!(show_tasks_with(&mut console, None), 0);
        assert_eq!(
            table_rows(console.output()),
            [
                "1|⏳ Task 4 (proc 8) (deferred today)",
                "2|⏳ Task 1 (proc 5) (3 days overdue)",
                "3|Task 2 (curr 9)",
                "4|Task 5 (curr 3)",
                "5|Task 7 (today)",
                "6|💤 Task 3 (inc 10) (coming in 2 days)",
                "7|💤 Task 6 (inc 2) (coming tomorrow)",
            ]
        );
        assert!(console.output().contains("■ read papers  ■ gym work  ■ coding  ■ other\n"));
        assert!(console.output().ends_with("Total tasks: 7\n"));
    }

    #[test]
    fn show_tasks_filters_by_type_keeping_ids() {
        let _env = env_with(json!([
            {"description": "Task A", "priority": 3, "type": "1"},
            {"description": "Task B", "priority": 9, "type": "1"},
            {"description": "Task C", "priority": 7, "type": "2"}
        ]));
        let mut console = Console::capture(100);
        assert_eq!(show_tasks_with(&mut console, Some("1")), 0);
        assert_eq!(table_rows(console.output()), ["1|Task B", "3|Task A"]);
        assert!(console.output().ends_with("Showing 2 of 3 tasks (type: read papers)\n"));

        let mut console = Console::capture(100);
        assert_eq!(show_tasks_with(&mut console, Some("GYM WORK")), 0);
        assert_eq!(table_rows(console.output()), ["2|Task C"]);

        let mut console = Console::capture(100);
        assert_eq!(show_tasks_with(&mut console, Some("4")), 0);
        assert_eq!(console.output(), "📋 No tasks found for type 'other'\n");

        let mut console = Console::capture(100);
        assert_eq!(show_tasks_with(&mut console, Some("nonexistent")), 1);
        assert_eq!(
            console.output(),
            "❌ Unknown task type 'nonexistent'. Valid types: 1=read papers, 2=gym work, 3=coding, 4=other\n"
        );
    }

    #[test]
    fn show_tasks_empty_list() {
        let _env = env_with(json!([]));
        let mut console = Console::capture(80);
        assert_eq!(show_tasks_with(&mut console, Some("nonexistent")), 0);
        assert_eq!(console.output(), "📋 No tasks found\n");
    }

    #[test]
    fn task_type_colors_never_repeat() {
        let ids = |n: usize| (1..=n).map(|i| i.to_string()).collect::<Vec<_>>();
        let small = task_type_colors(&ids(4));
        assert_eq!(small.values().cloned().collect::<Vec<_>>(), TASK_TYPE_BASE_COLORS[..4]);
        let exact = task_type_colors(&ids(TASK_TYPE_BASE_COLORS.len()));
        assert_eq!(exact.values().collect::<BTreeSet<_>>().len(), TASK_TYPE_BASE_COLORS.len());
        let large = task_type_colors(&ids(TASK_TYPE_BASE_COLORS.len() + 25));
        assert_eq!(large.values().collect::<BTreeSet<_>>().len(), TASK_TYPE_BASE_COLORS.len() + 25);
        assert!(large.values().all(|c| c.starts_with('#') && c.len() == 7));
        assert_eq!(
            [&large["1"], &large["2"], &large["33"], &large["65"]],
            ["#ed2a2a", "#ed3c2a", "#2aede4", "#ed2a3c"]
        );
    }

    #[test]
    fn resolve_task_type_id_by_id_or_name() {
        let types: IndexMap<String, String> =
            [("1", "read papers"), ("2", "gym work")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        assert_eq!(resolve_task_type_id("1", &types).as_deref(), Some("1"));
        assert_eq!(resolve_task_type_id("gym work", &types).as_deref(), Some("2"));
        assert_eq!(resolve_task_type_id("GYM WORK", &types).as_deref(), Some("2"));
        assert_eq!(resolve_task_type_id("zzz", &types), None);
    }
}
