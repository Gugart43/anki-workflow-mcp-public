"""checks/cloze.py — anki-cloze-creator implementation.

Checks:
  CLOZE.NONSEQ   — indices must start at c1, be sequential, no gaps or duplicates
  CLOZE.NESTED   — no nested {{c1::...{{c2::...}}...}} deletions
  CLOZE.BOUNDARY — deletion swallows preposition/punctuation/unit (heuristic WARN)
  CLOZE.CONTEXT_WARN — carrier sentence lacks clinical context (heuristic WARN)

Also exposes:
  count_cloze_indices(sentence) -> set[int]  — used by batch counter
"""

from __future__ import annotations

import itertools
import re

from anki_mcp.core import vocab
from anki_mcp.models import Diagnostic

# Matches {{cN::target}} or {{cN::target::hint}} (N is the captured number)
_CLOZE_RE = re.compile(r"\{\{c(\d+)::([^}]*?)(?:::([^}]*?))?\}\}")

# Prepositions / prefixes that should NOT be inside the deletion boundary
_PREFIX_WORDS = re.compile(
    r"^\s*(de|do|da|dos|das|em|no|na|nos|nas|por|para|ao|à|até|com|entre|sobre"
    r"|of|in|at|to|for|by|with|from|the|a|an)\s+",
    re.IGNORECASE,
)
# Suffixes that suggest the deletion swallowed a unit or punctuation
_SUFFIX_UNIT = re.compile(
    r"\s+(mg|mcg|µg|g|kg|mL|L|mmHg|mmol|mEq|UI|U|%|°C)\s*$",
    re.IGNORECASE,
)
_SUFFIX_PUNCT = re.compile(r"[,;:\.]\s*$")


def _find_cloze_spans(sentence: str) -> list[tuple[int, str, str | None, int, int]]:
    """Extrai (index_num, target, hint_or_None, start, end) suportando chaves LaTeX {a}/{b}."""
    results: list[tuple[int, str, str | None, int, int]] = []
    n = len(sentence)
    i = 0
    while i < n - 6:
        if sentence[i : i + 3] == "{{c" and sentence[i + 3].isdigit():
            j = i + 3
            while j < n and sentence[j].isdigit():
                j += 1
            if sentence[j : j + 2] != "::":
                i += 1
                continue
            idx = int(sentence[i + 3 : j])
            content_start = j + 2
            k = content_start
            brace_depth = 0
            sep_pos: int | None = None
            closed = False
            while k < n - 1:
                # Fecha o Cloze quando encontra '}}' em brace_depth == 0
                if sentence[k : k + 2] == "}}" and brace_depth == 0:
                    raw_body = sentence[content_start:k]
                    if sep_pos is not None:
                        target = sentence[content_start:sep_pos]
                        hint = sentence[sep_pos + 2 : k]
                    else:
                        target = raw_body
                        hint = None
                    results.append((idx, target, hint, i, k + 2))
                    i = k + 2
                    closed = True
                    break
                elif sentence[k : k + 2] == "::" and brace_depth == 0 and sep_pos is None:
                    sep_pos = k
                    k += 2
                    continue
                elif sentence[k] == "{" and (k == 0 or sentence[k - 1] != "\\"):
                    brace_depth += 1
                elif sentence[k] == "}" and (k == 0 or sentence[k - 1] != "\\"):
                    if brace_depth > 0:
                        brace_depth -= 1
                k += 1
            if not closed:
                i += 1
        else:
            i += 1
    return results


def count_cloze_indices(sentence: str) -> set[int]:
    """Return the set of distinct cN indices found in *sentence*."""
    return {idx for idx, *_ in _find_cloze_spans(sentence)}


def check_cloze(sentence: str, row: int | None = None, field: str = "Sentença") -> list[Diagnostic]:
    """Full cloze check for a single sentence field."""
    diags: list[Diagnostic] = []
    spans = _find_cloze_spans(sentence)

    if not spans:
        return diags  # no cloze deletions — nothing to check here

    # ── CLOZE.NESTED ─────────────────────────────────────────────────────────
    # A nested cloze would have a {{c inside a target — detect it
    if re.search(r"\{\{c\d+::[^}]*\{\{c\d+", sentence):
        diags.append(
            Diagnostic.from_code(
                "CLOZE.NESTED",
                "Nested cloze deletions found",
                row=row,
                field=field,
            )
        )

    # ── CLOZE.ID_IN_TEXT ─────────────────────────────────────────────────────
    if re.search(r"^\s*[\(\[]?(?:ANKI-\d+|B\d+-[A-Z]+-\d+)[\)\]]?", sentence):
        diags.append(
            Diagnostic.from_code(
                "CLOZE.ID_IN_TEXT",
                "Visible card ID found in carrier sentence. IDs must reside in tags or metadata, not in the prompt.",
                row=row,
                field=field,
            )
        )

    # ── CLOZE.EXT_HINT_LEAK ──────────────────────────────────────────────────
    if re.search(
        r"\{\{c\d+::[^}]+\}\}\s*\((?:ex|exemplo|ex\.|e\.g\.)\s*:[^)]+\)", sentence, re.IGNORECASE
    ):
        diags.append(
            Diagnostic.from_code(
                "CLOZE.EXT_HINT_LEAK",
                "Parenthetical example immediately following cloze deletion may leak/spoil the answer. Use canonical hint syntax {{cN::target::hint}} instead.",
                row=row,
                field=field,
            )
        )

    # ── CLOZE.COTEXT_LEAK ───────────────────────────────────────────────────
    # Detects patterns like: {{c1::classe...}}, como {{c2::droga}} ou droga_visivel
    if re.search(
        r"\{\{c\d+::[^}]*?(?:classe|grupo|categoria)[^}]*?\}\}\s*,\s*(?:como|a exemplo de|tais como)\s+.*?\bou\b",
        sentence,
        re.IGNORECASE,
    ):
        diags.append(
            Diagnostic.from_code(
                "CLOZE.COTEXT_LEAK",
                "Visible exemplar tokens following a class deletion may leak the answer. Separate into independent cards or omit visible exemplars.",
                row=row,
                field=field,
            )
        )

    # ── CLOZE.ACRONYM_LEAK & CLOZE.TOO_LONG ──────────────────────────────────
    for idx, target, _hint, start, end in spans:
        target_clean = target.strip()
        target_vis_len = vocab.visible_len(target_clean)
        if target_vis_len > 120:
            diags.append(
                Diagnostic.from_code(
                    "CLOZE.TOO_LONG",
                    f"c{idx}: Deletion target is long ({target_vis_len} visible chars > 120; sinal de revisão, não prova de não atomicidade).",
                    row=row,
                    field=field,
                    suggestion=(
                        "Revise se a lacuna oculta itens independentes ou detalhe opcional; "
                        "preserve o contexto necessário na sentença e mova explicações para Notas Adicionais."
                    ),
                )
            )

        # Check acronym leak: Target followed by (ACRONYM) where acronym matches target
        post = sentence[end : end + 25].strip()
        m_post = re.match(r"^\(([A-Z0-9]{2,8})\)", post)
        if m_post and len(target_clean.split()) >= 2:
            sigla = m_post.group(1)
            words = [w for w in re.split(r"[\s\-]+", target_clean) if len(w) > 2 or w.isupper()]
            initials = "".join(w[0].upper() for w in words)
            if (
                initials
                and sigla[0] == initials[0]
                and (sigla in initials or initials in sigla or len(set(sigla) & set(initials)) >= 2)
            ):
                diags.append(
                    Diagnostic.from_code(
                        "CLOZE.ACRONYM_LEAK",
                        f"c{idx}: Target '{target_clean}' is followed by visible parenthetical acronym '({sigla})', which spoils the answer. Enclose both in the deletion: '{{{{c{idx}::{target_clean} ({sigla})}}}}'.",
                        row=row,
                        field=field,
                    )
                )

        # Check acronym leak: Target is ACRONYM preceded by Full Name (
        if re.match(r"^[A-Z0-9]{2,8}$", target_clean):
            pre = sentence[max(0, start - 70) : start].strip()
            if pre.endswith("("):
                name_part = pre[:-1].strip()
                words = [w for w in re.split(r"[\s\-]+", name_part) if len(w) > 2]
                if len(words) >= 2:
                    initials = "".join(w[0].upper() for w in words[-len(target_clean) :])
                    if initials and target_clean[0] == initials[0]:
                        diags.append(
                            Diagnostic.from_code(
                                "CLOZE.ACRONYM_LEAK",
                                f"c{idx}: Acronym target '{target_clean}' is preceded by visible full name '{name_part}', which spoils the answer. Enclose both in the deletion.",
                                row=row,
                                field=field,
                            )
                        )

    # ── CLOZE.NONSEQ ─────────────────────────────────────────────────────────
    indices = [idx for idx, *_ in spans]
    distinct = sorted(set(indices))

    if len(indices) != len(distinct):
        diags.append(
            Diagnostic.from_code(
                "CLOZE.NONSEQ",
                "Duplicate cloze index found in the same note (each cN index must appear exactly once)",
                row=row,
                field=field,
            )
        )

    if distinct[0] != 1:
        diags.append(
            Diagnostic.from_code(
                "CLOZE.NONSEQ",
                f"Cloze indices must start at c1, but first index is c{distinct[0]}",
                row=row,
                field=field,
            )
        )
    for a, b in itertools.pairwise(distinct):
        if b != a + 1:
            diags.append(
                Diagnostic.from_code(
                    "CLOZE.NONSEQ",
                    f"Gap in cloze indices: c{a} followed by c{b} (c{a + 1} missing)",
                    row=row,
                    field=field,
                )
            )

    # ── CLOZE.BOUNDARY (heuristic WARN) ──────────────────────────────────────
    for idx, target, _hint, _start, _end in spans:
        if _PREFIX_WORDS.match(target):
            diags.append(
                Diagnostic.from_code(
                    "CLOZE.BOUNDARY",
                    f"c{idx}: deletion starts with a preposition/article ('{target[:20]}…')",
                    row=row,
                    field=field,
                    suggestion=f"Move the preposition outside the deletion: 'de {{{{c{idx}::...}}}}'",
                )
            )
        if _SUFFIX_PUNCT.search(target):
            diags.append(
                Diagnostic.from_code(
                    "CLOZE.BOUNDARY",
                    f"c{idx}: deletion ends with punctuation — move it outside",
                    row=row,
                    field=field,
                )
            )

    # ── CLOZE.CONTEXT_WARN (heuristic WARN) ──────────────────────────────────
    # Very crude: if sentence is too short, warn
    cleaned = re.sub(r"\{\{c\d+::.*?\}\}", "BLANK", sentence)
    if len(cleaned) < 40:
        diags.append(
            Diagnostic.from_code(
                "CLOZE.CONTEXT_WARN",
                "Carrier sentence is very short (<40 chars) — may lack clinical context",
                row=row,
                field=field,
                suggestion="Provide full clinical scenario/guideline/population in the sentence",
            )
        )

    # ── CARD.SPLIT_CLOZE_INDICES / CARD.TOO_LONG_* (operational cap + advisory length) ──
    if len(distinct) > vocab.CLOZE_MAX_INDICES:
        diags.append(
            vocab.make_diag(
                "CARD.SPLIT_CLOZE_INDICES",
                f"{len(distinct)} índices cloze (máximo operacional {vocab.CLOZE_MAX_INDICES}): "
                "cada índice gera um cartão separado no Anki e contar índices não prova atomicidade",
                row=row,
                field=field,
                suggestion=(
                    "SPLIT: mantenha no máximo "
                    f"{vocab.CLOZE_MAX_INDICES} índices c1..c{vocab.CLOZE_MAX_INDICES} sequenciais por nota "
                    "e revise cada lacuna quanto a listas ocultas ou obrigações independentes."
                ),
            )
        )

    diag = vocab.limit_diag(
        "CARD.TOO_LONG_SENTENCE",
        "Sentença (carrier) extensa",
        limit=vocab.LIMITS["cloze_carrier"],
        value=vocab.visible_len(sentence),
        row=row,
        field=field,
        suggestion=(
            "Revise se há fatos independentes; preserve o contexto necessário na sentença "
            "e mova explicação opcional para Notas Adicionais."
        ),
    )
    if diag is not None:
        diags.append(diag)

    for idx, target, _hint, _start, _end in spans:
        diag = vocab.limit_diag(
            "CARD.TOO_LONG_DELETION",
            f"c{idx}: deleção extensa",
            limit=vocab.LIMITS["cloze_deletion"],
            value=vocab.visible_len(target),
            row=row,
            field=field,
            suggestion=(
                "Revise se a lacuna oculta uma lista ou explicação opcional; preserve o contexto necessário "
                "na sentença e mova detalhes para Notas Adicionais."
            ),
        )
        if diag is not None:
            diags.append(diag)

    return diags
