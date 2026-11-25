"""Tests for notes information quality rules (empty/short, normalized HTML dup, NOTE.EXAM_ONLY_WARN)."""

from __future__ import annotations

from anki_mcp.checks.information import is_exam_only_note, profile_information
from anki_mcp.checks.notes import check_notes
from anki_mcp.core import codes
from anki_mcp.models import Severity


def test_empty_and_short_useful_notes_have_no_minimum_volume():
    assert check_notes("") == []
    assert check_notes("   ") == []
    assert check_notes("Meia-vida longa.") == []
    assert check_notes("Via renal.") == []


def test_exact_normalized_dup_catches_one_word_with_html_entities_case_punctuation():
    d1 = check_notes("<b>Riluzol.</b>", answer="riluzol")
    assert [d.code for d in d1] == ["NOTE.DUP_ANSWER_WARN"]
    assert d1[0].severity == Severity.WARN

    d2 = check_notes("<p>&ge;&nbsp;140 mmHg!</p>", answer="<b>&ge; 140 mmHg</b>")
    assert [d.code for d in d2] == ["NOTE.DUP_ANSWER_WARN"]

    d3 = check_notes("A resposta correta é: IECA.", answer="<i>IECA</i>")
    assert [d.code for d in d3] == ["NOTE.DUP_ANSWER_WARN"]


def test_explanation_mentioning_target_with_new_content_is_not_flagged():
    diags = check_notes(
        "<p><b>Riluzol</b> reduz a liberação de glutamato e prolonga moderadamente a sobrevida.</p>",
        answer="Riluzol",
    )
    assert diags == []


def test_exam_only_notes_emit_warn_registered_in_codes():
    assert "NOTE.EXAM_ONLY_WARN" in codes.all_codes()
    assert codes.get("NOTE.EXAM_ONLY_WARN").severity == "WARN"

    for txt in (
        "Já caiu em prova.",
        "Caiu na prova de 2024.",
        "<p>• <b>Prova:</b> Muito cobrado em provas de residência.</p>",
    ):
        diags = check_notes(txt)
        diag_codes = [(d.code, d.severity) for d in diags]
        assert ("NOTE.EXAM_ONLY_WARN", Severity.WARN) in diag_codes, (txt, diags)


def test_real_explanation_with_exam_mention_or_clinical_prova_not_flagged():
    for txt in (
        "Caiu na prova de 2024. A meia-vida longa explica o platô tardio.",
        "Tema cobrado em prova: segue cinética de ordem zero em doses terapêuticas.",
        "Prova calórica ausente indica arreflexia vestibular.",
        "Caiu na prova de 2024 por distinguir o grupo alfa do beta",
        "Caiu na prova de 2024 de mecanismo ainda desconhecido",
        "Caiu na prova de 2024 para distinguir o grupo alfa do beta",
        "Muito cobrado em provas na diferenciação entre crise focal e síncope",
        "Já caiu em prova em pacientes com hepatopatia crônica para evitar toxicidade",
    ):
        assert not is_exam_only_note(txt), txt
        diags = check_notes(txt)
        assert "NOTE.EXAM_ONLY_WARN" not in {d.code for d in diags}, (txt, diags)


def test_exam_only_allows_optional_year_and_known_uppercase_exam_label():
    for txt in (
        "Já caiu em prova.",
        "Caiu na prova de 2024.",
        "Caiu na prova da USP de 2024.",
        "Prova do ENARE 2024.",
    ):
        assert is_exam_only_note(txt), txt


def test_restatement_with_real_explanation_not_flagged_and_profile_agrees():
    notes_with_expl = (
        "A resposta correta é: Riluzol. Ele reduz a liberação de glutamato e prolonga "
        "moderadamente a sobrevida."
    )
    diags = check_notes(notes_with_expl, answer="Riluzol")
    assert "NOTE.DUP_ANSWER_WARN" not in {d.code for d in diags}

    prof = profile_information(
        note_type="QA",
        prompt="Em paciente com ELA, qual fármaco aumenta sobrevida?",
        answer="Riluzol",
        notes=notes_with_expl,
    )
    assert prof["layers"]["optional_explanation"]["exact_answer_repetition"] is False
    assert "NOTE.DUP_ANSWER_WARN" not in {s["code"] for s in prof["advisory_signals"]}


def test_pure_restatement_still_warns_and_profile_agrees():
    for txt, ans in (
        ("A resposta correta é: Riluzol.", "Riluzol"),
        ("A resposta correta é Alfa.", "Alfa"),
    ):
        diags = check_notes(txt, answer=ans)
        assert "NOTE.DUP_ANSWER_WARN" in {d.code for d in diags}, txt
        prof = profile_information(note_type="QA", prompt="Qual?", answer=ans, notes=txt)
        assert prof["layers"]["optional_explanation"]["exact_answer_repetition"] is True, txt
        assert "NOTE.DUP_ANSWER_WARN" in {s["code"] for s in prof["advisory_signals"]}, txt


def test_metadata_still_critical():
    diags = check_notes("ID: 1234 — nota de teste")
    assert any(d.code == "NOTE.METADATA" and d.severity == Severity.CRITICAL for d in diags)


def test_metadata_survives_markdown_and_bullet_prefixes():
    """B26: `**ID:**` e `• ID:` são o mesmo metadado que `ID:` e não podem escapar."""
    for notes in (
        "**ID:** 1234 — nota de teste",
        "__ID:__ 1234 — nota de teste",
        "• ID: 1234 — nota de teste",
        "• **ID:** 1234 — nota de teste",
        "▪ Relevância: alta",
        "* Âncora: p. 83",
    ):
        diags = check_notes(notes)
        assert any(d.code == "NOTE.METADATA" for d in diags), notes

    # Mensagem histórica preservada: só o rótulo, sem asteriscos nem bullet.
    diags = check_notes("• **ID:** 1234")
    metadata = next(d for d in diags if d.code == "NOTE.METADATA")
    assert metadata.message == "Forbidden metadata header found: 'ID:'"
    assert metadata.severity == Severity.CRITICAL
