"""Shared binary resolver and fixtures for differential parity tests."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
from functools import cache
from pathlib import Path
from typing import Any

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


class OracleRunner:
    """Consolidated runner for Rust oracle subprocess requests."""

    def __init__(self, binary: Path) -> None:
        self.binary = binary

    def run_raw_process(
        self, requests: list[dict[str, Any]], check: bool = True
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [str(self.binary), "oracle"],
            input=json.dumps(requests, ensure_ascii=False),
            text=True,
            encoding="utf-8",
            capture_output=True,
            check=check,
        )

    def raw(self, requests: list[dict[str, Any]]) -> list[dict[str, Any]]:
        proc = self.run_raw_process(requests, check=True)
        return json.loads(proc.stdout)

    def batch(self, requests: list[dict[str, Any]]) -> list[dict[str, Any]]:
        return self.raw(requests)

    def __call__(self, op: str, **kwargs: Any) -> dict[str, Any]:
        results = self.raw([{"op": op, **kwargs}])
        return results[0]


@pytest.fixture(scope="session")
def run_oracle(rust_binary: Path) -> OracleRunner:
    return OracleRunner(rust_binary)
