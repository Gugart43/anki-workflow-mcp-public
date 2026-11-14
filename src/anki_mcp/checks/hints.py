"""checks/hints.py — anki-cloze-hints implementation.

Checks:
  HINT.FORMAT        — invalid hint syntax (structural issues)
  HINT.SYNONYM       — hint is a direct synonym / clue for the target
  HINT.ELIMINATION   — hint enables binary/small-category elimination
  HINT.ROOT_LEAK     — hint contains a root/derivative of the hidden target
"""

from __future__ import annotations

import re

from anki_mcp.models import Diagnostic

_CLOZE_WITH_HINT_RE = re.compile(r"\{\{c(\d+)::([^:}]+)::([^}]+)\}\}")


def _normalize(text: str) -> str:
    """Lowercase, strip accents crudely, remove punctuation for comparison."""
    import unicodedata

    text = unicodedata.normalize("NFD", text.lower())
    text = "".join(c for c in text if unicodedata.category(c) != "Mn")
    return re.sub(r"[^a-z0-9 ]", " ", text).strip()


def _token_overlap_ratio(a: str, b: str) -> float:
    """Jaccard overlap of word tokens (normalized)."""
    ta = set(_normalize(a).split())
    tb = set(_normalize(b).split())
    if not ta or not tb:
        return 0.0
    return len(ta & tb) / len(ta | tb)


def _has_root_leak(target: str, hint: str) -> bool:
    """True if the hint contains the target's primary content word verbatim.

    ROOT_LEAK fires when the hint literally contains the key term being tested,
    not just a shared category descriptor.

    Examples:
      LEAK:    target='miastenia gravis', hint='doença da placa... com miastenia' → True
      LEAK:    target='masculino', hint='masculino ou feminino' → True
      NO LEAK: target='vitamina D', hint='vitamina sérica' → shared adjective, not leak
      NO LEAK: target='transaminases hepáticas', hint='enzimas hepáticas monitoradas' → category
    """
    nt = _normalize(target)
    nh = _normalize(hint)

    # Common medical/Portuguese modifiers that should NOT trigger ROOT_LEAK
    # (they're shared categories, not leaked content words)
    _COMMON_MODIFIERS = frozenset(
        [
            "hepatica",
            "hepaticas",
            "serica",
            "sericas",
            "sistemica",
            "sistemicas",
            "cronica",
            "cronico",
            "aguda",
            "agudo",
            "primaria",
            "primario",
            "secundaria",
            "secundario",
            "severa",
            "grave",
            "monitorada",
            "monitoradas",
            "monitorado",
        ]
    )

    target_tokens = set(nt.split())
    hint_tokens = set(nh.split())

    # Only match content words from the target (≥5 chars, not common modifiers)
    leaked = (target_tokens & hint_tokens) - _COMMON_MODIFIERS
    leaked = {w for w in leaked if len(w) >= 5}

    return bool(leaked)


# Patterns that suggest binary/small-category elimination in the hint
# (must be 2-way alternation with "ou/or", not slash-separated class descriptions)
_ELIMINATION_PATTERN = re.compile(
    r"\b(ou|or|e\/ou|and\/or)\b.{0,20}\b(sim|não|yes|no|masculino|feminino|male|female"
    r"|ieca|bra|bcc|ara|aines|aas)\b"
    r"|\b(sim|não|yes|no|masculino|feminino|male|female)\b.{0,20}\b(ou|or)\b",
    re.IGNORECASE,
)
# Short alternations "IECA/BRA" style (acronym/acronym or short-word/short-word, ≤6 chars each)
_ALT_SLASH = re.compile(r"\b([A-Z]{2,6})\s*/\s*([A-Z]{2,6})\b")


def check_hints(sentence: str, row: int | None = None, field: str = "Sentença") -> list[Diagnostic]:
    """Check all hints in a cloze sentence."""
    diags: list[Diagnostic] = []
    for m in _CLOZE_WITH_HINT_RE.finditer(sentence):
        cn = m.group(1)
        target = m.group(2).strip()
        hint = m.group(3).strip()

        if not hint:
            diags.append(
                Diagnostic.from_code(
                    "HINT.FORMAT",
                    f"c{cn}: hint is empty (either provide a valid hint or remove the :: separator)",
                    row=row,
                    field=field,
                )
            )
            continue

        # HINT.ROOT_LEAK — fast, deterministic (WARN when target itself appears in hint)
        if _has_root_leak(target, hint):
            # Determine severity: if ROOT_LEAK is the only signal (not a synonym), WARN
            diags.append(
                Diagnostic.from_code(
                    "HINT.ROOT_LEAK",
                    f"c{cn}: hint '{hint}' appears to contain a root/derivative of target '{target}'",
                    row=row,
                    field=field,
                    suggestion="Use a category label (e.g. 'classe farmacológica') not a derivative of the term",
                )
            )

        # HINT.SYNONYM — high token overlap indicates likely synonym
        overlap = _token_overlap_ratio(target, hint)
        if overlap > 0.5:
            diags.append(
                Diagnostic.from_code(
                    "HINT.SYNONYM",
                    f"c{cn}: hint '{hint}' has high token overlap ({overlap:.0%}) with target '{target}' — likely a synonym",
                    row=row,
                    field=field,
                    suggestion="The hint must state the *type* of entity, not what it looks like",
                )
            )

        # HINT.ELIMINATION — binary alternatives in hint
        if _ELIMINATION_PATTERN.search(hint) or _ALT_SLASH.search(hint):
            diags.append(
                Diagnostic.from_code(
                    "HINT.ELIMINATION",
                    f"c{cn}: hint '{hint}' may enable binary/small-category elimination",
                    row=row,
                    field=field,
                    suggestion="Avoid alternatives (A ou B / A/B) in hints — use a broad category instead",
                )
            )

    return diags
