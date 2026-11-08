//! Weekly and daily insight statistics for the browser workspace.
//!
//! The weekly overview (Activity page) summarizes one Monday-Sunday week of
//! task outcomes and habit records, and compares it with the previous week and
//! a short baseline of earlier weeks. The daily overview (Overview page) does
//! the same for one day: what was finished and when, against yesterday and
//! the usual day.
//!
//! Numbers follow Python's `statistics.mean` (exact, and an `int` when the
//! mean of integers is whole) and `round(x, n)` (correctly rounded, ties to
//! even), so the JSON matches the original byte for byte.

use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike};
use indexmap::IndexMap;
use serde_json::{Value, json};

use crate::commands::history::{pair_task_activities, parse_log_timestamp};
use crate::data::{load_habit_records, load_habits, load_task_log};
use crate::py::{self, Dict};
use crate::web::services::{WebResult, default_task_type, invalid, iso_date, load_task_types, today};

pub const TREND_WEEKS: i64 = 8;
pub const BASELINE_WEEKS: usize = 4;
pub const HIGHLIGHT_COUNT: usize = 3;
/// The usual day is the average of this many days before the one shown.
pub const USUAL_DAYS: i64 = 28;

// =============================================================================
// Python numbers
// =============================================================================

/// A Python number: `statistics.mean` of integers stays an `int` when whole.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Num {
    Int(i64),
    Float(f64),
}

impl Num {
    fn as_f64(self) -> f64 {
        match self {
            Num::Int(i) => i as f64,
            Num::Float(f) => f,
        }
    }

    fn to_value(self) -> Value {
        match self {
            Num::Int(i) => json!(i),
            Num::Float(f) => json!(f),
        }
    }

    /// `round(x, ndigits)`: integers stay integers.
    fn round(self, ndigits: usize) -> Num {
        match self {
            Num::Int(i) => Num::Int(i),
            Num::Float(f) => Num::Float(round_float(f, ndigits)),
        }
    }
}

/// Python `round(x, ndigits)` for a float: the decimal string correctly
/// rounded (ties to even on the exact binary value), read back.
fn round_float(value: f64, ndigits: usize) -> f64 {
    if !value.is_finite() {
        return value;
    }
    format!("{value:.ndigits$}").parse().unwrap_or(value)
}

/// `(mantissa, exponent)` with `value == mantissa * 2**exponent`.
fn decompose(value: f64) -> (i64, i32) {
    let bits = value.to_bits();
    let sign = if bits >> 63 == 0 { 1 } else { -1 };
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let fraction = (bits & ((1 << 52) - 1)) as i64;
    if exponent == 0 { (sign * fraction, -1074) } else { (sign * (fraction | (1 << 52)), exponent - 1075) }
}

fn pow2(exponent: i32) -> f64 {
    if (-1022..=1023).contains(&exponent) {
        f64::from_bits(((exponent + 1023) as u64) << 52)
    } else {
        2f64.powi(exponent)
    }
}

/// The float nearest to `numerator / denominator * 2**exponent`.
fn ratio_to_f64(numerator: i128, denominator: u128, exponent: i32) -> f64 {
    if numerator == 0 {
        return 0.0;
    }
    let negative = numerator < 0;
    let magnitude = numerator.unsigned_abs();
    let shift = 124 - (128 - magnitude.leading_zeros()) as i32;
    let scaled = magnitude << shift;
    let (quotient, sticky) = (scaled / denominator, !scaled.is_multiple_of(denominator));
    let bits = 128 - quotient.leading_zeros() as i32;
    let drop = bits - 53;
    let mut mantissa = quotient >> drop;
    let rest = quotient & ((1u128 << drop) - 1);
    let half = 1u128 << (drop - 1);
    let mut scale = exponent - shift + drop;
    if rest > half || (rest == half && (sticky || mantissa & 1 == 1)) {
        mantissa += 1;
        if mantissa == 1 << 53 {
            mantissa >>= 1;
            scale += 1;
        }
    }
    let result = mantissa as f64 * pow2(scale);
    if negative { -result } else { result }
}

/// `statistics.mean`: exact, then rounded once (an `int` for whole means of integers).
fn mean(values: &[Num]) -> Num {
    let count = values.len() as i64;
    if values.iter().all(|v| matches!(v, Num::Int(_))) {
        let sum: i64 = values.iter().map(|v| v.as_f64() as i64).sum();
        if sum % count == 0 {
            return Num::Int(sum / count);
        }
        return Num::Float(sum as f64 / count as f64);
    }
    let parts: Vec<(i64, i32)> = values.iter().map(|v| decompose(v.as_f64())).collect();
    let low = parts.iter().filter(|(m, _)| *m != 0).map(|(_, e)| *e).min().unwrap_or(0);
    let high = parts.iter().filter(|(m, _)| *m != 0).map(|(_, e)| *e).max().unwrap_or(0);
    if high - low > 60 {
        // Too wide for exact integer arithmetic; never the case for these statistics.
        return Num::Float(values.iter().map(|v| v.as_f64()).sum::<f64>() / count as f64);
    }
    let sum: i128 = parts.iter().filter(|(m, _)| *m != 0).map(|(m, e)| i128::from(*m) << (e - low)).sum();
    Num::Float(ratio_to_f64(sum, count as u128, low))
}

/// `statistics.median` of floats.
fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    if n % 2 == 1 { sorted[n / 2] } else { (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0 }
}

/// `round(numerator / denominator, ndigits)` for integers.
fn rounded_ratio(numerator: i64, denominator: i64, ndigits: usize) -> f64 {
    round_float(numerator as f64 / denominator as f64, ndigits)
}

/// `max(0.0, (ended - started).total_seconds() / 3600)`.
fn lead_hours(started: NaiveDateTime, ended: NaiveDateTime) -> f64 {
    let micros = (ended - started).num_microseconds().unwrap_or(i64::MAX);
    let hours = micros as f64 / 1e6 / 3600.0;
    if hours < 0.0 { 0.0 } else { hours }
}

// =============================================================================
// History
// =============================================================================

fn week_start(day: NaiveDate) -> NaiveDate {
    day - Duration::days(i64::from(day.weekday().num_days_from_monday()))
}

fn week_days(start: NaiveDate) -> Vec<NaiveDate> {
    (0..7).map(|offset| start + Duration::days(offset)).collect()
}

/// A finished task with its priority as an integer and its type resolved.
struct Activity {
    description: String,
    priority: i64,
    started_at: NaiveDateTime,
    ended_at: NaiveDateTime,
    status: String,
    type_id: String,
}

/// `dict(Counter(...))`: counts in first-seen order.
fn count_types<'a>(types: impl Iterator<Item = &'a str>) -> IndexMap<String, i64> {
    let mut counts = IndexMap::new();
    for type_id in types {
        *counts.entry(type_id.to_string()).or_insert(0) += 1;
    }
    counts
}

fn counts_value(counts: &IndexMap<String, i64>) -> Value {
    Value::Object(counts.iter().map(|(key, count)| (key.clone(), json!(count))).collect())
}

struct WeekStats {
    week_start: NaiveDate,
    active: bool,
    completed: i64,
    cancelled: i64,
    dropped: i64,
    added: i64,
    habit_rate: Option<f64>,
    avg_priority: Option<Num>,
    median_lead_hours: Option<f64>,
    by_type: IndexMap<String, i64>,
}

impl WeekStats {
    /// Value of one of the numbers the baseline averages.
    fn number(&self, key: &str) -> Option<Num> {
        match key {
            "completed" => Some(Num::Int(self.completed)),
            "cancelled" => Some(Num::Int(self.cancelled)),
            "dropped" => Some(Num::Int(self.dropped)),
            "added" => Some(Num::Int(self.added)),
            "habitRate" => self.habit_rate.map(Num::Float),
            "avgPriority" => self.avg_priority,
            "medianLeadHours" => self.median_lead_hours.map(Num::Float),
            _ => None,
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "weekStart": iso_date(self.week_start),
            "active": self.active,
            "completed": self.completed,
            "cancelled": self.cancelled,
            "dropped": self.dropped,
            "added": self.added,
            "habitRate": self.habit_rate,
            "avgPriority": self.avg_priority.map(Num::to_value),
            "medianLeadHours": self.median_lead_hours,
            "byType": counts_value(&self.by_type),
        })
    }
}

const NUMERIC_KEYS: [&str; 7] =
    ["completed", "cancelled", "dropped", "added", "habitRate", "avgPriority", "medianLeadHours"];

/// Indexed task and habit history shared by every computed week.
struct History {
    today: NaiveDate,
    task_types: Dict,
    activities: Vec<Activity>,
    added_days: Vec<NaiveDate>,
    habits: IndexMap<String, String>,
    habit_days: HashMap<String, HashSet<String>>,
    first_day: Option<NaiveDate>,
}

impl History {
    fn new(today: NaiveDate) -> Self {
        let task_types = load_task_types();
        let default_type = default_task_type(&task_types);
        let log_entries = load_task_log();
        let activities: Vec<Activity> = pair_task_activities(&log_entries)
            .into_iter()
            .map(|a| Activity {
                priority: py::py_int(&a.priority).unwrap_or(0),
                type_id: if py::truthy(&a.task_type) { py::py_str(&a.task_type) } else { default_type.clone() },
                description: a.description,
                started_at: a.started_at,
                ended_at: a.ended_at,
                status: a.status,
            })
            .collect();
        let added_days: Vec<NaiveDate> = log_entries
            .iter()
            .filter_map(Value::as_object)
            .filter(|entry| entry.get("action").and_then(Value::as_str) == Some("added"))
            .filter_map(|entry| entry.get("timestamp").and_then(parse_log_timestamp))
            .map(|parsed| parsed.date())
            .collect();
        let habits = load_habits();
        let mut habit_days: HashMap<String, HashSet<String>> = HashMap::new();
        for record in load_habit_records() {
            let Some(record) = record.as_object() else { continue };
            let empty = Dict::new();
            let completed = match record.get("completed") {
                None => Some(&empty),
                Some(Value::Object(completed)) => Some(completed),
                Some(_) => None,
            };
            if let (Some(Value::String(day)), Some(completed)) = (record.get("date"), completed) {
                let done = completed.keys().filter(|id| habits.contains_key(*id)).cloned().collect();
                habit_days.insert(day.clone(), done);
            }
        }
        let mut known_days: Vec<NaiveDate> = activities.iter().map(|a| a.ended_at.date()).collect();
        known_days.extend(added_days.iter().copied());
        known_days.extend(habit_days.keys().filter_map(|day| py::parse_iso_date(day)));
        let first_day = known_days.into_iter().min();
        Self { today, task_types, activities, added_days, habits, habit_days, first_day }
    }

    fn in_range(&self, start: NaiveDate, end: NaiveDate) -> Vec<&Activity> {
        self.activities.iter().filter(|a| (start..=end).contains(&a.ended_at.date())).collect()
    }

    fn completed_in(&self, start: NaiveDate, end: NaiveDate) -> Vec<&Activity> {
        self.in_range(start, end).into_iter().filter(|a| a.status == "completed").collect()
    }

    fn habit_count(&self, day: NaiveDate) -> i64 {
        self.habit_days.get(&iso_date(day)).map_or(0, |done| done.len() as i64)
    }

    fn habit_done(&self, habit_id: &str, day: NaiveDate) -> bool {
        self.habit_days.get(&iso_date(day)).is_some_and(|done| done.contains(habit_id))
    }

    /// Whether a week overlaps the period the user has recorded anything.
    fn is_active(&self, start: NaiveDate) -> bool {
        self.first_day.is_some_and(|first| start + Duration::days(6) >= first && start <= self.today)
    }

    fn week_stats(&self, start: NaiveDate) -> WeekStats {
        let end = start + Duration::days(6);
        let activities = self.in_range(start, end);
        let completed: Vec<&&Activity> = activities.iter().filter(|a| a.status == "completed").collect();
        let elapsed: Vec<NaiveDate> = week_days(start).into_iter().filter(|day| *day <= self.today).collect();
        let slots = (self.habits.len() * elapsed.len()) as i64;
        let habit_done: i64 = elapsed.iter().map(|day| self.habit_count(*day)).sum();
        let leads: Vec<f64> = completed.iter().map(|a| lead_hours(a.started_at, a.ended_at)).collect();
        let priorities: Vec<Num> = completed.iter().map(|a| Num::Int(a.priority)).collect();
        WeekStats {
            week_start: start,
            active: self.is_active(start),
            completed: completed.len() as i64,
            cancelled: activities.iter().filter(|a| a.status == "cancelled").count() as i64,
            dropped: activities.iter().filter(|a| a.status == "dropped").count() as i64,
            added: self.added_days.iter().filter(|day| (start..=end).contains(*day)).count() as i64,
            habit_rate: (slots > 0).then(|| rounded_ratio(habit_done, slots, 4)),
            avg_priority: (!priorities.is_empty()).then(|| mean(&priorities).round(2)),
            median_lead_hours: (!leads.is_empty()).then(|| round_float(median(&leads), 2)),
            by_type: count_types(completed.iter().map(|a| a.type_id.as_str())),
        }
    }

    fn day_rows(&self, start: NaiveDate) -> Vec<Value> {
        week_days(start)
            .into_iter()
            .map(|day| {
                let completed = self.completed_in(day, day);
                json!({
                    "date": iso_date(day),
                    "completed": completed.len(),
                    "byType": counts_value(&count_types(completed.iter().map(|a| a.type_id.as_str()))),
                    "habitsDone": self.habit_count(day),
                    "habitsTotal": self.habits.len(),
                    "future": day > self.today,
                })
            })
            .collect()
    }

    fn habit_rows(&self, start: NaiveDate) -> Vec<Value> {
        let days = week_days(start);
        let previous_days: Vec<NaiveDate> =
            week_days(start - Duration::days(7)).into_iter().filter(|day| *day <= self.today).collect();
        let mut habits: Vec<(&String, &String)> = self.habits.iter().collect();
        habits.sort();
        habits
            .into_iter()
            .map(|(habit_id, description)| {
                let marks: Vec<Option<bool>> = days
                    .iter()
                    .map(|day| if *day > self.today { None } else { Some(self.habit_done(habit_id, *day)) })
                    .collect();
                let elapsed: Vec<bool> = marks.iter().flatten().copied().collect();
                let previous: Vec<bool> = previous_days.iter().map(|day| self.habit_done(habit_id, *day)).collect();
                let rate = |done: &[bool]| {
                    (!done.is_empty())
                        .then(|| rounded_ratio(done.iter().filter(|d| **d).count() as i64, done.len() as i64, 4))
                };
                json!({
                    "id": habit_id,
                    "description": description,
                    "days": marks,
                    "rate": rate(&elapsed),
                    "previousRate": rate(&previous),
                })
            })
            .collect()
    }

    /// Consecutive days with a completed task, ending at the week's last elapsed day.
    fn streak(&self, end: NaiveDate) -> i64 {
        let completed_days: HashSet<NaiveDate> =
            self.activities.iter().filter(|a| a.status == "completed").map(|a| a.ended_at.date()).collect();
        let mut anchor = end.min(self.today);
        if anchor == self.today && !completed_days.contains(&anchor) {
            anchor -= Duration::days(1);
        }
        let mut count = 0;
        while completed_days.contains(&anchor) {
            count += 1;
            anchor -= Duration::days(1);
        }
        count
    }
}

/// Average earlier active weeks; values are null before any history exists.
fn baseline(weeks: &[WeekStats]) -> Value {
    let active: Vec<&WeekStats> = weeks.iter().filter(|week| week.active).collect();
    let mut result = Dict::new();
    result.insert("weeks".into(), json!(active.len()));
    for key in NUMERIC_KEYS {
        let values: Vec<Num> = active.iter().filter_map(|week| week.number(key)).collect();
        let average = (!values.is_empty()).then(|| mean(&values).round(2).to_value());
        result.insert(key.into(), average.unwrap_or(Value::Null));
    }
    let type_ids: BTreeSet<&String> = active.iter().flat_map(|week| week.by_type.keys()).collect();
    let by_type: Dict = type_ids
        .into_iter()
        .map(|type_id| {
            let total: i64 = active.iter().map(|week| week.by_type.get(type_id).copied().unwrap_or(0)).sum();
            (type_id.clone(), json!(rounded_ratio(total, active.len() as i64, 2)))
        })
        .collect();
    result.insert("byType".into(), Value::Object(by_type));
    Value::Object(result)
}

fn target_date(payload: &Dict, today: NaiveDate) -> WebResult<NaiveDate> {
    match payload.get("date") {
        None | Some(Value::Null) => Ok(today),
        Some(Value::String(s)) if s.is_empty() => Ok(today),
        Some(raw) => py::parse_iso_date(&py::py_str(raw)).ok_or_else(|| invalid("date must be YYYY-MM-DD.")),
    }
}

/// The most important completions, latest first among equals.
fn highlight_rows(completed: &[&Activity]) -> Vec<Value> {
    let mut best: Vec<&&Activity> = completed.iter().collect();
    best.sort_by_key(|a| (Reverse(a.priority), Reverse(crate::clock::local_from_naive(a.ended_at).timestamp_micros())));
    best.into_iter()
        .take(HIGHLIGHT_COUNT)
        .map(|a| {
            json!({
                "description": a.description,
                "priority": a.priority,
                "type": a.type_id,
                "endedAt": py::isoformat_naive(&a.ended_at),
            })
        })
        .collect()
}

/// Statistics for the day `payload["date"]` (default today). `hours` holds
/// the day's completions per hour, by task type; `yesterday` and `usual` (the
/// average of the 28 days before, counting only days since the history
/// began) are null while there is no history to compare with.
pub fn daily_overview(payload: &Dict) -> WebResult {
    let today = today();
    let target = target_date(payload, today)?;
    let history = History::new(today);
    let activities = history.in_range(target, target);
    let completed: Vec<&Activity> = activities.iter().copied().filter(|a| a.status == "completed").collect();
    let mut hours: Vec<IndexMap<String, i64>> = vec![IndexMap::new(); 24];
    for activity in &completed {
        *hours[activity.ended_at.hour() as usize].entry(activity.type_id.clone()).or_insert(0) += 1;
    }
    let mut done_days: HashMap<NaiveDate, i64> = HashMap::new();
    for activity in history.activities.iter().filter(|a| a.status == "completed") {
        *done_days.entry(activity.ended_at.date()).or_insert(0) += 1;
    }
    let first = history.first_day;
    let counted: Vec<NaiveDate> = (1..=USUAL_DAYS)
        .map(|offset| target - Duration::days(offset))
        .filter(|day| first.is_some_and(|first| *day >= first))
        .collect();
    let yesterday = target - Duration::days(1);
    let leads: Vec<f64> = completed.iter().map(|a| lead_hours(a.started_at, a.ended_at)).collect();
    let usual: Vec<Num> = counted.iter().map(|day| Num::Int(done_days.get(day).copied().unwrap_or(0))).collect();
    Ok(json!({
        "date": iso_date(target),
        "today": iso_date(today),
        "isToday": target == today,
        "firstActivity": first.map(iso_date),
        "taskTypes": history.task_types,
        "completed": completed.len(),
        "cancelled": activities.iter().filter(|a| a.status == "cancelled").count(),
        "dropped": activities.iter().filter(|a| a.status == "dropped").count(),
        "added": history.added_days.iter().filter(|day| **day == target).count(),
        "byType": counts_value(&count_types(completed.iter().map(|a| a.type_id.as_str()))),
        "hours": hours.iter().map(counts_value).collect::<Vec<_>>(),
        "yesterday": first.filter(|first| yesterday >= *first).map(|_| done_days.get(&yesterday).copied().unwrap_or(0)),
        "usual": (!usual.is_empty()).then(|| mean(&usual).round(2).to_value()),
        "usualDays": counted.len(),
        "habitsDone": history.habit_count(target),
        "habitsTotal": history.habits.len(),
        "streak": history.streak(target),
        "medianLeadHours": (!leads.is_empty()).then(|| round_float(median(&leads), 2)),
        "highlights": highlight_rows(&completed),
    }))
}

/// Statistics for the week containing `payload["date"]` (default today).
pub fn weekly_overview(payload: &Dict) -> WebResult {
    let today = today();
    let target = target_date(payload, today)?;
    let history = History::new(today);
    let start = week_start(target);
    let end = start + Duration::days(6);
    let trend: Vec<WeekStats> =
        (0..TREND_WEEKS).rev().map(|offset| history.week_stats(start - Duration::weeks(offset))).collect();
    let current = &trend[trend.len() - 1];
    let previous = &trend[trend.len() - 2];
    let completed = history.completed_in(start, end);
    let baseline_weeks = &trend[trend.len() - 1 - BASELINE_WEEKS..trend.len() - 1];
    Ok(json!({
        "weekStart": iso_date(start),
        "weekEnd": iso_date(end),
        "today": iso_date(today),
        "isCurrentWeek": start == week_start(today),
        "firstActivity": history.first_day.map(iso_date),
        "taskTypes": history.task_types,
        "current": current.to_value(),
        "previous": previous.to_value(),
        "baseline": baseline(baseline_weeks),
        "trend": trend.iter().map(WeekStats::to_value).collect::<Vec<_>>(),
        "days": history.day_rows(start),
        "previousDays": history.day_rows(start - Duration::days(7)),
        "habits": history.habit_rows(start),
        "streak": history.streak(end),
        "highlights": highlight_rows(&completed),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    fn log(action: &str, when: &str, description: &str, priority: i64, task_type: Option<&str>) -> Value {
        let mut task = json!({"description": description, "priority": priority});
        if let Some(task_type) = task_type {
            task["type"] = json!(task_type);
        }
        json!({"action": action, "timestamp": when, "task": task})
    }

    fn empty() -> Dict {
        Dict::new()
    }

    fn with_date(date: &str) -> Dict {
        json!({"date": date}).as_object().cloned().unwrap()
    }

    /// Wednesday of the week starting Monday 2026-10-26.
    fn overview() -> TestEnv {
        let mut env = TestEnv::new();
        env.set_now("2026-10-28T12:00:00");
        env.write("user_config_0/settings.toml", "[task_types]\n1 = \"Work\"\n2 = \"Study\"\n");
        env.write("user_config_0/habits.toml", "[habits]\n1 = \"Exercise\"\n2 = \"Read\"\n");
        let records = json!([
            {"date": "2026-10-26", "completed": {"1": "Exercise", "2": "Read"}},
            {"date": "2026-10-27", "completed": {"1": "Exercise", "retired": "Old habit"}},
            {"date": "2026-10-20", "completed": {"2": "Read"}},
        ]);
        env.write("tasks/record.json", &records.to_string());
        let entries = json!([
            // Two weeks ago: one completion.
            log("added", "2026-10-12T08:00:00", "Old report", 4, Some("1")),
            log("deleted", "2026-10-13T08:00:00", "Old report", 4, Some("1")),
            // Last week: one completion and one drop.
            log("added", "2026-10-19T09:00:00", "Plan", 6, Some("2")),
            log("deleted", "2026-10-21T09:00:00", "Plan", 6, Some("2")),
            log("added", "2026-10-19T09:00:00", "Idea", 3, Some("1")),
            log("dropped", "2026-10-22T09:00:00", "Idea", 3, Some("1")),
            // This week: three completions (one legacy without a type) and a cancel.
            log("added", "2026-10-25T10:00:00", "Draft", 9, Some("1")),
            log("deleted", "2026-10-26T10:00:00", "Draft", 9, Some("1")),
            log("added", "2026-10-26T12:00:00", "Notes", 5, Some("2")),
            log("deleted", "2026-10-27T12:00:00", "Notes", 5, Some("2")),
            log("added", "2026-10-27T08:00:00", "Legacy", 7, None),
            log("deleted", "2026-10-27T10:00:00", "Legacy", 7, None),
            log("added", "2026-10-27T08:00:00", "Mistake", 2, Some("1")),
            log("cancelled", "2026-10-27T08:30:00", "Mistake", 2, Some("1")),
        ]);
        env.write("tasks/tasks.log", &entries.to_string());
        env
    }

    fn without_history() -> TestEnv {
        let mut env = TestEnv::new();
        env.set_now("2026-10-28T12:00:00");
        env.write("user_config_0/settings.toml", "[task_types]\n1 = \"Work\"\n");
        env
    }

    fn column(rows: &Value, key: &str) -> Vec<Value> {
        rows.as_array().unwrap().iter().map(|row| row[key].clone()).collect()
    }

    #[test]
    fn weekly_overview_summarizes_current_week() {
        let _env = overview();
        let data = weekly_overview(&empty()).unwrap();
        assert_eq!(
            (&data["weekStart"], &data["weekEnd"], &data["isCurrentWeek"]),
            (&json!("2026-10-26"), &json!("2026-11-01"), &json!(true))
        );
        let current = &data["current"];
        assert_eq!(
            (&current["completed"], &current["cancelled"], &current["dropped"]),
            (&json!(3), &json!(1), &json!(0))
        );
        assert_eq!(current["added"], json!(3), "Draft was added the previous Sunday");
        assert_eq!(current["byType"], json!({"1": 2, "2": 1}), "the untyped legacy task uses the default type");
        // statistics.mean of whole priorities that divide evenly stays an int, as in the JSON Python wrote.
        assert_eq!(current["avgPriority"].to_string(), "7");
        assert_eq!(current["medianLeadHours"], json!(24.0));
        // Monday to Wednesday elapsed: 3 completed habit slots out of 2 habits x 3 days.
        assert_eq!(current["habitRate"], json!(0.5));

        assert_eq!(column(&data["days"], "completed"), [1, 2, 0, 0, 0, 0, 0].map(|n| json!(n)));
        assert_eq!(column(&data["days"], "future"), [false, false, false, true, true, true, true].map(|b| json!(b)));
        assert_eq!(data["days"][1]["byType"], json!({"1": 1, "2": 1}));
        assert_eq!(column(&data["previousDays"], "completed"), [0, 0, 1, 0, 0, 0, 0].map(|n| json!(n)));

        let exercise = &data["habits"][0];
        assert_eq!(exercise["days"].as_array().unwrap()[..4], [json!(true), json!(true), json!(false), Value::Null]);
        assert_eq!(exercise["rate"], json!(0.6667));
        assert_eq!(exercise["previousRate"], json!(0.0));
        assert_eq!(data["habits"][1]["previousRate"], json!(0.1429));

        assert_eq!(column(&data["highlights"], "description"), [json!("Draft"), json!("Legacy"), json!("Notes")]);
        assert_eq!(data["streak"], json!(2), "today has no completion yet, so the streak ends yesterday");
    }

    #[test]
    fn weekly_overview_compares_with_history() {
        let _env = overview();
        let data = weekly_overview(&empty()).unwrap();
        let trend = data["trend"].as_array().unwrap();
        assert_eq!(trend.len() as i64, TREND_WEEKS);
        assert_eq!(column(&data["trend"], "completed")[5..], [json!(1), json!(1), json!(3)]);
        assert_eq!(data["previous"]["dropped"], json!(1));
        assert_eq!(data["firstActivity"], json!("2026-10-12"));
        // Only the two weeks since history began count toward the baseline.
        assert_eq!(data["baseline"]["weeks"], json!(2));
        assert_eq!(data["baseline"]["completed"].to_string(), "1");
        assert_eq!(data["baseline"]["byType"], json!({"1": 0.5, "2": 0.5}));
        assert_eq!(trend[0]["active"], json!(false));
    }

    #[test]
    fn weekly_overview_navigates_to_past_weeks() {
        let _env = overview();
        let data = weekly_overview(&with_date("2026-10-22")).unwrap();
        assert_eq!(data["weekStart"], json!("2026-10-19"));
        assert_eq!(data["isCurrentWeek"], json!(false));
        assert_eq!(data["current"]["completed"], json!(1));
        assert!(column(&data["days"], "future").iter().all(|future| *future == json!(false)));
        assert_eq!(data["streak"], json!(0));
    }

    #[test]
    fn overviews_reject_invalid_dates() {
        let _env = overview();
        for date in ["next week", "tomorrow", "07/11/2026"] {
            assert_eq!(weekly_overview(&with_date(date)).unwrap_err().code, "invalid_input");
            let error = daily_overview(&with_date(date)).unwrap_err();
            assert_eq!((error.code.as_str(), error.message.as_str()), ("invalid_input", "date must be YYYY-MM-DD."));
        }
    }

    #[test]
    fn weekly_overview_without_history() {
        let _env = without_history();
        let data = weekly_overview(&empty()).unwrap();
        assert_eq!(data["current"]["completed"], json!(0));
        assert_eq!(data["current"]["habitRate"], Value::Null);
        assert_eq!(data["baseline"]["weeks"], json!(0));
        assert_eq!(data["baseline"]["completed"], Value::Null);
        assert_eq!(data["firstActivity"], Value::Null);
    }

    #[test]
    fn daily_overview_summarizes_one_day() {
        let _env = overview();
        let data = daily_overview(&with_date("2026-10-27")).unwrap();
        assert_eq!(data["date"], json!("2026-10-27"));
        assert_eq!(data["isToday"], json!(false));
        let counts = ["completed", "cancelled", "dropped", "added"].map(|key| data[key].clone());
        assert_eq!(counts, [json!(2), json!(1), json!(0), json!(2)]);
        // The legacy completion without a type counts toward the default type.
        assert_eq!(data["byType"], json!({"1": 1, "2": 1}));
        assert_eq!(data["hours"][10], json!({"1": 1}));
        assert_eq!(data["hours"][12], json!({"2": 1}));
        let filled: usize = data["hours"].as_array().unwrap().iter().map(|h| h.as_object().unwrap().len()).sum();
        assert_eq!(filled, 2);
        // Yesterday had one completion; the usual day averages the 15 days since the history began.
        assert_eq!(data["yesterday"], json!(1));
        assert_eq!((&data["usual"], &data["usualDays"]), (&json!(0.2), &json!(15)));
        // Only configured habits count.
        assert_eq!((&data["habitsDone"], &data["habitsTotal"]), (&json!(1), &json!(2)));
        assert_eq!(data["streak"], json!(2));
        assert_eq!(data["medianLeadHours"], json!(13.0));
        assert_eq!(column(&data["highlights"], "description"), [json!("Legacy"), json!("Notes")]);
        assert_eq!(
            data["highlights"][0],
            json!({"description": "Legacy", "priority": 7, "type": "1", "endedAt": "2026-10-27T10:00:00"})
        );
    }

    #[test]
    fn daily_overview_defaults_to_today() {
        let _env = overview();
        let data = daily_overview(&empty()).unwrap();
        assert_eq!((&data["date"], &data["today"]), (&json!("2026-10-28"), &json!("2026-10-28")));
        assert_eq!(data["isToday"], json!(true));
        assert_eq!(data["completed"], json!(0));
        assert_eq!(data["hours"], json!(vec![json!({}); 24]));
        assert_eq!(data["yesterday"], json!(2));
        assert_eq!(data["medianLeadHours"], Value::Null);
        assert_eq!(data["highlights"], json!([]));
        // Nothing finished yet today, so the streak still counts up to yesterday.
        assert_eq!(data["streak"], json!(2));
    }

    #[test]
    fn daily_overview_without_history() {
        let _env = without_history();
        let data = daily_overview(&empty()).unwrap();
        assert_eq!(data["completed"], json!(0));
        assert_eq!(data["yesterday"], Value::Null);
        assert_eq!(data["usual"], Value::Null);
        assert_eq!(data["usualDays"], json!(0));
        assert_eq!(data["firstActivity"], Value::Null);
        assert_eq!(data["habitsTotal"], json!(0));
    }

    #[test]
    fn numbers_follow_python() {
        // round() is correctly rounded with ties to even; mean() is exact and keeps whole int means ints.
        assert_eq!(round_float(0.125, 2), 0.12);
        assert_eq!(round_float(0.375, 2), 0.38);
        assert_eq!(round_float(2.675, 2), 2.67);
        assert_eq!(round_float(2.0 / 3.0, 4), 0.6667);
        assert_eq!(mean(&[Num::Int(9), Num::Int(5), Num::Int(7)]), Num::Int(7));
        assert_eq!(mean(&[Num::Int(1), Num::Int(2)]), Num::Float(1.5));
        assert_eq!(mean(&[Num::Int(7), Num::Float(6.5)]), Num::Float(6.75));
        // The exact sum is 0.25 + 2e-18, so the mean rounds to 0.125 (Python: 0.12), not 0.13.
        assert_eq!(mean(&[Num::Float(0.1429), Num::Float(0.1071)]).round(2), Num::Float(0.12));
        assert_eq!(mean(&[Num::Float(0.0), Num::Float(0.5)]), Num::Float(0.25));
        assert_eq!(median(&[24.0, 2.0, 24.0]), 24.0);
        assert_eq!(median(&[2.0, 24.0]), 13.0);
    }
}
