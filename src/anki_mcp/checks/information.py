"""Deterministic read-only 3-layer information profiler for QA and Cloze cards.

Distinguishes:
1. ``retrieved_target`` (recall load: QA answer or per-index Cloze deletion)
2. ``prompt_context`` (necessary context/cue in QA stem or Cloze carrier)
3. ``optional_explanation`` (``Notas Adicionais`` — optional, empty/short allowed)

Never emits quality scores, clinical approvals, or fake source verification.
"""

from __future__ import annotations

import re
from collections import defaultdict
from html.parser import HTMLParser
from typing import Any

from anki_mcp.checks.cloze import _find_cloze_spans

EDITORIAL_CHECKLIST: tuple[str, ...] = (
    "O que exatamente o estudante precisa recuperar para acertar?",
    "É uma unidade principal ou uma coleção de respostas independentes?",
    "Quais dados da frente são necessários para tornar essa resposta única?",
    "Qual informação nova e relacionada as notas acrescentam?",
    "Algum detalhe opcional foi indevidamente incorporado à resposta obrigatória?",
    "O verso está restrito à unidade estudada, com profundidade proporcional à dificuldade e à fonte?",
)

INFO_POLICY: dict[str, str] = {
    "three_layers": (
        "Separa carga recuperada (alvo), contexto necessário (frente/enunciado) "
        "e explicação opcional (Notas Adicionais)."
    ),
    "length_policy": (
        "Comprimento em caracteres é métrica descritiva e sinal de revisão; "
        "nunca bloqueia nem prova atomicidade ou múltipla cobrança."
    ),
    "notes_policy": (
        "Notas vazias ou curtas são permitidas sem volume mínimo; profundidade varia "
        "conforme dificuldade e fonte autorizada, mantendo-se na mesma unidade."
    ),
    "cloze_aggregation_policy": (
        "Em Cloze, cada índice cN gera um cartão separado com os demais índices visíveis; "
        "alvos de índices diferentes não são somados como carga de um único cartão."
    ),
}

BASE_LIMITATIONS: tuple[str, ...] = (
    "Não avalia semântica clínica, veracidade médica nem independência semântica dos itens.",
    "Não verifica fidelidade nem existência das afirmações na fonte bibliográfica.",
    "Não emite pontuação de qualidade nem aprovação clínica.",
)
_BASE_LIMITATIONS = BASE_LIMITATIONS

_BLOCK_BREAK_TAGS = frozenset(
    {
        "br",
        "hr",
        "p",
        "div",
        "li",
        "tr",
        "blockquote",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "ul",
        "ol",
    }
)
_SKIP_CONTENT_TAGS = frozenset({"script", "style"})


class _BlockTextParser(HTMLParser):
    """Safe stdlib HTMLParser preserving <li>/<br>/block breaks and decoding entities."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.chunks: list[str] = []
        self.li_count: int = 0
        self.br_count: int = 0
        self._skip_depth: int = 0

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        t = tag.lower()
        if t in _SKIP_CONTENT_TAGS:
            self._skip_depth += 1
            return
        if self._skip_depth:
            return
        if t == "li":
            self.li_count += 1
            self.chunks.append("\n")
        elif t == "br":
            self.br_count += 1
            self.chunks.append("\n")
        elif t in _BLOCK_BREAK_TAGS:
            self.chunks.append("\n")
        elif t in {"td", "th"}:
            self.chunks.append(" ")

    def handle_startendtag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        t = tag.lower()
        if self._skip_depth:
            return
        if t == "br":
            self.br_count += 1
            self.chunks.append("\n")
        elif t in _BLOCK_BREAK_TAGS:
            self.chunks.append("\n")

    def handle_endtag(self, tag: str) -> None:
        t = tag.lower()
        if t in _SKIP_CONTENT_TAGS:
            if self._skip_depth > 0:
                self._skip_depth -= 1
            return
        if self._skip_depth:
            return
        if t in _BLOCK_BREAK_TAGS:
            self.chunks.append("\n")
        elif t in {"td", "th"}:
            self.chunks.append(" ")

    def handle_data(self, data: str) -> None:
        if not self._skip_depth and data:
            self.chunks.append(data.replace("\xa0", " "))


def parse_html_text(raw: str) -> tuple[str, int, int]:
    """Return ``(line_preserved_text, li_count, br_count)`` using safe HTMLParser."""
    if not raw:
        return "", 0, 0
    parser = _BlockTextParser()
    parser.feed(raw)
    parser.close()
    joined = "".join(parser.chunks)
    lines = [re.sub(r"[ \t\r\f\v]+", " ", ln).strip() for ln in joined.splitlines()]
    cleaned = "\n".join(ln for ln in lines if ln)
    return cleaned, parser.li_count, parser.br_count


def html_to_visible_text(raw: str) -> str:
    """Return collapsed single-line visible text with HTML stripped and entities decoded."""
    text, _, _ = parse_html_text(raw)
    return re.sub(r"\s+", " ", text).strip()


def visible_char_count(raw: str) -> int:
    """Return length of decoded visible text."""
    return len(html_to_visible_text(raw))


def normalize_for_exact_match(raw: str) -> str:
    """Normalize text for exact repetition checks (HTML, entities, case, trailing punctuation)."""
    vis = html_to_visible_text(raw).casefold()
    vis = re.sub(r"^[•\-–—*\s]+", "", vis)
    vis = re.sub(r"[.!?…:;\s]+$", "", vis)
    return vis.strip()


_RESTATEMENT_PREFIX_RE = re.compile(
    r"^(?:a\s+resposta\s+(?:correta\s+)?(?:é|e)|resposta(?:\s+correta)?)\s*:?\s*",
    re.IGNORECASE,
)


def is_exact_answer_restatement(notes: str, answer: str) -> bool:
    """Return True iff *notes* only repeats or restates *answer* without adding explanation."""
    norm_answer = normalize_for_exact_match(answer)
    norm_notes = normalize_for_exact_match(notes)
    if not norm_answer or not norm_notes:
        return False
    if norm_notes == norm_answer:
        return True
    stripped = _RESTATEMENT_PREFIX_RE.sub("", norm_notes, count=1).strip()
    return bool(stripped and stripped == norm_answer and stripped != norm_notes)


_KNOWN_EXAM_LABELS = (
    r"USP|UNIFESP|UNICAMP|UNESP|IAMSPE|FCMSCSP|SUS-SP|ENARE|PSU-MG|PSU|AMRIGS|AMP|SURCE|"
    r"SES-DF|SES-PE|SES-RJ|UFRJ|UERJ|UFMG|UFPR|UFRGS|UFBA|UFC|UFPE|HCFMUSP|HC-FMUSP|"
    r"HC-UFMG|INCA|INTO|FHEMIG|SCMSP|FAMERP|FAMEMA|REVALIDA|ABN|TE-ABN|AMB|CFM|CBR|SBC|SBN"
)

_EXAM_QUALIFIER = (
    r"(?:"
    r"\s+d[eao]\s+resid[êe]ncia(?:\s+m[ée]dica)?"
    rf"|\s+(?:(?:d[aeo]|n[ao]|em)\s+)?(?-i:(?:{_KNOWN_EXAM_LABELS}))\b"
    r"|\s+(?:(?:d[aeo]|n[ao]|em)\s+)?(?:19|20)\d{2}(?:\s*[/\-]\s*(?:19|20)?\d{2,4})?"
    r")"
)

_EXAM_BOILERPLATE_RE = re.compile(
    r"^(?:(?:aten[çc][ãa]o|dica|nota|obs\.?|observa[çc][ãa]o|prova|alerta)\s*[:\-–—]\s*)?"
    r"(?:"
    r"(?:j[áa]\s+)?(?:foi\s+)?(?:caiu|cobrad[oa]|pedid[oa]|perguntad[oa]|apareceu|costuma\s+cair|cai(?:\s+(?:muito|sempre|frequentemente))?)"
    r"\s+(?:em|na|no|nas|nos)\s+(?:provas?|concursos?|resid[êe]ncia(?:\s+m[ée]dica)?|quest[õo]es|exames?|bancas?)"
    r"|(?:tema|assunto|conceito|t[óo]pico|detalhe|quest[ãa]o)\s+"
    r"(?:cl[áa]ssic[oa]|frequente|recorrente|quente|(?:muito\s+)?cobrad[oa])"
    r"\s+(?:em|na|no|nas|nos|de)\s+(?:provas?|concursos?|resid[êe]ncia(?:\s+m[ée]dica)?|bancas?|exames?)"
    r"|(?:muito\s+)?(?:cobrad[oa]|frequente|recorrente|cl[áa]ssic[oa])\s+(?:em|na|no|nas|nos)\s+(?:provas?|concursos?|resid[êe]ncia(?:\s+m[ée]dica)?|bancas?)"
    rf"|prova(?:\s+(?:d[eao]\s+)?(?:resid[êe]ncia(?:\s+m[ée]dica)?|(?-i:(?:{_KNOWN_EXAM_LABELS}))))?(?:\s+(?:de|em))?\s+(?:19|20)\d{{2}}"
    r")"
    rf"(?:{_EXAM_QUALIFIER}){{0,3}}$",
    re.IGNORECASE,
)


def is_exam_only_note(notes: str) -> bool:
    """Return True iff non-empty notes contain ONLY exam-appearance mentions without explanation."""
    text, _, _ = parse_html_text(notes)
    if not text:
        return False
    raw_segments = [s.strip() for s in re.split(r"[\n.!?]+", text) if s.strip()]
    if not raw_segments:
        return False
    for seg in raw_segments:
        cleaned = re.sub(r"^[•\-–—*\s]+", "", seg).strip()
        cleaned = re.sub(r"[,:;()\s]+$", "", cleaned).strip()
        if not cleaned:
            continue
        if not _EXAM_BOILERPLATE_RE.match(cleaned):
            return False
    return True


_APPOSITIVE_AFTER_COMMA_RE = re.compile(
    r"^(?:"
    r"caracterizad[oa]s?|definid[oa]s?|conhecid[oa]s?|denominad[oa]s?|chamad[oa]s?"
    r"|ou\s+seja|isto\s+[ée]|i\.e\.|e\.g\.|p\.ex\."
    r"|como\s+tamb[ée]m|bem\s+como|assim\s+como"
    r"|manifestando-se|apresentando-se|cursando\s+com|podendo"
    r"|que\s+|o\s+qual|a\s+qual|os\s+quais|as\s+quais|cuj[oa]s?|onde|na\s+qual|no\s+qual"
    r"|com\s+(?:incapacidade|aus[êe]ncia|presen[çc]a|dura[çc][ãa]o|predom[íi]nio|exce[çc][ãa]o|risco|desvio|melhora|piora)"
    r"|sem\s+|principalmente|especialmente|sobretudo|geralmente|tipicamente|frequentemente|habitualmente|usualmente"
    r"|em\s+geral|em\s+especial|ap[óo]s|devido\s+a|associad[oa]s?\s+a|secund[áa]ri[oa]s?\s+a"
    r"|pois\s+|porque\s+|visto\s+que|embora\s+|por[ée]m\b|contudo\b|entretanto\b|todavia\b|mas\b"
    r"|incluindo\b|exceto\b|salvo\b|inclusive\b|segundo\b|conforme\b"
    r")\b",
    re.IGNORECASE,
)

_DOSE_REGIMEN_AFTER_COMMA_RE = re.compile(
    r"^(?:"
    r"(?:VO|IV|IM|SC|EV|SL|VR|ID|inalat[óo]rio|t[óo]pico|oral|intravenos[oa]|subcut[âa]ne[oa])\b"
    r"|\d+x\s*/\s*(?:dia|semana|m[êe]s)"
    r"|a\s+cada\s+\d+"
    r"|(?:de\s+)?\d+\s*/\s*\d+\s*h\b"
    r"|em\s+(?:bolus|dose\s+[úu]nica|doses?\s+divididas?|infus[ãa]o)\b"
    r"|(?:bolus|dose\s+[úu]nica|m[áa]ximo|m[íi]nimo)\b|at[ée]\s+\d+"
    r"|por\s+\d+\s+(?:dias?|horas?|semanas?|meses?)"
    r"|durante\s+\d+|se\s+necess[áa]rio|dividid[oa]\s+em|dilu[íi]d[oa]"
    r"|(?:mg|mcg|ug|g|kg|ml|mL|L|UI|mEq|mmol)\b"
    r")",
    re.IGNORECASE,
)

_COMPARISON_RE = re.compile(r"\b(?:vs\.?|versus)\b", re.IGNORECASE)
_NUMBERED_INLINE_RE = re.compile(
    r"(?:^|\s)(?:\(\d{1,2}\)|(?<!\d)\d{1,2}\)|\d{1,2}\.(?=\s+[A-Za-zÀ-ÖØ-öø-ÿ]))\s+"
)
_BULLET_INLINE_RE = re.compile(r"(?:^|\s)[•]\s+")


def _split_top_level(text: str, sep_char: str) -> list[str]:
    """Split on ``sep_char`` only outside (), [], LaTeX \\(\\) / \\[\\], and appositive clauses."""
    parts: list[str] = []
    buf: list[str] = []
    paren = 0
    bracket = 0
    in_appositive = False
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
        if ch == "\\" and i + 1 < n and text[i + 1] in "()[]":
            nxt = text[i + 1]
            if nxt in "([":
                paren += 1
            else:
                paren = max(0, paren - 1)
            buf.append(ch)
            buf.append(nxt)
            i += 2
            continue
        if ch == "(":
            paren += 1
        elif ch == ")" and paren > 0:
            paren -= 1
        elif ch == "[":
            bracket += 1
        elif ch == "]" and bracket > 0:
            bracket -= 1
        elif ch == sep_char and paren == 0 and bracket == 0:
            if sep_char == ",":
                if in_appositive:
                    buf.append(ch)
                    i += 1
                    continue
                prev_ch = text[i - 1] if i > 0 else ""
                next_ch = text[i + 1] if i + 1 < n else ""
                rest = text[i + 1 :].lstrip()
                is_decimal = prev_ch.isdigit() and next_ch.isdigit()
                is_app = bool(_APPOSITIVE_AFTER_COMMA_RE.match(rest))
                is_dose = bool(_DOSE_REGIMEN_AFTER_COMMA_RE.match(rest))
                if is_app:
                    in_appositive = True
                if is_decimal or is_app or is_dose:
                    buf.append(ch)
                    i += 1
                    continue
            parts.append("".join(buf).strip())
            buf = []
            i += 1
            continue
        buf.append(ch)
        i += 1
    parts.append("".join(buf).strip())
    return [p for p in parts if p]


def _split_trailing_conjunction(items: list[str]) -> tuple[list[str], bool]:
    """When comma enumeration ends with 'B e C' or 'B and C', split the last element."""
    if len(items) < 2:
        return items, False
    last = items[-1]
    m = re.match(r"^(.+?)\s+(?:e|and)\s+(.+)$", last, re.IGNORECASE)
    if m and m.group(1).strip() and m.group(2).strip():
        return [*items[:-1], m.group(1).strip(), m.group(2).strip()], True
    return items, False


def extract_candidate_items(raw_html: str) -> tuple[list[str], list[str]]:
    """Extract candidate recalled items and structural/punctuation evidence deterministically."""
    text, li_count, br_count = parse_html_text(raw_html)
    if not text:
        return [], []

    evidence: list[str] = []
    lines = [ln.strip() for ln in text.splitlines() if ln.strip()]

    if li_count >= 2 and len(lines) >= 2:
        evidence.append("html_li")
    elif br_count >= 1 and len(lines) >= 2:
        evidence.append("html_br")
    elif len(lines) >= 2:
        evidence.append("newline")

    # Stage 1: split each line by top-level semicolons
    stage1: list[str] = []
    for ln in lines:
        semi_parts = _split_top_level(ln, ";")
        if len(semi_parts) >= 2:
            if "semicolon" not in evidence:
                evidence.append("semicolon")
            stage1.extend(semi_parts)
        else:
            stage1.append(ln)

    # Stage 2: inline bullet / numbered markers
    stage2: list[str] = []
    for seg in stage1:
        if _BULLET_INLINE_RE.search(seg):
            parts = [p.strip() for p in _BULLET_INLINE_RE.split(seg) if p.strip()]
            if len(parts) >= 2:
                if "bullet_marker" not in evidence:
                    evidence.append("bullet_marker")
                stage2.extend(parts)
                continue
        if _NUMBERED_INLINE_RE.search(seg):
            parts = [p.strip() for p in _NUMBERED_INLINE_RE.split(seg) if p.strip()]
            if len(parts) >= 2:
                if "numbered_list" not in evidence:
                    evidence.append("numbered_list")
                stage2.extend(parts)
                continue
        stage2.append(seg)

    # Stage 3: safe top-level commas (skipping comparisons like 'A vs. B' and dose/appositive/decimal)
    if len(stage2) == 1 and not _COMPARISON_RE.search(stage2[0]):
        comma_parts = _split_top_level(stage2[0], ",")
        if len(comma_parts) >= 2:
            comma_parts, had_conj = _split_trailing_conjunction(comma_parts)
            evidence.append("comma_enumeration")
            if had_conj:
                evidence.append("conjunction_e")
            stage2 = comma_parts

    cleaned = [re.sub(r"^(?:[•\-–—*]|\(\d+\)|\d+\))\s*", "", item).strip() for item in stage2]
    cleaned = [c for c in cleaned if c]
    if not cleaned:
        cleaned = [html_to_visible_text(raw_html)]
    return cleaned, evidence


def _strip_cloze_markup(sentence: str) -> str:
    """Strip ``{{cN::target(::hint)?}}`` wrappers leaving visible unmasked carrier text."""
    spans = _find_cloze_spans(sentence)
    if not spans:
        return sentence
    out: list[str] = []
    pos = 0
    for _idx, target, _hint, start, end in spans:
        out.append(sentence[pos:start])
        out.append(target)
        pos = end
    out.append(sentence[pos:])
    return "".join(out)


def _render_cloze_context_for_index(sentence: str, target_idx: int) -> str:
    """Render carrier sentence as seen by card ``c{target_idx}`` (other indices visible)."""
    spans = _find_cloze_spans(sentence)
    if not spans:
        return sentence
    out: list[str] = []
    pos = 0
    for idx, target, hint, start, end in spans:
        out.append(sentence[pos:start])
        if idx == target_idx:
            out.append(f"[{hint}]" if hint else "[...]")
        else:
            out.append(target)
        pos = end
    out.append(sentence[pos:])
    return "".join(out)


def profile_information(
    note_type: str = "QA",
    prompt: str = "",
    answer: str = "",
    notes: str = "",
    *,
    prompt_available: bool = True,
) -> dict[str, Any]:
    """Build a deterministic, JSON-serializable 3-layer information profile."""
    nt = note_type.strip().upper()
    if nt == "CLOZE":
        nt = "CL"
    if nt not in {"QA", "CL"}:
        raise ValueError(f"Unsupported note_type {note_type!r}; expected 'QA' or 'CL'.")

    advisory_signals: list[dict[str, Any]] = []
    limitations = list(_BASE_LIMITATIONS)

    if not prompt_available:
        limitations.append(
            "Enunciado/sentença não fornecido nesta chamada; o contexto necessário da frente "
            "e a relação pergunta-resposta não puderam ser avaliados."
        )
        advisory_signals.append(
            {
                "code": "INFO.PROMPT_UNAVAILABLE",
                "message": "Perfil parcial: apenas Notas Adicionais (e resposta opcional) disponíveis sem enunciado.",
            }
        )

    notes_chars = visible_char_count(notes)
    exam_only = is_exam_only_note(notes) if notes.strip() else False

    if nt == "QA":
        prompt_chars = visible_char_count(prompt) if prompt_available else 0
        items, evidence = extract_candidate_items(answer)
        ans_chars = visible_char_count(answer)
        hidden_list = len(items) >= 3
        if hidden_list:
            advisory_signals.append(
                {
                    "code": "INFO.QA_HIDDEN_LIST",
                    "candidate_item_count": len(items),
                    "visible_chars": ans_chars,
                    "evidence": evidence,
                    "message": (
                        f"Resposta QA apresenta {len(items)} itens candidatos ({ans_chars} caracteres); "
                        "sinal advisory — não avalia independência semântica."
                    ),
                }
            )
        exact_dup = is_exact_answer_restatement(notes, answer)
        retrieved_layer: dict[str, Any] = {
            "role": "retrieved_recall_load",
            "visible_chars": ans_chars,
            "candidate_items": items,
            "candidate_item_count": len(items),
            "evidence": evidence,
            "hidden_list_signal": hidden_list,
        }
    else:
        spans = _find_cloze_spans(prompt) if prompt_available else []
        prompt_chars = visible_char_count(_strip_cloze_markup(prompt)) if prompt_available else 0
        if prompt_available and not spans:
            limitations.append("Nenhuma omissão {{cN::...}} detectada na sentença fornecida.")

        by_index: dict[int, list[tuple[str, str | None]]] = defaultdict(list)
        for idx, target, hint, _start, _end in spans:
            by_index[idx].append((target, hint))

        per_index: list[dict[str, Any]] = []
        distinct_indices = sorted(by_index.keys())
        exact_dup_indices: list[int] = []

        for idx in distinct_indices:
            entries = by_index[idx]
            combined_target = (
                " ; ".join(t for t, _ in entries) if len(entries) > 1 else entries[0][0]
            )
            target_chars = sum(visible_char_count(t) for t, _ in entries)
            hints = [h for _, h in entries if h]
            hint_chars = sum(visible_char_count(h) for h in hints) if hints else None

            if len(entries) == 1:
                items, evidence = extract_candidate_items(entries[0][0])
            else:
                items = []
                evidence = ["duplicate_index_spans"]
                for t, _ in entries:
                    sub_items, sub_ev = extract_candidate_items(t)
                    items.extend(sub_items)
                    for e in sub_ev:
                        if e not in evidence:
                            evidence.append(e)

            ctx_rendered = _render_cloze_context_for_index(prompt, idx)
            ctx_chars = visible_char_count(ctx_rendered)
            hidden_list = len(items) >= 3

            if hidden_list:
                advisory_signals.append(
                    {
                        "code": "INFO.CLOZE_HIDDEN_LIST",
                        "index": idx,
                        "card": f"c{idx}",
                        "candidate_item_count": len(items),
                        "visible_chars": target_chars,
                        "evidence": evidence,
                        "message": (
                            f"c{idx} contém possível lista oculta ({len(items)} itens candidatos em "
                            f"{target_chars} caracteres); sinal advisory independente de tamanho — "
                            "não avalia independência semântica."
                        ),
                    }
                )

            # Advisory-only per-index restatement check (batch check_notes receives answer="" for CL).
            if is_exact_answer_restatement(notes, combined_target):
                exact_dup_indices.append(idx)

            per_index.append(
                {
                    "index": idx,
                    "card": f"c{idx}",
                    "visible_chars": target_chars,
                    "visible_context_chars": ctx_chars,
                    "hint_visible_chars": hint_chars,
                    "candidate_items": items,
                    "candidate_item_count": len(items),
                    "evidence": evidence,
                    "hidden_list_signal": hidden_list,
                }
            )

        exact_dup = bool(exact_dup_indices)
        retrieved_layer = {
            "role": "retrieved_recall_load",
            "distinct_indices": distinct_indices,
            "card_count": len(distinct_indices),
            "per_index": per_index,
            "max_target_visible_chars": max((p["visible_chars"] for p in per_index), default=0),
            "note_on_aggregation": (
                "Por índice: alvos de cN diferentes não são somados como carga de um único cartão; "
                "os demais índices permanecem visíveis no contexto."
            ),
        }

    if exact_dup:
        advisory_signals.append(
            {
                "code": "NOTE.DUP_ANSWER_WARN",
                "message": "Notas Adicionais reproduzem exatamente o alvo normalizado sem acrescentar conteúdo.",
            }
        )
    if exam_only:
        advisory_signals.append(
            {
                "code": "NOTE.EXAM_ONLY_WARN",
                "message": "Notas Adicionais contêm apenas menção a prova/concurso sem explicação pedagógica.",
            }
        )

    return {
        "note_type": nt,
        "prompt_available": prompt_available,
        "descriptive_only": True,
        "semantic_review_required": True,
        "layers": {
            "prompt_context": {
                "role": "necessary_context_and_cue",
                "available": prompt_available,
                "visible_chars": prompt_chars,
            },
            "retrieved_target": retrieved_layer,
            "optional_explanation": {
                "role": "optional_explanation",
                "visible_chars": notes_chars,
                "is_empty": notes_chars == 0,
                "empty_allowed": True,
                "exact_answer_repetition": exact_dup,
                "exam_only_mention": exam_only,
            },
        },
        "advisory_signals": advisory_signals,
        "editorial_checklist": list(EDITORIAL_CHECKLIST),
        "policy": dict(INFO_POLICY),
        "limitations": limitations,
    }


def compact_information_profiles(
    profiles: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    """Hoist shared policy/checklist/base limitations once for compact batch payloads."""
    base_set = set(BASE_LIMITATIONS)
    compact_rows: list[dict[str, Any]] = []
    for p in profiles:
        row_entry: dict[str, Any] = {
            "note_type": p["note_type"],
            "prompt_available": p["prompt_available"],
            "descriptive_only": p["descriptive_only"],
            "semantic_review_required": p["semantic_review_required"],
            "layers": p["layers"],
            "advisory_signals": p["advisory_signals"],
        }
        if "row" in p:
            row_entry["row"] = p["row"]
        row_lims = [lim for lim in p.get("limitations", ()) if lim not in base_set]
        if row_lims:
            row_entry["limitations"] = row_lims
        compact_rows.append(row_entry)

    guidance = {
        "editorial_checklist": list(EDITORIAL_CHECKLIST),
        "policy": dict(INFO_POLICY),
        "limitations": list(BASE_LIMITATIONS),
    }
    return compact_rows, guidance
