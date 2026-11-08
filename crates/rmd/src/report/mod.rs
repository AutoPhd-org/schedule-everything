//! Weekly and monthly PDF reports (port of `report.py`).
//!
//! A report covers one calendar week (Monday to Sunday) or month: the tasks
//! completed in it (`deleted` entries of the task log), the habit completion
//! rate and a habit grid. Reports land in `[paths] reports_path` (default
//! `~/Desktop/reports`) as `weekly_report_YYYYMMDD_YYYYMMDD.pdf` and
//! `monthly_report_YYYYMM.pdf`. The reminder daemon generates the reports that
//! are due according to `[tasks] weekly_review` / `monthly_review`.

mod pages;
pub(crate) mod pdf;

use std::path::{Path, PathBuf};

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};
use indexmap::IndexMap;
use serde_json::Value;

use crate::py::{self, Dict};

const DEFAULT_REPORTS_PATH: &str = "~/Desktop/reports";

/// Weekday names accepted by `[tasks] weekly_review` (Monday = 0).
const WEEKDAY_ALIASES: &[(&str, u32)] = &[
    ("mon", 0),
    ("monday", 0),
    ("tue", 1),
    ("tues", 1),
    ("tuesday", 1),
    ("wed", 2),
    ("wednesday", 2),
    ("thu", 3),
    ("thur", 3),
    ("thurs", 3),
    ("thursday", 3),
    ("fri", 4),
    ("friday", 4),
    ("sat", 5),
    ("saturday", 5),
    ("sun", 6),
    ("sunday", 6),
];

// =============================================================================
// Small Python-compatible helpers
// =============================================================================

/// An `OSError` worded like Python's: `[Errno 13] Permission denied: '/path'`.
pub(crate) fn os_error(error: std::io::Error, path: &Path) -> anyhow::Error {
    anyhow::anyhow!(os_error_message(&error, path))
}

fn os_error_message(error: &std::io::Error, path: &Path) -> String {
    match error.raw_os_error() {
        Some(code) => {
            let text = error.to_string();
            let reason = text.strip_suffix(&format!(" (os error {code})")).unwrap_or(&text);
            format!("[Errno {code}] {reason}: {}", py::repr_str(&path.display().to_string()))
        }
        None => error.to_string(),
    }
}

/// Python's type name of a JSON value, for `AttributeError` messages.
pub(crate) fn py_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// `os.path.expandvars`: `$NAME` and `${NAME}` for variables that are set.
fn expandvars(text: &str) -> String {
    static VAR: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = VAR.get_or_init(|| regex::Regex::new(r"\$([A-Za-z0-9_]+|\{[^}]*\})").expect("valid regex"));
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for caps in re.captures_iter(text) {
        let whole = caps.get(0).expect("match");
        let name = caps[1].strip_prefix('{').and_then(|n| n.strip_suffix('}')).unwrap_or(&caps[1]);
        if let Some(value) = std::env::var_os(name) {
            out.push_str(&text[last..whole.start()]);
            out.push_str(&value.to_string_lossy());
            last = whole.end();
        }
    }
    out.push_str(&text[last..]);
    out
}

/// `_expand_path`: expand `$VARS` and `~`; relative paths are resolved
/// against `base_dir`.
fn expand_path(raw: &str, base_dir: &Path) -> PathBuf {
    // `Path(...)` normalises separators before `expanduser()`.
    let normalized: PathBuf = Path::new(&expandvars(raw)).components().collect();
    let expanded = normalized.to_str().map_or_else(|| normalized.clone(), crate::paths::expand_user);
    if expanded.is_absolute() { expanded } else { crate::paths::resolve(&base_dir.join(expanded)) }
}

/// `[paths] reports_path` from settings, resolved against the config dir.
fn reports_path_from(settings: &Dict, config_dir: &Path) -> PathBuf {
    let raw = match settings.get("paths") {
        Some(Value::Object(paths)) => paths.get("reports_path").map(py::py_str),
        _ => None,
    };
    expand_path(raw.as_deref().unwrap_or(DEFAULT_REPORTS_PATH), config_dir)
}

/// `len(record.get("completed", {}))`.
fn completed_len(record: &Value) -> usize {
    match record.get("completed") {
        Some(Value::Object(map)) => map.len(),
        Some(Value::Array(items)) => items.len(),
        Some(Value::String(text)) => text.chars().count(),
        _ => 0,
    }
}

/// `habit_id in record.get("completed", {})`.
fn habit_completed(record: &Value, habit_id: &str) -> bool {
    match record.get("completed") {
        Some(Value::Object(map)) => map.contains_key(habit_id),
        Some(Value::Array(items)) => items.iter().any(|item| item.as_str() == Some(habit_id)),
        Some(Value::String(text)) => text.contains(habit_id),
        _ => false,
    }
}

// =============================================================================
// Date ranges and filtering
// =============================================================================

/// Monday and Sunday of the week containing `target`.
pub fn week_range(target: NaiveDate) -> (NaiveDate, NaiveDate) {
    let start = target - Duration::days(i64::from(target.weekday().num_days_from_monday()));
    (start, start + Duration::days(6))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(next_year, next_month, 1).and_then(|first| first.pred_opt()).map_or(31, |last| last.day())
}

/// First and last day of the month containing `target`.
pub fn month_range(target: NaiveDate) -> (NaiveDate, NaiveDate) {
    let start = target.with_day(1).unwrap_or(target);
    let end = target.with_day(days_in_month(target.year(), target.month())).unwrap_or(target);
    (start, end)
}

/// Completed (`deleted`) task-log entries whose timestamp falls in the range.
/// Offsets are dropped, not converted (`ts.replace(tzinfo=None)`).
pub fn filter_tasks(task_log: &[Value], start: NaiveDate, end: NaiveDate) -> Vec<Value> {
    let start_dt = start.and_time(NaiveTime::MIN);
    let end_dt = end.and_hms_micro_opt(23, 59, 59, 999_999).unwrap_or(start_dt);
    task_log
        .iter()
        .filter(|entry| entry.get("action").and_then(Value::as_str) == Some("deleted"))
        .filter(|entry| {
            let Some(Value::String(ts)) = entry.get("timestamp") else {
                return false;
            };
            let ts = match py::parse_iso_datetime(ts) {
                Some(py::PyDateTime::Naive(naive)) => naive,
                Some(py::PyDateTime::Aware(aware)) => aware.naive_local(),
                None => return false,
            };
            start_dt <= ts && ts <= end_dt
        })
        .cloned()
        .collect()
}

/// Habit records dated within the range, by date string (the last record of
/// a day wins). Dates compare as strings, like the Python.
pub fn filter_habits(habit_records: &[Value], start: NaiveDate, end: NaiveDate) -> IndexMap<String, Value> {
    let (start, end) = (start.format("%Y-%m-%d").to_string(), end.format("%Y-%m-%d").to_string());
    let mut filtered = IndexMap::new();
    for record in habit_records {
        if let Some(Value::String(date)) = record.get("date")
            && !date.is_empty()
            && start.as_str() <= date.as_str()
            && date.as_str() <= end.as_str()
        {
            filtered.insert(date.clone(), record.clone());
        }
    }
    filtered
}

// =============================================================================
// Review schedules
// =============================================================================

/// `[tasks] weekly_review` such as `"sunday 20:00"` → (weekday, time).
/// Errors carry the Python `ValueError` text.
fn parse_weekly_schedule(value: &str) -> Result<(u32, NaiveTime), String> {
    if value.is_empty() {
        return Err("Weekly schedule value is empty".to_string());
    }
    let parts: Vec<&str> = value.split_whitespace().collect();
    let [weekday_token, time_token] = parts[..] else {
        return Err("Weekly schedule must look like 'sunday 20:00' (weekday + time).".to_string());
    };
    let lowered = weekday_token.to_lowercase();
    let Some(&(_, weekday)) = WEEKDAY_ALIASES.iter().find(|(name, _)| *name == lowered) else {
        return Err(format!("Unknown weekday '{weekday_token}' in weekly schedule"));
    };
    let time = py::parse_hhmm(time_token).ok_or_else(|| format!("Invalid weekly review time '{time_token}'"))?;
    Ok((weekday, time))
}

/// `[tasks] monthly_review` such as `"1 20:00"` → (day of month, time).
fn parse_monthly_schedule(value: &str) -> Result<(u32, NaiveTime), String> {
    if value.is_empty() {
        return Err("Monthly schedule value is empty".to_string());
    }
    let parts: Vec<&str> = value.split_whitespace().collect();
    let [day_token, time_token] = parts[..] else {
        return Err("Monthly schedule must look like '1 20:00' (day-of-month + time).".to_string());
    };
    let day = py::parse_int_str(day_token).ok_or_else(|| format!("Invalid day '{day_token}' in monthly schedule"))?;
    if !(1..=31).contains(&day) {
        return Err("Monthly review day must be between 1 and 31".to_string());
    }
    let time = py::parse_hhmm(time_token).ok_or_else(|| format!("Invalid monthly review time '{time_token}'"))?;
    Ok((day as u32, time))
}

/// Most recent `weekday` at `time` at or before `now`.
fn last_weekly_occurrence(now: NaiveDateTime, weekday: u32, time: NaiveTime) -> NaiveDateTime {
    let days_since = (i64::from(now.weekday().num_days_from_monday()) - i64::from(weekday)).rem_euclid(7);
    let occurrence = (now.date() - Duration::days(days_since)).and_time(time);
    if occurrence > now { occurrence - Duration::days(7) } else { occurrence }
}

/// Most recent `day` of a month (clamped to short months) at `time`, at or before `now`.
fn last_monthly_occurrence(now: NaiveDateTime, day: u32, time: NaiveTime) -> NaiveDateTime {
    let on_day = |year: i32, month: u32| {
        let day = day.min(days_in_month(year, month));
        NaiveDate::from_ymd_opt(year, month, day).map(|date| date.and_time(time))
    };
    let (year, month) = (now.year(), now.month());
    let occurrence = on_day(year, month).unwrap_or(now);
    if occurrence <= now {
        return occurrence;
    }
    let (year, month) = if month == 1 { (year - 1, 12) } else { (year, month - 1) };
    on_day(year, month).unwrap_or(occurrence)
}

/// The review value as text; Python called `.strip()` on it, so anything but
/// a string fails the same way.
fn schedule_text(value: &Value) -> anyhow::Result<&str> {
    value.as_str().ok_or_else(|| anyhow::anyhow!("'{}' object has no attribute 'strip'", py_type_name(value)))
}

// =============================================================================
// Report generator
// =============================================================================

/// Writes report PDFs into one directory (Python `ReportGenerator`).
#[derive(Clone, Debug)]
pub struct ReportGenerator {
    reports_path: PathBuf,
}

impl ReportGenerator {
    /// Use `reports_path` (with `~` expanded), creating it if missing.
    pub fn new(reports_path: &Path) -> anyhow::Result<Self> {
        let reports_path = match reports_path.to_str() {
            Some(text) if text.starts_with('~') => crate::paths::expand_user(text),
            _ => reports_path.to_path_buf(),
        };
        std::fs::create_dir_all(&reports_path).map_err(|e| os_error(e, &reports_path))?;
        Ok(Self { reports_path })
    }

    /// Directory the reports are written to.
    pub fn reports_path(&self) -> &Path {
        &self.reports_path
    }

    /// `weekly_report_YYYYMMDD_YYYYMMDD.pdf` for a Monday–Sunday week.
    pub fn weekly_report_path(&self, start: NaiveDate, end: NaiveDate) -> PathBuf {
        self.reports_path.join(format!("weekly_report_{}_{}.pdf", start.format("%Y%m%d"), end.format("%Y%m%d")))
    }

    /// `monthly_report_YYYYMM.pdf` for the month starting at `month_start`.
    pub fn monthly_report_path(&self, month_start: NaiveDate) -> PathBuf {
        self.reports_path.join(format!("monthly_report_{}.pdf", month_start.format("%Y%m")))
    }

    /// Generate the weekly/monthly reports that are due per the `[tasks]`
    /// review schedule: the report for the period before the most recent
    /// review time, unless its file already exists. Returns `{kind: path}`.
    pub fn generate_due_reports(
        &self,
        task_schedule_config: &Dict,
        task_log: &[Value],
        habit_records: &[Value],
        habits_config: &IndexMap<String, String>,
        now: Option<NaiveDateTime>,
    ) -> anyhow::Result<IndexMap<String, PathBuf>> {
        let now = now.unwrap_or_else(crate::clock::now);
        let mut generated = IndexMap::new();

        if let Some(value) = task_schedule_config.get("weekly_review").filter(|v| py::truthy(v)) {
            match parse_weekly_schedule(schedule_text(value)?) {
                Ok((weekday, time)) => {
                    let last = last_weekly_occurrence(now, weekday, time);
                    let (week_start, week_end) = week_range(last.date());
                    let target = self.weekly_report_path(week_start, week_end);
                    if !target.exists() {
                        self.generate_weekly_report(task_log, habit_records, habits_config, Some(week_end))?;
                        generated.insert("weekly".to_string(), target);
                    }
                }
                Err(message) => println!("⚠️  Skipping weekly auto-report: {message}"),
            }
        }

        if let Some(value) = task_schedule_config.get("monthly_review").filter(|v| py::truthy(v)) {
            match parse_monthly_schedule(schedule_text(value)?) {
                Ok((day, time)) => {
                    let last = last_monthly_occurrence(now, day, time);
                    let target_date = last.date() - Duration::days(1);
                    let (month_start, _) = month_range(target_date);
                    let target = self.monthly_report_path(month_start);
                    if !target.exists() {
                        self.generate_monthly_report(task_log, habit_records, habits_config, Some(target_date))?;
                        generated.insert("monthly".to_string(), target);
                    }
                }
                Err(message) => println!("⚠️  Skipping monthly auto-report: {message}"),
            }
        }

        Ok(generated)
    }

    /// Weekly report for the week containing `target_date` (default today).
    pub fn generate_weekly_report(
        &self,
        task_log: &[Value],
        habit_records: &[Value],
        habits_config: &IndexMap<String, String>,
        target_date: Option<NaiveDate>,
    ) -> anyhow::Result<PathBuf> {
        let (start, end) = week_range(target_date.unwrap_or_else(crate::clock::today));
        let tasks = filter_tasks(task_log, start, end);
        let habits_data = filter_habits(habit_records, start, end);
        let filepath = self.weekly_report_path(start, end);

        let fonts = pdf::Fonts::load();
        let summary = pages::Summary { start, end, tasks: &tasks, habits_data: &habits_data, habits_config };
        let mut doc = vec![pages::weekly_summary(&fonts, &summary)];
        if !tasks.is_empty() {
            doc.extend(pages::task_list(&fonts, &tasks, "Weekly Completed Tasks"));
        }
        let title = format!("Weekly Report: {} - {}", start.format("%b %d"), end.format("%b %d, %Y"));
        pdf::write_pdf(&filepath, &title, &doc)?;

        println!("✅ Weekly report generated: {}", filepath.display());
        Ok(filepath)
    }

    /// Monthly report for the month containing `target_date` (default today).
    pub fn generate_monthly_report(
        &self,
        task_log: &[Value],
        habit_records: &[Value],
        habits_config: &IndexMap<String, String>,
        target_date: Option<NaiveDate>,
    ) -> anyhow::Result<PathBuf> {
        let (start, end) = month_range(target_date.unwrap_or_else(crate::clock::today));
        let tasks = filter_tasks(task_log, start, end);
        let habits_data = filter_habits(habit_records, start, end);
        let filepath = self.monthly_report_path(start);

        let fonts = pdf::Fonts::load();
        let summary = pages::Summary { start, end, tasks: &tasks, habits_data: &habits_data, habits_config };
        let mut doc = vec![pages::monthly_summary(&fonts, &summary)];
        if !tasks.is_empty() {
            doc.extend(pages::task_list(&fonts, &tasks, "Monthly Completed Tasks"));
        }
        let title = format!("Monthly Report: {}", start.format("%B %Y"));
        pdf::write_pdf(&filepath, &title, &doc)?;

        println!("✅ Monthly report generated: {}", filepath.display());
        Ok(filepath)
    }
}

// =============================================================================
// Data files
// =============================================================================

/// A JSON list file (task log, habit records); warns and gives `[]` when
/// missing or unreadable.
fn load_json_file(path: &Path) -> Vec<Value> {
    if !path.exists() {
        println!("⚠️  Data file not found: {}", path.display());
        return Vec::new();
    }
    let parsed = std::fs::read_to_string(path)
        .map_err(|e| os_error_message(&e, path))
        .and_then(|text| serde_json::from_str::<Value>(&text).map_err(|e| e.to_string()));
    match parsed {
        Ok(Value::Array(items)) => items,
        Ok(_) => Vec::new(),
        Err(message) => {
            println!("⚠️  Could not read {}: {message}", path.display());
            Vec::new()
        }
    }
}

/// Habits as `{id: name}`; warns and gives `{}` when missing or invalid.
fn load_habits_config(path: &Path) -> IndexMap<String, String> {
    if !path.exists() {
        println!("⚠️  Habits configuration not found: {}", path.display());
        return IndexMap::new();
    }
    match crate::config::load_toml_file(path) {
        Ok(data) => crate::data::habits_from_dict(&data),
        Err(e) => {
            println!("⚠️  Could not load habits file {}: {e}", path.display());
            IndexMap::new()
        }
    }
}

/// Everything a report needs from the active config set.
struct ReportInputs {
    settings: Dict,
    generator: ReportGenerator,
    task_log: Vec<Value>,
    habit_records: Vec<Value>,
    habits_config: IndexMap<String, String>,
}

fn load_inputs(settings_path: Option<&Path>) -> anyhow::Result<ReportInputs> {
    let paths = crate::paths::runtime();
    let settings_path = settings_path.unwrap_or(&paths.settings_path);
    let settings = crate::config::load_toml_file(settings_path)?;
    let reports_path = reports_path_from(&settings, &paths.active_config_dir);
    let task_log = load_json_file(&paths.task_log_path);
    let habit_records = load_json_file(&paths.record_path);
    let habits_config = load_habits_config(&paths.habit_path);
    let generator = ReportGenerator::new(&reports_path)?;
    Ok(ReportInputs { settings, generator, task_log, habit_records, habits_config })
}

/// Generate the weekly or monthly report for `target_date` (default today)
/// into `[paths] reports_path` (default `~/Desktop/reports`). `report_type` is
/// `"weekly"` or `"monthly"`. Returns the PDF path (`None` if nothing written).
///
/// A missing settings file is a [`crate::config::TomlLoadError::NotFound`]
/// inside the returned error.
pub fn generate_manual_report(
    report_type: &str,
    target_date: Option<NaiveDate>,
    settings_path: Option<&Path>,
) -> anyhow::Result<Option<PathBuf>> {
    let inputs = load_inputs(settings_path)?;
    let (log, records, habits) = (&inputs.task_log, &inputs.habit_records, &inputs.habits_config);
    match report_type {
        "weekly" => inputs.generator.generate_weekly_report(log, records, habits, target_date).map(Some),
        "monthly" => inputs.generator.generate_monthly_report(log, records, habits, target_date).map(Some),
        other => Err(anyhow::anyhow!("Unknown report type: {other}")),
    }
}

/// Generate any weekly/monthly reports that are due according to
/// `[tasks] weekly_review` / `monthly_review`. Returns `{kind: path}` for the
/// reports written.
///
/// A missing settings file is a [`crate::config::TomlLoadError::NotFound`]
/// inside the returned error (the daemon logs it as "settings.toml not found").
pub fn auto_generate_reports(
    settings_path: Option<&Path>,
    now: Option<NaiveDateTime>,
) -> anyhow::Result<IndexMap<String, PathBuf>> {
    let inputs = load_inputs(settings_path)?;
    let schedule = match inputs.settings.get("tasks") {
        Some(Value::Object(tasks)) => tasks.clone(),
        _ => Dict::new(),
    };
    inputs.generator.generate_due_reports(
        &schedule,
        &inputs.task_log,
        &inputs.habit_records,
        &inputs.habits_config,
        now,
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use serde_json::json;

    use super::*;
    use crate::config::TomlLoadError;
    use crate::test_support::TestEnv;

    pub(crate) fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn at(text: &str) -> NaiveDateTime {
        py::parse_naive_datetime(text).unwrap()
    }

    fn time(hour: u32, minute: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(hour, minute, 0).unwrap()
    }

    pub(crate) fn log_entry(action: &str, timestamp: &str, description: &str, priority: i64) -> Value {
        json!({"timestamp": timestamp, "action": action, "task": {"description": description, "priority": priority}})
    }

    /// Completed tasks across November 2025 (two a day, some in Chinese).
    pub(crate) fn sample_log() -> Vec<Value> {
        let names = ["Review code", "写周报并发给导师", "Read the attention paper and take detailed notes", "Gym"];
        let mut log = Vec::new();
        for day in 1..=30 {
            for (slot, hour) in [(0, 9), (1, 18)] {
                let name = names[(day as usize + slot) % names.len()];
                log.push(log_entry("added", &format!("2025-11-{day:02}T{:02}:00:00+00:00", hour - 2), name, 5));
                log.push(log_entry("deleted", &format!("2025-11-{day:02}T{hour:02}:30:00+00:00"), name, 5));
            }
        }
        log
    }

    pub(crate) fn sample_habits() -> IndexMap<String, String> {
        [
            ("1", "Go to sleep on time"),
            ("2", "早起读书二十分钟"),
            ("3", "Practice the piano scales and arpeggios every evening"),
        ]
        .into_iter()
        .map(|(id, name)| (id.to_string(), name.to_string()))
        .collect()
    }

    pub(crate) fn habit_record(date: &str, ids: &[&str]) -> Value {
        let completed: Dict = ids.iter().map(|id| (id.to_string(), json!(format!("habit {id}")))).collect();
        json!({"date": date, "completed": completed, "timestamp": format!("{date}T22:00:00+00:00")})
    }

    pub(crate) fn sample_records() -> Vec<Value> {
        (1..=30)
            .map(|day| {
                let ids: Vec<&str> = ["1", "2", "3"].into_iter().filter(|id| (day + id.len() * 7) % 3 != 0).collect();
                habit_record(&format!("2025-11-{day:02}"), &ids)
            })
            .collect()
    }

    fn schedule(value: Value) -> Dict {
        value.as_object().cloned().unwrap()
    }

    /// Page count from the PDF page tree.
    pub(crate) fn pdf_page_count(path: &Path) -> usize {
        let bytes = std::fs::read(path).unwrap();
        assert!(bytes.starts_with(b"%PDF-"), "{} is not a PDF", path.display());
        let text = String::from_utf8_lossy(&bytes);
        let re = regex::Regex::new(r"/Count (\d+)").unwrap();
        re.captures_iter(&text).filter_map(|caps| caps[1].parse().ok()).max().unwrap_or(0)
    }

    /// Fixture config root with `HOME` inside it, so the default
    /// `~/Desktop/reports` never touches the real desktop.
    fn report_env() -> TestEnv {
        let mut env = TestEnv::with_fixture();
        let home = env.root().join("home");
        std::fs::create_dir_all(&home).unwrap();
        env.set_var("HOME", home.to_str().unwrap());
        env.write("tasks/tasks.log", &py::dumps_pretty(&sample_log()));
        env.write("tasks/record.json", &py::dumps_pretty(&sample_records()));
        env.write(
            "user_config_0/habits.toml",
            "[habits]\n1 = \"Go to sleep on time\"\n2 = \"早起读书二十分钟\"\n3 = \"Meditate\"\n",
        );
        env
    }

    /// Add lines at the top of the fixture's `[paths]` table.
    fn add_paths_settings(env: &TestEnv, lines: &str) {
        let settings = env.read("user_config_0/settings.toml").replace("[paths]\n", &format!("[paths]\n{lines}\n"));
        env.write("user_config_0/settings.toml", &settings);
    }

    #[test]
    fn review_time_properties_default_to_empty() {
        // test_report_generation.py: weekly/monthly_review_time properties.
        let mut config = crate::config::ScheduleConfig::default();
        assert_eq!(config.weekly_review_time(), "");
        assert_eq!(config.monthly_review_time(), "");
        config.tasks.insert("weekly_review".into(), json!("sunday 20:00"));
        config.tasks.insert("monthly_review".into(), json!("1 20:00"));
        assert_eq!(config.weekly_review_time(), "sunday 20:00");
        assert_eq!(config.monthly_review_time(), "1 20:00");
    }

    #[test]
    fn week_range_runs_monday_to_sunday() {
        let week = (date(2025, 11, 10), date(2025, 11, 16));
        assert_eq!(week_range(date(2025, 11, 10)), week);
        assert_eq!(week_range(date(2025, 11, 12)), week);
        assert_eq!(week_range(date(2025, 11, 16)), week);
        assert_eq!(week_range(date(2025, 12, 31)), (date(2025, 12, 29), date(2026, 1, 4)));
    }

    #[test]
    fn month_range_covers_the_whole_month() {
        assert_eq!(month_range(date(2024, 2, 10)), (date(2024, 2, 1), date(2024, 2, 29)));
        assert_eq!(month_range(date(2025, 2, 28)), (date(2025, 2, 1), date(2025, 2, 28)));
        assert_eq!(month_range(date(2025, 4, 1)), (date(2025, 4, 1), date(2025, 4, 30)));
        assert_eq!(month_range(date(2025, 12, 31)), (date(2025, 12, 1), date(2025, 12, 31)));
    }

    #[test]
    fn filter_tasks_keeps_completions_in_range() {
        let log = vec![
            log_entry("added", "2025-11-12T09:00:00+00:00", "Added only", 1),
            log_entry("deleted", "2025-11-09T23:59:59", "Day before", 1),
            log_entry("deleted", "2025-11-10T00:00:00", "First moment", 2),
            // The offset is dropped, not converted (07:30 UTC on the 17th).
            log_entry("deleted", "2025-11-16T23:30:00-08:00", "Last evening", 3),
            log_entry("deleted", "2025-11-17T00:00:00", "Day after", 4),
            log_entry("cancelled", "2025-11-12T10:00:00", "Cancelled", 5),
            log_entry("deleted", "not a date", "Bad timestamp", 6),
            json!({"action": "deleted", "task": {"description": "No timestamp"}}),
            json!({"action": "deleted", "timestamp": "", "task": {"description": "Empty timestamp"}}),
            json!("not an entry"),
        ];
        let kept: Vec<String> = filter_tasks(&log, date(2025, 11, 10), date(2025, 11, 16))
            .iter()
            .map(|entry| entry["task"]["description"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(kept, ["First moment", "Last evening"]);
    }

    #[test]
    fn filter_habits_keys_records_by_date() {
        let records = vec![
            habit_record("2025-11-09", &["1"]),
            habit_record("2025-11-10", &["1"]),
            habit_record("2025-11-12", &["1", "2"]),
            habit_record("2025-11-12", &["2"]),
            habit_record("2025-11-17", &["1"]),
            json!({"completed": {"1": "no date"}}),
            json!({"date": 20251111, "completed": {}}),
        ];
        let filtered = filter_habits(&records, date(2025, 11, 10), date(2025, 11, 16));
        assert_eq!(filtered.keys().collect::<Vec<_>>(), ["2025-11-10", "2025-11-12"]);
        // The last record of a day wins.
        assert_eq!(filtered["2025-11-12"], records[3]);
    }

    #[test]
    fn habit_completion_accepts_tables_lists_and_text() {
        let table = json!({"completed": {"1": "Sleep", "2": "Read"}});
        assert!(habit_completed(&table, "1") && !habit_completed(&table, "3"));
        assert_eq!(completed_len(&table), 2);
        let list = json!({"completed": ["1", "3"]});
        assert!(habit_completed(&list, "3") && !habit_completed(&list, "2"));
        assert_eq!(completed_len(&list), 2);
        assert!(!habit_completed(&json!({"date": "2025-11-10"}), "1"));
        assert_eq!(completed_len(&json!({"date": "2025-11-10"})), 0);
    }

    #[test]
    fn report_file_names() {
        let env = TestEnv::new();
        let generator = ReportGenerator::new(&env.root().join("nested/reports")).unwrap();
        assert!(generator.reports_path().is_dir());
        let weekly = generator.weekly_report_path(date(2025, 11, 10), date(2025, 11, 16));
        assert_eq!(weekly, env.root().join("nested/reports/weekly_report_20251110_20251116.pdf"));
        let monthly = generator.monthly_report_path(date(2025, 11, 1));
        assert_eq!(monthly, env.root().join("nested/reports/monthly_report_202511.pdf"));
    }

    #[test]
    fn review_schedules_parse_like_python() {
        assert_eq!(parse_weekly_schedule("sunday 20:00"), Ok((6, time(20, 0))));
        assert_eq!(parse_weekly_schedule("  Thurs 9:05 "), Ok((3, time(9, 5))));
        assert_eq!(parse_weekly_schedule("SUN 07:30"), Ok((6, time(7, 30))));
        let shape = "Weekly schedule must look like 'sunday 20:00' (weekday + time).";
        assert_eq!(parse_weekly_schedule("sunday"), Err(shape.to_string()));
        assert_eq!(parse_weekly_schedule("tues 20:00 x"), Err(shape.to_string()));
        assert_eq!(parse_weekly_schedule("funday 20:00"), Err("Unknown weekday 'funday' in weekly schedule".into()));
        assert_eq!(parse_weekly_schedule("sunday 25:00"), Err("Invalid weekly review time '25:00'".into()));

        assert_eq!(parse_monthly_schedule("1 20:00"), Ok((1, time(20, 0))));
        assert_eq!(parse_monthly_schedule("31 08:30"), Ok((31, time(8, 30))));
        assert_eq!(parse_monthly_schedule("+5 20:00"), Ok((5, time(20, 0))));
        assert_eq!(parse_monthly_schedule(" 07 6:5"), Ok((7, time(6, 5))));
        let shape = "Monthly schedule must look like '1 20:00' (day-of-month + time).";
        assert_eq!(parse_monthly_schedule("1 20:00 extra"), Err(shape.to_string()));
        assert_eq!(parse_monthly_schedule("first 20:00"), Err("Invalid day 'first' in monthly schedule".into()));
        let range = "Monthly review day must be between 1 and 31";
        assert_eq!(parse_monthly_schedule("0 20:00"), Err(range.to_string()));
        assert_eq!(parse_monthly_schedule("32 20:00"), Err(range.to_string()));
        assert_eq!(parse_monthly_schedule("1 8pm"), Err("Invalid monthly review time '8pm'".into()));
    }

    #[test]
    fn last_review_occurrences() {
        let sunday = |now: &str| last_weekly_occurrence(at(now), 6, time(20, 0));
        assert_eq!(sunday("2025-11-16T20:00:00"), at("2025-11-16T20:00:00"));
        assert_eq!(sunday("2025-11-16T19:59:00"), at("2025-11-09T20:00:00"));
        assert_eq!(sunday("2025-11-12T08:00:00"), at("2025-11-09T20:00:00"));

        // Day 31 falls back to the last day of shorter months.
        assert_eq!(last_monthly_occurrence(at("2025-03-05T10:00:00"), 31, time(20, 0)), at("2025-02-28T20:00:00"));
        assert_eq!(last_monthly_occurrence(at("2026-01-01T19:00:00"), 1, time(20, 0)), at("2025-12-01T20:00:00"));
        assert_eq!(last_monthly_occurrence(at("2026-01-01T20:00:00"), 1, time(20, 0)), at("2026-01-01T20:00:00"));
    }

    #[test]
    fn due_reports_cover_the_period_before_the_last_review() {
        let env = TestEnv::new();
        let generator = ReportGenerator::new(&env.root().join("reports")).unwrap();
        let reviews = schedule(json!({"weekly_review": "sunday 20:00", "monthly_review": "1 20:00"}));
        let (log, records, habits) = (sample_log(), sample_records(), sample_habits());
        let due = |now: &str| generator.generate_due_reports(&reviews, &log, &records, &habits, Some(at(now))).unwrap();

        // Sunday 20:00: this week's report, and October's (reviewed on Nov 1).
        let generated = due("2025-11-16T20:00:00");
        assert_eq!(generated.keys().collect::<Vec<_>>(), ["weekly", "monthly"]);
        assert_eq!(generated["weekly"], generator.weekly_report_path(date(2025, 11, 10), date(2025, 11, 16)));
        assert_eq!(generated["monthly"], generator.monthly_report_path(date(2025, 10, 1)));
        // Summary page plus one page of completed tasks (14 this week).
        assert_eq!(pdf_page_count(&generated["weekly"]), 2);
        // October had no completed tasks: only the summary page.
        assert_eq!(pdf_page_count(&generated["monthly"]), 1);

        // Existing reports are not generated again.
        assert!(due("2025-11-16T20:00:00").is_empty());

        // A minute before the review, the previous week is the one due.
        let generated = due("2025-11-16T19:59:00");
        assert_eq!(generated.keys().collect::<Vec<_>>(), ["weekly"]);
        assert_eq!(generated["weekly"], generator.weekly_report_path(date(2025, 11, 3), date(2025, 11, 9)));

        // On Dec 1 after 20:00, November's report (60 tasks: summary + 3 pages).
        let generated = due("2025-12-01T20:00:00");
        assert_eq!(generated["monthly"], generator.monthly_report_path(date(2025, 11, 1)));
        assert_eq!(pdf_page_count(&generated["monthly"]), 4);
    }

    #[test]
    fn invalid_or_missing_review_schedules_generate_nothing() {
        let env = TestEnv::new();
        let generator = ReportGenerator::new(&env.root().join("reports")).unwrap();
        let now = Some(at("2025-11-16T20:00:00"));
        for reviews in [
            json!({"weekly_review": "someday 20:00", "monthly_review": "1"}),
            json!({"weekly_review": "", "monthly_review": ""}),
            json!({"daily_summary": "22:00"}),
        ] {
            let generated = generator.generate_due_reports(&schedule(reviews), &[], &[], &IndexMap::new(), now);
            assert!(generated.unwrap().is_empty());
        }
        assert_eq!(std::fs::read_dir(generator.reports_path()).unwrap().count(), 0);

        // Python called `.strip()` on the value.
        let reviews = schedule(json!({"weekly_review": 7}));
        let error = generator.generate_due_reports(&reviews, &[], &[], &IndexMap::new(), now).unwrap_err();
        assert_eq!(error.to_string(), "'int' object has no attribute 'strip'");
    }

    #[test]
    fn manual_reports_use_the_settings_reports_path() {
        let mut env = report_env();
        env.set_now("2025-11-12T10:30:00");
        // Relative paths are resolved against the active config directory.
        add_paths_settings(&env, "reports_path = \"reports\"");
        let reports = crate::paths::resolve(&env.root().join("user_config_0/reports"));

        let weekly = generate_manual_report("weekly", None, None).unwrap().unwrap();
        assert_eq!(weekly, reports.join("weekly_report_20251110_20251116.pdf"));
        assert_eq!(pdf_page_count(&weekly), 2);

        let monthly = generate_manual_report("monthly", Some(date(2025, 11, 30)), None).unwrap().unwrap();
        assert_eq!(monthly, reports.join("monthly_report_202511.pdf"));
        assert_eq!(pdf_page_count(&monthly), 4);
    }

    #[test]
    fn manual_reports_default_to_the_desktop() {
        let mut env = report_env();
        env.set_now("2025-11-12T10:30:00");
        let path = generate_manual_report("weekly", None, None).unwrap().unwrap();
        assert_eq!(path, env.root().join("home/Desktop/reports/weekly_report_20251110_20251116.pdf"));
        assert!(path.is_file());
    }

    #[test]
    fn manual_reports_survive_missing_data_files() {
        let env = report_env();
        add_paths_settings(&env, "reports_path = \"reports\"");
        for file in ["tasks/tasks.log", "tasks/record.json", "user_config_0/habits.toml"] {
            std::fs::remove_file(env.root().join(file)).unwrap();
        }
        let path = generate_manual_report("monthly", Some(date(2025, 11, 1)), None).unwrap().unwrap();
        assert_eq!(pdf_page_count(&path), 1);
    }

    #[test]
    fn unknown_report_type_is_an_error() {
        let env = report_env();
        let error = generate_manual_report("daily", None, None).unwrap_err();
        assert_eq!(error.to_string(), "Unknown report type: daily");
        // The reports directory is still prepared first, as in Python.
        assert!(env.root().join("home/Desktop/reports").is_dir());
    }

    #[test]
    fn auto_reports_follow_the_settings_schedule() {
        let env = report_env();
        add_paths_settings(&env, "reports_path = \"reports\"");
        // The fixture has no [tasks] review times: nothing is due.
        assert!(auto_generate_reports(None, Some(at("2025-11-16T20:00:00"))).unwrap().is_empty());

        let settings = env.read("user_config_0/settings.toml");
        let settings = format!("{settings}\n[tasks]\nweekly_review = \"sunday 20:00\"\nmonthly_review = \"1 20:00\"\n");
        env.write("user_config_0/settings.toml", &settings);
        let generated = auto_generate_reports(None, Some(at("2025-12-01T20:00:00"))).unwrap();
        let reports = crate::paths::resolve(&env.root().join("user_config_0/reports"));
        assert_eq!(generated["weekly"], reports.join("weekly_report_20251124_20251130.pdf"));
        assert_eq!(generated["monthly"], reports.join("monthly_report_202511.pdf"));
        assert!(generated.values().all(|path| path.is_file()));
        assert!(auto_generate_reports(None, Some(at("2025-12-01T21:00:00"))).unwrap().is_empty());
    }

    #[test]
    fn missing_settings_is_a_not_found_error() {
        let env = report_env();
        let missing = env.root().join("missing/settings.toml");
        let error = auto_generate_reports(Some(&missing), None).unwrap_err();
        assert!(matches!(error.downcast_ref::<TomlLoadError>(), Some(TomlLoadError::NotFound(_))));
        assert_eq!(error.to_string(), format!("[Errno 2] No such file or directory: '{}'", missing.display()));
    }

    #[test]
    fn reports_path_expands_variables_and_home() {
        let mut env = TestEnv::new();
        env.set_var("HOME", "/home/tester");
        env.set_var("RMD_TEST_REPORTS", "/srv/reports");
        env.remove_var("RMD_UNSET_VAR");
        let base = crate::paths::resolve(env.root());
        assert_eq!(expand_path("~/Desktop/reports", &base), PathBuf::from("/home/tester/Desktop/reports"));
        assert_eq!(expand_path("~//Desktop/./reports/", &base), PathBuf::from("/home/tester/Desktop/reports"));
        assert_eq!(expand_path("$RMD_TEST_REPORTS/weekly", &base), PathBuf::from("/srv/reports/weekly"));
        assert_eq!(expand_path("${RMD_TEST_REPORTS}", &base), PathBuf::from("/srv/reports"));
        assert_eq!(expand_path("$RMD_UNSET_VAR/x", &base), base.join("$RMD_UNSET_VAR/x"));
        assert_eq!(expand_path("${}x", &base), base.join("${}x"));
        assert_eq!(expand_path("reports", &base), base.join("reports"));
        assert_eq!(expand_path("", &base), base);
        assert_eq!(expand_path("/a//b/./c/", &base), PathBuf::from("/a/b/c"));

        let settings = schedule(json!({"paths": {"reports_path": "~/r"}}));
        assert_eq!(reports_path_from(&settings, &base), PathBuf::from("/home/tester/r"));
        assert_eq!(reports_path_from(&Dict::new(), &base), PathBuf::from("/home/tester/Desktop/reports"));
    }

    #[test]
    fn os_errors_read_like_python() {
        let error = std::io::Error::from_raw_os_error(13);
        assert_eq!(os_error_message(&error, Path::new("/x/it's")), "[Errno 13] Permission denied: \"/x/it's\"");
    }
}
