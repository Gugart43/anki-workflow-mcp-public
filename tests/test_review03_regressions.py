"""Regression tests for revision 03 findings (docs/03-REVISAO-E-MELHORIAS.md).

One block per finding. Each test reproduces the original failure mode through the real
tool path and asserts the corrected behaviour.
"""

from __future__ import annotations

import asyncio
import json
from pathlib import Path

from filelock import FileLock

import anki_mcp.core.deck as deck_mod
from anki_mcp.core import receipt as receipt_mod
from anki_mcp.core.deck import load_manifesto, save_manifesto
from anki_mcp.core.receipt import RECEIPT_RE, compute_receipt, verify_receipt
from anki_mcp.core.rows import split_row
from anki_mcp.core.sanitize import auto_fix_csv_block
from anki_mcp.models import BatchMeta, Manifesto, NoteType, TaxonomyNode
from anki_mcp.server import mcp
from anki_mcp.tools import pipeline as pipeline_mod
from anki_mcp.tools import state as state_mod
from anki_mcp.tools import validate as validate_mod
from anki_mcp.tools.pipeline import _parse_csv

QA_H = (
    "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas"
)
CL_H = "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas"
QA_OK = (
    QA_H
    + "\nQual o tratamento da ELA?|Riluzol|Medicina::Neurologia::ELA||  |Kiernan 2011|ELA::Tratamento"
)
CL_OK = (
    CL_H + "\nO riluzol reduz a progressão da {{c1::ELA}} em média.|Fonte: Kiernan 2011|"
    "Medicina::Neurologia::ELA|||ELA::Tratamento"
)


def call(name: str, **kwargs):
    return asyncio.run(mcp.get_tool(name)).fn(**kwargs)


def _deck(tmp_path: Path, batch_state: str | None = None) -> Path:
    d = tmp_path / "deck"
    d.mkdir()
    m = Manifesto(
        deck_slug="t",
        campo_estudo="Medicina",
        created="2026-01-01",
        updated="2026-01-01",
        taxonomy=TaxonomyNode(root="ELA", nodes=["Tratamento"]),
    )
    if batch_state:
        m.batches["B01"] = BatchMeta(state=batch_state, cards=1)
    save_manifesto(d, m)
    return d


# ── B1 — over-limit batch crashed with AttributeError ─────────────────────────


def _qa_rows(n: int) -> str:
    rows = [
        f"Pergunta {i} sobre ELA?|Resposta {i}|Medicina::Neurologia::ELA|||Ref {i}|ELA::Tratamento"
        for i in range(n)
    ]
    return QA_H + "\n" + "\n".join(rows)


def test_b1_batch_of_25_cards_is_judged_not_crashed(tmp_path):
    d = _deck(tmp_path)
    qa = _qa_rows(25)
    gate = call(
        "pipeline_gate_check",
        deck_dir=str(d),
        batch_id="B01",
        phase="validated",
        qa_csv=qa,
        cloze_csv=CL_OK,
    )
    assert gate["status"] == "FAIL", gate
    assert gate["content_hash"] is None
    assert "BATCH.OVER_LIMIT" in gate["by_code"], gate["by_code"]

    vb = call("validate_batch", batch_id="B01", qa_csv=qa, cloze_csv=CL_OK)
    assert vb["result"] == "FAIL"
    assert any(e["code"] == "BATCH.OVER_LIMIT" for e in vb["errors"])
    assert vb["batch_count"]["within_limit"] is False and vb["batch_count"]["total"] == 26


def test_b1_batch_limit_fields_are_consistent_across_models_and_pipeline():
    from anki_mcp.checks.batch import count_batch

    bc = count_batch(_qa_rows(25), CL_OK, "B01")
    assert bc.ext_batch_suggested == "B01-EXT"
    assert not hasattr(bc, "ext_suggestion")


# ── B2 — receipt must be issued by the tools and verified by one formula ──────


def test_b2_gate_and_validate_issue_the_same_receipt(tmp_path):
    d = _deck(tmp_path, "started")
    gate = call(
        "pipeline_gate_check",
        deck_dir=str(d),
        batch_id="B01",
        phase="drafted",
        qa_csv=QA_OK,
        cloze_csv=CL_OK,
    )
    vb = call("validate_batch", batch_id="B01", qa_csv=QA_OK, cloze_csv=CL_OK)
    assert gate["status"] == "PASS" and vb["result"] == "PASS"
    assert RECEIPT_RE.match(gate["content_hash"])
    assert gate["content_hash"] == vb["content_hash"] == compute_receipt(QA_OK, CL_OK)


def test_b2_single_receipt_implementation_is_shared():
    # M4: one formula, imported by every consumer — no local copies.
    assert pipeline_mod.compute_receipt is receipt_mod.compute_receipt
    assert pipeline_mod.verify_receipt is receipt_mod.verify_receipt
    assert validate_mod.compute_receipt is receipt_mod.compute_receipt
    assert state_mod.verify_receipt is receipt_mod.verify_receipt
    assert verify_receipt(compute_receipt(QA_OK, CL_OK), QA_OK, CL_OK)
    assert not verify_receipt(compute_receipt(QA_OK, CL_OK), QA_OK + " ", CL_OK)


# ── B3 — exported files must pass the MCP's own BOM rule ──────────────────────


def test_b3_export_writes_utf8_without_bom_and_revalidates_clean(tmp_path):
    d = _deck(tmp_path, "validated")
    receipt = compute_receipt(QA_OK, CL_OK)
    res = call(
        "export_final",
        deck_dir=str(d),
        batch_id="B01",
        qa_csv=QA_OK,
        cloze_csv=CL_OK,
        receipt_hash=receipt,
    )
    assert res["status"] == "EXPORTED", res
    raw = Path(res["qa_file"]).read_bytes()
    assert not raw.startswith(b"\xef\xbb\xbf")
    assert (
        call("validate_csv_structure", csv_text=raw.decode("utf-8"), note_type="QA")["result"]
        == "PASS"
    )


# ── A1 — raw '|' shifts columns: semantic checks must not run on shifted values ─


def test_a1_split_row_never_pads_or_truncates():
    row = split_row("a|b|c|d|e|f|g|h", 7)
    assert row.count == 8 and row.structural_ok is False
    assert split_row("a|b|c|d|e|f|g", 7).structural_ok is True


def test_a1_shifted_row_is_structural_and_gets_no_field_values():
    bad = QA_H + "\nPergunta?|Resposta|Medicina|Nota com | pipe cru|||Tag::X"
    rows = _parse_csv(bad, NoteType.QA)
    assert len(rows) == 1
    assert rows[0].structural_ok is False and rows[0].column_count == 8
    assert rows[0].fields == {}  # no misaligned values to judge


def test_a1_gate_reports_field_shifted_and_skips_semantic_checks(tmp_path):
    d = _deck(tmp_path)
    bad = QA_H + "\nPergunta?|Resposta|Medicina|Nota com | pipe cru|||Tag::X"
    gate = call(
        "pipeline_gate_check",
        deck_dir=str(d),
        batch_id="B01",
        phase="drafted",
        qa_csv=bad,
        cloze_csv=CL_OK,
        detail="full",
    )
    assert gate["status"] == "FAIL"
    codes = {
        x["code"]
        for c in gate["checks"]
        if c["name"] == "csv_structure_qa"
        for x in c["diagnostics"]
    }
    assert {"FMT.COLUMN_COUNT", "FMT.FIELD_SHIFTED"} <= codes
    skipped = {c["name"]: c for c in gate["checks"] if c["result"] == "SKIP"}
    assert skipped["qa_row_1"]["reason"] == "SKIPPED_STRUCTURAL"
    # no semantic diagnostic may point at the shifted row
    semantic_on_row = [
        x["code"]
        for c in gate["checks"]
        for x in c["diagnostics"]
        if x.get("row") == 2 and x["code"] not in ("FMT.COLUMN_COUNT", "FMT.FIELD_SHIFTED")
    ]
    assert semantic_on_row == []


def test_a1_validate_batch_marks_shifted_rows_skipped():
    bad = QA_H + "\nPergunta?|Resposta|Medicina|Nota com | pipe cru|||Tag::X"
    vb = call("validate_batch", batch_id="B01", qa_csv=bad, cloze_csv=CL_OK)
    assert vb["result"] == "FAIL"
    assert any(s["reason"] == "SKIPPED_STRUCTURAL" for s in vb["skipped"])


# ── A2 — phase advance must be gated by a valid, recorded receipt ─────────────


def test_a2_broken_cards_cannot_advance_even_with_a_stale_pass(tmp_path):
    d = _deck(tmp_path, "drafted")
    ok = call(
        "pipeline_gate_check",
        deck_dir=str(d),
        batch_id="B01",
        phase="drafted",
        qa_csv=QA_OK,
        cloze_csv=CL_OK,
    )
    assert ok["status"] == "PASS"
    broken = QA_H + "\nPergunta?|Resposta|Medicina|Nota com | pipe cru|||Tag::X"
    fail = call(
        "pipeline_gate_check",
        deck_dir=str(d),
        batch_id="B01",
        phase="drafted",
        qa_csv=broken,
        cloze_csv=CL_OK,
    )
    assert fail["status"] == "FAIL"
    # the failing re-check invalidates the earlier receipt
    adv = call("pipeline_phase_advance", deck_dir=str(d), batch_id="B01")
    assert adv["status"] == "BLOCKED" and adv["code"] == "PIPELINE.GATE_REQUIRED"


def test_a2_invalid_recorded_gate_is_refused(tmp_path):
    d = _deck(tmp_path, "drafted")
    m = load_manifesto(d)
    m.batches["B01"].gates["drafted"] = "PASS"  # legacy, not a receipt
    save_manifesto(d, m)
    adv = call("pipeline_phase_advance", deck_dir=str(d), batch_id="B01", receipt_hash="PASS")
    assert adv["status"] == "BLOCKED" and adv["code"] == "PIPELINE.GATE_INVALID"


def test_a2_gate_for_unregistered_batch_is_reported_not_silent(tmp_path):
    d = _deck(tmp_path)
    gate = call(
        "pipeline_gate_check",
        deck_dir=str(d),
        batch_id="B99",
        phase="drafted",
        qa_csv=QA_OK,
        cloze_csv=CL_OK,
    )
    assert gate["status"] == "PASS" and gate["gate_recorded"] is False


# ── A3 — every tool answers with the canonical error shape ────────────────────


def test_a3_missing_deck_gives_structured_error_not_exception(tmp_path):
    for name, kwargs in [
        ("manifesto_state", {"deck_dir": str(tmp_path / "nope")}),
        ("pipeline_phase_advance", {"deck_dir": str(tmp_path / "nope"), "batch_id": "B01"}),
        ("deck_audit", {"deck_dir": str(tmp_path / "nope")}),
    ]:
        res = call(name, **kwargs)
        assert res["status"] == "ERROR" and res["code"] == "DECK.NOT_FOUND", (name, res)
        assert res["action"] and res["message"], name


def test_a3_guard_applies_to_tools_registered_without_decorator():
    from fastmcp import FastMCP

    from anki_mcp.core.safe import guard_tools

    probe = FastMCP("probe")
    guard_tools(probe)

    @probe.tool
    def boom() -> dict:
        raise KeyError("internal")

    res = asyncio.run(probe.get_tool("boom")).fn()
    assert res["status"] == "ERROR" and res["code"] == "INTERNAL.UNEXPECTED"
    assert {"message", "action"} <= res.keys()


# ── M1 — degradation is SKIP with a reason, never a silent PASS ───────────────


def test_m1_image_check_without_catalog_is_incomplete_not_pass(tmp_path):
    d = _deck(tmp_path)
    qa = QA_H + '\nQ?|R|Medicina||<figure><img src="mat_x.png"></figure>|Ref|ELA::Tratamento'
    gate = call(
        "pipeline_gate_check",
        deck_dir=str(d),
        batch_id="B01",
        phase="drafted",
        qa_csv=qa,
        cloze_csv=CL_OK,
    )
    assert gate["status"] == "INCOMPLETE", gate
    assert gate["content_hash"] is None
    assert gate["checks_skipped"] >= 1
    assert any(
        "catalog" in reason or "deck_dir" in reason or "anki_catalog" in reason
        for reason in gate["skipped"]
    ), gate["skipped"]


def test_m1_unreadable_catalog_is_reported_as_skip(tmp_path):
    d = _deck(tmp_path)
    (d / "anki_catalog.csv").write_bytes(b"\xff\xfe\x00bad|bytes\n")
    qa = QA_H + '\nQ?|R|Medicina||<figure><img src="mat_x.png"></figure>|Ref|ELA::Tratamento'
    gate = call(
        "pipeline_gate_check",
        deck_dir=str(d),
        batch_id="B01",
        phase="drafted",
        qa_csv=qa,
        cloze_csv=CL_OK,
    )
    assert gate["status"] == "INCOMPLETE"
    assert any("catalog unreadable" in reason for reason in gate["skipped"]), gate["skipped"]


def test_m1_catalog_present_verifies_images_and_flags_phantoms(tmp_path):
    d = _deck(tmp_path)
    (d / "anki_catalog.csv").write_text(
        "slug|filename|caption\nmat|x.png|Legenda\n", encoding="utf-8"
    )
    phantom = QA_H + '\nQ?|R|Medicina||<figure><img src="mat_y.png"></figure>|Ref|ELA::Tratamento'
    gate = call(
        "pipeline_gate_check",
        deck_dir=str(d),
        batch_id="B01",
        phase="drafted",
        qa_csv=phantom,
        cloze_csv=CL_OK,
    )
    assert gate["status"] == "FAIL"
    assert "IMG.PHANTOM" in gate["by_code"]


def test_m1_validate_batch_without_catalog_is_incomplete():
    qa = QA_H + '\nQ?|R|Medicina||<figure><img src="mat_x.png"></figure>|Ref|ELA::Tratamento'
    vb = call("validate_batch", batch_id="B01", qa_csv=qa, cloze_csv=CL_OK)
    assert vb["result"] == "INCOMPLETE" and vb["content_hash"] is None
    assert vb["checks_skipped"] >= 1


def test_m1_no_images_no_catalog_still_passes(tmp_path):
    # Nothing to verify → not a skip. Avoids blocking text-only batches.
    d = _deck(tmp_path)
    gate = call(
        "pipeline_gate_check",
        deck_dir=str(d),
        batch_id="B01",
        phase="drafted",
        qa_csv=QA_OK,
        cloze_csv=CL_OK,
    )
    assert gate["status"] == "PASS" and gate["checks_skipped"] == 0


def test_m1_plan_set_rejects_malformed_json(tmp_path):
    d = _deck(tmp_path)
    res = call(
        "manifesto_plan_set",
        deck_dir=str(d),
        total_planned=40,
        batches_planned="B01,B02",
        breakdown_json="not json",
    )
    assert res["status"] == "ERROR" and res["code"] == "ARGUMENT.INVALID", res


def test_m1_corrupt_hierarchy_file_is_an_error_not_a_silent_resync(tmp_path, monkeypatch):
    bad = tmp_path / "h.json"
    bad.write_text("{not json", encoding="utf-8")
    from anki_mcp.tools import hierarchy as hm

    monkeypatch.setattr(hm, "get_hierarchy_file", lambda: bad)
    res = call("deck_hierarchy_get")
    assert res["status"] == "ERROR" and res["code"] == "HIERARCHY.CORRUPT", res


# ── M2 — export validates read-only first and rolls back on failure ───────────


def test_m2_duplicate_ids_in_payload_block_before_any_write(tmp_path):
    d = _deck(tmp_path, "validated")
    entry = {
        "id": "B01-QA-001",
        "batch": "B01",
        "type": "QA",
        "status": "active",
        "topic": "T",
        "anchor": "A",
    }
    before = (d / "manifesto.json").read_text(encoding="utf-8")
    res = call(
        "export_final",
        deck_dir=str(d),
        batch_id="B01",
        qa_csv=QA_OK,
        cloze_csv=CL_OK,
        receipt_hash=compute_receipt(QA_OK, CL_OK),
        new_ids_json=json.dumps([entry, entry]),
    )
    assert res["status"] == "BLOCKED" and res["code"] == "ID.DUPLICATE"
    assert not (d / "exports").exists()
    assert (d / "manifesto.json").read_text(encoding="utf-8") == before


def test_m2_failure_midway_rolls_everything_back(tmp_path, monkeypatch):
    d = _deck(tmp_path, "validated")
    before = (d / "manifesto.json").read_text(encoding="utf-8")

    def boom(*_a, **_k):
        raise OSError("disk full (injected)")

    monkeypatch.setattr(pipeline_mod, "save_manifesto", boom)
    res = call(
        "export_final",
        deck_dir=str(d),
        batch_id="B01",
        qa_csv=QA_OK,
        cloze_csv=CL_OK,
        receipt_hash=compute_receipt(QA_OK, CL_OK),
        write_to_deck_root=True,
    )
    assert res["status"] == "ERROR" and res["code"] == "INTERNAL.UNEXPECTED"
    assert not (d / "exports" / "B01_QA.csv").exists()
    assert not (d / "exports" / "B01_Cloze.csv").exists()
    assert not (d / "B01_QA.csv").exists()
    assert not (d / "manifesto.md").exists() or "exported" not in (d / "manifesto.md").read_text(
        encoding="utf-8"
    )
    assert (d / "manifesto.json").read_text(encoding="utf-8") == before
    assert not (d / "exports" / ".tmp").exists()


def test_m2_export_respects_the_deck_lock(tmp_path, monkeypatch):
    d = _deck(tmp_path, "validated")
    monkeypatch.setattr(deck_mod, "_LOCK_TIMEOUT_S", 0.2)
    with FileLock(str(d / ".anki-workflow.lock")):
        res = call(
            "export_final",
            deck_dir=str(d),
            batch_id="B01",
            qa_csv=QA_OK,
            cloze_csv=CL_OK,
            receipt_hash=compute_receipt(QA_OK, CL_OK),
        )
    assert res["status"] == "ERROR" and res["code"] == "GOV.LOCK", res
    assert not (d / "exports").exists()


# ── M3 — no stripping of field content ────────────────────────────────────────


def test_m3_fields_keep_their_exact_text():
    rows = _parse_csv(QA_H + "\nStem |Resp  |C|N||R|T ", NoteType.QA)
    assert rows[0].fields["Enunciado"] == "Stem "
    assert rows[0].fields["Resposta"] == "Resp  "
    assert rows[0].fields["Etiquetas"] == "T "


def test_m3_sanitizer_keeps_trailing_space_of_last_field():
    out, _ = auto_fix_csv_block(CL_H + "\n{{c1::A}} x |N |C||R|T \n", "CLOZE")
    assert out.splitlines()[1].endswith("|T ")


# ── M5 — export is idempotent ─────────────────────────────────────────────────


def test_m5_second_export_with_same_receipt_does_not_duplicate_changelog(tmp_path):
    d = _deck(tmp_path, "validated")
    receipt = compute_receipt(QA_OK, CL_OK)
    cl = json.dumps([{"date": "2026-10-12", "batch": "B01", "action": "X", "reason": "y"}])
    first = call(
        "export_final",
        deck_dir=str(d),
        batch_id="B01",
        qa_csv=QA_OK,
        cloze_csv=CL_OK,
        receipt_hash=receipt,
        changelog_json=cl,
    )
    n = len(load_manifesto(d).changelog)
    second = call(
        "export_final",
        deck_dir=str(d),
        batch_id="B01",
        qa_csv=QA_OK,
        cloze_csv=CL_OK,
        receipt_hash=receipt,
        changelog_json=cl,
    )
    assert first["status"] == "EXPORTED" and second["status"] == "ALREADY_EXPORTED"
    assert len(load_manifesto(d).changelog) == n


# ── M6 — gate payload is aggregated by default ────────────────────────────────


def test_m6_compact_gate_payload_is_aggregated_and_smaller(tmp_path):
    d = _deck(tmp_path)
    qa = _qa_rows(20)
    cl = (
        CL_H
        + "\n"
        + "\n".join(
            f"Frase {i} com {{{{c1::alvo{i}}}}} e {{{{c2::outro{i}}}}}.|N|Medicina|||Tag"
            for i in range(20)
        )
    )
    compact = call(
        "pipeline_gate_check",
        deck_dir=str(d),
        batch_id="B01",
        phase="drafted",
        qa_csv=qa,
        cloze_csv=cl,
    )
    full = call(
        "pipeline_gate_check",
        deck_dir=str(d),
        batch_id="B01",
        phase="drafted",
        qa_csv=qa,
        cloze_csv=cl,
        detail="full",
    )
    assert "checks" not in compact and "by_code" in compact
    assert "checks" in full
    c_len, f_len = len(json.dumps(compact)), len(json.dumps(full))
    assert c_len * 2 < f_len, (c_len, f_len)
