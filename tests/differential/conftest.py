"""Shared binary resolver and fixtures for differential parity tests."""

from __future__ import annotations

import os
import shutil
import subprocess
from functools import cache
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


@cache
def resolve_rust_binary() -> Path:
    if os.environ.get("ANKI_SKIP_RUST_ORACLE") == "1":
        pytest.skip("ANKI_SKIP_RUST_ORACLE=1")

    env_bin = os.environ.get("ANKI_RUST_BIN")
    if env_bin is not None:
        if not env_bin.strip():
            raise FileNotFoundError("ANKI_RUST_BIN is set to empty string or whitespace.")
        candidate = Path(env_bin)
        if candidate.is_file():
            return candidate
        raise FileNotFoundError(
            f"ANKI_RUST_BIN is set to '{env_bin}', but the file does not exist or is not a regular file."
        )

    exe = "anki-workflow-mcp.exe" if os.name == "nt" else "anki-workflow-mcp"
    candidate = ROOT / "target" / "debug" / exe
    if candidate.is_file():
        return candidate

    if shutil.which("cargo") is None:
        pytest.skip("Rust cargo unavailable in this environment")

    subprocess.run(
        ["cargo", "build", "--locked", "--features", "oracle", "-p", "anki-workflow-mcp"],
        cwd=ROOT,
        check=True,
    )
    candidate = ROOT / "target" / "debug" / exe
    if candidate.is_file():
        return candidate
    raise FileNotFoundError(f"cargo build completed but binary not found at {candidate}")


@pytest.fixture(scope="session")
def rust_binary() -> Path:
    return resolve_rust_binary()
