"""checks/csv_structure.py — formatting-guide implementation.

Checks:
  1  FMT.HEADER_MISMATCH  — header must match note type exactly
  2  FMT.COLUMN_COUNT     — constant column count per row
  3  FMT.RAW_PIPE         — no raw '|' inside fields (must be &#124;)
  4  FMT.RAW_NEWLINE      — no physical \\r/\\n inside fields (use <br>)
  5  FMT.ENTITY_MISSING   — raw < or > outside allowed tags → &lt;/&gt;
  6  FMT.AMP_RAW          — raw & not part of a valid entity
  7  FMT.TAG_FORBIDDEN    — only whitelisted HTML tags allowed
  8  FMT.BOM              — UTF-8 BOM
  9  MATH.UNBALANCED      — \\(...\\) and \\[...\\] must be balanced
  10 MATH.DOLLAR          — raw $ used for math
"""

from __future__ import annotations

import re

from anki_mcp.core import vocab
from anki_mcp.core.rows import split_row
from anki_mcp.core.sanitize import sanitize_field
from anki_mcp.models import CLOZE_FIELDS, QA_FIELDS, Diagnostic

# ── Constants ─────────────────────────────────────────────────────────────────
QA_HEADER_CANONICAL = "|".join(QA_FIELDS)
CLOZE_HEADER_CANONICAL = "|".join(CLOZE_FIELDS)

# Tags permitidas fora do campo de imagens (fonte única: core/vocab.py)
_ALWAYS_ALLOWED = frozenset(set(vocab.ALLOWED_TAGS) | {f"/{t}" for t in vocab.ALLOWED_TAGS})
# Tags permitidas SOMENTE no campo "Imagens Relacionadas"
_IMAGES_ONLY = frozenset(set(vocab.IMAGES_ONLY_TAGS) | {f"/{t}" for t in vocab.IMAGES_ONLY_TAGS})
# Paired tags que precisam de fechamento
_PAIRED_TAGS = vocab.PAIRED_TAGS
# Regex patterns
_ENTITY_RE = re.compile(r"&(#\d+|#x[\da-fA-F]+|[a-zA-Z]+);")
#: A tag is ``<`` + tag name/slash + (quoted attribute values | any char that is not `>`, `"`, `'`) + `>`
#: Requiring [a-zA-Z/?!] prevents clinical inequalities like ``< 135 mEq/L`` from false-matching as HTML tags.
_TAG_RE = re.compile(r"""<([a-zA-Z/?!](?:"[^"]*"|'[^']*'|[^>"'])*)>""")
#: ``class`` attribute value: double-quoted, single-quoted or unquoted (HTML5).
#: The lookbehind keeps ``data-class="x"`` / ``myclass="x"`` from matching.
_CLASS_ATTR_RE = re.compile(
    r"""(?<![-\w])class\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+))""",
    re.IGNORECASE,
)
_DOLLAR_MATH = re.compile(r"(?<![a-zA-Z])\$(?!\$)")  # single $, not $$
_COMMENT_RE = re.compile(r"<!--.*?-->", re.DOTALL)


def _mask_comments(value: str) -> tuple[str, int]:
    """Replace HTML comments with an inert, same-length run of ``#``.

    Comment content is never rendered, so it must not be parsed as markup nor
    contribute phantom diagnostics. Returns the masked text and the count.
    """
    count = 0

    def _repl(m: re.Match[str]) -> str:
        nonlocal count
        count += 1
        return "#" * len(m.group(0))

    return _COMMENT_RE.sub(_repl, value), count


def check_bom(raw_bytes: bytes) -> list[Diagnostic]:
    """Check #8: detects UTF-8 BOM prefix."""
    if raw_bytes.startswith(b"\xef\xbb\xbf"):
        return [Diagnostic.from_code("FMT.BOM", "UTF-8 BOM detected at start of file")]
    return []


def _tag_name(tag_str: str) -> str:
    """Extract tag name (lower-cased) from raw tag content like 'img src=".."' or '/b'."""
    s = tag_str.strip()
    is_close = s.startswith("/")
    m = re.match(r"/?([a-zA-Z][a-zA-Z0-9]*)", s)
    if not m:
        return ""
    name = m.group(1).lower()
    return f"/{name}" if is_close else name


def check_csv(
    csv_text: str,
    note_type: str,  # "QA" or "CL"
    *,
    enforce_header: bool = True,
) -> list[Diagnostic]:
    """Full structural check of a CSV block (header + data rows)."""
    diags: list[Diagnostic] = []

    lines = csv_text.splitlines()
    if not lines:
        return diags

    expected_fields = QA_FIELDS if note_type == "QA" else CLOZE_FIELDS
    expected_count = len(expected_fields)

    # ── Header check ─────────────────────────────────────────────────────────
    header_line = lines[0]
    if enforce_header:
        canonical = QA_HEADER_CANONICAL if note_type == "QA" else CLOZE_HEADER_CANONICAL
        if header_line.strip() != canonical:
            diags.append(
                Diagnostic.from_code(
                    "FMT.HEADER_MISMATCH",
                    f"Expected: '{canonical}' | Got: '{header_line.strip()}'",
                    row=1,
                    suggestion=canonical,
                )
            )

    data_lines = lines[1:]  # everything after header

    for lineno, line in enumerate(data_lines, start=2):
        if not line.strip():
            continue  # skip blank lines

        row = split_row(line, expected_count)

        # ── Column count ─────────────────────────────────────────────────────
        if not row.structural_ok:
            # Structurally invalid: the values are misaligned, so per-field checks
            # on them would blame the wrong cell. Report and skip the row's fields.
            diags.append(
                Diagnostic.from_code(
                    "FMT.COLUMN_COUNT",
                    f"Row {lineno}: expected {expected_count} columns, got {row.count}",
                    row=lineno,
                )
            )
            diags.append(
                Diagnostic.from_code(
                    "FMT.FIELD_SHIFTED",
                    f"Row {lineno}: field checks SKIPPED_STRUCTURAL "
                    f"(found {row.count} columns; a raw '|' inside a field shifts the rest)",
                    row=lineno,
                    suggestion="Escape every raw '|' inside a field as &#124;",
                )
            )
            continue

        # ── Per-field checks ─────────────────────────────────────────────────
        for field_name, value in zip(expected_fields, row.cols, strict=True):
            is_images_field = field_name == "Imagens Relacionadas"

            # Check 3: raw newline (shouldn't appear — we split on newlines — but
            # could slip in via \\n literals in the CSV content itself)
            if "\n" in value or "\r" in value:
                diags.append(
                    Diagnostic.from_code(
                        "FMT.RAW_NEWLINE",
                        f"Row {lineno}, field '{field_name}': raw newline character",
                        row=lineno,
                        field=field_name,
                        suggestion="Replace with <br>",
                    )
                )

            # HTML comments are invisible when rendered: everything inside them
            # (tags, entities, class attributes, '$') is dead text. Mask them so
            # they yield exactly one diagnostic and never phantom ones.
            masked, n_comments = _mask_comments(value)
            for _ in range(n_comments):
                diags.append(
                    Diagnostic.from_code(
                        "FMT.TAG_FORBIDDEN",
                        f"Row {lineno}, field '{field_name}': HTML comment is not allowed "
                        f"in a card field (never rendered)",
                        row=lineno,
                        field=field_name,
                        suggestion="Delete the comment",
                    )
                )

            # Check 5+6: entities and raw < > &
            diags.extend(_check_entities_and_brackets(masked, lineno, field_name, is_images_field))

            # Check 7: forbidden tags
            diags.extend(_check_tags(masked, lineno, field_name, is_images_field))

            # Check 7b: semantic vocabulary (class names) — WARN only
            diags.extend(_check_classes(masked, lineno, field_name))

            # Check 9+10: MathJax
            diags.extend(_check_mathjax(masked, lineno, field_name))

    return diags


def _check_entities_and_brackets(
    value: str, row: int, field: str, is_images_field: bool = False
) -> list[Diagnostic]:
    diags: list[Diagnostic] = []
    # Strip all known valid entities and allowed HTML tags to find naked < > &
    stripped = _ENTITY_RE.sub("", value)
    # Remove allowed tags (we'll check tags separately)
    stripped = _TAG_RE.sub("", stripped)

    sanitized, _actions = sanitize_field(value, is_images_field=is_images_field)
    auto_fix_available = sanitized != value
    fix_preview = sanitized[:200] if auto_fix_available else None

    if "<" in stripped or ">" in stripped:
        diags.append(
            Diagnostic.from_code(
                "FMT.ENTITY_MISSING",
                f"Row {row}, field '{field}': raw '<' or '>' — use &lt; / &gt;",
                row=row,
                field=field,
                suggestion="Replace < with &lt; and > with &gt;",
                auto_fix_available=auto_fix_available,
                fix_preview=fix_preview,
            )
        )

    # Check raw & — anything after removing valid entities
    amp_stripped = _ENTITY_RE.sub("", value)
    if "&" in amp_stripped:
        diags.append(
            Diagnostic.from_code(
                "FMT.AMP_RAW",
                f"Row {row}, field '{field}': raw '&' not part of a valid HTML entity",
                row=row,
                field=field,
                suggestion="Replace & with &amp;",
                auto_fix_available=auto_fix_available,
                fix_preview=fix_preview,
            )
        )
    return diags


def _check_classes(value: str, row: int, field: str) -> list[Diagnostic]:
    """Check 7b: class tokens must belong to the semantic vocabulary.

    Severity is WARN by design: an unknown class renders unstyled, it does not
    corrupt the card. Rejecting would be hostile to a writer exploring the
    vocabulary; warning keeps the feedback loop tight without blocking.

    The scan is *tag-scoped*: only ``class`` attributes inside a real tag count.
    Text that merely looks like markup is ignored, so an escaped tag
    (``&lt;span class="x"&gt;``), prose mentioning ``class="x"`` and HTML
    comments never raise a false positive. Unquoted HTML5 values
    (``class=nl``) are scanned too, and the lookbehind keeps ``data-class=``
    and ``myclass=`` from matching.
    """
    diags: list[Diagnostic] = []
    seen: set[str] = set()
    for tag_match in _TAG_RE.finditer(value):
        tag_inner = tag_match.group(1)
        if tag_inner.startswith("!") or tag_inner.startswith("?"):
            continue  # comment / declaration — never rendered, flagged elsewhere
        for m in _CLASS_ATTR_RE.finditer(tag_inner):
            tokens = next((g for g in m.groups() if g is not None), "") or ""
            for bad in vocab.unknown_classes(tokens):
                if bad in seen:
                    continue
                seen.add(bad)
                diags.append(
                    Diagnostic.from_code(
                        "FMT.CLASS_UNKNOWN",
                        f"Row {row}, field '{field}': class '{bad}' is not in the semantic vocabulary "
                        f"(renders unstyled)",
                        row=row,
                        field=field,
                        suggestion="Use one of: "
                        + ", ".join(f".{c}" for c in sorted(vocab.ALLOWED_CLASSES)),
                    )
                )
    return diags


def _check_tags(value: str, row: int, field: str, is_images_field: bool) -> list[Diagnostic]:
    diags: list[Diagnostic] = []
    tag_stack: list[str] = []

    for m in _TAG_RE.finditer(value):
        tag_raw = m.group(1).strip()
        if tag_raw.startswith("!") or tag_raw.startswith("?"):
            # ``<!DOCTYPE …>``, ``<?processing?>`` — declarations, not elements
            diags.append(
                Diagnostic.from_code(
                    "FMT.TAG_FORBIDDEN",
                    f"Row {row}, field '{field}': HTML declaration is not allowed "
                    f"in a card field (never rendered)",
                    row=row,
                    field=field,
                    suggestion="Delete the declaration",
                )
            )
            continue
        tag_name = _tag_name(tag_raw)

        if tag_name in _ALWAYS_ALLOWED:
            pass
        elif tag_name in _IMAGES_ONLY and is_images_field:
            pass
        elif tag_name in _IMAGES_ONLY and not is_images_field:
            diags.append(
                Diagnostic.from_code(
                    "FMT.TAG_FORBIDDEN",
                    f"Row {row}, field '{field}': <{tag_name}> only allowed in 'Imagens Relacionadas'",
                    row=row,
                    field=field,
                )
            )
        elif tag_name not in _ALWAYS_ALLOWED:
            diags.append(
                Diagnostic.from_code(
                    "FMT.TAG_FORBIDDEN",
                    f"Row {row}, field '{field}': tag <{tag_name}> is not in the allowed whitelist",
                    row=row,
                    field=field,
                    suggestion="Allowed: <b> <i> <strong> <em> <br>; in Imagens field also <figure> <img> <figcaption>",
                )
            )

        # Track pairing for paired tags
        if tag_name.startswith("/"):
            close_name = tag_name[1:]
            if close_name in _PAIRED_TAGS:
                if tag_stack and tag_stack[-1] == close_name:
                    tag_stack.pop()
                else:
                    diags.append(
                        Diagnostic.from_code(
                            "FMT.TAG_UNCLOSED",
                            f"Row {row}, field '{field}': unmatched closing tag </{close_name}>",
                            row=row,
                            field=field,
                            suggestion=f"Remove or match </{close_name}>",
                        )
                    )
        elif tag_name in _PAIRED_TAGS:
            tag_stack.append(tag_name)

    for unclosed in tag_stack:
        diags.append(
            Diagnostic.from_code(
                "FMT.TAG_UNCLOSED",
                f"Row {row}, field '{field}': unclosed tag <{unclosed}>",
                row=row,
                field=field,
                suggestion=f"Close tag with </{unclosed}>",
            )
        )

    return diags


def _check_mathjax(value: str, row: int, field: str) -> list[Diagnostic]:
    diags: list[Diagnostic] = []
    # Count \\( ... \\) pairs
    inline_opens = len(re.findall(r"\\\(", value))
    inline_closes = len(re.findall(r"\\\)", value))
    if inline_opens != inline_closes:
        diags.append(
            Diagnostic.from_code(
                "MATH.UNBALANCED",
                f"Row {row}, field '{field}': unbalanced \\( \\) — {inline_opens} open, {inline_closes} close",
                row=row,
                field=field,
            )
        )
    block_opens = len(re.findall(r"\\\[", value))
    block_closes = len(re.findall(r"\\\]", value))
    if block_opens != block_closes:
        diags.append(
            Diagnostic.from_code(
                "MATH.UNBALANCED",
                f"Row {row}, field '{field}': unbalanced \\[ \\] — {block_opens} open, {block_closes} close",
                row=row,
                field=field,
            )
        )
    # $ used for math
    if _DOLLAR_MATH.search(value):
        diags.append(
            Diagnostic.from_code(
                "MATH.DOLLAR",
                f"Row {row}, field '{field}': raw '$' — use \\( \\) for math",
                row=row,
                field=field,
            )
        )
    return diags
