//! Weekly and monthly PDF reports (port of `report.py`). OWNER: reports agent.

use std::path::{Path, PathBuf};

use chrono::{NaiveDate, NaiveDateTime};

/// Generate the weekly or monthly report for `target_date` (default today)
/// into `[paths] reports_path` (default `~/Desktop/reports`). `report_type` is
/// `"weekly"` or `"monthly"`. Returns the PDF path (`None` if nothing written).
pub fn generate_manual_report(
    _report_type: &str,
    _target_date: Option<NaiveDate>,
    _settings_path: Option<&Path>,
) -> anyhow::Result<Option<PathBuf>> {
    todo!("reports agent: port report.py generate_manual_report")
}

/// Generate any weekly/monthly reports that are due according to
/// `[tasks] weekly_review` / `monthly_review`. Returns `{kind: path}` for the
/// reports written.
pub fn auto_generate_reports(
    _settings_path: Option<&Path>,
    _now: Option<NaiveDateTime>,
) -> anyhow::Result<indexmap::IndexMap<String, PathBuf>> {
    todo!("reports agent: port report.py auto_generate_reports")
}
