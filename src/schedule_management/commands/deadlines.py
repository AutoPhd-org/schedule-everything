"""
Deadline Commands - CLI commands for deadline management.

This module provides CLI command handlers for managing deadlines:
- add_deadline: Add a new deadline event
- delete_deadline: Remove one or more deadlines
- show_deadlines: Display all deadlines in a formatted table

Deadlines are stored in a JSON file with 'event', 'deadline',
and 'added' fields. A recurring deadline also has 'repeat' (weekly,
biweekly, or monthly); once it passes, it rolls forward to its next
occurrence instead of being removed.

Example Usage (via CLI):
    $ rmd ddl add "project1" 7.15    # Add deadline for July 15th
    $ rmd ddl add "rent" 12.1 --repeat monthly
    $ rmd ddl                         # List all deadlines
    $ rmd ddl rm "project1"           # Delete a deadline
"""

import sys
from datetime import date, datetime, timedelta, timezone
from typing import Any
from schedule_management.i18n import _t

try:
    from rich import box
    from rich.console import Console
    from rich.table import Table
except ImportError:
    print(_t("Please install the 'rich' library: pip install rich"))
    sys.exit(1)

from schedule_management.data import load_deadlines, save_deadlines
from schedule_management.recurrence import REPEAT_LABELS, anchor_day, repeat_rule, roll_forward

AUTO_REMOVE_OVERDUE_DAYS = -2


def refresh_deadlines(
    deadlines: list[dict[str, Any]],
    today: date | None = None,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]], bool]:
    """Remove long-overdue one-off deadlines and roll recurring ones forward.

    Returns (kept, removed, changed); callers persist `kept` when `changed`.
    A recurring deadline is never removed: once it is as overdue as a one-off
    deadline would be when pruned, it moves to its next occurrence.
    """
    current_date = today or datetime.now().date()
    earliest_kept = current_date + timedelta(days=AUTO_REMOVE_OVERDUE_DAYS + 1)
    kept_deadlines: list[dict[str, Any]] = []
    removed_deadlines: list[dict[str, Any]] = []
    rolled = False

    for deadline in deadlines:
        try:
            deadline_date = datetime.strptime(
                deadline["deadline"], "%Y-%m-%d"
            ).date()
        except Exception:
            kept_deadlines.append(deadline)
            continue

        if deadline_date >= earliest_kept:
            kept_deadlines.append(deadline)
            continue
        rule = repeat_rule(deadline)
        if rule is None:
            removed_deadlines.append(deadline)
            continue
        next_date = roll_forward(deadline_date, rule, earliest_kept, anchor_day(deadline, deadline_date))
        kept_deadlines.append({**deadline, "deadline": next_date.isoformat()})
        rolled = True

    return kept_deadlines, removed_deadlines, rolled or bool(removed_deadlines)


def prune_expired_deadlines(
    deadlines: list[dict[str, Any]],
    today: date | None = None,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Split deadlines into retained and auto-removed overdue entries.

    Recurring deadlines in the retained list are rolled to their next occurrence.
    """
    kept_deadlines, removed_deadlines, _changed = refresh_deadlines(deadlines, today)
    return kept_deadlines, removed_deadlines


# =============================================================================
# ADD DEADLINE COMMAND
# =============================================================================


def add_deadline(args) -> int:
    """
    Handle the 'ddl add' command - add a new deadline event.

    Supports date formats:
    - M.D (e.g., '7.4' for July 4th)
    - MM.DD (e.g., '07.04' for July 4th)

    If the date has passed this year, uses next year automatically.
    If a deadline with the same event name exists, updates its date.

    Args:
        args: Namespace with 'event' (name) and 'date' (M.D format)

    Returns:
        0 on success, 1 on error

    Example:
        $ rmd ddl add "homework2" 7.4
        ✅ Deadline 'homework2' added successfully for 2024-07-04!
    """
    event_name = args.event
    date_str = args.date

    # Parse the date
    try:
        parts = date_str.split(".")
        if len(parts) != 2:
            print(_t("❌ Error: Date must be in format M.D or MM.DD (e.g., 7.4 or 07.04)"))
            return 1

        month = int(parts[0])
        day = int(parts[1])

        # Validate month and day
        if not (1 <= month <= 12):
            print(_t("❌ Error: Month must be between 1 and 12"))
            return 1
        if not (1 <= day <= 31):
            print(_t("❌ Error: Day must be between 1 and 31"))
            return 1

        # Determine year (current or next if date has passed)
        current_date = datetime.now()
        current_year = current_date.year
        deadline_date = datetime(current_year, month, day)

        if deadline_date.date() < current_date.date():
            deadline_date = datetime(current_year + 1, month, day)

        # Format as ISO date string
        deadline_str = deadline_date.strftime("%Y-%m-%d")

    except ValueError as e:
        print(_t("❌ Error: Invalid date format - {e}").format(e=e))
        return 1

    # Load existing deadlines
    deadlines = load_deadlines()

    # Check if event already exists
    existing_index = None
    for i, ddl in enumerate(deadlines):
        if ddl["event"] == event_name:
            existing_index = i
            break

    # Create new deadline entry
    new_deadline = {
        "event": event_name,
        "deadline": deadline_str,
        "added": datetime.now(timezone.utc).isoformat(),
    }
    repeat = getattr(args, "repeat", None)
    if isinstance(repeat, str) and repeat:
        new_deadline["repeat"] = repeat
        if repeat == "monthly":
            new_deadline["repeat_day"] = deadline_date.day

    # Update existing or add new
    if existing_index is not None:
        old_date = deadlines[existing_index]["deadline"]
        deadlines[existing_index] = new_deadline
        action_msg = _t("✅ Deadline for '{event_name}' updated from {old_date} to {deadline_str}").format(
            event_name=event_name, old_date=old_date, deadline_str=deadline_str
        )
    else:
        deadlines.append(new_deadline)
        action_msg = _t("✅ Deadline '{event}' added successfully for {date}!").format(
            event=event_name, date=deadline_str
        )

    # Save deadlines
    try:
        save_deadlines(deadlines)
        print(action_msg)
        return 0
    except Exception as e:
        print(_t("❌ Error saving deadline: {e}").format(e=e))
        return 1


# =============================================================================
# SHOW DEADLINES COMMAND
# =============================================================================


def show_deadlines(args) -> int:
    """
    Handle the 'ddl' command - display all deadlines in a table.

    Shows deadlines sorted by date (earliest first) with:
    - Event name
    - Deadline date
    - Days remaining
    - Status indicator (color-coded urgency)

    Args:
        args: Namespace (unused, for CLI compatibility)

    Returns:
        0 always (display operation)

    Status indicators:
        🟢 OK: More than 7 days remaining
        🟡 SOON: 4-7 days remaining
        🔴 URGENT: 1-3 days remaining
        🔴 TODAY: Due today
        ⚠️ OVERDUE: Past deadline (dimmed row)
    """
    console = Console()
    current_date = datetime.now().date()

    deadlines, _removed, changed = refresh_deadlines(
        load_deadlines(), today=current_date
    )
    if changed:
        try:
            save_deadlines(deadlines)
        except Exception as e:
            console.print(_t("[bold red]❌ Error saving pruned deadlines: {e}[/bold red]").format(e=e))
            return 1

    if not deadlines:
        console.print(_t("[bold yellow]📅 No deadlines found[/bold yellow]"))
        return 0

    # Sort by date (earliest first)
    sorted_deadlines = sorted(deadlines, key=lambda x: x["deadline"])

    # Create table
    table = Table(
        title="[bold]" + _t("Current Deadlines") + "[/bold]",
        box=box.ROUNDED,
        header_style="bold cyan",
        expand=True,
    )

    table.add_column(_t("Event"), justify="left", style="bold")
    table.add_column(_t("Deadline"), justify="left", width=15)
    table.add_column(_t("Days Left"), justify="right", width=12)
    table.add_column(_t("Status"), justify="center", width=10)

    for ddl in sorted_deadlines:
        event_name = ddl["event"]
        deadline_str = ddl["deadline"]

        # Parse deadline date
        deadline_date = datetime.strptime(deadline_str, "%Y-%m-%d").date()
        days_left = (deadline_date - current_date).days

        row_style = None

        # Determine color and status based on days left
        if days_left < 0:
            # Overdue: dimmed row
            color = "dim"
            status = _t("⚠️ OVERDUE")
            days_text = ""
            row_style = "dim"
        elif days_left == 0:
            color = "red"
            status = _t("🔴 TODAY")
            days_text = f"[{color}]" + _t("Today!") + f"[/{color}]"
        elif days_left <= 3:
            color = "red"
            status = _t("🔴 URGENT")
            days_text = f"[{color}]" + _t("{days} days left").format(days=days_left) + f"[/{color}]"
        elif days_left <= 7:
            color = "yellow"
            status = _t("🟡 SOON")
            days_text = f"[{color}]" + _t("{days} days left").format(days=days_left) + f"[/{color}]"
        else:
            color = "green"
            status = _t("🟢 OK")
            days_text = f"[{color}]" + _t("{days} days left").format(days=days_left) + f"[/{color}]"

        # Format date for display
        deadline_display = deadline_date.strftime("%b %d, %Y")

        rule = repeat_rule(ddl)
        if rule:
            event_name = f"{event_name} [dim]↻ {_t(REPEAT_LABELS[rule])}[/dim]"
        table.add_row(event_name, deadline_display, days_text, status, style=row_style)

    console.print(table)
    console.print("[dim]" + _t("Total deadlines: {count}").format(count=len(deadlines)) + "[/dim]", justify="right")

    return 0


# =============================================================================
# DELETE DEADLINE COMMAND
# =============================================================================


def delete_deadline(args) -> int:
    """
    Handle the 'ddl rm' command - delete one or more deadlines.

    Args:
        args: Namespace with 'events' (list of event names)

    Returns:
        0 on success, 1 if any deletions failed

    Example:
        $ rmd ddl rm homework1 homework2
        ✅ 2 sets of deadlines deleted successfully
    """
    event_identifiers = args.events

    # Load existing deadlines
    deadlines = load_deadlines()

    if not deadlines:
        print(_t("⚠️  No deadlines found to delete"))
        return 1

    total_deleted_count = 0
    all_errors = []
    successful_deletions = []

    for event_identifier in event_identifiers:
        event_name = event_identifier
        original_count = len(deadlines)

        # Find and remove matching deadlines
        deleted_deadlines = [ddl for ddl in deadlines if ddl["event"] == event_name]
        deadlines = [ddl for ddl in deadlines if ddl["event"] != event_name]

        if len(deadlines) == original_count:
            error_msg = _t("❌ Deadline '{event}' not found").format(event=event_name)
            all_errors.append(error_msg)
            continue

        deleted_count = original_count - len(deadlines)
        total_deleted_count += deleted_count

        if deleted_count == 1:
            successful_deletions.append(_t("Deadline '{event}'").format(event=event_name))
        else:
            successful_deletions.append(
                _t("{deleted_count} deadlines with name '{event}'").format(
                    deleted_count=deleted_count, event=event_name
                )
            )

    # Print results
    for error in all_errors:
        print(error)

    if successful_deletions:
        try:
            save_deadlines(deadlines)
            if len(successful_deletions) == 1:
                print(_t("✅ {deletion} deleted successfully!").format(deletion=successful_deletions[0]))
            else:
                print(
                    _t("✅ {count} deadlines deleted successfully:").format(count=len(successful_deletions))
                )
                for deletion in successful_deletions:
                    print(f"   - {deletion}")
            return 0 if not all_errors else 1
        except Exception as e:
            print(_t("❌ Error saving deadlines: {e}").format(e=e))
            return 1
    else:
        return 1
