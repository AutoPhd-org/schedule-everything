"""
Reminder CLI Tool - Legacy compatibility module.

This module re-exports the CLI functionality from the new modular
structure for backward compatibility. New code should import from:
- schedule_management.cli (main, create_parser)
- schedule_management.commands.* (individual command handlers)

The primary 'rmd' entry point in pyproject.toml should point to:
    schedule_management.cli:main
The legacy 'reminder' alias may point to the same target for compatibility.

DEPRECATED: Direct imports from reminder.py are deprecated.
Migrate to schedule_management.cli for the main entry point.
"""

# =============================================================================
# Re-exports for backward compatibility
# =============================================================================

# Main CLI entry point
from schedule_management.cli import main, create_parser

# Command handlers - all re-exported from commands package
from schedule_management.commands import (
    # Task commands
    add_task,
    delete_task,
    show_tasks,
    cancel_task,
    drop_task,
    # History command
    history_command,
    # Deadline commands
    add_deadline,
    delete_deadline,
    show_deadlines,
    # Habit commands
    track_habits,
    # Completion command
    completion_command,
    # Status commands
    status_command,
    view_command,
    sync_command,
    # Service commands
    update_command,
    stop_command,
    switch_command,
    report_command,
    edit_schedule_command,
    mode_command,
    settings_command,
)

# Data loaders
from schedule_management.data import (
    load_tasks,
    save_tasks,
    load_deadlines,
    save_deadlines,
    load_habits,
    load_habit_records,
    save_habit_records,
    log_task_action,
    load_mode,
    save_mode,
)

# Config classes (from config.py)
from schedule_management.config import ScheduleConfig, WeeklySchedule

# Status helpers
from schedule_management.commands.status import (
    get_today_schedule_for_status,
    get_current_and_next_events,
)

__all__ = [
    # Main CLI
    "main",
    "create_parser",
    # Task commands
    "add_task",
    "delete_task",
    "show_tasks",
    "cancel_task",
    "drop_task",
    # History command
    "history_command",
    # Deadline commands
    "add_deadline",
    "delete_deadline",
    "show_deadlines",
    # Habit commands
    "track_habits",
    # Completion command
    "completion_command",
    # Status commands
    "status_command",
    "view_command",
    "sync_command",
    "get_today_schedule_for_status",
    "get_current_and_next_events",
    # Service commands
    "update_command",
    "stop_command",
    "switch_command",
    "report_command",
    "edit_schedule_command",
    "mode_command",
    "settings_command",
    # Data loaders
    "load_tasks",
    "save_tasks",
    "load_deadlines",
    "save_deadlines",
    "load_habits",
    "load_habit_records",
    "save_habit_records",
    "log_task_action",
    "load_mode",
    "save_mode",
    # Config
    "ScheduleConfig",
    "WeeklySchedule",
]


# =============================================================================
# Script execution (keep entry point working)
# =============================================================================

if __name__ == "__main__":
    import sys

    sys.exit(main())
