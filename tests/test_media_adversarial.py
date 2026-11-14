"""Adversarial regression tests for media preflight and rollback."""

import asyncio
import json
from pathlib import Path

import pytest
from fastmcp import FastMCP

from anki_mcp.tools.media import register_media_tools


def setup(tmp_path, items):
    src = tmp_path / "src"
    src.mkdir()
    dst = tmp_path / "dst"
    dst.mkdir()
    for filename, data in [("a.png", b"a"), ("b.png", b"b")]:
        (src / filename).write_bytes(data)
    catalog = src / "catalog.json"
    catalog.write_text(json.dumps({"slug": "audit", "items": items}), encoding="utf-8")
    mcp = FastMCP("adversarial")
    register_media_tools(mcp)
    return asyncio.run(mcp.get_tool("image_sync")).fn, catalog, dst


def test_missing_source_blocks_entire_apply(tmp_path):
    fn, cat, dst = setup(
        tmp_path,
        [
            {"filename": "a.png", "caption": "Imagem válida"},
            {"filename": "missing.png", "caption": "Imagem ausente"},
        ],
    )
    res = fn(str(cat), apply=True, collection_media_dir=str(dst))
    assert res["status"] == "ERROR", res
    assert res["code"] == "MEDIA.SOURCE_MISSING"
    assert not (dst / "audit_a.png").exists()


def test_media_rollback_failure_is_reported(tmp_path, monkeypatch):
    import shutil

    fn, cat, dst = setup(
        tmp_path,
        [
            {"filename": "a.png", "caption": "Imagem válida"},
            {"filename": "b.png", "caption": "Imagem válida"},
        ],
    )
    copy = shutil.copy2
    unlink = Path.unlink

    def fail_copy(src, target, *args, **kwargs):
        if Path(src).name == "b.png":
            raise OSError("full")
        return copy(src, target, *args, **kwargs)

    def fail_unlink(self, *args, **kwargs):
        if self.name == "audit_a.png":
            raise PermissionError("rollback blocked")
        return unlink(self, *args, **kwargs)

    monkeypatch.setattr(shutil, "copy2", fail_copy)
    monkeypatch.setattr(Path, "unlink", fail_unlink)
    res = fn(str(cat), apply=True, collection_media_dir=str(dst))
    assert res["status"] == "ERROR"
    assert res["rolled_back_count"] == 0, res
    assert res["rollback_errors"]
    assert res["copied"] == 1


def test_intra_batch_normalized_collision_blocks_copy(tmp_path):
    fn, cat, dst = setup(
        tmp_path,
        [
            {"filename": "diag 1.png", "caption": "Imagem clínica"},
            {"filename": "diag_1.png", "caption": "Imagem clínica"},
        ],
    )
    (cat.parent / "diag 1.png").write_bytes(b"first")
    (cat.parent / "diag_1.png").write_bytes(b"second")
    res = fn(str(cat), apply=True, collection_media_dir=str(dst))
    assert res.get("code") == "MEDIA.COLLISION", res
    assert not (dst / "audit_diag_1.png").exists()


def test_overlapping_sync_waits_before_inspection(tmp_path, monkeypatch):
    import shutil
    import threading
    from concurrent.futures import ThreadPoolExecutor

    fn, cat, dst = setup(
        tmp_path,
        [
            {"filename": "a.png", "caption": "Imagem clínica"},
            {"filename": "b.png", "caption": "Imagem clínica"},
        ],
    )
    other = cat.parent / "other.json"
    other.write_text(
        json.dumps(
            {"slug": "audit", "items": [{"filename": "a.png", "caption": "Imagem clínica"}]}
        ),
        encoding="utf-8",
    )
    entered = threading.Event()
    release = threading.Event()
    real = shutil.copy2

    def delayed(src, target, *args, **kwargs):
        if Path(src).name == "b.png":
            entered.set()
            assert release.wait(4)
            raise OSError("batch failure")
        return real(src, target, *args, **kwargs)

    monkeypatch.setattr(shutil, "copy2", delayed)
    with ThreadPoolExecutor(2) as pool:
        first = pool.submit(fn, str(cat), apply=True, collection_media_dir=str(dst))
        assert entered.wait(4)
        second = pool.submit(fn, str(other), apply=True, collection_media_dir=str(dst))
        try:
            # It must not report success for files owned by an unfinished transaction.
            import concurrent.futures

            try:
                second.result(timeout=0.2)
            except concurrent.futures.TimeoutError:
                pass
            else:
                pytest.fail("overlapping sync inspected uncommitted media")
        finally:
            release.set()
        assert first.result()["status"] == "ERROR"
        assert second.result()["status"] == "OK"
    assert (dst / "audit_a.png").read_bytes() == b"a"


def test_media_lock_is_shared_with_other_process(tmp_path):
    import subprocess
    import sys

    from filelock import FileLock

    _, cat, dst = setup(tmp_path, [{"filename": "a.png", "caption": "Imagem clínica"}])
    script = """
import asyncio, json, sys
from fastmcp import FastMCP
from anki_mcp.tools.media import register_media_tools
mcp = FastMCP('child'); register_media_tools(mcp)
fn = asyncio.run(mcp.get_tool('image_sync')).fn
print(json.dumps(fn(sys.argv[1], apply=True, collection_media_dir=sys.argv[2])))
"""
    with FileLock(str(dst / ".anki-media-sync.lock")):
        child = subprocess.run(
            [sys.executable, "-c", script, str(cat), str(dst)],
            capture_output=True,
            text=True,
            timeout=20,
        )
    assert child.returncode == 0, child.stderr
    assert json.loads(child.stdout)["code"] == "GOV.LOCK"
    assert not (dst / "audit_a.png").exists()
