"""End-to-end flow on a temporary deck (revision 03, §2.1 flow test).

manifesto_init → pipeline_phase_advance (through every phase, each gated by a PASS
pipeline_gate_check) → validate_batch → export_final → idempotent re-run.

Only registered MCP tools are used; the receipt is always taken from a tool result,
never computed by the test (the LLM must not compute the hash either — B2).
"""

from __future__ import annotations

import asyncio
import json
from pathlib import Path

import pytest

from anki_mcp.checks.csv_structure import check_bom
from anki_mcp.core.deck import load_manifesto
from anki_mcp.server import mcp

QA_CSV = (
    "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas\n"
    "Qual é o tratamento de primeira linha da ELA?|Riluzol|Medicina::Neurologia::ELA||  |Kiernan 2011|ELA::Tratamento"
)
CLOZE_CSV = (
    "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas\n"
    "O riluzol reduz a progressão da {{c1::ELA}} em média.|Fonte: Kiernan 2011|Medicina::Neurologia::ELA|||ELA::Tratamento"
)
NEW_IDS = json.dumps(
    [
        {
            "id": "B01-QA-001",
            "batch": "B01",
            "type": "QA",
            "status": "active",
            "topic": "ELA::Tratamento",
            "anchor": "Kiernan 2011",
        }
    ]
)
CHANGELOG = json.dumps(
    [{"date": "2026-10-12", "batch": "B01", "action": "VALIDAÇÃO", "reason": "e2e"}]
)


def call(name: str, **kwargs):
    return asyncio.run(mcp.get_tool(name)).fn(**kwargs)


@pytest.fixture
def deck(tmp_path: Path) -> Path:
    d = tmp_path / "deck"
    d.mkdir()
    res = call(
        "manifesto_init",
        deck_dir=str(d),
        campo_estudo="Medicina::Neurologia::ELA",
        deck_slug="ela",
        taxonomy_root="ELA",
    )
    assert res["status"] in ("CREATED", "OK", "INITIALIZED") or (d / "manifesto.json").exists(), res
    assert (d / "manifesto.json").exists()
    return d


def _advance_through_gates(deck: Path) -> str:
    """Drive B01 from new → validated, each step gated by a PASS receipt from the tool."""
    adv = call("pipeline_phase_advance", deck_dir=str(deck), batch_id="B01")
    assert adv["status"] == "ADVANCED" and adv["current_state"] == "started", adv
    for phase in ("drafted", "curated", "enriched", "illustrated"):
        adv = call("pipeline_phase_advance", deck_dir=str(deck), batch_id="B01")
        assert adv["status"] == "ADVANCED", (phase, adv)  # previous gate must be recorded
        gate = call(
            "pipeline_gate_check",
            deck_dir=str(deck),
            batch_id="B01",
            phase=phase,
            qa_csv=QA_CSV,
            cloze_csv=CLOZE_CSV,
        )
        assert gate["status"] == "PASS", (phase, gate)
        assert gate["content_hash"].startswith("sha256:")
    adv = call("pipeline_phase_advance", deck_dir=str(deck), batch_id="B01")
    assert adv["status"] == "ADVANCED" and adv["current_state"] == "validated", adv
    gate = call(
        "pipeline_gate_check",
        deck_dir=str(deck),
        batch_id="B01",
        phase="validated",
        qa_csv=QA_CSV,
        cloze_csv=CLOZE_CSV,
    )
    assert gate["status"] == "PASS", gate
    return gate["content_hash"]


def test_end_to_end_validate_gate_export_is_idempotent(deck: Path):
    receipt = _advance_through_gates(deck)

    # validate_batch emits the SAME receipt as the gate (single formula, B2).
    vb = call(
        "validate_batch",
        batch_id="B01",
        qa_csv=QA_CSV,
        cloze_csv=CLOZE_CSV,
    )
    assert vb["result"] == "PASS", vb
    assert vb["content_hash"] == receipt

    # Export with the receipt returned by the tool — no hash computed by the caller.
    exp = call(
        "export_final",
        deck_dir=str(deck),
        batch_id="B01",
        qa_csv=QA_CSV,
        cloze_csv=CLOZE_CSV,
        receipt_hash=receipt,
        new_ids_json=NEW_IDS,
        new_topics_json=json.dumps(["ELA::Tratamento"]),
        changelog_json=CHANGELOG,
    )
    assert exp["status"] == "EXPORTED", exp

    # Files land where the contract says, byte-for-byte, without BOM.
    exports = deck / "exports"
    for path in (
        exports / "B01_QA.csv",
        exports / "B01_Cloze.csv",
        deck / "B01_QA.csv",
        deck / "B01_Cloze.csv",
    ):
        assert path.exists(), path
    assert (exports / "B01_QA.csv").read_text(encoding="utf-8") == QA_CSV
    assert (exports / "B01_Cloze.csv").read_text(encoding="utf-8") == CLOZE_CSV
    assert check_bom((exports / "B01_QA.csv").read_bytes()) == []
    assert not (exports / ".tmp").exists(), "staging dir must be removed"

    # The exported files pass the MCP's own structural validation (B3 round trip).
    assert call("validate_csv_structure", csv_text=QA_CSV, note_type="QA")["result"] == "PASS"
    assert call("validate_csv_structure", csv_text=CLOZE_CSV, note_type="CL")["result"] == "PASS"

    # Manifesto updated.
    m = load_manifesto(deck)
    assert m.batches["B01"].state == "exported"
    assert m.batches["B01"].gates["exported"] == receipt
    assert [e.id for e in m.id_registry] == ["B01-QA-001"]
    assert m.totals["active"] == 1 and m.totals["qa"] == 1

    # Re-run with the same receipt: nothing written, changelog not duplicated (M5).
    changelog_before = len(m.changelog)
    again = call(
        "export_final",
        deck_dir=str(deck),
        batch_id="B01",
        qa_csv=QA_CSV,
        cloze_csv=CLOZE_CSV,
        receipt_hash=receipt,
        new_ids_json=NEW_IDS,
        new_topics_json=json.dumps(["ELA::Tratamento"]),
        changelog_json=CHANGELOG,
    )
    assert again["status"] == "ALREADY_EXPORTED", again
    assert len(load_manifesto(deck).changelog) == changelog_before

    # An exported batch cannot be advanced again.
    adv = call("pipeline_phase_advance", deck_dir=str(deck), batch_id="B01")
    assert adv["status"] == "ERROR" and adv["code"] == "PIPELINE.TERMINAL"


def test_export_refuses_a_receipt_from_other_content(deck: Path):
    receipt = _advance_through_gates(deck)
    other_qa = QA_CSV.replace("Riluzol", "Edaravone")  # content changed after validation
    exp = call(
        "export_final",
        deck_dir=str(deck),
        batch_id="B01",
        qa_csv=other_qa,
        cloze_csv=CLOZE_CSV,
        receipt_hash=receipt,
    )
    assert exp["status"] == "BLOCKED" and exp["code"] == "EXPORT.NO_RECEIPT", exp
    assert not (deck / "exports").exists()
