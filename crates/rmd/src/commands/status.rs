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
