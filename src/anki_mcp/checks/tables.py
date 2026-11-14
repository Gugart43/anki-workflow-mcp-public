"""checks/tables.py — Table validation rules for Anki cards.

Checks table structure, formatting, mobile safety, and theme consistency:
  TBL.MALFORMED     — Unbalanced or malformed <table> HTML (CRITICAL)
  TBL.RAGGED        — Rows have inconsistent column counts (CRITICAL)
  TBL.NESTED        — Nested <table> is not allowed (CRITICAL)
  TBL.MARKDOWN_PIPE — Markdown table or raw pipes inside field (CRITICAL)
  TBL.NO_HEADER     — Table without a header row (WARN)
  TBL.TOO_WIDE      — Too many columns for mobile (>4) (WARN)
  TBL.TOO_LONG      — Too many body rows for one card (>8) (WARN)
  TBL.CELL_TOO_LONG — Table cell text too long (>120 chars) (WARN)
  TBL.INLINE_STYLE  — Inline style/width/bgcolor overrides theme (WARN)
  TBL.IN_CARD_FACE  — Table located on card face (WARN)
"""

from __future__ import annotations

import re
from html.parser import HTMLParser

from anki_mcp.models import Diagnostic

MAX_COLS = 4
MAX_BODY_ROWS = 8
MAX_CELL_CHARS = 120

_TABLE_TAGS = {"table", "thead", "tbody", "tfoot", "tr", "th", "td", "caption"}
_FORBIDDEN_ATTRS = {"style", "width", "height", "bgcolor", "align", "border", "cellpadding"}

_SEP_CELL_RE = re.compile(r"^:?-+:?$")
_LINE_BREAK_RE = re.compile(r"(?i)<br\s*/?>\s*\n?|\r?\n")


def _is_separator_row(line: str) -> bool:
    s = line.strip()
    if not s:
        return False
    if "|" in s:
        delim = "|"
    elif "&#124;" in s:
        delim = "&#124;"
    else:
        return False
    if s.startswith(delim):
        s = s[len(delim) :]
    if s.endswith(delim):
        s = s[: -len(delim)]
    s = s.strip()
    if not s:
        return False
    cells = [c.strip() for c in s.split(delim)]
    return len(cells) > 0 and all(_SEP_CELL_RE.match(c) for c in cells)


class _TableHTMLParser(HTMLParser):
    def __init__(self, *, field: str, row: int | None, face: bool) -> None:
        super().__init__()
        self.field = field
        self.row = row
        self.face = face
        self.diags: list[Diagnostic] = []
        self.tag_stack: list[str] = []
        self.table_depth = 0

        # Stats for the current table
        self.has_table = False
        self.current_has_thead = False
        self.row_cols: list[int] = []
        self.row_is_body: list[bool] = []
        self.first_row_th = False
        self.current_row_cols = 0
        self.current_row_has_th = False
        self.current_row_in_thead = False
        self.current_cell_chars = 0
        self.in_cell = False

    def _emit(self, code: str, message: str, suggestion: str | None = None) -> None:
        self.diags.append(
            Diagnostic.from_code(
                code,
                message,
                field=self.field,
                row=self.row,
                suggestion=suggestion,
            )
        )

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        tag_lower = tag.lower()
        if tag_lower not in _TABLE_TAGS:
            return

        for attr, _ in attrs:
            if attr.lower() in _FORBIDDEN_ATTRS:
                self._emit(
                    "TBL.INLINE_STYLE",
                    f"Inline presentation attribute '{attr}' on <{tag_lower}> overrides note type theme",
                    suggestion="Remove inline style/width/bgcolor attributes and rely on the AnkiMBR CSS",
                )

        if tag_lower == "table":
            self.has_table = True
            if self.face:
                self._emit(
                    "TBL.IN_CARD_FACE",
                    "Table in Enunciado/Resposta/Sentença (move to Notas Adicionais or split the card)",
                    suggestion="Move table to Notas Adicionais or split the card",
                )
            if self.table_depth > 0:
                self._emit(
                    "TBL.NESTED",
                    "Nested <table> is not allowed",
                    suggestion="Flatten nested tables into separate tables or linear text",
                )
            self.table_depth += 1
            self.current_has_thead = False
            self.row_cols = []
            self.row_is_body = []
            self.first_row_th = False
            self.tag_stack.append(tag_lower)
        elif tag_lower in {"thead", "tbody", "tfoot", "caption"}:
            if not self.tag_stack or self.tag_stack[-1] != "table":
                self._emit(
                    "TBL.MALFORMED",
                    f"<{tag_lower}> must be directly inside <table>",
                    suggestion="Check table structure and nesting",
                )
            if tag_lower == "thead":
                self.current_has_thead = True
            self.tag_stack.append(tag_lower)
        elif tag_lower == "tr":
            if not self.tag_stack or self.tag_stack[-1] not in {
                "table",
                "thead",
                "tbody",
                "tfoot",
            }:
                self._emit(
                    "TBL.MALFORMED",
                    "<tr> must be inside <table>, <thead>, <tbody>, or <tfoot>",
                    suggestion="Check table row placement",
                )
            self.current_row_cols = 0
            self.current_row_has_th = False
            self.current_row_in_thead = "thead" in self.tag_stack
            self.tag_stack.append(tag_lower)
        elif tag_lower in {"th", "td"}:
            if not self.tag_stack or self.tag_stack[-1] != "tr":
                self._emit(
                    "TBL.MALFORMED",
                    f"<{tag_lower}> must be directly inside <tr>",
                    suggestion="Check table cell placement",
                )
            self.in_cell = True
            self.current_cell_chars = 0
            if tag_lower == "th":
                self.current_row_has_th = True

            colspan = 1
            for attr, val in attrs:
                if attr.lower() == "colspan" and val:
                    try:
                        cs = int(val)
                        if cs > 0:
                            colspan = cs
                    except (ValueError, TypeError):
                        pass
            self.current_row_cols += colspan
            self.tag_stack.append(tag_lower)

    def handle_data(self, data: str) -> None:
        if self.in_cell:
            self.current_cell_chars += len(data)

    def handle_endtag(self, tag: str) -> None:
        tag_lower = tag.lower()
        if tag_lower not in _TABLE_TAGS:
            return

        if not self.tag_stack or self.tag_stack[-1] != tag_lower:
            self._emit(
                "TBL.MALFORMED",
                f"Unbalanced or unexpected closing tag </{tag_lower}>",
                suggestion="Ensure all table tags are matched and closed in proper order",
            )
            if tag_lower in self.tag_stack:
                while self.tag_stack and self.tag_stack[-1] != tag_lower:
                    self.tag_stack.pop()
                if self.tag_stack:
                    self.tag_stack.pop()
            return

        self.tag_stack.pop()

        if tag_lower in {"th", "td"}:
            self.in_cell = False
            if self.current_cell_chars > MAX_CELL_CHARS:
                self._emit(
                    "TBL.CELL_TOO_LONG",
                    f"Table cell text too long ({self.current_cell_chars} chars, max {MAX_CELL_CHARS})",
                    suggestion=f"Shorten table cell content to <= {MAX_CELL_CHARS} visible characters",
                )
            self.current_cell_chars = 0
        elif tag_lower == "tr":
            row_idx = len(self.row_cols)
            if row_idx == 0:
                self.first_row_th = self.current_row_has_th
            self.row_cols.append(self.current_row_cols)
            is_body = not self.current_row_in_thead
            if not self.current_has_thead and row_idx == 0 and self.first_row_th:
                is_body = False
            self.row_is_body.append(is_body)
        elif tag_lower == "table":
            self.table_depth = max(0, self.table_depth - 1)
            # Evaluate table-level properties
            if len(self.row_cols) > 1 and len(set(self.row_cols)) > 1:
                self._emit(
                    "TBL.RAGGED",
                    f"Rows have inconsistent column counts: {self.row_cols}",
                    suggestion="Ensure all rows have the same number of columns (matching the header)",
                )
            if not self.current_has_thead and not self.first_row_th:
                self._emit(
                    "TBL.NO_HEADER",
                    "Table without a header row (<thead>/<th>)",
                    suggestion="Add a <thead><tr><th>...</th></tr></thead> row to provide context for columns",
                )
            max_cols = max(self.row_cols) if self.row_cols else 0
            if max_cols > MAX_COLS:
                self._emit(
                    "TBL.TOO_WIDE",
                    f"Table has {max_cols} columns (maximum recommended: {MAX_COLS})",
                    suggestion="Dividir a tabela ou transpor",
                )
            body_rows = sum(1 for b in self.row_is_body if b)
            if body_rows > MAX_BODY_ROWS:
                self._emit(
                    "TBL.TOO_LONG",
                    f"Table has {body_rows} body rows (maximum recommended: {MAX_BODY_ROWS})",
                    suggestion=f"Keep table body to <= {MAX_BODY_ROWS} rows to preserve card atomicity",
                )

    def finalize(self) -> None:
        if self.tag_stack:
            self._emit(
                "TBL.MALFORMED",
                f"Unclosed table tags at end of field: {self.tag_stack}",
                suggestion="Ensure all table tags (table, tr, td, th) are properly closed",
            )


def check_tables_field(
    html: str,
    *,
    field: str = "Notas Adicionais",
    row: int | None = None,
    face: bool = False,
) -> list[Diagnostic]:
    """Validate HTML table structure and detect markdown tables in a card field.

    Empty fields return an empty list [].
    """
    if not html or not html.strip():
        return []

    diags: list[Diagnostic] = []

    # 1. TBL.MARKDOWN_PIPE detection
    lines = _LINE_BREAK_RE.split(html)
    pipe_lines_count = sum(1 for line in lines if line.count("|") + line.count("&#124;") >= 3)
    has_sep = any(_is_separator_row(line) for line in lines)
    if has_sep or pipe_lines_count >= 2:
        diags.append(
            Diagnostic.from_code(
                "TBL.MARKDOWN_PIPE",
                "Markdown table or raw pipe syntax detected in field (collides with CSV delimiter)",
                field=field,
                row=row,
                suggestion="Convert markdown tables to HTML via table_from_markdown or table_build",
            )
        )

    # 2. HTMLParser for table HTML tags
    parser = _TableHTMLParser(field=field, row=row, face=face)
    try:
        parser.feed(html)
        parser.close()
        parser.finalize()
    except Exception as exc:
        diags.append(
            Diagnostic.from_code(
                "TBL.MALFORMED",
                f"Malformed HTML in table structure: {exc}",
                field=field,
                row=row,
                suggestion="Fix HTML syntax in table markup",
            )
        )
        return diags

    diags.extend(parser.diags)
    return diags
