//! Wall-clock access for the whole crate.
//!
//! Every "now" goes through this module so tests can freeze time with the
//! `RMD_NOW` environment variable (a local wall-clock time such as
//! `2026-11-09T10:30:00`). The Python implementation's golden recordings were
//! made with the same frozen instant, so outputs that embed timestamps match.

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};

/// Environment variable that freezes the clock (local naive datetime).
pub const NOW_ENV: &str = "RMD_NOW";

fn frozen() -> Option<NaiveDateTime> {
    let raw = std::env::var(NOW_ENV).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    crate::py::parse_naive_datetime(raw)
}

/// Current local time as a naive datetime (Python `datetime.now()`).
pub fn now() -> NaiveDateTime {
    frozen().unwrap_or_else(|| Local::now().naive_local())
}

/// Current local date (Python `date.today()`).
pub fn today() -> NaiveDate {
    now().date()
}

/// Current time with the local offset attached (Python `datetime.now().astimezone()`).
pub fn now_local() -> DateTime<Local> {
    match frozen() {
        Some(naive) => local_from_naive(naive),
        None => Local::now(),
    }
}

/// Current UTC time (Python `datetime.now(timezone.utc)`).
pub fn now_utc() -> DateTime<Utc> {
    now_local().with_timezone(&Utc)
}

/// Seconds since the Unix epoch (Python `time.time()`).
pub fn unix_time() -> f64 {
    let now = now_utc();
    now.timestamp() as f64 + f64::from(now.timestamp_subsec_micros()) / 1_000_000.0
}

/// Attach the local offset to a naive local datetime, picking the earlier
/// instant for ambiguous times and shifting forward across DST gaps.
pub fn local_from_naive(naive: NaiveDateTime) -> DateTime<Local> {
    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(earliest, _) => earliest,
        chrono::LocalResult::None => {
            let shifted = naive + chrono::Duration::hours(1);
            Local
                .from_local_datetime(&shifted)
                .earliest()
                .unwrap_or_else(|| Utc.from_utc_datetime(&naive).with_timezone(&Local))
        }
    }
}
