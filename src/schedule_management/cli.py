"""
CLI - Command-line interface entry point for Schedule Everything.

This module provides the main CLI entry point that routes user commands
to their appropriate handlers. It uses argparse to parse commands and
delegates to the various command modules.

Architecture:
    cli.py (this file)
    ├── commands/tasks.py     - add, rm, ls commands
    ├── commands/history.py   - history command
    ├── commands/deadlines.py - ddl add, rm, show commands
    ├── commands/habits.py    - track command
    ├── commands/completion.py - shell completion script generation
    ├── commands/status.py    - status, view commands
    ├── commands/sync.py      - sync command
    ├── commands/service.py   - update, switch, stop, report commands
    ├── commands/settings.py  - settings command (interactive TUI)
    └── commands/setup.py     - setup command

Entry Points:
    - `rmd`: The main CLI command (defined in pyproject.toml)
    - `reminder`: Legacy compatibility alias
    - `reminder-runner`: The background service (in runner.py)

Example Usage:
    $ rmd                       # Show help
    $ rmd add "task" 5     # Add task with priority 5
    $ rmd ls               # List all tasks
    $ rmd ddl add hw 12.15 # Add deadline for Dec 15
    $ rmd status           # Show current status
    $ rmd status -v        # Show verbose schedule
    $ rmd completion zsh   # Print zsh completion script

Module Dependencies:
    - schedule_management.commands.tasks
    - schedule_management.commands.deadlines
    - schedule_management.commands.habits
    - schedule_management.commands.status
    - schedule_management.commands.service
    - schedule_management.commands.setup
"""

import argparse
import sys

from schedule_management import COLORS
from schedule_management.i18n import _t
from schedule_management.config_layout import (
    preview_active_config_dir,
    resolve_config_root_dir,
)

# Import command handlers from organized modules
from schedule_management.commands.tasks import add_task, delete_task, show_tasks, cancel_task, drop_task
from schedule_management.commands.history import history_command
from schedule_management.commands.deadlines import (
    add_deadline,
    delete_deadline,
    show_deadlines,
)
from schedule_management.commands.habits import track_habits
from schedule_management.commands.completion import (
    SUPPORTED_COMPLETION_SHELLS,
    completion_command,
)
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
from schedule_management.commands.setup import setup_command
from schedule_management.commands.settings import settings_command
from schedule_management.commands.browser import browser_command


# =============================================================================
# ARGUMENT PARSER SETUP
# =============================================================================


def create_parser() -> argparse.ArgumentParser:
    """
    Create and configure the argument parser for the CLI.

    Returns:
        Configured ArgumentParser with all subcommands registered.

    Structure:
        rmd
        ├── add <task> <priority>       - Add new task
        ├── rm <tasks...>               - Remove tasks (counts as completed)
        ├── cancel <tasks...>           - Remove tasks added by mistake (not completed)
        ├── drop <tasks...>             - Give up on tasks (not completed)
        ├── ls                          - List tasks
        ├── history [n]                 - Show recent activities
        ├── ddl                         - Deadline management
        │   ├── add <event> <date>      - Add deadline
        │   └── rm <events...>          - Remove deadlines
        ├── track [habit_ids...]        - Track habits
        ├── status [-v]                 - Show current status
        ├── sync                        - Assign today's work blocks to tasks
        ├── view                        - Generate PDF visualization
        ├── update                      - Update config from git
        ├── switch <config_id>          - Switch active config snapshot
        ├── stop                        - Stop reminder service
        ├── report <type>               - Generate report
        ├── edit <file>                 - Edit config file
        ├── settings                    - Interactive settings editor
        ├── completion [shell]          - Print shell completion script
        └── setup                       - Interactive schedule setup
    """
    # Resolve config directories at runtime so test fixtures and env overrides apply.
    config_root_dir = resolve_config_root_dir()
    active_config_dir = preview_active_config_dir(config_root_dir)

    # Build colored help text
    colored_description = (
        f"{COLORS['BOLD']}{COLORS['CYAN']}{_t('rmd CLI')}{COLORS['RESET']} - "
        f"{COLORS['GREEN']}{_t('Manage your schedule management system')}{COLORS['RESET']}"
    )

    colored_epilog = f"""
{COLORS["UNDERLINE"]}{COLORS["YELLOW"]}{_t('Configuration root:')}{COLORS["RESET"]} {COLORS["BLUE"]}{config_root_dir}{COLORS["RESET"]}
{COLORS["UNDERLINE"]}{COLORS["YELLOW"]}{_t('Active config:')}{COLORS["RESET"]} {COLORS["BLUE"]}{active_config_dir}{COLORS["RESET"]}
    """

    # Create main parser
    parser = argparse.ArgumentParser(
        prog="rmd",
        description=colored_description,
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=colored_epilog,
    )

    # Create subparsers container
    subparsers = parser.add_subparsers(
        dest="command",
        title=_t("Available commands"),
        metavar="<command>",
    )
    web_parser = subparsers.add_parser(
        "web", help="Open the local browser workspace (reuses an existing tab)"
    )
    web_parser.add_argument(
        "--no-open", action="store_true",
        help="Start the local server without opening a browser",
    )
    web_parser.add_argument(
        "--port", type=int, default=0,
        help="Local port for a new server (default: automatically chosen)",
    )
    web_parser.set_defaults(func=browser_command)

    # -------------------------------------------------------------------------
    # Task Management Commands
    # -------------------------------------------------------------------------

    # add - Add new task
    add_parser = subparsers.add_parser(
        "add",
        help="Add a new task with description and priority level (1-10)",
        description="Add a task to your task list with a priority level.",
    )
    add_parser.add_argument(
        "task",
        nargs="?",
        default=None,
        help="Description of the task (e.g., 'biology homework')",
    )
    add_parser.add_argument(
        "priority",
        type=int,
        nargs="?",
        default=None,
        choices=range(1, 11),
        metavar="PRIORITY",
        help="Priority level 1-10 (higher = more important)",
    )
    add_parser.add_argument(
        "task_type",
        type=int,
        nargs="?",
        default=None,
        help="Task type ID (e.g., 1 for 'read papers')",
    )
    add_parser.add_argument(
        "postpone",
        type=int,
        nargs="?",
        default=None,
        help="Optional days to postpone daily urgent alarms (e.g., 1 for tomorrow, 2 for two days later)",
    )
    add_parser.set_defaults(func=add_task)

    # rm - Delete tasks
    delete_parser = subparsers.add_parser(
        "rm",
        help="Delete one or more tasks by description or ID",
        description="Remove tasks from your task list.",
    )
    delete_parser.add_argument(
        "tasks",
        nargs="+",
        help="Task descriptions or ID numbers from 'rmd ls'",
    )
    delete_parser.set_defaults(func=delete_task)

    # cancel - Cancel a task that was added by mistake (not counted as completion)
    cancel_parser = subparsers.add_parser(
        "cancel",
        help="Cancel tasks added by mistake (not counted as done)",
        description=(
            "Remove tasks that were added by mistake. Like 'rm' but recorded as "
            "'cancelled' so it does not count as a completion in history/reports."
        ),
    )
    cancel_parser.add_argument(
        "tasks",
        nargs="+",
        help="Task descriptions or ID numbers from 'rmd ls'",
    )
    cancel_parser.set_defaults(func=cancel_task)

    # drop - Give up on a task (not counted as completion)
    drop_parser = subparsers.add_parser(
        "drop",
        help="Give up on tasks (not counted as done)",
        description=(
            "Abandon tasks you no longer intend to do. Like 'rm' but recorded as "
            "'dropped' so it does not count as a completion in history/reports."
        ),
    )
    drop_parser.add_argument(
        "tasks",
        nargs="+",
        help="Task descriptions or ID numbers from 'rmd ls'",
    )
    drop_parser.set_defaults(func=drop_task)

    # ls - List tasks
    show_parser = subparsers.add_parser(
        "ls",
        help="Show all tasks sorted by importance",
        description="Display your task list sorted by priority.",
    )
    show_parser.add_argument(
        "-t",
        "--type",
        dest="task_type",
        default=None,
        help="Only show tasks of this type. Accepts a task type ID (e.g. 1) or name (e.g. 'coding').",
    )
    show_parser.set_defaults(func=show_tasks)

    # history - Show recent completed activities
    history_parser = subparsers.add_parser(
        "history",
        help="Show recent completed task activities",
        description="Display the most recent completed tasks with lifecycle details.",
    )
    history_parser.add_argument(
        "count",
        type=int,
        nargs="?",
        default=5,
        help="Number of recent activities to show (default: 5)",
    )
    history_parser.set_defaults(func=history_command)

    # -------------------------------------------------------------------------
    # Deadline Management Commands
    # -------------------------------------------------------------------------

    # ddl - Deadline management (with subcommands)
    ddl_parser = subparsers.add_parser(
        "ddl",
        help="Manage deadlines (use 'ddl add' or just 'ddl' to list)",
        description="Deadline management. Without subcommand, shows all deadlines.",
    )
    ddl_subparsers = ddl_parser.add_subparsers(
        dest="ddl_command",
        title="Deadline commands",
    )

    # ddl add
    ddl_add_parser = ddl_subparsers.add_parser(
        "add",
        help="Add a new deadline event",
        description="Add a deadline with name and due date.",
    )
    ddl_add_parser.add_argument(
        "event",
        help="Name of the event (e.g., 'homework2')",
    )
    ddl_add_parser.add_argument(
        "date",
        help="Due date in M.D or MM.DD format (e.g., '7.4' for July 4th)",
    )
    ddl_add_parser.set_defaults(func=add_deadline)

    # ddl rm
    ddl_rm_parser = ddl_subparsers.add_parser(
        "rm",
        help="Delete one or more deadline events",
        description="Remove deadlines by their event names.",
    )
    ddl_rm_parser.add_argument(
        "events",
        nargs="+",
        help="Event names to delete (e.g., 'homework2' 'project')",
    )
    ddl_rm_parser.set_defaults(func=delete_deadline)

    # Default: show deadlines when 'ddl' called without subcommand
    ddl_parser.set_defaults(func=show_deadlines)

    # -------------------------------------------------------------------------
    # Habit Tracking Commands
    # -------------------------------------------------------------------------

    # track - Track habits
    track_parser = subparsers.add_parser(
        "track",
        help="Track completed habits for today",
        description="Mark habits as completed. Opens interactive prompt if no IDs given.",
    )
    track_parser.add_argument(
        "habit_ids",
        nargs="*",
        help="Optional habit IDs to mark complete (e.g., '1 2 3')",
    )
    track_parser.set_defaults(func=track_habits)

    # -------------------------------------------------------------------------
    # Status and Visualization Commands
    # -------------------------------------------------------------------------

    # status - Show current status
    status_parser = subparsers.add_parser(
        "status",
        help="Show current status and next events",
        description="Display current event, next scheduled event, and optionally full schedule.",
    )
    status_parser.add_argument(
        "-v",
        "--verbose",
        action="store_true",
        help="Show detailed schedule for today",
    )
    status_parser.set_defaults(func=status_command)

    # view - Generate PDF visualization
    view_parser = subparsers.add_parser(
        "view",
        help="Generate schedule visualizations",
        description="Create a multi-page PDF visualization of your schedules.",
    )
    view_parser.set_defaults(func=view_command)

    # sync - Generate task assignments for today's work blocks
    sync_parser = subparsers.add_parser(
        "sync",
        help="Assign today's pomodoro/potato blocks to tasks with an LLM",
        description=(
            "Generate a preview of today's task-to-block assignments and save "
            "them only after approval."
        ),
    )
    sync_parser.set_defaults(func=sync_command)

    # -------------------------------------------------------------------------
    # Service Management Commands
    # -------------------------------------------------------------------------

    # update - Update config from git
    update_parser = subparsers.add_parser(
        "update",
        help="Reload config and pull from git when available",
        description=(
            "Reload the reminder service, pulling latest schedule files first "
            "when the config directory is git-managed."
        ),
    )
    update_parser.set_defaults(func=update_command)

    # stop - Stop reminder service
    stop_parser = subparsers.add_parser(
        "stop",
        help="Stop the reminder service",
        description="Stop the running reminder-runner background service.",
    )
    stop_parser.set_defaults(func=stop_command)

    # switch - Switch active config set
    switch_parser = subparsers.add_parser(
        "switch",
        help="Switch to a different versioned config set and reload the service",
        description=(
            "Activate a different user_config_n directory and reload the "
            "reminder service."
        ),
    )
    switch_parser.add_argument(
        "config_id",
        help="Numeric config id to activate (for example: 0, 1, 2)",
    )
    switch_parser.set_defaults(func=switch_command)

    # report - Generate report
    report_parser = subparsers.add_parser(
        "report",
        help="Generate weekly or monthly PDF reports",
        description="Generate a productivity report for a specified time period.",
    )
    report_parser.add_argument(
        "type",
        choices=["weekly", "monthly"],
        help="Type of report to generate",
    )
    report_parser.add_argument(
        "-d",
        "--date",
        help="Target date in YYYY-MM-DD format (default: today)",
    )
    report_parser.add_argument(
        "--days",
        type=int,
        default=None,
        help="Compatibility flag; only '--days 7' is accepted for weekly reports",
    )
    report_parser.set_defaults(func=report_command)

    # edit - Edit config file
    edit_parser = subparsers.add_parser(
        "edit",
        help="Edit schedule configuration files",
        description="Open a configuration file in your editor.",
    )
    edit_parser.add_argument(
        "file",
        choices=["settings", "odd", "even", "deadlines", "ddl", "habits"],
        nargs="?",
        default="settings",
        help="File to edit (default: settings)",
    )
    edit_parser.set_defaults(func=edit_schedule_command)

    # setup - Interactive LLM-assisted setup
    setup_parser = subparsers.add_parser(
        "setup",
        help="Interactive setup with LLM-assisted schedule generation",
        description=(
            "Configure model credentials and build or modify schedules "
            "through an interactive wizard."
        ),
    )
    setup_parser.set_defaults(func=setup_command)

    # completion - Print shell completion script
    completion_parser = subparsers.add_parser(
        "completion",
        help="Print a shell completion script for bash, zsh, or tcsh",
        description="Generate a shell completion script for the rmd CLI.",
    )
    completion_parser.add_argument(
        "shell",
        choices=SUPPORTED_COMPLETION_SHELLS,
        nargs="?",
        default="bash",
        help="Shell type to target (default: bash)",
    )
    completion_parser.set_defaults(
        func=completion_command,
        parser_factory=create_parser,
    )

    # settings - Interactive settings editor
    settings_parser = subparsers.add_parser(
        "settings",
        help="Interactive TUI editor for settings.toml",
        description="Browse and edit settings.toml interactively with keyboard navigation.",
    )
    settings_parser.set_defaults(func=settings_command)

    # mode - display or switch the active mode
    mode_parser = subparsers.add_parser(
        "mode",
        help="Switch or display the current mode (j mode or p mode)",
        description="Switch or display the current mode. j mode allows all notifications, p mode cancels specific event notifications.",
    )
    mode_parser.add_argument(
        "mode",
        choices=["j", "p"],
        nargs="?",
        help="Mode to switch to ('j' or 'p')",
    )
    mode_parser.set_defaults(func=mode_command)

    return parser


# =============================================================================
# MAIN ENTRY POINT
# =============================================================================


def main() -> int:
    """
    Main entry point for the rmd CLI.

    Parses command-line arguments and dispatches to the appropriate
    command handler. Shows help if no command is provided.

    Returns:
        Exit code (0 for success, 1 for error)

    Exit Codes:
        0 - Success
        1 - Error or no command provided

    Examples:
        >>> main()  # With sys.argv = ['rmd', 'ls']
        # Displays task list
        0

        >>> main()  # With sys.argv = ['rmd']
        # Displays help text
        1
    """
    parser = create_parser()
    args = parser.parse_args()

    # Show help if no command provided
    if not args.command:
        parser.print_help()
        return 1

    # Execute the command handler
    try:
        result = args.func(args)
        return result if isinstance(result, int) else 0

    except KeyboardInterrupt:
        print("\n" + _t("❌ Operation cancelled by user"))
        return 1

    except Exception as e:
        print(_t("❌ Unexpected error: {e}").format(e=e))
        # For debugging, uncomment:
        # import traceback
        # traceback.print_exc()
        return 1


# =============================================================================
# SCRIPT EXECUTION
# =============================================================================

if __name__ == "__main__":
    sys.exit(main())
