"""tools/pipeline.py — pipeline orchestration and export MCP tools.

These tools implement the Phase 0-6 gates from anki-card-generator skill,
enforced in code rather than by LLM self-assertion.

Tools:
  pipeline_phase_advance  — advance batch state machine; leaving a phase requires its recorded receipt
  pipeline_gate_check     — evaluate all checks for a phase gate; PASS records the receipt
  review_batch            — one-shot review (same engine as the gate), aggregated output
  gap_request             — log a gap/decision point that halts the pipeline
  export_final            — transactional export: read-only validation, staged writes,
                            rollback on failure, idempotent re-run
  deck_audit              — deck health audit (orphans, phantoms, integrity)

Verdicts (gate / review / export):
  FAIL        — at least one CRITICAL diagnostic
  INCOMPLETE  — no CRITICAL, but a check could not run (SKIP with reason); no receipt
                is issued: "could not verify" is never reported as PASS
  PASS        — every check ran and none is CRITICAL; receipt issued
"""

from __future__ import annotations

import csv
import io
import json
import os
import re
import shutil
import tempfile
from datetime import date
from pathlib import Path
from typing import Any

from fastmcp import FastMCP

from anki_mcp.checks.batch import count_batch
from anki_mcp.checks.information import compact_information_profiles
from anki_mcp.core.deck import load_manifesto, save_manifesto, transaction, write_journal
from anki_mcp.core.evaluate import (
    EvaluationResult,
    evaluate_batch,
)
from anki_mcp.core.evaluate import (
    load_catalog as _load_catalog,
)
from anki_mcp.core.evaluate import (
    parse_csv as _parse_csv,
)
from anki_mcp.core.evaluate import (
    run_all_checks as _run_all_checks,
)
from anki_mcp.core.receipt import RECEIPT_RE, compute_receipt, verify_receipt

__all__ = [
    "EvaluationResult",
    "_load_catalog",
    "_parse_csv",
    "_run_all_checks",
    "compute_receipt",
    "evaluate_batch",
    "register_pipeline_tools",
]
from anki_mcp.core.safe import error_result, safe_tool
from anki_mcp.models import (
    BatchMeta,
    CardStatus,
    ChangelogEntry,
    Diagnostic,
    IDEntry,
    NoteType,
)

# ── State machine ─────────────────────────────────────────────────────────────

_VALID_STATES = [
    "started",
    "drafted",
    "curated",
    "enriched",
    "illustrated",
    "validated",
    "exported",
]

_STATE_TRANSITIONS: dict[str | None, str | None] = {
    None: "started",
    "started": "drafted",
    "drafted": "curated",
    "curated": "enriched",
    "enriched": "illustrated",
    "illustrated": "validated",
    "validated": "exported",
}

# Checks required to advance THROUGH each phase gate. Leaving phase P requires the
# receipt recorded for P by pipeline_gate_check (enforced in pipeline_phase_advance).
_PHASE_GATES: dict[str, list[str]] = {
    "drafted": ["csv_structure"],
    "curated": ["csv_structure", "cloze", "hints", "qa", "notes"],
    "enriched": ["csv_structure", "notes"],
    "illustrated": ["images"],
    "validated": ["csv_structure", "cloze", "hints", "qa", "notes", "tags", "batch", "images"],
    "exported": [],  # receipt-based only (export_final)
}

_CHECK_RESULTS = ("PASS", "WARN", "FAIL", "SKIP")


def _verdict(checks: list[dict[str, Any]], errors: list[Diagnostic]) -> str:
    if errors:
        return "FAIL"
    if any(c["result"] == "SKIP" for c in checks):
        return "INCOMPLETE"
    return "PASS"


def _aggregate(checks: list[dict[str, Any]]) -> dict[str, Any]:
    """Compact view: one entry per diagnostic code with affected rows; skips grouped by reason."""
    by_code: dict[str, dict[str, Any]] = {}
    for c in checks:
        for d in c["diagnostics"]:
            entry = by_code.setdefault(
                d["code"],
                {
                    "severity": d["severity"],
                    "count": 0,
                    "rows": [],
                    "suggestion": d.get("suggestion"),
                },
            )
            entry["count"] += 1
            row = d.get("row")
            if row is not None and row not in entry["rows"]:
                entry["rows"].append(row)
    for entry in by_code.values():
        entry["rows"].sort()

    skipped: dict[str, dict[str, Any]] = {}
    for c in checks:
        if c["result"] != "SKIP":
            continue
        group = skipped.setdefault(c.get("reason") or "SKIP", {"count": 0, "rows": []})
        group["count"] += 1
        if c.get("row") is not None and c["row"] not in group["rows"]:
            group["rows"].append(c["row"])

    counts = {r: sum(1 for c in checks if c["result"] == r) for r in _CHECK_RESULTS}
    return {"by_code": by_code, "skipped": skipped, "check_counts": counts}


def _evaluate(qa_csv: str, cloze_csv: str, deck_dir: str | None) -> EvaluationResult:
    """The single evaluation engine shared by gate, review and export. Read-only."""
    return evaluate_batch(qa_csv, cloze_csv, deck_dir=deck_dir)


def _record_gate(deck_dir: str, batch_id: str, phase: str, receipt: str | None) -> bool:
    """Store the receipt for *phase* (or invalidate it when receipt is None).

    Returns False when the batch is not registered in the manifesto yet.
    """
    with transaction(deck_dir) as m:
        if batch_id not in m.batches:
            return False
        gates = m.batches[batch_id].gates
        if receipt:
            gates[phase] = receipt
        else:
            # A failing re-check must invalidate an earlier PASS, or the stale receipt
            # would still unlock pipeline_phase_advance.
            gates.pop(phase, None)
        save_manifesto(deck_dir, m)
        return True


def _gate_payload(
    ev: dict[str, Any] | EvaluationResult,
    batch_id: str,
    phase: str,
    detail: str,
    recorded: bool | None,
) -> dict[str, Any]:
    checks = ev["checks"]
    skipped = sum(1 for c in checks if c["result"] == "SKIP")
    payload: dict[str, Any] = {
        "status": ev["verdict"],
        "batch": batch_id,
        "phase": phase,
        "total_qa": len(ev["qa_rows"]),
        "total_cloze": len(ev["cloze_rows"]),
        "checks_run": len(checks),
        "checks_skipped": skipped,
        "errors": len(ev["errors"]),
        "warnings": len(ev["warnings"]),
        "content_hash": ev["receipt"],
        "gate_recorded": recorded,
        "summary": (
            f"{ev['verdict']}: {len(ev['errors'])} CRITICAL, {len(ev['warnings'])} WARN, "
            f"{skipped} SKIP across {len(ev['qa_rows'])} QA + {len(ev['cloze_rows'])} Cloze cards"
        ),
    }
    if ev["verdict"] == "INCOMPLETE":
        payload["action"] = (
            "Some checks could not run (see skipped). Provide deck_dir containing "
            "anki_catalog.csv so images can be verified; no receipt is issued until then."
        )
    info_profiles = (
        ev.get("information_profiles", [])
        if isinstance(ev, dict)
        else getattr(ev, "information_profiles", [])
    )
    if detail == "full":
        payload["checks"] = checks
        payload["information_profiles"] = info_profiles
        return payload
    agg = _aggregate(checks)
    payload["by_code"] = agg["by_code"]
    payload["skipped"] = agg["skipped"]
    payload["check_counts"] = agg["check_counts"]
    compact_profiles, guidance = compact_information_profiles(info_profiles)
    payload["information_profiles"] = compact_profiles
    payload["information_guidance"] = guidance
    return payload


# ── Export helpers ────────────────────────────────────────────────────────────


def _stage(stage_dir: Path, target: Path, content: str) -> Path:
    """Write *content* (UTF-8, no BOM) to a fsynced temp file in *stage_dir*."""
    fd, tmp = tempfile.mkstemp(dir=stage_dir, prefix="stage_", suffix=target.suffix or ".tmp")
    with os.fdopen(fd, "wb") as fh:
        fh.write(content.encode("utf-8"))
        fh.flush()
        os.fsync(fh.fileno())
    return Path(tmp)


def _rollback(backups: dict[Path, bytes | None]) -> list[str]:
    """Restore every target to its pre-export bytes (or remove it if it did not exist)."""
    failures: list[str] = []
    for target, previous in backups.items():
        try:
            if previous is None:
                target.unlink(missing_ok=True)
            else:
                target.write_bytes(previous)
        except OSError as exc:
            failures.append(f"{target}: {exc}")
    return failures


def _unsupported_phase(phase: str) -> dict[str, Any]:
    return error_result(
        "ARGUMENT.INVALID",
        f"Unknown phase '{phase}'. Valid: {list(_PHASE_GATES)}",
        "Use one of the listed phases.",
    )


# ── Tool registration ─────────────────────────────────────────────────────────


def register_pipeline_tools(mcp: FastMCP) -> None:

    @mcp.tool(
        description=(
            "Advance a batch through the pipeline state machine. "
            "deck_dir: absolute path to the deck folder. batch_id: e.g. 'B07'. "
            "Leaving a phase requires the receipt recorded by a PASS of "
            "pipeline_gate_check(phase=<current state>). receipt_hash is optional; if given "
            "it must equal that recorded receipt. "
            "Allowed transitions: started→drafted→curated→enriched→illustrated→validated→exported."
        )
    )
    @safe_tool
    def pipeline_phase_advance(
        deck_dir: str, batch_id: str, receipt_hash: str | None = None
    ) -> dict[str, Any]:
        with transaction(deck_dir) as m:
            current_meta = m.batches.get(batch_id)
            current_state = current_meta.state if current_meta else None

            if current_state == "exported":
                return error_result(
                    "PIPELINE.TERMINAL",
                    f"{batch_id} is already exported. No further transitions.",
                    "Use a new batch id for further work.",
                )

            next_state = _STATE_TRANSITIONS.get(current_state)
            if next_state is None:
                return error_result(
                    "PIPELINE.INVALID_STATE",
                    f"Unknown state '{current_state}' for {batch_id}",
                    "Inspect manifesto_state(sections='batches') and correct the batch state.",
                )

            phase_key = current_state or ""
            if _PHASE_GATES.get(phase_key):
                stored = current_meta.gates.get(phase_key) if current_meta else None
                if not stored:
                    return {
                        "status": "BLOCKED",
                        "code": "PIPELINE.GATE_REQUIRED",
                        "message": f"Cannot advance {batch_id} from '{current_state}' to "
                        f"'{next_state}': no passing gate is recorded for '{current_state}'.",
                        "action": f"Run pipeline_gate_check(phase='{current_state}') until PASS.",
                    }
                if not RECEIPT_RE.match(stored):
                    return {
                        "status": "BLOCKED",
                        "code": "PIPELINE.GATE_INVALID",
                        "message": f"The gate recorded for '{current_state}' is not a receipt.",
                        "action": f"Re-run pipeline_gate_check(phase='{current_state}').",
                    }
                if receipt_hash is not None and receipt_hash != stored:
                    return {
                        "status": "BLOCKED",
                        "code": "PIPELINE.RECEIPT_MISMATCH",
                        "message": "receipt_hash does not match the gate recorded for this phase.",
                        "action": f"Use the content_hash from "
                        f"pipeline_gate_check(phase='{current_state}').",
                    }

            if batch_id not in m.batches:
                m.batches[batch_id] = BatchMeta(state=next_state, cards=0)
            else:
                m.batches[batch_id].state = next_state
            m.updated = str(date.today())
            m.changelog.append(
                ChangelogEntry(
                    date=str(date.today()),
                    batch=batch_id,
                    action=f"PHASE_{next_state.upper()}",
                    reason=f"Batch {batch_id} advanced to {next_state}",
                )
            )
            save_manifesto(deck_dir, m)

        return {
            "status": "ADVANCED",
            "batch": batch_id,
            "previous_state": current_state or "new",
            "current_state": next_state,
            "gates_for_next": _PHASE_GATES.get(next_state, []),
            "next_advance_to": _STATE_TRANSITIONS.get(next_state),
        }

    @mcp.tool(
        description=(
            "Evaluate all gate checks for a pipeline phase without advancing the state. "
            "A PASS records the receipt for the batch (content_hash); a FAIL or INCOMPLETE "
            "invalidates any earlier receipt for that phase. "
            "Also returns read-only information_profiles (3-layer breakdown per row, excluded from "
            "receipt hash; length-only signals are advisory WARN, never blocking). "
            "deck_dir: deck folder (manifesto + anki_catalog.csv). batch_id: e.g. 'B07'. "
            "phase: drafted|curated|enriched|illustrated|validated. "
            "qa_csv / cloze_csv: full CSV texts. "
            "detail: 'compact' (default; diagnostics aggregated by code with affected rows) "
            "or 'full' (every check). "
            "Verdict: PASS | INCOMPLETE (a check could not run; no receipt) | FAIL."
        )
    )
    @safe_tool
    def pipeline_gate_check(
        deck_dir: str,
        batch_id: str,
        phase: str,
        qa_csv: str,
        cloze_csv: str,
        detail: str = "compact",
    ) -> dict[str, Any]:
        if phase not in _PHASE_GATES:
            return _unsupported_phase(phase)
        ev = _evaluate(qa_csv, cloze_csv, deck_dir)
        recorded = _record_gate(deck_dir, batch_id, phase, ev["receipt"])
        return _gate_payload(ev, batch_id, phase, detail, recorded)

    @mcp.tool(
        description=(
            "One-shot batch review: runs every check over QA + Cloze CSVs and returns an "
            "aggregated report with fix hints, read-only information_profiles (separating retrieved "
            "target, necessary context, and optional explanation; semantic_review_required=True; "
            "no automatic QA->Cloze conversion; excluded from receipt hash), and the receipt "
            "(content_hash on PASS). "
            "If deck_dir is given and batch_id is registered, the gate is recorded like "
            "pipeline_gate_check. detail: 'compact' (default) or 'full'."
        )
    )
    @safe_tool
    def review_batch(
        qa_csv: str,
        cloze_csv: str,
        phase: str = "validated",
        deck_dir: str | None = None,
        batch_id: str = "B01",
        detail: str = "compact",
    ) -> dict[str, Any]:
        if phase not in _PHASE_GATES:
            return _unsupported_phase(phase)
        ev = _evaluate(qa_csv, cloze_csv, deck_dir)
        recorded = _record_gate(deck_dir, batch_id, phase, ev["receipt"]) if deck_dir else None
        payload = _gate_payload(ev, batch_id, phase, detail, recorded)
        if detail == "full":
            return payload

        categories: dict[str, dict[str, Any]] = {}
        for c in ev["checks"]:
            prefix = c["name"].split("_")[0]
            cat = categories.setdefault(prefix, {"status": "PASS", "count": 0, "issues": 0})
            cat["count"] += 1
            if c["result"] == "FAIL":
                cat["status"] = "FAIL"
                cat["issues"] += 1
            elif c["result"] in ("WARN", "SKIP"):
                if cat["status"] != "FAIL":
                    cat["status"] = c["result"]
                cat["issues"] += 1

        payload["categories"] = categories
        payload["fixes"] = [
            {"code": code, "count": e["count"], "rows": e["rows"], "suggestion": e["suggestion"]}
            for code, e in payload["by_code"].items()
            if e["severity"] in ("CRITICAL", "WARN")
        ]
        if ev["verdict"] == "PASS":
            payload["next_action"] = "Ready for export via export_final."
        elif ev["verdict"] == "FAIL":
            payload["next_action"] = (
                f"Resolve {len(ev['errors'])} critical error(s) before advancing or exporting."
            )
        else:
            payload["next_action"] = payload["action"]
        return payload

    @mcp.tool(
        description=(
            "Log a gap or unresolved decision point that halts the pipeline. "
            "Per the Gap Request Protocol in anki-card-generator: when the pipeline "
            "encounters a decision without a governing rule, it MUST halt and log here "
            "instead of inventing ad-hoc rules. "
            "deck_dir: deck folder path. batch_id: e.g. 'B07'. "
            "gap_description: clear description of the decision needed. "
            "context: relevant snippet or options. "
            "Returns a gap_id and logs it to the Manifesto suggestion queue."
        )
    )
    @safe_tool
    def gap_request(
        deck_dir: str,
        batch_id: str,
        gap_description: str,
        context: str = "",
    ) -> dict[str, Any]:
        with transaction(deck_dir) as m:
            n_gaps = len([q for q in m.suggestion_queue if q.startswith("GAP-")])
            gap_id = f"GAP-{batch_id}-{n_gaps + 1:03d}"

            entry = f"{gap_id}: {gap_description}"
            if context:
                entry += f" | Context: {context[:200]}"

            m.suggestion_queue.append(entry)
            m.changelog.append(
                ChangelogEntry(
                    date=str(date.today()),
                    batch=batch_id,
                    action="GAP_REQUEST",
                    reason=gap_description[:120],
                )
            )
            m.updated = str(date.today())
            save_manifesto(deck_dir, m)

        return {
            "status": "LOGGED",
            "gap_id": gap_id,
            "batch": batch_id,
            "description": gap_description,
            "action": "PIPELINE HALTED — resolve this gap before advancing the batch.",
        }

    @mcp.tool(
        description=(
            "Final transactional export of a validated batch; the SOLE writer of the final "
            "CSV files and of the Manifesto post-export. Requires the content_hash returned by "
            "pipeline_gate_check / validate_batch on PASS. "
            "Order: receipt check and full re-validation (read-only) → under one deck lock: "
            "ID checks, staged writes, replace, manifesto save; any failure rolls files back. "
            "Re-running with the same receipt on an exported batch returns ALREADY_EXPORTED "
            "and writes nothing. "
            "Parameters: deck_dir; batch_id; qa_csv; cloze_csv; receipt_hash; "
            "new_ids_json (JSON array of {id, batch, type, status, topic, anchor}); "
            "new_topics_json (JSON array of strings); changelog_json (JSON array of changelog "
            "objects); topic_slug (optional, e.g. 'EscleroseMultipla'); write_to_deck_root "
            "(also copy into the deck root, default True). "
            "Writes: deck_dir/exports/{batch_id}[_{topic_slug}]_QA.csv and _Cloze.csv."
        )
    )
    @safe_tool
    def export_final(
        deck_dir: str,
        batch_id: str,
        qa_csv: str,
        cloze_csv: str,
        receipt_hash: str,
        new_ids_json: str = "[]",
        new_topics_json: str = "[]",
        changelog_json: str = "[]",
        topic_slug: str | None = None,
        write_to_deck_root: bool = True,
    ) -> dict[str, Any]:
        # Filenames are identifiers, never paths (on Windows or Linux).
        for name, value in (("batch_id", batch_id), ("topic_slug", topic_slug)):
            if value is None and name == "topic_slug":
                continue
            if not value or not re.fullmatch(r"[\w-]+", value, flags=re.UNICODE):
                raise ValueError(
                    f"{name} must contain only letters, numbers, underscores or hyphens"
                )
        # ── 1. Read-only: receipt, payloads, full validation ──────────────────
        if not verify_receipt(receipt_hash, qa_csv, cloze_csv):
            return {
                "status": "BLOCKED",
                "code": "EXPORT.NO_RECEIPT",
                "message": "Receipt hash mismatch. Run pipeline_gate_check(phase='validated') "
                "first and pass its content_hash as receipt_hash.",
                "action": "Re-validate the exact CSV text that is being exported.",
            }

        new_ids = [IDEntry.model_validate(r) for r in json.loads(new_ids_json)]
        new_topics = [str(t) for t in json.loads(new_topics_json)]
        changelog = [ChangelogEntry.model_validate(r) for r in json.loads(changelog_json)]

        ev = _evaluate(qa_csv, cloze_csv, deck_dir)
        if ev["verdict"] != "PASS":
            return {
                "status": "BLOCKED",
                "code": "EXPORT.VALIDATION_FAIL",
                "message": f"Re-validation verdict is {ev['verdict']} "
                f"({len(ev['errors'])} CRITICAL). Fix before export.",
                "action": "Run review_batch(detail='full') and fix the reported rows.",
                "errors": [e.model_dump(mode="json") for e in ev["errors"][:10]],
            }

        counts = count_batch(qa_csv, cloze_csv, batch_id)
        filename_prefix = f"{batch_id}_{topic_slug}" if topic_slug else batch_id
        deck = Path(deck_dir)
        exports_dir = deck / "exports"
        qa_path = exports_dir / f"{filename_prefix}_QA.csv"
        cloze_path = exports_dir / f"{filename_prefix}_Cloze.csv"
        md_path = deck / "manifesto.md"

        # ── 2. One transaction for the whole write cycle with write-ahead journal ──
        with transaction(deck_dir) as m:
            write_journal(deck_dir, "export_begin", batch_id=batch_id)
            meta = m.batches.get(batch_id)
            if meta is not None and any(
                meta.gates.get(gate) not in (None, receipt_hash)
                for gate in ("committed", "exported")
            ):
                return {
                    "status": "BLOCKED",
                    "code": "EXPORT.BATCH_CONFLICT",
                    "message": "Batch content differs from an existing receipt.",
                    "action": "Use a new batch id.",
                }
            if meta is not None and meta.state == "exported":
                if meta.gates.get("exported") == receipt_hash:
                    return {
                        "status": "ALREADY_EXPORTED",
                        "batch_id": batch_id,
                        "receipt": receipt_hash,
                        "message": "Same receipt already exported; nothing was written.",
                    }
                return {
                    "status": "BLOCKED",
                    "code": "EXPORT.BATCH_CONFLICT",
                    "message": f"{batch_id} was already exported with different content.",
                    "action": "Use a new batch id for different content.",
                }

            existing = {e.id for e in m.id_registry}
            seen: set[str] = set()
            pending_ids: list[IDEntry] = []
            for entry in new_ids:
                prior_entry = next((e for e in m.id_registry if e.id == entry.id), None)
                if (
                    entry.id not in seen
                    and prior_entry == entry
                    and meta is not None
                    and meta.gates.get("committed") == receipt_hash
                    and entry.batch == batch_id
                ):
                    seen.add(entry.id)
                    continue
                if entry.id in existing or entry.id in seen:
                    return {
                        "status": "BLOCKED",
                        "code": "ID.DUPLICATE",
                        "message": f"ID {entry.id} already exists in the ledger. "
                        "No files were written.",
                        "action": "Assign unique IDs in new_ids_json.",
                    }
                seen.add(entry.id)
                pending_ids.append(entry)

            # Build the post-export manifesto in memory — nothing is written yet.
            m.id_registry.extend(pending_ids)
            for topic in new_topics:
                if topic not in m.topic_log:
                    m.topic_log.append(topic)
            m.changelog.extend(changelog)
            m.changelog.append(
                ChangelogEntry(
                    date=str(date.today()),
                    batch=batch_id,
                    action="EXPORTAÇÃO",
                    reason=f"Batch {batch_id} exported: {len(ev['qa_rows'])} QA + "
                    f"{len(ev['cloze_rows'])} Cloze cards",
                )
            )
            active = [e for e in m.id_registry if e.status == CardStatus.ACTIVE]
            m.totals = {
                "active": len(active),
                "qa": sum(1 for e in active if e.type == NoteType.QA),
                "cloze": sum(1 for e in active if e.type == NoteType.CLOZE),
            }
            m.last_validated_batch = batch_id
            m.updated = str(date.today())
            m.batches[batch_id] = BatchMeta(
                state="exported",
                cards=counts.total,
                gates={**(meta.gates if meta else {}), "exported": receipt_hash},
            )

            from anki_mcp.core.render import render_manifesto_md

            targets: list[tuple[Path, str]] = [(qa_path, qa_csv), (cloze_path, cloze_csv)]
            if write_to_deck_root:
                targets += [
                    (deck / f"{filename_prefix}_QA.csv", qa_csv),
                    (deck / f"{filename_prefix}_Cloze.csv", cloze_csv),
                ]
            targets.append((md_path, render_manifesto_md(m)))

            # ── 3. Stage → replace → save manifesto; rollback on any failure ──
            stage_dir = exports_dir / ".tmp"
            stage_dir.mkdir(parents=True, exist_ok=True)
            staged: list[tuple[Path, Path]] = []
            backups: dict[Path, bytes | None] = {}
            try:
                for target, content in targets:
                    target.parent.mkdir(parents=True, exist_ok=True)
                    staged.append((target, _stage(stage_dir, target, content)))
                    backups[target] = target.read_bytes() if target.exists() else None
                for target, tmp in staged:
                    os.replace(tmp, target)
                write_journal(deck_dir, "files_replaced", batch_id=batch_id)
                save_manifesto(deck_dir, m)  # last: the manifesto records a completed export
            except BaseException:
                failures = _rollback(backups)
                write_journal(
                    deck_dir,
                    "rollback_failed" if failures else "rolled_back",
                    batch_id=batch_id,
                    errors=failures,
                )
                raise
            finally:
                shutil.rmtree(stage_dir, ignore_errors=True)

        return {
            "status": "EXPORTED",
            "batch_id": batch_id,
            "receipt": receipt_hash,
            "qa_cards": len(ev["qa_rows"]),
            "cloze_cards": counts.cloze_indices,
            "cloze_notes": counts.cloze_notes,
            "total_cards": counts.total,
            "registered_notes": sum(
                e.batch == batch_id for e in m.id_registry if e.status == CardStatus.ACTIVE
            ),
            "unregistered_notes": max(
                0,
                counts.qa_cards
                + counts.cloze_notes
                - sum(e.batch == batch_id for e in m.id_registry if e.status == CardStatus.ACTIVE),
            ),
            "ledger_complete": sum(
                e.batch == batch_id for e in m.id_registry if e.status == CardStatus.ACTIVE
            )
            == counts.qa_cards + counts.cloze_notes,
            "ids_added": [e.id for e in new_ids],
            "totals": m.totals,
            "qa_file": str(qa_path),
            "cloze_file": str(cloze_path),
            "manifesto_md": str(md_path),
        }

    # ── deck_audit ────────────────────────────────────────────────────────────

    @mcp.tool(
        name="deck_audit",
        description="Full deck health audit: orphan IDs, phantom IDs, batch integrity, and coverage summary.",
    )
    @safe_tool
    def deck_audit(deck_dir: str) -> dict[str, Any]:
        """Run a comprehensive health audit on the entire deck.

        Checks:
        - **orphan_ids**: IDs in the manifesto id_registry with no matching CSV file
        - **phantom_ids**: IDs found in CSV files but absent from id_registry
        - **duplicate_topics**: topics that appear in multiple batches
        - **batch_integrity**: each batch's CSV files exist and have correct row counts
        - **summary**: overall deck health metrics
        """
        manifesto = load_manifesto(deck_dir)
        deck = Path(deck_dir)

        registry_ids: set[str] = set()
        batch_topics: dict[str, list[str]] = {}
        for entry in manifesto.id_registry:
            if getattr(entry, "status", None) in (CardStatus.RETIRED, "retired"):
                continue
            registry_ids.add(entry.id)
            batch_topics.setdefault(entry.batch, []).append(entry.topic or "")

        csv_ids: set[str] = set()
        batch_files: dict[str, dict[str, Any]] = {}
        id_tag_pattern = re.compile(r"\bid::([A-Za-z0-9_-]+)")
        for csv_path in sorted(deck.glob("**/*.csv")):
            if csv_path.name.startswith("."):
                continue
            try:
                text = csv_path.read_text(encoding="utf-8")
                rows = list(csv.reader(io.StringIO(text), delimiter="|"))
                if len(rows) < 2:
                    continue
                header = rows[0]
                id_col = None
                for i, h in enumerate(header):
                    if h.strip().upper() in ("ID", "IDENTIFICADOR"):
                        id_col = i
                        break
                data_rows = rows[1:]
                for row in data_rows:
                    if id_col is not None and len(row) > id_col and row[id_col].strip():
                        csv_ids.add(row[id_col].strip())
                    else:
                        # Fallback: scan columns for id::<CARD_ID> tags
                        for cell in row:
                            match = id_tag_pattern.search(cell)
                            if match:
                                csv_ids.add(match.group(1))

                # Infer batch from filename (e.g. B01-QA.csv or B01_catarata_QA.csv -> B01)
                stem = csv_path.stem
                batch_id = re.split(r"[_\-]", stem)[0] or stem
                note_type = (
                    "QA" if "QA" in stem.upper() else "Cloze" if "CL" in stem.upper() else "unknown"
                )
                entry_files = batch_files.setdefault(
                    batch_id,
                    {
                        "batch": batch_id,
                        "qa_file": None,
                        "cloze_file": None,
                        "qa_rows": 0,
                        "cloze_rows": 0,
                        "issues": [],
                    },
                )
                if note_type == "QA":
                    entry_files["qa_file"] = str(csv_path)
                    entry_files["qa_rows"] = len(data_rows)
                elif note_type == "Cloze":
                    entry_files["cloze_file"] = str(csv_path)
                    entry_files["cloze_rows"] = len(data_rows)
            except (OSError, csv.Error):
                continue

        orphan_ids = sorted(registry_ids - csv_ids)
        phantom_ids = sorted(csv_ids - registry_ids)

        topic_batches: dict[str, list[str]] = {}
        for batch, topics in batch_topics.items():
            for topic in topics:
                if topic:
                    topic_batches.setdefault(topic, []).append(batch)
        duplicate_topics = {t: bs for t, bs in topic_batches.items() if len(bs) > 1}

        batch_integrity = sorted(batch_files.values(), key=lambda x: x["batch"])

        all_topics = {entry.topic for entry in manifesto.id_registry if entry.topic}
        issues_count = len(orphan_ids) + len(phantom_ids) + len(duplicate_topics)
        health = "HEALTHY" if issues_count == 0 else "WARN" if issues_count < 5 else "UNHEALTHY"

        return {
            "status": "OK",
            "orphan_ids": orphan_ids,
            "phantom_ids": phantom_ids,
            "duplicate_topics": duplicate_topics,
            "batch_integrity": batch_integrity,
            "summary": {
                "total_ids": len(registry_ids),
                "total_batches": len(batch_topics),
                "total_topics": len(all_topics),
                "csv_ids_found": len(csv_ids),
                "orphan_count": len(orphan_ids),
                "phantom_count": len(phantom_ids),
                "duplicate_topic_count": len(duplicate_topics),
                "health": health,
            },
        }
