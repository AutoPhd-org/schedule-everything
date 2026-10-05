"""
TOML Writer - Minimal serializer for settings.toml round-trips.

Handles the flat table structure used by settings.toml: each section
contains only scalar values (str, int, bool) or arrays of scalars.
This avoids adding tomli_w as an external dependency.
"""

from pathlib import Path
from typing import Any

import json
import re
import tomllib


def load_toml_raw(path: Path) -> dict[str, Any]:
    """Load a TOML file and return the raw parsed dict."""
    with open(path, "rb") as f:
        return tomllib.load(f)


def _format_value(value: Any) -> str:
    """Format a single Python value as a TOML literal."""
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        return str(value)
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, list):
        items = ", ".join(_format_value(item) for item in value)
        return f"[{items}]"
    raise TypeError(f"Unsupported TOML value type: {type(value).__name__}")


def dump_toml(data: dict[str, dict[str, Any]], path: Path) -> None:
    """Write a dict of {section: {key: value}} to a TOML file.

    Sections are written in the order they appear in the dict.
    A blank line separates consecutive sections for readability.
    """
    lines: list[str] = []
    for i, (section, values) in enumerate(data.items()):
        if i > 0:
            lines.append("")
        lines.append(f"[{section}]")
        if not isinstance(values, dict):
            continue
        for key, value in values.items():
            quoted_key = key if re.fullmatch(r"[A-Za-z0-9_-]+", str(key)) else json.dumps(str(key), ensure_ascii=False)
            lines.append(f"{quoted_key} = {_format_value(value)}")

    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
