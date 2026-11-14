"""core/vocab.py — the semantic vocabulary of the AnkiMBR note types.

Closed vocabulary for HTML markers compatible with AnkiMBR conventions.
Note-type templates and stylesheets are not bundled in this distribution.

Design contract
---------------
1. **Inline-first.** Every field is already rendered *inside* a styled
   container (`{{Enunciado}}` inside `.enunciado`, `{{Notas Adicionais}}`
   inside `.hints.style-notas`, `{{Imagens Relacionadas}}` inside `.media`).
   So content emits *inline* markers; it must never re-wrap the container.
2. **Graceful degradation.** Every class below is purely additive. If the
   `design-additions.css` block is lost (e.g. AnkiMBR resync), the card still
   renders as readable plain HTML.
3. **Vocabulary, not free-form CSS.** The writer picks from this closed list.
   An unknown class is a WARN, not a rejection — it still renders.
"""

from __future__ import annotations

import html
import re
from dataclasses import dataclass

# ── Tags ─────────────────────────────────────────────────────────────────────

#: Allowed in any text field (includes table caption and tfoot for TableSpec).
ALLOWED_TAGS: frozenset[str] = frozenset(
    {
        "b",
        "strong",
        "i",
        "em",
        "u",
        "s",
        "small",
        "span",
        "div",
        "p",
        "blockquote",
        "br",
        "hr",
        "sub",
        "sup",
        "code",
        "mark",
        "table",
        "caption",
        "thead",
        "tbody",
        "tfoot",
        "tr",
        "th",
        "td",
        "ul",
        "ol",
        "li",
    }
)

#: Allowed only inside the "Imagens Relacionadas" field.
IMAGES_ONLY_TAGS: frozenset[str] = frozenset({"img", "figure", "figcaption"})

#: Paired (non-void) tags tracked for matching open/close.
PAIRED_TAGS: frozenset[str] = frozenset(
    {
        "b",
        "strong",
        "i",
        "em",
        "u",
        "s",
        "small",
        "span",
        "div",
        "p",
        "blockquote",
        "sub",
        "sup",
        "code",
        "mark",
        "table",
        "caption",
        "thead",
        "tbody",
        "tfoot",
        "tr",
        "th",
        "td",
        "ul",
        "ol",
        "li",
        "figure",
        "figcaption",
    }
)

#: Void elements — never require a closing tag.
VOID_TAGS: frozenset[str] = frozenset({"br", "hr", "img"})

# ── Attributes ───────────────────────────────────────────────────────────────

#: Allowed attributes, by tag (``*`` = any tag).
ALLOWED_ATTRS: dict[str, frozenset[str]] = {
    "*": frozenset({"class"}),
    "img": frozenset({"src", "alt", "width", "height"}),
    "td": frozenset({"colspan", "rowspan"}),
    "th": frozenset({"colspan", "rowspan"}),
}

# ── Classes ──────────────────────────────────────────────────────────────────

#: Semantic class vocabulary. Grouped by intent so the writer can pick fast.
CLASS_VOCABULARY: dict[str, frozenset[str]] = {
    # Listas em Notas Adicionais (corrige a centralização do .hints)
    "lista": frozenset({"nl", "nl-i", "nl-sub", "rot"}),
    # Valores quantitativos: doses, unidades, faixas de estratificação
    "quantitativo": frozenset({"lab", "dose", "faixa", "uni"}),
    # Callouts clínicos
    "callout": frozenset({"alerta", "pearl"}),
    # Critérios diagnósticos e escalas
    "criterios": frozenset({"crit", "crit-i"}),
    # Tabelas comparativas
    "tabela": frozenset({"tbl"}),
    # Realce em linha
    "realce": frozenset({"termo", "sigla"}),
    # Já existentes no CSS dos note types (permitidas, não precisam do bloco aditivo)
    "nativo": frozenset({"media", "hints", "style-notas", "style-ref"}),
    # Mídia e figuras AnkiMBR (inclui suporte a figuras e quadros extraídos)
    "midia": frozenset({"anki-figure", "anki-table-figure", "fig-title", "fig-context"}),
}

#: Flat lookup used by the validator.
ALLOWED_CLASSES: frozenset[str] = frozenset().union(*CLASS_VOCABULARY.values())

# ── Helpers ──────────────────────────────────────────────────────────────────


def is_allowed_tag(name: str, *, is_images_field: bool) -> bool:
    """Return True when *name* (no angle brackets, no slash) may appear."""
    if name in ALLOWED_TAGS:
        return True
    return is_images_field and name in IMAGES_ONLY_TAGS


def allowed_attrs_for(tag: str) -> frozenset[str]:
    """Union of the catch-all and the tag-specific attribute sets."""
    return ALLOWED_ATTRS["*"] | ALLOWED_ATTRS.get(tag.lower(), frozenset())


def unknown_classes(class_attr: str) -> list[str]:
    """Split a ``class="..."`` value and return the tokens not in the vocabulary."""
    return [t for t in class_attr.split() if t and t not in ALLOWED_CLASSES]


# ── Card size and atomicity limits ───────────────────────────────────────────

BATCH_MAX_CARDS: int = 20
"""Hard ceiling of Anki cards per batch (mirrors ``checks.batch.MAX_BATCH``)."""

CLOZE_MAX_INDICES: int = 3
"""Operational compatibility cap on distinct cN indices in one Cloze note.

Each distinct index expands to its own card in Anki (one note != one card), and
index count alone does not prove atomicity (e.g. c1 can hide a list).
"""

QA_MAX_ITEMS: int = 2
"""Heuristic item ceiling in a QA answer before cardinality review (3+ items require unit review)."""


@dataclass(frozen=True)
class Limit:
    """Advisory review thresholds for one length measure (visible characters).

    ``error`` is kept for legacy API compatibility only; character length alone
    never emits CRITICAL and serves strictly as a review signal.
    """

    warn: int
    error: int


LIMITS: dict[str, Limit] = {
    "cloze_carrier": Limit(warn=220, error=320),  # Sentença (contexto + lacunas)
    "cloze_deletion": Limit(warn=60, error=80),  # uma lacuna atômica
    "qa_stem": Limit(warn=200, error=350),  # Enunciado
    "qa_answer": Limit(warn=240, error=400),  # Resposta
}


_TAG_RE = re.compile(r"<[^>]+>")


def _strip_cloze_markers(text: str) -> str:
    """Unwrap ``{{cN::target(::hint)?}}`` keeping *target*, supporting nested LaTeX braces."""
    if "{{c" not in text:
        return text
    out: list[str] = []
    n = len(text)
    i = 0
    while i < n:
        if i < n - 6 and text[i : i + 3] == "{{c" and text[i + 3].isdigit():
            j = i + 3
            while j < n and text[j].isdigit():
                j += 1
            if text[j : j + 2] == "::":
                content_start = j + 2
                k = content_start
                brace_depth = 0
                sep_pos: int | None = None
                closed = False
                while k < n - 1:
                    if text[k : k + 2] == "}}" and brace_depth == 0:
                        target_end = sep_pos if sep_pos is not None else k
                        out.append(text[content_start:target_end])
                        i = k + 2
                        closed = True
                        break
                    elif text[k : k + 2] == "::" and brace_depth == 0 and sep_pos is None:
                        sep_pos = k
                        k += 2
                        continue
                    elif text[k] == "{" and (k == 0 or text[k - 1] != "\\"):
                        brace_depth += 1
                    elif text[k] == "}" and (k == 0 or text[k - 1] != "\\"):
                        if brace_depth > 0:
                            brace_depth -= 1
                    k += 1
                if closed:
                    continue
        out.append(text[i])
        i += 1
    return "".join(out)


def visible_len(text: str) -> int:
    """Length of the text a reader actually sees (tags/entities/cloze markers removed)."""
    s = _strip_cloze_markers(text or "")
    s = re.sub(r"<br\s*/?>", " ", s, flags=re.IGNORECASE)
    s = _TAG_RE.sub("", s)
    s = html.unescape(s)
    return len(re.sub(r"\s+", " ", s).strip())


def make_diag(
    code: str,
    message: str,
    *,
    severity: str = "CRITICAL",
    row: int | None = None,
    field: str | None = None,
    suggestion: str | None = None,
):
    """Build a :class:`Diagnostic` for an atomicity rule (default CRITICAL)."""
    from anki_mcp.models import Diagnostic, Severity

    return Diagnostic(
        code=code,
        severity=Severity(severity),
        owner_skill="anki-card-generator",
        message=message,
        row=row,
        field=field,
        suggestion=suggestion,
    )


def limit_diag(
    code: str,
    message: str,
    *,
    limit: Limit,
    value: int,
    row: int | None = None,
    field: str | None = None,
    suggestion: str | None = None,
):
    """Return an advisory WARN :class:`Diagnostic` for a length breach, or None.

    Uses ``limit.warn`` and legacy ``limit.error`` thresholds for descriptive messaging,
    but never emits CRITICAL based on character count alone.
    """
    if value <= limit.warn:
        return None
    severe = value > limit.error
    threshold = limit.error if severe else limit.warn
    default_suggestion = (
        "Sinal de revisão: preserve o contexto necessário para tornar a resposta única "
        "e mova explicação opcional para Notas Adicionais."
    )
    return make_diag(
        code,
        f"{message} ({value} > {threshold} caracteres; sinal de revisão, não prova de múltipla cobrança)",
        severity="WARN",
        row=row,
        field=field,
        suggestion=suggestion or default_suggestion,
    )


def vocabulary_cheatsheet() -> str:
    """Compact, LLM-facing rendering of the vocabulary (for prompts/docs)."""
    lines = []
    for group, members in CLASS_VOCABULARY.items():
        lines.append(f"- {group}: " + ", ".join(f"`.{m}`" for m in sorted(members)))
    return "\n".join(lines)
