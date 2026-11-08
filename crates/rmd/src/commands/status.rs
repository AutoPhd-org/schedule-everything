//! Current status (`rmd status`) and schedule visualization (`rmd view`).

use chrono::{Duration, NaiveDateTime};
use serde_json::Value;

use crate::config::{ScheduleConfig, TomlLoadError, WeeklySchedule};
use crate::i18n::tf;
use crate::py::{self, Dict};
use crate::synced_schedule::{apply_synced_schedule, format_event_label};
use crate::time_utils::{get_week_parity, parse_time};

/// Today's schedule with metadata.
#[derive(Clone, Debug)]
pub struct TodaySchedule {
    /// Merged schedule (empty on a skip day).
    pub schedule: Dict,
    /// `"odd"` or `"even"`.
    pub parity: String,
    pub is_skipped: bool,
    pub config: ScheduleConfig,
}

/// Today's schedule (with the synced plan applied when `apply_sync`).
pub fn get_today_schedule_for_status(apply_sync: bool) -> Result<TodaySchedule, TomlLoadError> {
    let paths = crate::paths::runtime();
    let config = ScheduleConfig::load(&paths.settings_path)?;
    let weekly = WeeklySchedule::load(&paths.odd_path, &paths.even_path)?;
    let is_skipped = config.should_skip_today();
    let parity = get_week_parity();
    let mut schedule = if is_skipped { Dict::new() } else { weekly.get_today_schedule(&config) };
    if apply_sync && !schedule.is_empty() {
        let today = crate::clock::today();
        schedule = apply_synced_schedule(&schedule, Some(today), Some(&parity), Some(py::weekday_name(today)), None);
    }
    Ok(TodaySchedule { schedule, parity, is_skipped, config })
}

fn event_duration_minutes(event: &Value, config: Option<&ScheduleConfig>) -> i64 {
    let Some(config) = config else {
        return 1;
    };
    let block = match event {
        Value::String(s) => Some(s.as_str()),
        Value::Object(map) if map.contains_key("block") => map.get("block").and_then(Value::as_str),
        _ => None,
    };
    block.and_then(|b| config.time_blocks.get(b)).and_then(py::py_int).unwrap_or(1)
}

/// Current event, next event, and time until the next event.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CurrentAndNext {
    pub current: Option<String>,
    pub next: Option<String>,
    pub time_to_next: Option<String>,
}

/// Current and next events at `now` (blocks last their configured duration,
/// other events one minute).
pub fn current_and_next_at(schedule: &Dict, config: Option<&ScheduleConfig>, now: NaiveDateTime) -> CurrentAndNext {
    let mut out = CurrentAndNext::default();
    if schedule.is_empty() {
        return out;
    }
    let today = now.date();
    let mut times: Vec<(&String, NaiveDateTime)> =
        schedule.keys().filter_map(|time| parse_time(time).map(|t| (time, today.and_time(t)))).collect();
    times.sort_by_key(|(_, dt)| *dt);
    for (time, start) in times {
        let event = &schedule[time.as_str()];
        let name = format_event_label(event);
        let end = start + Duration::minutes(event_duration_minutes(event, config));
        if start <= now && now < end {
            out.current = Some(tf("{event} at {time}", &[("event", &name), ("time", time)]));
        }
        if start > now && out.next.is_none() {
            out.next = Some(tf("{event} at {time}", &[("event", &name), ("time", time)]));
            let seconds = (start - now).num_microseconds().map(|us| us as f64 / 1e6).unwrap_or(0.0);
            let total_minutes = (seconds / 60.0).floor() as i64;
            let (hours, minutes) = (total_minutes / 60, total_minutes % 60);
            out.time_to_next = Some(if hours > 0 {
                tf("{hours}h {minutes}m", &[("hours", &hours), ("minutes", &minutes)])
            } else {
                tf("{minutes}m", &[("minutes", &minutes)])
            });
        }
    }
    out
}

/// Current and next events right now.
pub fn get_current_and_next_events(schedule: &Dict, config: Option<&ScheduleConfig>) -> CurrentAndNext {
    current_and_next_at(schedule, config, crate::clock::now())
}

// --- CLI commands (`rmd status [-v]`, `rmd view`) are added below. ---

use crate::i18n::t;
use crate::synced_schedule::get_event_block_name;
use crate::ui::{Align, BoxStyle, Column, Console, Justify, Panel, Table, Text};

const P_MODE_ERROR: &str = "❌ Currently in p mode. Switch back to j mode to execute this command.";

/// `rmd status [-v]`: week parity, the current and next event, and with
/// `-v` today's full schedule.
pub fn status_command(verbose: bool) -> i32 {
    status_command_with(&mut Console::stdout(), verbose)
}

/// [`status_command`] writing to a given console.
pub fn status_command_with(console: &mut Console, verbose: bool) -> i32 {
    if crate::data::load_mode() == "p" {
        console.println(&t(P_MODE_ERROR));
        return 1;
    }
    let today = match get_today_schedule_for_status(true) {
        Ok(today) => today,
        Err(e) => {
            console.print(&tf("[bold red]❌ Error checking status:[/bold red] {e}", &[("e", &e)]));
            return 1;
        }
    };
    let odd = today.parity == "odd";
    let parity_text = tf("📅 {parity} Week", &[("parity", &if odd { t("Odd") } else { t("Even") })]);
    let parity_style = if odd { "bold magenta" } else { "bold cyan" };
    console.render(&Align::center(console.markup(&format!("[{parity_style}]{parity_text}[/{parity_style}]"))));

    if today.is_skipped {
        let message = console.markup(&t("⏭️  Today is a skipped day - enjoy your time off!"));
        console.render(&Panel::new(Align::center(message)).style("yellow").box_style(BoxStyle::Rounded));
        return 0;
    }

    let events = get_current_and_next_events(&today.schedule, Some(&today.config));
    let mut lines = Vec::new();
    match &events.current {
        Some(current) => lines.push(tf("[bold green]🔔 NOW:[/bold green]  {event}", &[("event", current)])),
        None => lines.push(t("[bold yellow]🟡 IDLE[/bold yellow]")),
    }
    lines.push(String::new());
    match &events.next {
        Some(next) => {
            let time_str = events
                .time_to_next
                .as_ref()
                .filter(|s| !s.is_empty())
                .map(|time_until| tf(" (in {time_until})", &[("time_until", time_until)]))
                .unwrap_or_default();
            lines.push(tf(
                "[bold blue]⏰ NEXT:[/bold blue] {event}{time_str}",
                &[("event", next), ("time_str", &time_str)],
            ));
        }
        None => lines.push(t("[dim]📭 No upcoming events[/dim]")),
    }
    let panel = Panel::new(console.markup(&lines.join("\n")))
        .title(&format!("[bold]{}[/bold]", t("Status")))
        .expand(false)
        .border_style(if events.current.is_some() { "green" } else { "dim" })
        .box_style(BoxStyle::Rounded)
        .padding(1, 2);
    console.render(&panel);

    if verbose && !today.schedule.is_empty() {
        console.newline();
        console.render(&schedule_table(&today.schedule));
        console.print_justified(
            &format!("[dim italic]{}[/dim italic]", tf("Total events: {count}", &[("count", &today.schedule.len())])),
            Justify::Right,
        );
    }
    0
}

/// Today's events by time of day, styled by activity.
fn schedule_table(schedule: &Dict) -> Table {
    let mut periods: [Vec<(String, String, String)>; 3] = Default::default();
    let mut times: Vec<&String> = schedule.keys().collect();
    times.sort();
    for time in times {
        let event = &schedule[time.as_str()];
        let name = format_event_label(event);
        let block = get_event_block_name(event).filter(|b| !b.is_empty()).unwrap_or_else(|| name.clone());
        let hour = time.split(':').next().and_then(py::parse_int_str);
        let period = match hour {
            Some(h) if (5..12).contains(&h) => 0,
            Some(h) if (12..18).contains(&h) => 1,
            _ => 2,
        };
        periods[period].push((time.clone(), name, block));
    }

    let mut table = Table::new().box_style(BoxStyle::SimpleHead).header_style("bold").expand(true);
    table.add_column(Column::new(&t("Time")).justify(Justify::Right).style("cyan").width(8).no_wrap());
    table.add_column(Column::new(&t("Activity")));
    let sections = [(t("Morning"), "🌅", "yellow"), (t("Afternoon"), "☀️ ", "orange1"), (t("Evening"), "🌆", "purple")];
    for ((title, icon, color), events) in sections.iter().zip(&periods) {
        if events.is_empty() {
            continue;
        }
        table.add_section();
        table.add_row([format!("[{color}]{icon}[/{color}]"), format!("[bold {color}]{title}[/bold {color}]")]);
        for (time, name, block) in events {
            let block_lower = block.to_lowercase();
            let (style, icon) = if block_lower.contains("break")
                || name.to_lowercase().contains("break")
                || block_lower.contains("napping")
            {
                ("italic dim", "☕")
            } else if block_lower.contains("pomodoro") {
                ("bold red", "🍅")
            } else if block_lower.contains("potato") {
                ("bold yellow", "🥔")
            } else if block_lower.contains("go_to_bed") {
                ("bold blue", "🌙")
            } else if block_lower.contains("summary_time") {
                ("bold magenta", "📝")
            } else {
                ("bold", "•")
            };
            table.add_row([Text::from(time.as_str()), Text::from(format!("{icon}  [{style}]{name}[/{style}]"))]);
        }
    }
    table
}

/// `rmd view`: render the odd/even schedules to a PDF and open it.
pub fn view_command() -> i32 {
    view_command_with(&mut Console::stdout())
}

/// [`view_command`] writing to a given console.
pub fn view_command_with(console: &mut Console) -> i32 {
    if crate::data::load_mode() == "p" {
        console.println(&t(P_MODE_ERROR));
        return 1;
    }
    console.println(&t("📊 Generating schedule visualizations..."));
    let generated = (|| -> anyhow::Result<std::path::PathBuf> {
        let config = ScheduleConfig::load_active()?;
        let weekly = WeeklySchedule::load_active()?;
        crate::visualizer::visualize(&config, &weekly.odd_data, &weekly.even_data, None)
    })();
    let path = match generated {
        Ok(path) => path,
        Err(e) => {
            console.println(&tf("❌ Error generating visualizations: {e}", &[("e", &e)]));
            return 1;
        }
    };
    console.println(&format!("\n{}", t("📁 Visualization file generated:")));
    console.println(&format!("\n{}", t("🖼️  Opening visualization...")));
    if !crate::platform::open_file(&path) {
        console
            .println(&tf("⚠️  Could not open file automatically. File saved to: {path}", &[("path", &path.display())]));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;
    use chrono::NaiveDate;
    use serde_json::json;

    const SETTINGS: &str = "[settings]\nlanguage = \"en\"\nskip_days = [\"sunday\"]\n\n[time_blocks]\npomodoro = 25\npotato = 50\nlong_break = 40\n\n[time_points]\nlunch = \"Lunch\"\n";
    const WEEK: &str = "[monday]\n\"08:30\" = \"potato\"\n\"10:00\" = { block = \"pomodoro\", title = \"Focus Task\" }\n\"12:00\" = \"lunch\"\n\"13:30\" = \"long_break\"\n\n[common]\n\"21:00\" = \"summary_time\"\n\"22:30\" = \"go_to_bed\"\n";

    fn env_at(now: &str) -> TestEnv {
        let mut env = TestEnv::new();
        env.set_now(now);
        env.write("user_config_0/settings.toml", SETTINGS);
        env.write("user_config_0/odd_weeks.toml", WEEK);
        env.write("user_config_0/even_weeks.toml", WEEK);
        env
    }

    fn at(time: &str) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2024, 1, 1).unwrap().and_time(crate::time_utils::parse_time(time).unwrap())
    }

    fn schedule(value: Value) -> Dict {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn current_and_next_events() {
        let plain = schedule(json!({"08:00": "morning_task", "10:00": "focus_session", "14:00": "afternoon_meeting"}));
        let result = current_and_next_at(&plain, None, at("09:30"));
        assert_eq!(result.current, None);
        assert_eq!(result.next.as_deref(), Some("focus_session at 10:00"));
        assert_eq!(result.time_to_next.as_deref(), Some("30m"));

        let config = ScheduleConfig::from_dict(&schedule(
            json!({"time_blocks": {"block_a": 60, "block_b": 30, "pomodoro": 25}}),
        ));
        let blocks = schedule(json!({"08:00": "block_a", "10:00": "block_b"}));
        let result = current_and_next_at(&blocks, Some(&config), at("09:30"));
        assert_eq!((result.current, result.next.as_deref()), (None, Some("block_b at 10:00")));
        let result = current_and_next_at(&blocks, Some(&config), at("08:59"));
        assert_eq!(result.current.as_deref(), Some("block_a at 08:00"));
        assert_eq!(result.time_to_next.as_deref(), Some("1h 1m"));

        let titled = schedule(json!({"10:00": {"block": "pomodoro", "title": "Finish proposal draft"}}));
        let result = current_and_next_at(&titled, Some(&config), at("09:30"));
        assert_eq!(result.next.as_deref(), Some("pomodoro: Finish proposal draft at 10:00"));
        assert_eq!(current_and_next_at(&Dict::new(), None, at("09:30")), CurrentAndNext::default());
    }

    #[test]
    fn today_schedule_respects_skip_days_and_overlay() {
        let mut env = env_at("2026-11-09T10:30:00");
        env.set_var("REMINDER_SYNCED_SCHEDULE_PATH", &env.root().join("synced.toml").display().to_string());
        env.write(
            "synced.toml",
            "date = \"2026-11-09\"\nparity = \"even\"\nweekday = \"monday\"\n\n[schedule]\n\"08:30\" = { block = \"potato\", title = \"Write release notes\" }\n",
        );
        let today = get_today_schedule_for_status(true).unwrap();
        assert_eq!(today.parity, "even");
        assert!(!today.is_skipped);
        assert_eq!(today.schedule["08:30"], json!({"block": "potato", "title": "Write release notes"}));
        let base = get_today_schedule_for_status(false).unwrap();
        assert_eq!(base.schedule["08:30"], json!("potato"));

        env.set_now("2026-11-15T09:00:00");
        let sunday = get_today_schedule_for_status(true).unwrap();
        assert!(sunday.is_skipped);
        assert!(sunday.schedule.is_empty());
    }

    #[test]
    fn status_shows_parity_and_events() {
        let _env = env_at("2026-11-09T10:10:00");
        let mut console = Console::capture(80);
        assert_eq!(status_command_with(&mut console, false), 0);
        let expected = [
            "                                  📅 Even Week                                  ",
            "╭───────────────── Status ─────────────────╮",
            "│                                          │",
            "│  🔔 NOW:  pomodoro: Focus Task at 10:00  │",
            "│                                          │",
            "│  ⏰ NEXT: lunch at 12:00 (in 1h 50m)     │",
            "│                                          │",
            "╰──────────────────────────────────────────╯",
            "",
        ];
        assert_eq!(console.output(), expected.join("\n"));
    }

    #[test]
    fn status_verbose_lists_the_day() {
        let _env = env_at("2026-11-09T23:00:00");
        let mut console = Console::capture(60);
        assert_eq!(status_command_with(&mut console, true), 0);
        let output = console.output();
        assert!(output.contains("🟡 IDLE"));
        assert!(output.contains("📭 No upcoming events"));
        let table: Vec<&str> = output.lines().skip_while(|l| !l.contains("Time")).collect();
        let expected = [
            "            Time   Activity                                 ",
            " ────────────────────────────────────────────────────────── ",
            "              🌅   Morning                                  ",
            "           08:30   🥔  potato                               ",
            "           10:00   🍅  pomodoro: Focus Task                 ",
            "                                                            ",
            "               ☀\u{fe0f}   Afternoon                                ",
            "           12:00   •  lunch                                 ",
            "           13:30   ☕  long_break                           ",
            "                                                            ",
            "              🌆   Evening                                  ",
            "           21:00   📝  summary_time                         ",
            "           22:30   🌙  go_to_bed                            ",
            "                                                            ",
            "                                             Total events: 6",
        ];
        assert_eq!(table, expected);
    }

    #[test]
    fn status_on_skip_day_and_in_p_mode() {
        let env = env_at("2026-11-15T09:00:00");
        let mut console = Console::capture(100);
        assert_eq!(status_command_with(&mut console, false), 0);
        let expected = [
            "                                            📅 Even Week                                            ",
            "╭──────────────────────────────────────────────────────────────────────────────────────────────────╮",
            "│                         ⏭️  Today is a skipped day - enjoy your time off!                         │",
            "╰──────────────────────────────────────────────────────────────────────────────────────────────────╯",
            "",
        ];
        assert_eq!(console.output(), expected.join("\n"));

        env.write("tasks/mode.txt", "p");
        let mut console = Console::capture(40);
        assert_eq!(status_command_with(&mut console, false), 1);
        assert_eq!(console.output(), "❌ Currently in p mode. Switch back to j mode to execute this command.\n");
        let mut console = Console::capture(40);
        assert_eq!(view_command_with(&mut console), 1);
        assert_eq!(console.output(), "❌ Currently in p mode. Switch back to j mode to execute this command.\n");
    }

    #[test]
    fn status_reports_config_errors() {
        let env = env_at("2026-11-09T10:10:00");
        std::fs::remove_file(env.root().join("user_config_0/odd_weeks.toml")).unwrap();
        let mut console = Console::capture(200);
        assert_eq!(status_command_with(&mut console, false), 1);
        assert!(console.output().starts_with("❌ Error checking status: [Errno 2] No such file or directory: '"));
    }
}
