"""
Sync command for assigning today's work blocks to tasks with an LLM.

This command reads the current day's schedule and tasks.json, asks the model to
assign untitled pomodoro/potato blocks to concrete task titles, shows a
preview, and only writes the accepted overlay file after the user confirms it.
"""

from __future__ import annotations

import json
import sys
from datetime import date, datetime
from typing import Any

from schedule_management.i18n import _t

try:
    from rich import box
    from rich.panel import Panel
    from rich.table import Table
except ImportError:
    print(_t("Please install the 'rich' library: pip install rich"))
    sys.exit(1)

from schedule_management import EVEN_PATH, ODD_PATH, SETTINGS_PATH
from schedule_management.commands.setup import CONSOLE, LLMClient, ensure_llm_config
from schedule_management.commands.setup_agent.configuration import (
    _interpret_confirmation,
)
from schedule_management.config import ScheduleConfig, WeeklySchedule
from schedule_management.data import load_tasks, load_mode
from schedule_management.synced_schedule import (
    SyncedDaySchedule,
    apply_synced_schedule,
    format_event_label,
    get_event_block_name,
    iter_syncable_slots,
    save_synced_schedule,
)
from schedule_management.time_utils import get_week_parity

SYNC_SYSTEM_PROMPT = """
You assign today's focus blocks to tasks from a todo list.

Return strict JSON only with this schema:
{
  "summary": "one short paragraph",
  "assignments": {
    "08:30": "short task title",
    "09:00": "short task title"
  }
}

Rules:
- Assign every provided time slot exactly once.
- Use only the provided task list as source material.
- Prefer higher-priority tasks earlier in the day.
- You may split a large task into short concrete sub-steps across multiple slots.
- Keep titles concise and specific.
- Do not include markdown fences or commentary outside the JSON object.
""".strip()


def _today() -> date:
    """Return today's date for sync operations."""
    return date.today()


def _now() -> datetime:
    """Return the current datetime for sync operations."""
    return datetime.now()


def _get_base_today_schedule() -> tuple[dict[str, Any], str, bool, ScheduleConfig]:
    """Load today's base schedule without applying an existing sync overlay."""
    config = ScheduleConfig(SETTINGS_PATH)
    weekly = WeeklySchedule(ODD_PATH, EVEN_PATH)
    is_skipped = config.should_skip_today()
    parity = get_week_parity()
    schedule = {} if is_skipped else weekly.get_today_schedule(config)
    return schedule, parity, is_skipped, config


def _task_priority(task: dict[str, Any]) -> int:
    """Safely parse task priority values."""
    try:
        return int(task.get("priority", 0))
    except (TypeError, ValueError):
        return 0


def _load_ranked_tasks() -> list[dict[str, Any]]:
    """Return valid tasks sorted by priority descending."""
    tasks = load_tasks()
    normalized = []

    for task in tasks:
        if not isinstance(task, dict):
            continue
        description = task.get("description")
        if not isinstance(description, str) or not description.strip():
            continue
        normalized.append(
            {
                "description": description.strip(),
                "priority": _task_priority(task),
            }
        )

    return sorted(normalized, key=lambda item: item["priority"], reverse=True)


def _render_sync_user_prompt(
    *,
    target_date: str,
    parity: str,
    weekday: str,
    schedule: dict[str, Any],
    slots: list[tuple[str, str]],
    tasks: list[dict[str, Any]],
    feedback: list[str],
    config: ScheduleConfig,
) -> str:
    """Build the model prompt for task-to-block assignment."""
    schedule_lines = []
    for time_str in sorted(schedule.keys()):
        event = schedule[time_str]
        schedule_lines.append(f"- {time_str}: {format_event_label(event)}")

    slot_lines = []
    for time_str, block in slots:
        duration = config.time_blocks.get(block, 0)
        slot_lines.append(f"- {time_str}: {block} ({duration} minutes)")

    task_lines = []
    for index, task in enumerate(tasks, 1):
        task_lines.append(
            f"- {index}. {task['description']} (priority {task['priority']})"
        )

    feedback_block = ""
    if feedback:
        feedback_lines = "\n".join(f"- {item}" for item in feedback)
        feedback_block = (
            "\nFeedback from previous rejected previews:\n"
            f"{feedback_lines}\n"
            "Address every feedback point in the next proposal.\n"
        )

    return (
        f"Date: {target_date}\n"
        f"Week parity: {parity}\n"
        f"Weekday: {weekday}\n\n"
        "Today's full schedule:\n"
        f"{chr(10).join(schedule_lines)}\n\n"
        "Work blocks that need assignments:\n"
        f"{chr(10).join(slot_lines)}\n\n"
        "Available tasks from tasks.json:\n"
        f"{chr(10).join(task_lines)}\n"
        f"{feedback_block}"
        "\nReturn JSON only."
    )


def _extract_json_payload(raw_text: str) -> dict[str, Any]:
    """Extract the JSON object from a model response."""
    stripped = raw_text.strip()
    candidates = [stripped]

    if stripped.startswith("```"):
        fence_lines = stripped.splitlines()
        if len(fence_lines) >= 3:
            candidates.append("\n".join(fence_lines[1:-1]).strip())

    start = stripped.find("{")
    end = stripped.rfind("}")
    if start != -1 and end != -1 and end > start:
        candidates.append(stripped[start : end + 1])

    for candidate in candidates:
        if not candidate:
            continue
        try:
            payload = json.loads(candidate)
        except json.JSONDecodeError:
            continue
        if isinstance(payload, dict):
            return payload

    raise ValueError("Model response did not contain a valid JSON object.")


def _parse_assignments(
    payload: dict[str, Any],
    slots: list[tuple[str, str]],
) -> tuple[str | None, dict[str, str]]:
    """Validate the model payload and return summary plus assignments."""
    summary = payload.get("summary")
    if summary is not None and not isinstance(summary, str):
        raise ValueError("'summary' must be a string when present.")

    raw_assignments = payload.get("assignments")
    assignments: dict[str, str] = {}

    if isinstance(raw_assignments, dict):
        for time_str, title in raw_assignments.items():
            if not isinstance(title, str) or not title.strip():
                raise ValueError(f"Assignment for {time_str} is empty.")
            assignments[str(time_str)] = title.strip()
    elif isinstance(raw_assignments, list):
        for item in raw_assignments:
            if not isinstance(item, dict):
                raise ValueError("Assignment list items must be objects.")
            time_str = item.get("time")
            title = item.get("title")
            if not isinstance(time_str, str) or not isinstance(title, str):
                raise ValueError("Assignment list items require string time/title.")
            if not title.strip():
                raise ValueError(f"Assignment for {time_str} is empty.")
            assignments[time_str] = title.strip()
    else:
        raise ValueError("Response must include an 'assignments' object or list.")

    expected_times = {time_str for time_str, _ in slots}
    received_times = set(assignments.keys())
    missing = sorted(expected_times - received_times)
    extra = sorted(received_times - expected_times)

    if missing or extra:
        problems = []
        if missing:
            problems.append(f"missing: {', '.join(missing)}")
        if extra:
            problems.append(f"unexpected: {', '.join(extra)}")
        raise ValueError("Invalid assignment times: " + "; ".join(problems))

    return summary.strip() if isinstance(summary, str) and summary.strip() else None, assignments


def _build_plan(
    *,
    target_date: str,
    parity: str,
    weekday: str,
    slots: list[tuple[str, str]],
    assignments: dict[str, str],
) -> SyncedDaySchedule:
    """Build the persisted synced schedule from validated assignments."""
    slot_lookup = dict(slots)
    overlay: dict[str, dict[str, str]] = {}

    for time_str, title in assignments.items():
        overlay[time_str] = {
            "block": slot_lookup[time_str],
            "title": title,
        }

    return SyncedDaySchedule(
        target_date=target_date,
        parity=parity,
        weekday=weekday,
        assignments=overlay,
    )


def _render_preview_table(schedule: dict[str, Any]) -> Table:
    """Render a full-day preview table."""
    table = Table(
        title="[bold]" + _t("Synced Schedule Preview") + "[/bold]",
        box=box.ROUNDED,
        header_style="bold cyan",
        expand=True,
    )
    table.add_column(_t("Time"), justify="right", style="cyan", width=8, no_wrap=True)
    table.add_column(_t("Activity"), justify="left")

    for time_str in sorted(schedule.keys()):
        event = schedule[time_str]
        label = format_event_label(event)
        block = get_event_block_name(event) or ""

        if block == "pomodoro":
            styled = f"[bold red]{label}[/bold red]"
        elif block == "potato":
            styled = f"[bold yellow]{label}[/bold yellow]"
        elif "break" in label.lower() or "nap" in label.lower():
            styled = f"[italic dim]{label}[/italic dim]"
        else:
            styled = label

        table.add_row(time_str, styled)

    return table


def _schedule_preview_rows(schedule: dict[str, Any]) -> list[dict[str, str | None]]:
    """Return GUI-friendly preview rows for a schedule."""
    rows: list[dict[str, str | None]] = []
    for time_str in sorted(schedule.keys()):
        event = schedule[time_str]
        rows.append(
            {
                "time": time_str,
                "label": format_event_label(event),
                "block": get_event_block_name(event),
            }
        )
    return rows


def _plan_to_payload(plan: SyncedDaySchedule) -> dict[str, Any]:
    """Serialize a synced day plan for the GUI bridge."""
    return {
        "target_date": plan.target_date,
        "parity": plan.parity,
        "weekday": plan.weekday,
        "assignments": plan.assignments,
    }


def _plan_from_payload(payload: dict[str, Any]) -> SyncedDaySchedule:
    """Validate a GUI-provided synced plan payload."""
    target_date = payload.get("target_date")
    parity = payload.get("parity")
    weekday = payload.get("weekday")
    assignments = payload.get("assignments")

    if not isinstance(target_date, str) or not target_date.strip():
        raise ValueError("target_date is required.")
    if not isinstance(parity, str) or not parity.strip():
        raise ValueError("parity is required.")
    if not isinstance(weekday, str) or not weekday.strip():
        raise ValueError("weekday is required.")
    if not isinstance(assignments, dict):
        raise ValueError("assignments must be an object.")

    normalized: dict[str, dict[str, str]] = {}
    for time_str, raw_assignment in assignments.items():
        if not isinstance(raw_assignment, dict):
            raise ValueError(f"assignment for {time_str} must be an object.")
        block = raw_assignment.get("block")
        title = raw_assignment.get("title")
        if not isinstance(block, str) or not block.strip():
            raise ValueError(f"assignment for {time_str} requires block.")
        if not isinstance(title, str) or not title.strip():
            raise ValueError(f"assignment for {time_str} requires title.")
        normalized[str(time_str)] = {
            "block": block.strip(),
            "title": title.strip(),
        }

    return SyncedDaySchedule(
        target_date=target_date.strip(),
        parity=parity.strip(),
        weekday=weekday.strip(),
        assignments=normalized,
    )


def _generate_sync_proposal_from_context(
    *,
    schedule: dict[str, Any],
    parity: str,
    config: ScheduleConfig,
    feedback: list[str] | None = None,
    llm_config: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Generate a sync proposal from already-loaded schedule context."""
    slots = iter_syncable_slots(schedule)
    if not slots:
        raise ValueError("No untitled pomodoro/potato blocks need syncing today.")

    tasks = _load_ranked_tasks()
    if not tasks:
        raise ValueError("No tasks found in tasks.json. Add tasks before running sync.")

    actual_llm_config = llm_config or ensure_llm_config()
    client = LLMClient(actual_llm_config)
    current_date = _today()
    target_date = current_date.isoformat()
    weekday = _now().strftime("%A").lower()
    feedback_items = feedback or []

    user_prompt = _render_sync_user_prompt(
        target_date=target_date,
        parity=parity,
        weekday=weekday,
        schedule=schedule,
        slots=slots,
        tasks=tasks,
        feedback=feedback_items,
        config=config,
    )
    raw_response = client.generate(SYNC_SYSTEM_PROMPT, user_prompt)
    payload = _extract_json_payload(raw_response)
    summary, assignments = _parse_assignments(payload, slots)
    plan = _build_plan(
        target_date=target_date,
        parity=parity,
        weekday=weekday,
        slots=slots,
        assignments=assignments,
    )
    preview_schedule = apply_synced_schedule(
        schedule,
        target_date=current_date,
        parity=parity,
        weekday=weekday,
        synced=plan,
    )

    return {
        "summary": summary,
        "plan": _plan_to_payload(plan),
        "preview": _schedule_preview_rows(preview_schedule),
    }


def generate_sync_proposal(feedback: list[str] | None = None) -> dict[str, Any]:
    """Generate a synced schedule proposal without prompting for acceptance."""
    schedule, parity, is_skipped, config = _get_base_today_schedule()
    if is_skipped:
        raise ValueError("Today is a skipped day. Nothing to sync.")
    if not schedule:
        raise ValueError("No schedule found for today.")

    return _generate_sync_proposal_from_context(
        schedule=schedule,
        parity=parity,
        config=config,
        feedback=feedback,
    )


def accept_sync_plan(plan_payload: dict[str, Any]) -> dict[str, str]:
    """Persist a GUI-accepted synced schedule proposal."""
    plan = _plan_from_payload(plan_payload)
    saved_path = save_synced_schedule(plan)
    return {"savedPath": str(saved_path)}


def _prompt_rejection_reason() -> str:
    """Collect non-empty feedback for the next sync attempt."""
    while True:
        reason = CONSOLE.input(
            "[bold yellow]" + _t("What should change before I regenerate it?") + "[/] "
        ).strip()
        if reason:
            return reason
        CONSOLE.print("[bold yellow]" + _t("Please provide a short reason.") + "[/]")


def _prompt_acceptance() -> bool:
    """Ask whether the user accepts the preview."""
    while True:
        answer = CONSOLE.input(
            "[bold cyan]" + _t("Accept this synced schedule?") + "[/] [bright_black][y/n][/]: "
        )
        decision = _interpret_confirmation(answer)
        if decision is not None:
            return decision
        CONSOLE.print("[bold yellow]" + _t("Please answer with yes or no.") + "[/]")


def sync_command(args) -> int:
    """Handle `rmd sync`."""
    del args  # command has no flags yet

    if load_mode() == "p":
        print(_t("❌ Currently in p mode. Switch back to j mode to execute this command."))
        return 1

    try:
        schedule, parity, is_skipped, config = _get_base_today_schedule()
    except Exception as exc:
        CONSOLE.print("[bold red]" + _t("Failed to load today's schedule:") + "[/] " + str(exc))
        return 1

    if is_skipped:
        CONSOLE.print("[bold yellow]" + _t("Today is a skipped day. Nothing to sync.") + "[/]")
        return 0

    if not schedule:
        CONSOLE.print("[bold yellow]" + _t("No schedule found for today.") + "[/]")
        return 0

    slots = iter_syncable_slots(schedule)
    if not slots:
        CONSOLE.print(
            "[bold yellow]" + _t("No untitled pomodoro/potato blocks need syncing today.") + "[/]"
        )
        return 0

    tasks = _load_ranked_tasks()
    if not tasks:
        CONSOLE.print(
            "[bold red]" + _t("No tasks found in tasks.json. Add tasks before running sync.") + "[/]"
        )
        return 1

    try:
        llm_config = ensure_llm_config()
    except Exception as exc:
        CONSOLE.print("[bold red]" + _t("Failed to initialize LLM config:") + "[/] " + str(exc))
        return 1

    feedback: list[str] = []

    while True:
        try:
            with CONSOLE.status(
                "[bold green]" + _t("Generating task assignments...") + "[/]",
                spinner="line",
            ):
                proposal = _generate_sync_proposal_from_context(
                    schedule=schedule,
                    parity=parity,
                    config=config,
                    feedback=feedback,
                    llm_config=llm_config,
                )
            summary = proposal.get("summary")
            plan_payload = proposal["plan"]
            plan = _plan_from_payload(plan_payload)
            preview_schedule = apply_synced_schedule(
                schedule,
                target_date=date.fromisoformat(plan.target_date),
                parity=parity,
                weekday=plan.weekday,
                synced=plan,
            )
        except Exception as exc:
            CONSOLE.print("[bold red]" + _t("Could not generate a synced schedule:") + "[/] " + str(exc))
            return 1

        if summary:
            CONSOLE.print(
                Panel.fit(str(summary), title=_t("Model Summary"), border_style="cyan")
            )
        CONSOLE.print(_render_preview_table(preview_schedule))

        if _prompt_acceptance():
            try:
                result = accept_sync_plan(plan_payload)
            except Exception as exc:
                CONSOLE.print("[bold red]" + _t("Failed to save synced schedule:") + "[/] " + str(exc))
                return 1

            CONSOLE.print(
                "[bold green]" + _t("Saved accepted sync overlay to") + "[/] [cyan]" + str(result['savedPath']) + "[/]."
            )
            CONSOLE.print(
                "[bold cyan]" + _t("Run `rmd status` to inspect the assigned focus blocks.") + "[/]"
            )
            return 0

        feedback.append(_prompt_rejection_reason())
