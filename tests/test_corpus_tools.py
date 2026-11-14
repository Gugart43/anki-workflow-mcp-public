"""tests/test_corpus_tools.py — tools/corpus.py integration tests.

Uses tmp_path fixture and FastMCP get_tool to exercise:
  corpus_load, corpus_triage, corpus_coverage, corpus_digest,
  EMPTY / NOT_FOUND error paths.
"""

from __future__ import annotations

import asyncio
from pathlib import Path

from fastmcp import FastMCP

from anki_mcp.core.deck import save_manifesto
from anki_mcp.models import IDEntry, Manifesto, NoteType, TaxonomyNode
from anki_mcp.tools.corpus import register_corpus_tools

# ── Helpers ───────────────────────────────────────────────────────────────────


def _mcp() -> FastMCP:
    mcp = FastMCP("test-corpus")
    register_corpus_tools(mcp)
    return mcp


def _get_fn(mcp: FastMCP, name: str):
    """Get a tool function synchronously."""
    tool = asyncio.run(mcp.get_tool(name))
    return tool.fn


def _seed_manifesto(deck: Path) -> None:
    """Write a minimal manifesto with some topics."""
    save_manifesto(
        deck,
        Manifesto(
            deck_slug="neuro",
            campo_estudo="Medicina::Neurologia",
            created="2026-09-27",
            updated="2026-09-27",
            taxonomy=TaxonomyNode(root="Neurologia", nodes=["Neurologia::Epilepsia"]),
            id_registry=[
                IDEntry(
                    id="B01-QA-001",
                    batch="B01",
                    type=NoteType.QA,
                    topic="Epilepsia",
                    anchor="crise",
                )
            ],
        ),
    )


CLINICAL_SOURCE = (
    "O tratamento de primeira linha da meningite bacteriana e ceftriaxona 2 g IV.\n"
    "O mecanismo fisiopatologico envolve internalizacao de receptores GABA-A.\n"
    "E importante entender que o contexto geral deste capitulo e fundamental.\n"
    "A dose de fenitoina e 20 mg/kg IV a no maximo 50 mg/min.\n"
    "A prevalencia e de 12% em estudos recentes publicados em 2024.\n"
)


# ── corpus_load ───────────────────────────────────────────────────────────────


def test_corpus_load_creates_file(tmp_path: Path):
    mcp = _mcp()
    fn = _get_fn(mcp, "corpus_load")
    result = fn(deck_dir=str(tmp_path), text="Conteudo de teste.", label="teste_load")
    assert result["status"] == "OK"
    assert Path(result["path"]).exists()
    assert result["label"] == "teste-load"


def test_corpus_load_appends_on_duplicate(tmp_path: Path):
    mcp = _mcp()
    fn = _get_fn(mcp, "corpus_load")
    fn(deck_dir=str(tmp_path), text="Parte 1.", label="dup")
    fn(deck_dir=str(tmp_path), text="Parte 2.", label="dup")
    content = (tmp_path / "corpus" / "dup.md").read_text(encoding="utf-8")
    assert "Parte 1" in content
    assert "Parte 2" in content


# ── corpus_triage ─────────────────────────────────────────────────────────────


def test_corpus_triage_inline_text(tmp_path: Path):
    mcp = _mcp()
    fn = _get_fn(mcp, "corpus_triage")
    result = fn(deck_dir=str(tmp_path), text=CLINICAL_SOURCE)
    assert result["status"] == "OK"
    assert isinstance(result["counts"], dict)
    assert "CARD" in result["counts"]
    assert len(result["candidates"]) > 0


def test_corpus_triage_from_label(tmp_path: Path):
    mcp = _mcp()
    load_fn = _get_fn(mcp, "corpus_load")
    triage_fn = _get_fn(mcp, "corpus_triage")
    load_fn(deck_dir=str(tmp_path), text=CLINICAL_SOURCE, label="meningite")
    result = triage_fn(deck_dir=str(tmp_path), label="meningite")
    assert result["status"] == "OK"
    assert result["counts"]["CARD"] >= 1


def test_corpus_triage_missing_label(tmp_path: Path):
    mcp = _mcp()
    fn = _get_fn(mcp, "corpus_triage")
    result = fn(deck_dir=str(tmp_path), label="naoexiste")
    assert result["status"] == "EMPTY"
    assert result["code"] == "CORPUS.NOT_FOUND"


def test_corpus_triage_no_input(tmp_path: Path):
    mcp = _mcp()
    fn = _get_fn(mcp, "corpus_triage")
    result = fn(deck_dir=str(tmp_path))
    assert result["status"] == "EMPTY"


# ── corpus_coverage ───────────────────────────────────────────────────────────


def test_corpus_coverage_reports_gaps(tmp_path: Path):
    _seed_manifesto(tmp_path)
    mcp = _mcp()
    load_fn = _get_fn(mcp, "corpus_load")
    coverage_fn = _get_fn(mcp, "corpus_coverage")
    load_fn(deck_dir=str(tmp_path), text=CLINICAL_SOURCE, label="meningite")
    result = coverage_fn(deck_dir=str(tmp_path))
    assert result["status"] == "OK"
    assert result["corpus_files"] == 1
    assert len(result["gaps"]) > 0
    assert all(g["code"] == "COVERAGE.GAP" for g in result["gaps"])


def test_corpus_coverage_empty_corpus(tmp_path: Path):
    _seed_manifesto(tmp_path)
    mcp = _mcp()
    fn = _get_fn(mcp, "corpus_coverage")
    result = fn(deck_dir=str(tmp_path))
    assert result["status"] == "EMPTY"


# ── corpus_digest ─────────────────────────────────────────────────────────────


def test_corpus_digest_shape(tmp_path: Path):
    _seed_manifesto(tmp_path)
    mcp = _mcp()
    load_fn = _get_fn(mcp, "corpus_load")
    digest_fn = _get_fn(mcp, "corpus_digest")
    load_fn(deck_dir=str(tmp_path), text=CLINICAL_SOURCE, label="meningite")
    result = digest_fn(deck_dir=str(tmp_path))
    assert result["status"] == "OK"
    summary = result["summary"]
    assert summary["corpus_files"] == 1
    assert summary["total_candidates"] > 0
    for key in ("CARD", "MAYBE", "DISCARD", "gaps"):
        assert key in summary
    assert isinstance(result["card"], list)
    assert isinstance(result["gaps"], list)
    assert isinstance(result["discard"], list)


def test_corpus_digest_empty(tmp_path: Path):
    _seed_manifesto(tmp_path)
    mcp = _mcp()
    fn = _get_fn(mcp, "corpus_digest")
    result = fn(deck_dir=str(tmp_path))
    assert result["status"] == "EMPTY"


def test_corpus_digest_survives_discard_without_reasons(tmp_path: Path, monkeypatch):
    """Regressao: DISCARD com `reasons` vazio derrubava o digest com IndexError."""
    _seed_manifesto(tmp_path)
    mcp = _mcp()
    load_fn = _get_fn(mcp, "corpus_load")
    digest_fn = _get_fn(mcp, "corpus_digest")
    load_fn(deck_dir=str(tmp_path), text=CLINICAL_SOURCE, label="meningite")

    monkeypatch.setattr(
        "anki_mcp.tools.corpus._triage_corpus",
        lambda files: [
            {
                "verdict": "DISCARD",
                "reasons": [],
                "composite": 0.1,
                "source_label": "meningite",
                "line": 1,
                "text": "x",
            }
        ],
    )
    result = digest_fn(deck_dir=str(tmp_path))
    assert result["status"] == "OK"
    assert result["summary"]["DISCARD"] == 1
    assert result["discard"][0]["reason"] == "no reason recorded"


def test_discard_reason_falls_back_when_missing():
    from anki_mcp.tools.corpus import _discard_reason

    assert _discard_reason(None) == "no reason recorded"
    assert _discard_reason([]) == "no reason recorded"
    assert _discard_reason(["   "]) == "no reason recorded"
    assert _discard_reason(["single"]) == "single"
    assert _discard_reason(["a", "b"]) == "a"


def test_corpus_digest_card_section_has_card_items(tmp_path: Path):
    _seed_manifesto(tmp_path)
    mcp = _mcp()
    load_fn = _get_fn(mcp, "corpus_load")
    digest_fn = _get_fn(mcp, "corpus_digest")
    load_fn(deck_dir=str(tmp_path), text=CLINICAL_SOURCE, label="meningite")
    result = digest_fn(deck_dir=str(tmp_path))
    for item in result["card"]:
        assert item["verdict"] == "CARD"
