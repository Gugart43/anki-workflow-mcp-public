"""tools/validate.py — FastMCP tool registrations for validation.

Tools (read-only, no state):
  validate_csv_structure  — structural CSV checks (pipe, newlines, entities, MathJax…)
  validate_cloze          — cloze syntax (NONSEQ, NESTED, boundary)
  validate_hints          — hint quality (synonym, elimination, root_leak)
  validate_qa             — QA card quality (cardinality, yesno, mc_embed, context…)
  validate_notes          — notes field quality (metadata, dup_answer, length)
  validate_tags_field     — tag format and taxonomy compliance
  validate_crossrefs      — cross-reference ID resolution
  validate_images_field   — image HTML structure (figure, src, namespace, caps)
  validate_batch          — full batch validation (all checks + BatchCount)

Every tool is wrapped by safe_tool: failures come back as {status:'ERROR', code, message, action}.
"""

from __future__ import annotations

from typing import Any, Literal

from fastmcp import FastMCP

from anki_mcp.checks import (
    cloze as cloze_mod,
)
from anki_mcp.checks import (
    crossrefs as xrefs_mod,
)
from anki_mcp.checks import (
    csv_structure,
)
from anki_mcp.checks import (
    hints as hints_mod,
)
from anki_mcp.checks import (
    images as images_mod,
)
from anki_mcp.checks import (
    notes as notes_mod,
)
from anki_mcp.checks import (
    qa as qa_mod,
)
from anki_mcp.checks import (
    tags as tags_mod,
)
from anki_mcp.checks.information import compact_information_profiles, profile_information
from anki_mcp.core.evaluate import evaluate_batch
from anki_mcp.core.receipt import compute_receipt

__all__ = [
    "compute_receipt",
    "evaluate_batch",
    "register_validate_tools",
]
from anki_mcp.core.safe import safe_tool
from anki_mcp.models import (
    CheckStatus,
    Diagnostic,
    Severity,
    ValidationReport,
)


def _build_check(name: str, diags: list[Diagnostic]) -> CheckStatus:
    criticals = [d for d in diags if d.severity == Severity.CRITICAL]
    warnings = [d for d in diags if d.severity == Severity.WARN]
    result: Literal["FAIL", "WARN", "PASS"] = (
        "FAIL" if criticals else ("WARN" if warnings else "PASS")
    )
    return CheckStatus(name=name, result=result, diagnostics=diags)


def _images_catalog(catalog_filenames: str) -> tuple[set[str] | None, str | None]:
    """(catalog, None) when provided; (None, reason) when the phantom check cannot run."""
    catalog = {f.strip() for f in catalog_filenames.split(",") if f.strip()}
    if catalog:
        return catalog, None
    return None, "catalog_filenames not provided: IMG.PHANTOM cannot be verified"


def register_validate_tools(mcp: FastMCP) -> None:
    @mcp.tool(
        description=(
            "Validate the structural integrity of a QA or Cloze CSV block. "
            "Checks: header match, column count, raw pipes, raw newlines, HTML entities, "
            "forbidden tags, MathJax balance, and UTF-8 BOM. "
            "note_type must be 'QA' (7 fields) or 'CL' (6 fields)."
        )
    )
    @safe_tool
    def validate_csv_structure(
        csv_text: str,
        note_type: Literal["QA", "CL"],
    ) -> dict[str, Any]:
        """Structural CSV validation — formatting-guide checks."""
        raw_bytes = csv_text.encode("utf-8", errors="replace")
        diags: list[Diagnostic] = []
        diags.extend(csv_structure.check_bom(raw_bytes))
        diags.extend(csv_structure.check_csv(csv_text, note_type))
        check = _build_check("csv_structure", diags)
        return check.model_dump(mode="json")

    @mcp.tool(
        description=(
            "Validate cloze syntax in a 'Sentença' field value. "
            "Checks: sequential c1..cN indices (operational cap 3; each index creates a separate Anki card "
            "with other indices visible), no nested deletions, boundary/length advisory WARN signals "
            "(length alone never proves atomicity and a single gap can hide a list). "
            "Returns: status, detected indices, card_count (= distinct cN), diagnostics, and "
            "information_profile (per-index 3-layer breakdown, candidate_items, 6-question checklist; "
            "read-only, no quality score)."
        )
    )
    @safe_tool
    def validate_cloze(sentence: str, row: int = 0) -> dict[str, Any]:
        """Cloze syntax check — anki-cloze-creator rules."""
        diags = cloze_mod.check_cloze(sentence, row=row or None)
        indices = sorted(cloze_mod.count_cloze_indices(sentence))
        check = _build_check("cloze", diags)
        info_prof = profile_information(note_type="CL", prompt=sentence)
        return {
            **check.model_dump(mode="json"),
            "indices": indices,
            "card_count": len(indices),
            "information_profile": info_prof,
        }

    @mcp.tool(
        description=(
            "Validate hints in a cloze sentence '{{cN::target::hint}}'. "
            "Checks: HINT.SYNONYM (high overlap), HINT.ELIMINATION (binary alternatives), "
            "HINT.ROOT_LEAK (target root in hint), HINT.FORMAT (empty hint)."
        )
    )
    @safe_tool
    def validate_hints(sentence: str, row: int = 0) -> dict[str, Any]:
        """Hint quality check — anki-cloze-hints rules."""
        diags = hints_mod.check_hints(sentence, row=row or None)
        check = _build_check("hints", diags)
        return check.model_dump(mode="json")

    @mcp.tool(
        description=(
            "Validate a QA pair (stem + answer), separating retrieved recall target from necessary stem context "
            "(stem context length is not extra recall; length-only checks emit WARN, never CRITICAL). "
            "Checks: QA.CARDINALITY (≥3 items → review unit, split independent recalls or move optional details "
            "to notes; no automatic QA->Cloze conversion), QA.YESNO, QA.MC_EMBED, QA.CONTEXT_WARN, QA.NUMBER_UNIT. "
            "Returns information_profile with candidate_items, evidence, and 6-question editorial checklist."
        )
    )
    @safe_tool
    def validate_qa(enunciado: str, resposta: str, row: int = 0) -> dict[str, Any]:
        """QA quality check — anki-qa-designer rules."""
        diags = qa_mod.check_qa(enunciado, resposta, row=row or None)
        check = _build_check("qa", diags)
        info_prof = profile_information(note_type="QA", prompt=enunciado, answer=resposta)
        return {**check.model_dump(mode="json"), "information_profile": info_prof}

    @mcp.tool(
        description=(
            "Validate the 'Notas Adicionais' optional explanation field (same unit, authorized source; "
            "empty/short notes allowed with no minimum volume and variable depth). "
            "Checks: NOTE.METADATA (CRITICAL), NOTE.DUP_ANSWER_WARN (normalized HTML/entity/punctuation "
            "exact repeat), NOTE.EXAM_ONLY_WARN (exam mention without explanation), NOTE.LENGTH_WARN, NOTE.STYLE_WARN. "
            "Returns information_profile (note_type='QA', prompt_available=False limitation noted)."
        )
    )
    @safe_tool
    def validate_notes(
        notes: str,
        answer: str = "",
        max_length: int = 1200,
        row: int = 0,
    ) -> dict[str, Any]:
        """Notes field quality — anki-notes-enricher rules."""
        diags = notes_mod.check_notes(notes, answer, row=row or None, max_length=max_length)
        check = _build_check("notes", diags)
        info_prof = profile_information(
            note_type="QA",
            prompt="",
            answer=answer,
            notes=notes,
            prompt_available=False,
        )
        return {**check.model_dump(mode="json"), "information_profile": info_prof}

    @mcp.tool(
        description=(
            "Validate the 'Etiquetas' field. "
            "Checks: TAG.FORMAT (spaces/invalid chars), TAG.DEPTH (>4 levels), "
            "TAG.UNREGISTERED (against provided taxonomy). "
            "taxonomy_tags: space-separated string of all registered tags (or empty to skip taxonomy check)."
        )
    )
    @safe_tool
    def validate_tags_field(
        tags_field: str,
        taxonomy_tags: str = "",
        row: int = 0,
    ) -> dict[str, Any]:
        """Tag validation — anki-registry rules."""
        taxonomy_nodes = set(taxonomy_tags.split()) if taxonomy_tags.strip() else None
        diags = tags_mod.check_tags(tags_field, taxonomy_nodes, row=row or None)
        check = _build_check("tags", diags)
        return check.model_dump(mode="json")

    @mcp.tool(
        description=(
            "Check cross-references in any field text. "
            "active_ids and current_batch_ids are comma-separated ID strings. "
            "XREF.BROKEN is emitted for IDs that do not resolve."
        )
    )
    @safe_tool
    def validate_crossrefs(
        text: str,
        active_ids: str = "",
        current_batch_ids: str = "",
        row: int = 0,
        field: str = "",
    ) -> dict[str, Any]:
        """Cross-reference resolution — anki-export-validator check #10."""
        active = set(active_ids.split(",")) if active_ids.strip() else None
        current = set(current_batch_ids.split(",")) if current_batch_ids.strip() else None
        diags = xrefs_mod.check_crossrefs(
            text, active, current, row=row or None, field=field or None
        )
        check = _build_check("crossrefs", diags)
        return check.model_dump(mode="json")

    @mcp.tool(
        description=(
            "Validate the 'Imagens Relacionadas' HTML field. "
            "Checks: IMG.CAP_OVER (>3), IMG.PHANTOM (src not in catalog), "
            "IMG.NAMESPACE (invalid slug prefix), IMG.DUP_IN_CARD, "
            "IMG.STRUCTURE (<figure> malformed), IMG.CAPTION (empty/noisy). "
            "catalog_filenames: comma-separated list of valid namespaced filenames. "
            "Without it, a field containing <img> returns status INCOMPLETE (phantom unverifiable)."
        )
    )
    @safe_tool
    def validate_images_field(
        html: str,
        catalog_filenames: str = "",
        row: int = 0,
    ) -> dict[str, Any]:
        """Image HTML validation — anki-image-injector rules."""
        catalog, reason = _images_catalog(catalog_filenames)
        if catalog is None and "<img" in html.lower():
            check = CheckStatus(name="images", result="SKIP", reason=reason)
            return {**check.model_dump(mode="json"), "status": "INCOMPLETE"}
        diags = images_mod.check_images_field(html, catalog, row=row or None)
        check = _build_check("images", diags)
        return check.model_dump(mode="json")

    @mcp.tool(
        description=(
            "Full batch validation: runs every export-validator check over a complete QA CSV + "
            "Cloze CSV pair. Returns a ValidationReport with a verdict: PASS (content_hash = "
            "receipt), INCOMPLETE (some check could not run; see skipped; no receipt) or FAIL, "
            "plus read-only information_profiles (3-layer profile per row with note_type and row; "
            "excluded from receipt hash; length-only signals are advisory WARN). "
            "taxonomy_tags: space-separated registered tags. "
            "active_ids / current_batch_ids: comma-separated. "
            "catalog_filenames: comma-separated namespaced image filenames (needed to verify "
            "<img> sources; without it, image-bearing rows are reported as skipped)."
        )
    )
    @safe_tool
    def validate_batch(
        batch_id: str,
        qa_csv: str,
        cloze_csv: str,
        taxonomy_tags: str = "",
        active_ids: str = "",
        current_batch_ids: str = "",
        catalog_filenames: str = "",
        max_notes_length: int = 1200,
    ) -> dict[str, Any]:
        """Full batch validation — all export-validator checks."""
        taxonomy_nodes = set(taxonomy_tags.split()) if taxonomy_tags.strip() else None
        active_id_set = set(active_ids.split(",")) if active_ids.strip() else None
        current_id_set = set(current_batch_ids.split(",")) if current_batch_ids.strip() else None
        catalog_set, _catalog_reason = _images_catalog(catalog_filenames)

        ev = evaluate_batch(
            qa_csv,
            cloze_csv,
            deck_dir=None,
            taxonomy_tags=taxonomy_nodes,
            active_ids=active_id_set,
            current_batch_ids=current_id_set,
            catalog_filenames=catalog_set,
            batch_id=batch_id,
        )

        checks: list[CheckStatus] = []
        for c in ev.checks:
            diags = [Diagnostic.model_validate(d) for d in c.get("diagnostics", [])]
            checks.append(
                CheckStatus(
                    name=c["name"],
                    result=c["result"],
                    diagnostics=diags,
                    reason=c.get("reason"),
                )
            )

        report = ValidationReport(
            result=ev.verdict,
            batch_id=batch_id,
            checks=checks,
            errors=ev.errors,
            warnings=ev.warnings,
            content_hash=ev.receipt,
        )
        compact_profiles, guidance = compact_information_profiles(ev.information_profiles)
        return {
            **report.model_dump(mode="json"),
            "checks_skipped": len(ev.skipped),
            "skipped": ev.skipped,
            "batch_count": ev.batch_count.model_dump(mode="json"),
            "information_profiles": compact_profiles,
            "information_guidance": guidance,
        }
