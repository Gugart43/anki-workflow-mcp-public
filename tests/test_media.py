"""test_media.py — smoke tests for catalog and media tools."""

from __future__ import annotations

import csv
from pathlib import Path

from anki_mcp.tools.media import (
    _hash_dir,
    _load_catalog_csv,
)

# ── Fixtures ──────────────────────────────────────────────────────────────────


def _make_catalog(tmp_path: Path, rows: list[dict]) -> Path:
    p = tmp_path / "anki_catalog.csv"
    with p.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(
            fh, fieldnames=["slug", "filename", "caption", "anki_tag"], delimiter="|"
        )
        writer.writeheader()
        writer.writerows(rows)
    return p


SAMPLE_ROWS = [
    {
        "slug": "ait",
        "filename": "fig_01.png",
        "caption": "Placa de ateroma",
        "anki_tag": "AIT::Fisiopatologia",
    },
    {
        "slug": "ait",
        "filename": "fig_02.png",
        "caption": "Escala NIHSS",
        "anki_tag": "AIT::Avaliação",
    },
    {
        "slug": "ela",
        "filename": "fig_03.png",
        "caption": "Neurônio motor",
        "anki_tag": "ELA::Fisiopatologia",
    },
]


# ── Tests ─────────────────────────────────────────────────────────────────────


def test_load_catalog_roundtrip(tmp_path):
    p = _make_catalog(tmp_path, SAMPLE_ROWS)
    entries = _load_catalog_csv(p)
    assert len(entries) == 3
    assert entries[0].slug == "ait"
    assert entries[0].namespaced == "ait_fig_01.png"
    assert entries[2].namespaced == "ela_fig_03.png"


def test_image_namespace_format(tmp_path):
    """namespaced = slug_filename."""
    p = _make_catalog(tmp_path, SAMPLE_ROWS)
    entries = _load_catalog_csv(p)
    for e in entries:
        assert e.namespaced == f"{e.slug}_{e.filename}"


def test_candidates_filter(tmp_path):
    p = _make_catalog(tmp_path, SAMPLE_ROWS)
    entries = _load_catalog_csv(p)
    q = "nihss"
    matches = [e for e in entries if q in e.caption.lower() or q in e.anki_tag.lower()]
    assert len(matches) == 1
    assert matches[0].filename == "fig_02.png"


def test_hash_dir_deterministic(tmp_path):
    # Create two dirs with same files → same hash
    d1 = tmp_path / "d1"
    d2 = tmp_path / "d2"
    d1.mkdir()
    d2.mkdir()
    for name in ("a.png", "b.png", "c.png"):
        (d1 / name).write_text("x")
        (d2 / name).write_text("y")  # different content, same names
    assert _hash_dir(d1) == _hash_dir(d2)  # hash is of names, not content


def test_hash_dir_differs_on_name_change(tmp_path):
    d = tmp_path / "d"
    d.mkdir()
    (d / "a.png").write_text("x")
    h1 = _hash_dir(d)
    (d / "b.png").write_text("x")
    h2 = _hash_dir(d)
    assert h1 != h2


def test_no_slug_entries(tmp_path):
    rows = [{"slug": "", "filename": "fig_01.png", "caption": "Test", "anki_tag": "Tag"}]
    p = _make_catalog(tmp_path, rows)
    entries = _load_catalog_csv(p)
    assert entries[0].namespaced == "fig_01.png"  # falls back to just filename


def test_load_catalog_universal_json(tmp_path):
    """Test loading extractor catalog.json format."""
    import json

    catalog_data = {
        "pdf_name": "Catarata adultos",
        "items": [
            {
                "filename": "img_p17_1.png",
                "page": 17,
                "caption": "Referencias - 2",
            },
            {
                "filename": "img_p17_2.png",
                "page": 17,
                "caption": "Imagem 2",
            },
        ],
    }
    json_path = tmp_path / "catalog.json"
    json_path.write_text(json.dumps(catalog_data), encoding="utf-8")
    entries = _load_catalog_csv(json_path)
    assert len(entries) == 2
    assert entries[0].filename == "img_p17_1.png"
    assert entries[0].slug == "catarata_adultos"
    assert entries[0].namespaced == "catarata_adultos_img_p17_1.png"


def test_load_catalog_universal_headerless_csv(tmp_path):
    """Test loading extractor headerless 2-column anki_catalog.csv."""
    content = (
        "#separator:Pipe\n"
        '"<img src=""img_p17_1.png"">"|Referencias - 2\n'
        '"<img src=""img_p17_2.png"">"|Imagem 2\n'
    )
    csv_path = tmp_path / "anki_catalog.csv"
    csv_path.write_text(content, encoding="utf-8")
    entries = _load_catalog_csv(csv_path)
    assert len(entries) == 2
    assert entries[0].filename == "img_p17_1.png"
    assert entries[0].caption == "Referencias - 2"
    assert entries[1].filename == "img_p17_2.png"


def test_image_sync_apply_copies_files(tmp_path, monkeypatch):
    """Test that image_sync with apply=True copies files from source to collection.media."""
    import asyncio

    from fastmcp import FastMCP

    from anki_mcp.tools.media import register_media_tools

    source_dir = tmp_path / "images_neuro"
    source_dir.mkdir()
    img_file = source_dir / "brain.png"
    img_file.write_text("fake_png_data", encoding="utf-8")

    catalog_file = source_dir / "anki_catalog.csv"
    catalog_file.write_text(
        '#separator:Pipe\n"<img src=""brain.png"">"|Cortex cerebral\n',
        encoding="utf-8",
    )

    mock_collection = tmp_path / "collection.media"
    mock_collection.mkdir()

    monkeypatch.setattr("anki_mcp.tools.media._collection_media", lambda: mock_collection)

    mcp = FastMCP("test-media")
    register_media_tools(mcp)

    tool = asyncio.run(mcp.get_tool("image_sync"))
    sync_fn = tool.fn

    # 1. Dry run (apply=False)
    res_dry = sync_fn(str(catalog_file), apply=False)
    assert res_dry["status"] == "OK"
    assert res_dry["missing"] == 1
    assert res_dry["copied"] == 0
    assert not (mock_collection / "neuro_brain.png").exists()

    # 2. Apply run (apply=True)
    res_apply = sync_fn(str(catalog_file), apply=True)
    assert res_apply["status"] == "OK"
    assert res_apply["copied"] == 1
    assert res_apply["synced"] == 1
    assert (mock_collection / "neuro_brain.png").exists()
    assert (mock_collection / "neuro_brain.png").read_text(encoding="utf-8") == "fake_png_data"


def test_load_catalog_various_formats(tmp_path):
    import json

    from anki_mcp.tools.media import _load_catalog_csv

    # 1. central_images.json format
    hub_dir = tmp_path / "hub"
    hub_dir.mkdir()
    hub_file = hub_dir / "central_images.json"
    hub_data = {
        "images": [
            {
                "folder": "neuro_docs",
                "filename": "fig1.png",
                "caption": "RM sagital de encéfalo",
                "width_px": 800,
                "height_px": 600,
                "page": 5,
                "type": "figure",
            }
        ]
    }
    hub_file.write_text(json.dumps(hub_data), encoding="utf-8")
    entries_hub = _load_catalog_csv(hub_file)
    assert len(entries_hub) == 1
    assert entries_hub[0].filename == "fig1.png"
    assert "RM sagital" in entries_hub[0].caption

    # 2. catalog.json (per-doc) format
    doc_dir = tmp_path / "doc"
    doc_dir.mkdir()
    doc_file = doc_dir / "catalog.json"
    doc_data = {
        "document": "AVC Protocol",
        "images": [
            {
                "filename": "nihss.png",
                "caption": "Escala de NIHSS",
                "page": 2,
                "type": "table",
            }
        ],
    }
    doc_file.write_text(json.dumps(doc_data), encoding="utf-8")
    entries_doc = _load_catalog_csv(doc_file)
    assert len(entries_doc) == 1
    assert entries_doc[0].filename == "nihss.png"
    assert entries_doc[0].item_type == "table"

    # 3. catalog.md (markdown table) format
    md_dir = tmp_path / "md"
    md_dir.mkdir()
    md_file = md_dir / "catalog.md"
    md_content = (
        "# Catálogo\n"
        "| Tipo | Imagem | Página | Legenda | Tag Anki |\n"
        "|---|---|---|---|---|\n"
        "| Tabela | (cortex.png) | 4 | Áreas de Brodmann | Neuro |\n"
    )
    md_file.write_text(md_content, encoding="utf-8")
    entries_md = _load_catalog_csv(md_file)
    assert len(entries_md) == 1
    assert entries_md[0].filename == "cortex.png"

    # 4. Headerless 2-column CSV (<img src="x.png"> | Caption)
    csv2_dir = tmp_path / "csv2"
    csv2_dir.mkdir()
    csv2_file = csv2_dir / "catalog_2col.csv"
    csv2_file.write_text('<img src="ecg.png">|Traçado de fibrilação atrial\n', encoding="utf-8")
    entries_csv2 = _load_catalog_csv(csv2_file)
    assert len(entries_csv2) == 1
    assert entries_csv2[0].filename == "ecg.png"


def test_catalog_load_tool_and_manifesto_registration(tmp_path):
    import asyncio
    import json

    from fastmcp import FastMCP

    from anki_mcp.core.deck import load_manifesto, save_manifesto
    from anki_mcp.models import Manifesto, TaxonomyNode
    from anki_mcp.tools.media import register_media_tools

    deck = tmp_path / "deck"
    deck.mkdir()
    m = Manifesto(
        deck_slug="neuro-deck",
        campo_estudo="Medicina",
        created="2026-01-01",
        updated="2026-01-01",
        taxonomy=TaxonomyNode(root="Neuro", nodes=["AVC"]),
    )
    save_manifesto(deck, m)

    media_dir = deck / "media"
    media_dir.mkdir()
    cat_json = media_dir / "catalog.json"
    cat_json.write_text(
        json.dumps(
            {
                "document": "AVC Protocol",
                "images": [
                    {
                        "filename": "penumbra.png",
                        "caption": "Área de penumbra isquêmica",
                        "page": 10,
                        "type": "figure",
                    },
                    {
                        "filename": "doses.png",
                        "caption": "Tabela de trombolítico",
                        "page": 12,
                        "type": "table",
                    },
                ],
            }
        ),
        encoding="utf-8",
    )

    mcp = FastMCP("test-media-load")
    register_media_tools(mcp)
    cat_tool = asyncio.run(mcp.get_tool("catalog_load")).fn

    res = cat_tool(str(cat_json), deck_dir=str(deck))
    assert res["status"] == "OK"
    assert res["count"] == 2
    assert res["registered_in_manifesto"] is True

    m_updated = load_manifesto(deck)
    assert len(m_updated.catalogs) == 1
    assert m_updated.catalogs[0].slug == "doses_png"


def test_image_candidates_search_and_filter(tmp_path):
    import asyncio

    from fastmcp import FastMCP

    from anki_mcp.tools.media import register_media_tools

    cat_file = tmp_path / "anki_catalog.csv"
    cat_file.write_text(
        "filename|caption|slug\n"
        "tpa_tabela_1.png|Dose de alteplase no AVC|avc\n"
        "ct_scan.png|Tomografia de crânio sem contraste demonstrando hipodensidade precoce|avc\n",
        encoding="utf-8",
    )

    mcp = FastMCP("test-media-candidates")
    register_media_tools(mcp)
    cand_tool = asyncio.run(mcp.get_tool("image_candidates")).fn

    # 1. Query match
    r1 = cand_tool(str(cat_file), "alteplase")
    assert r1["count"] >= 1
    assert r1["entries"][0]["filename"] == "tpa_tabela_1.png"

    # 2. Filter by item_type
    r2 = cand_tool(str(cat_file), "alteplase", item_type="table")
    assert r2["count"] == 1
    assert r2["entries"][0]["item_type"] == "table"

    # 3. No match
    r3 = cand_tool(str(cat_file), "pneumonia_bacteriana")
    assert r3["count"] == 0


def test_media_namespace_and_report(tmp_path, monkeypatch):
    import asyncio

    from fastmcp import FastMCP

    from anki_mcp.tools.media import register_media_tools

    mock_collection = tmp_path / "collection.media"
    mock_collection.mkdir()
    monkeypatch.setattr("anki_mcp.tools.media._collection_media", lambda: mock_collection)

    mcp = FastMCP("test-media-report")
    register_media_tools(mcp)
    ns_tool = asyncio.run(mcp.get_tool("image_namespace")).fn
    report_tool = asyncio.run(mcp.get_tool("image_report")).fn

    # image_namespace
    ns_res = ns_tool("Cérebro & Tronco", "scan.PNG")
    assert ns_res["slug"] == "cerebro_tronco"
    assert ns_res["namespaced"] == "cerebro_tronco_scan.png"

    # image_report with nonexistent catalog
    nonexistent = tmp_path / "missing_catalog.csv"
    rep_res = report_tool(catalog_paths=str(nonexistent))
    assert rep_res["status"] == "OK"
    assert len(rep_res["catalogs"]) == 1
    assert rep_res["catalogs"][0]["status"] == "NOT_FOUND"
