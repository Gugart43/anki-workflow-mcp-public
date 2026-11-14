"""core/evaluate.py — single evaluation engine shared by validate, gate, review and export.

Eliminates divergence between validate_batch and pipeline evaluation (class B2 bug).
"""

from __future__ import annotations

import csv
from pathlib import Path
from typing import Any

from pydantic import BaseModel, Field

from anki_mcp.checks.batch import count_batch
from anki_mcp.checks.cloze import check_cloze
from anki_mcp.checks.crossrefs import check_crossrefs
from anki_mcp.checks.csv_structure import check_bom, check_csv
from anki_mcp.checks.hints import check_hints
from anki_mcp.checks.images import check_images_field
from anki_mcp.checks.information import profile_information
from anki_mcp.checks.notes import check_notes
from anki_mcp.checks.qa import check_qa
from anki_mcp.checks.tables import check_tables_field
from anki_mcp.checks.tags import check_tags
from anki_mcp.core.receipt import compute_receipt
from anki_mcp.core.rows import iter_data_lines, split_row
from anki_mcp.models import (
    CLOZE_FIELDS,
    QA_FIELDS,
    BatchCount,
    CardRow,
    Diagnostic,
    NoteType,
    Severity,
    ValidationResult,
)


class EvaluationResult(BaseModel):
    """Structured result returned by the central evaluation engine."""

    verdict: ValidationResult
    qa_rows: list[CardRow]
    cloze_rows: list[CardRow]
    checks: list[dict[str, Any]]
    errors: list[Diagnostic] = Field(default_factory=list)
    warnings: list[Diagnostic] = Field(default_factory=list)
    skipped: list[dict[str, Any]] = Field(default_factory=list)
    batch_count: BatchCount
    information_profiles: list[dict[str, Any]] = Field(default_factory=list)
    receipt: str | None = None

    def __getitem__(self, item: str) -> Any:
        return getattr(self, item)


def parse_csv(csv_text: str, note_type: NoteType) -> list[CardRow]:
    """Parse CSV text into CardRow models, reporting structural failures per row."""
    fields = QA_FIELDS if note_type == NoteType.QA else CLOZE_FIELDS
    lines = csv_text.splitlines()
    has_header = (
        bool(lines) and "|" in lines[0] and any(f.lower() in lines[0].lower() for f in fields)
    )
    numbered = (
        iter_data_lines(csv_text)
        if has_header
        else [(i, ln) for i, ln in enumerate(lines, start=1) if ln.strip()]
    )
    rows: list[CardRow] = []
    for lineno, raw in numbered:
        split = split_row(raw, len(fields))
        field_map = dict(zip(fields, split.cols, strict=True)) if split.structural_ok else {}
        rows.append(
            CardRow(
                row_number=lineno,
                note_type=note_type,
                fields=field_map,
                raw=raw,
                column_count=split.count,
                structural_ok=split.structural_ok,
            )
        )
    return rows


def load_catalog(deck_dir: str | Path | None) -> tuple[set[str] | None, str | None]:
    """Find and load anki_catalog.csv under *deck_dir* if available."""
    if not deck_dir:
        return None, "no deck_dir provided"
    deck = Path(deck_dir)
    files = [
        deck / "anki_catalog.csv",
        deck / "catalog.csv",
        deck / "media" / "anki_catalog.csv",
        deck / "media" / "catalog.csv",
    ]
    files = [f for f in files if f.is_file()]
    if not files:
        return None, "no anki_catalog.csv found under deck_dir"
    names: set[str] = set()
    problems: list[str] = []
    for path in files:
        try:
            with path.open(encoding="utf-8") as fh:
                for row in csv.DictReader(fh, delimiter="|"):
                    slug = (row.get("slug") or "").strip()
                    fn = (row.get("filename") or "").strip()
                    if slug and fn:
                        names.add(f"{slug}_{fn}")
                    elif fn:
                        names.add(fn)
        except (OSError, UnicodeDecodeError, csv.Error) as exc:
            problems.append(f"{path.name}: {type(exc).__name__}: {exc}")
    if problems:
        return None, "catalog unreadable (" + "; ".join(problems) + ")"
    if not names:
        return None, "catalog has no entries"
    return names, None


def run_all_checks(
    qa_rows: list[CardRow],
    cloze_rows: list[CardRow],
    qa_csv: str,
    cloze_csv: str,
    deck_dir: str | None = None,
    *,
    taxonomy_tags: set[str] | None = None,
    active_ids: set[str] | None = None,
    current_batch_ids: set[str] | None = None,
    catalog_filenames: set[str] | None = None,
    batch_id: str = "B01",
) -> tuple[list[dict[str, Any]], list[Diagnostic], list[Diagnostic]]:
    """Run all validation checks over parsed rows. Returns (checks, errors, warnings)."""
    all_rows = qa_rows + cloze_rows
    checks: list[dict[str, Any]] = []
    errors: list[Diagnostic] = []
    warnings: list[Diagnostic] = []

    def _add(name: str, diags: list[Diagnostic]) -> None:
        crits = [d for d in diags if d.severity == Severity.CRITICAL]
        warns = [d for d in diags if d.severity == Severity.WARN]
        result = "FAIL" if crits else "PASS"
        checks.append(
            {
                "name": name,
                "result": result,
                "diagnostics": [d.model_dump() for d in diags],
            }
        )
        errors.extend(crits)
        warnings.extend(warns)

    def _skip(name: str, reason: str, row: int | None) -> None:
        checks.append(
            {
                "name": name,
                "result": "SKIP",
                "reason": reason,
                "row": row,
                "diagnostics": [],
            }
        )

    # 1. Structure checks (including BOM and delimiter)
    qa_diags = list(check_bom(qa_csv.encode("utf-8", errors="replace"))) + check_csv(qa_csv, "QA")
    cloze_diags = list(check_bom(cloze_csv.encode("utf-8", errors="replace"))) + check_csv(
        cloze_csv, "CL"
    )
    _add("csv_structure_qa", qa_diags)
    _add("csv_structure_cloze", cloze_diags)

    # 2. Batch limit
    bc = count_batch(qa_csv, cloze_csv, batch_id)
    if bc.within_limit:
        _add("batch_limit", [])
    else:
        suggestion = (
            f" Consider splitting into {bc.ext_batch_suggested}." if bc.ext_batch_suggested else ""
        )
        _add(
            "batch_limit",
            [
                Diagnostic.from_code(
                    "BATCH.OVER_LIMIT",
                    f"Batch has {bc.total} Anki cards (limit: 20). Excess: {bc.excess}.{suggestion}",
                    suggestion="Split the batch so each has at most 20 cards",
                )
            ],
        )

    # 3. Cloze rows
    for i, row in enumerate(cloze_rows):
        if not row.structural_ok:
            _skip(f"cloze_row_{i + 1}", "SKIPPED_STRUCTURAL", row.row_number)
            _skip(f"hints_row_{i + 1}", "SKIPPED_STRUCTURAL", row.row_number)
            _skip(f"tables_sentence_{i + 1}", "SKIPPED_STRUCTURAL", row.row_number)
            continue
        sentence = row.fields.get("Sentença", "")
        _add(f"cloze_row_{i + 1}", check_cloze(sentence, row=row.row_number))
        _add(f"hints_row_{i + 1}", check_hints(sentence, row=row.row_number))
        _add(
            f"tables_sentence_{i + 1}",
            check_tables_field(sentence, field="Sentença", row=row.row_number, face=True),
        )

    # 4. QA rows
    for i, row in enumerate(qa_rows):
        if not row.structural_ok:
            _skip(f"qa_row_{i + 1}", "SKIPPED_STRUCTURAL", row.row_number)
            _skip(f"tables_enunciado_{i + 1}", "SKIPPED_STRUCTURAL", row.row_number)
            _skip(f"tables_resposta_{i + 1}", "SKIPPED_STRUCTURAL", row.row_number)
            continue
        _add(
            f"qa_row_{i + 1}",
            check_qa(
                row.fields.get("Enunciado", ""),
                row.fields.get("Resposta", ""),
                row=row.row_number,
            ),
        )
        _add(
            f"tables_enunciado_{i + 1}",
            check_tables_field(
                row.fields.get("Enunciado", ""),
                field="Enunciado",
                row=row.row_number,
                face=True,
            ),
        )
        _add(
            f"tables_resposta_{i + 1}",
            check_tables_field(
                row.fields.get("Resposta", ""),
                field="Resposta",
                row=row.row_number,
                face=False,
            ),
        )

    # 5. Common fields on all rows: tags, notes, tables, crossrefs
    for i, row in enumerate(all_rows):
        if not row.structural_ok:
            _skip(f"tags_{i + 1}", "SKIPPED_STRUCTURAL", row.row_number)
            _skip(f"notes_{i + 1}", "SKIPPED_STRUCTURAL", row.row_number)
            _skip(f"tables_notes_{i + 1}", "SKIPPED_STRUCTURAL", row.row_number)
            _skip(f"crossrefs_{i + 1}", "SKIPPED_STRUCTURAL", row.row_number)
            continue
        _add(
            f"tags_{i + 1}",
            check_tags(
                row.fields.get("Etiquetas", ""),
                taxonomy_nodes=taxonomy_tags,
                row=row.row_number,
            ),
        )
        notes = row.fields.get("Notas Adicionais", "")
        # CL uses empty answer in check_notes; per-index restatement stays advisory in information_profiles.
        ans_for_notes = row.fields.get("Resposta", "") if row.note_type == NoteType.QA else ""
        _add(f"notes_{i + 1}", check_notes(notes, answer=ans_for_notes, row=row.row_number))
        _add(
            f"tables_notes_{i + 1}",
            check_tables_field(notes, field="Notas Adicionais", row=row.row_number, face=False),
        )
        _add(
            f"crossrefs_{i + 1}",
            check_crossrefs(
                notes,
                active_ids=active_ids,
                current_batch_ids=current_batch_ids,
                row=row.row_number,
            ),
        )

    # 6. Images on all rows
    catalog = catalog_filenames
    catalog_reason: str | None = None
    if catalog is None and deck_dir:
        catalog, catalog_reason = load_catalog(deck_dir)

    for i, row in enumerate(all_rows):
        name = f"images_{i + 1}"
        if not row.structural_ok:
            _skip(name, "SKIPPED_STRUCTURAL", row.row_number)
            continue
        img_field = row.fields.get("Imagens Relacionadas", "")
        if catalog is None and "<img" in img_field.lower():
            _skip(name, catalog_reason or "catalog unavailable", row.row_number)
            continue
        _add(name, check_images_field(img_field, catalog, row=row.row_number))

    return checks, errors, warnings


def evaluate_batch(
    qa_csv: str,
    cloze_csv: str,
    deck_dir: str | None = None,
    *,
    taxonomy_tags: set[str] | None = None,
    active_ids: set[str] | None = None,
    current_batch_ids: set[str] | None = None,
    catalog_filenames: set[str] | None = None,
    batch_id: str = "B01",
) -> EvaluationResult:
    """The canonical single evaluation engine for all flashcard batch validation."""
    qa_rows = parse_csv(qa_csv, NoteType.QA)
    cloze_rows = parse_csv(cloze_csv, NoteType.CLOZE)
    checks, errors, warnings = run_all_checks(
        qa_rows,
        cloze_rows,
        qa_csv,
        cloze_csv,
        deck_dir=deck_dir,
        taxonomy_tags=taxonomy_tags,
        active_ids=active_ids,
        current_batch_ids=current_batch_ids,
        catalog_filenames=catalog_filenames,
        batch_id=batch_id,
    )
    skipped = [
        {"check": c["name"], "reason": c.get("reason"), "row": c.get("row")}
        for c in checks
        if c["result"] == "SKIP"
    ]
    if errors:
        verdict = ValidationResult.FAIL
    elif any(c["result"] == "SKIP" for c in checks):
        verdict = ValidationResult.INCOMPLETE
    else:
        verdict = ValidationResult.PASS

    bc = count_batch(qa_csv, cloze_csv, batch_id)
    receipt = compute_receipt(qa_csv, cloze_csv) if verdict == ValidationResult.PASS else None

    information_profiles: list[dict[str, Any]] = []
    for row in qa_rows:
        if not row.structural_ok:
            continue
        prof = profile_information(
            note_type="QA",
            prompt=row.fields.get("Enunciado", ""),
            answer=row.fields.get("Resposta", ""),
            notes=row.fields.get("Notas Adicionais", ""),
        )
        prof["row"] = row.row_number
        information_profiles.append(prof)
    for row in cloze_rows:
        if not row.structural_ok:
            continue
        prof = profile_information(
            note_type="CL",
            prompt=row.fields.get("Sentença", ""),
            answer="",
            notes=row.fields.get("Notas Adicionais", ""),
        )
        prof["row"] = row.row_number
        information_profiles.append(prof)

    return EvaluationResult(
        verdict=verdict,
        qa_rows=qa_rows,
        cloze_rows=cloze_rows,
        checks=checks,
        errors=errors,
        warnings=warnings,
        skipped=skipped,
        batch_count=bc,
        information_profiles=information_profiles,
        receipt=receipt,
    )
