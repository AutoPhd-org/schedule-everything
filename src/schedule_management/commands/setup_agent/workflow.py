"""
Interactive setup command with pi-powered schedule generation.

This module now focuses on the high-level build and modify flows while helper
logic lives in dedicated setup-agent submodules.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
from collections.abc import Callable
from pathlib import Path
from typing import Any

from rich.panel import Panel

from schedule_management.config_layout import (
    clone_active_config_dir,
    get_next_config_id,
    write_active_config_id,
)
from schedule_management.commands.setup_agent.attachments import (
    _load_source_attachment,
    _resolve_source_path_input,
)
from schedule_management.commands.setup_agent.configuration import (
    _ask_yes_no,
    _interpret_confirmation,
    _prompt_non_empty,
    _resolve_config_dir,
    _resolve_llm_config_path,  # noqa: F401 - re-exported by the setup facade
    ensure_llm_config,
    has_completed_configuration,
    load_llm_config,
    save_llm_config,
)
from schedule_management.commands.setup_agent.console import CONSOLE
from schedule_management.commands.setup_agent.interaction import (
    _append_conversation_history,
    _merge_request_with_details,
    _render_conversation_message,
    _render_bundle_preview,
    _render_current_files,
    _render_missing_information,
    _render_schedule_summary,
)
from schedule_management.commands.setup_agent.models import (
    AgentTurn,
    LLMConfig,
    SourceAttachment,
    ToolCall,
)
from schedule_management.commands.setup_agent.prompts import (
    BUILD_SYSTEM_PROMPT,
    MODIFY_SYSTEM_PROMPT,
    render_build_user_prompt,
    render_modify_user_prompt,
)
from schedule_management.commands.setup_agent.profile_store import (
    _load_profile_markdown,
    _resolve_profile_path,
    _write_profile_markdown,
)
from schedule_management.commands.setup_agent.response_parser import (
    _parse_agent_turn,  # noqa: F401 - re-exported by the setup facade
    _request_agent_turn,
)
from schedule_management.commands.setup_agent.tools import LocalFileTools

DEFAULT_HABITS_TOML = """[habits]\n# Example: 1 = "Read for 20 minutes"\n"""


def _build_local_file_tools(config_dir: Path) -> LocalFileTools:
    roots = [
        config_dir,
        Path.cwd(),
        Path.home(),
    ]
    return LocalFileTools(allowed_roots=roots)


def _add_text_attachment(user_prompt: str, attachment: SourceAttachment | None) -> str:
    if not attachment or not attachment.text_content:
        if attachment and attachment.image_base64:
            return (
                f"{user_prompt}\n\n"
                f"An image attachment is included ({attachment.path.name}). "
                "The runtime already passed this image via native vision input, "
                "so inspect the image directly and do not ask the user to "
                "manually transcribe its full content."
            )
        return user_prompt

    return (
        f"{user_prompt}\n\n"
        f"Attached file content ({attachment.path.name}):\n"
        f"```\n{attachment.text_content}\n```"
    )


class LLMClient:
    """pi-backed runtime adapter for setup-agent turns."""

    def __init__(self, config: LLMConfig):
        self.config = config

    @staticmethod
    def _resolve_pi_bin() -> str:
        explicit = os.getenv("REMINDER_PI_BIN", "").strip()
        if explicit:
            return explicit

        discovered = shutil.which("pi")
        if discovered:
            return discovered

        raise RuntimeError(
            "pi CLI not found in PATH. Install it with "
            "'npm install -g @earendil-works/pi-coding-agent' "
            "or set REMINDER_PI_BIN."
        )

    @staticmethod
    def _compose_user_prompt(
        user_prompt: str,
        attachment: SourceAttachment | None,
    ) -> str:
        """Enrich the user prompt with attachment text for the pi CLI call."""
        return _add_text_attachment(user_prompt, attachment).strip()

    @staticmethod
    def _extract_pi_error_message(error_payload: Any) -> str:
        if isinstance(error_payload, str):
            return error_payload.strip()

        if isinstance(error_payload, dict):
            data = error_payload.get("data")
            if isinstance(data, dict):
                message = data.get("message")
                if isinstance(message, str) and message.strip():
                    return message.strip()

            message = error_payload.get("message")
            if isinstance(message, str) and message.strip():
                return message.strip()

            name = error_payload.get("name")
            if isinstance(name, str) and name.strip():
                return name.strip()

        return str(error_payload).strip()

    @staticmethod
    def _extract_message_text(message: Any) -> str:
        """Pull assistant text out of a pi message object.

        pi emits ``message.content`` either as a list of parts
        (``[{"type": "text", "text": "..."}]``) or, on some adapters, as a
        bare string. Concatenate text parts in order and return the joined
        result (empty string when nothing was produced).
        """
        content = message.get("content") if isinstance(message, dict) else None
        if isinstance(content, str):
            return content
        if not isinstance(content, list):
            return ""

        parts: list[str] = []
        for item in content:
            if isinstance(item, dict):
                if item.get("type") == "text":
                    text = item.get("text")
                    if isinstance(text, str) and text.strip():
                        parts.append(text)
                else:
                    text = item.get("text")
                    if isinstance(text, str) and text.strip():
                        parts.append(text)
            elif isinstance(item, str) and item.strip():
                parts.append(item)
        return "".join(parts)

    @classmethod
    def _parse_pi_json_events(cls, stdout: str) -> tuple[str, str | None]:
        """Parse pi ``--mode json --print`` newline-delimited events.

        Returns ``(assistant_text, error_or_None)``. Assistant text is taken
        from the latest assistant ``message_end``/``turn_end``/``agent_end``
        event so partial streaming deltas do not fragment the final answer.
        Errors are collected from explicit ``type:"error"`` events and from
        assistant messages whose ``stopReason`` is ``"error"``.
        """
        assistant_text = ""
        errors: list[str] = []

        for raw_line in stdout.splitlines():
            line = raw_line.strip()
            if not line:
                continue

            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue

            if not isinstance(event, dict):
                continue

            event_type = event.get("type")

            if event_type == "error":
                message = cls._extract_pi_error_message(event.get("error"))
                if message:
                    errors.append(message)
                continue

            if event_type in {"message_end", "turn_end", "agent_end"}:
                message = event.get("message")
                if not isinstance(message, dict):
                    continue
                if message.get("role") != "assistant":
                    continue

                if message.get("stopReason") == "error":
                    err = message.get("errorMessage")
                    if isinstance(err, str) and err.strip():
                        errors.append(err.strip())

                extracted = cls._extract_message_text(message)
                if extracted.strip():
                    assistant_text = extracted.strip()

        rendered_error = "\n".join(errors).strip() or None
        return assistant_text, rendered_error

    def generate(
        self,
        system_prompt: str,
        user_prompt: str,
        attachment: SourceAttachment | None = None,
        on_text: Callable[[str], None] | None = None,
        file_tools: LocalFileTools | None = None,
        on_tool_activity: Callable[[str], None] | None = None,
    ) -> str:
        command = [
            self._resolve_pi_bin(),
            "--mode",
            "json",
            "--print",
            "--tools",
            "read,grep,find,ls",
        ]
        if self.config.model:
            command.extend(["--model", self.config.model])
        command.extend(["--system-prompt", system_prompt.strip()])
        if attachment is not None:
            command.append(f"@{attachment.path}")

        command.append(self._compose_user_prompt(user_prompt, attachment))

        # pi owns credentials and model selection; inherit the user's
        # environment so pi reads its own auth store and provider env vars.
        env = os.environ.copy()

        if on_tool_activity is not None:
            if file_tools is None:
                on_tool_activity("pi agent is running...")
            else:
                on_tool_activity("pi agent is reading schedule context...")

        completed = subprocess.run(
            command,
            capture_output=True,
            text=True,
            check=False,
            env=env,
        )

        stdout = completed.stdout.strip()
        stderr = completed.stderr.strip()
        parsed_text, parsed_error = self._parse_pi_json_events(stdout)

        if completed.returncode != 0:
            detail = (
                parsed_error
                or stderr
                or parsed_text
                or stdout
                or f"exit code {completed.returncode}"
            )
            raise RuntimeError(f"pi CLI execution failed: {detail}")

        if parsed_error:
            raise RuntimeError(f"pi CLI reported an error: {parsed_error}")

        response_text = parsed_text
        if not response_text and stdout:
            raise RuntimeError("pi CLI returned no assistant message in its JSON output")

        if not response_text and stderr:
            raise RuntimeError(f"pi CLI stderr: {stderr}")

        if not response_text:
            raise RuntimeError("pi CLI returned an empty response")

        if on_text is not None:
            on_text(response_text)

        return response_text


def _write_bundle(config_dir: Path, bundle: dict[str, str]) -> None:
    config_dir.mkdir(parents=True, exist_ok=True)
    for file_name, content in bundle.items():
        target = config_dir / file_name
        target.write_text(content, encoding="utf-8")


def _persist_profile_draft(config_dir: Path, profile_markdown: str | None) -> str | None:
    if not profile_markdown:
        return None
    _write_profile_markdown(config_dir, profile_markdown)
    return profile_markdown.strip()


def _apply_versioned_schedule_update(
    config_dir: Path,
    bundle: dict[str, str],
    *,
    profile_markdown: str | None,
) -> tuple[int, Path]:
    root_dir = config_dir.parent
    new_config_id, new_config_dir = clone_active_config_dir(
        root_dir,
        source_dir=config_dir,
    )
    _write_bundle(new_config_dir, bundle)
    if profile_markdown:
        _write_profile_markdown(new_config_dir, profile_markdown)
    write_active_config_id(root_dir, new_config_id)
    return new_config_id, new_config_dir


def _reload_runner_after_config_change() -> tuple[bool, str]:
    from schedule_management.commands.service import _restart_reminder_service

    return _restart_reminder_service()


def _turn_requests_manual_image_transcription(turn: AgentTurn) -> bool:
    combined = " ".join(
        [
            turn.conversation,
            turn.question_to_user or "",
            " ".join(turn.missing_information),
        ]
    ).lower()

    if not combined:
        return False

    blindness_markers = (
        "cannot see image",
        "can't see image",
        "cannot see images",
        "can't see images",
        "cannot view image",
        "can't view image",
        "unable to see image",
        "unable to view image",
    )
    if any(marker in combined for marker in blindness_markers):
        return True

    manual_description_markers = (
        "describe the image",
        "describe changes from the image",
        "details from the image",
        "what is in the image",
        "transcribe the image",
        "type out the image",
    )
    if "image" not in combined:
        return False
    if not any(marker in combined for marker in manual_description_markers):
        return False

    quality_markers = (
        "blurry",
        "unclear",
        "low resolution",
        "cropped",
        "not legible",
        "hard to read",
    )
    return not any(marker in combined for marker in quality_markers)


def modify_schedule_agent(llm_config: LLMConfig, config_dir: Path) -> int:
    client = LLMClient(llm_config)
    file_tools = _build_local_file_tools(config_dir)
    current_profile = _load_profile_markdown(config_dir)
    CONSOLE.print(
        Panel.fit(
            "Schedule modification assistant is ready.",
            title="Modify",
            border_style="cyan",
        )
    )

    while True:
        base_change_request = _prompt_non_empty(
            "What would you like to change in your schedule? "
        )
        follow_up_details: list[str] = []
        conversation_history = ""

        while True:
            current_files = _render_current_files(config_dir)
            change_request = _merge_request_with_details(
                base_change_request,
                follow_up_details,
            )
            user_prompt = render_modify_user_prompt(
                change_request=change_request,
                current_files=current_files,
                profile_context=current_profile,
                conversation_history=conversation_history,
            )

            turn, error = _request_agent_turn(
                client,
                MODIFY_SYSTEM_PROMPT,
                user_prompt,
                file_tools=file_tools,
            )
            if turn is None:
                CONSOLE.print(
                    f"[bold red]Could not update schedule from model response:[/] {error}"
                )
                return 1

            _render_conversation_message(turn.conversation)
            if turn.profile_markdown:
                current_profile = turn.profile_markdown.strip()

            if turn.needs_user_input:
                _render_missing_information(turn.missing_information)
                follow_up_prompt = (
                    turn.question_to_user or "Please provide the missing information: "
                )
                user_answer = _prompt_non_empty(follow_up_prompt)
                follow_up_details.append(user_answer)
                conversation_history = _append_conversation_history(
                    conversation_history,
                    assistant_text=turn.conversation,
                    user_text=user_answer,
                )
                continue

            if turn.bundle is None:
                CONSOLE.print(
                    "[bold red]Model did not return schedule files for this turn.[/]"
                )
                return 1

            bundle = dict(turn.bundle)
            if (
                "habits.toml" not in bundle
                and not (config_dir / "habits.toml").exists()
            ):
                bundle["habits.toml"] = DEFAULT_HABITS_TOML

            _render_bundle_preview(config_dir, bundle)
            if current_profile and current_profile != _load_profile_markdown(config_dir):
                CONSOLE.print("[cyan]The profile draft will also be updated.[/]")

            pending_config_id = get_next_config_id(config_dir.parent)
            if not _ask_yes_no(
                f"Apply this update as {config_dir.parent / f'user_config_{pending_config_id}'} "
                "and switch to it?",
                default=True,
            ):
                revision_request = _prompt_non_empty(
                    "What should I adjust before trying again? "
                )
                follow_up_details.append(revision_request)
                conversation_history = _append_conversation_history(
                    conversation_history,
                    assistant_text=turn.conversation,
                    user_text=revision_request,
                )
                continue

            with CONSOLE.status(
                "[bold green]Applying schedule updates...[/]",
                spinner="line",
            ):
                new_config_id, new_config_dir = _apply_versioned_schedule_update(
                    config_dir,
                    bundle,
                    profile_markdown=current_profile,
                )

            CONSOLE.print(
                "[bold green]Schedule updated as[/] "
                f"[cyan]{new_config_dir.name}[/]."
            )
            reloaded, details = _reload_runner_after_config_change()
            if reloaded:
                CONSOLE.print("[bold green]Reminder service reloaded.[/]")
            elif details == "No installer restart script found.":
                CONSOLE.print(
                    "[bold yellow]No installer restart script found.[/] "
                    "Restart the reminder service manually if it is running."
                )
            else:
                CONSOLE.print(
                    f"[bold red]Reminder service reload failed:[/] {details}"
                )
                return 1

            config_dir = new_config_dir
            CONSOLE.print("[bold cyan]Run rmd view to preview the result.[/]")
            break

        if not _ask_yes_no("Do you want to apply another adjustment?", default=False):
            return 0


def build_schedule_agent(llm_config: LLMConfig, config_dir: Path) -> int:
    client = LLMClient(llm_config)
    file_tools = _build_local_file_tools(config_dir)
    current_profile = _load_profile_markdown(config_dir)
    CONSOLE.print(
        Panel.fit(
            "Hello! I can help build your first schedule configuration.",
            title="Build",
            border_style="green",
        )
    )
    if current_profile:
        CONSOLE.print(
            f"[bright_black]Loaded existing profile draft from "
            f"{_resolve_profile_path(config_dir)}.[/]"
        )
    else:
        CONSOLE.print(
            f"[bright_black]No profile draft found at {_resolve_profile_path(config_dir)}. "
            "The agent will build one with you first.[/]"
        )

    attachment: SourceAttachment | None = None
    description: str | None = None

    if _ask_yes_no(
        "Can you provide a path to an image/file describing your timetable?",
        default=True,
    ):
        source_path = _prompt_non_empty("Enter path: ")
        resolved_source_path = _resolve_source_path_input(source_path)
        loaded, error = _load_source_attachment(resolved_source_path)
        if loaded is None:
            CONSOLE.print(f"[bold yellow]{error}[/]")
            description = _prompt_non_empty(
                "Please provide a short text description instead: "
            )
        else:
            if resolved_source_path != Path(source_path).expanduser():
                CONSOLE.print(
                    "[bold cyan]Detected file in a common folder:[/] "
                    f"[cyan]{resolved_source_path}[/]"
                )
            attachment = loaded
    else:
        description = _prompt_non_empty(
            "Please describe your weekly timetable and constraints: "
        )

    follow_up_details: list[str] = []
    conversation_history = ""
    summary_presented = False
    summary_confirmed = False
    latest_summary: str | None = None

    while True:
        merged_description = description.strip() if description else ""
        if follow_up_details:
            detail_lines = "\n".join(f"- {item}" for item in follow_up_details)
            extra_block = (
                "Additional details provided by the user in follow-up turns:\n"
                f"{detail_lines}"
            )
            merged_description = (
                f"{merged_description}\n\n{extra_block}"
                if merged_description
                else extra_block
            )

        user_prompt = render_build_user_prompt(
            config_dir,
            description=merged_description or None,
            attachment_name=attachment.path.name if attachment else None,
            conversation_history=conversation_history,
            profile_context=current_profile,
            summary_presented=summary_presented,
            summary_confirmed=summary_confirmed,
            latest_summary=latest_summary,
        )

        def _build_turn_validator(turn: AgentTurn) -> str | None:
            if (
                attachment is not None
                and attachment.image_base64 is not None
                and _turn_requests_manual_image_transcription(turn)
            ):
                return (
                    "Image is already attached through native vision input. "
                    "Do not claim you cannot see images or ask the user to "
                    "transcribe image details manually. Analyze the attachment "
                    "directly and only ask targeted clarifications if content "
                    "is ambiguous."
                )
            if turn.phase == "summary" and not turn.profile_markdown:
                return (
                    "Build flow violation: summary phase requires a complete "
                    "profile_markdown draft."
                )
            if turn.phase == "final" and not summary_presented:
                return (
                    "Build flow violation: final configuration was returned before "
                    "a summary phase. Provide a pure-text summary first."
                )
            if turn.phase == "final" and not summary_confirmed:
                return (
                    "Build flow violation: final configuration was returned before "
                    "the user confirmed the schedule summary."
                )
            if turn.phase == "final" and not turn.profile_markdown:
                return (
                    "Build flow violation: final phase requires profile_markdown "
                    "together with the TOML files."
                )
            return None

        turn, error = _request_agent_turn(
            client,
            BUILD_SYSTEM_PROMPT,
            user_prompt,
            attachment=attachment,
            turn_validator=_build_turn_validator,
            file_tools=file_tools,
        )
        if turn is None:
            CONSOLE.print(
                f"[bold red]Could not build schedule from model response:[/] {error}"
            )
            return 1

        _render_conversation_message(turn.conversation)
        updated_profile = _persist_profile_draft(
            config_dir,
            turn.profile_markdown,
        )
        if updated_profile is not None:
            current_profile = updated_profile

        if turn.needs_user_input:
            if turn.phase == "summary" and turn.schedule_summary:
                latest_summary = turn.schedule_summary
                summary_presented = True
                _render_schedule_summary(turn.schedule_summary)

            _render_missing_information(turn.missing_information)
            follow_up_prompt = (
                turn.question_to_user or "Please provide the missing information: "
            )
            user_answer = _prompt_non_empty(follow_up_prompt)
            if turn.phase == "summary":
                confirmation = _interpret_confirmation(user_answer)
                if confirmation is True:
                    summary_confirmed = True
                elif confirmation is False:
                    summary_confirmed = False
            follow_up_details.append(user_answer)
            conversation_history = _append_conversation_history(
                conversation_history,
                assistant_text=turn.conversation,
                user_text=user_answer,
            )
            continue

        if turn.bundle is None:
            CONSOLE.print(
                "[bold red]Model did not return schedule files for this turn.[/]"
            )
            return 1

        bundle = dict(turn.bundle)
        if "habits.toml" not in bundle and not (config_dir / "habits.toml").exists():
            bundle["habits.toml"] = DEFAULT_HABITS_TOML

        with CONSOLE.status(
            "[bold green]Applying generated schedule...[/]",
            spinner="line",
        ):
            _write_bundle(config_dir, bundle)
        break

    CONSOLE.print("[bold green]Initial schedule created.[/]")
    CONSOLE.print("[bold cyan]Run rmd view to visualize your schedule.[/]")

    if _ask_yes_no("Do you need to adjust this plan now?", default=True):
        return modify_schedule_agent(llm_config, config_dir)

    return 0

def setup_command(args) -> int:
    del args  # command has no CLI flags yet

    try:
        llm_config = ensure_llm_config()
    except KeyboardInterrupt:
        CONSOLE.print("[bold yellow]Setup cancelled by user.[/]")
        return 1
    except Exception as exc:
        CONSOLE.print(f"[bold red]Failed to initialize LLM config:[/] {exc}")
        return 1

    config_dir = _resolve_config_dir()
    config_dir.mkdir(parents=True, exist_ok=True)

    is_complete, reason = has_completed_configuration(config_dir)

    if is_complete:
        CONSOLE.print(
            f"[bold green]Detected an existing completed configuration in[/] "
            f"[cyan]{config_dir}[/]."
        )
        if _ask_yes_no("Do you want to modify existing schedules?", default=False):
            return modify_schedule_agent(llm_config, config_dir)
        CONSOLE.print("[bright_black]No changes made.[/]")
        return 0

    CONSOLE.print(
        f"[bold yellow]No valid completed configuration detected[/] ({reason})."
    )
    if _ask_yes_no("Do you want to build a new schedule?", default=True):
        return build_schedule_agent(llm_config, config_dir)

    CONSOLE.print("[bright_black]No changes made.[/]")
    return 0


__all__ = [
    "AgentTurn",
    "LLMClient",
    "LLMConfig",
    "LocalFileTools",
    "SourceAttachment",
    "ToolCall",
    "build_schedule_agent",
    "ensure_llm_config",
    "has_completed_configuration",
    "load_llm_config",
    "modify_schedule_agent",
    "save_llm_config",
    "setup_command",
]
