"""core/tables.py — Table specifications, escaping, and markdown-to-HTML conversion.

Provides deterministic rendering of structured tables into AnkiMBR-compliant HTML,
safe cell escaping that prevents delimiter collisions with Anki's pipe-delimited CSV,
and extraction of GitHub-style markdown tables into single-line HTML.
"""

from __future__ import annotations

import re
from typing import Self

from pydantic import BaseModel, model_validator

# Allowed inline HTML tags inside table cells and captions
_ALLOWED_TAGS = {
    "b",
    "/b",
    "i",
    "/i",
    "em",
    "/em",
    "strong",
    "/strong",
    "sub",
    "/sub",
    "sup",
    "/sup",
    "br",
    "br/",
    "br /",
}

_REOPEN_RE = re.compile(r"&lt;(/?[a-zA-Z]+(?:\s*/?)?)&gt;")
_SEP_CELL_RE = re.compile(r"^:?-+:?$")
_LINE_BREAK_RE = re.compile(r"(?i)<br\s*/?>\s*\n?|\r?\n")


def escape_cell(text: str) -> str:
    """Apply deterministic escaping to table cells or captions.

    Escapes bare '&', '<', and '>', while:
      - Preserving valid existing entities (&amp;, &lt;, &gt;, &le;, &ge;, &#124;, &nbsp;).
      - Reopening only allowed tags: <b>, </b>, <i>, </i>, <em>, </em>,
        <strong>, </strong>, <sub>, </sub>, <sup>, </sup>, <br>.
      - Converting raw delimiter pipes '|' into '&#124;'.
    """
    if not text:
        return ""
    # 1. Escape bare '&' without double-escaping valid known entities
    s = re.sub(r"&(?!(?:amp|lt|gt|le|ge|#124|nbsp);)", "&amp;", text)
    # 2. Escape '<' and '>'
    s = s.replace("<", "&lt;").replace(">", "&gt;")

    # 3. Reopen allowed tags
    def _reopen(m: re.Match[str]) -> str:
        inner = m.group(1).strip().lower()
        if inner in {"br/", "br /"}:
            return "<br>"
        if inner in _ALLOWED_TAGS:
            return f"<{inner}>"
        return m.group(0)

    s = _REOPEN_RE.sub(_reopen, s)
    # 4. Convert raw pipes '|' to '&#124;'
    s = s.replace("|", "&#124;")
    return s


class TableSpec(BaseModel):
    """Specification for a structured table.

    Validates that headers are non-empty and every data row matches
    the headers column count. Renders into a clean single-line HTML table
    without inline styles, suitable for AnkiMBR CSS styling.
    """

    caption: str = ""
    headers: list[str]
    rows: list[list[str]]

    @model_validator(mode="after")
    def _validate_table_structure(self) -> Self:
        if not self.headers:
            raise ValueError("headers must not be empty")
        expected_len = len(self.headers)
        for idx, row in enumerate(self.rows):
            if len(row) != expected_len:
                raise ValueError(
                    f"Row {idx} length {len(row)} does not match headers length {expected_len}: {row}"
                )
        return self

    def render_html(self) -> str:
        """Render table as a single-line HTML string without inline styles."""
        parts: list[str] = ["<table>"]
        if self.caption and self.caption.strip():
            parts.append(f"<caption>{escape_cell(self.caption.strip())}</caption>")
        parts.append("<thead><tr>")
        for h in self.headers:
            parts.append(f"<th>{escape_cell(h)}</th>")
        parts.append("</tr></thead><tbody>")
        for r in self.rows:
            parts.append("<tr>")
            for cell in r:
                parts.append(f"<td>{escape_cell(cell)}</td>")
            parts.append("</tr>")
        parts.append("</tbody></table>")
        return "".join(parts)


def _split_markdown_cells(line: str) -> list[str]:
    s = line.strip()
    if "|" in s:
        delim = "|"
    elif "&#124;" in s:
        delim = "&#124;"
    else:
        return [s]
    if s.startswith(delim):
        s = s[len(delim) :]
    if s.endswith(delim):
        s = s[: -len(delim)]
    return [c.strip() for c in s.split(delim)]


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


def _is_table_row(line: str) -> bool:
    s = line.strip()
    if not s:
        return False
    if "|" not in s and "&#124;" not in s:
        return False
    if _is_separator_row(s):
        return False
    # Avoid picking up starting HTML block elements
    lower = s.lower()
    if lower.startswith("<table") or lower.startswith("<div") or lower.startswith("<p"):
        return False
    return True


def _normalize_cells(cells: list[str], expected: int) -> list[str]:
    """Pad rows with fewer cells, or join excess cells into the last cell."""
    if len(cells) < expected:
        return cells + [""] * (expected - len(cells))
    elif len(cells) > expected:
        base = cells[: expected - 1]
        surplus = " ".join(cells[expected - 1 :])
        return [*base, surplus]
    return cells


def markdown_tables_to_html(text: str) -> tuple[str, int]:
    """Find GitHub-style markdown tables in text and convert them to HTML via TableSpec.

    Supports lines separated by '\n' or '<br>' (including <br/>). Accepts raw pipes '|'
    or escaped '&#124;'.
    If a row has fewer cells than headers, it is padded with empty strings ('').
    If a row has more cells than headers, the surplus cells are joined into the last cell
    using a space (' ').

    Returns:
        tuple[str, int]: (converted_text, number_of_tables_converted).
        If no tables were found, returns the exact identical input text and 0.
    """
    if not text:
        return text, 0

    segments: list[tuple[str, str]] = []
    last = 0
    for m in _LINE_BREAK_RE.finditer(text):
        segments.append((text[last : m.start()], m.group(0)))
        last = m.end()
    segments.append((text[last:], ""))

    out_parts: list[str] = []
    tables_count = 0
    i = 0
    n = len(segments)

    while i < n:
        line_content, line_sep = segments[i]
        # Check whether a markdown table starts at line i
        if (
            i + 1 < n
            and ("|" in line_content or "&#124;" in line_content)
            and not _is_separator_row(line_content)
            and _is_separator_row(segments[i + 1][0])
        ):
            headers = _split_markdown_cells(line_content)
            expected_cols = len(headers)
            rows: list[list[str]] = []
            j = i + 2
            while j < n and _is_table_row(segments[j][0]):
                row_cells = _split_markdown_cells(segments[j][0])
                row_cells = _normalize_cells(row_cells, expected_cols)
                rows.append(row_cells)
                j += 1
            spec = TableSpec(headers=headers, rows=rows)
            table_html = spec.render_html()
            # Preserve the delimiter trailing the last row of the converted table block
            final_sep = segments[j - 1][1]
            out_parts.append(table_html + final_sep)
            tables_count += 1
            i = j
        else:
            out_parts.append(line_content + line_sep)
            i += 1

    if tables_count == 0:
        return text, 0
    return "".join(out_parts), tables_count
