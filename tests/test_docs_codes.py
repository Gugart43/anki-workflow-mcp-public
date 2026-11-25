"""Contract tests for ``scripts/check_docs_codes.py``.

The script is the guard that keeps ``docs/`` from citing diagnostic codes the
binary can never emit. These tests exercise it end to end through a subprocess,
so what is verified is exactly the exit status a CI gate would see.
"""

from __future__ import annotations

import subprocess
import sys
import textwrap
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "check_docs_codes.py"

#: Minimal stand-in for ``src/anki_mcp/core/codes.py``: the script only needs
#: ``all_codes()`` (and optionally ``RETIRED_ALIASES``).
MINIMAL_CODES_PY = textwrap.dedent(
    '''
    """Minimal registry fixture."""

    _CODES = ["FMT.RAW_PIPE", "CLOZE.NESTED"]

    RETIRED_ALIASES = {"QA.INTERFERENCE_WARN": "CARD.INTERFERENCE_PAIR_WARN"}


    def all_codes() -> list[str]:
        return list(_CODES)
    '''
).lstrip()

#: One Rust literal, proving the ``apps/``+``crates/`` harvest feeds the catalog.
MINIMAL_RUST_RS = 'pub const CODE: &str = "EXPORT.BATCH_CONFLICT";\n'


def run_script(*args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(SCRIPT), *args],
        cwd=str(cwd or REPO_ROOT),
        capture_output=True,
        text=True,
        timeout=300,
    )


def make_fixture(root: Path) -> None:
    """Create a miniature repo the checker can scan: registry, Rust, docs."""
    codes_py = root / "src" / "anki_mcp" / "core" / "codes.py"
    codes_py.parent.mkdir(parents=True)
    codes_py.write_text(MINIMAL_CODES_PY, encoding="utf-8")

    rust = root / "crates" / "anki-domain" / "src" / "lib.rs"
    rust.parent.mkdir(parents=True)
    rust.write_text(MINIMAL_RUST_RS, encoding="utf-8")


def test_repository_docs_cite_only_registered_codes() -> None:
    """Every diagnostic code referenced in the real docs must exist in the catalog."""
    result = run_script()
    assert result.returncode == 0, (
        f"check_docs_codes.py failed\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )
    assert "OK:" in result.stdout


def test_registered_and_rust_codes_pass(tmp_path: Path) -> None:
    make_fixture(tmp_path)
    (tmp_path / "README.md").write_text(
        "Python registry: `FMT.RAW_PIPE`. Rust literal: `EXPORT.BATCH_CONFLICT`.\n",
        encoding="utf-8",
    )
    result = run_script("--root", str(tmp_path))
    assert result.returncode == 0, result.stderr
    assert "unknown=0" in result.stdout or "OK:" in result.stdout


def test_unregistered_code_fails_with_locations(tmp_path: Path) -> None:
    make_fixture(tmp_path)
    doc = tmp_path / "docs" / "note.md"
    doc.parent.mkdir(parents=True)
    doc.write_text("Esse código não existe: `FMT.NOT_REGISTERED`.\n", encoding="utf-8")

    result = run_script("--root", str(tmp_path))
    assert result.returncode == 1
    assert "FMT.NOT_REGISTERED" in result.stderr
    assert "docs/note.md:1" in result.stderr


def test_fenced_code_blocks_are_skipped(tmp_path: Path) -> None:
    """Sample tool output inside a fence is illustrative, not a code reference."""
    make_fixture(tmp_path)
    (tmp_path / "README.md").write_text(
        'Real code: `FMT.RAW_PIPE`.\n\n```json\n{"code": "FMT.NOT_REGISTERED"}\n```\n',
        encoding="utf-8",
    )
    result = run_script("--root", str(tmp_path))
    assert result.returncode == 0, result.stderr


def test_historical_migration_docs_are_skipped(tmp_path: Path) -> None:
    """``docs/migration/`` records the retired Python names verbatim."""
    make_fixture(tmp_path)
    hist = tmp_path / "docs" / "migration" / "creation-pipeline-wave2.md"
    hist.parent.mkdir(parents=True)
    hist.write_text("* `TAG.EMPTY`: cartão sem etiquetas (legado).\n", encoding="utf-8")

    result = run_script("--root", str(tmp_path))
    assert result.returncode == 0, result.stderr


def test_contributing_is_scanned(tmp_path: Path) -> None:
    """``CONTRIBUTING.md`` is part of the documented surface and must be checked."""
    make_fixture(tmp_path)
    (tmp_path / "CONTRIBUTING.md").write_text("Emite `CLOZE.NOT_REAL`.\n", encoding="utf-8")
    result = run_script("--root", str(tmp_path))
    assert result.returncode == 1
    assert "CLOZE.NOT_REAL" in result.stderr
