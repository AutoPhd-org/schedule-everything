"""
Shared dataclasses for the setup-agent workflow.

These types are imported across the setup-agent submodules and re-exported
through compatibility facades.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


@dataclass
class LLMConfig:
    """Optional model override for the pi-backed agent.

    Credentials and model selection are owned by pi itself (its own auth
    store and provider env vars). This config only carries an optional
    ``--model`` override passed straight through to the pi CLI.
    """

    model: str | None = None


@dataclass
class SourceAttachment:
    path: Path
    mime_type: str
    text_content: str | None = None
    image_base64: str | None = None


@dataclass
class AgentTurn:
    phase: str
    conversation: str
    needs_user_input: bool
    profile_markdown: str | None = None
    question_to_user: str | None = None
    missing_information: list[str] = field(default_factory=list)
    actions: list[str] = field(default_factory=list)
    schedule_summary: str | None = None
    bundle: dict[str, str] | None = None


@dataclass
class ToolCall:
    call_id: str
    name: str
    arguments: dict[str, Any]
