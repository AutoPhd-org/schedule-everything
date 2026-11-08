//! Multi-page PDF visualization of the odd/even schedules (port of
//! `visualizer.py`).
//!
//! - Page 1: odd week schedule (color-coded blocks by activity type)
//! - Page 2: even week schedule
//! - Page 3: statistics comparison (work hours, pomodoros, potatoes)
//!
//! Days are columns and hours rows (06:00 to midnight). A block lasts its
//! `[time_blocks]` duration, else a default for its kind, else 30 minutes.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::ScheduleConfig;
use crate::py::{self, Dict};
use crate::report::pdf::{self, Anchor, Canvas, Fonts, HEAVY, MEDIUM, Stroke, TextStyle};

/// Block colors, matched as substrings of the lowercased activity name in
/// this order.
const COLORS: &[(&str, &str)] = &[
    ("pomodoro", "#FF6B6B"),
    ("potato", "#EE5253"),
    ("long_break", "#1DD1A1"),
    ("short_break", "#48DBFB"),
    ("napping", "#54A0FF"),
    ("meeting", "#FF9F43"),
    ("exercise", "#Feca57"),
    ("lunch", "#5F27CD"),
    ("summary_time", "#C8D6E5"),
    ("go_to_bed", "#576574"),
    ("other", "#8395A7"),
    ("deep_work", "#0ABDE3"),
];
const OTHER_COLOR: &str = "#8395A7";

/// Label colors for contrast (dark text on light blocks).
const TEXT_COLORS: &[(&str, &str)] = &[("exercise", "#333333"), ("summary_time", "#333333"), ("default", "#FFFFFF")];
const DEFAULT_TEXT_COLOR: &str = "#FFFFFF";

/// Minutes for common activity kinds missing from `[time_blocks]`.
const DEFAULT_DURATIONS: &[(&str, f64)] = &[
    ("potato", 50.0),
    ("pomodoro", 25.0),
    ("long_break", 15.0),
    ("short_break", 5.0),
    ("lunch", 60.0),
    ("napping", 20.0),
];

/// Days of the week in order (Monday first).
const DAYS_ORDER: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

/// Activities counted as work time.
const WORK_ACTIVITIES: [&str; 4] = ["pomodoro", "potato", "deep_work", "meeting"];

/// matplotlib's `figsize=(14, 9)`, in points.
const PAGE: (f32, f32) = (14.0 * 72.0, 9.0 * 72.0);

const INK: &str = "#2f3542";
const MUTED: &str = "#a4b0be";
const RULE: &str = "#dfe4ea";
const STRIPE: &str = "#F7F9FA";

/// First hour shown (the chart runs from 06:00 to 24:00).
const FIRST_HOUR: f64 = 6.0;
const LAST_HOUR: f64 = 24.0;

/// Python `str.title()`: upper-case the first cased letter of every word.
fn py_title(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous_cased = false;
    for c in text.chars() {
        if previous_cased {
            out.extend(c.to_lowercase());
        } else {
            out.extend(c.to_uppercase());
        }
        previous_cased = c.is_lowercase() || c.is_uppercase();
    }
    out
}

/// Label shown for an activity (`name.replace("_", " ").title()`).
fn display_name(activity_name: &str) -> String {
    py_title(&activity_name.replace('_', " "))
}

/// Name of an activity: the string itself, or `title` (else `block`) of a
/// `{block, title}` table.
fn extract_activity_name(activity: &Value) -> String {
    match activity {
        Value::String(name) => name.clone(),
        Value::Object(map) if map.contains_key("block") => py::py_str(map.get("title").unwrap_or(&map["block"])),
        other => py::py_str(other),
    }
}

/// Background color of an activity block.
fn activity_color(activity_name: &str) -> &'static str {
    let lower = activity_name.to_lowercase();
    COLORS.iter().find(|(key, _)| lower.contains(key)).map_or(OTHER_COLOR, |(_, color)| color)
}

/// Label color of an activity block.
fn text_color(activity_name: &str) -> &'static str {
    let lower = activity_name.to_lowercase();
    TEXT_COLORS.iter().find(|(key, _)| lower.contains(key)).map_or(DEFAULT_TEXT_COLOR, |(_, color)| color)
}

/// `"HH:MM"` as decimal hours (`int(parts[0]) + int(parts[1]) / 60`), with
/// Python's error messages for keys that are not times.
fn parse_block_time(time: &str) -> anyhow::Result<f64> {
    let mut parts = time.split(':');
    let int = |text: &str| {
        py::parse_int_str(text)
            .ok_or_else(|| anyhow::anyhow!("invalid literal for int() with base 10: {}", py::repr_str(text)))
    };
    let hour = int(parts.next().unwrap_or_default())?;
    let minute = int(parts.next().ok_or_else(|| anyhow::anyhow!("list index out of range"))?)?;
    Ok(hour as f64 + minute as f64 / 60.0)
}

/// `common` merged with one day's entries (day entries override).
fn day_schedule(schedule: &Dict, day: &str) -> Dict {
    let mut merged = Dict::new();
    for section in ["common", day] {
        if let Some(Value::Object(entries)) = schedule.get(section) {
            for (time, activity) in entries {
                merged.insert(time.clone(), activity.clone());
            }
        }
    }
    merged
}

/// Totals for one week's schedule.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WeeklyStats {
    pub pomodoro_count: usize,
    pub potato_count: usize,
    pub work_hours: f64,
}

/// Renders the odd/even week schedules (Python `ScheduleVisualizer`).
pub struct ScheduleVisualizer<'a> {
    config: &'a ScheduleConfig,
    odd_schedule: &'a Dict,
    even_schedule: &'a Dict,
}

impl<'a> ScheduleVisualizer<'a> {
    pub fn new(config: &'a ScheduleConfig, odd_schedule: &'a Dict, even_schedule: &'a Dict) -> Self {
        Self { config, odd_schedule, even_schedule }
    }

    /// Duration of an activity in hours: `[time_blocks]` (exact name), then
    /// the defaults (substring match), then 30 minutes.
    pub fn activity_duration(&self, activity_name: &str) -> anyhow::Result<f64> {
        if let Some(minutes) = self.config.time_blocks.get(activity_name) {
            return match minutes {
                Value::Number(_) | Value::Bool(_) => Ok(py::py_float(minutes).unwrap_or_default() / 60.0),
                other => Err(anyhow::anyhow!(
                    "unsupported operand type(s) for /: '{}' and 'float'",
                    crate::report::py_type_name(other)
                )),
            };
        }
        let lower = activity_name.to_lowercase();
        Ok(DEFAULT_DURATIONS.iter().find(|(key, _)| lower.contains(key)).map_or(0.5, |(_, minutes)| minutes / 60.0))
    }

    /// Pomodoro and potato counts and work hours of one week.
    pub fn weekly_stats(&self, schedule: &Dict) -> anyhow::Result<WeeklyStats> {
        let mut stats = WeeklyStats::default();
        for day in DAYS_ORDER {
            for activity in day_schedule(schedule, day).values() {
                let name = extract_activity_name(activity);
                let duration = self.activity_duration(&name)?;
                let lower = name.to_lowercase();
                let (pomodoro, potato) = (lower.contains("pomodoro"), lower.contains("potato"));
                stats.pomodoro_count += usize::from(pomodoro);
                stats.potato_count += usize::from(potato);
                if WORK_ACTIVITIES.contains(&name.as_str()) || pomodoro || potato {
                    stats.work_hours += duration;
                }
            }
        }
        Ok(stats)
    }

    /// Write the three-page PDF to `output_path` (default
    /// `~/Desktop/schedule_visualization.pdf`) and return the path.
    pub fn visualize(&self, output_path: Option<&Path>) -> anyhow::Result<PathBuf> {
        let pdf_filename = output_path
            .map(Path::to_path_buf)
            .unwrap_or_else(|| crate::paths::home_dir().join("Desktop").join("schedule_visualization.pdf"));
        if let Some(parent) = pdf_filename.parent() {
            std::fs::create_dir_all(parent).map_err(|e| crate::report::os_error(e, parent))?;
        }

        let fonts = Fonts::load();
        let pages = [
            self.chart_page(&fonts, self.odd_schedule, "Odd Week Schedule")?,
            self.chart_page(&fonts, self.even_schedule, "Even Week Schedule")?,
            stats_page(&fonts, &self.weekly_stats(self.odd_schedule)?, &self.weekly_stats(self.even_schedule)?),
        ];
        pdf::write_pdf(&pdf_filename, "Schedule Visualization", &pages)?;

        println!("Schedule visualization saved as '{}'", pdf_filename.display());
        println!("\nSchedule visualization complete!");
        println!("Generated file:");
        println!("- schedule_visualization.pdf (on Desktop)");
        println!("  - Page 1: Odd Week Schedule");
        println!("  - Page 2: Even Week Schedule");
        println!("  - Page 3: Statistics");
        Ok(pdf_filename)
    }

    /// One week as a time grid: days across, 06:00–24:00 down.
    fn chart_page<'f>(&self, fonts: &'f Fonts, schedule: &Dict, title: &str) -> anyhow::Result<Canvas<'f>> {
        // Collect the blocks first: the legend size decides the chart height.
        let mut blocks = Vec::new();
        let mut used_activities = BTreeSet::new();
        for (day_index, day) in DAYS_ORDER.iter().enumerate() {
            for (time, activity) in &day_schedule(schedule, day) {
                let name = extract_activity_name(activity);
                used_activities.insert(name.clone());
                let start = parse_block_time(time)?;
                let duration = self.activity_duration(&name)?;
                blocks.push((day_index, start, duration, name));
            }
        }
        let legend: Vec<(String, &str)> =
            used_activities.iter().map(|name| (display_name(name), activity_color(name))).collect();

        let mut canvas = Canvas::new(fonts, PAGE);
        let (w, h) = (canvas.width, canvas.height);
        let legend_rows = legend.len().div_ceil(legend.len().clamp(1, 6));
        let plot = Plot { left: 64.0, top: 72.0, right: w - 24.0, bottom: h - 82.0 - legend_rows as f32 * 14.0 };

        // Alternating day stripes, hour grid and the left axis line.
        for day in (0..DAYS_ORDER.len()).step_by(2) {
            let (x0, x1) = (plot.x(day as f64 - 0.5), plot.x(day as f64 + 0.5));
            canvas.rect(x0, plot.top, x1 - x0, plot.bottom - plot.top, STRIPE, 1.0);
        }
        let hour_label = TextStyle::new(9.0).color(MUTED).mono().anchor(Anchor::End).middle();
        for hour in FIRST_HOUR as u32..=LAST_HOUR as u32 {
            let y = plot.y(f64::from(hour));
            canvas.line(plot.left, y, plot.right, y, Stroke::new(RULE, 0.8).opacity(0.4).dashed("0.8,1.3"));
            canvas.text(plot.left - 8.0, y, &format!("{hour:02}:00"), &hour_label);
        }
        canvas.line(plot.left, plot.top, plot.left, plot.bottom, Stroke::new(RULE, 1.0));

        for (day_index, start, duration, name) in &blocks {
            draw_block(&mut canvas, &plot, *day_index, *start, *duration, name);
        }

        let day_label = TextStyle::new(11.0).bold().color("#57606f").centered().middle();
        for (day_index, day) in DAYS_ORDER.iter().enumerate() {
            let label: String = day.to_uppercase().chars().take(3).collect();
            canvas.text(plot.x(day_index as f64), plot.bottom + 18.0, &label, &day_label);
        }
        let title_style = TextStyle::new(20.0).weight(HEAVY).color(INK).middle();
        canvas.text(plot.left, plot.top - 32.0, &title.to_uppercase(), &title_style);
        draw_legend(&mut canvas, &legend, (plot.left + plot.right) / 2.0, plot.bottom + 44.0);
        Ok(canvas)
    }
}

/// The chart area of a schedule page: day `d` is centered at `x(d)`.
struct Plot {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Plot {
    fn column_width(&self) -> f32 {
        (self.right - self.left) / DAYS_ORDER.len() as f32
    }

    /// x of a day coordinate (-0.5 is the left edge, 6.5 the right edge).
    fn x(&self, day: f64) -> f32 {
        self.left + (day as f32 + 0.5) * self.column_width()
    }

    /// y of a time in hours (06:00 at the top).
    fn y(&self, hour: f64) -> f32 {
        let fraction = (hour - FIRST_HOUR) / (LAST_HOUR - FIRST_HOUR);
        self.top + fraction as f32 * (self.bottom - self.top)
    }
}

/// One activity block with its left accent and (when tall enough) its label.
fn draw_block(canvas: &mut Canvas, plot: &Plot, day_index: usize, start: f64, duration: f64, name: &str) {
    let width = 0.85 * plot.column_width();
    let x = plot.x(day_index as f64) - width / 2.0;
    // Clip to the visible hours.
    let (top, bottom) = (start.max(FIRST_HOUR), (start + duration).min(LAST_HOUR));
    if bottom > top {
        let (y0, y1) = (plot.y(top), plot.y(bottom));
        canvas.rect(x, y0, width, y1 - y0, activity_color(name), 0.9);
        canvas.rect(x, y0, 0.04 * plot.column_width(), y1 - y0, "#000000", 0.1);
    }

    // The label sits in the middle of the visible part (the whole block when
    // it is inside 06:00–24:00) and needs as much room as a full block.
    if duration < 0.3 || bottom - top < 0.3 {
        return;
    }
    let center = (top + bottom) / 2.0;
    let size = if duration > 0.5 { 8.0 } else { 6.0 };
    let mut label = display_name(name);
    if duration < 0.4 {
        label = label.chars().take(3).collect();
    }
    let style = TextStyle::new(size).bold().color(text_color(name)).centered().middle();
    let label = canvas.fonts().fit(&label, width - 10.0, &style);
    canvas.text(plot.x(day_index as f64), plot.y(center), &label, &style);
}

/// Color swatches with activity names, centered under the chart. Entries
/// fill columns first, at most six columns (matplotlib's legend layout).
fn draw_legend(canvas: &mut Canvas, entries: &[(String, &str)], center_x: f32, top: f32) {
    if entries.is_empty() {
        return;
    }
    let style = TextStyle::new(8.0).color(INK).middle();
    let (swatch_width, swatch_height, gap, column_gap, row_height) = (16.0, 6.0, 5.0, 16.0, 14.0);
    let columns = entries.len().min(6);
    let (rows, large_columns) = (entries.len() / columns, entries.len() % columns);

    let mut column_entries = Vec::with_capacity(columns);
    let mut next = 0;
    for column in 0..columns {
        let count = if column < large_columns { rows + 1 } else { rows };
        let labels: Vec<(String, &str)> = entries[next..next + count]
            .iter()
            .map(|(label, color)| (canvas.fonts().fit(label, 150.0, &style), *color))
            .collect();
        let width = labels
            .iter()
            .map(|(label, _)| swatch_width + gap + canvas.fonts().width(label, &style))
            .fold(0.0_f32, f32::max);
        column_entries.push((width, labels));
        next += count;
    }

    let total_width: f32 =
        column_entries.iter().map(|(width, _)| width).sum::<f32>() + column_gap * (columns - 1) as f32;
    let mut x = center_x - total_width / 2.0;
    for (width, labels) in column_entries {
        for (row, (label, color)) in labels.iter().enumerate() {
            let y = top + row as f32 * row_height;
            canvas.rect(x, y - swatch_height / 2.0, swatch_width, swatch_height, color, 1.0);
            canvas.text(x + swatch_width + gap, y, label, &style);
        }
        x += width + column_gap;
    }
}

/// Page 3: odd and even week totals side by side.
fn stats_page<'f>(fonts: &'f Fonts, odd: &WeeklyStats, even: &WeeklyStats) -> Canvas<'f> {
    let mut canvas = Canvas::new(fonts, PAGE);
    let (w, h) = (canvas.width, canvas.height);
    // Axes fractions (origin bottom-left, as in matplotlib) to page points.
    let x = |fraction: f32| w * fraction;
    let y = |fraction: f32| h * (1.0 - fraction);

    canvas.ellipse(x(0.5), y(0.5), w * 0.4, h * 0.4, STRIPE, 1.0);
    let title = TextStyle::new(22.0).bold().color(INK).centered().middle();
    canvas.text(x(0.5), y(0.92), "WEEKLY STATISTICS", &title);
    canvas.line(x(0.3), y(0.89), x(0.7), y(0.89), Stroke::new("#ff6b6b", 2.0));

    let mut column = |x_pos: f32, heading: &str, stats: &WeeklyStats, theme: &str| {
        canvas.text(x_pos, y(0.78), heading, &TextStyle::new(16.0).bold().color(theme).centered());
        let number = TextStyle::new(45.0).weight(HEAVY).color(INK).centered();
        let caption = TextStyle::new(12.0).weight(MEDIUM).color(MUTED).centered();
        let rows = [
            (format!("{:.1}h", stats.work_hours), "Work Time", 0.65, 0.58),
            (stats.pomodoro_count.to_string(), "Pomodoros", 0.45, 0.38),
            (stats.potato_count.to_string(), "Potatoes", 0.25, 0.18),
        ];
        for (value, label, value_y, label_y) in rows {
            canvas.text(x_pos, y(value_y), &value, &number);
            canvas.text(x_pos, y(label_y), label, &caption);
        }
    };
    column(x(0.25), "ODD WEEK", odd, "#ff6b6b");
    column(x(0.75), "EVEN WEEK", even, "#54a0ff");

    canvas.line(x(0.5), y(0.15), x(0.5), y(0.8), Stroke::new(RULE, 1.0).dashed("4,3"));
    canvas
}

/// Render the schedule visualization PDF to `output_path` (default
/// `~/Desktop/schedule_visualization.pdf`) and return the path written.
pub fn visualize(
    config: &ScheduleConfig,
    odd_schedule: &Dict,
    even_schedule: &Dict,
    output_path: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    ScheduleVisualizer::new(config, odd_schedule, even_schedule).visualize(output_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TEMPORARY: render the fixture schedules for visual inspection.
    #[test]
    #[ignore]
    fn render_visualization_for_inspection() {
        let Ok(out) = std::env::var("RMD_SAMPLE_OUT") else {
            return;
        };
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/config/user_config_0");
        let config = ScheduleConfig::load(&fixture.join("settings.toml")).unwrap();
        let weekly =
            crate::config::WeeklySchedule::load(&fixture.join("odd_weeks.toml"), &fixture.join("even_weeks.toml"))
                .unwrap();
        let started = std::time::Instant::now();
        visualize(
            &config,
            &weekly.odd_data,
            &weekly.even_data,
            Some(&Path::new(&out).join("schedule_visualization.pdf")),
        )
        .unwrap();
        eprintln!("visualization: {:?}", started.elapsed());
        let template = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/user_config_0");
        let config = ScheduleConfig::load(&template.join("settings_template.toml")).unwrap();
        let week = crate::config::load_toml_file(&template.join("week_schedule_template.toml")).unwrap();
        let mut even = week.clone();
        if let Some(Value::Object(monday)) = even.get_mut("monday") {
            let mut custom = Dict::new();
            custom.insert("block".into(), Value::from("potato"));
            custom.insert("title".into(), Value::from("论文写作 deep_work session with a very long title"));
            monday.insert("14:00".into(), Value::Object(custom));
            monday.insert("05:30".into(), Value::from("exercise"));
            monday.insert("12:00".into(), Value::from("lunch"));
            monday.insert("16:30".into(), Value::from("meeting"));
        }
        visualize(&config, &week, &even, Some(&Path::new(&out).join("template_visualization.pdf"))).unwrap();
    }
}
