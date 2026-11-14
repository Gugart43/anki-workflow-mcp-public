"""tests/test_staging_concurrency.py — Concurrency and locking tests for batch_patch_draft."""

from __future__ import annotations

import asyncio
import json
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest
from filelock import FileLock

from anki_mcp.server import mcp


def call(name: str, **kwargs):
    return asyncio.run(mcp.get_tool(name)).fn(**kwargs)


def _init_staging_file(path: Path) -> None:
    card1 = {"tipo": "QA", "enunciado_ou_sentenca": "Card 0 original", "resposta": "Ans 0"}
    card2 = {"tipo": "QA", "enunciado_ou_sentenca": "Card 1 original", "resposta": "Ans 1"}
    path.write_text(
        json.dumps(card1) + "\n" + json.dumps(card2) + "\n",
        encoding="utf-8",
    )


def test_batch_patch_draft_lock_timeout_structured_error(tmp_path: Path):
    """When a staging file lock is held, batch_patch_draft returns structured GOV.LOCK error on timeout."""
    staging_file = tmp_path / "drafts.jsonl"
    _init_staging_file(staging_file)
    lock_file = staging_file.resolve().with_name(f"{staging_file.resolve().name}.lock")

    with FileLock(str(lock_file), timeout=5):
        res = call(
            "batch_patch_draft",
            staging_path=str(staging_file),
            card_index=0,
            patch_json=json.dumps({"enunciado_ou_sentenca": "Should fail timeout"}),
            timeout_s=0.2,
        )

    assert res.get("status") == "ERROR"
    assert res.get("code") == "GOV.LOCK"
    assert res.get("success") is False
    assert "GOV.LOCK" in res.get("message", "") or "GOV.LOCK" in res.get("error", "")
    assert "action" in res


def test_batch_patch_draft_reentrant_sequential_patches_no_deadlock(tmp_path: Path):
    """Multiple sequential patches on the same file in the same thread execute cleanly without deadlock."""
    staging_file = tmp_path / "drafts.jsonl"
    _init_staging_file(staging_file)

    r0 = call(
        "batch_patch_draft",
        staging_path=str(staging_file),
        card_index=0,
        patch_json=json.dumps({"enunciado_ou_sentenca": "Card 0 seq patch"}),
    )
    r1 = call(
        "batch_patch_draft",
        staging_path=str(staging_file),
        card_index=1,
        patch_json=json.dumps({"enunciado_ou_sentenca": "Card 1 seq patch"}),
    )

    assert r0.get("success") is True
    assert r1.get("success") is True
    lines = [
        json.loads(line)
        for line in staging_file.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    assert lines[0]["enunciado_ou_sentenca"] == "Card 0 seq patch"
    assert lines[1]["enunciado_ou_sentenca"] == "Card 1 seq patch"


def test_batch_patch_draft_threads_concurrency_preserves_both_patches(tmp_path: Path):
    """Concurrent threads patching distinct cards in the same staging file preserve both updates."""
    staging_file = tmp_path / "drafts.jsonl"
    _init_staging_file(staging_file)

    def worker(card_idx: int, text: str):
        return call(
            "batch_patch_draft",
            staging_path=str(staging_file),
            card_index=card_idx,
            patch_json=json.dumps({"enunciado_ou_sentenca": text}),
            timeout_s=5.0,
        )

    with ThreadPoolExecutor(max_workers=2) as executor:
        f0 = executor.submit(worker, 0, "Card 0 patched by thread")
        f1 = executor.submit(worker, 1, "Card 1 patched by thread")
        r0 = f0.result(timeout=10)
        r1 = f1.result(timeout=10)

    assert r0.get("success") is True, r0
    assert r1.get("success") is True, r1

    lines = [
        json.loads(line)
        for line in staging_file.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    assert len(lines) == 2
    assert lines[0]["enunciado_ou_sentenca"] == "Card 0 patched by thread"
    assert lines[1]["enunciado_ou_sentenca"] == "Card 1 patched by thread"


def test_batch_patch_draft_processes_concurrency_preserves_both_patches(tmp_path: Path):
    """Concurrent OS processes patching distinct cards in the same staging file preserve both updates."""
    staging_file = tmp_path / "drafts.jsonl"
    _init_staging_file(staging_file)

    script = """
import asyncio, json, sys
from anki_mcp.server import mcp

def call(name, **kwargs):
    return asyncio.run(mcp.get_tool(name)).fn(**kwargs)

staging_path = sys.argv[1]
card_index = int(sys.argv[2])
text = sys.argv[3]

res = call(
    "batch_patch_draft",
    staging_path=staging_path,
    card_index=card_index,
    patch_json=json.dumps({"enunciado_ou_sentenca": text}),
    timeout_s=5.0,
)
if not res.get("success"):
    print(f"FAILED: {res}", file=sys.stderr)
    sys.exit(1)
"""

    p0 = subprocess.Popen(
        [sys.executable, "-c", script, str(staging_file), "0", "Card 0 patched by process"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    p1 = subprocess.Popen(
        [sys.executable, "-c", script, str(staging_file), "1", "Card 1 patched by process"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )

    out0, err0 = p0.communicate(timeout=15)
    out1, err1 = p1.communicate(timeout=15)

    assert p0.returncode == 0, f"Process 0 failed: stdout={out0}, stderr={err0}"
    assert p1.returncode == 0, f"Process 1 failed: stdout={out1}, stderr={err1}"

    lines = [
        json.loads(line)
        for line in staging_file.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    assert len(lines) == 2
    assert lines[0]["enunciado_ou_sentenca"] == "Card 0 patched by process"
    assert lines[1]["enunciado_ou_sentenca"] == "Card 1 patched by process"


def test_batch_patch_draft_resolved_path_locking(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    """Lock on resolved file path blocks calls using relative paths to the same staging file."""
    monkeypatch.chdir(tmp_path)
    rel_staging = Path("drafts.jsonl")
    _init_staging_file(rel_staging)

    # Lock using absolute resolved path lock file
    abs_lock = rel_staging.resolve().with_name(f"{rel_staging.resolve().name}.lock")
    with FileLock(str(abs_lock), timeout=5):
        res = call(
            "batch_patch_draft",
            staging_path=str(rel_staging),  # relative path!
            card_index=0,
            patch_json=json.dumps({"enunciado_ou_sentenca": "Should fail via relative"}),
            timeout_s=0.2,
        )

    assert res.get("status") == "ERROR"
    assert res.get("code") == "GOV.LOCK"
    assert res.get("success") is False
