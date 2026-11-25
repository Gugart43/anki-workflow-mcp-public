"""checks/notes.py — anki-notes-enricher implementation.

Checks:
  NOTE.METADATA       — forbidden metadata headers (ID:, Âncora:, Cross-ref:, Relevância:)
  NOTE.DUP_ANSWER_WARN— notes field verbatim-repeats the answer (normalized HTML/entities/punct)
  NOTE.EXAM_ONLY_WARN — notes contain only exam-appearance mention without explanation
  NOTE.LENGTH_WARN    — notes field outside recommended length (WARN, configurable)
  NOTE.STYLE_WARN     — items not using • <b>…</b> pattern (WARN)
"""

from __future__ import annotations

import re

from anki_mcp.checks.information import (
    BULLET_GLYPHS,
    is_exact_answer_restatement,
    is_exam_only_note,
    parse_html_text,
    visible_char_count,
)
from anki_mcp.core import vocab
from anki_mcp.models import Diagnostic

DEFAULT_MAX_LENGTH = 1200  # characters; configurable via deck config

# Rótulo de metadado proibido, ancorado no começo de uma linha visível.
#
# `[*_]{0,2}` aceita o realce markdown que o autor digita (`**ID:**`, `__Âncora:__`) — sem
# ele, um campo com `**ID:**` escapava da checagem (Onda 2, B26). Espelha
# `notes::METADATA_HEADER_LABEL_RE` no Rust; o bullet inicial é removido antes por
# `_strip_leading_marker`. Grupo 1 = rótulo, para a mensagem continuar a histórica.
_METADATA_HEADER_LABEL = re.compile(
    r"^[*_]{0,2}\s*(ID\s*:|Âncora\s*:|Anchor\s*:|Cross-ref\s*:|Cross\s+ref\s*:|Relevância\s*:|Relevancia\s*:)",
    re.IGNORECASE,
)

# A união de glifos vive em `checks.information.BULLET_GLYPHS` (espelha
# `anki_validate::text_norm::BULLET_GLYPHS`, Onda 2/B24) — não repetir aqui.


def _strip_leading_marker(line: str) -> str:
    """Remove um bullet inicial e o espaço seguinte (espelha `text_norm::strip_leading_marker`)."""
    stripped = line.lstrip()
    if stripped and stripped[0] in BULLET_GLYPHS:
        return stripped[1:].lstrip()
    return line


# Style: list items should use "• <b>Tópico:</b>"
_STYLE_ITEM_RE = re.compile(r"[•\-\*]\s+(?!<b>)", re.MULTILINE)


def check_notes(
    notes: str,
    answer: str = "",
    *,
    row: int | None = None,
    field: str = "Notas Adicionais",
    max_length: int = DEFAULT_MAX_LENGTH,
) -> list[Diagnostic]:
    diags: list[Diagnostic] = []

    # Notes are optional post-answer study context, not a recall obligation.
    # Length is advisory only; atomicity limits apply to the stem and answer.
    # ── NOTE.METADATA ─────────────────────────────────────────────────────────
    #
    # Varre o texto visível linha a linha (mesmo `parse_html_text` do Rust) para poder
    # remover o bullet inicial antes do rótulo (Onda 2, B26): `• ID:` e `**ID:**` são o
    # mesmo metadado que `ID:`.
    block_text, _, _ = parse_html_text(notes)
    for line in block_text.splitlines():
        m = _METADATA_HEADER_LABEL.match(_strip_leading_marker(line))
        if m:
            diags.append(
                Diagnostic.from_code(
                    "NOTE.METADATA",
                    f"Forbidden metadata header found: '{m.group(1).strip()}'",
                    row=row,
                    field=field,
                    suggestion="Remove metadata — Notes field is for pedagogical explanations only",
                )
            )

    # ── NOTE.DUP_ANSWER_WARN ─────────────────────────────────────────────────
    if answer and notes and is_exact_answer_restatement(notes, answer):
        diags.append(
            Diagnostic.from_code(
                "NOTE.DUP_ANSWER_WARN",
                "Notes repeat the answer verbatim — deepen the explanation instead",
                row=row,
                field=field,
                suggestion="Explain *why* the answer is correct or add related nuance from the source",
            )
        )

    # ── NOTE.EXAM_ONLY_WARN ──────────────────────────────────────────────────
    if notes and is_exam_only_note(notes):
        diags.append(
            Diagnostic.from_code(
                "NOTE.EXAM_ONLY_WARN",
                "Notes contain only an exam mention without pedagogical explanation",
                row=row,
                field=field,
                suggestion=(
                    "Menção isolada a prova não explica a unidade; acrescente explicação útil da fonte "
                    "ou deixe Notas Adicionais vazio."
                ),
            )
        )

    # ── NOTE.LENGTH_WARN ─────────────────────────────────────────────────────
    has_table = "<table" in notes.lower()
    effective_max = max(max_length, 2500) if has_table else max_length
    visible_length = visible_char_count(notes) if notes else vocab.visible_len(notes)
    if visible_length > effective_max:
        diags.append(
            Diagnostic.from_code(
                "NOTE.LENGTH_WARN",
                f"Notes field is {visible_length} visible chars (recommended ≤ {effective_max}; advisory only)",
                row=row,
                field=field,
            )
        )

    # ── NOTE.STYLE_WARN ──────────────────────────────────────────────────────
    notes_outside_tables = re.sub(r"<table[\s\S]*?</table>", "", notes, flags=re.IGNORECASE)
    if _STYLE_ITEM_RE.search(notes_outside_tables):
        diags.append(
            Diagnostic.from_code(
                "NOTE.STYLE_WARN",
                "Legacy list formatting found — preferred: '• <b>Tópico:</b> …'",
                row=row,
                field=field,
            )
        )

    return diags
