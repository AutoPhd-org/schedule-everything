"""
Schedule Runner - Main scheduling loop and event handling.

This module provides the ScheduleRunner class which is the core
runtime engine of the schedule management system. It:

- Monitors the current time and triggers events at scheduled times
- Handles time blocks (activities with duration) and time points
- Shows daily summary and habit tracking popups
- Checks for urgent tasks and deadlines
- Auto-generates weekly/monthly reports

The runner operates as a continuous loop, checking every 20 seconds
for events that need to be triggered.

Example Usage:
    >>> from schedule_management.runner import ScheduleRunner
    >>> from schedule_management.config import ScheduleConfig, WeeklySchedule
    >>>
    >>> config = ScheduleConfig('settings.toml')
    >>> weekly = WeeklySchedule('odd_weeks.toml', 'even_weeks.toml')
    >>> runner = ScheduleRunner(config, weekly)
    >>> runner.run()  # Starts the main loop (blocking)

Entry Point:
    This module is typically run via the reminder-runner command,
    which loads configuration and starts the runner.
"""

import json
import sys
import threading
import time
from datetime import datetime
from typing import Any

from schedule_management import (
    SETTINGS_PATH,
    DDL_PATH,
)
from schedule_management.config import ScheduleConfig, WeeklySchedule
from schedule_management.i18n import _t
from schedule_management.commands.deadlines import refresh_deadlines
from schedule_management.synced_schedule import apply_synced_schedule
from schedule_management.time_utils import add_minutes_to_time, alarm, get_week_parity
from schedule_management.platform import ask_yes_no
from schedule_management.data import (
    load_tasks,
    save_tasks,
    load_procrastinate_list,
    load_procrastinate_records,
    save_procrastinate_list,
    get_procrastinate_age_days,
    log_task_action,
    load_mode,
)
from schedule_management.popups import (
    show_daily_summary_popup,
    show_habit_tracking_popup,
)
from schedule_management.report import auto_generate_reports


# =============================================================================
# RUNTIME LOGGING
# =============================================================================


def _log_runtime_event(message: str) -> None:
    """Write a timestamped runtime event line to stdout for service log capture."""
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    print(f"[{timestamp}] {message}", flush=True)


# =============================================================================
# AUTO REPORT GENERATION
# =============================================================================


def try_auto_generate_reports(settings_path: str) -> None:
    """
    Attempt to generate scheduled reports using settings.toml.

    Called at configured weekly/monthly review times. Generates
    reports if they don't already exist for the current period.

    Args:
        settings_path: Path to settings.toml file
    """
    try:
        generated = auto_generate_reports(settings_path)
        if generated:
            for span, pdf_path in generated.items():
                _log_runtime_event(f"Generated {span} report at {pdf_path}")
    except FileNotFoundError:
        _log_runtime_event(f"Auto reports skipped: settings.toml not found at {settings_path}")
    except Exception as exc:
        _log_runtime_event(f"Auto report generation skipped: {exc}")


# =============================================================================
# SCHEDULE RUNNER
# =============================================================================


class ScheduleRunner:
    """
    Main runtime engine for the schedule management system.

    The runner continuously monitors the clock and triggers:
    - Scheduled activities (time blocks with start/end alarms)
    - Point notifications (single-time reminders)
    - Daily summary popup
    - Habit tracking popup
    - Urgent task/deadline reminders
    - Report generation

    Attributes:
        config: ScheduleConfig instance with settings
        weekly_schedule: WeeklySchedule instance with schedules
        notified_today: Set of events already triggered today
        pending_end_alarms: Dict of end-time alarms to trigger

    Example:
        >>> runner = ScheduleRunner(config, weekly_schedule)
        >>> runner.run()  # Starts blocking main loop
    """

    def __init__(self, config: ScheduleConfig, weekly_schedule: WeeklySchedule):
        """
        Initialize the runner with configuration.

        Args:
            config: ScheduleConfig instance
            weekly_schedule: WeeklySchedule instance
        """
        self.config = config
        self.weekly_schedule = weekly_schedule
        self.notified_today = set()  # Events already handled today
        self.pending_end_alarms = {}  # {end_time_str: message}
        self._urgent_task_prompt_lock = threading.Lock()

    # =========================================================================
    # ALARM HANDLING
    # =========================================================================

    def _trigger_alarm(self, title: str, message: str, sound: str | None = None) -> None:
        """
        Trigger an alarm notification in a background thread.

        Args:
            title: Alarm title (for logging, not displayed)
            message: Message to show in the dialog
            sound: Optional sound name/path (defaults to config sound)
        """
        sound_file = sound if sound else self.config.sound_file

        # On macOS, expand a short sound name (e.g. "Glass") to the matching
        # /System/Library/Sounds entry. On other platforms the short name/path
        # is passed through unchanged so play_sound can use whatever default
        # or backend the platform supports.
        if (
            sys.platform == "darwin"
            and sound
            and "/" not in sound
            and not sound.endswith(".aiff")
        ):
            sound_file = f"/System/Library/Sounds/{sound}.aiff"

        # Run alarm in background thread to not block main loop
        threading.Thread(
            target=alarm,
            args=(
                title,
                message,
                sound_file,
                self.config.alarm_interval,
                self.config.max_alarm_duration,
            ),
            daemon=True,
        ).start()

    # =========================================================================
    # EVENT HANDLING
    # =========================================================================

    def _handle_event(self, time_str: str, event: Any) -> None:
        """
        Handle a scheduled event (time block or time point).

        Events can be:
        - String: Time block name or direct message
        - Dict: {'block': 'type', 'title': 'Custom Title'}

        Args:
            time_str: The scheduled time (HH:MM)
            event: Event definition (string or dict)
        """
        if isinstance(event, str):
            if event in self.config.time_blocks:
                # It's a time block (e.g., 'pomodoro')
                self._start_time_block(time_str, event, event)
            else:
                # It's a time_point key or direct message
                if event in self.config.time_points:
                    message = self.config.time_points[event]
                else:
                    message = event  # Use string as message directly
                _log_runtime_event(f"Time point triggered at {time_str}: {message}")
                self._trigger_alarm(_t("Reminder"), message)
                self.notified_today.add(time_str)
        elif isinstance(event, dict) and "block" in event:
            # Dict with block type and optional title
            block_type = event["block"]
            title = event.get("title", block_type)
            if block_type in self.config.time_blocks:
                self._start_time_block(time_str, block_type, title)
            else:
                _log_runtime_event(
                    f"Warning: Unknown block type '{block_type}' at {time_str}"
                )

    def _start_time_block(self, start_time: str, block_type: str, title: str) -> None:
        """
        Start a time block and schedule its end alarm.

        Shows a start notification and queues an end notification
        for when the block duration expires.

        Args:
            start_time: Start time (HH:MM)
            block_type: Block type name (for duration lookup)
            title: Display title for notifications
        """
        duration = self.config.time_blocks[block_type]
        end_time_str = add_minutes_to_time(start_time, duration)

        # Start notification
        start_message = _t("{title} ⏱️ ({duration}min)").format(title=title, duration=duration)
        self._trigger_alarm(_t("Start"), start_message)
        self.notified_today.add(start_time)

        # Schedule end notification
        end_message = _t("{title} finished! Take a break 🎉").format(title=title)
        self.pending_end_alarms[end_time_str] = end_message
        _log_runtime_event(
            f"Time block started at {start_time}: {title} ({duration}min), ends at {end_time_str}"
        )

    # =========================================================================
    # URGENT TASKS & DEADLINES
    # =========================================================================

    @staticmethod
    def _task_priority(task: dict[str, Any]) -> int:
        """Safely parse a task priority value."""
        try:
            return int(task.get("priority", 0))
        except (TypeError, ValueError):
            return 0

    @staticmethod
    def _is_task_postponed(task: dict[str, Any], today: Any = None) -> bool:
        """Check if a task's warning is postponed relative to the target date (today)."""
        alarm_from = task.get("alarm_from")
        if not alarm_from:
            return False
        try:
            if today is None:
                today = datetime.now().date()
            alarm_from_date = datetime.strptime(alarm_from, "%Y-%m-%d").date()
            return today < alarm_from_date
        except Exception:
            return False

    def _get_unfinished_urgent_tasks(self) -> list[dict[str, Any]]:
        """
        Get high-priority tasks that are still pending.

        Returns:
            List of task dicts with priority > 7
        """
        tasks = load_tasks()
        urgent_tasks = []
        today = datetime.now().date()
        for task in tasks:
            if not isinstance(task, dict):
                continue
            if self._task_priority(task) > 7:
                if self._is_task_postponed(task, today):
                    continue
                urgent_tasks.append(task)
        return urgent_tasks

    @staticmethod
    def _format_procrastination_age(age_days: int | None) -> str | None:
        """Format a procrastination age label for prompts and task displays."""
        if age_days is None:
            return None
        if age_days == 0:
            return _t("Deferred today")
        if age_days == 1:
            return _t("Procrastinated for 1 day")
        return _t("Procrastinated for {age_days} days").format(age_days=age_days)

    def _prompt_urgent_tasks(self, is_last_time: bool = False) -> None:
        """Ask for completion of urgent tasks one-by-one and sync procrastinate list."""
        if not hasattr(self, "_urgent_task_prompt_lock"):
            self._urgent_task_prompt_lock = threading.Lock()

        if not self._urgent_task_prompt_lock.acquire(blocking=False):
            return

        try:
            tasks = load_tasks()
            today = datetime.now().date()
            procrastinate_records = load_procrastinate_records()
            procrastinate_list = set(procrastinate_records)

            urgent_tasks = []
            for task in tasks:
                if not isinstance(task, dict):
                    continue
                if self._is_task_postponed(task, today):
                    continue

                is_urgent = self._task_priority(task) > 7
                is_overdue = task.get("description") in procrastinate_list

                if is_urgent or is_overdue:
                    urgent_tasks.append(task)

            if not urgent_tasks:
                _log_runtime_event("Urgent task prompt found no unfinished high-priority or overdue tasks")
                return

            _log_runtime_event(
                f"Urgent task prompt opened for {len(urgent_tasks)} task(s)"
            )
            urgent_tasks.sort(
                key=lambda task: (
                    -self._task_priority(task),
                    str(task.get("description", "")),
                )
            )

            tasks_changed = False
            procrastinate_changed = False
            total_tasks = len(urgent_tasks)
            today = datetime.now().date()

            for index, task in enumerate(urgent_tasks, 1):
                description = str(task.get("description", _t("Unknown Task"))).strip()
                if not description:
                    description = _t("Unknown Task")
                priority = self._task_priority(task)
                age_days = get_procrastinate_age_days(
                    procrastinate_records.get(description, {}).get("since"),
                    today=today,
                )
                age_label = self._format_procrastination_age(age_days)

                question_lines = [
                    _t("Did you complete this task?"),
                    "",
                    description,
                    _t("Priority: {priority}").format(priority=priority),
                ]
                if age_label:
                    question_lines.append(age_label)
                question = "\n".join(question_lines)
                title = _t("Urgent Task ({index}/{total_tasks})").format(index=index, total_tasks=total_tasks)

                result = ask_yes_no(question, title)
                if result is None:
                    break

                if result:
                    if task in tasks:
                        tasks.remove(task)
                        tasks_changed = True
                        try:
                            log_task_action("deleted", task)
                        except Exception as exc:
                            _log_runtime_event(
                                f"Warning: Could not log task deletion: {exc}"
                            )

                    if description in procrastinate_list:
                        procrastinate_list.discard(description)
                        procrastinate_changed = True
                else:
                    if is_last_time:
                        if description not in procrastinate_list:
                            procrastinate_list.add(description)
                            procrastinate_changed = True

            if tasks_changed:
                save_tasks(tasks)
            if procrastinate_changed:
                save_procrastinate_list(procrastinate_list, today=today)
        except Exception as exc:
            _log_runtime_event(f"Urgent task prompt skipped: {exc}")
        finally:
            self._urgent_task_prompt_lock.release()

    def _get_urgent_deadlines(self) -> list[dict[str, Any]]:
        """
        Get deadlines that are due within 3 days.

        Returns:
            List of deadline dicts with days_left <= 3, sorted by urgency
        """
        try:
            with open(DDL_PATH, "r", encoding="utf-8") as f:
                deadlines = json.load(f)
        except (json.JSONDecodeError, FileNotFoundError):
            return []

        today = datetime.now().date()
        deadlines, _removed, changed = refresh_deadlines(deadlines, today=today)
        if changed:
            try:
                with open(DDL_PATH, "w", encoding="utf-8") as f:
                    json.dump(deadlines, f, indent=2, ensure_ascii=False)
            except OSError as exc:
                _log_runtime_event(f"Expired deadline cleanup skipped: {exc}")

        urgent: list[dict[str, Any]] = []

        for ddl in deadlines:
            if not isinstance(ddl, dict):
                continue
            event = ddl.get("event")
            deadline_str = ddl.get("deadline")
            if not event or not deadline_str:
                continue

            try:
                deadline_date = datetime.strptime(deadline_str, "%Y-%m-%d").date()
            except Exception:
                continue

            days_left = (deadline_date - today).days
            if days_left <= 3:
                urgent.append(
                    {
                        "event": str(event),
                        "deadline": deadline_str,
                        "days_left": days_left,
                    }
                )

        # Sort by days left, then deadline, then event name
        urgent.sort(
            key=lambda d: (
                d.get("days_left", 0),
                d.get("deadline", ""),
                d.get("event", ""),
            )
        )
        return urgent

    def _check_urgent_tasks(self, is_last_time: bool = False) -> None:
        """Prompt high-priority unfinished tasks one-by-one without blocking the run loop."""
        if not hasattr(self, "_urgent_task_prompt_lock"):
            self._urgent_task_prompt_lock = threading.Lock()

        if self._urgent_task_prompt_lock.locked():
            return

        _log_runtime_event("Urgent task check triggered")
        threading.Thread(target=self._prompt_urgent_tasks, args=(is_last_time,), daemon=True).start()

    def _check_urgent_deadlines(self) -> None:
        """Show reminder for approaching deadlines."""
        urgent_deadlines = self._get_urgent_deadlines()
        if not urgent_deadlines:
            _log_runtime_event("Urgent deadline check found no approaching deadlines")
            return

        lines: list[str] = []
        for ddl in urgent_deadlines:
            event = ddl.get("event", _t("Unknown Event"))
            deadline_str = ddl.get("deadline", "")
            days_left = ddl.get("days_left", 0)

            if days_left < 0:
                lines.append(_t("⚠️ {event} - {deadline_str} (Overdue {days_left} days)").format(event=event, deadline_str=deadline_str, days_left=-days_left))
            elif days_left == 0:
                lines.append(_t("🔴 {event} - {deadline_str} (Due today)").format(event=event, deadline_str=deadline_str))
            else:
                lines.append(_t("🚨 {event} - {deadline_str} ({days_left} days left)").format(event=event, deadline_str=deadline_str, days_left=days_left))

        message = _t("📅 Urgent Deadline Reminder\n\n") + "\n".join(lines)
        _log_runtime_event(
            f"Urgent deadline reminder triggered for {len(urgent_deadlines)} deadline(s)"
        )
        self._trigger_alarm(_t("Urgent Deadlines"), message, sound="Glass")

    # =========================================================================
    # MAIN LOOP
    # =========================================================================

    def run(self) -> None:
        """
        Start the main scheduling loop.

        Continuously monitors the clock and triggers events.
        This method runs forever until the process is terminated.

        The loop:
        1. Checks if daily summary time is reached
        2. Checks if habit prompt time is reached
        3. Checks urgent task/deadline times
        4. Checks weekly/monthly review times
        5. Triggers scheduled events from today's schedule
        6. Triggers pending end alarms
        7. Resets at midnight
        8. Sleeps 20 seconds before next check
        """
        while True:
            now = datetime.now()
            now_str = now.strftime("%H:%M")
            current_mode = load_mode()
            today_schedule = self.weekly_schedule.get_today_schedule(self.config)
            if today_schedule:
                today_schedule = apply_synced_schedule(
                    today_schedule,
                    target_date=now.date(),
                    parity=get_week_parity(),
                    weekday=now.strftime("%A").lower(),
                )

            # -----------------------------------------------------------------
            # Daily Summary Time
            # -----------------------------------------------------------------
            daily_summary_key = f"daily_summary_{now_str}"
            if (
                now_str == self.config.daily_summary_time
                and daily_summary_key not in self.notified_today
                and not self.config.should_skip_today()
            ):
                _log_runtime_event("Daily summary popup triggered")
                threading.Thread(target=show_daily_summary_popup, daemon=True).start()
                self.notified_today.add(daily_summary_key)

            # -----------------------------------------------------------------
            # Habit Prompt Time
            # -----------------------------------------------------------------
            habit_prompt_time = self.config.habit_prompt_time
            habit_prompt_key = f"habit_prompt_{now.strftime('%Y-%m-%d')}"
            if (
                habit_prompt_time
                and now_str == habit_prompt_time
                and habit_prompt_key not in self.notified_today
                and not self.config.should_skip_today()
            ):
                _log_runtime_event("Habit tracking popup triggered")
                threading.Thread(target=show_habit_tracking_popup, daemon=True).start()
                self.notified_today.add(habit_prompt_key)

            # -----------------------------------------------------------------
            # Urgent Tasks & Deadlines
            # -----------------------------------------------------------------
            if not self.config.should_skip_today():
                # Check urgent tasks
                sorted_urgent_times = sorted(self.config.daily_urgent_times) if self.config.daily_urgent_times else []
                for urgent_time in self.config.daily_urgent_times:
                    urgent_tasks_key = f"urgent_tasks_{urgent_time}"
                    if (
                        now_str == urgent_time
                        and urgent_tasks_key not in self.notified_today
                    ):
                        _log_runtime_event(
                            f"Scheduled urgent task check reached at {urgent_time}"
                        )
                        is_last_time = (urgent_time == sorted_urgent_times[-1]) if sorted_urgent_times else False
                        self._check_urgent_tasks(is_last_time=is_last_time)
                        self.notified_today.add(urgent_tasks_key)

                # Check urgent deadlines
                for urgent_time in self.config.ddl_urgent_times:
                    urgent_ddls_key = f"urgent_ddls_{urgent_time}"
                    if (
                        now_str == urgent_time
                        and urgent_ddls_key not in self.notified_today
                    ):
                        _log_runtime_event(
                            f"Scheduled urgent deadline check reached at {urgent_time}"
                        )
                        self._check_urgent_deadlines()
                        self.notified_today.add(urgent_ddls_key)

            # -----------------------------------------------------------------
            # Weekly Review
            # -----------------------------------------------------------------
            weekly_review_setting = self.config.weekly_review_time
            if weekly_review_setting:
                try:
                    parts = weekly_review_setting.split()
                    if len(parts) == 2:
                        day_of_week, review_time = parts
                        if (
                            now.strftime("%A").lower() == day_of_week.lower()
                            and now_str == review_time
                        ):
                            review_key = f"weekly_review_{now.strftime('%Y-%m-%d')}"
                            if review_key not in self.notified_today:
                                threading.Thread(
                                    target=try_auto_generate_reports,
                                    args=(SETTINGS_PATH,),
                                    daemon=True,
                                ).start()
                                self.notified_today.add(review_key)
                except Exception:
                    pass

            # -----------------------------------------------------------------
            # Monthly Review
            # -----------------------------------------------------------------
            monthly_review_setting = self.config.monthly_review_time
            if monthly_review_setting:
                try:
                    parts = monthly_review_setting.split()
                    if len(parts) == 2:
                        day_of_month, review_time = parts
                        if now.day == int(day_of_month) and now_str == review_time:
                            review_key = f"monthly_review_{now.strftime('%Y-%m')}"
                            if review_key not in self.notified_today:
                                threading.Thread(
                                    target=try_auto_generate_reports,
                                    args=(SETTINGS_PATH,),
                                    daemon=True,
                                ).start()
                                self.notified_today.add(review_key)
                except Exception:
                    pass

            # -----------------------------------------------------------------
            # Skip if no schedule today
            # -----------------------------------------------------------------
            if not today_schedule:
                time.sleep(20)
                continue

            # -----------------------------------------------------------------
            # Handle Start Events
            # -----------------------------------------------------------------
            if current_mode == "j" and now_str in today_schedule and now_str not in self.notified_today:
                self._handle_event(now_str, today_schedule[now_str])

            # -----------------------------------------------------------------
            # Handle End Alarms
            # -----------------------------------------------------------------
            if (
                current_mode == "j"
                and now_str in self.pending_end_alarms
                and now_str not in self.notified_today
            ):
                message = self.pending_end_alarms[now_str]
                _log_runtime_event(f"End alarm triggered at {now_str}: {message}")
                self._trigger_alarm(_t("End Reminder"), message)
                self.notified_today.add(now_str)
                del self.pending_end_alarms[now_str]

            # -----------------------------------------------------------------
            # Midnight Reset
            # -----------------------------------------------------------------
            if now_str == "00:00":
                _log_runtime_event("Midnight reset triggered")
                self.notified_today.clear()
                self.pending_end_alarms.clear()

            # Sleep before next check
            time.sleep(20)


# =============================================================================
# ENTRY POINT
# =============================================================================


def main() -> None:
    """
    Main entry point for the schedule runner.

    Loads configuration and starts the scheduling loop.
    This function runs forever until terminated.
    """
    from schedule_management import ODD_PATH, EVEN_PATH

    config = ScheduleConfig(SETTINGS_PATH)
    weekly = WeeklySchedule(ODD_PATH, EVEN_PATH)
    _log_runtime_event(
        "Runner started with "
        f"settings={SETTINGS_PATH}, daily_urgent={config.daily_urgent_times}, "
        f"ddl_urgent={config.ddl_urgent_times}, daily_summary={config.daily_summary_time}, "
        f"habit_prompt={config.habit_prompt_time}"
    )
    runner = ScheduleRunner(config, weekly)
    runner.run()


if __name__ == "__main__":
    main()
