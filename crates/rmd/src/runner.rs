//! The reminder loop: time blocks, urgent tasks, deadlines, habits, and
//! automatic reports.
//!
//! [`ScheduleRunner::tick`] makes every decision for one check of the clock
//! (the service calls it every 20 seconds); everything it does to the outside
//! world (alarms, popups, prompts, reports, log lines, background threads)
//! goes through the [`Effects`] trait, so the timing logic can be tested with
//! `RMD_NOW` and no GUI.

use std::collections::{BTreeSet, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use indexmap::IndexMap;
use serde_json::Value;

use crate::commands::deadlines::refresh_deadlines;
use crate::config::{ScheduleConfig, TomlLoadError, WeeklySchedule};
use crate::i18n::{t, tf};
use crate::py::{self, Dict};
use crate::synced_schedule::apply_synced_schedule;
use crate::time_utils::week_parity_of;
use crate::{clock, data, paths, platform, popups, report, time_utils};

/// Time between two checks of the clock.
pub const POLL_INTERVAL: Duration = Duration::from_secs(20);

// =============================================================================
// RUNTIME LOGGING
// =============================================================================

/// Write a timestamped runtime event line to stdout for the service log.
pub fn log_runtime_event(message: &str) {
    let timestamp = clock::now().format("%Y-%m-%d %H:%M:%S");
    let mut stdout = std::io::stdout().lock();
    // A closed stdout must not take the service down.
    let _ = writeln!(stdout, "[{timestamp}] {message}");
    let _ = stdout.flush();
}

// =============================================================================
// SIDE EFFECTS
// =============================================================================

/// Everything the reminder loop does to the outside world.
///
/// [`SystemEffects`] is the real implementation; tests substitute one that
/// records calls and runs background jobs inline.
pub trait Effects: Send + Sync {
    /// Run `job` in the background (a detached thread in the service).
    fn spawn(&self, job: Box<dyn FnOnce() + Send>);
    /// Ring the repeating alarm: sound plus dialog until dismissed.
    fn alarm(&self, title: &str, message: &str, sound_file: &str, alarm_interval: f64, max_alarm_duration: f64);
    /// Show the daily summary popup.
    fn daily_summary_popup(&self);
    /// Show the habit tracking popup.
    fn habit_tracking_popup(&self);
    /// Ask a Yes/No/Stop question (`None` for Stop).
    fn ask_yes_no(&self, question: &str, title: &str) -> Option<bool>;
    /// Generate the weekly/monthly reports that are due; `{kind: pdf path}`.
    fn auto_generate_reports(&self, settings_path: &Path) -> anyhow::Result<IndexMap<String, PathBuf>>;
    /// Write a runtime log line.
    fn log(&self, message: &str);
}

/// Native dialogs, sounds, report generation and detached threads.
pub struct SystemEffects;

impl Effects for SystemEffects {
    fn spawn(&self, job: Box<dyn FnOnce() + Send>) {
        if let Err(error) = std::thread::Builder::new().spawn(job) {
            log_runtime_event(&format!("Warning: Could not start background thread: {error}"));
        }
    }

    fn alarm(&self, title: &str, message: &str, sound_file: &str, alarm_interval: f64, max_alarm_duration: f64) {
        time_utils::alarm(title, message, sound_file, alarm_interval, max_alarm_duration);
    }

    fn daily_summary_popup(&self) {
        popups::show_daily_summary_popup();
    }

    fn habit_tracking_popup(&self) {
        popups::show_habit_tracking_popup(None);
    }

    fn ask_yes_no(&self, question: &str, title: &str) -> Option<bool> {
        platform::ask_yes_no(question, title)
    }

    fn auto_generate_reports(&self, settings_path: &Path) -> anyhow::Result<IndexMap<String, PathBuf>> {
        report::auto_generate_reports(Some(settings_path), None)
    }

    fn log(&self, message: &str) {
        log_runtime_event(message);
    }
}

// =============================================================================
// AUTO REPORT GENERATION
// =============================================================================

/// Whether an error (or any of its causes) is a missing file.
fn is_file_not_found(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(cause.downcast_ref::<TomlLoadError>(), Some(TomlLoadError::NotFound(_)))
            || cause.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    })
}

/// Generate the scheduled reports that do not exist yet (called at the
/// configured weekly/monthly review times) and log the outcome.
pub fn try_auto_generate_reports(effects: &dyn Effects, settings_path: &Path) {
    match effects.auto_generate_reports(settings_path) {
        Ok(generated) => {
            for (span, pdf_path) in &generated {
                effects.log(&format!("Generated {span} report at {}", pdf_path.display()));
            }
        }
        Err(error) if is_file_not_found(&error) => {
            effects.log(&format!("Auto reports skipped: settings.toml not found at {}", settings_path.display()));
        }
        Err(error) => effects.log(&format!("Auto report generation skipped: {error}")),
    }
}

// =============================================================================
// URGENT TASKS & DEADLINES
// =============================================================================

/// A deadline due within three days (or overdue).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UrgentDeadline {
    pub event: String,
    /// The deadline exactly as stored (`YYYY-MM-DD`).
    pub deadline: String,
    /// Negative when overdue.
    pub days_left: i64,
}

/// Releases the urgent-task prompt flag when the prompt ends, even on panic.
struct PromptGuard<'a>(&'a AtomicBool);

impl Drop for PromptGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Ask about each urgent task unless a prompt is already open.
fn prompt_urgent_tasks_once(effects: &dyn Effects, active: &AtomicBool, is_last_time: bool) {
    if active.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return;
    }
    let _guard = PromptGuard(active);
    if let Err(error) = prompt_urgent_tasks_unguarded(effects, is_last_time) {
        effects.log(&format!("Urgent task prompt skipped: {error}"));
    }
}

/// Ask about high-priority (> 7) and procrastinated tasks one by one: Yes
/// completes the task, No at the day's last check marks it procrastinated,
/// Stop ends the prompt.
fn prompt_urgent_tasks_unguarded(effects: &dyn Effects, is_last_time: bool) -> std::io::Result<()> {
    let mut tasks = data::load_tasks();
    let today = clock::today();
    let procrastinate_records = data::load_procrastinate_records();
    let mut procrastinate_list: BTreeSet<String> = procrastinate_records.keys().cloned().collect();

    let mut urgent_tasks: Vec<Value> = Vec::new();
    for task in &tasks {
        if !task.is_object() || ScheduleRunner::is_task_postponed(task, Some(today)) {
            continue;
        }
        let is_urgent = ScheduleRunner::task_priority(task) > 7;
        let is_overdue =
            task.get("description").and_then(Value::as_str).is_some_and(|d| procrastinate_list.contains(d));
        if is_urgent || is_overdue {
            urgent_tasks.push(task.clone());
        }
    }

    if urgent_tasks.is_empty() {
        effects.log("Urgent task prompt found no unfinished high-priority or overdue tasks");
        return Ok(());
    }

    effects.log(&format!("Urgent task prompt opened for {} task(s)", urgent_tasks.len()));
    let sort_key = |task: &Value| {
        (-ScheduleRunner::task_priority(task), task.get("description").map(py::py_str).unwrap_or_default())
    };
    urgent_tasks.sort_by_cached_key(sort_key);

    let mut tasks_changed = false;
    let mut procrastinate_changed = false;
    let total_tasks = urgent_tasks.len();
    let today = clock::today();

    for (index, task) in urgent_tasks.iter().enumerate() {
        let mut description =
            task.get("description").map(py::py_str).unwrap_or_else(|| t("Unknown Task")).trim().to_string();
        if description.is_empty() {
            description = t("Unknown Task");
        }
        let priority = ScheduleRunner::task_priority(task);
        let since =
            procrastinate_records.get(&description).and_then(|record| record.get("since")).and_then(Value::as_str);
        let age_days = data::get_procrastinate_age_days(since, Some(today));
        let age_label = ScheduleRunner::format_procrastination_age(age_days);

        let mut question_lines = vec![
            t("Did you complete this task?"),
            String::new(),
            description.clone(),
            tf("Priority: {priority}", &[("priority", &priority)]),
        ];
        question_lines.extend(age_label);
        let question = question_lines.join("\n");
        let title =
            tf("Urgent Task ({index}/{total_tasks})", &[("index", &(index + 1)), ("total_tasks", &total_tasks)]);

        match effects.ask_yes_no(&question, &title) {
            None => break,
            Some(true) => {
                if let Some(position) = tasks.iter().position(|candidate| candidate == task) {
                    tasks.remove(position);
                    tasks_changed = true;
                    if let Err(error) = data::log_task_action("deleted", task, None) {
                        effects.log(&format!("Warning: Could not log task deletion: {error}"));
                    }
                }
                if procrastinate_list.remove(&description) {
                    procrastinate_changed = true;
                }
            }
            Some(false) => {
                if is_last_time && procrastinate_list.insert(description) {
                    procrastinate_changed = true;
                }
            }
        }
    }

    if tasks_changed {
        data::save_tasks(&tasks)?;
    }
    if procrastinate_changed {
        data::save_procrastinate_list(&procrastinate_list, Some(today))?;
    }
    Ok(())
}

// =============================================================================
// SCHEDULE RUNNER
// =============================================================================

/// The reminder engine: watches the clock and fires time blocks, time points,
/// the daily summary, the habit prompt, urgent task/deadline reminders and
/// automatic reports.
pub struct ScheduleRunner {
    pub config: ScheduleConfig,
    pub weekly_schedule: WeeklySchedule,
    /// Events already handled today: schedule times plus keys such as
    /// `daily_summary_22:00`, `urgent_tasks_10:00` or `habit_prompt_2026-04-08`.
    pub notified_today: HashSet<String>,
    /// End alarms waiting for their time: `{end_time: message}`.
    pub pending_end_alarms: IndexMap<String, String>,
    effects: Arc<dyn Effects>,
    urgent_task_prompt_active: Arc<AtomicBool>,
}

impl ScheduleRunner {
    /// A runner with the real side effects.
    pub fn new(config: ScheduleConfig, weekly_schedule: WeeklySchedule) -> Self {
        Self::with_effects(config, weekly_schedule, Arc::new(SystemEffects))
    }

    /// A runner with custom side effects (tests).
    pub fn with_effects(config: ScheduleConfig, weekly_schedule: WeeklySchedule, effects: Arc<dyn Effects>) -> Self {
        Self {
            config,
            weekly_schedule,
            notified_today: HashSet::new(),
            pending_end_alarms: IndexMap::new(),
            effects,
            urgent_task_prompt_active: Arc::new(AtomicBool::new(false)),
        }
    }

    fn log(&self, message: &str) {
        self.effects.log(message);
    }

    // =========================================================================
    // ALARM HANDLING
    // =========================================================================

    /// Ring an alarm in the background. `sound` overrides the configured
    /// sound; on macOS a bare name such as `"Glass"` means the system sound.
    pub fn trigger_alarm(&self, title: &str, message: &str, sound: Option<&str>) {
        let sound = sound.filter(|s| !s.is_empty());
        let mut sound_file = sound.map_or_else(|| self.config.sound_file(), str::to_string);
        if platform::get_platform() == "macos"
            && let Some(name) = sound
            && !name.contains('/')
            && !name.ends_with(".aiff")
        {
            sound_file = format!("/System/Library/Sounds/{name}.aiff");
        }
        let alarm_interval = self.config.alarm_interval();
        let max_alarm_duration = self.config.max_alarm_duration();
        let (title, message) = (title.to_string(), message.to_string());
        let effects = Arc::clone(&self.effects);
        self.effects.spawn(Box::new(move || {
            effects.alarm(&title, &message, &sound_file, alarm_interval, max_alarm_duration);
        }));
    }

    // =========================================================================
    // EVENT HANDLING
    // =========================================================================

    /// Handle a scheduled event: a time block name, a `time_points` key, a
    /// plain message, or `{block, title}`.
    pub fn handle_event(&mut self, time_str: &str, event: &Value) {
        match event {
            Value::String(name) => {
                if self.config.time_blocks.contains_key(name) {
                    self.start_time_block(time_str, name, name);
                } else {
                    let message = self.config.time_points.get(name).map_or_else(|| name.clone(), py::py_str);
                    self.log(&format!("Time point triggered at {time_str}: {message}"));
                    self.trigger_alarm(&t("Reminder"), &message, None);
                    self.notified_today.insert(time_str.to_string());
                }
            }
            Value::Object(map) if map.contains_key("block") => {
                let block = &map["block"];
                let title = py::py_str(map.get("title").unwrap_or(block));
                match block {
                    Value::String(block_type) if self.config.time_blocks.contains_key(block_type) => {
                        self.start_time_block(time_str, block_type, &title);
                    }
                    _ => self.log(&format!("Warning: Unknown block type '{}' at {time_str}", py::py_str(block))),
                }
            }
            _ => {}
        }
    }

    /// Announce a time block and queue its end alarm.
    pub fn start_time_block(&mut self, start_time: &str, block_type: &str, title: &str) {
        let duration = self.config.time_blocks.get(block_type).cloned().unwrap_or(Value::Null);
        let Some(end_time_str) = end_time(start_time, &duration) else {
            self.log(&format!(
                "Warning: Invalid duration '{}' for block type '{block_type}' at {start_time}",
                py::py_str(&duration)
            ));
            return;
        };
        let duration = py::py_str(&duration);

        let start_message = tf("{title} ⏱️ ({duration}min)", &[("title", &title), ("duration", &duration)]);
        self.trigger_alarm(&t("Start"), &start_message, None);
        self.notified_today.insert(start_time.to_string());

        let end_message = tf("{title} finished! Take a break 🎉", &[("title", &title)]);
        self.pending_end_alarms.insert(end_time_str.clone(), end_message);
        self.log(&format!("Time block started at {start_time}: {title} ({duration}min), ends at {end_time_str}"));
    }

    // =========================================================================
    // URGENT TASKS & DEADLINES
    // =========================================================================

    /// A task's priority as an integer (0 when missing or invalid).
    pub fn task_priority(task: &Value) -> i64 {
        task.get("priority").and_then(py::py_int).unwrap_or(0)
    }

    /// Whether the task's reminders are postponed (`alarm_from`) past `today`.
    pub fn is_task_postponed(task: &Value, today: Option<NaiveDate>) -> bool {
        let Some(alarm_from) = task.get("alarm_from").filter(|value| py::truthy(value)) else {
            return false;
        };
        let Some(alarm_from_date) = alarm_from.as_str().and_then(|s| py::strptime_date(s, "%Y-%m-%d")) else {
            return false;
        };
        today.unwrap_or_else(clock::today) < alarm_from_date
    }

    /// Pending tasks with priority above 7 whose reminders are not postponed.
    pub fn get_unfinished_urgent_tasks(&self) -> Vec<Value> {
        let today = clock::today();
        data::load_tasks()
            .into_iter()
            .filter(|task| {
                task.is_object() && Self::task_priority(task) > 7 && !Self::is_task_postponed(task, Some(today))
            })
            .collect()
    }

    /// "Deferred today" / "Procrastinated for N days" (`None` when unknown).
    pub fn format_procrastination_age(age_days: Option<i64>) -> Option<String> {
        Some(match age_days? {
            0 => t("Deferred today"),
            1 => t("Procrastinated for 1 day"),
            days => tf("Procrastinated for {age_days} days", &[("age_days", &days)]),
        })
    }

    /// Ask about each urgent task now, on this thread (no-op while another
    /// urgent-task prompt is open).
    pub fn prompt_urgent_tasks(&self, is_last_time: bool) {
        prompt_urgent_tasks_once(self.effects.as_ref(), &self.urgent_task_prompt_active, is_last_time);
    }

    /// Deadlines due within three days or overdue, most urgent first. Long
    /// overdue one-off deadlines are removed from `ddl.json` and recurring ones
    /// rolled forward on the way.
    pub fn get_urgent_deadlines(&self) -> Vec<UrgentDeadline> {
        let ddl_path = paths::runtime().ddl_path;
        let Some(Value::Array(deadlines)) =
            std::fs::read_to_string(&ddl_path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok())
        else {
            return Vec::new();
        };

        let today = clock::today();
        let refreshed = refresh_deadlines(&deadlines, Some(today));
        if refreshed.changed
            && let Err(error) = py::write_json_pretty(&ddl_path, &refreshed.kept)
        {
            self.log(&format!("Expired deadline cleanup skipped: {error}"));
        }

        let mut urgent: Vec<UrgentDeadline> = Vec::new();
        for ddl in &refreshed.kept {
            let Some(item) = ddl.as_object() else {
                continue;
            };
            let (Some(event), Some(deadline)) = (item.get("event"), item.get("deadline")) else {
                continue;
            };
            if !py::truthy(event) || !py::truthy(deadline) {
                continue;
            }
            let Some(deadline_str) = deadline.as_str() else {
                continue;
            };
            let Some(deadline_date) = py::strptime_date(deadline_str, "%Y-%m-%d") else {
                continue;
            };
            let days_left = (deadline_date - today).num_days();
            if days_left <= 3 {
                urgent.push(UrgentDeadline { event: py::py_str(event), deadline: deadline_str.to_string(), days_left });
            }
        }
        // By days left, then deadline, then event name.
        urgent.sort_by(|a, b| (a.days_left, &a.deadline, &a.event).cmp(&(b.days_left, &b.deadline, &b.event)));
        urgent
    }

    /// Start the urgent-task prompt in the background unless one is open.
    pub fn check_urgent_tasks(&self, is_last_time: bool) {
        if self.urgent_task_prompt_active.load(Ordering::SeqCst) {
            return;
        }
        self.log("Urgent task check triggered");
        let effects = Arc::clone(&self.effects);
        let active = Arc::clone(&self.urgent_task_prompt_active);
        self.effects.spawn(Box::new(move || prompt_urgent_tasks_once(effects.as_ref(), &active, is_last_time)));
    }

    /// Ring the deadline alarm when deadlines are close.
    pub fn check_urgent_deadlines(&self) {
        let urgent_deadlines = self.get_urgent_deadlines();
        if urgent_deadlines.is_empty() {
            self.log("Urgent deadline check found no approaching deadlines");
            return;
        }

        let lines: Vec<String> = urgent_deadlines
            .iter()
            .map(|ddl| {
                let (event, deadline_str) = (&ddl.event, &ddl.deadline);
                if ddl.days_left < 0 {
                    tf(
                        "⚠️ {event} - {deadline_str} (Overdue {days_left} days)",
                        &[("event", event), ("deadline_str", deadline_str), ("days_left", &-ddl.days_left)],
                    )
                } else if ddl.days_left == 0 {
                    tf("🔴 {event} - {deadline_str} (Due today)", &[("event", event), ("deadline_str", deadline_str)])
                } else {
                    tf(
                        "🚨 {event} - {deadline_str} ({days_left} days left)",
                        &[("event", event), ("deadline_str", deadline_str), ("days_left", &ddl.days_left)],
                    )
                }
            })
            .collect();

        let message = t("📅 Urgent Deadline Reminder\n\n") + &lines.join("\n");
        self.log(&format!("Urgent deadline reminder triggered for {} deadline(s)", urgent_deadlines.len()));
        self.trigger_alarm(&t("Urgent Deadlines"), &message, Some("Glass"));
    }

    // =========================================================================
    // MAIN LOOP
    // =========================================================================

    /// Today's merged schedule (`common` + weekday), empty on a skip day.
    fn schedule_for(&self, today: NaiveDate) -> Dict {
        if self.config.should_skip_date(today) {
            return Dict::new();
        }
        self.weekly_schedule.schedule_for_day(week_parity_of(today), py::weekday_name(today))
    }

    fn spawn_auto_reports(&self) {
        let effects = Arc::clone(&self.effects);
        self.effects.spawn(Box::new(move || {
            try_auto_generate_reports(effects.as_ref(), &paths::runtime().settings_path);
        }));
    }

    /// One check of the clock at `now`:
    ///
    /// 1. the daily summary and habit prompt at their times (not on skip days);
    /// 2. urgent task and deadline checks at their times (not on skip days);
    /// 3. weekly/monthly automatic reports;
    /// 4. in j mode, start events and end alarms from today's schedule
    ///    (with the synced plan applied);
    /// 5. at 00:00, forget the day's notifications and pending end alarms.
    ///
    /// Steps 4 and 5 only run when today's schedule is not empty.
    pub fn tick(&mut self, now: NaiveDateTime) {
        let now_str = now.format("%H:%M").to_string();
        let today = now.date();
        let current_mode = data::load_mode();
        let mut today_schedule = self.schedule_for(today);
        if !today_schedule.is_empty() {
            today_schedule = apply_synced_schedule(
                &today_schedule,
                Some(today),
                Some(week_parity_of(today)),
                Some(py::weekday_name(today)),
                None,
            );
        }
        let skip_today = self.config.should_skip_date(today);

        // ---------------------------------------------------------------------
        // Daily Summary Time
        // ---------------------------------------------------------------------
        let daily_summary_key = format!("daily_summary_{now_str}");
        if now_str == self.config.daily_summary_time()
            && !self.notified_today.contains(&daily_summary_key)
            && !skip_today
        {
            self.log("Daily summary popup triggered");
            let effects = Arc::clone(&self.effects);
            self.effects.spawn(Box::new(move || effects.daily_summary_popup()));
            self.notified_today.insert(daily_summary_key);
        }

        // ---------------------------------------------------------------------
        // Habit Prompt Time
        // ---------------------------------------------------------------------
        let habit_prompt_time = self.config.habit_prompt_time();
        let habit_prompt_key = format!("habit_prompt_{}", today.format("%Y-%m-%d"));
        if !habit_prompt_time.is_empty()
            && now_str == habit_prompt_time
            && !self.notified_today.contains(&habit_prompt_key)
            && !skip_today
        {
            self.log("Habit tracking popup triggered");
            let effects = Arc::clone(&self.effects);
            self.effects.spawn(Box::new(move || effects.habit_tracking_popup()));
            self.notified_today.insert(habit_prompt_key);
        }

        // ---------------------------------------------------------------------
        // Urgent Tasks & Deadlines
        // ---------------------------------------------------------------------
        if !skip_today {
            let daily_urgent_times = self.config.daily_urgent_times();
            let last_urgent_time = daily_urgent_times.iter().max().cloned();
            for urgent_time in &daily_urgent_times {
                let urgent_tasks_key = format!("urgent_tasks_{urgent_time}");
                if now_str == *urgent_time && !self.notified_today.contains(&urgent_tasks_key) {
                    self.log(&format!("Scheduled urgent task check reached at {urgent_time}"));
                    let is_last_time = last_urgent_time.as_ref() == Some(urgent_time);
                    self.check_urgent_tasks(is_last_time);
                    self.notified_today.insert(urgent_tasks_key);
                }
            }

            for urgent_time in self.config.ddl_urgent_times() {
                let urgent_ddls_key = format!("urgent_ddls_{urgent_time}");
                if now_str == urgent_time && !self.notified_today.contains(&urgent_ddls_key) {
                    self.log(&format!("Scheduled urgent deadline check reached at {urgent_time}"));
                    self.check_urgent_deadlines();
                    self.notified_today.insert(urgent_ddls_key);
                }
            }
        }

        // ---------------------------------------------------------------------
        // Weekly Review ("sunday 20:00")
        // ---------------------------------------------------------------------
        let weekly_review_setting = self.config.weekly_review_time();
        if let [day_of_week, review_time] = weekly_review_setting.split_whitespace().collect::<Vec<_>>()[..]
            && py::weekday_name(today) == day_of_week.to_lowercase()
            && now_str == review_time
        {
            let review_key = format!("weekly_review_{}", today.format("%Y-%m-%d"));
            if !self.notified_today.contains(&review_key) {
                self.spawn_auto_reports();
                self.notified_today.insert(review_key);
            }
        }

        // ---------------------------------------------------------------------
        // Monthly Review ("1 20:00")
        // ---------------------------------------------------------------------
        let monthly_review_setting = self.config.monthly_review_time();
        if let [day_of_month, review_time] = monthly_review_setting.split_whitespace().collect::<Vec<_>>()[..]
            && py::parse_int_str(day_of_month) == Some(i64::from(now.day()))
            && now_str == review_time
        {
            let review_key = format!("monthly_review_{}", today.format("%Y-%m"));
            if !self.notified_today.contains(&review_key) {
                self.spawn_auto_reports();
                self.notified_today.insert(review_key);
            }
        }

        // ---------------------------------------------------------------------
        // Nothing scheduled today (or a skip day)
        // ---------------------------------------------------------------------
        if today_schedule.is_empty() {
            return;
        }

        // ---------------------------------------------------------------------
        // Start Events
        // ---------------------------------------------------------------------
        if current_mode == "j"
            && !self.notified_today.contains(&now_str)
            && let Some(event) = today_schedule.get(&now_str)
        {
            self.handle_event(&now_str, event);
        }

        // ---------------------------------------------------------------------
        // End Alarms
        // ---------------------------------------------------------------------
        if current_mode == "j"
            && !self.notified_today.contains(&now_str)
            && let Some(message) = self.pending_end_alarms.get(&now_str).cloned()
        {
            self.log(&format!("End alarm triggered at {now_str}: {message}"));
            self.trigger_alarm(&t("End Reminder"), &message, None);
            self.notified_today.insert(now_str.clone());
            self.pending_end_alarms.shift_remove(&now_str);
        }

        // ---------------------------------------------------------------------
        // Midnight Reset
        // ---------------------------------------------------------------------
        if now_str == "00:00" {
            self.log("Midnight reset triggered");
            self.notified_today.clear();
            self.pending_end_alarms.clear();
        }
    }

    /// Check the clock every 20 seconds, forever.
    pub fn run(&mut self) -> ! {
        loop {
            self.tick(clock::now());
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

/// `HH:MM` after `duration` minutes (an integer or fractional number of
/// minutes, like Python's `timedelta(minutes=...)`), wrapping at midnight.
fn end_time(start_time: &str, duration: &Value) -> Option<String> {
    let start = py::parse_hhmm(start_time)?;
    let micros: i64 = match duration {
        Value::Bool(b) => i64::from(*b) * 60_000_000,
        Value::Number(n) => match n.as_i64() {
            Some(minutes) => minutes.checked_mul(60_000_000)?,
            None => {
                let micros = (n.as_f64()? * 60_000_000.0).round();
                if !micros.is_finite() || micros.abs() >= 9.0e18 {
                    return None;
                }
                micros as i64
            }
        },
        _ => return None,
    };
    let (end, _) = start.overflowing_add_signed(chrono::TimeDelta::microseconds(micros));
    Some(py::hhmm(end))
}

// =============================================================================
// ENTRY POINT
// =============================================================================

/// `str()` of a raw `[tasks]` value (the first key present), for the start-up log.
fn raw_task_setting(config: &ScheduleConfig, keys: &[&str], default: &str) -> String {
    keys.iter().find_map(|key| config.tasks.get(*key)).map_or_else(|| default.to_string(), py::py_str)
}

/// Load the active configuration and run the reminder loop forever. Returns 1
/// when the settings or schedules cannot be loaded.
pub fn main() -> i32 {
    let runtime_paths = paths::runtime();
    let loaded = ScheduleConfig::load(&runtime_paths.settings_path).and_then(|config| {
        WeeklySchedule::load(&runtime_paths.odd_path, &runtime_paths.even_path).map(|weekly| (config, weekly))
    });
    let (config, weekly) = match loaded {
        Ok(loaded) => loaded,
        Err(error) => {
            let timestamp = clock::now().format("%Y-%m-%d %H:%M:%S");
            eprintln!("[{timestamp}] Runner failed to start: {error}");
            return 1;
        }
    };
    log_runtime_event(&format!(
        "Runner started with settings={}, daily_urgent={}, ddl_urgent={}, daily_summary={}, habit_prompt={}",
        runtime_paths.settings_path.display(),
        raw_task_setting(&config, &["daily_urgency", "daily_urgent"], "[]"),
        raw_task_setting(&config, &["ddl_urgency", "ddl_urgent"], "[]"),
        config.daily_summary_time(),
        config.habit_prompt_time(),
    ));
    ScheduleRunner::new(config, weekly).run()
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;
    use crate::test_support::TestEnv;

    /// Records every side effect and runs background jobs inline.
    #[derive(Default)]
    struct Recorder {
        calls: Mutex<Vec<String>>,
        answers: Mutex<VecDeque<Option<bool>>>,
        questions: Mutex<Vec<(String, String)>>,
        report_result: Mutex<Option<anyhow::Result<IndexMap<String, PathBuf>>>>,
    }

    impl Recorder {
        fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        fn answering(answers: &[Option<bool>]) -> Arc<Self> {
            let recorder = Self::default();
            recorder.answers.lock().unwrap().extend(answers.iter().copied());
            Arc::new(recorder)
        }

        fn push(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn take(&self) -> Vec<String> {
            std::mem::take(&mut *self.calls.lock().unwrap())
        }

        fn alarms(&self) -> Vec<String> {
            self.calls().into_iter().filter(|c| c.starts_with("alarm: ")).collect()
        }

        fn logs(&self) -> Vec<String> {
            self.calls().into_iter().filter_map(|c| c.strip_prefix("log: ").map(str::to_string)).collect()
        }

        fn questions(&self) -> Vec<(String, String)> {
            self.questions.lock().unwrap().clone()
        }
    }

    impl Effects for Recorder {
        fn spawn(&self, job: Box<dyn FnOnce() + Send>) {
            job();
        }

        fn alarm(&self, title: &str, message: &str, sound_file: &str, alarm_interval: f64, max_alarm_duration: f64) {
            self.push(format!("alarm: {title} | {message} | {sound_file} | {alarm_interval} | {max_alarm_duration}"));
        }

        fn daily_summary_popup(&self) {
            self.push("daily summary popup".into());
        }

        fn habit_tracking_popup(&self) {
            self.push("habit tracking popup".into());
        }

        fn ask_yes_no(&self, question: &str, title: &str) -> Option<bool> {
            self.questions.lock().unwrap().push((question.to_string(), title.to_string()));
            self.answers.lock().unwrap().pop_front().unwrap_or(None)
        }

        fn auto_generate_reports(&self, settings_path: &Path) -> anyhow::Result<IndexMap<String, PathBuf>> {
            self.push(format!("reports: {}", settings_path.display()));
            self.report_result.lock().unwrap().take().unwrap_or_else(|| Ok(IndexMap::new()))
        }

        fn log(&self, message: &str) {
            self.push(format!("log: {message}"));
        }
    }

    const SETTINGS: &str = r#"
[settings]
sound_file = "/mock/sound.aiff"
alarm_interval = 5
max_alarm_duration = 300

[time_blocks]
pomodoro = 25
break = 5

[time_points]
go_to_bed = "上床睡觉 😴 该休息了！"
summary = "今天的工作结束 🎉, 总结一下"
"#;

    fn config(toml: &str) -> ScheduleConfig {
        ScheduleConfig::from_dict(&crate::config::parse_toml_str(toml).expect("valid test TOML"))
    }

    /// The same events every day of both week parities.
    fn every_day(schedule: Value) -> WeeklySchedule {
        let data = json!({ "common": schedule }).as_object().unwrap().clone();
        WeeklySchedule { odd_data: data.clone(), even_data: data }
    }

    fn runner(settings: &str, weekly: WeeklySchedule, effects: &Arc<Recorder>) -> ScheduleRunner {
        ScheduleRunner::with_effects(config(settings), weekly, effects.clone())
    }

    fn at(text: &str) -> NaiveDateTime {
        py::parse_naive_datetime(text).expect("valid test datetime")
    }

    /// Poll three times within the minute, like the 20-second loop.
    fn poll_minute(runner: &mut ScheduleRunner, env: &mut TestEnv, minute: &str) {
        for seconds in ["00", "20", "40"] {
            let now = format!("{minute}:{seconds}");
            env.set_now(&now);
            runner.tick(at(&now));
        }
    }

    fn write_json(env: &TestEnv, relative: &str, value: Value) {
        env.write(relative, &py::dumps_pretty(&value));
    }

    fn read_json(env: &TestEnv, relative: &str) -> Value {
        serde_json::from_str(&env.read(relative)).expect("valid JSON file")
    }

    // --- time helpers (test_reminder_logic.py) -------------------------------

    #[test]
    fn time_helpers() {
        let parsed = crate::time_utils::parse_time("09:30").unwrap();
        assert_eq!(crate::time_utils::time_to_str(parsed), "09:30");
        assert_eq!(crate::time_utils::time_to_str(chrono::NaiveTime::from_hms_opt(14, 5, 0).unwrap()), "14:05");
        for (start, minutes, end) in
            [("09:10", 25, "09:35"), ("09:50", 20, "10:10"), ("23:50", 30, "00:20"), ("10:00", 60, "11:00")]
        {
            assert_eq!(crate::time_utils::add_minutes_to_time(start, minutes).as_deref(), Some(end));
            assert_eq!(end_time(start, &json!(minutes)).as_deref(), Some(end));
        }
        assert_eq!(end_time("09:00", &json!(25.5)).as_deref(), Some("09:25"));
        assert_eq!(end_time("09:00", &json!(true)).as_deref(), Some("09:01"));
        assert_eq!(end_time("09:00", &json!("25")), None);
    }

    // --- ScheduleConfig / WeeklySchedule ------------------------------------

    #[test]
    fn skip_days() {
        let sunday = NaiveDate::from_ymd_opt(2026, 4, 12).unwrap();
        let saturday = NaiveDate::from_ymd_opt(2026, 4, 11).unwrap();
        let monday = NaiveDate::from_ymd_opt(2026, 4, 13).unwrap();
        assert!(!config("[settings]\nskip_days = []\n").should_skip_date(sunday));
        assert!(!config("[settings]\n").should_skip_date(sunday));
        assert!(config("[settings]\nskip_days = [\"sunday\"]\n").should_skip_date(sunday));
        assert!(!config("[settings]\nskip_days = [\"sunday\"]\n").should_skip_date(monday));
        let several = config("[settings]\nskip_days = [\"sunday\", \"saturday\", \"friday\"]\n");
        assert!(several.should_skip_date(saturday));
    }

    #[test]
    fn weekly_schedule_for_today() {
        let mut env = TestEnv::new();
        let odd = json!({"monday": {"09:00": "pomodoro"}, "common": {"21:00": "summary_time"}});
        let weekly = WeeklySchedule { odd_data: odd.as_object().unwrap().clone(), even_data: Dict::new() };
        assert_eq!(weekly.get_schedule_for_parity("odd"), odd.as_object().unwrap());
        assert!(weekly.get_schedule_for_parity("even").is_empty());

        // 2026-04-13 is a Monday in ISO week 16 (even); 2026-04-06 is in week 15 (odd).
        let config = config("[settings]\nskip_days = [\"sunday\"]\n");
        env.set_now("2026-04-06T08:00:00");
        assert_eq!(
            Value::Object(weekly.get_today_schedule(&config)),
            json!({"21:00": "summary_time", "09:00": "pomodoro"})
        );
        env.set_now("2026-04-13T08:00:00");
        assert!(weekly.get_today_schedule(&config).is_empty());

        let sunday_weekly = WeeklySchedule {
            odd_data: json!({"sunday": {"10:00": "relax"}}).as_object().unwrap().clone(),
            even_data: Dict::new(),
        };
        env.set_now("2026-04-12T08:00:00");
        assert!(sunday_weekly.get_today_schedule(&config).is_empty());
    }

    // --- event handling ------------------------------------------------------

    #[test]
    fn string_block_event_starts_block_and_queues_end_alarm() {
        let _env = TestEnv::new();
        let effects = Recorder::new();
        let mut runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        runner.handle_event("08:30", &json!("pomodoro"));

        assert_eq!(effects.alarms(), ["alarm: Start | pomodoro ⏱️ (25min) | /mock/sound.aiff | 5 | 300"]);
        assert!(runner.notified_today.contains("08:30"));
        assert_eq!(
            runner.pending_end_alarms.get("08:55").map(String::as_str),
            Some("pomodoro finished! Take a break 🎉")
        );
        assert_eq!(effects.logs(), ["Time block started at 08:30: pomodoro (25min), ends at 08:55"]);
    }

    #[test]
    fn time_point_event_rings_once() {
        let _env = TestEnv::new();
        let effects = Recorder::new();
        let mut runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        runner.handle_event("21:00", &json!("summary"));

        assert_eq!(effects.alarms(), ["alarm: Reminder | 今天的工作结束 🎉, 总结一下 | /mock/sound.aiff | 5 | 300"]);
        assert!(runner.notified_today.contains("21:00"));
        assert!(runner.pending_end_alarms.is_empty());
        assert_eq!(effects.logs(), ["Time point triggered at 21:00: 今天的工作结束 🎉, 总结一下"]);
    }

    #[test]
    fn direct_message_event_rings_once() {
        let _env = TestEnv::new();
        let effects = Recorder::new();
        let mut runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        runner.handle_event("10:00", &json!("该喝水了"));

        assert_eq!(effects.alarms(), ["alarm: Reminder | 该喝水了 | /mock/sound.aiff | 5 | 300"]);
        assert!(runner.notified_today.contains("10:00"));
        assert!(runner.pending_end_alarms.is_empty());
    }

    #[test]
    fn dict_block_event_uses_title() {
        let _env = TestEnv::new();
        let effects = Recorder::new();
        let mut runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        runner.handle_event("09:10", &json!({"block": "pomodoro", "title": "写代码"}));

        assert_eq!(effects.alarms(), ["alarm: Start | 写代码 ⏱️ (25min) | /mock/sound.aiff | 5 | 300"]);
        assert!(runner.notified_today.contains("09:10"));
        assert_eq!(
            runner.pending_end_alarms.get("09:35").map(String::as_str),
            Some("写代码 finished! Take a break 🎉")
        );
    }

    #[test]
    fn unknown_block_type_is_logged_and_ignored() {
        let _env = TestEnv::new();
        let effects = Recorder::new();
        let mut runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        runner.handle_event("10:00", &json!({"block": "unknown_block", "title": "Unknown"}));
        runner.handle_event("10:00", &json!(["not", "an", "event"]));

        assert!(effects.alarms().is_empty());
        assert!(!runner.notified_today.contains("10:00"));
        assert_eq!(effects.logs(), ["Warning: Unknown block type 'unknown_block' at 10:00"]);
    }

    #[test]
    fn chinese_messages() {
        let mut env = TestEnv::new();
        env.set_var("REMINDER_LANG", "zh");
        let effects = Recorder::new();
        let mut runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        runner.handle_event("08:30", &json!("pomodoro"));

        assert_eq!(effects.alarms(), ["alarm: 开始 | pomodoro ⏱️ (25分钟) | /mock/sound.aiff | 5 | 300"]);
        assert_eq!(runner.pending_end_alarms["08:55"], "pomodoro 结束！休息一下 🎉");
    }

    // --- the loop ------------------------------------------------------------

    #[test]
    fn end_alarm_fires_once_at_block_end() {
        let mut env = TestEnv::new();
        let effects = Recorder::new();
        let mut runner =
            runner(SETTINGS, every_day(json!({"09:10": {"block": "pomodoro", "title": "写代码"}})), &effects);

        poll_minute(&mut runner, &mut env, "2026-04-08T09:10");
        assert_eq!(effects.alarms().len(), 1);
        effects.take();

        poll_minute(&mut runner, &mut env, "2026-04-08T09:35");
        assert_eq!(
            effects.alarms(),
            ["alarm: End Reminder | 写代码 finished! Take a break 🎉 | /mock/sound.aiff | 5 | 300"]
        );
        assert_eq!(effects.logs(), ["End alarm triggered at 09:35: 写代码 finished! Take a break 🎉"]);
        assert!(runner.notified_today.contains("09:35"));
        assert!(runner.pending_end_alarms.is_empty());
    }

    #[test]
    fn full_day_flow() {
        let mut env = TestEnv::new();
        let effects = Recorder::new();
        let schedule = json!({
            "09:00": {"block": "pomodoro", "title": "Focus Task A"},
            "10:00": {"block": "pomodoro", "title": "Focus Task B"},
            "11:00": "Lunch time 🍜",
        });
        let mut runner = runner("[time_blocks]\npomodoro = 25\n", every_day(schedule), &effects);

        let mut alarms_by_minute = Vec::new();
        for minute in [
            "2026-04-08T08:59",
            "2026-04-08T09:00",
            "2026-04-08T09:10",
            "2026-04-08T09:25",
            "2026-04-08T11:00",
            "2026-04-09T00:00",
            "2026-04-09T09:00",
        ] {
            poll_minute(&mut runner, &mut env, minute);
            alarms_by_minute.push((minute, effects.alarms()));
            effects.take();
        }

        let sound = crate::config::ScheduleConfig::default().sound_file();
        let alarm = |title: &str, message: &str| format!("alarm: {title} | {message} | {sound} | 5 | 300");
        assert_eq!(
            alarms_by_minute,
            vec![
                ("2026-04-08T08:59", vec![]),
                ("2026-04-08T09:00", vec![alarm("Start", "Focus Task A ⏱️ (25min)")]),
                ("2026-04-08T09:10", vec![]),
                ("2026-04-08T09:25", vec![alarm("End Reminder", "Focus Task A finished! Take a break 🎉")]),
                ("2026-04-08T11:00", vec![alarm("Reminder", "Lunch time 🍜")]),
                ("2026-04-09T00:00", vec![]),
                ("2026-04-09T09:00", vec![alarm("Start", "Focus Task A ⏱️ (25min)")]),
            ]
        );
    }

    #[test]
    fn midnight_reset_clears_the_day() {
        let mut env = TestEnv::new();
        let effects = Recorder::new();
        let mut runner = runner(SETTINGS, every_day(json!({"21:00": "summary"})), &effects);
        runner.notified_today.insert("08:30".into());
        runner.pending_end_alarms.insert("08:55".into(), "pomodoro finished! Take a break 🎉".into());

        poll_minute(&mut runner, &mut env, "2026-04-09T00:00");

        assert!(runner.notified_today.is_empty());
        assert!(runner.pending_end_alarms.is_empty());
        assert_eq!(effects.logs(), ["Midnight reset triggered"; 3]);
    }

    #[test]
    fn midnight_reset_needs_a_schedule_today() {
        // As in the Python runner, the reset happens after the "nothing
        // scheduled today" early exit, so a skip day keeps yesterday's state.
        let mut env = TestEnv::new();
        let effects = Recorder::new();
        let settings = format!("{SETTINGS}\n[tasks]\n");
        let settings = settings.replace("[settings]\n", "[settings]\nskip_days = [\"sunday\"]\n");
        let mut runner = runner(&settings, every_day(json!({"21:00": "summary"})), &effects);
        runner.notified_today.insert("21:00".into());

        poll_minute(&mut runner, &mut env, "2026-04-12T00:00");

        assert!(runner.notified_today.contains("21:00"));
        assert!(effects.logs().is_empty());
    }

    #[test]
    fn events_fire_once_per_minute_and_back_to_back_blocks_skip_the_first_end_alarm() {
        let mut env = TestEnv::new();
        let effects = Recorder::new();
        let schedule = json!({"09:00": "pomodoro", "09:25": "break"});
        let mut runner = runner(SETTINGS, every_day(schedule), &effects);

        poll_minute(&mut runner, &mut env, "2026-04-08T09:00");
        poll_minute(&mut runner, &mut env, "2026-04-08T09:25");
        poll_minute(&mut runner, &mut env, "2026-04-08T09:30");

        let messages: Vec<String> =
            effects.alarms().iter().map(|a| a.split(" | ").nth(1).unwrap().to_string()).collect();
        assert_eq!(messages, ["pomodoro ⏱️ (25min)", "break ⏱️ (5min)", "break finished! Take a break 🎉"]);
        assert!(runner.pending_end_alarms.contains_key("09:25"));
    }

    #[test]
    fn p_mode_skips_schedule_events() {
        let mut env = TestEnv::new();
        env.write("tasks/mode.txt", "p");
        let effects = Recorder::new();
        let settings = format!("{SETTINGS}\n[tasks]\ndaily_summary = \"09:00\"\n");
        let mut runner = runner(&settings, every_day(json!({"09:00": "pomodoro"})), &effects);

        poll_minute(&mut runner, &mut env, "2026-05-25T09:00");

        assert!(!runner.notified_today.contains("09:00"));
        assert!(effects.alarms().is_empty());
        // Popups are not schedule events: they still appear in p mode.
        assert_eq!(effects.calls().iter().filter(|c| *c == "daily summary popup").count(), 1);

        env.write("tasks/mode.txt", "j");
        let mut runner_j = self::runner(SETTINGS, every_day(json!({"09:00": "pomodoro"})), &effects);
        poll_minute(&mut runner_j, &mut env, "2026-05-25T09:00");
        assert!(runner_j.notified_today.contains("09:00"));
        assert_eq!(effects.alarms().len(), 1);
    }

    #[test]
    fn synced_plan_titles_today_s_blocks() {
        let mut env = TestEnv::new();
        env.write(
            "synced_schedule.toml",
            "date = \"2026-04-08\"\nparity = \"odd\"\nweekday = \"wednesday\"\n\n[schedule]\n\"09:00\" = { block = \"pomodoro\", title = \"Write paper\" }\n",
        );
        let effects = Recorder::new();
        let mut runner = runner(SETTINGS, every_day(json!({"09:00": "pomodoro"})), &effects);

        poll_minute(&mut runner, &mut env, "2026-04-08T09:00");
        assert_eq!(effects.alarms(), ["alarm: Start | Write paper ⏱️ (25min) | /mock/sound.aiff | 5 | 300"]);

        // The plan is for one day only.
        poll_minute(&mut runner, &mut env, "2026-04-09T00:00");
        effects.take();
        poll_minute(&mut runner, &mut env, "2026-04-09T09:00");
        assert_eq!(effects.alarms(), ["alarm: Start | pomodoro ⏱️ (25min) | /mock/sound.aiff | 5 | 300"]);
    }

    #[test]
    fn daily_summary_and_habit_prompt_fire_once_and_not_on_skip_days() {
        let mut env = TestEnv::new();
        let effects = Recorder::new();
        let settings =
            "[settings]\nskip_days = [\"sunday\"]\n[tasks]\ndaily_summary = \"22:00\"\nhabit_prompt = \"21:30\"\n";
        let mut runner = runner(settings, WeeklySchedule::default(), &effects);

        poll_minute(&mut runner, &mut env, "2026-04-11T21:30");
        poll_minute(&mut runner, &mut env, "2026-04-11T22:00");
        assert_eq!(
            effects.take(),
            [
                "log: Habit tracking popup triggered",
                "habit tracking popup",
                "log: Daily summary popup triggered",
                "daily summary popup",
            ]
        );
        assert!(runner.notified_today.contains("habit_prompt_2026-04-11"));
        assert!(runner.notified_today.contains("daily_summary_22:00"));

        // Sunday is a skip day.
        runner.notified_today.clear();
        poll_minute(&mut runner, &mut env, "2026-04-12T21:30");
        poll_minute(&mut runner, &mut env, "2026-04-12T22:00");
        assert!(effects.take().is_empty());

        // The habit key is per date, so it fires again the next day even
        // without a midnight reset; the summary key is per time and does not.
        runner.notified_today.insert("habit_prompt_2026-04-11".into());
        runner.notified_today.insert("daily_summary_22:00".into());
        poll_minute(&mut runner, &mut env, "2026-04-13T21:30");
        poll_minute(&mut runner, &mut env, "2026-04-13T22:00");
        assert_eq!(effects.take(), ["log: Habit tracking popup triggered", "habit tracking popup"]);
    }

    #[test]
    fn daily_summary_defaults_to_22_00_and_habit_prompt_to_off() {
        let mut env = TestEnv::new();
        let effects = Recorder::new();
        let mut runner = runner("[tasks]\n", WeeklySchedule::default(), &effects);
        poll_minute(&mut runner, &mut env, "2026-04-08T22:00");
        assert_eq!(effects.take(), ["log: Daily summary popup triggered", "daily summary popup"]);
    }

    #[test]
    fn urgent_task_checks_mark_procrastination_only_at_the_last_time() {
        let mut env = TestEnv::new();
        write_json(&env, "tasks/tasks.json", json!([{"description": "Task A", "priority": 9}]));
        let effects = Recorder::answering(&[Some(false), Some(false)]);
        let settings = "[tasks]\ndaily_urgent = [\"20:00\", \"10:00\"]\ndaily_summary = \"23:00\"\n";
        let mut runner = runner(settings, WeeklySchedule::default(), &effects);

        poll_minute(&mut runner, &mut env, "2026-04-08T10:00");
        assert_eq!(
            effects.take(),
            [
                "log: Scheduled urgent task check reached at 10:00",
                "log: Urgent task check triggered",
                "log: Urgent task prompt opened for 1 task(s)",
            ]
        );
        assert!(!env.root().join("tasks/procrastinate.json").exists());

        poll_minute(&mut runner, &mut env, "2026-04-08T20:00");
        assert_eq!(
            read_json(&env, "tasks/procrastinate.json"),
            json!([{"description": "Task A", "since": "2026-04-08"}])
        );
        assert_eq!(effects.questions().len(), 2);
        assert!(runner.notified_today.contains("urgent_tasks_10:00"));
        assert!(runner.notified_today.contains("urgent_tasks_20:00"));
    }

    #[test]
    fn urgent_checks_skip_skip_days() {
        let mut env = TestEnv::new();
        write_json(&env, "tasks/tasks.json", json!([{"description": "Task A", "priority": 9}]));
        write_json(&env, "user_config_0/ddl.json", json!([{"event": "due", "deadline": "2026-04-12"}]));
        let effects = Recorder::new();
        let settings =
            "[settings]\nskip_days = [\"sunday\"]\n[tasks]\ndaily_urgent = [\"10:00\"]\nddl_urgent = [\"10:00\"]\n";
        let mut runner = runner(settings, WeeklySchedule::default(), &effects);

        poll_minute(&mut runner, &mut env, "2026-04-12T10:00");
        assert!(effects.calls().is_empty());
    }

    #[test]
    fn urgent_task_prompt_is_not_reopened_while_open() {
        let _env = TestEnv::new();
        let effects = Recorder::new();
        let runner = runner(SETTINGS, WeeklySchedule::default(), &effects);
        runner.urgent_task_prompt_active.store(true, Ordering::SeqCst);

        runner.check_urgent_tasks(true);
        runner.prompt_urgent_tasks(true);

        assert!(effects.calls().is_empty());
        assert!(runner.urgent_task_prompt_active.load(Ordering::SeqCst));
    }

    #[test]
    fn deadline_checks_ring_the_glass_alarm() {
        let mut env = TestEnv::new();
        write_json(
            &env,
            "user_config_0/ddl.json",
            json!([
                {"event": "in2", "deadline": "2026-04-10"},
                {"event": "late", "deadline": "2026-04-07"},
                {"event": "due", "deadline": "2026-04-08"},
                {"event": "later", "deadline": "2026-04-20"},
            ]),
        );
        let effects = Recorder::new();
        let mut runner = runner("[tasks]\nddl_urgent = [\"09:00\"]\n", WeeklySchedule::default(), &effects);

        poll_minute(&mut runner, &mut env, "2026-04-08T09:00");

        let sound = if cfg!(target_os = "macos") {
            "/System/Library/Sounds/Glass.aiff".to_string()
        } else {
            "Glass".to_string()
        };
        assert_eq!(
            effects.take(),
            [
                "log: Scheduled urgent deadline check reached at 09:00".to_string(),
                "log: Urgent deadline reminder triggered for 3 deadline(s)".to_string(),
                format!(
                    "alarm: Urgent Deadlines | 📅 Urgent Deadline Reminder\n\n\
                 ⚠️ late - 2026-04-07 (Overdue 1 days)\n\
                 🔴 due - 2026-04-08 (Due today)\n\
                 🚨 in2 - 2026-04-10 (2 days left) | {sound} | 5 | 300"
                ),
            ]
        );
        assert!(runner.notified_today.contains("urgent_ddls_09:00"));
    }

    #[test]
    fn weekly_and_monthly_reviews_generate_reports_once() {
        let mut env = TestEnv::new();
        let effects = Recorder::new();
        let settings = "[tasks]\nweekly_review = \"Wednesday 20:00\"\nmonthly_review = \"8 21:00\"\n";
        let mut runner = runner(settings, WeeklySchedule::default(), &effects);
        let settings_path = paths::runtime().settings_path;
        *effects.report_result.lock().unwrap() =
            Some(Ok(IndexMap::from([("weekly".to_string(), PathBuf::from("/reports/weekly.pdf"))])));

        poll_minute(&mut runner, &mut env, "2026-04-08T20:00");
        assert_eq!(
            effects.take(),
            [
                format!("reports: {}", settings_path.display()),
                "log: Generated weekly report at /reports/weekly.pdf".to_string(),
            ]
        );
        assert!(runner.notified_today.contains("weekly_review_2026-04-08"));

        poll_minute(&mut runner, &mut env, "2026-04-08T21:00");
        assert_eq!(effects.take(), [format!("reports: {}", settings_path.display())]);
        assert!(runner.notified_today.contains("monthly_review_2026-04"));

        // Not on other days.
        poll_minute(&mut runner, &mut env, "2026-04-09T20:00");
        poll_minute(&mut runner, &mut env, "2026-04-09T21:00");
        assert!(effects.take().is_empty());
    }

    #[test]
    fn malformed_review_settings_are_ignored() {
        let mut env = TestEnv::new();
        let effects = Recorder::new();
        let settings = "[tasks]\nweekly_review = \"20:00\"\nmonthly_review = \"first 21:00\"\n";
        let mut runner = runner(settings, WeeklySchedule::default(), &effects);
        poll_minute(&mut runner, &mut env, "2026-04-08T20:00");
        poll_minute(&mut runner, &mut env, "2026-04-08T21:00");
        assert!(effects.take().is_empty());
    }

    #[test]
    fn report_failures_are_logged() {
        let _env = TestEnv::new();
        let effects = Recorder::new();
        let settings_path = PathBuf::from("/missing/settings.toml");

        *effects.report_result.lock().unwrap() =
            Some(Err(anyhow::Error::from(TomlLoadError::NotFound(settings_path.clone()))));
        try_auto_generate_reports(effects.as_ref(), &settings_path);
        let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        *effects.report_result.lock().unwrap() = Some(Err(anyhow::Error::new(missing).context("reading settings")));
        try_auto_generate_reports(effects.as_ref(), &settings_path);
        *effects.report_result.lock().unwrap() = Some(Err(anyhow::anyhow!("boom")));
        try_auto_generate_reports(effects.as_ref(), &settings_path);

        assert_eq!(
            effects.logs(),
            [
                "Auto reports skipped: settings.toml not found at /missing/settings.toml",
                "Auto reports skipped: settings.toml not found at /missing/settings.toml",
                "Auto report generation skipped: boom",
            ]
        );
    }

    // --- urgent tasks (TestUrgentTasksProcrastinate) --------------------------

    #[test]
    fn prompt_urgent_tasks_updates_task_and_procrastinate_lists() {
        let mut env = TestEnv::new();
        env.set_now("2026-04-08T09:00:00");
        write_json(
            &env,
            "tasks/tasks.json",
            json!([
                {"description": "Task A", "priority": 9},
                {"description": "Task B", "priority": 8},
                {"description": "Task C", "priority": 7},
                {"description": "Task D", "priority": 4},
            ]),
        );
        write_json(&env, "tasks/procrastinate.json", json!([{"description": "Task B", "since": "2026-04-06"}]));
        let effects = Recorder::answering(&[Some(false), Some(true)]);
        let runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        runner.prompt_urgent_tasks(true);

        let tasks = read_json(&env, "tasks/tasks.json");
        let descriptions: Vec<&str> =
            tasks.as_array().unwrap().iter().map(|t| t["description"].as_str().unwrap()).collect();
        assert_eq!(descriptions, ["Task A", "Task C", "Task D"]);
        assert_eq!(
            read_json(&env, "tasks/procrastinate.json"),
            json!([{"description": "Task A", "since": "2026-04-08"}])
        );

        let questions = effects.questions();
        assert_eq!(questions.len(), 2);
        assert_eq!(
            questions[0],
            ("Did you complete this task?\n\nTask A\nPriority: 9".to_string(), "Urgent Task (1/2)".to_string())
        );
        assert_eq!(
            questions[1],
            (
                "Did you complete this task?\n\nTask B\nPriority: 8\nProcrastinated for 2 days".to_string(),
                "Urgent Task (2/2)".to_string()
            )
        );

        let log = read_json(&env, "tasks/tasks.log");
        let log = log.as_array().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0]["action"], "deleted");
        assert_eq!(log[0]["task"], json!({"description": "Task B", "priority": 8}));
    }

    #[test]
    fn prompt_urgent_tasks_not_last_time_does_not_add_to_procrastinate_list() {
        let mut env = TestEnv::new();
        env.set_now("2026-04-08T09:00:00");
        write_json(&env, "tasks/tasks.json", json!([{"description": "Task A", "priority": 9}]));
        write_json(&env, "tasks/procrastinate.json", json!([]));
        let effects = Recorder::answering(&[Some(false)]);
        let runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        runner.prompt_urgent_tasks(false);

        assert_eq!(read_json(&env, "tasks/tasks.json"), json!([{"description": "Task A", "priority": 9}]));
        assert_eq!(read_json(&env, "tasks/procrastinate.json"), json!([]));
    }

    #[test]
    fn prompt_urgent_tasks_includes_overdue_low_priority_tasks() {
        let mut env = TestEnv::new();
        env.set_now("2026-04-08T09:00:00");
        write_json(
            &env,
            "tasks/tasks.json",
            json!([
                {"description": "Task Low Prio Overdue", "priority": 5},
                {"description": "Task Low Prio Normal", "priority": 5},
            ]),
        );
        write_json(
            &env,
            "tasks/procrastinate.json",
            json!([{"description": "Task Low Prio Overdue", "since": "2026-04-05"}]),
        );
        let effects = Recorder::answering(&[Some(true)]);
        let runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        runner.prompt_urgent_tasks(true);

        let questions = effects.questions();
        assert_eq!(questions.len(), 1);
        assert!(questions[0].0.contains("Task Low Prio Overdue"));
        assert!(questions[0].0.contains("Procrastinated for 3 days"));
        assert_eq!(
            read_json(&env, "tasks/tasks.json"),
            json!([{"description": "Task Low Prio Normal", "priority": 5}])
        );
        assert_eq!(read_json(&env, "tasks/procrastinate.json"), json!([]));
    }

    #[test]
    fn prompt_urgent_tasks_stops_on_cancel() {
        let mut env = TestEnv::new();
        env.set_now("2026-04-08T09:00:00");
        let initial = json!([{"description": "Task A", "priority": 9}, {"description": "Task B", "priority": 8}]);
        write_json(&env, "tasks/tasks.json", initial.clone());
        write_json(&env, "tasks/procrastinate.json", json!([]));
        let effects = Recorder::answering(&[None]);
        let runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        runner.prompt_urgent_tasks(true);

        assert_eq!(read_json(&env, "tasks/tasks.json"), initial);
        assert_eq!(read_json(&env, "tasks/procrastinate.json"), json!([]));
        assert!(!env.root().join("tasks/tasks.log").exists());
        assert_eq!(effects.questions().len(), 1);
        assert!(!runner.urgent_task_prompt_active.load(Ordering::SeqCst));
    }

    #[test]
    fn prompt_urgent_tasks_logs_when_nothing_is_urgent() {
        let mut env = TestEnv::new();
        env.set_now("2026-04-08T09:00:00");
        write_json(&env, "tasks/tasks.json", json!([{"description": "Task A", "priority": 3}]));
        let effects = Recorder::new();
        let runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        runner.prompt_urgent_tasks(true);

        assert_eq!(effects.logs(), ["Urgent task prompt found no unfinished high-priority or overdue tasks"]);
        assert!(effects.questions().is_empty());
    }

    #[test]
    fn procrastination_age_labels() {
        let _env = TestEnv::new();
        assert_eq!(ScheduleRunner::format_procrastination_age(None), None);
        assert_eq!(ScheduleRunner::format_procrastination_age(Some(0)).as_deref(), Some("Deferred today"));
        assert_eq!(ScheduleRunner::format_procrastination_age(Some(1)).as_deref(), Some("Procrastinated for 1 day"));
        assert_eq!(ScheduleRunner::format_procrastination_age(Some(4)).as_deref(), Some("Procrastinated for 4 days"));
    }

    #[test]
    fn load_procrastinate_records_supports_legacy_and_record_entries() {
        let env = TestEnv::new();
        write_json(
            &env,
            "tasks/procrastinate.json",
            json!([
                "Legacy Task",
                {"description": "Tracked Task", "since": "2026-04-05"},
                {"description": "Broken Date", "since": "not-a-date"},
            ]),
        );
        let records = data::load_procrastinate_records();
        assert_eq!(
            Value::Object(records.iter().map(|(k, v)| (k.clone(), Value::Object(v.clone()))).collect()),
            json!({
                "Legacy Task": {"description": "Legacy Task"},
                "Tracked Task": {"description": "Tracked Task", "since": "2026-04-05"},
                "Broken Date": {"description": "Broken Date"},
            })
        );
        let list: Vec<String> = data::load_procrastinate_list().into_iter().collect();
        assert_eq!(list, ["Broken Date", "Legacy Task", "Tracked Task"]);
    }

    // --- urgent deadlines (TestUrgentDeadlines) -------------------------------

    #[test]
    fn get_urgent_deadlines_filters_and_sorts() {
        let mut env = TestEnv::new();
        env.set_now("2026-04-08T09:00:00");
        write_json(
            &env,
            "user_config_0/ddl.json",
            json!([
                {"event": "yesterday", "deadline": "2026-04-07"},
                {"event": "today", "deadline": "2026-04-08"},
                {"event": "in2", "deadline": "2026-04-10"},
                {"event": "in5", "deadline": "2026-04-13"},
            ]),
        );
        let effects = Recorder::new();
        let runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        let urgent = runner.get_urgent_deadlines();

        let summary: Vec<(&str, i64)> = urgent.iter().map(|d| (d.event.as_str(), d.days_left)).collect();
        assert_eq!(summary, [("yesterday", -1), ("today", 0), ("in2", 2)]);
    }

    #[test]
    fn get_urgent_deadlines_prunes_two_day_overdue_entries() {
        let mut env = TestEnv::new();
        env.set_now("2026-04-08T09:00:00");
        write_json(
            &env,
            "user_config_0/ddl.json",
            json!([
                {"event": "remove", "deadline": "2026-04-06"},
                {"event": "keep-overdue", "deadline": "2026-04-07"},
                {"event": "today", "deadline": "2026-04-08"},
            ]),
        );
        let effects = Recorder::new();
        let runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        let urgent: Vec<String> = runner.get_urgent_deadlines().into_iter().map(|d| d.event).collect();

        assert_eq!(urgent, ["keep-overdue", "today"]);
        assert_eq!(
            read_json(&env, "user_config_0/ddl.json"),
            json!([{"event": "keep-overdue", "deadline": "2026-04-07"}, {"event": "today", "deadline": "2026-04-08"}])
        );
    }

    #[test]
    fn get_urgent_deadlines_ignores_incomplete_or_missing_entries() {
        let mut env = TestEnv::new();
        env.set_now("2026-04-08T09:00:00");
        let effects = Recorder::new();
        let runner = runner(SETTINGS, WeeklySchedule::default(), &effects);
        assert!(runner.get_urgent_deadlines().is_empty());

        env.write("user_config_0/ddl.json", "not json");
        assert!(runner.get_urgent_deadlines().is_empty());

        write_json(
            &env,
            "user_config_0/ddl.json",
            json!([
                {"event": "", "deadline": "2026-04-08"},
                {"event": "no date"},
                {"event": "bad date", "deadline": "soon"},
                {"event": 42, "deadline": "2026-4-9"},
                "junk",
            ]),
        );
        assert_eq!(
            runner.get_urgent_deadlines(),
            [UrgentDeadline { event: "42".into(), deadline: "2026-4-9".into(), days_left: 1 }]
        );
    }

    #[test]
    fn check_urgent_deadlines_triggers_alarm() {
        let mut env = TestEnv::new();
        env.set_now("2026-04-08T09:00:00");
        write_json(&env, "user_config_0/ddl.json", json!([{"event": "due", "deadline": "2026-04-08"}]));
        let effects = Recorder::new();
        let runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        runner.check_urgent_deadlines();

        assert_eq!(effects.alarms().len(), 1);
    }

    #[test]
    fn check_urgent_deadlines_noop_when_empty() {
        let env = TestEnv::new();
        env.write("user_config_0/ddl.json", "[]");
        let effects = Recorder::new();
        let runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        runner.check_urgent_deadlines();

        assert!(effects.alarms().is_empty());
        assert_eq!(effects.logs(), ["Urgent deadline check found no approaching deadlines"]);
    }

    // --- postponed tasks (TestUrgentTasksPostpone, verify_urgency_repro.py) ---

    #[test]
    fn postponed_tasks_skipped_in_runner() {
        let mut env = TestEnv::new();
        env.set_now("2026-04-08T09:00:00");
        write_json(
            &env,
            "tasks/tasks.json",
            json!([
                {"description": "Task A", "priority": 9, "alarm_from": "2026-04-09"},
                {"description": "Task B", "priority": 8, "alarm_from": "2026-04-08"},
                {"description": "Task C", "priority": 9},
                {"description": "Task D", "priority": 10, "alarm_from": "2026-04-10"},
            ]),
        );
        let effects = Recorder::new();
        let runner = runner(SETTINGS, WeeklySchedule::default(), &effects);

        let unfinished: Vec<Value> = runner.get_unfinished_urgent_tasks();
        let descriptions: Vec<&str> = unfinished.iter().map(|t| t["description"].as_str().unwrap()).collect();
        assert_eq!(descriptions, ["Task B", "Task C"]);
    }

    #[test]
    fn urgent_tasks_are_priority_above_seven() {
        let env = TestEnv::new();
        env.write("user_config_0/settings.toml", "\n[tasks]\ndaily_urgent = [\"10:00\", \"20:00\"]\n");
        write_json(
            &env,
            "tasks/tasks.json",
            json!([
                {"description": "Low Priority", "priority": 1},
                {"description": "Medium Priority", "priority": 5},
                {"description": "High Priority", "priority": 8},
                {"description": "Critical Priority", "priority": 10},
                {"description": "Text Priority", "priority": "9"},
                {"description": "Broken Priority", "priority": "high"},
            ]),
        );
        let config = ScheduleConfig::load_active().unwrap();
        assert_eq!(config.daily_urgent_times(), ["10:00", "20:00"]);

        let effects = Recorder::new();
        let runner = ScheduleRunner::with_effects(config, WeeklySchedule::default(), effects.clone());
        let urgent: Vec<Value> = runner.get_unfinished_urgent_tasks();
        let descriptions: Vec<&str> = urgent.iter().map(|t| t["description"].as_str().unwrap()).collect();
        assert_eq!(descriptions, ["High Priority", "Critical Priority", "Text Priority"]);
    }

    #[test]
    fn task_priority_and_postponement_parsing() {
        assert_eq!(ScheduleRunner::task_priority(&json!({"priority": 8.9})), 8);
        assert_eq!(ScheduleRunner::task_priority(&json!({"priority": null})), 0);
        assert_eq!(ScheduleRunner::task_priority(&json!({})), 0);
        let today = NaiveDate::from_ymd_opt(2026, 4, 8);
        assert!(ScheduleRunner::is_task_postponed(&json!({"alarm_from": "2026-04-09"}), today));
        assert!(!ScheduleRunner::is_task_postponed(&json!({"alarm_from": "2026-04-08"}), today));
        assert!(!ScheduleRunner::is_task_postponed(&json!({"alarm_from": "tomorrow"}), today));
        assert!(!ScheduleRunner::is_task_postponed(&json!({"alarm_from": 20260409}), today));
        assert!(!ScheduleRunner::is_task_postponed(&json!({"alarm_from": ""}), today));
    }

    #[test]
    fn start_up_log_values_use_python_str() {
        let config = config("[tasks]\ndaily_urgency = [\"10:00\", \"20:00\"]\nddl_urgent = \"09:00\"\n");
        assert_eq!(raw_task_setting(&config, &["daily_urgency", "daily_urgent"], "[]"), "['10:00', '20:00']");
        assert_eq!(raw_task_setting(&config, &["ddl_urgency", "ddl_urgent"], "[]"), "09:00");
        assert_eq!(raw_task_setting(&config, &["missing"], "[]"), "[]");
    }
}
