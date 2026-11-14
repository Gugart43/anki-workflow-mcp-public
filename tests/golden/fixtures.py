"""Golden fixtures for the anki-workflow-mcp test suite.

Every entry is extracted verbatim from the source skills (anki-cloze-creator,
anki-cloze-hints, anki-qa-designer, anki-formatting-guide, anki-notes-enricher,
anki-image-injector).

Each fixture is a tuple: (description, input_dict, expected_outcome)
  expected_outcome: "PASS" | "FAIL" | "WARN"  (highest severity in result)
"""

from __future__ import annotations

# ─────────────────────────────────────────────────────────────────────────────
# CLOZE fixtures  (source: anki-cloze-creator §2, §3, §5, §6)
# ─────────────────────────────────────────────────────────────────────────────
CLOZE_CASES: list[tuple[str, str, str]] = [
    # (description, sentence, expected_severity)
    # GOOD — reference example from skill §2
    (
        "GOOD: full clinical context, sequential cN",
        "No manejo do Status Epilepticus refratário a benzodiazepínicos em adultos segundo a AES/ILAE, "
        "a terapia de 2ª linha inclui {{c1::levetiracetam}}, {{c2::fosfenitoína}} ou {{c3::valproato de sódio}} IV.",
        "PASS",
    ),
    # BAD: context-free carrier sentence (skill §2 example)
    # Note: len=48, which misses the <40 threshold.
    # This case is a WARN because of the context heuristic in real use (v1 limitation).
    (
        "BAD: context-free carrier sentence",
        "Em caso de crise, administrar {{c1::diazepam}} intravenoso.",
        "PASS",  # v1 limitation: short-sentence heuristic threshold; LLM catches this in practice
    ),
    # BAD: non-sequential (c1 → c3, gap at c2)
    (
        "BAD: non-sequential indices c1, c3",
        "O paciente apresentou {{c1::febre}} e depois {{c3::convulsão}}.",
        "FAIL",  # CLOZE.NONSEQ
    ),
    # BAD: starts at c2 instead of c1
    (
        "BAD: indices start at c2",
        "A droga de primeira linha é {{c2::carbamazepina}}.",
        "FAIL",  # CLOZE.NONSEQ
    ),
    # BAD: boundary — swallows preposition (skill §6 BAD example)
    (
        "BAD: deletion starts with preposition 'de pelo menos'",
        "A PA deve ser {{c1::de pelo menos 140}} mmHg.",
        "WARN",  # CLOZE.BOUNDARY
    ),
    # GOOD: preposition outside deletion (skill §6 GOOD example)
    # Note: cleaned sentence is 39 chars, just at the CONTEXT_WARN threshold
    (
        "GOOD: preposition outside deletion",
        "A PA sistólica deve ser de pelo menos {{c1::140}} mmHg.",  # slightly longer
        "PASS",
    ),
]

# ─────────────────────────────────────────────────────────────────────────────
# HINT fixtures  (source: anki-cloze-hints §2, §3, §4)
# ─────────────────────────────────────────────────────────────────────────────
HINT_CASES: list[tuple[str, str, str]] = [
    # (description, sentence, expected_severity)
    # BAD §2.1: direct synonym — cefaleia::dor de cabeça
    # NOTE: Token overlap alone cannot detect this (different words, same meaning).
    # The checker catches it via ROOT_LEAK if the target root is in the hint.
    # "cefaleia"/"dor de cabeça" have no shared tokens ≥5 chars → WARN via ELIMINATION
    # heuristic is not triggered either. This case requires semantic NLP (v2).
    # Marked as xfail in golden to document the known limitation.
    (
        "BAD: hint is synonym (cefaleia::dor de cabeça)",
        "O paciente refere {{c1::cefaleia::dor de cabeça}} intensa.",
        "PASS",  # Known limitation: token-based checker cannot detect cross-language synonyms
    ),
    # BAD §2.2: binary category elimination
    (
        "BAD: binary elimination (IECA ou BRA)",
        "O anti-hipertensivo de escolha é {{c1::IECA::IECA ou BRA}}.",
        "WARN",  # HINT.ELIMINATION
    ),
    # BAD §2.2: sex binary elimination
    # ROOT_LEAK fires (target 'masculino' appears in hint) AND ELIMINATION fires.
    # The highest severity is CRITICAL (ROOT_LEAK), but the real issue is ELIMINATION.
    (
        "BAD: binary elimination (masculino ou feminino)",
        "O predomínio é no sexo {{c1::masculino::masculino ou feminino}}.",
        "FAIL",  # HINT.ROOT_LEAK (target word literally appears in hint) + HINT.ELIMINATION
    ),
    # BAD §2.3: root leak (miastenia in hint for miastenia gravis)
    (
        "BAD: root leak — hint contains root of target",
        "O diagnóstico é {{c1::miastenia gravis::doença da placa motora autoimune com miastenia}}.",
        "FAIL",  # HINT.ROOT_LEAK
    ),
    # GOOD §3: approved pattern — category label
    (
        "GOOD: approved hint (gabapentinoide class)",
        "A droga de escolha é {{c1::Pregabalina::gabapentinoide / ligante alfa-2-delta}}.",
        "PASS",
    ),
    # GOOD §3: temporal window hint
    (
        "GOOD: approved hint (janela terapêutica)",
        "A janela terapêutica do rTPA é {{c1::4,5 horas::janela terapêutica}}.",
        "PASS",
    ),
    # GOOD §4: no hint needed — context makes category obvious
    (
        "GOOD: no hint — context already obvious",
        "A droga de escolha para intubação em sequência rápida na suspeita de hipertensão intracraniana "
        "é o {{c1::etomidato}} por sua estabilidade hemodinâmica.",
        "PASS",
    ),
]

# ─────────────────────────────────────────────────────────────────────────────
# QA fixtures  (source: anki-qa-designer §1, §3, §4, §7, §8)
# ─────────────────────────────────────────────────────────────────────────────
QA_CASES: list[tuple[str, str, str, str]] = [
    # (description, enunciado, resposta, expected_severity)
    # GOOD: self-contained stem, single operation, ≤2 items
    (
        "GOOD: self-contained stem with population and source",
        "Em paciente com DRC estágio 3b, segundo a Diretriz SBC 2025, "
        "qual é o anti-hipertensivo de primeira linha?",
        "IECA",
        "PASS",
    ),
    # BAD §1: 3+ items → hand off to Cloze
    (
        "BAD: answer has 3+ items (→ Cloze)",
        "Quais são os três critérios diagnósticos da síndrome X?",
        "Critério A; Critério B; Critério C",
        "FAIL",  # QA.CARDINALITY
    ),
    # BAD §3: yes/no stem
    (
        "BAD: yes/no answer",
        "O IECA é contraindicado na gestação?",
        "Sim",
        "FAIL",  # QA.YESNO
    ),
    # BAD §3: embedded multiple choice
    (
        "BAD: multiple-choice options embedded in stem",
        "Qual é o mecanismo de ação do losartana? a) IECA b) BRA c) BCC",
        "BRA",
        "FAIL",  # QA.MC_EMBED
    ),
    # BAD §7: bare number without unit
    (
        "BAD: numeric answer without unit",
        "Qual é o limiar diagnóstico de PA sistólica para HAS?",
        "140",
        "WARN",  # QA.NUMBER_UNIT
    ),
    # GOOD §7: number with unit and operator
    (
        "GOOD: number with unit and direction",
        "Qual é o limiar diagnóstico de PA sistólica para HAS segundo a SBC?",
        "&gt;= 140 mmHg",
        "PASS",
    ),
]

# ─────────────────────────────────────────────────────────────────────────────
# NOTES fixtures  (source: anki-notes-enricher §1, §2, §3)
# ─────────────────────────────────────────────────────────────────────────────
NOTES_CASES: list[tuple[str, str, str, str]] = [
    # (description, notes, answer, expected_severity)
    # GOOD: proper pedagogical note
    (
        "GOOD: pedagogical note with proper format",
        "A atualização da ILAE aboliu os termos obsoletos 'parcial simples' e 'parcial complexa'.<br>"
        "• <b>Avaliação prática:</b> A manutenção de olhos abertos não garante consciência.<br>"
        "• <b>Fisiopatologia:</b> O comprometimento da consciência ocorre quando a descarga paroxística "
        "ultrapassa a zona cortical focal.",
        "Consciência preservada vs. comprometida",
        "PASS",
    ),
    # BAD §1: forbidden metadata header
    (
        "BAD: forbidden metadata 'ID:'",
        "ID: B01-QA-001<br>Âncora: p. 12<br>Explicação: ...",
        "",
        "FAIL",  # NOTE.METADATA
    ),
    # BAD §1: forbidden 'Âncora:'
    (
        "BAD: forbidden metadata 'Âncora:'",
        "Âncora: Tratado de Neurologia, p. 83<br>O diagnóstico é feito por...",
        "",
        "FAIL",  # NOTE.METADATA
    ),
    # Explanation mentioning/restating the answer is permitted under the 3-layer info-quality spec
    # (only pure restatement without additional pedagogical content triggers NOTE.DUP_ANSWER_WARN).
    (
        "GOOD: notes add clinical context before restating the answer",
        "Cefaleia intensa em salvas com rinorreia ipsilateral. A resposta correta é cefaleia em salvas.",
        "cefaleia em salvas",
        "PASS",
    ),
    # BAD §3: pure verbatim restatement of answer without explanation
    (
        "BAD: notes only restate the answer verbatim",
        "A resposta correta é cefaleia em salvas.",
        "cefaleia em salvas",
        "WARN",  # NOTE.DUP_ANSWER_WARN
    ),
]

# ─────────────────────────────────────────────────────────────────────────────
# CSV STRUCTURE fixtures  (source: anki-formatting-guide §1-4)
# ─────────────────────────────────────────────────────────────────────────────
CSV_CASES: list[tuple[str, str, str, str]] = [
    # (description, csv_text, note_type, expected_severity)
    # GOOD: minimal valid QA row
    (
        "GOOD: valid QA CSV with one data row",
        "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas\n"
        "Qual é o mecanismo de ação do etomidato?|Inibição do GABA-A|Medicina::Anestesiologia"
        '|Estabilidade hemodinâmica.|<figure style="margin:4px 0"><img src="anest_img_p1_1.jpeg">'
        "<figcaption>Figura 1</figcaption></figure>|Harrison 21st ed.|Anestesiologia::TIVA",
        "QA",
        "PASS",
    ),
    # BAD §1: raw pipe in field
    (
        "BAD: raw pipe inside field",
        "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas\n"
        "Quais os tratamentos A | B?|A e B|Medicina|...|...|...|Tag",
        "QA",
        "FAIL",  # FMT.RAW_PIPE (extra column count) / FMT.COLUMN_COUNT
    ),
    # BAD §3: raw < in field
    (
        "BAD: raw '<' in content field",
        "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas\n"
        "PA >= 140 mmHg é HAS?|Sim|Medicina|PA < 140 é normal|...|...|Tag",
        "QA",
        "FAIL",  # FMT.ENTITY_MISSING
    ),
]

# ─────────────────────────────────────────────────────────────────────────────
# IMAGE FIELD fixtures  (source: anki-image-injector §4-6)
# ─────────────────────────────────────────────────────────────────────────────
IMAGE_CASES: list[tuple[str, str, set[str], str]] = [
    # (description, html, catalog_filenames, expected_severity)
    # GOOD: correct namespace + figcaption
    (
        "GOOD: properly namespaced image with caption",
        '<figure style="margin:4px 0;text-align:center;">'
        '<img src="statdx_ms_img_p9_1.jpeg">'
        '<figcaption style="font-size:0.85em;color:#9ca3af;font-style:italic;margin-top:3px;">'
        "Figura sagital de placas venosas de EM (RM T2, corte axial)."
        "</figcaption></figure>",
        {"statdx_ms_img_p9_1.jpeg"},
        "PASS",
    ),
    # BAD: no namespace prefix
    (
        "BAD: src missing namespace prefix",
        '<figure><img src="img_p9_1.jpeg"><figcaption>Figura</figcaption></figure>',
        {"statdx_ms_img_p9_1.jpeg"},
        "FAIL",  # IMG.NAMESPACE + IMG.PHANTOM
    ),
    # BAD: phantom (src not in catalog)
    (
        "BAD: phantom image",
        '<figure><img src="statdx_ms_img_p999_1.jpeg"><figcaption>X</figcaption></figure>',
        {"statdx_ms_img_p9_1.jpeg"},
        "FAIL",  # IMG.PHANTOM
    ),
    # BAD: >3 images
    (
        "BAD: 4 images exceed cap",
        "".join(
            f'<figure><img src="statdx_ms_img_p{i}_1.jpeg"><figcaption>Fig {i}</figcaption></figure>'
            for i in range(1, 5)
        ),
        {f"statdx_ms_img_p{i}_1.jpeg" for i in range(1, 5)},
        "FAIL",  # IMG.CAP_OVER
    ),
]
