//! Week parity, `HH:MM` helpers, and the repeating alarm.

use chrono::{Datelike, Duration, NaiveDate, NaiveTime};

use crate::py;

/// `"odd"` or `"even"` for the ISO week containing `date`.
pub fn week_parity_of(date: NaiveDate) -> &'static str {
    if date.iso_week().week() % 2 == 1 { "odd" } else { "even" }
}

/// `"odd"` or `"even"` for the current ISO week.
pub fn get_week_parity() -> String {
    week_parity_of(crate::clock::today()).to_string()
}

/// Parse `"HH:MM"` (lenient on zero padding, like `strptime`).
pub fn parse_time(text: &str) -> Option<NaiveTime> {
    py::parse_hhmm(text)
}

/// Format a time as `"HH:MM"`.
pub fn time_to_str(time: NaiveTime) -> String {
    py::hhmm(time)
}

/// Add minutes to an `"HH:MM"` string, wrapping around midnight.
pub fn add_minutes_to_time(text: &str, minutes: i64) -> Option<String> {
    let time = parse_time(text)?;
    let (shifted, _) = time.overflowing_add_signed(Duration::minutes(minutes));
    Some(time_to_str(shifted))
}

/// Repeat sound + dialog until the user stops it or the duration runs out.
pub fn alarm(_title: &str, message: &str, sound_file: &str, alarm_interval: f64, max_alarm_duration: f64) {
    let start = std::time::Instant::now();
    loop {
        crate::platform::play_sound(sound_file);
        let button = crate::platform::show_dialog(message);
        if button.contains(crate::platform::stop_alarm_label().as_str()) || button.contains("停止闹铃") {
            break;
        }
        if start.elapsed().as_secs_f64() > max_alarm_duration {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs_f64(alarm_interval.max(0.0)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_wrap_midnight() {
        assert_eq!(add_minutes_to_time("23:45", 30).as_deref(), Some("00:15"));
        assert_eq!(add_minutes_to_time("09:30", 25).as_deref(), Some("09:55"));
        assert_eq!(add_minutes_to_time("00:10", -20).as_deref(), Some("23:50"));
    }

    #[test]
    fn parity_uses_iso_weeks() {
        assert_eq!(week_parity_of(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()), "odd");
        assert_eq!(week_parity_of(NaiveDate::from_ymd_opt(2026, 1, 5).unwrap()), "even");
    }
}
