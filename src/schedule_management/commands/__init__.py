"""
Commands Package - CLI command handlers for the schedule management system.

This package contains all CLI command implementations, organized by domain:
- tasks: Task management (add, delete, list)
- deadlines: Deadline management (add, delete, show)
- habits: Habit tracking commands
- completion: Shell completion script generation
- status: Status and schedule viewing commands
- sync: Today's focus-block plan overlay (show, apply, clear)
- service: Service management (update, switch, stop, report)
- settings: Interactive TUI for editing settings.toml
"""

from schedule_management.commands.tasks import add_task, delete_task, show_tasks, cancel_task, drop_task
from schedule_management.commands.history import history_command
from schedule_management.commands.deadlines import (
    add_deadline,
    delete_deadline,
    show_deadlines,
)
from schedule_management.commands.habits import track_habits
from schedule_management.commands.completion import completion_command
from schedule_management.commands.status import status_command, view_command
from schedule_management.commands.sync import sync_command
from schedule_management.commands.service import (
    update_command,
    stop_command,
    switch_command,
    report_command,
    edit_schedule_command,
    mode_command,
)
from schedule_management.commands.settings import settings_command

__all__ = [
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
    # Service commands
    "update_command",
    "stop_command",
    "switch_command",
    "report_command",
    "edit_schedule_command",
    "mode_command",
    # Settings command
    "settings_command",
]
