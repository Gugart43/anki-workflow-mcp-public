"""tests/test_media_transactional.py — TDD tests for transactional media sync,
validation pre-checks, collision blocking, temp+replace copy, and batch rollback.
"""

from __future__ import annotations

import asyncio

import pytest
from fastmcp import FastMCP

from anki_mcp.tools.media import (
    _collection_media,
    register_media_tools,
)


def _get_tools():
    mcp = FastMCP("test-media-transactional")
    register_media_tools(mcp)
    sync_tool = asyncio.run(mcp.get_tool("image_sync")).fn
    report_tool = asyncio.run(mcp.get_tool("image_report")).fn
    return sync_tool, report_tool


def test_collection_media_resolution_order(tmp_path, monkeypatch):
    """Test explicit collection_media_dir and ANKI_COLLECTION_MEDIA resolution."""
    # 1. Explicit directory that exists
    explicit_dir = tmp_path / "explicit_media"
    explicit_dir.mkdir()
    assert _collection_media(str(explicit_dir)) == explicit_dir

    # 2. Explicit directory that does not exist -> raises FileNotFoundError
    missing_dir = tmp_path / "missing_media"
    with pytest.raises(FileNotFoundError, match="collection_media_dir not found"):
        _collection_media(str(missing_dir))

    # 3. Environment variable ANKI_COLLECTION_MEDIA when no explicit dir is passed
    env_dir = tmp_path / "env_media"
    env_dir.mkdir()
    monkeypatch.setenv("ANKI_COLLECTION_MEDIA", str(env_dir))
    assert _collection_media() == env_dir

    # 4. Environment variable points to non-existent dir -> raises FileNotFoundError
    monkeypatch.setenv("ANKI_COLLECTION_MEDIA", str(tmp_path / "nonexistent_env"))
    with pytest.raises(FileNotFoundError, match="ANKI_COLLECTION_MEDIA directory not found"):
        _collection_media()


def test_image_sync_nonexistent_catalog_and_destination(tmp_path):
    """Test image_sync returns structured errors for nonexistent catalog or collection."""
    sync_fn, _ = _get_tools()

    coll_dir = tmp_path / "collection.media"
    coll_dir.mkdir()

    # Nonexistent catalog
    res_cat = sync_fn("nonexistent_catalog.csv", collection_media_dir=str(coll_dir))
    assert res_cat["status"] == "ERROR"
    assert res_cat["code"] == "MEDIA.CATALOG_ERROR"
    assert "File not found" in res_cat["message"]

    # Valid catalog, but nonexistent collection_media_dir
    cat_file = tmp_path / "catalog.csv"
    cat_file.write_text("filename|caption|slug\nfig1.png|Cap|neuro\n", encoding="utf-8")

    nonexistent_coll = tmp_path / "does_not_exist"
    res_coll = sync_fn(str(cat_file), collection_media_dir=str(nonexistent_coll))
    assert res_coll["status"] == "ERROR"
    assert res_coll["code"] == "MEDIA.CATALOG_ERROR"
    assert "collection_media_dir not found" in res_coll["message"]


def test_image_report_with_explicit_collection_media(tmp_path):
    """Test image_report accepts and queries explicit collection_media_dir."""
    _, report_fn = _get_tools()
    coll_dir = tmp_path / "coll_report"
    coll_dir.mkdir()

    cat_file = tmp_path / "catalog.csv"
    cat_file.write_text("filename|caption|slug\nfig1.png|Cap|neuro\n", encoding="utf-8")

    res = report_fn(catalog_paths=str(cat_file), collection_media_dir=str(coll_dir))
    assert res["status"] == "OK"
    assert res["collection"] == str(coll_dir)
    assert res["grand_total"] == 1
    assert res["grand_missing"] == 1


def test_image_sync_dry_run_and_noise_default(tmp_path):
    """Test dry-run (apply=False) skips noise by default (copy_noise=False) and copies nothing."""
    sync_fn, _ = _get_tools()

    coll_dir = tmp_path / "coll_dry"
    coll_dir.mkdir()

    source_dir = tmp_path / "images_neuro"
    source_dir.mkdir()
    (source_dir / "brain.png").write_text("clinical_data", encoding="utf-8")
    (source_dir / "noise.png").write_text("noise_data", encoding="utf-8")

    catalog_file = source_dir / "catalog.csv"
    catalog_file.write_text(
        "filename|caption|slug\n"
        "brain.png|Ressonância magnética de encéfalo|neuro\n"
        "noise.png|Figura 2 da página 15|neuro\n",
        encoding="utf-8",
    )

    # 1. Default copy_noise=False in dry-run
    res = sync_fn(str(catalog_file), apply=False, collection_media_dir=str(coll_dir))
    assert res["status"] == "OK"
    assert res["copied"] == 0
    assert res["skipped_noise"] == 1
    assert res["missing"] == 1
    assert res["synced"] == 0
    assert res["sync_ratio"] == 0.0
    assert not (coll_dir / "neuro_brain.png").exists()
    assert not (coll_dir / "neuro_noise.png").exists()

    # 2. copy_noise=True includes noise
    res_noise = sync_fn(
        str(catalog_file),
        apply=False,
        copy_noise=True,
        collection_media_dir=str(coll_dir),
    )
    assert res_noise["status"] == "OK"
    assert res_noise["skipped_noise"] == 0
    assert res_noise["missing"] == 2


def test_image_sync_collisions_block_any_copy_and_fix_ratio(tmp_path):
    """Test that hash collisions block any file copying, return structured error, and fix ratio."""
    sync_fn, _ = _get_tools()

    coll_dir = tmp_path / "coll_collision"
    coll_dir.mkdir()

    source_dir = tmp_path / "images_neuro"
    source_dir.mkdir()

    # 1. Existing file in collection with old content
    existing_in_coll = coll_dir / "neuro_fig1.png"
    existing_in_coll.write_text("old_content_in_collection", encoding="utf-8")

    # Source has different content for fig1.png -> collision!
    (source_dir / "fig1.png").write_text("new_conflicting_content", encoding="utf-8")

    # Source has fig2.png which is missing from collection
    (source_dir / "fig2.png").write_text("valid_missing_content", encoding="utf-8")

    # Source has fig3.png which matches collection exactly -> synced
    (coll_dir / "neuro_fig3.png").write_text("matching_content", encoding="utf-8")
    (source_dir / "fig3.png").write_text("matching_content", encoding="utf-8")

    catalog_file = source_dir / "catalog.csv"
    catalog_file.write_text(
        "filename|caption|slug\n"
        "fig1.png|Fig 1 collision|neuro\n"
        "fig2.png|Fig 2 missing|neuro\n"
        "fig3.png|Fig 3 matching|neuro\n",
        encoding="utf-8",
    )

    # A. Dry run with collisions -> status ERROR, code MEDIA.COLLISION, ratio includes collisions!
    res_dry = sync_fn(str(catalog_file), apply=False, collection_media_dir=str(coll_dir))
    assert res_dry["status"] == "ERROR"
    assert res_dry["code"] == "MEDIA.COLLISION"
    assert len(res_dry["collisions"]) == 1
    assert res_dry["collisions"][0]["namespaced"] == "neuro_fig1.png"
    assert res_dry["copied"] == 0
    assert res_dry["synced"] == 1
    assert res_dry["missing"] == 1
    # active_total MUST be 1 synced + 1 missing + 1 collision = 3
    assert res_dry["active_evaluated"] == 3
    assert res_dry["sync_ratio"] == round(1 / 3, 4)

    # B. Apply with collisions -> ANY copy is blocked!
    res_apply = sync_fn(str(catalog_file), apply=True, collection_media_dir=str(coll_dir))
    assert res_apply["status"] == "ERROR"
    assert res_apply["code"] == "MEDIA.COLLISION"
    assert res_apply["copied"] == 0
    # Verification that fig2 was NOT copied despite being valid and missing
    assert not (coll_dir / "neuro_fig2.png").exists()
    # Verification that fig1 was not overwritten
    assert (coll_dir / "neuro_fig1.png").read_text(encoding="utf-8") == "old_content_in_collection"


def test_image_sync_successful_copy_transactional(tmp_path):
    """Test successful transactional copy with temp+replace leaves no leftover temp files."""
    sync_fn, _ = _get_tools()

    coll_dir = tmp_path / "coll_success"
    coll_dir.mkdir()

    source_dir = tmp_path / "images_neuro"
    source_dir.mkdir()
    (source_dir / "ecg.png").write_text("ecg_binary_data", encoding="utf-8")
    (source_dir / "eeg.png").write_text("eeg_binary_data", encoding="utf-8")

    catalog_file = source_dir / "catalog.csv"
    catalog_file.write_text(
        "filename|caption|slug\n"
        "ecg.png|Eletrocardiograma|cardio\n"
        "eeg.png|Eletroencefalograma|neuro\n",
        encoding="utf-8",
    )

    res = sync_fn(str(catalog_file), apply=True, collection_media_dir=str(coll_dir))
    assert res["status"] == "OK"
    assert res["copied"] == 2
    assert res["synced"] == 2
    assert res["missing"] == 0
    assert res["sync_ratio"] == 1.0

    # Verify destination files
    assert (coll_dir / "cardio_ecg.png").read_text(encoding="utf-8") == "ecg_binary_data"
    assert (coll_dir / "neuro_eeg.png").read_text(encoding="utf-8") == "eeg_binary_data"

    # Verify no temp files remain
    tmp_files = list(coll_dir.glob(".tmp_*"))
    assert len(tmp_files) == 0


def test_image_sync_partial_io_rollback_on_oserror(tmp_path, monkeypatch):
    """Test that an OSError during batch copying rolls back already copied files and returns structured error."""
    import shutil

    sync_fn, _ = _get_tools()

    coll_dir = tmp_path / "coll_rollback"
    coll_dir.mkdir()

    source_dir = tmp_path / "images_neuro"
    source_dir.mkdir()
    (source_dir / "img1.png").write_text("data_1", encoding="utf-8")
    (source_dir / "img2.png").write_text("data_2", encoding="utf-8")
    (source_dir / "img3.png").write_text("data_3", encoding="utf-8")

    catalog_file = source_dir / "catalog.csv"
    catalog_file.write_text(
        "filename|caption|slug\n"
        "img1.png|Imagem 1|neuro\n"
        "img2.png|Imagem 2|neuro\n"
        "img3.png|Imagem 3|neuro\n",
        encoding="utf-8",
    )

    original_copy2 = shutil.copy2

    def fail_on_img2(src, dst, **kwargs):
        if "img2.png" in str(src):
            raise OSError("Simulated disk full or permission error on img2")
        return original_copy2(src, dst, **kwargs)

    monkeypatch.setattr(shutil, "copy2", fail_on_img2)

    res = sync_fn(str(catalog_file), apply=True, collection_media_dir=str(coll_dir))

    # Assert structured error response (NOT "OK")
    assert res["status"] == "ERROR"
    assert res["code"] == "MEDIA.IO_ERROR"
    assert "Simulated disk full" in res["message"]
    assert res["rolled_back_count"] == 1  # img1 was copied, then rolled back
    assert res["copied"] == 0

    # Crucial assertion: img1.png must NOT exist in coll_dir because of batch rollback!
    assert not (coll_dir / "neuro_img1.png").exists()
    assert not (coll_dir / "neuro_img2.png").exists()
    assert not (coll_dir / "neuro_img3.png").exists()

    # No leftover .tmp_ files
    assert list(coll_dir.glob(".tmp_*")) == []
