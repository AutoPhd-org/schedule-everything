//! Repeat rules for recurring deadlines (weekly, every two weeks, monthly).
//!
//! A recurring deadline keeps its next occurrence in `deadline` and its rule in
//! `repeat`. Monthly deadlines also keep `repeat_day`, the day of the month the
//! user picked, so a deadline on the 31st returns to the 31st after a shorter
//! month.

use chrono::{Datelike, Duration, NaiveDate};
use serde_json::Value;

use crate::py::{self, Dict};

pub const REPEAT_RULES: [&str; 3] = ["weekly", "biweekly", "monthly"];
const MAX_OCCURRENCES: usize = 5000;

/// Human label for a repeat rule.
pub fn repeat_label(rule: &str) -> Option<&'static str> {
    match rule {
        "weekly" => Some("Weekly"),
        "biweekly" => Some("Every 2 weeks"),
        "monthly" => Some("Monthly"),
        _ => None,
    }
}

/// The deadline's repeat rule, or `None` for one-off or invalid values.
pub fn repeat_rule(item: &Dict) -> Option<&'static str> {
    let raw = match item.get("repeat") {
        Some(v) if py::truthy(v) => py::py_str(v),
        _ => String::new(),
    };
    let rule = raw.trim().to_lowercase();
    REPEAT_RULES.iter().copied().find(|r| *r == rule)
}

/// Day of the month a monthly deadline returns to (defaults to its current day).
pub fn anchor_day(item: &Dict, current: NaiveDate) -> u32 {
    let day = match item.get("repeat_day") {
        Some(v) if py::truthy(v) => py::py_int(v).unwrap_or(i64::from(current.day())),
        _ => i64::from(current.day()),
    };
    day.clamp(1, 31) as u32
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(next_year, next_month, 1).and_then(|d| d.pred_opt()).map(|d| d.day()).unwrap_or(28)
}

fn add_months(value: NaiveDate, months: i32, day: u32) -> NaiveDate {
    let month_index = value.month0() as i32 + months;
    let year = value.year() + month_index.div_euclid(12);
    let month = month_index.rem_euclid(12) as u32 + 1;
    let day = day.min(days_in_month(year, month));
    NaiveDate::from_ymd_opt(year, month, day).expect("clamped day is valid")
}

/// The occurrence after `current` for a repeat rule.
pub fn next_occurrence(current: NaiveDate, rule: &str, day: Option<u32>) -> NaiveDate {
    match rule {
        "weekly" => current + Duration::days(7),
        "biweekly" => current + Duration::days(14),
        "monthly" => add_months(current, 1, day.filter(|d| *d > 0).unwrap_or(current.day())),
        other => panic!("unknown repeat rule: {other}"),
    }
}

/// Occurrences from `start` (inclusive) through `until` (inclusive).
pub fn occurrences(start: NaiveDate, rule: &str, until: NaiveDate, day: Option<u32>) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let mut current = start;
    for _ in 0..MAX_OCCURRENCES {
        if current > until {
            break;
        }
        out.push(current);
        current = next_occurrence(current, rule, day);
    }
    out
}

/// Advance `current` by whole repeats until it is on or after `earliest`.
pub fn roll_forward(current: NaiveDate, rule: &str, earliest: NaiveDate, day: Option<u32>) -> NaiveDate {
    let mut current = current;
    for _ in 0..MAX_OCCURRENCES {
        if current >= earliest {
            return current;
        }
        current = next_occurrence(current, rule, day);
    }
    current
}

/// Whether a JSON value names a valid repeat rule.
pub fn is_repeat_rule(value: &Value) -> bool {
    value.as_str().is_some_and(|s| REPEAT_RULES.contains(&s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn monthly_returns_to_anchor_day() {
        let feb = next_occurrence(d(2025, 1, 31), "monthly", Some(31));
        assert_eq!(feb, d(2025, 2, 28));
        assert_eq!(next_occurrence(feb, "monthly", Some(31)), d(2025, 3, 31));
        assert_eq!(next_occurrence(d(2025, 12, 15), "monthly", None), d(2026, 1, 15));
    }

    #[test]
    fn rules_and_anchor() {
        let item = json!({"repeat": " Weekly ", "repeat_day": "40"});
        let item = item.as_object().unwrap();
        assert_eq!(repeat_rule(item), Some("weekly"));
        assert_eq!(anchor_day(item, d(2025, 1, 2)), 31);
        assert_eq!(roll_forward(d(2025, 1, 1), "biweekly", d(2025, 1, 20), None), d(2025, 1, 29));
        assert_eq!(occurrences(d(2025, 1, 1), "weekly", d(2025, 1, 15), None).len(), 3);
    }
}
