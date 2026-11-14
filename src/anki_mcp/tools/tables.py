"""tools/tables.py — FastMCP tools for table creation, markdown conversion, and validation.

Tools:
  table_build          — validate TableSpec JSON, render clean HTML, and run table checks
  table_from_markdown  — detect and convert markdown tables to HTML in card field text
  validate_tables_field— check table structure, mobile safety, and theme conformance
"""

from __future__ import annotations

import json
from typing import Any

from fastmcp import FastMCP

from anki_mcp.checks.tables import check_tables_field
from anki_mcp.core.safe import safe_tool
from anki_mcp.core.tables import TableSpec, markdown_tables_to_html
from anki_mcp.models import Diagnostic, Severity


def register_tables_tools(mcp: FastMCP) -> None:
    """Register all table-related MCP tools."""

    @mcp.tool(
        description=(
            "Validate a TableSpec JSON string, render it to single-line HTML without inline styles, "
            "and run table checks. Returns {status: 'PASS'|'FAIL', html: str, diagnostics: list}."
        )
    )
    @safe_tool
    def table_build(spec_json: str) -> dict[str, Any]:
        """Validate TableSpec, render HTML, and run table checks."""
        try:
            if not spec_json or not spec_json.strip():
                raise ValueError("spec_json must not be empty")
            data = json.loads(spec_json)
            if not isinstance(data, dict):
                raise ValueError("spec_json must be a JSON object")
            spec = TableSpec.model_validate(data)
            html = spec.render_html()
            diags = check_tables_field(html, field="Notas Adicionais")
        except Exception as exc:
            code = "TBL.RAGGED" if "does not match headers length" in str(exc) else "TBL.MALFORMED"
            diag = Diagnostic.from_code(
                code,
                f"Table specification error: {exc}",
                field="Notas Adicionais",
                suggestion="Provide valid TableSpec JSON with non-empty headers and matching row lengths",
            )
            return {
                "status": "FAIL",
                "html": "",
                "diagnostics": [diag.model_dump(mode="json")],
            }

        has_critical = any(d.severity == Severity.CRITICAL for d in diags)
        status = "FAIL" if has_critical else "PASS"
        return {
            "status": status,
            "html": html,
            "diagnostics": [d.model_dump(mode="json") for d in diags],
        }

    @mcp.tool(
        description=(
            "Convert GitHub-style markdown tables in text (with raw or escaped pipes, separated by "
            "newlines or <br>) into HTML tables. Returns {status: 'PASS'|'FAIL', text: str, "
            "tables_converted: int, diagnostics: list}."
        )
    )
    @safe_tool
    def table_from_markdown(text: str) -> dict[str, Any]:
        """Convert markdown tables in text to HTML and validate the result."""
        converted_text, count = markdown_tables_to_html(text)
        diags = check_tables_field(converted_text, field="Notas Adicionais")
        has_critical = any(d.severity == Severity.CRITICAL for d in diags)
        status = "FAIL" if has_critical else "PASS"
        return {
            "status": status,
            "text": converted_text,
            "tables_converted": count,
            "diagnostics": [d.model_dump(mode="json") for d in diags],
        }

    @mcp.tool(
        description=(
            "Validate HTML table syntax, mobile screen constraints, and check for markdown table leaks. "
            "Returns {status: 'PASS'|'FAIL', diagnostics: list}."
        )
    )
    @safe_tool
    def validate_tables_field(
        html: str,
        field: str = "Notas Adicionais",
        row: int = 0,
        face: bool = False,
    ) -> dict[str, Any]:
        """Validate table syntax and mobile limits in an Anki card field."""
        diags = check_tables_field(html, field=field, row=row or None, face=face)
        has_critical = any(d.severity == Severity.CRITICAL for d in diags)
        status = "FAIL" if has_critical else "PASS"
        return {
            "status": status,
            "diagnostics": [d.model_dump(mode="json") for d in diags],
        }
