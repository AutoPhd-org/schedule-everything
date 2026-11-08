//! Page layouts of the weekly and monthly reports (the matplotlib figures of
//! `report.py`): a summary page with the habit grid, then the completed tasks.

use chrono::{Duration, NaiveDate};
use indexmap::IndexMap;
use serde_json::Value;

use super::pdf::{A4_LANDSCAPE, A4_PORTRAIT, Anchor, Canvas, Fonts, Stroke, TextStyle};
use super::{completed_len, habit_completed};
use crate::py::{self, Dict};

const INK: &str = "#2f3542";
const MUTED: &str = "#7f8c8d";
const RULE: &str = "#bdc3c7";
const DONE: &str = "#2ecc71";
const NOT_DONE: &str = "#ecf0f1";
const SEPARATOR: &str = "#ecf0f1";

/// Completed tasks per task-list page.
const TASKS_PER_PAGE: usize = 20;

/// What a summary page shows.
pub(super) struct Summary<'a> {
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub tasks: &'a [Value],
    /// Habit records in the period, by `YYYY-MM-DD`.
    pub habits_data: &'a IndexMap<String, Value>,
    /// Configured habits, `{id: name}`.
    pub habits_config: &'a IndexMap<String, String>,
}

impl Summary<'_> {
    fn days(&self) -> i64 {
        (self.end - self.start).num_days() + 1
    }

    /// Habit ids sorted as strings (`sorted(habits_config.keys())`).
    fn habit_ids(&self) -> Vec<&String> {
        let mut ids: Vec<&String> = self.habits_config.keys().collect();
        ids.sort();
        ids
    }

    /// Share of habit slots (habits × days) completed, in percent. Every
    /// recorded completion counts, as in the Python report.
    fn completion_rate(&self) -> f64 {
        let total_slots = self.habits_config.len() as i64 * self.days();
        let completed: usize = self.habits_data.values().map(completed_len).sum();
        if total_slots > 0 { completed as f64 / total_slots as f64 * 100.0 } else { 0.0 }
    }

    fn is_completed(&self, habit_id: &str, day: NaiveDate) -> bool {
        let key = day.format("%Y-%m-%d").to_string();
        self.habits_data.get(&key).is_some_and(|record| habit_completed(record, habit_id))
    }

    /// Title and the two headline numbers shared by both summary pages.
    fn draw_header(&self, canvas: &mut Canvas, title: &str) {
        let (w, h) = (canvas.width, canvas.height);
        canvas.text(w * 0.5, h * 0.07, title, &TextStyle::new(20.0).bold().color(INK).centered().middle());
        let stat = TextStyle::new(14.0).color(INK).centered();
        canvas.text(w * 0.2, h * 0.15, &format!("Tasks Completed: {}", self.tasks.len()), &stat);
        let rate = format!("Habit Completion Rate: {:.1}%", self.completion_rate());
        canvas.text(w * 0.8, h * 0.15, &rate, &stat);
    }
}

/// Habit name, wrapped onto up to two lines when the row is tall enough.
fn draw_habit_name(canvas: &mut Canvas, x: f32, center_y: f32, max_width: f32, row_height: f32, name: &str, size: f32) {
    let style = TextStyle::new(size).color(INK).middle();
    let line_height = size * 1.2;
    let max_lines = if row_height >= line_height * 2.2 { 2 } else { 1 };
    let lines = canvas.fonts().wrap(name, max_width, max_lines, &style);
    let first_y = center_y - line_height * (lines.len() as f32 - 1.0) / 2.0;
    for (i, line) in lines.iter().enumerate() {
        canvas.text(x, first_y + line_height * i as f32, line, &style);
    }
}

/// Page 1 of the weekly report: totals and a habits × weekdays check grid.
pub(super) fn weekly_summary<'f>(fonts: &'f Fonts, summary: &Summary) -> Canvas<'f> {
    let mut canvas = Canvas::new(fonts, A4_LANDSCAPE);
    let (w, h) = (canvas.width, canvas.height);
    let title = format!("Weekly Report: {} - {}", summary.start.format("%b %d"), summary.end.format("%b %d, %Y"));
    summary.draw_header(&mut canvas, &title);

    let habit_ids = summary.habit_ids();
    let left = w * 0.1;
    let table_width = w * 0.8;
    let name_width = table_width * 0.28;
    let col_width = (table_width - name_width) / 7.0;
    let top = h * 0.22;
    let header_height = 36.0;
    let body_top = top + header_height;
    let row_height =
        if habit_ids.is_empty() { 0.0 } else { ((h * 0.92 - body_top) / habit_ids.len() as f32).min(44.0) };
    let day_x = |i: usize| left + name_width + col_width * (i as f32 + 0.5);

    // Header: "Habit", then weekday names with the day of month underneath.
    let header_y = top + header_height / 2.0;
    let bold = TextStyle::new(10.0).bold().color(INK).middle();
    canvas.text(left + 8.0, header_y, "Habit", &bold);
    for (i, day) in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].iter().enumerate() {
        canvas.text(day_x(i), header_y - 5.0, day, &bold.centered());
        let date = summary.start + Duration::days(i as i64);
        let small = TextStyle::new(8.0).color(MUTED).centered().middle();
        canvas.text(day_x(i), header_y + 8.0, &date.format("%d").to_string(), &small);
    }

    let name_size = (row_height * 0.6).clamp(5.0, 10.0);
    let radius = (row_height * 0.32).min(col_width * 0.3).min(11.0);
    for (i, habit_id) in habit_ids.iter().enumerate() {
        let center_y = body_top + row_height * (i as f32 + 0.5);
        let name = &summary.habits_config[*habit_id];
        draw_habit_name(&mut canvas, left + 8.0, center_y, name_width - 16.0, row_height, name, name_size);
        for day in 0..7 {
            let x = day_x(day);
            if summary.is_completed(habit_id, summary.start + Duration::days(day as i64)) {
                canvas.circle(x, center_y, radius, DONE, 0.6);
                let r = radius;
                let tick = [
                    (x - 0.42 * r, center_y + 0.02 * r),
                    (x - 0.12 * r, center_y + 0.32 * r),
                    (x + 0.45 * r, center_y - 0.3 * r),
                ];
                canvas.polyline(&tick, Stroke::new("#ffffff", (r * 0.2).max(1.0)));
            } else {
                canvas.circle(x, center_y, radius, NOT_DONE, 1.0);
            }
        }
    }

    // Rules above the header, under the header and under every row.
    let rule = Stroke::new(RULE, 0.5);
    canvas.line(left, top, left + table_width, top, rule);
    for i in 0..=habit_ids.len() {
        let y = body_top + row_height * i as f32;
        canvas.line(left, y, left + table_width, y, rule);
    }
    canvas
}

/// Page 1 of the monthly report: totals and a habits × days heatmap.
pub(super) fn monthly_summary<'f>(fonts: &'f Fonts, summary: &Summary) -> Canvas<'f> {
    let mut canvas = Canvas::new(fonts, A4_LANDSCAPE);
    let (w, h) = (canvas.width, canvas.height);
    let title = format!("Monthly Report: {}", summary.start.format("%B %Y"));
    summary.draw_header(&mut canvas, &title);

    let habit_ids = summary.habit_ids();
    let days = summary.days().max(1) as usize;
    let left = w * 0.05;
    let chart_width = w * 0.9;
    let label_width = chart_width * 0.2;
    let col_width = (chart_width - label_width) / days as f32;
    let top = h * 0.22;
    let header_height = 20.0;
    let body_top = top + header_height;
    let row_height =
        if habit_ids.is_empty() { 0.0 } else { ((h * 0.92 - body_top) / habit_ids.len() as f32).min(30.0) };
    let day_x = |d: usize| left + label_width + col_width * d as f32;

    // Day numbers (every other day, as in the Python report).
    let number = TextStyle::new(7.0).color(MUTED).centered().middle();
    for d in (0..days).step_by(2) {
        canvas.text(day_x(d) + col_width / 2.0, top + header_height / 2.0, &(d + 1).to_string(), &number);
    }

    let name_size = (row_height * 0.5).clamp(5.0, 8.0);
    for (i, habit_id) in habit_ids.iter().enumerate() {
        let row_top = body_top + row_height * i as f32;
        let name = &summary.habits_config[*habit_id];
        draw_habit_name(&mut canvas, left, row_top + row_height / 2.0, label_width - 10.0, row_height, name, name_size);
        for d in 0..days {
            let done = summary.is_completed(habit_id, summary.start + Duration::days(d as i64));
            let gap = (col_width * 0.08).min(1.5);
            let color = if done { DONE } else { NOT_DONE };
            canvas.rounded_rect(
                day_x(d) + gap,
                row_top + 2.0,
                col_width - 2.0 * gap,
                row_height - 4.0,
                1.5,
                color,
                1.0,
            );
        }
    }
    canvas
}

/// `datetime.fromisoformat(ts).strftime("%Y-%m-%d %H:%M")`, or the raw text.
fn completion_time(entry: &Value) -> String {
    let raw = match entry.get("timestamp") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => py::py_str(other),
        None => String::new(),
    };
    match py::parse_iso_datetime(&raw) {
        Some(py::PyDateTime::Naive(ts)) => ts.format("%Y-%m-%d %H:%M").to_string(),
        Some(py::PyDateTime::Aware(ts)) => ts.naive_local().format("%Y-%m-%d %H:%M").to_string(),
        None => raw,
    }
}

/// `task.get("description", "Unknown")`, drawn by matplotlib (which shows
/// nothing for `None`).
fn task_description(task: Option<&Dict>) -> String {
    match task.and_then(|t| t.get("description")) {
        None => "Unknown".to_string(),
        Some(Value::Null) => String::new(),
        Some(other) => py::py_str(other),
    }
}

/// `str(task.get("priority", "-"))`.
fn task_priority(task: Option<&Dict>) -> String {
    task.and_then(|t| t.get("priority")).map_or_else(|| "-".to_string(), py::py_str)
}

/// Tasks by timestamp text, newest first; ties keep their order, like
/// `sorted(..., reverse=True)`.
fn newest_first(tasks: &[Value]) -> Vec<&Value> {
    let mut sorted: Vec<&Value> = tasks.iter().collect();
    let timestamp = |entry: &Value| match entry.get("timestamp") {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    sorted.sort_by_key(|entry| std::cmp::Reverse(timestamp(entry)));
    sorted
}

/// The completed tasks, newest first, 20 per A4 portrait page.
pub(super) fn task_list<'f>(fonts: &'f Fonts, tasks: &[Value], title: &str) -> Vec<Canvas<'f>> {
    let sorted = newest_first(tasks);
    let total_pages = tasks.len().div_ceil(TASKS_PER_PAGE);
    let mut pages = Vec::with_capacity(total_pages);
    for (page_index, page_tasks) in sorted.chunks(TASKS_PER_PAGE).enumerate() {
        let mut canvas = Canvas::new(fonts, A4_PORTRAIT);
        let (w, h) = (canvas.width, canvas.height);
        let heading = format!("{title} (Page {}/{total_pages})", page_index + 1);
        canvas.text(w * 0.5, h * 0.05, &heading, &TextStyle::new(16.0).bold().color(INK).centered().middle());

        let (date_x, task_x, priority_x) = (w * 0.1, w * 0.3, w * 0.8);
        let header_y = h * 0.15;
        let bold = TextStyle::new(10.0).bold().color(INK);
        canvas.text(date_x, header_y, "Date", &bold);
        canvas.text(task_x, header_y, "Task", &bold);
        canvas.text(priority_x, header_y, "Priority", &bold);
        let rule_y = header_y + h * 0.01;
        canvas.line(w * 0.05, rule_y, w * 0.95, rule_y, Stroke::new("#000000", 1.0));

        let row_height = h * 0.04;
        let cell = TextStyle::new(9.0).color(INK).middle();
        for (i, entry) in page_tasks.iter().enumerate() {
            let row_top = rule_y + row_height * i as f32;
            let center_y = row_top + row_height / 2.0;
            let task = entry.get("task").and_then(Value::as_object);
            let (description, priority) = (task_description(task), task_priority(task));

            let date = canvas.fonts().fit(&completion_time(entry), task_x - date_x - 8.0, &cell);
            canvas.text(date_x, center_y, &date, &cell);
            let lines = canvas.fonts().wrap(&description, priority_x - task_x - 12.0, 2, &cell);
            let line_height = 11.0;
            let first_y = center_y - line_height * (lines.len() as f32 - 1.0) / 2.0;
            for (j, line) in lines.iter().enumerate() {
                canvas.text(task_x, first_y + line_height * j as f32, line, &cell);
            }
            let priority = canvas.fonts().fit(&priority, w * 0.95 - priority_x, &cell);
            canvas.text(priority_x, center_y, &priority, &cell.anchor(Anchor::Start));
            let separator_y = row_top + row_height;
            canvas.line(w * 0.05, separator_y, w * 0.95, separator_y, Stroke::new(SEPARATOR, 0.5));
        }
        pages.push(canvas);
    }
    pages
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use serde_json::json;
    use usvg::fontdb::Database;

    use super::*;
    use crate::report::pdf::write_pdf;
    use crate::report::tests::{date, habit_record, log_entry, pdf_page_count, sample_habits};
    use crate::report::{filter_habits, filter_tasks};
    use crate::test_support::TestEnv;

    #[test]
    fn completion_rate_counts_every_recorded_completion() {
        let habits = sample_habits();
        // "9" is not a configured habit but still counts, as in Python.
        let records = vec![habit_record("2025-11-10", &["1", "2"]), habit_record("2025-11-11", &["1", "9"])];
        let (start, end) = (date(2025, 11, 10), date(2025, 11, 16));
        let habits_data = filter_habits(&records, start, end);
        let summary = Summary { start, end, tasks: &[], habits_data: &habits_data, habits_config: &habits };
        assert_eq!(format!("{:.1}", summary.completion_rate()), "19.0"); // 4 of 3 × 7 slots
        assert!(summary.is_completed("1", date(2025, 11, 11)));
        assert!(!summary.is_completed("2", date(2025, 11, 11)));
        assert!(!summary.is_completed("1", date(2025, 11, 12)));
        assert_eq!(summary.habit_ids(), ["1", "2", "3"]);

        let no_habits = IndexMap::new();
        let summary = Summary { habits_config: &no_habits, ..summary };
        assert_eq!(summary.completion_rate(), 0.0);
    }

    #[test]
    fn habit_ids_sort_as_text() {
        let habits: IndexMap<String, String> =
            ["2", "10", "1"].iter().map(|id| (id.to_string(), format!("habit {id}"))).collect();
        let summary = Summary {
            start: date(2025, 11, 1),
            end: date(2025, 11, 30),
            tasks: &[],
            habits_data: &IndexMap::new(),
            habits_config: &habits,
        };
        assert_eq!(summary.habit_ids(), ["1", "10", "2"]);
        assert_eq!(summary.days(), 30);
    }

    #[test]
    fn task_list_is_newest_first_twenty_per_page() {
        let tasks = vec![
            log_entry("deleted", "2025-11-10T09:00:00", "Monday", 1),
            log_entry("deleted", "2025-11-12T09:00:00", "Wednesday", 2),
            log_entry("deleted", "2025-11-11T09:00:00", "Tuesday A", 3),
            log_entry("deleted", "2025-11-11T09:00:00", "Tuesday B", 4),
        ];
        let order: Vec<&str> =
            newest_first(&tasks).iter().map(|t| t["task"]["description"].as_str().unwrap()).collect();
        assert_eq!(order, ["Wednesday", "Tuesday A", "Tuesday B", "Monday"]);

        let fonts = Fonts::with_database(Arc::new(Database::new()));
        let many: Vec<Value> = (0..45)
            .map(|i| log_entry("deleted", &format!("2025-11-{:02}T10:00:00", i % 28 + 1), &format!("Task {i}"), 1))
            .collect();
        assert_eq!(task_list(&fonts, &many, "Monthly Completed Tasks").len(), 3);
        assert_eq!(task_list(&fonts, &many[..20], "Weekly Completed Tasks").len(), 1);
        assert!(task_list(&fonts, &[], "Weekly Completed Tasks").is_empty());
    }

    #[test]
    fn completion_time_shows_the_wall_clock_time() {
        assert_eq!(completion_time(&json!({"timestamp": "2025-11-16T23:30:00.123-08:00"})), "2025-11-16 23:30");
        assert_eq!(completion_time(&json!({"timestamp": "2025-11-16 07:05"})), "2025-11-16 07:05");
        assert_eq!(completion_time(&json!({"timestamp": "yesterday"})), "yesterday");
    }

    #[test]
    fn task_cells_follow_python_str() {
        let task = |value: Value| value.as_object().cloned();
        let full = task(json!({"description": "Review code", "priority": 7}));
        assert_eq!((task_description(full.as_ref()), task_priority(full.as_ref())), ("Review code".into(), "7".into()));
        let nulls = task(json!({"description": null, "priority": null}));
        assert_eq!((task_description(nulls.as_ref()), task_priority(nulls.as_ref())), (String::new(), "None".into()));
        let empty = task(json!({}));
        assert_eq!((task_description(empty.as_ref()), task_priority(empty.as_ref())), ("Unknown".into(), "-".into()));
        assert_eq!((task_description(None), task_priority(None)), ("Unknown".into(), "-".into()));
        let numbers = task(json!({"description": 42, "priority": 2.5}));
        assert_eq!((task_description(numbers.as_ref()), task_priority(numbers.as_ref())), ("42".into(), "2.5".into()));
    }

    fn crowded_pages(fonts: &Fonts, path: &Path) {
        let mut habits = IndexMap::new();
        for i in 0..40 {
            let name = format!("Habit {i} <b>&\"'</b> with a rather long name 😴 中文名称很长很长很长很长很长很长很长");
            habits.insert(i.to_string(), name);
        }
        let records: Vec<Value> =
            (1..=30).map(|day| habit_record(&format!("2025-11-{day:02}"), &["1", "2", "30", "x"])).collect();
        let log = vec![
            log_entry(
                "deleted",
                "2025-11-12T10:00:00",
                "Line\nbreak\ttab \u{FFFF} <tag> & \"quotes\" 'apos' 🎉 超长的中文任务描述没有空格可以断开超长的中文任务描述没有空格可以断开超长的中文任务描述没有空格",
                3,
            ),
            log_entry("deleted", "2025-11-12T11:00:00", &"x".repeat(400), 1),
            json!({"action": "deleted", "timestamp": "2025-11-12T12:00:00", "task": {"priority": null}}),
            json!({"action": "deleted", "timestamp": "2025-11-12T13:00:00", "task": "not a table"}),
        ];
        let (start, end) = (date(2025, 11, 10), date(2025, 11, 16));
        let (tasks, habits_data) = (filter_tasks(&log, start, end), filter_habits(&records, start, end));
        let week = Summary { start, end, tasks: &tasks, habits_data: &habits_data, habits_config: &habits };
        let (start, end) = (date(2025, 11, 1), date(2025, 11, 30));
        let month_data = filter_habits(&records, start, end);
        let month = Summary { start, end, tasks: &tasks, habits_data: &month_data, habits_config: &habits };

        let mut pages = vec![weekly_summary(fonts, &week), monthly_summary(fonts, &month)];
        pages.extend(task_list(fonts, &tasks, "Weekly Completed Tasks"));
        write_pdf(path, "Crowded", &pages).unwrap();
        assert_eq!(pdf_page_count(path), 3);
    }

    #[test]
    fn crowded_and_unusual_text_still_renders() {
        let env = TestEnv::new();
        crowded_pages(&Fonts::load(), &env.root().join("crowded.pdf"));
    }

    #[test]
    fn pages_render_without_any_fonts() {
        let env = TestEnv::new();
        crowded_pages(&Fonts::with_database(Arc::new(Database::new())), &env.root().join("no-fonts.pdf"));
    }

    #[test]
    fn long_text_is_fitted_and_wrapped() {
        let _env = TestEnv::new();
        let fonts = Fonts::load();
        let style = TextStyle::new(9.0);
        let text = "Read the attention paper and take detailed notes on the experiments section";
        let fitted = fonts.fit(text, 120.0, &style);
        assert!(fitted.ends_with('…') && fonts.width(&fitted, &style) <= 120.0, "{fitted}");
        assert_eq!(fonts.fit("Gym", 120.0, &style), "Gym");

        let lines = fonts.wrap(text, 150.0, 2, &style);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|line| fonts.width(line, &style) <= 150.0), "{lines:?}");
        // Breaks at spaces: the first line is whole words.
        assert!(text.starts_with(&format!("{} ", lines[0])), "{lines:?}");
        assert!(lines[1].ends_with('…'));

        let chinese = "超长的中文任务描述没有空格可以断开超长的中文任务描述";
        let lines = fonts.wrap(chinese, 60.0, 3, &style);
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| fonts.width(line, &style) <= 60.0), "{lines:?}");
        assert!(chinese.starts_with(&lines[0]));
    }
}
