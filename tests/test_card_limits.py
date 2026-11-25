"""Atomicity (one obligation per card) and per-field size limits.

Limits live in ``core/vocab.py`` (``CLOZE_MAX_INDICES``, ``QA_MAX_ITEMS``,
``LIMITS``). These tests pin behaviour at and around the thresholds, and check
that the stable codes are emitted with the documented severity.
"""

from __future__ import annotations

import pytest

from anki_mcp.checks import cloze as cloze_mod
from anki_mcp.checks import notes as notes_mod
from anki_mcp.checks import qa as qa_mod
from anki_mcp.core import vocab
from anki_mcp.models import Severity

CARRIER = "No manejo do status epilepticus refratário em adultos, segundo a diretriz de 2020, "


def _codes(diags, prefix="CARD."):
    return {(d.code, d.severity) for d in diags if d.code.startswith(prefix)}


# ── Cloze: indices, carrier, deletion ─────────────────────────────────────────


def test_cloze_three_indices_is_allowed():
    s = (
        CARRIER
        + "a 2ª linha inclui {{c1::levetiracetam}}, {{c2::fosfenitoína}} ou {{c3::valproato}} IV."
    )
    assert ("CARD.SPLIT_CLOZE_INDICES", Severity.CRITICAL) not in _codes(cloze_mod.check_cloze(s))


def test_cloze_four_indices_must_split_critical():
    s = CARRIER + "a 2ª linha inclui {{c1::a}}, {{c2::b}}, {{c3::c}} e {{c4::d}} IV."
    diags = cloze_mod.check_cloze(s)
    assert ("CARD.SPLIT_CLOZE_INDICES", Severity.CRITICAL) in _codes(diags)
    msg = next(d for d in diags if d.code == "CARD.SPLIT_CLOZE_INDICES")
    assert "SPLIT" in (msg.suggestion or "")
    assert "não prova atomicidade" in msg.message


def test_cloze_carrier_warn_never_critical_by_length():
    """Comprimento de sentença é apenas sinal de revisão (WARN), nunca bloqueio CRITICAL."""
    short = CARRIER + "a 2ª linha inclui {{c1::levetiracetam}} IV."
    assert len(short) < vocab.LIMITS["cloze_carrier"].warn
    assert "CARD.TOO_LONG_SENTENCE" not in {d.code for d in cloze_mod.check_cloze(short)}

    warn = (
        CARRIER
        + "x" * (vocab.LIMITS["cloze_carrier"].warn - len(CARRIER) + 5)
        + " {{c1::levetiracetam}}"
    )
    got = {
        (d.code, d.severity)
        for d in cloze_mod.check_cloze(warn)
        if d.code == "CARD.TOO_LONG_SENTENCE"
    }
    assert got == {("CARD.TOO_LONG_SENTENCE", Severity.WARN)}

    big = CARRIER + "x" * (vocab.LIMITS["cloze_carrier"].error + 10) + " {{c1::levetiracetam}}"
    big_diags = [d for d in cloze_mod.check_cloze(big) if d.code == "CARD.TOO_LONG_SENTENCE"]
    assert [(d.code, d.severity) for d in big_diags] == [("CARD.TOO_LONG_SENTENCE", Severity.WARN)]
    assert "sinal de revisão, não prova de múltipla cobrança" in big_diags[0].message
    assert "contexto necessário" in (big_diags[0].suggestion or "")


def test_cloze_deletion_over_error_is_warn_never_critical():
    """Deleção longa emite WARN (sinal de revisão), nunca CRITICAL por contagem de caracteres."""
    target = "a " + "levetiracetam em dose de ataque seguida de manutenção por via intravenosa " * 2
    s = CARRIER + "inclui {{c1::" + target.strip() + "}}."
    diags = [d for d in cloze_mod.check_cloze(s) if d.code == "CARD.TOO_LONG_DELETION"]
    assert diags and diags[0].severity == Severity.WARN
    assert "sinal de revisão, não prova de múltipla cobrança" in diags[0].message
    too_long = [d for d in cloze_mod.check_cloze(s) if d.code == "CLOZE.TOO_LONG"]
    assert too_long and too_long[0].severity == Severity.WARN
    assert "atomic" not in too_long[0].message.lower() or "não prova" in too_long[0].message.lower()


def test_cloze_deletion_counts_visible_text_not_markup():
    # 60 visible chars wrapped in tags + cloze hint should not trip the limit.
    target = "<b>" + "x" * 50 + "</b>"
    s = CARRIER + "inclui {{c1::" + target + "::dica}}."
    assert "CARD.TOO_LONG_DELETION" not in {d.code for d in cloze_mod.check_cloze(s)}


# ── QA: stem, answer, one question ────────────────────────────────────────────


def test_qa_two_questions_is_a_split_warning():
    diags = qa_mod.check_qa("Qual o tratamento? E qual a dose inicial?", "10 mg/dia")
    assert ("CARD.SPLIT_QA_MULTI_QUESTION", Severity.WARN) in _codes(diags)


def test_qa_single_question_is_clean_of_split():
    diags = qa_mod.check_qa(
        "Em adulto com coreia, qual o fármaco de primeira linha?", "Tetrabenazina"
    )
    assert "CARD.SPLIT_QA_MULTI_QUESTION" not in {d.code for d in diags}


def test_qa_stem_over_error_is_warn_never_critical():
    """Enunciado extenso pode ser contexto necessário; emite apenas WARN de revisão."""
    stem = "Em paciente adulto com doença de Huntington em fase inicial " + "x " * 180 + "qual?"
    diags = qa_mod.check_qa(stem, "resposta")
    assert ("CARD.TOO_LONG_STEM", Severity.WARN) in _codes(diags)
    assert ("CARD.TOO_LONG_STEM", Severity.CRITICAL) not in _codes(diags)
    stem_diag = next(d for d in diags if d.code == "CARD.TOO_LONG_STEM")
    assert "sinal de revisão, não prova de múltipla cobrança" in stem_diag.message
    assert "contexto necessário" in (stem_diag.suggestion or "")


def test_qa_cardinality_real_stays_critical_without_auto_cloze_conversion():
    """QA.CARDINALITY (>=3 itens) permanece CRITICAL, orientando revisão da unidade sem auto-Cloze."""
    for ans in (
        "Item alfa; Item beta; Item gama",
        "<ul><li>alfa</li><li>beta</li><li>gama</li></ul>",
    ):
        diags = qa_mod.check_qa(
            "Em paciente adulto, quais os achados da tríade?",
            ans,
        )
        card_diag = next(d for d in diags if d.code == "QA.CARDINALITY")
        assert card_diag.severity == Severity.CRITICAL
        sug = (card_diag.suggestion or "").lower()
        assert "notas" in sug
        assert "lacuna" in sug or "lista" in sug
        assert "convert to a cloze" not in sug
        assert "converta para cloze" not in sug


@pytest.mark.parametrize(
    "answer",
    [
        "5 mg, IV, a cada 8 horas",
        "500 mg, 12/12h, por 7 dias",
        "Cefaleia (unilateral, pulsátil, moderada a grave)",
        "Glossoplegia, caracterizada por paralisia da língua, com desvio ipsilateral, sem disartria",
        "glossoplegia, caracterizada por atrofia, fraqueza e desvio",
        "Pressão 140&#124;90 mmHg &#124; pulso 80 bpm",
        # B25: só vírgulas — nenhum marcador/separador enumerativo explícito.
        "alfa, beta e gama",
        "Dor retroesternal, em aperto, com irradiação para o braço esquerdo",
    ],
)
def test_qa_cardinality_does_not_false_positive_on_single_unit_continuations(answer):
    diags = qa_mod.check_qa("Em paciente adulto, qual a conduta ou achado?", answer)
    assert "QA.CARDINALITY" not in {d.code for d in diags}, (answer, diags)


def test_cloze_too_long_and_visible_len_handle_html_and_latex():
    assert vocab.visible_len(r"{{c1::\frac{a}{b}::dica longa}}") == len(r"\frac{a}{b}")
    html_wrapped = CARRIER + '{{c1::<span class="termo">' + "x" * 105 + "</span>}}"
    assert "CLOZE.TOO_LONG" not in {d.code for d in cloze_mod.check_cloze(html_wrapped)}


def test_qa_answer_limit_uses_visible_text():
    resposta = "<b>" + "a" * 100 + "</b> &amp; " + "b" * 100
    assert "CARD.TOO_LONG_ANSWER" not in {
        d.code for d in qa_mod.check_qa("Em adulto, qual?", resposta)
    }


# ── Notas Adicionais: advisory size, not part of the recall budget ───────────


def test_notes_length_is_advisory_including_long_explanations_and_tables():
    base = "Explicação clínica. " * 60  # ~1200 chars
    assert "CARD.TOO_LONG_NOTES" not in {d.code for d in notes_mod.check_notes(base)}
    huge = "Explicação clínica. " * 300
    assert not any(d.severity == Severity.CRITICAL for d in notes_mod.check_notes(huge))
    assert "NOTE.LENGTH_WARN" in {d.code for d in notes_mod.check_notes(huge)}
    table = "<table><tr><td>" + "Explicação clínica. " * 150 + "</td></tr></table>"
    # Tables and long explanations remain supplementary, never a split obligation.
    assert "CARD.TOO_LONG_NOTES" not in {d.code for d in notes_mod.check_notes(table)}


# ── vocab: single source of truth ─────────────────────────────────────────────


def test_limits_match_the_card_writing_skills():
    assert vocab.CLOZE_MAX_INDICES == 3
    assert vocab.QA_MAX_ITEMS == 2
    assert vocab.BATCH_MAX_CARDS == 20
    assert vocab.LIMITS["cloze_carrier"].warn == 220
    assert vocab.LIMITS["cloze_deletion"].error == 80


@pytest.mark.parametrize(
    "markup, expected",
    [
        ("<b>abc</b>", 3),
        ("{{c1::abc::dica}}", 3),
        ("a&amp;b", 3),
        ("a<br>b", 3),
        ("  espaços   múltiplos  ", 17),
    ],
)
def test_visible_len_ignores_markup(markup, expected):
    assert vocab.visible_len(markup) == expected


def test_limit_diag_returns_none_within_warn_and_never_critical_above():
    """Justificativa: limiar legado `error` é preservado em Limit, mas nunca bloqueia (sempre WARN)."""
    lim = vocab.Limit(warn=10, error=20)
    assert vocab.limit_diag("X.Y", "m", limit=lim, value=10) is None
    d_warn = vocab.limit_diag("X.Y", "m", limit=lim, value=11)
    assert d_warn is not None and d_warn.severity == Severity.WARN
    assert "sinal de revisão, não prova de múltipla cobrança" in d_warn.message
    d_over = vocab.limit_diag("X.Y", "m", limit=lim, value=21)
    assert d_over is not None and d_over.severity == Severity.WARN
    assert "sinal de revisão, não prova de múltipla cobrança" in d_over.message
    assert "contexto necessário" in (d_over.suggestion or "")


def test_codes_are_owned_by_card_generator():
    d = vocab.make_diag("CARD.X", "m")
    assert d.owner_skill == "anki-card-generator"
