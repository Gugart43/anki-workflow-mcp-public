"""tests/test_deck_audit.py — tools/pipeline.py deck_audit tests."""

from __future__ import annotations

import asyncio
import csv
import io
from pathlib import Path

from fastmcp import FastMCP

from anki_mcp.core.deck import save_manifesto
from anki_mcp.models import CardStatus, IDEntry, Manifesto, NoteType, TaxonomyNode
from anki_mcp.tools.pipeline import register_pipeline_tools


def _mcp() -> FastMCP:
    mcp = FastMCP("test-audit")
    register_pipeline_tools(mcp)
    return mcp


def _get_fn(mcp: FastMCP, name: str):
    tool = asyncio.run(mcp.get_tool(name))
    return tool.fn


def _seed(deck: Path, ids: list[str] | None = None, topics: list[str] | None = None) -> None:
    """Write manifesto + matching CSVs."""
    if ids is None:
        ids = ["B01-QA-001", "B01-QA-002", "B01-CL-001"]
    if topics is None:
        topics = ["Epilepsia", "Epilepsia", "Meningite"]
    entries = [
        IDEntry(
            id=i,
            batch=i.split("-")[0],
            type=NoteType.QA if "QA" in i else NoteType.CLOZE,
            topic=topics[idx] if idx < len(topics) else "",
            anchor="x",
        )
        for idx, i in enumerate(ids)
    ]
    save_manifesto(
        deck,
        Manifesto(
            deck_slug="neuro",
            campo_estudo="Neurologia",
            created="2026-09-28",
            updated="2026-09-28",
            taxonomy=TaxonomyNode(root="Neurologia", nodes=["Neurologia::Epilepsia"]),
            id_registry=entries,
        ),
    )
    # Write matching CSV files
    qa_ids = [i for i in ids if "QA" in i]
    cl_ids = [i for i in ids if "CL" in i]
    if qa_ids:
        batch = qa_ids[0].split("-")[0]
        qa_csv = deck / f"{batch}-QA.csv"
        buf = io.StringIO()
        w = csv.writer(buf, delimiter="|")
        w.writerow(["ID", "Pergunta", "Resposta", "Notas", "Tags", "Imagens", "Refs"])
        for qid in qa_ids:
            w.writerow([qid, "Q?", "A", "", "tag", "", ""])
        qa_csv.write_text(buf.getvalue(), encoding="utf-8")
    if cl_ids:
        batch = cl_ids[0].split("-")[0]
        cl_csv = deck / f"{batch}-Cloze.csv"
        buf = io.StringIO()
        w = csv.writer(buf, delimiter="|")
        w.writerow(["ID", "Sentença", "Notas", "Tags", "Imagens", "Refs"])
        for cid in cl_ids:
            w.writerow([cid, "{{c1::X}}", "", "tag", "", ""])
        cl_csv.write_text(buf.getvalue(), encoding="utf-8")


def test_deck_audit_healthy(tmp_path: Path):
    _seed(tmp_path, ids=["B01-QA-001", "B01-CL-001"], topics=["Epilepsia", "Meningite"])
    mcp = _mcp()
    fn = _get_fn(mcp, "deck_audit")
    result = fn(deck_dir=str(tmp_path))
    assert result["status"] == "OK"
    assert result["summary"]["health"] == "HEALTHY"
    assert result["summary"]["orphan_count"] == 0
    assert result["summary"]["phantom_count"] == 0


def test_deck_audit_with_tag_embedded_ids(tmp_path: Path):
    """Deck audit detects IDs embedded in Etiquetas as id::<CARD_ID> (AnkiMBR 7/6 col)."""
    deck = tmp_path
    _seed(deck, ids=["B01-QA-001"], topics=["Epilepsia"])
    # Rewrite B01-QA.csv without ID column, using 7-col AnkiMBR format with id:: tag
    qa_csv = deck / "B01-QA.csv"
    buf = io.StringIO()
    w = csv.writer(buf, delimiter="|")
    w.writerow(
        [
            "Enunciado",
            "Resposta",
            "Campo de Estudo",
            "Notas Adicionais",
            "Imagens Relacionadas",
            "Referências",
            "Etiquetas",
        ]
    )
    w.writerow(
        [
            "Enunciado?",
            "Resposta!",
            "Neurologia",
            "",
            "",
            "",
            "Neurologia::Epilepsia id::B01-QA-001",
        ]
    )
    qa_csv.write_text(buf.getvalue(), encoding="utf-8")

    mcp = _mcp()
    fn = _get_fn(mcp, "deck_audit")
    result = fn(deck_dir=str(deck))
    assert result["status"] == "OK"
    assert result["summary"]["health"] == "HEALTHY"
    assert result["summary"]["orphan_count"] == 0
    assert result["summary"]["phantom_count"] == 0


def test_deck_audit_orphan_ids(tmp_path: Path):
    """ID in registry but no CSV -> orphan."""
    # Seed only B01 IDs with CSV
    _seed(tmp_path, ids=["B01-QA-001"], topics=["Epilepsia"])
    # Manually add B02-QA-001 to registry but no CSV
    m = Manifesto(
        deck_slug="neuro",
        campo_estudo="Neurologia",
        created="2026-09-28",
        updated="2026-09-28",
        taxonomy=TaxonomyNode(root="Neurologia", nodes=[]),
        id_registry=[
            IDEntry(id="B01-QA-001", batch="B01", type=NoteType.QA, topic="Epilepsia", anchor="x"),
            IDEntry(id="B02-QA-001", batch="B02", type=NoteType.QA, topic="Meningite", anchor="x"),
            IDEntry(
                id="B02-QA-002",
                batch="B02",
                type=NoteType.QA,
                topic="Meningite 2",
                anchor="x",
                status=CardStatus.RETIRED,
            ),
        ],
    )
    save_manifesto(tmp_path, m)
    mcp = _mcp()
    fn = _get_fn(mcp, "deck_audit")
    result = fn(deck_dir=str(tmp_path))
    assert "B02-QA-001" in result["orphan_ids"]
    # Retired ID should not be counted as orphan
    assert "B02-QA-002" not in result["orphan_ids"]


def test_deck_audit_phantom_ids(tmp_path: Path):
    """ID in CSV but not in registry -> phantom."""
    _seed(tmp_path, ids=["B01-QA-001"])
    # Add a CSV with an extra ID not in registry
    buf = io.StringIO()
    w = csv.writer(buf, delimiter="|")
    w.writerow(["ID", "Pergunta", "Resposta", "Notas", "Tags", "Imagens", "Refs"])
    w.writerow(["B01-QA-001", "Q?", "A", "", "", "", ""])
    w.writerow(["B01-QA-999", "Q?", "A", "", "", "", ""])  # phantom
    (tmp_path / "B01-QA.csv").write_text(buf.getvalue(), encoding="utf-8")
    mcp = _mcp()
    fn = _get_fn(mcp, "deck_audit")
    result = fn(deck_dir=str(tmp_path))
    assert "B01-QA-999" in result["phantom_ids"]


def test_deck_audit_duplicate_topics(tmp_path: Path):
    """Same topic in multiple batches."""
    _seed(
        tmp_path,
        ids=["B01-QA-001", "B02-QA-001"],
        topics=["Epilepsia", "Epilepsia"],
    )
    # Need B02 CSV too
    buf = io.StringIO()
    w = csv.writer(buf, delimiter="|")
    w.writerow(["ID", "Pergunta", "Resposta", "Notas", "Tags", "Imagens", "Refs"])
    w.writerow(["B02-QA-001", "Q?", "A", "", "", "", ""])
    (tmp_path / "B02-QA.csv").write_text(buf.getvalue(), encoding="utf-8")
    mcp = _mcp()
    fn = _get_fn(mcp, "deck_audit")
    result = fn(deck_dir=str(tmp_path))
    assert "Epilepsia" in result["duplicate_topics"]
    assert len(result["duplicate_topics"]["Epilepsia"]) >= 2


def test_deck_audit_underscore_export_filenames(tmp_path: Path):
    """export_final grava `{prefix}_{topic}_QA.csv`; o batch_id deve sair do prefixo.

    Regressao: `stem.split('-')[0]` devolvia o stem inteiro
    (`B01_catarata_QA`), o que quebrava batch_integrity e duplicate_topics.
    """
    deck = tmp_path
    _seed(deck, ids=["B01-QA-001", "B02-QA-001"], topics=["Catarata", "Catarata"])
    (deck / "B01-QA.csv").unlink()
    for prefix in ("B01", "B02"):
        buf = io.StringIO()
        w = csv.writer(buf, delimiter="|")
        w.writerow(
            [
                "Pergunta",
                "Resposta",
                "Campo de Estudo",
                "Notas Adicionais",
                "Imagens Relacionadas",
                "Referências",
                "Etiquetas",
            ]
        )
        w.writerow(
            [
                "Enunciado?",
                "Resposta!",
                "Oftalmologia",
                "",
                "",
                "",
                f"Oftalmologia::Catarata id::{prefix}-QA-001",
            ]
        )
        (deck / f"{prefix}_catarata_QA.csv").write_text(buf.getvalue(), encoding="utf-8")

    mcp = _mcp()
    fn = _get_fn(mcp, "deck_audit")
    result = fn(deck_dir=str(deck))

    assert result["status"] == "OK"
    batches = [entry["batch"] for entry in result["batch_integrity"]]
    assert batches == ["B01", "B02"], batches
    assert result["summary"]["phantom_count"] == 0
    assert result["summary"]["orphan_count"] == 0
    assert "Catarata" in result["duplicate_topics"]


def test_deck_audit_no_manifesto(tmp_path: Path):
    """Missing manifesto should be caught by @safe_tool."""
    mcp = _mcp()
    fn = _get_fn(mcp, "deck_audit")
    result = fn(deck_dir=str(tmp_path))
    assert result["status"] == "ERROR"
