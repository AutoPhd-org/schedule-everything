"""
Presentation helpers for the setup-agent terminal flow.

These helpers keep the build and modify workflows focused on state transitions
instead of console formatting details.
"""

from __future__ import annotations

from difflib import unified_diff
from pathlib import Path

from rich.panel import Panel
from rich.text import Text

from schedule_management.commands.setup_agent.console import CONSOLE


def _render_current_files(config_dir: Path) -> str:
    sections: list[str] = []
    for file_name in (
        "settings.toml",
        "odd_weeks.toml",
        "even_weeks.toml",
        "habits.toml",
    ):
        path = config_dir / file_name
        if not path.exists():
            continue
        content = path.read_text(encoding="utf-8")
        sections.append(f"[{file_name}]\n{content}")
    return "\n\n".join(sections)


def _render_conversation_message(message: str) -> None:
    text = message.strip()
    if not text:
        return
    CONSOLE.print(
        Panel.fit(
            Text(text),
            title="Conversation",
            border_style="blue",
        )
    )


def _render_missing_information(items: list[str]) -> None:
    if not items:
        return
    CONSOLE.print("[bold yellow]I still need the following information:[/]")
    for item in items:
        CONSOLE.print(Text(f"- {item}", style="yellow"))


def _render_schedule_summary(summary: str) -> None:
    text = summary.strip()
    if not text:
        return
    CONSOLE.print(
        Panel.fit(
            Text(text),
            title="Schedule Summary",
            border_style="magenta",
        )
    )


def _render_bundle_preview(config_dir: Path, bundle: dict[str, str]) -> None:
    """Show the proposed file changes before a modification is accepted."""
    lines: list[str] = []
    for file_name, proposed in bundle.items():
        path = config_dir / file_name
        current = path.read_text(encoding="utf-8") if path.exists() else ""
        if current == proposed:
            continue
        lines.extend(
            unified_diff(
                current.splitlines(),
                proposed.splitlines(),
                fromfile=f"current/{file_name}",
                tofile=f"proposed/{file_name}",
                lineterm="",
            )
        )

    if not lines:
        lines = ["No schedule file content changes were proposed."]
    elif len(lines) > 100:
        omitted = len(lines) - 100
        lines = [*lines[:100], f"... {omitted} more diff lines omitted"]

    CONSOLE.print(
        Panel(
            Text("\n".join(lines)),
            title="Proposed schedule changes",
            border_style="cyan",
        )
    )


def _append_conversation_history(
    history: str,
    *,
    assistant_text: str,
    user_text: str,
) -> str:
    lines: list[str] = []
    if history.strip():
        lines.append(history.strip())
    lines.append(f"Assistant: {assistant_text.strip()}")
    lines.append(f"User: {user_text.strip()}")
    return "\n".join(lines).strip()


def _merge_request_with_details(base_text: str, details: list[str]) -> str:
    if not details:
        return base_text
    detail_lines = "\n".join(f"- {item}" for item in details)
    return (
        f"{base_text}\n\n"
        "Additional details provided by the user in follow-up turns:\n"
        f"{detail_lines}"
    )
