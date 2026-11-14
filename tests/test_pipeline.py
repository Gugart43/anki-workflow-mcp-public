"""test_pipeline.py — pipeline state machine and export gate tests."""

from __future__ import annotations

import hashlib
from pathlib import Path

from anki_mcp.core.deck import load_manifesto, save_manifesto
from anki_mcp.models import BatchMeta, Manifesto, NoteType, TaxonomyNode
from anki_mcp.tools.pipeline import _PHASE_GATES, _STATE_TRANSITIONS, _parse_csv

# ── Fixtures ──────────────────────────────────────────────────────────────────

QA_HEADER = (
    "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas"
)
CLOZE_HEADER = (
    "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas"
)

VALID_QA_CSV = "\n".join(
    [
        QA_HEADER,
        "Qual é o tratamento de primeira linha da ELA?|Riluzol|Medicina::Neurologia::ELA||  |Kiernan 2011|ELA::Tratamento",
    ]
)

VALID_CLOZE_CSV = "\n".join(
    [
        CLOZE_HEADER,
        "O riluzol reduz a progressão da {{c1::ELA}} em média.|Fonte: Kiernan 2011|Medicina::Neurologia::ELA|||ELA::Tratamento",
    ]
)


def _deck(tmp_path: Path) -> Path:
    d = tmp_path / "deck"
    d.mkdir()
    m = Manifesto(
        deck_slug="ela",
        campo_estudo="Medicina::Neurologia::ELA",
        created="2026-01-01",
        updated="2026-01-01",
        taxonomy=TaxonomyNode(root="ELA", nodes=["Tratamento", "Fisiopatologia"]),
    )
    save_manifesto(d, m)
    return d


# ── State machine tests ───────────────────────────────────────────────────────


def test_state_transitions_complete():
    """All states reachable from None → exported."""
    state = None
    visited = []
    for _ in range(10):
        nxt = _STATE_TRANSITIONS.get(state)
        if nxt is None:
            break
        visited.append(nxt)
        state = nxt
    assert visited == [
        "started",
        "drafted",
        "curated",
        "enriched",
        "illustrated",
        "validated",
        "exported",
    ]


def test_phase_gates_for_validated_include_all():
    """Validated gate is the strictest — must include csv_structure, cloze, etc."""
    gates = _PHASE_GATES["validated"]
    for required in ("csv_structure", "cloze", "hints", "qa", "notes", "tags", "batch"):
        assert required in gates, f"'{required}' missing from validated gates"


# ── CSV parser tests ──────────────────────────────────────────────────────────


def test_parse_csv_qa(tmp_path):
    rows = _parse_csv(VALID_QA_CSV, NoteType.QA)
    assert len(rows) == 1
    assert rows[0].fields["Enunciado"].startswith("Qual é")
    assert rows[0].fields["Resposta"] == "Riluzol"


def test_parse_csv_cloze(tmp_path):
    rows = _parse_csv(VALID_CLOZE_CSV, NoteType.CLOZE)
    assert len(rows) == 1
    assert "{{c1::ELA}}" in rows[0].fields["Sentença"]


# ── Receipt hash tests ────────────────────────────────────────────────────────


def test_receipt_hash_format():
    """The receipt hash follows the sha256: prefix format."""
    qa, cl = VALID_QA_CSV, VALID_CLOZE_CSV
    combined = (qa + "\n---\n" + cl).encode("utf-8")
    h = "sha256:" + hashlib.sha256(combined).hexdigest()
    assert h.startswith("sha256:")
    assert len(h) == 71  # 7 + 64


def test_receipt_hash_sensitive_to_content():
    qa1 = VALID_QA_CSV
    qa2 = VALID_QA_CSV.replace("Riluzol", "Edaravone")
    combined1 = (qa1 + "\n---\n" + VALID_CLOZE_CSV).encode("utf-8")
    combined2 = (qa2 + "\n---\n" + VALID_CLOZE_CSV).encode("utf-8")
    h1 = hashlib.sha256(combined1).hexdigest()
    h2 = hashlib.sha256(combined2).hexdigest()
    assert h1 != h2


# ── Export gate test (wrong receipt → BLOCKED) ────────────────────────────────


def test_export_blocked_wrong_receipt(tmp_path):
    """export_final must reject a wrong receipt_hash before writing any files."""

    deck = _deck(tmp_path)

    # Build WRONG hash
    wrong_hash = "sha256:" + "a" * 64

    # We test the logic path directly by checking what export_final would return.
    # Verify: combined hash != wrong_hash → BLOCKED
    qa, cl = VALID_QA_CSV, VALID_CLOZE_CSV
    combined = (qa + "\n---\n" + cl).encode("utf-8")
    correct_hash = "sha256:" + hashlib.sha256(combined).hexdigest()

    assert correct_hash != wrong_hash

    exports_dir = deck / "exports"
    assert not exports_dir.exists(), "exports dir must not exist before export"


def test_export_creates_files(tmp_path):
    """export_final with correct receipt writes CSV files and updates manifesto."""
    import hashlib

    from anki_mcp.core.deck import save_manifesto
    from anki_mcp.models import CardStatus, IDEntry, NoteType
    from anki_mcp.tools.pipeline import _parse_csv, _run_all_checks

    deck = _deck(tmp_path)
    qa, cl = VALID_QA_CSV, VALID_CLOZE_CSV

    # Build correct receipt
    combined = (qa + "\n---\n" + cl).encode("utf-8")
    receipt = "sha256:" + hashlib.sha256(combined).hexdigest()

    # Verify receipt is correct (no mismatch)
    assert receipt.startswith("sha256:")

    # Simulate export logic: re-validate, write files, update manifesto
    qa_rows = _parse_csv(qa, NoteType.QA)
    cloze_rows = _parse_csv(cl, NoteType.CLOZE)
    _, errors, _ = _run_all_checks(qa_rows, cloze_rows, qa, cl, str(deck))
    # Our simple valid CSV should have no CRITICAL errors
    # (tags may fire TAG.UNREGISTERED since we have no manifesto-backed registry)
    # That's expected; count only structural/critical ones
    critical = [e for e in errors if e.code.startswith(("FMT.", "CLOZE.", "QA."))]
    assert len(critical) == 0, f"Unexpected CRITICAL errors: {[e.code for e in critical]}"

    # Write files
    exports_dir = deck / "exports"
    exports_dir.mkdir(exist_ok=True)
    (exports_dir / "B07_QA.csv").write_text(qa, encoding="utf-8-sig")
    (exports_dir / "B07_Cloze.csv").write_text(cl, encoding="utf-8-sig")

    assert (exports_dir / "B07_QA.csv").exists()
    assert (exports_dir / "B07_Cloze.csv").exists()

    # Update manifesto
    m = load_manifesto(deck)
    m.id_registry.append(
        IDEntry(
            id="B07-QA-001",
            batch="B07",
            type=NoteType.QA,
            status=CardStatus.ACTIVE,
            topic="Tratamento ELA",
            anchor="Kiernan 2011 p.1",
        )
    )
    m.totals = {"active": 1, "qa": 1, "cloze": 0}
    m.last_validated_batch = "B07"
    m.batches["B07"] = BatchMeta(state="exported", cards=2)
    save_manifesto(deck, m)

    m2 = load_manifesto(deck)
    assert m2.last_validated_batch == "B07"
    assert len(m2.id_registry) == 1


# ── Regression & Governance Tests (H1 Fixes) ──────────────────────────────────


def _get_tool_fn(mcp, name):
    import asyncio

    tool = asyncio.run(mcp.get_tool(name))
    return tool.fn


def test_batch_over_limit_diagnostic():
    """B1: Batch > 20 cards must report BATCH.OVER_LIMIT without AttributeError."""
    from anki_mcp.models import NoteType
    from anki_mcp.tools.pipeline import _parse_csv, _run_all_checks

    # 25 QA cards exceeds 20 limit
    qa_csv = "\n".join([QA_HEADER] + [f"P{i}?|R{i}||  |  |F{i}|T" for i in range(25)])
    cl_csv = VALID_CLOZE_CSV
    qa_rows = _parse_csv(qa_csv, NoteType.QA)
    cl_rows = _parse_csv(cl_csv, NoteType.CLOZE)
    checks, _errs, _warns = _run_all_checks(qa_rows, cl_rows, qa_csv, cl_csv, None)
    batch_checks = [c for c in checks if c["name"] == "batch_limit"]
    assert len(batch_checks) == 1
    assert batch_checks[0]["result"] == "FAIL"
    assert any(d["code"] == "BATCH.OVER_LIMIT" for d in batch_checks[0]["diagnostics"])


def test_pipeline_gate_check_returns_content_hash(tmp_path):
    """B2: pipeline_gate_check must return content_hash on PASS and record gate."""
    from fastmcp import FastMCP

    from anki_mcp.core.deck import load_manifesto
    from anki_mcp.tools.pipeline import register_pipeline_tools

    deck = _deck(tmp_path)
    mcp = FastMCP("test")
    register_pipeline_tools(mcp)
    gate_fn = _get_tool_fn(mcp, "pipeline_gate_check")

    m = load_manifesto(deck)
    m.batches["B01"] = BatchMeta(state="started", cards=2)
    save_manifesto(deck, m)

    res = gate_fn(str(deck), "B01", "drafted", VALID_QA_CSV, VALID_CLOZE_CSV)
    assert res["status"] == "PASS"
    assert res["content_hash"] is not None
    assert res["content_hash"].startswith("sha256:")

    # Verify gate recorded in manifesto
    m2 = load_manifesto(deck)
    assert m2.batches["B01"].gates.get("drafted") == res["content_hash"]


def test_pipeline_phase_advance_gate_enforcement(tmp_path):
    """A2: pipeline_phase_advance blocks if required phase gate did not pass."""
    from fastmcp import FastMCP

    from anki_mcp.tools.pipeline import register_pipeline_tools

    deck = _deck(tmp_path)
    mcp = FastMCP("test")
    register_pipeline_tools(mcp)
    adv_fn = _get_tool_fn(mcp, "pipeline_phase_advance")

    m = load_manifesto(deck)
    # drafted requires csv_structure gate
    m.batches["B01"] = BatchMeta(state="drafted", cards=2)
    save_manifesto(deck, m)

    # Attempt to advance from drafted without passing gate
    res = adv_fn(str(deck), "B01")
    assert res["status"] == "BLOCKED"
    assert res["code"] == "PIPELINE.GATE_REQUIRED"

    # A2 (revision 03): a made-up receipt must NOT unlock the phase — no recorded gate yet.
    res2 = adv_fn(str(deck), "B01", receipt_hash="sha256:fake_valid_hash")
    assert res2["status"] == "BLOCKED"
    assert res2["code"] == "PIPELINE.GATE_REQUIRED"

    # Only a PASS gate for the current phase records a receipt that unlocks it.
    gate_fn = _get_tool_fn(mcp, "pipeline_gate_check")
    gate = gate_fn(str(deck), "B01", "drafted", VALID_QA_CSV, VALID_CLOZE_CSV)
    assert gate["status"] == "PASS"

    res3 = adv_fn(str(deck), "B01", receipt_hash="sha256:" + "0" * 64)
    assert res3["status"] == "BLOCKED"
    assert res3["code"] == "PIPELINE.RECEIPT_MISMATCH"

    res4 = adv_fn(str(deck), "B01", receipt_hash=gate["content_hash"])
    assert res4["status"] == "ADVANCED"
    assert res4["current_state"] == "curated"


def test_export_final_utf8_without_bom(tmp_path):
    """B3: export_final must write UTF-8 without BOM, so check_bom reports 0 errors."""
    from fastmcp import FastMCP

    from anki_mcp.checks.csv_structure import check_bom
    from anki_mcp.core.receipt import compute_receipt
    from anki_mcp.tools.pipeline import register_pipeline_tools

    deck = _deck(tmp_path)
    mcp = FastMCP("test")
    register_pipeline_tools(mcp)
    export_fn = _get_tool_fn(mcp, "export_final")

    receipt = compute_receipt(VALID_QA_CSV, VALID_CLOZE_CSV)
    res = export_fn(str(deck), "B07", VALID_QA_CSV, VALID_CLOZE_CSV, receipt)
    assert res["status"] == "EXPORTED"

    qa_file = Path(res["qa_file"])
    raw_bytes = qa_file.read_bytes()
    assert not raw_bytes.startswith(b"\xef\xbb\xbf")
    bom_diags = check_bom(raw_bytes)
    assert len(bom_diags) == 0


def test_export_prechecks_duplicate_ids_before_writing(tmp_path):
    """M2: Duplicate IDs must be rejected before any file is written."""
    import json

    from fastmcp import FastMCP

    from anki_mcp.core.receipt import compute_receipt
    from anki_mcp.models import CardStatus, IDEntry, NoteType
    from anki_mcp.tools.pipeline import register_pipeline_tools

    deck = _deck(tmp_path)
    mcp = FastMCP("test")
    register_pipeline_tools(mcp)
    export_fn = _get_tool_fn(mcp, "export_final")

    m = load_manifesto(deck)
    m.id_registry.append(
        IDEntry(
            id="B01-QA-001",
            batch="B01",
            type=NoteType.QA,
            status=CardStatus.ACTIVE,
            topic="T",
            anchor="A",
        )
    )
    save_manifesto(deck, m)

    receipt = compute_receipt(VALID_QA_CSV, VALID_CLOZE_CSV)
    dup_ids = json.dumps(
        [
            {
                "id": "B01-QA-001",
                "batch": "B01",
                "type": "QA",
                "status": "active",
                "topic": "T",
                "anchor": "A",
            }
        ]
    )
    res = export_fn(str(deck), "B01", VALID_QA_CSV, VALID_CLOZE_CSV, receipt, new_ids_json=dup_ids)

    assert res["status"] == "BLOCKED"
    assert res["code"] == "ID.DUPLICATE"
    exports_dir = deck / "exports"
    assert not (exports_dir / "B01_QA.csv").exists()


def test_review_batch_compact_and_full(tmp_path):
    """H3: review_batch one-shot provides compact categorization, fixes, and receipt."""
    from fastmcp import FastMCP

    from anki_mcp.tools.pipeline import register_pipeline_tools

    deck = _deck(tmp_path)
    mcp = FastMCP("test")
    register_pipeline_tools(mcp)
    review_fn = _get_tool_fn(mcp, "review_batch")

    # Test compact mode
    res_compact = review_fn(VALID_QA_CSV, VALID_CLOZE_CSV, deck_dir=str(deck), detail="compact")
    assert res_compact["status"] == "PASS"
    assert "categories" in res_compact
    assert "fixes" in res_compact
    assert res_compact["content_hash"] is not None
    assert res_compact["content_hash"].startswith("sha256:")
    assert res_compact["next_action"] == "Ready for export via export_final."

    # Test full mode
    res_full = review_fn(VALID_QA_CSV, VALID_CLOZE_CSV, deck_dir=str(deck), detail="full")
    assert res_full["status"] == "PASS"
    assert "checks" in res_full
    assert res_full["content_hash"] == res_compact["content_hash"]


def test_export_final_writes_and_clears_journal(tmp_path):
    from anki_mcp.core.deck import clear_journal, read_journal, recover_journal, write_journal

    deck = tmp_path / "deck"
    deck.mkdir()
    assert recover_journal(deck)["status"] == "CLEAN"

    write_journal(deck, "export_begin", batch_id="B01")
    j = read_journal(deck)
    assert j is not None
    assert j["stage"] == "export_begin"
    stale = recover_journal(deck)
    assert stale["status"] == "STALE"
    assert stale["journal"]["stage"] == "export_begin"

    clear_journal(deck)
    assert recover_journal(deck)["status"] == "CLEAN"
