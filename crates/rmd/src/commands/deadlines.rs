//! Deadline management (`rmd ddl ...`).
//!
//! Deadlines live in `ddl.json` as `{"event", "deadline", "added", ...}`. A
//! recurring deadline also has `repeat` (weekly, biweekly, or monthly); once it
//! passes it rolls forward to its next occurrence instead of being removed.

use chrono::{Duration, NaiveDate};
use serde_json::Value;

use crate::py;
use crate::recurrence::{anchor_day, repeat_rule, roll_forward};

/// One-off deadlines more than this many days overdue are removed.
pub const AUTO_REMOVE_OVERDUE_DAYS: i64 = -2;

/// Result of [`refresh_deadlines`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RefreshedDeadlines {
    pub kept: Vec<Value>,
    pub removed: Vec<Value>,
    /// Whether `kept` differs from the input and should be saved.
    pub changed: bool,
}

/// Remove long-overdue one-off deadlines and roll recurring ones forward.
pub fn refresh_deadlines(deadlines: &[Value], today: Option<NaiveDate>) -> RefreshedDeadlines {
    let current = today.unwrap_or_else(crate::clock::today);
    let earliest_kept = current + Duration::days(AUTO_REMOVE_OVERDUE_DAYS + 1);
    let mut out = RefreshedDeadlines::default();
    let mut rolled = false;
    for deadline in deadlines {
        let parsed = deadline.get("deadline").and_then(Value::as_str).and_then(|s| py::strptime_date(s, "%Y-%m-%d"));
        let Some(deadline_date) = parsed else {
            out.kept.push(deadline.clone());
            continue;
        };
        if deadline_date >= earliest_kept {
            out.kept.push(deadline.clone());
            continue;
        }
        let empty = py::Dict::new();
        let item = deadline.as_object().unwrap_or(&empty);
        let Some(rule) = repeat_rule(item) else {
            out.removed.push(deadline.clone());
            continue;
        };
        let next = roll_forward(deadline_date, rule, earliest_kept, Some(anchor_day(item, deadline_date)));
        let mut updated = item.clone();
        updated.insert("deadline".into(), Value::String(next.format("%Y-%m-%d").to_string()));
        out.kept.push(Value::Object(updated));
        rolled = true;
    }
    out.changed = rolled || !out.removed.is_empty();
    out
}

/// Split deadlines into retained and auto-removed entries (recurring ones rolled).
pub fn prune_expired_deadlines(deadlines: &[Value], today: Option<NaiveDate>) -> (Vec<Value>, Vec<Value>) {
    let refreshed = refresh_deadlines(deadlines, today);
    (refreshed.kept, refreshed.removed)
}

// --- CLI commands (`rmd ddl`, `rmd ddl add`, `rmd ddl rm`) are added below. ---
