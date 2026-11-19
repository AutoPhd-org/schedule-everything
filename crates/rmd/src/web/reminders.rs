//! Settings → Reminders: which reminders ring and when.
//!
//! The switches live in `[reminders]` of the active settings.toml (every kind
//! is on unless switched off; see [`ReminderKind`]), the times in `[tasks]`,
//! and the quiet days in `[settings] skip_days`. Saving edits only the lines
//! it changes, so the file's comments stay; the reminder service notices the
//! change on its next check of the clock (see
//! [`crate::runner::ScheduleRunner::reload_if_changed`]).

use serde_json::{Value, json};

use crate::commands::settings::{SettingsModel, WEEKDAYS};
use crate::config::{
    DEFAULT_DEADLINE_DAYS, DEFAULT_URGENT_IMPORTANCE, MAX_DEADLINE_DAYS, ReminderKind, ScheduleConfig, WeeklySchedule,
};
use crate::py::{self, Dict};
use crate::toml_writer::edit_toml;
use crate::web::services::{WebError, WebResult, invalid, os_error, snapshot_runtime_paths};

/// Times a kind gets when it is switched on without any.
fn default_time(kind: ReminderKind) -> Value {
    match kind {
        ReminderKind::UrgentTasks | ReminderKind::Deadlines => json!(["09:00"]),
        ReminderKind::DailySummary => json!("22:00"),
        ReminderKind::HabitPrompt => json!("21:30"),
        ReminderKind::WeeklyReport => json!("sunday 20:00"),
        ReminderKind::MonthlyReport => json!("1 20:00"),
        ReminderKind::BlockStart | ReminderKind::BlockEnd | ReminderKind::TimePoints => Value::Null,
    }
}

/// `HH:MM` for a time written `H:MM` or `HH:MM`, the way the service compares
/// times; `None` for anything else.
fn clock_time(text: &str) -> Option<String> {
    let text = text.trim();
    if !crate::commands::settings::valid_time(text) {
        return None;
    }
    let (hour, minute) = text.split_once(':')?;
    Some(format!("{:02}:{minute}", hour.parse::<u32>().ok()?))
}

/// `"sunday 20:00"` as `(weekday, time)`.
fn weekly_parts(text: &str) -> Option<(String, String)> {
    let [day, time] = text.split_whitespace().collect::<Vec<_>>()[..] else { return None };
    let day = day.to_lowercase();
    WEEKDAYS.contains(&day.as_str()).then_some(())?;
    Some((day, clock_time(time)?))
}

/// `"1 20:00"` as `(day of month, time)`.
fn monthly_parts(text: &str) -> Option<(i64, String)> {
    let [day, time] = text.split_whitespace().collect::<Vec<_>>()[..] else { return None };
    let day = py::parse_int_str(day).filter(|day| (1..=31).contains(day))?;
    Some((day, clock_time(time)?))
}

/// The `[tasks]` key a kind's times are saved under: its alias when the file
/// uses that one (the service reads `daily_urgency` before `daily_urgent`).
fn time_key(config: &ScheduleConfig, kind: ReminderKind) -> Option<&'static str> {
    let key = kind.time_key()?;
    let alias = match kind {
        ReminderKind::UrgentTasks => Some("daily_urgency"),
        ReminderKind::Deadlines => Some("ddl_urgency"),
        ReminderKind::HabitPrompt => Some("habit_tracking"),
        _ => None,
    };
    Some(match alias {
        Some(alias) if kind == ReminderKind::HabitPrompt && !config.tasks.contains_key(key) => {
            if config.tasks.contains_key(alias) { alias } else { key }
        }
        Some(alias) if kind != ReminderKind::HabitPrompt && config.tasks.contains_key(alias) => alias,
        _ => key,
    })
}

/// When a kind rings, as the browser shows it: a list of times, one time,
/// `{day, time}`, or `null` when it has none (or none the service can use).
fn times_of(config: &ScheduleConfig, kind: ReminderKind) -> Value {
    let valid = |times: Vec<String>| {
        let mut times: Vec<String> = times.iter().filter_map(|time| clock_time(time)).collect();
        times.sort();
        times.dedup();
        json!(times)
    };
    let single = |text: String| clock_time(&text).map_or(Value::Null, Value::String);
    match kind {
        ReminderKind::UrgentTasks => valid(config.daily_urgent_times()),
        ReminderKind::Deadlines => valid(config.ddl_urgent_times()),
        ReminderKind::DailySummary => single(config.daily_summary_time()),
        ReminderKind::HabitPrompt => single(config.habit_prompt_time()),
        ReminderKind::WeeklyReport => weekly_parts(&config.weekly_review_time())
            .map_or(Value::Null, |(day, time)| json!({"day": day, "time": time})),
        ReminderKind::MonthlyReport => monthly_parts(&config.monthly_review_time())
            .map_or(Value::Null, |(day, time)| json!({"day": day, "time": time})),
        ReminderKind::BlockStart | ReminderKind::BlockEnd | ReminderKind::TimePoints => Value::Null,
    }
}

/// Whether `times` (from [`times_of`]) says when to ring.
fn has_times(times: &Value) -> bool {
    match times {
        Value::Array(items) => !items.is_empty(),
        Value::Null => false,
        _ => true,
    }
}

/// The plain messages the week schedules ring (labels that are neither a time
/// block nor a time point), in the order they first appear.
fn schedule_messages(config: &ScheduleConfig, weekly: &WeeklySchedule) -> Vec<String> {
    let mut messages: Vec<String> = Vec::new();
    for week in [&weekly.odd_data, &weekly.even_data] {
        for day in week.values() {
            let Value::Object(events) = day else { continue };
            for event in events.values() {
                let Value::String(label) = event else { continue };
                if !config.time_blocks.contains_key(label)
                    && !config.time_points.contains_key(label)
                    && !messages.contains(label)
                {
                    messages.push(label.clone());
                }
            }
        }
    }
    messages
}

fn load_config() -> WebResult<ScheduleConfig> {
    ScheduleConfig::load(&snapshot_runtime_paths().settings_path)
        .map_err(|e| WebError::new("config_error", format!("failed to load settings: {e}")))
}

/// The week schedules, when they load.
fn load_weekly() -> Option<WeeklySchedule> {
    let paths = snapshot_runtime_paths();
    WeeklySchedule::load(&paths.odd_path, &paths.even_path).ok()
}

/// Everything the Reminders card shows: each kind's switch (on only when it
/// also has a time to ring at), its times, the quiet days, which tasks and
/// deadlines count, and each time block and schedule message with whether it
/// rings.
pub fn reminders_read(_payload: &Dict) -> WebResult {
    let config = load_config()?;
    describe(&config)
}

fn describe(config: &ScheduleConfig) -> WebResult {
    let mut on = Dict::new();
    let mut times = Dict::new();
    for kind in ReminderKind::ALL {
        let kind_times = times_of(config, kind);
        let rings = config.reminder_on(kind) && (kind.time_key().is_none() || has_times(&kind_times));
        on.insert(kind.key().into(), json!(rings));
        if kind.time_key().is_some() {
            times.insert(kind.key().into(), kind_times);
        }
    }
    let skip_days: Vec<&str> = WEEKDAYS.into_iter().filter(|day| config.skips_weekday(day)).collect();

    let muted_blocks = config.muted_blocks();
    let mut blocks: Vec<Value> = config
        .time_blocks
        .iter()
        .map(|(name, minutes)| json!({"name": name, "minutes": py::py_int(minutes), "on": config.block_rings(name)}))
        .collect();
    // A muted name that is no longer a time block can still be turned back on.
    blocks.extend(
        muted_blocks
            .iter()
            .filter(|name| !config.time_blocks.contains_key(*name))
            .map(|name| json!({"name": name, "minutes": null, "on": false})),
    );

    let mut points: Vec<Value> = config
        .time_points
        .iter()
        .map(|(name, message)| {
            let message = py::py_str(message);
            json!({"name": name, "message": message, "on": config.point_rings(name, &message)})
        })
        .collect();
    let messages = load_weekly().map(|weekly| schedule_messages(config, &weekly)).unwrap_or_default();
    for message in &messages {
        points.push(json!({"name": message, "message": message, "on": config.point_rings(message, message)}));
    }
    points.extend(
        config
            .muted_points()
            .iter()
            .filter(|name| !config.time_points.contains_key(*name) && !messages.contains(name))
            .map(|name| json!({"name": name, "message": name, "on": false})),
    );

    Ok(json!({
        "on": on,
        "times": times,
        "skipDays": skip_days,
        "urgentImportance": config.urgent_importance(),
        "deadlineDays": config.deadline_days(),
        "blocks": blocks,
        "points": points,
        "defaults": {"urgentImportance": DEFAULT_URGENT_IMPORTANCE, "deadlineDays": DEFAULT_DEADLINE_DAYS},
        "limits": {"maxDeadlineDays": MAX_DEADLINE_DAYS},
    }))
}

/// A list of `HH:MM` times from the browser, sorted, without repeats.
fn time_list(value: &Value, label: &str) -> WebResult<Value> {
    let Value::Array(items) = value else { return Err(invalid(format!("{label} must be a list of times."))) };
    let mut times = Vec::new();
    for item in items {
        let time = item.as_str().and_then(clock_time).ok_or_else(|| {
            invalid(format!("{label}: {} is not a time. Use HH:MM, such as 09:30.", py::py_str(item)))
        })?;
        times.push(time);
    }
    times.sort();
    times.dedup();
    Ok(json!(times))
}

fn one_time(value: &Value, label: &str) -> WebResult<String> {
    value
        .as_str()
        .and_then(clock_time)
        .ok_or_else(|| invalid(format!("{label}: {} is not a time. Use HH:MM, such as 21:30.", py::py_str(value))))
}

/// A kind's times from the browser, as its `[tasks]` value.
fn parse_times(kind: ReminderKind, value: &Value) -> WebResult<Value> {
    let label = kind.key();
    let field = |name: &str| value.as_object().and_then(|map| map.get(name)).cloned().unwrap_or(Value::Null);
    Ok(match kind {
        ReminderKind::UrgentTasks | ReminderKind::Deadlines => time_list(value, label)?,
        ReminderKind::DailySummary | ReminderKind::HabitPrompt => json!(one_time(value, label)?),
        ReminderKind::WeeklyReport => {
            let day = field("day").as_str().map(str::to_lowercase).filter(|day| WEEKDAYS.contains(&day.as_str()));
            let day = day.ok_or_else(|| invalid(format!("{label}: the day must be a weekday such as sunday.")))?;
            json!(format!("{day} {}", one_time(&field("time"), label)?))
        }
        ReminderKind::MonthlyReport => {
            let day = py::py_int(&field("day")).filter(|day| (1..=31).contains(day));
            let day = day.ok_or_else(|| invalid(format!("{label}: the day must be a day of the month, 1 to 31.")))?;
            json!(format!("{day} {}", one_time(&field("time"), label)?))
        }
        ReminderKind::BlockStart | ReminderKind::BlockEnd | ReminderKind::TimePoints => {
            return Err(invalid(format!("{label} rings at the times in your week schedule.")));
        }
    })
}

/// `current` with each `{name: on}` choice applied: names switched off are
/// added, names switched on taken out. `known` are the names that may be
/// switched off.
fn muted_list(current: Vec<String>, choices: &Value, known: &[String], label: &str) -> WebResult<Value> {
    let Value::Object(choices) = choices else { return Err(invalid(format!("{label} must map names to on or off."))) };
    let mut muted = current;
    for (name, on) in choices {
        let Value::Bool(on) = on else { return Err(invalid(format!("{label}: {name} must be on or off."))) };
        if *on {
            muted.retain(|item| item != name);
        } else if !muted.contains(name) {
            if !known.contains(name) {
                return Err(invalid(format!("{label}: there is no {name} to turn off.")));
            }
            muted.push(name.clone());
        }
    }
    Ok(json!(muted))
}

fn bounded(payload: &Dict, key: &str, low: i64, high: i64, what: &str) -> WebResult<Option<Value>> {
    let Some(value) = payload.get(key) else { return Ok(None) };
    let number = py::py_int(value).filter(|n| (low..=high).contains(n));
    number
        .map(|n| Some(json!(n)))
        .ok_or_else(|| invalid(format!("{what} must be a whole number from {low} to {high}.")))
}

/// Save what the Reminders card changed. Every field is optional:
///
/// - `on`: `{kind: bool}`; switching on a kind with no time gives it one;
/// - `times`: `{kind: times}`, a list of `HH:MM` for `urgent_tasks` and
///   `deadlines`, one `HH:MM` for `daily_summary` and `habit_prompt`, and
///   `{day, time}` for `weekly_report` (a weekday) and `monthly_report` (1-31);
/// - `skipDays`: the weekdays no reminders ring;
/// - `urgentImportance`: 1-10; `deadlineDays`: 0 to [`MAX_DEADLINE_DAYS`];
/// - `blocks` / `points`: `{name: bool}` for time blocks and schedule messages.
///
/// Returns the card's data as [`reminders_read`] does.
pub fn reminders_save(payload: &Dict) -> WebResult {
    let config = load_config()?;
    let mut edits: Vec<(&str, &str, Value)> = Vec::new();

    let given_times = payload.get("times").and_then(Value::as_object);
    if let Some(times) = payload.get("times") {
        let Value::Object(times) = times else { return Err(invalid("times must map reminders to their times.")) };
        for (key, value) in times {
            let kind = ReminderKind::from_key(key).ok_or_else(|| invalid(format!("unknown reminder: {key}.")))?;
            let parsed = parse_times(kind, value)?;
            edits.push(("tasks", time_key(&config, kind).unwrap_or_default(), parsed));
        }
    }
    if let Some(on) = payload.get("on") {
        let Value::Object(on) = on else { return Err(invalid("on must map reminders to on or off.")) };
        for (key, value) in on {
            let kind = ReminderKind::from_key(key).ok_or_else(|| invalid(format!("unknown reminder: {key}.")))?;
            let Value::Bool(value) = value else { return Err(invalid(format!("{key} must be on or off."))) };
            // A switch is only written when it changes, so a file without
            // [reminders] keeps having none until something is turned off.
            if config.reminder_on(kind) != *value || config.reminders.contains_key(kind.key()) {
                edits.push(("reminders", kind.key(), json!(value)));
            }
            let timed = kind.time_key().is_some() && !given_times.is_some_and(|times| times.contains_key(key));
            if *value && timed && !has_times(&times_of(&config, kind)) {
                edits.push(("tasks", time_key(&config, kind).unwrap_or_default(), default_time(kind)));
            }
        }
    }
    if let Some(days) = payload.get("skipDays") {
        let Value::Array(days) = days else { return Err(invalid("skipDays must be a list of weekdays.")) };
        let mut chosen: Vec<String> = Vec::new();
        for day in days {
            let name = day.as_str().map(str::to_lowercase).filter(|name| WEEKDAYS.contains(&name.as_str()));
            chosen.push(name.ok_or_else(|| invalid(format!("{} is not a weekday.", py::py_str(day))))?);
        }
        let ordered: Vec<&str> = WEEKDAYS.into_iter().filter(|day| chosen.iter().any(|c| c == day)).collect();
        edits.push(("settings", "skip_days", json!(ordered)));
    }
    if let Some(value) = bounded(payload, "urgentImportance", 1, 10, "The importance")? {
        edits.push(("reminders", "urgent_importance", value));
    }
    if let Some(value) = bounded(payload, "deadlineDays", 0, MAX_DEADLINE_DAYS, "Days before a deadline")? {
        edits.push(("reminders", "deadline_days", value));
    }
    if let Some(choices) = payload.get("blocks") {
        let known: Vec<String> = config.time_blocks.keys().cloned().collect();
        edits.push(("reminders", "muted_blocks", muted_list(config.muted_blocks(), choices, &known, "blocks")?));
    }
    if let Some(choices) = payload.get("points") {
        let mut known: Vec<String> = config.time_points.keys().cloned().collect();
        known.extend(config.time_points.values().map(py::py_str));
        if let Some(weekly) = load_weekly() {
            known.extend(schedule_messages(&config, &weekly));
        }
        edits.push(("reminders", "muted_points", muted_list(config.muted_points(), choices, &known, "points")?));
    }

    if !edits.is_empty() {
        save(&edits)?;
    }
    describe(&load_config()?)
}

/// Apply `edits` to the active settings.toml, line by line when it can and by
/// rewriting the file otherwise. The file is replaced in one step, so the
/// reminder service never reads half of it.
fn save(edits: &[(&str, &str, Value)]) -> WebResult<()> {
    let path = snapshot_runtime_paths().settings_path;
    let storage =
        |e: std::io::Error| WebError::new("storage_error", format!("failed to save settings: {}", os_error(&e, &path)));
    let content = std::fs::read_to_string(&path).map_err(storage)?;
    let edited = match edit_toml(&content, edits) {
        Some(edited) => edited,
        None => {
            let mut model = SettingsModel::new(&path)
                .map_err(|e| WebError::new("config_error", format!("failed to load settings: {e}")))?;
            for (section, key, value) in edits {
                model.set(section, key, value.clone());
            }
            crate::toml_writer::render_toml(&model.data).map_err(|e| WebError::new("storage_error", e))?
        }
    };
    let mut temporary = path.clone().into_os_string();
    temporary.push(".tmp");
    std::fs::write(&temporary, edited).map_err(storage)?;
    std::fs::rename(&temporary, &path).map_err(storage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    fn payload(value: Value) -> Dict {
        value.as_object().cloned().unwrap_or_default()
    }

    fn settings(env: &TestEnv) -> String {
        std::fs::read_to_string(env.root().join("user_config_0/settings.toml")).unwrap()
    }

    #[test]
    fn reads_every_reminder_with_its_times() {
        let env = TestEnv::with_fixture();
        env.write(
            "user_config_0/settings.toml",
            "[settings]\nskip_days = [\"sunday\", \"saturday\"]\n[time_blocks]\npomodoro = 25\n[time_points]\nlunch = \"Lunch 🍱\"\n[tasks]\ndaily_urgent = [\"14:01\", \"8:31\", \"oops\"]\ndaily_summary = \"23:35\"\nweekly_review = \"Sunday 20:00\"\nmonthly_review = \"40 20:00\"\n[paths]\n",
        );
        env.write(
            "user_config_0/odd_weeks.toml",
            "[monday]\n\"09:00\" = \"pomodoro\"\n\"12:00\" = \"lunch\"\n\"15:00\" = \"Drink water\"\n",
        );
        env.write("user_config_0/even_weeks.toml", "[common]\n\"15:00\" = \"Drink water\"\n");
        let data = reminders_read(&Dict::new()).unwrap();
        assert_eq!(
            data["on"],
            json!({
                "block_start": true, "block_end": true, "time_points": true, "urgent_tasks": true,
                // No ddl_urgent, habit_prompt or usable monthly_review: nothing to ring at.
                "deadlines": false, "daily_summary": true, "habit_prompt": false,
                "weekly_report": true, "monthly_report": false,
            })
        );
        assert_eq!(
            data["times"],
            json!({
                "urgent_tasks": ["08:31", "14:01"], "deadlines": [], "daily_summary": "23:35", "habit_prompt": null,
                "weekly_report": {"day": "sunday", "time": "20:00"}, "monthly_report": null,
            })
        );
        assert_eq!(data["skipDays"], json!(["saturday", "sunday"]));
        assert_eq!((data["urgentImportance"].clone(), data["deadlineDays"].clone()), (json!(8), json!(3)));
        assert_eq!(data["blocks"], json!([{"name": "pomodoro", "minutes": 25, "on": true}]));
        assert_eq!(
            data["points"],
            json!([
                {"name": "lunch", "message": "Lunch 🍱", "on": true},
                {"name": "Drink water", "message": "Drink water", "on": true},
            ])
        );
    }

    #[test]
    fn saving_switches_keeps_the_file_s_comments() {
        let env = TestEnv::with_fixture();
        let before = settings(&env);
        let data = reminders_save(&payload(json!({"on": {"block_end": false, "deadlines": false}}))).unwrap();
        assert_eq!((data["on"]["block_end"].clone(), data["on"]["deadlines"].clone()), (json!(false), json!(false)));
        let after = settings(&env);
        assert!(after.starts_with(before.trim_end()), "{after}");
        assert!(after.ends_with("\n[reminders]\nblock_end = false\ndeadlines = false\n"), "{after}");
        let config = ScheduleConfig::load_active().unwrap();
        assert!(!config.reminder_on(ReminderKind::BlockEnd) && config.reminder_on(ReminderKind::BlockStart));

        // On again: the switch is rewritten in place, and other lines stay. The
        // fixture has no deadline times, so the switch brings one.
        let data = reminders_save(&payload(json!({"on": {"deadlines": true}}))).unwrap();
        assert_eq!(data["times"]["deadlines"], json!(["09:00"]));
        let after = settings(&env);
        assert!(after.contains("\n[reminders]\nblock_end = false\ndeadlines = true\n"), "{after}");
        assert!(after.ends_with("\n[tasks]\nddl_urgent = [\"09:00\"]\n"), "{after}");
    }

    #[test]
    fn switching_on_a_reminder_without_a_time_gives_it_one() {
        let env = TestEnv::with_fixture();
        env.write("user_config_0/settings.toml", "[settings]\n[time_blocks]\n[time_points]\n[tasks]\n[paths]\n");
        let data =
            reminders_save(&payload(json!({"on": {"habit_prompt": true, "deadlines": true, "weekly_report": true}})))
                .unwrap();
        assert_eq!(data["on"]["habit_prompt"], json!(true));
        assert_eq!(data["times"]["habit_prompt"], json!("21:30"));
        assert_eq!(data["times"]["deadlines"], json!(["09:00"]));
        assert_eq!(data["times"]["weekly_report"], json!({"day": "sunday", "time": "20:00"}));
        // The switches were on already, so only the times were written.
        assert!(!settings(&env).contains("[reminders]"), "{}", settings(&env));

        // Times sent with the switch win over the default.
        let data = reminders_save(&payload(
            json!({"on": {"monthly_report": true}, "times": {"monthly_report": {"day": 15, "time": "8:05"}}}),
        ))
        .unwrap();
        assert_eq!(data["times"]["monthly_report"], json!({"day": 15, "time": "08:05"}));
        assert!(settings(&env).contains("monthly_review = \"15 08:05\""));
    }

    #[test]
    fn saves_times_quiet_days_and_what_counts() {
        let env = TestEnv::with_fixture();
        let data = reminders_save(&payload(json!({
            "times": {"urgent_tasks": ["21:31", "9:00", "09:00"], "daily_summary": "22:15", "weekly_report": {"day": "Friday", "time": "17:00"}},
            "skipDays": ["sunday", "saturday", "Saturday"],
            "urgentImportance": 6,
            "deadlineDays": 7,
        })))
        .unwrap();
        assert_eq!(data["times"]["urgent_tasks"], json!(["09:00", "21:31"]));
        assert_eq!(data["times"]["daily_summary"], json!("22:15"));
        assert_eq!(data["times"]["weekly_report"], json!({"day": "friday", "time": "17:00"}));
        assert_eq!(data["skipDays"], json!(["saturday", "sunday"]));
        assert_eq!((data["urgentImportance"].clone(), data["deadlineDays"].clone()), (json!(6), json!(7)));
        let text = settings(&env);
        assert!(text.contains("daily_urgent = [\"09:00\", \"21:31\"]\n"), "{text}");
        assert!(text.contains("skip_days = [\"saturday\", \"sunday\"]\n"), "{text}");
        assert!(text.contains("weekly_review = \"friday 17:00\"\n"), "{text}");
    }

    #[test]
    fn mutes_and_unmutes_blocks_and_messages() {
        let env = TestEnv::with_fixture();
        env.write("user_config_0/odd_weeks.toml", "[monday]\n\"15:00\" = \"Drink water\"\n");
        let config = ScheduleConfig::load_active().unwrap();
        let block = config.time_blocks.keys().next().cloned().unwrap();
        let point = config.time_points.keys().next().cloned().unwrap();
        let data = reminders_save(&payload(json!({
            "blocks": {block.clone(): false},
            "points": {point.clone(): false, "Drink water": false},
        })))
        .unwrap();
        let state = |list: &str, name: &str| {
            data[list]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["name"] == json!(name))
                .map(|item| item["on"].clone())
        };
        assert_eq!(state("blocks", &block), Some(json!(false)));
        assert_eq!(state("points", &point), Some(json!(false)));
        assert_eq!(state("points", "Drink water"), Some(json!(false)));
        let config = ScheduleConfig::load_active().unwrap();
        assert!(!config.block_rings(&block));
        assert!(!config.point_rings("Drink water", "Drink water"));

        let data = reminders_save(&payload(json!({"points": {"Drink water": true}}))).unwrap();
        assert_eq!(data["points"].as_array().unwrap().iter().filter(|item| item["on"] == json!(false)).count(), 1);
        assert!(settings(&env).contains(&format!("muted_points = [\"{point}\"]")));
    }

    #[test]
    fn rejects_what_the_service_could_not_use() {
        let _env = TestEnv::with_fixture();
        let message = |value: Value| reminders_save(&payload(value)).unwrap_err().message;
        assert_eq!(message(json!({"on": {"bedtime": true}})), "unknown reminder: bedtime.");
        assert_eq!(message(json!({"on": {"deadlines": "yes"}})), "deadlines must be on or off.");
        assert_eq!(
            message(json!({"times": {"urgent_tasks": ["25:00"]}})),
            "urgent_tasks: 25:00 is not a time. Use HH:MM, such as 09:30."
        );
        assert_eq!(
            message(json!({"times": {"block_start": "09:00"}})),
            "block_start rings at the times in your week schedule."
        );
        assert_eq!(
            message(json!({"times": {"monthly_report": {"day": 32, "time": "20:00"}}})),
            "monthly_report: the day must be a day of the month, 1 to 31."
        );
        assert_eq!(message(json!({"skipDays": ["someday"]})), "someday is not a weekday.");
        assert_eq!(message(json!({"urgentImportance": 11})), "The importance must be a whole number from 1 to 10.");
        assert_eq!(message(json!({"deadlineDays": -1})), "Days before a deadline must be a whole number from 0 to 60.");
        assert_eq!(message(json!({"blocks": {"nap": false}})), "blocks: there is no nap to turn off.");
    }

    #[test]
    fn a_file_the_line_edit_cannot_handle_is_rewritten() {
        let env = TestEnv::with_fixture();
        env.write(
            "user_config_0/settings.toml",
            "[settings]\n[time_blocks]\n[time_points]\n[tasks]\ndaily_urgent = [\n  \"08:31\",\n]\n[paths]\n",
        );
        let data = reminders_save(&payload(json!({"times": {"urgent_tasks": ["10:00"]}}))).unwrap();
        assert_eq!(data["times"]["urgent_tasks"], json!(["10:00"]));
    }
}
