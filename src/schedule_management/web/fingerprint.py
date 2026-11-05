"""Identify the code a browser server runs so a stale server can be replaced.

A running server keeps the Python modules it imported at startup but reads the
bundled frontend from disk on every request. After an update, an old server
would pair the new frontend with old API commands, so the launcher compares
the server's fingerprint with the code currently on disk.
"""
from __future__ import annotations

import hashlib
from pathlib import Path

PACKAGE_DIR = Path(__file__).resolve().parent.parent


def _code_files(package: Path):
    for path in package.rglob("*"):
        if not path.is_file() or "__pycache__" in path.parts:
            continue
        relative = path.relative_to(package)
        if path.suffix == ".py" or relative.parts[:2] == ("web", "static"):
            yield relative, path


def code_fingerprint(package: Path = PACKAGE_DIR) -> str:
    """Hash the package's Python sources and bundled frontend."""
    digest = hashlib.sha256()
    for relative, path in sorted(_code_files(package)):
        try:
            content = path.read_bytes()
        except OSError:
            continue
        digest.update(relative.as_posix().encode())
        digest.update(b"\0")
        digest.update(hashlib.sha256(content).digest())
    return digest.hexdigest()[:16]
