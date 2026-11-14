"""core/sanitize.py — Deterministic sanitization & auto-repair engine for Anki card fields.

Solves the critical fragility of long free-text fields in clinical flashcards:
  - Preserves legitimate HTML tags (<b>, <i>, <code>, <span>, <br>, <ul>, <li>, <figure>, <img>...)
  - Automatically escapes mathematical & clinical comparisons (< 10, > 50, <= 5, >= 3, p < 0.05)
    into &lt;, &gt;, &le;, &ge;
  - Automatically escapes bare '&' into '&amp;' without double-escaping existing entities
  - Normalizes raw physical newlines (\\r\\n, \\r, \\n) into <br>
  - Escapes raw delimiter pipes '|' into '&#124;'
  - Converts basic markdown formatting (**bold** -> <b>, *italic* -> <i>) to valid HTML
  - Automatically closes simple unclosed paired inline tags to avoid FMT.TAG_UNCLOSED
"""

from __future__ import annotations

import re
from typing import Literal

from anki_mcp.core.vocab import ALLOWED_TAGS, IMAGES_ONLY_TAGS

# Allowed tags set
_ALL_ALLOWED = ALLOWED_TAGS
_ALL_IMAGES = ALLOWED_TAGS | IMAGES_ONLY_TAGS

# HTML tag matcher: <(/?)tag_name(attrs)?>
_TAG_RE = re.compile(
    r"""<(/?)([a-zA-Z][a-zA-Z0-9]*)((\s+[^>]*?)?)(/?)>""",
    re.DOTALL,
)

# HTML comments
_COMMENT_RE = re.compile(r"""<!--.*?-->""", re.DOTALL)

# Valid entity matcher: &name; or &#123; or &#x1a;
_ENTITY_RE = re.compile(r"""&(#\d+|#x[0-9a-fA-F]+|[a-zA-Z]+);""")

# Paired tags that should be tracked for auto-closing if unclosed
_AUTO_CLOSE_TAGS = (
    "b",
    "strong",
    "i",
    "em",
    "u",
    "s",
    "small",
    "span",
    "code",
    "mark",
    "sub",
    "sup",
)


def sanitize_field(
    value: str,
    *,
    is_images_field: bool = False,
    convert_markdown: bool = True,
) -> tuple[str, list[str]]:
    """Sanitize a single card field value for guaranteed CSV and HTML safety.

    Returns (sanitized_string, list_of_fixes_applied).
    """
    if not value:
        return "", []

    fixes: list[str] = []

    # 1. Pipe escape — must be done so CSV parsing never splits fields
    if "|" in value:
        value = value.replace("|", "&#124;")
        fixes.append("escaped_pipe")

    # 2. Normalize raw physical newlines to <br>
    if "\r" in value or "\n" in value:
        value = value.replace("\r\n", "<br>").replace("\r", "<br>").replace("\n", "<br>")
        fixes.append("normalized_newlines_to_br")

    # 3. Optional Markdown bold and italic conversion
    if convert_markdown:
        # Convert **bold** or __bold__ to <b>...</b>
        def _bold_repl(m: re.Match[str]) -> str:
            fixes.append("converted_markdown_bold")
            return f"<b>{m.group(1)}</b>"

        value = re.sub(r"\*\*(.+?)\*\*", _bold_repl, value)
        value = re.sub(r"__(.+?)__", _bold_repl, value)

        # Convert *italic* or _italic_ (avoiding intra-word or already processed)
        def _italic_repl(m: re.Match[str]) -> str:
            fixes.append("converted_markdown_italic")
            return f"<i>{m.group(1)}</i>"

        value = re.sub(r"(?<!\*)\*([^\*\s][^\*]*?[^\*\s]|[^\*\s])\*(?!\*)", _italic_repl, value)

    # 4. Tokenize allowed HTML comments and tags vs plain text
    allowed = _ALL_IMAGES if is_images_field else _ALL_ALLOWED

    # Mask comments temporarily
    comments: list[str] = []

    def _mask_comment(m: re.Match[str]) -> str:
        comments.append(m.group(0))
        return f"__COMMENT_TOKEN_{len(comments) - 1}__"

    value = _COMMENT_RE.sub(_mask_comment, value)

    tokens: list[tuple[bool, str]] = []  # (is_tag, text)
    last_idx = 0

    for m in _TAG_RE.finditer(value):
        start, end = m.span()
        if start > last_idx:
            tokens.append((False, value[last_idx:start]))

        tag_name = m.group(2).lower()
        if tag_name in allowed:
            tokens.append((True, m.group(0)))
        else:
            # Tag not allowed -> treat as text containing brackets!
            tokens.append((False, m.group(0)))
        last_idx = end

    if last_idx < len(value):
        tokens.append((False, value[last_idx:]))

    # 5. Process non-tag tokens: escape inequalities and bare &
    out_tokens: list[str] = []
    tag_stack: list[str] = []

    for is_tag, text in tokens:
        if is_tag:
            out_tokens.append(text)
            # Track paired tags
            m_tag = _TAG_RE.match(text)
            if m_tag:
                is_close = bool(m_tag.group(1))
                tname = m_tag.group(2).lower()
                is_self_closing = bool(m_tag.group(5)) or tname in {"br", "hr", "img"}
                if not is_self_closing:
                    if is_close:
                        if tag_stack and tag_stack[-1] == tname:
                            tag_stack.pop()
                        elif tname in tag_stack:
                            # pop until matching
                            while tag_stack and tag_stack[-1] != tname:
                                tag_stack.pop()
                            if tag_stack:
                                tag_stack.pop()
                    else:
                        if tname in _AUTO_CLOSE_TAGS:
                            tag_stack.append(tname)
        else:
            # Non-tag text: escape inequalities
            if "<=" in text:
                text = text.replace("<=", "&le;")
                fixes.append("escaped_inequality_le")
            if ">=" in text:
                text = text.replace(">=", "&ge;")
                fixes.append("escaped_inequality_ge")
            if "<" in text:
                text = text.replace("<", "&lt;")
                fixes.append("escaped_inequality_lt")
            if ">" in text:
                text = text.replace(">", "&gt;")
                fixes.append("escaped_inequality_gt")

            # Escape bare & (not start of valid entity)
            if "&" in text:
                escaped_amp = _ENTITY_RE.sub("", text)
                if "&" in escaped_amp:
                    text = re.sub(r"&(?!(#\d+|#x[0-9a-fA-F]+|[a-zA-Z]+);)", "&amp;", text)
                    fixes.append("escaped_bare_amp")

            out_tokens.append(text)

    # 6. Auto-close remaining open tags in reverse order
    while tag_stack:
        unclosed = tag_stack.pop()
        out_tokens.append(f"</{unclosed}>")
        fixes.append(f"auto_closed_tag_{unclosed}")

    result = "".join(out_tokens)

    # Restore comments
    for i, comm in enumerate(comments):
        result = result.replace(f"__COMMENT_TOKEN_{i}__", comm)

    # Remove duplicates from fixes while preserving order
    deduped_fixes = list(dict.fromkeys(fixes))
    return result, deduped_fixes


def auto_fix_csv_line(
    line: str,
    note_type: Literal["QA", "CLOZE"],
    *,
    expected_columns: int = 7,
) -> tuple[str, list[str]]:
    """Sanitize and repair a single pipe-delimited CSV line."""
    line = line.strip("\r\n")
    if not line:
        return "", []

    cols = line.split("|")
    fixes: list[str] = []

    # If row had extra columns due to unescaped pipes, try to reconcile
    if len(cols) > expected_columns:
        # Heuristic: columns 3 (Notas Adicionais) or 0 (Enunciado/Sentenca) usually contain unescaped pipes
        excess = len(cols) - expected_columns
        # Merge columns at index 3 (Notas Adicionais)
        notas_merged = "&#124;".join(cols[3 : 3 + excess + 1])
        cols = [*cols[:3], notas_merged, *cols[3 + excess + 1 :]]
        fixes.append(f"reconciled_excess_columns_{excess}")

    # Pad if missing columns
    while len(cols) < expected_columns:
        cols.append("")
        fixes.append("padded_missing_column")

    # Sanitize each column
    sanitized_cols = []
    for idx, col in enumerate(cols):
        is_images = idx == 4 if note_type == "QA" else idx == 3
        clean_val, col_fixes = sanitize_field(col, is_images_field=is_images)
        sanitized_cols.append(clean_val)
        if col_fixes:
            fixes.extend([f"col_{idx}:{f}" for f in col_fixes])

    return "|".join(sanitized_cols), fixes


def auto_fix_csv_block(
    csv_text: str,
    note_type: Literal["QA", "CLOZE"],
) -> tuple[str, list[str]]:
    """Auto-repair an entire CSV block (header + data rows)."""
    from anki_mcp.checks.csv_structure import (
        CLOZE_HEADER_CANONICAL,
        QA_HEADER_CANONICAL,
    )

    lines = csv_text.splitlines()
    if not lines:
        return "", ["empty_input"]

    canonical_header = QA_HEADER_CANONICAL if note_type == "QA" else CLOZE_HEADER_CANONICAL
    expected_cols = 7 if note_type == "QA" else 6

    output_lines = [canonical_header]
    all_fixes: list[str] = []

    # Check if first line was header
    start_idx = 0
    first_line = lines[0].strip()
    if (
        "Enunciado" in first_line
        or "Sentença" in first_line
        or "Resposta" in first_line
        or "Notas Adicionais" in first_line
    ):
        start_idx = 1
        if first_line != canonical_header:
            all_fixes.append("standardized_header")
    else:
        all_fixes.append("inserted_canonical_header")

    # Process data lines
    row_num = 1
    for raw_line in lines[start_idx:]:
        # Only the line terminator is removed: field content is never stripped (M3).
        raw_line = raw_line.rstrip("\r\n")
        if not raw_line.strip():
            continue
        fixed_line, line_fixes = auto_fix_csv_line(
            raw_line,
            note_type,
            expected_columns=expected_cols,
        )
        output_lines.append(fixed_line)
        if line_fixes:
            all_fixes.append(f"row_{row_num}: {', '.join(line_fixes)}")
        row_num += 1

    return "\n".join(output_lines), all_fixes
