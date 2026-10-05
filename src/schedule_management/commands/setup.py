"""Compatibility facade for the `rmd setup` command.

The implementation lives in the `setup_agent` subpackage. This module keeps
existing import paths stable for CLI wiring and tests.
"""

from __future__ import annotations

from schedule_management.commands.setup_agent import workflow as _workflow
from schedule_management.i18n import _t

# Re-export data structures and agent helpers used by tests/callers.
LLMConfig = _workflow.LLMConfig
SourceAttachment = _workflow.SourceAttachment
AgentTurn = _workflow.AgentTurn
ToolCall = _workflow.ToolCall
LocalFileTools = _workflow.LocalFileTools
LLMClient = _workflow.LLMClient

# Re-export internal helpers patched in tests.
_resolve_config_dir = _workflow._resolve_config_dir
_resolve_llm_config_path = _workflow._resolve_llm_config_path
_ask_yes_no = _workflow._ask_yes_no
_parse_agent_turn = _workflow._parse_agent_turn

# Re-export setup workflow operations.
has_completed_configuration = _workflow.has_completed_configuration
load_llm_config = _workflow.load_llm_config
save_llm_config = _workflow.save_llm_config
ensure_llm_config = _workflow.ensure_llm_config
build_schedule_agent = _workflow.build_schedule_agent
modify_schedule_agent = _workflow.modify_schedule_agent

CONSOLE = _workflow.CONSOLE


def setup_command(args) -> int:
    """Entry point for the `rmd setup` command."""
    del args  # command has no CLI flags yet

    try:
        llm_config = ensure_llm_config()
    except KeyboardInterrupt:
        CONSOLE.print("[bold yellow]" + _t("Setup cancelled by user.") + "[/]")
        return 1
    except Exception as exc:
        CONSOLE.print("[bold red]" + _t("Failed to initialize LLM config:") + "[/] " + str(exc))
        return 1

    config_dir = _resolve_config_dir()
    config_dir.mkdir(parents=True, exist_ok=True)

    is_complete, reason = has_completed_configuration(config_dir)

    try:
        if is_complete:
            CONSOLE.print(
                "[bold green]" + _t("Detected an existing completed configuration in") + "[/] "
                f"[cyan]{config_dir}[/]."
            )
            if _ask_yes_no(_t("Do you want to modify existing schedules?"), default=False):
                return modify_schedule_agent(llm_config, config_dir)
            CONSOLE.print("[bright_black]" + _t("No changes made.") + "[/]")
            return 0

        CONSOLE.print(
            "[bold yellow]" + _t("No valid completed configuration detected") + f"[/] ({reason})."
        )
        if _ask_yes_no(_t("Do you want to build a new schedule?"), default=True):
            return build_schedule_agent(llm_config, config_dir)

        CONSOLE.print("[bright_black]" + _t("No changes made.") + "[/]")
        return 0
    except (EOFError, KeyboardInterrupt):
        CONSOLE.print("[bold yellow]" + _t("Setup cancelled by user.") + "[/]")
        return 1


def __getattr__(name: str):
    """Fallback to the moved implementation for backwards compatibility."""
    return getattr(_workflow, name)


__all__ = [
    "LLMConfig",
    "has_completed_configuration",
    "load_llm_config",
    "save_llm_config",
    "ensure_llm_config",
    "build_schedule_agent",
    "modify_schedule_agent",
    "setup_command",
]
