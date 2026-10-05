"""
Interactive config helpers for the setup agent.

This module owns the persisted optional-model settings file plus the terminal
prompts shared by the build/modify flows. Credentials and model selection are
owned by the pi CLI; we only carry an optional ``--model`` override.
"""

from __future__ import annotations

import json
import os
import stat
from pathlib import Path
from typing import Any

import tomllib
from rich.panel import Panel

from schedule_management.config_layout import resolve_active_config_dir
from schedule_management.commands.setup_agent.console import CONSOLE
from schedule_management.commands.setup_agent.models import LLMConfig

REQUIRED_CONFIG_FILES = (
    "settings.toml",
    "odd_weeks.toml",
    "even_weeks.toml",
    "habits.toml",
)


def _resolve_config_dir() -> Path:
    return resolve_active_config_dir(create=True)


def _resolve_llm_config_path() -> Path:
    override = os.getenv("REMINDER_LLM_CONFIG_PATH")
    if override:
        return Path(override).expanduser().resolve()
    return (Path.home() / ".schedule_management" / "llm.toml").resolve()


def _ask_yes_no(prompt: str, *, default: bool) -> bool:
    suffix = "[Y/n]" if default else "[y/N]"
    while True:
        try:
            answer = (
                CONSOLE.input(f"[bold cyan]{prompt}[/] [bright_black]{suffix}[/]: ")
                .strip()
                .lower()
            )
        except EOFError:
            raise EOFError("Setup input closed") from None

        if answer == "":
            return default
        if answer in {"y", "yes"}:
            return True
        if answer in {"n", "no"}:
            return False
        CONSOLE.print("[bold yellow]Please answer with 'y' or 'n'.[/]")


def _prompt_non_empty(prompt: str, *, secret: bool = False) -> str:
    del secret  # retained for signature compatibility with call sites
    while True:
        try:
            value = CONSOLE.input(f"[bold cyan]{prompt}[/]")
        except EOFError:
            raise EOFError("Setup input closed") from None
        value = value.strip()
        if value:
            return value
        CONSOLE.print("[bold yellow]This value cannot be empty.[/]")


def _interpret_confirmation(answer: str) -> bool | None:
    normalized = answer.strip().lower()
    if not normalized:
        return None

    affirmative = {
        "y",
        "yes",
        "ok",
        "okay",
        "approve",
        "approved",
        "confirm",
        "confirmed",
        "looks good",
        "good",
        "sounds good",
    }
    negative = {
        "n",
        "no",
        "reject",
        "rejected",
        "not yet",
        "needs changes",
        "change",
        "adjust",
    }

    if normalized in affirmative:
        return True
    if normalized in negative:
        return False
    return None


def _parse_llm_config(raw: dict[str, Any]) -> LLMConfig | None:
    """Build an LLMConfig from a parsed llm.toml table.

    Only the optional ``model`` key is honored; any legacy ``vendor`` /
    ``api_key`` / ``base_url`` keys are ignored so existing files keep working
    without forcing a migration. Returns ``None`` only when the table shape is
    fundamentally invalid (not a dict / non-string model).
    """
    model_raw = raw.get("model")
    if model_raw is None:
        return LLMConfig(model=None)
    if not isinstance(model_raw, str):
        return None
    return LLMConfig(model=model_raw.strip() or None)


def load_llm_config(path: Path) -> LLMConfig | None:
    if not path.exists():
        return None

    try:
        with open(path, "rb") as handle:
            raw = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError):
        return None

    if not isinstance(raw, dict):
        return None

    return _parse_llm_config(raw)


def _toml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def save_llm_config(path: Path, config: LLMConfig) -> None:
    lines: list[str] = ["# Optional pi --model override. Credentials and model"]
    lines.append("# selection are managed by pi itself; see `pi --help`.")
    if config.model:
        lines.append(f"model = {_toml_string(config.model)}")
    else:
        lines.append("# model = \"provider/model-id\"")

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")

    try:
        path.chmod(stat.S_IRUSR | stat.S_IWUSR)
    except OSError:
        pass


def ensure_llm_config() -> LLMConfig:
    """Return the optional model override, if any.

    pi owns credentials and model selection, so there is nothing to prompt
    for here. We simply read the optional override from disk; a missing or
    empty file means "let pi choose".
    """
    llm_path = _resolve_llm_config_path()
    existing = load_llm_config(llm_path)
    if existing is not None:
        return existing

    config = LLMConfig(model=None)
    save_llm_config(llm_path, config)
    CONSOLE.print(
        Panel.fit(
            "No model override set. pi will use its own credentials and default "
            f"model. Optional overrides go in [cyan]{llm_path}[/].",
            title="Setup",
            border_style="yellow",
        )
    )
    return config


def has_completed_configuration(config_dir: Path) -> tuple[bool, str]:
    for file_name in REQUIRED_CONFIG_FILES:
        candidate = config_dir / file_name
        if not candidate.exists():
            return False, f"Missing {file_name}"

        try:
            with open(candidate, "rb") as handle:
                tomllib.load(handle)
        except (OSError, tomllib.TOMLDecodeError) as exc:
            return False, f"Invalid TOML in {file_name}: {exc}"

    return True, "ok"
