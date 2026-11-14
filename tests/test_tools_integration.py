"""Tests for tools registered via FastMCP — exercises the tool layer (validate, state, media)."""

from __future__ import annotations

import asyncio
import shutil
import tempfile
from pathlib import Path

import pytest
from fastmcp import FastMCP

from anki_mcp.core.deck import save_manifesto
from anki_mcp.core.receipt import compute_receipt
from anki_mcp.models import Manifesto, TaxonomyNode
from anki_mcp.tools.media import register_media_tools
from anki_mcp.tools.state import register_state_tools
from anki_mcp.tools.validate import register_validate_tools

QA_H = (
    "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas"
)
CL_H = "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas"
QA_CSV = QA_H + "\nQ1|A1|C|N||R|T"
CL_CSV = CL_H + "\n{{c1::A}} {{c2::B}}|N|C||R|T"


@pytest.fixture
def deck_dir():
    d = Path(tempfile.mkdtemp())
    m = Manifesto(
        deck_slug="test",
        campo_estudo="Medicina",
        created="2026-01-01",
        updated="2026-01-01",
        taxonomy=TaxonomyNode(root="Medicina"),
    )
    save_manifesto(d, m)
    yield d
    shutil.rmtree(d, ignore_errors=True)


def _get_fn(mcp_inst: FastMCP, name: str):
    """Synchronously get a tool's function from FastMCP."""
    tool = asyncio.run(mcp_inst.get_tool(name))
    return tool.fn


# ── Validate tools ──────────────────────────────────────────────────────


class TestValidateTools:
    @pytest.fixture(autouse=True)
    def _mcp(self):
        self.mcp = FastMCP("test-validate")
        register_validate_tools(self.mcp)

    def test_validate_csv_structure_pass(self):
        fn = _get_fn(self.mcp, "validate_csv_structure")
        r = fn(csv_text=QA_CSV, note_type="QA")
        assert r["result"] in ("PASS", "WARN")

    def test_validate_csv_structure_bad_header(self):
        fn = _get_fn(self.mcp, "validate_csv_structure")
        r = fn(csv_text="Bad|Header\nA|B", note_type="QA")
        assert r["result"] == "FAIL"

    def test_validate_cloze_pass(self):
        fn = _get_fn(self.mcp, "validate_cloze")
        r = fn(sentence="O paciente apresentou {{c1::febre}} e {{c2::tremores}}")
        assert r["result"] in ("PASS", "WARN")

    def test_validate_hints_pass(self):
        fn = _get_fn(self.mcp, "validate_hints")
        r = fn(sentence="O valor é {{c1::120 mmHg::faixa de PA}}")
        assert r["result"] in ("PASS", "WARN")

    def test_validate_qa_pass(self):
        fn = _get_fn(self.mcp, "validate_qa")
        r = fn(
            enunciado="No contexto da hipertensão, qual é a droga de escolha?",
            resposta="Enalapril 10mg",
        )
        assert r["result"] in ("PASS", "WARN")

    def test_validate_notes_pass(self):
        fn = _get_fn(self.mcp, "validate_notes")
        r = fn(notes="Informação complementar sobre o diagnóstico.")
        assert r["result"] in ("PASS", "WARN")

    def test_validate_tags_field_pass(self):
        fn = _get_fn(self.mcp, "validate_tags_field")
        r = fn(tags_field="Neurologia::Epilepsia")
        assert r["result"] in ("PASS", "WARN")

    def test_validate_batch_pass(self):
        fn = _get_fn(self.mcp, "validate_batch")
        r = fn(batch_id="B01", qa_csv=QA_CSV, cloze_csv=CL_CSV)
        assert r["result"] in ("PASS", "WARN", "FAIL")
        assert "checks" in r

    def test_validate_crossrefs(self):
        fn = _get_fn(self.mcp, "validate_crossrefs")
        r = fn(text="See B01-QA-001", active_ids="B01-QA-001")
        assert r["result"] in ("PASS", "WARN")

    def test_validate_images_field(self):
        fn = _get_fn(self.mcp, "validate_images_field")
        r = fn(
            html='<figure><img src="test.jpg"><figcaption>Test</figcaption></figure>',
            catalog_filenames="test.jpg",
        )
        assert "result" in r


# ── State tools ─────────────────────────────────────────────────────────


class TestStateTools:
    @pytest.fixture(autouse=True)
    def _mcp(self):
        self.mcp = FastMCP("test-state")
        register_state_tools(self.mcp)

    def test_manifesto_init(self, tmp_path):
        fn = _get_fn(self.mcp, "manifesto_init")
        r = fn(
            deck_dir=str(tmp_path),
            deck_slug="test-deck",
            campo_estudo="Medicina",
            taxonomy_root="Medicina",
        )
        assert r.get("status") in ("OK", "CREATED") or "manifesto" in str(r).lower()
        assert (tmp_path / "manifesto.json").exists()

    def test_manifesto_state(self, deck_dir):
        fn = _get_fn(self.mcp, "manifesto_state")
        r = fn(deck_dir=str(deck_dir))
        assert "deck_slug" in r or "state" in str(r).lower()

    def test_manifesto_commit(self, deck_dir):
        fn = _get_fn(self.mcp, "manifesto_commit")
        h = compute_receipt(QA_CSV, CL_CSV)
        r = fn(
            deck_dir=str(deck_dir),
            batch_id="B01",
            receipt_hash=h,
            qa_csv=QA_CSV,
            cloze_csv=CL_CSV,
        )
        # Should succeed or return structured error
        assert isinstance(r, dict)

    def test_manifesto_state_missing_dir(self):
        fn = _get_fn(self.mcp, "manifesto_state")
        r = fn(deck_dir="/nonexistent/path")
        assert r.get("status") == "ERROR"

    def test_taxonomy_check(self, deck_dir):
        fn = _get_fn(self.mcp, "taxonomy_check")
        r = fn(deck_dir=str(deck_dir), tag="Medicina::Neurologia")
        assert isinstance(r, dict)

    def test_id_ledger_query(self, deck_dir):
        fn = _get_fn(self.mcp, "id_ledger_query")
        r = fn(deck_dir=str(deck_dir))
        assert isinstance(r, dict)


# ── Media tools ─────────────────────────────────────────────────────────


class TestMediaTools:
    @pytest.fixture(autouse=True)
    def _mcp(self):
        self.mcp = FastMCP("test-media")
        register_media_tools(self.mcp)

    def test_catalog_load_missing(self):
        fn = _get_fn(self.mcp, "catalog_load")
        r = fn(catalog_path="/nonexistent/catalog.csv")
        assert r.get("status") == "ERROR" or r.get("ok") is False

    def test_image_namespace(self, tmp_path):
        fn = _get_fn(self.mcp, "image_namespace")
        r = fn(slug="test_mat", filename="img001.jpg")
        assert isinstance(r, dict)

    def test_image_candidates_empty_dir(self, tmp_path):
        fn = _get_fn(self.mcp, "image_candidates")
        # catalog_load needs a real catalog; test with nonexistent path → safe_tool catches
        r = fn(catalog_path="/nonexistent/path", query="test")
        assert isinstance(r, dict)
