"""checks/qa.py — anki-qa-designer implementation.

Checks:
  QA.CARDINALITY  — answer contains 3+ atomic items (→ hand off to Cloze)
  QA.YESNO        — yes/no question or single-word Sim/Não answer
  QA.MC_EMBED     — multiple-choice options embedded in the stem
  QA.CONTEXT_WARN — stem lacks clinical scenario/population/source framing (WARN)
  QA.NUMBER_UNIT  — numeric answer without unit or direction operator (WARN)
  QA.MULTI_OP     — stem appears to request two independent retrievals (WARN)
"""

from __future__ import annotations

import re

from anki_mcp.checks.information import extract_candidate_items
from anki_mcp.core import vocab
from anki_mcp.models import Diagnostic

# Patterns for 3+ item detection in the answer
_LIST_SEPARATOR_RE = re.compile(r"(?:;\s*|\n|<br\s*/?>|\d+\)\s*|•\s*)")
# Comma split: skip HTML entity boundaries (&le; &gt; etc.) and "vs." comparisons
_COMMA_SPLIT_RE = re.compile(r",\s*(?:e\s+|and\s+)?(?!(?:\d|\s*\d))")
# HTML entity boundaries that the list separator should NOT split at
_ENTITY_BOUNDARY_RE = re.compile(r"&\w+;")

# Yes/No patterns
_YESNO_STEM_RE = re.compile(
    # Match only when the word introduces a binary judgment at the START or as
    # the main predicate — not when embedded inside a "qual X confirma Y?" stem.
    # Anchored more tightly: must be a standalone predicate, not a relative clause.
    r"^\s*(?:é\s+verdadeiro|é\s+falso|é\s+correto|é\s+incorreto|concordo|discordo"
    r"|é\s+contraindicado)\b"
    r"|^.*\b(?:confirma[mr]?\s*(?:ou|e)\s|is\s+(?:true|false|correct|indicated))\b",
    re.IGNORECASE,
)
_YESNO_ANSWER_RE = re.compile(
    r"^\s*(sim|não|yes|no|verdadeiro|falso|correto|incorreto)\s*$",
    re.IGNORECASE,
)

# MC embed: "a) ... b) ... c)" or "(a) (b)" style (with or without opening paren)
_MC_RE = re.compile(
    r"[(\[]\s*[a-dA-D]\s*[)\]]"  # (a), [a]
    r"|\ba\s*\)\s*\S+.{0,40}\bb\s*\)",  # a) X ... b) Y
    re.DOTALL,
)

# Context anchors: any reference to patient, guideline, condition
_CONTEXT_MARKERS_RE = re.compile(
    r"\b(paciente|patient|segundo|conforme|diretriz|guideline|critério|criteria"
    r"|portador|com\s+(diagnóstico|suspeita)|população|adulto|criança|pediátrico"
    r"|gestante|idoso|IRC|DRC|ICC|ELA|EM|AVC|SNC)\b",
    re.IGNORECASE,
)

# Number with mandatory unit check
_BARE_NUMBER_RE = re.compile(
    r"\b\d+(?:[.,]\d+)?\b(?!\s*(?:mg|mcg|g|kg|mL|L|mmHg|%|UI|U|°C|anos|dias|horas|h\b|m\b))"
)
_DIRECTION_OP = re.compile(r"[><≥≤=]|&[gl]t;|&[gl]e;")
# Appositive clause indicators — a comma followed by one of these words starts
# an explanation of the SAME item, not a new item. E.g. "Glossoplegia, caracterizada por..."
_APPOSITIVE_RE = re.compile(
    r",\s*(?:caracterizada?|definida?|conhecida?|denominada?|ou\s+seja|i\.e\.|como\s+também"
    r"|manifestando-se|podendo|que\s+se|cuja|cujo|onde|na\s+qual|com\s+(?:incapacidade|ausência|presença))\b",
    re.IGNORECASE,
)


def _count_items(answer: str) -> int:
    """Heuristic item count in an answer field.

    Structural separators (semicolons, bullets, <br>) split clearly.
    Comma splits are restricted to coordinated noun phrases — appositive
    or explanatory clauses following a single noun are NOT counted as
    separate items. HTML entities (&le; &gt; etc.) in the middle of an
    expression are also protected from being split at adjacent commas.
    """
    # Temporarily replace HTML entities so they don't get split at adjacent commas
    protected = _ENTITY_BOUNDARY_RE.sub("ENTITY", answer)
    # Strip appositive continuations before counting commas
    collapsed = _APPOSITIVE_RE.sub("", protected)

    # First try structural separators (semicolons, bullets, numbered lists)
    parts = _LIST_SEPARATOR_RE.split(collapsed)
    parts = [p.strip() for p in parts if p.strip()]
    if len(parts) >= 3:
        return len(parts)
    # Then try comma enumeration on the collapsed form
    parts2 = _COMMA_SPLIT_RE.split(collapsed)
    parts2 = [p.strip() for p in parts2 if p.strip()]
    return len(parts2)


def check_qa(
    enunciado: str,
    resposta: str,
    row: int | None = None,
) -> list[Diagnostic]:
    """Check a QA card (stem + answer pair)."""
    diags: list[Diagnostic] = []

    # ── QA.ID_IN_TEXT ────────────────────────────────────────────────────────
    if re.search(r"^\s*[\(\[]?(?:ANKI-\d+|B\d+-[A-Z]+-\d+)[\)\]]?", enunciado):
        diags.append(
            Diagnostic.from_code(
                "QA.ID_IN_TEXT",
                "Visible card ID found in question stem. IDs must reside in tags or metadata, not in the prompt.",
                row=row,
                field="Enunciado",
            )
        )

    # ── QA.YESNO ─────────────────────────────────────────────────────────────
    if _YESNO_ANSWER_RE.match(resposta):
        diags.append(
            Diagnostic.from_code(
                "QA.YESNO",
                f"Answer is a yes/no value: '{resposta.strip()}'",
                row=row,
                field="Resposta",
                suggestion="Rephrase as a direct-recall question with a factual answer",
            )
        )
    if _YESNO_STEM_RE.search(enunciado):
        diags.append(
            Diagnostic.from_code(
                "QA.YESNO",
                "Stem appears to be a true/false or yes/no question",
                row=row,
                field="Enunciado",
            )
        )

    # ── QA.MC_EMBED ───────────────────────────────────────────────────────────
    if _MC_RE.search(enunciado):
        diags.append(
            Diagnostic.from_code(
                "QA.MC_EMBED",
                "Stem contains embedded multiple-choice options (a)/b)/c) pattern)",
                row=row,
                field="Enunciado",
                suggestion="Remove options — the stem must be a free-recall question",
            )
        )

    # ── QA.CARDINALITY ────────────────────────────────────────────────────────
    # Shared structural candidate extraction (keeps triage _count_items untouched).
    candidate_items, _ = extract_candidate_items(resposta)
    item_count = len(candidate_items)
    if item_count >= 3:
        diags.append(
            Diagnostic.from_code(
                "QA.CARDINALITY",
                f"Answer appears to enumerate ~{item_count} items (≥3) — review unit granularity",
                row=row,
                field="Resposta",
                suggestion=(
                    "Revise a unidade: separe recuperações independentes em cartões próprios "
                    "ou mova detalhes opcionais para Notas Adicionais (uma lacuna Cloze única pode ocultar lista)."
                ),
            )
        )

    # ── QA.CONTEXT_WARN ──────────────────────────────────────────────────────
    if not _CONTEXT_MARKERS_RE.search(enunciado):
        diags.append(
            Diagnostic.from_code(
                "QA.CONTEXT_WARN",
                "Stem lacks explicit clinical context (patient/guideline/condition/population)",
                row=row,
                field="Enunciado",
                suggestion="Add: population, source guideline, or clinical scenario",
            )
        )

    # ── QA.NUMBER_UNIT ────────────────────────────────────────────────────────
    # Only warn when answer looks numeric but lacks unit or comparison
    # Strip alphanumeric identifiers (e.g. CYP2D6, VMAT2, 4p16.3, HLA-B27) and ordinals/classes (tipo 2, fase 1, classe 3)
    cleaned_num = re.sub(
        r"\b(?:tipo|fase|classe|est[áa]gio|grau)\s+\d+\b", "", resposta, flags=re.IGNORECASE
    )
    cleaned_num = re.sub(r"\b[A-Za-z]+\d+[A-Za-z\d]*|\b\d+[pq]\d+(?:\.\d+)?\b", "", cleaned_num)
    if re.search(r"\d", cleaned_num) and not _DIRECTION_OP.search(resposta):
        if _BARE_NUMBER_RE.search(cleaned_num):
            diags.append(
                Diagnostic.from_code(
                    "QA.NUMBER_UNIT",
                    "Answer contains a bare number without unit or direction (≥/≤/>/< + unit)",
                    row=row,
                    field="Resposta",
                    suggestion="e.g. '≥ 140 mmHg' not '140'",
                )
            )

    # ── CARD.SPLIT_QA_MULTI_QUESTION (one obligation per stem) ───────────────
    stem_text = re.sub(r"<[^>]+>", " ", enunciado)
    question_marks = stem_text.count("?")
    coordinated_q = re.search(
        r"\b(?:e|ou)\s+(?:qual|quais|quando|onde|como|por\s+que)\b", stem_text, re.IGNORECASE
    )
    if question_marks > 1 or coordinated_q:
        diags.append(
            vocab.make_diag(
                "CARD.SPLIT_QA_MULTI_QUESTION",
                "Enunciado com mais de uma pergunta: o cartão cobra duas obrigações",
                severity="WARN",
                row=row,
                field="Enunciado",
                suggestion=(
                    "SPLIT: uma recuperação por cartão; separe perguntas independentes "
                    "e mova detalhes opcionais para Notas Adicionais."
                ),
            )
        )

    # ── CARD.TOO_LONG_STEM / CARD.TOO_LONG_ANSWER (advisory length signal) ───
    for code, text, limit_key, field, sugg in (
        (
            "CARD.TOO_LONG_STEM",
            enunciado,
            "qa_stem",
            "Enunciado",
            "Revise se há duas cobranças independentes; preserve o contexto necessário para tornar a resposta única e mova explicação opcional para Notas Adicionais.",
        ),
        (
            "CARD.TOO_LONG_ANSWER",
            resposta,
            "qa_answer",
            "Resposta",
            "Revise se a resposta agrupa itens independentes ou detalhe opcional; preserve o contexto necessário e mova explicação opcional para Notas Adicionais.",
        ),
    ):
        diag = vocab.limit_diag(
            code,
            f"{field} extenso",
            limit=vocab.LIMITS[limit_key],
            value=vocab.visible_len(text),
            row=row,
            field=field,
            suggestion=sugg,
        )
        if diag is not None:
            diags.append(diag)

    return diags
