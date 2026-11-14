"""tools/mechanics.py — utility tools (no state, no network).

Tools:
  csv_build_row       — build a guaranteed-safe pipe-delimited CSV row
  sanitize_card_field — sanitize a single card field (inequalities, entities, HTML safety)
  batch_auto_fix      — automatically repair and sanitize an entire draft CSV block
  count_batch_cards   — count QA + Cloze expansion against the ≤20 limit
"""

from __future__ import annotations

from typing import Literal

from fastmcp import FastMCP

from anki_mcp.checks.batch import count_batch
from anki_mcp.core.sanitize import (
    auto_fix_csv_block,
    sanitize_field,
)


def _escape_field(value: str) -> str:
    """Apply all formatting-guide escaping rules to a single field value.

    Guarantees:
      - Raw newlines -> <br>
      - Pipes -> &#124;
      - Clinical inequalities (< 10, > 50, <=, >=) -> &lt;, &gt;, &le;, &ge;
      - Bare & -> &amp; without double escaping
      - Preserves legitimate HTML tags (<b>, <i>, etc.)
    """
    clean, _ = sanitize_field(value, convert_markdown=False)
    return clean


def register_mechanics_tools(mcp: FastMCP) -> None:

    @mcp.tool(
        description=(
            "Build a single, guaranteed-safe pipe-delimited CSV row from a list of field values. "
            "Applies all formatting-guide rules: newlines→<br>, pipe→&#124;, inequalities→&lt;/&gt;, "
            "bare &→&amp; without double-escaping, while preserving valid HTML tags. "
            "fields must match the note type: 7 values for QA, 6 for Cloze. "
            "Returns the safe CSV line string."
        )
    )
    def csv_build_row(fields: list[str]) -> dict:
        """Produce a safe CSV row — formatting-guide compliant."""
        safe_fields = [_escape_field(f) for f in fields]
        return {
            "csv_line": "|".join(safe_fields),
            "field_count": len(safe_fields),
        }

    @mcp.tool(
        description=(
            "Sanitize a single card field: converts mathematical/clinical inequalities (< 10, > 50, <=, >=) "
            "to HTML entities (&lt;, &gt;, &le;, &ge;), bare & to &amp;, pipes to &#124;, newlines to <br>, "
            "while strictly preserving valid HTML tags (<b>, <i>, <code>, <span>, <ul>, <li>, <figure>, <img>). "
            "Returns sanitized string and list of fixes applied."
        )
    )
    def sanitize_card_field(
        text: str,
        is_images_field: bool = False,
        convert_markdown: bool = True,
    ) -> dict:
        """Sanitize a single field for guaranteed Anki CSV/HTML compliance."""
        clean, fixes = sanitize_field(
            text,
            is_images_field=is_images_field,
            convert_markdown=convert_markdown,
        )
        return {"sanitized": clean, "fixes": fixes}

    @mcp.tool(
        description=(
            "Automatically repair and sanitize an entire draft CSV block (QA or Cloze). "
            "Enforces canonical header, repairs unescaped pipes and newlines in long free-text fields, "
            "escapes clinical inequalities (< 10, > 50) and bare '&', converts basic markdown, "
            "and auto-closes simple unclosed tags. "
            "Returns fixed_csv, fixes_applied, and residual diagnostics."
        )
    )
    def batch_auto_fix(
        csv_text: str,
        note_type: Literal["QA", "CLOZE"],
    ) -> dict:
        """Auto-repair a full CSV block and check resulting structural health."""
        from anki_mcp.checks.csv_structure import check_csv

        fixed_csv, fixes = auto_fix_csv_block(csv_text, note_type)
        ntype_code = "QA" if note_type == "QA" else "CL"
        diags = check_csv(fixed_csv, ntype_code)
        criticals = [d.model_dump(mode="json") for d in diags if d.severity.value == "CRITICAL"]
        warnings = [d.model_dump(mode="json") for d in diags if d.severity.value == "WARN"]
        return {
            "status": "PASS" if not criticals else "NEEDS_REVIEW",
            "fixed_csv": fixed_csv,
            "fixes_applied": fixes,
            "critical_count": len(criticals),
            "warning_count": len(warnings),
            "diagnostics": criticals + warnings,
        }

    @mcp.tool(
        description=(
            "Count the Anki card budget for a batch. "
            "QA notes = 1 card each. Cloze notes: each distinct cN index = 1 card. "
            "Returns qa_cards, cloze_notes, cloze_indices (expanded count), total, within_limit (≤20), "
            "excess, and ext_batch_suggested if over the limit."
        )
    )
    def count_batch_cards(
        qa_csv: str,
        cloze_csv: str,
        batch_id: str = "",
    ) -> dict:
        """Enforce the ≤20 card limit — anki-cloze-creator §5 + post-curator §5."""
        result = count_batch(qa_csv, cloze_csv, batch_id or None)
        return result.model_dump(mode="json")
