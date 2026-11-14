"""test_checks_golden.py — golden-fixture tests for all check modules.

Every BAD example from the skills must FAIL or WARN.
Every GOOD example must PASS.
"""

from __future__ import annotations

import pytest

from anki_mcp.checks import cloze as cloze_mod
from anki_mcp.checks import csv_structure
from anki_mcp.checks import hints as hints_mod
from anki_mcp.checks import images as images_mod
from anki_mcp.checks import notes as notes_mod
from anki_mcp.checks import qa as qa_mod
from anki_mcp.models import Severity
from tests.golden.fixtures import (
    CLOZE_CASES,
    CSV_CASES,
    HINT_CASES,
    IMAGE_CASES,
    NOTES_CASES,
    QA_CASES,
)


def _highest(diags) -> str:
    """Return 'FAIL'/'WARN'/'PASS' based on the worst diagnostic severity."""
    codes = {d.severity for d in diags}
    if Severity.CRITICAL in codes:
        return "FAIL"
    if Severity.WARN in codes:
        return "WARN"
    return "PASS"


# ── Cloze golden tests ────────────────────────────────────────────────────────


@pytest.mark.parametrize("desc,sentence,expected", CLOZE_CASES)
def test_cloze_golden(desc, sentence, expected):
    diags = cloze_mod.check_cloze(sentence)
    result = _highest(diags)
    assert result == expected, (
        f"[{desc}]\n"
        f"  Expected: {expected}\n"
        f"  Got:      {result}\n"
        f"  Diags:    {[(d.code, d.message) for d in diags]}"
    )


# ── Hint golden tests ─────────────────────────────────────────────────────────


@pytest.mark.parametrize("desc,sentence,expected", HINT_CASES)
def test_hints_golden(desc, sentence, expected):
    diags = hints_mod.check_hints(sentence)
    result = _highest(diags)
    assert result == expected, (
        f"[{desc}]\n"
        f"  Expected: {expected}\n"
        f"  Got:      {result}\n"
        f"  Diags:    {[(d.code, d.message) for d in diags]}"
    )


# ── QA golden tests ───────────────────────────────────────────────────────────


@pytest.mark.parametrize("desc,enunciado,resposta,expected", QA_CASES)
def test_qa_golden(desc, enunciado, resposta, expected):
    diags = qa_mod.check_qa(enunciado, resposta)
    result = _highest(diags)
    assert result == expected, (
        f"[{desc}]\n"
        f"  Expected: {expected}\n"
        f"  Got:      {result}\n"
        f"  Diags:    {[(d.code, d.message) for d in diags]}"
    )


# ── Notes golden tests ────────────────────────────────────────────────────────


@pytest.mark.parametrize("desc,notes,answer,expected", NOTES_CASES)
def test_notes_golden(desc, notes, answer, expected):
    diags = notes_mod.check_notes(notes, answer)
    result = _highest(diags)
    assert result == expected, (
        f"[{desc}]\n"
        f"  Expected: {expected}\n"
        f"  Got:      {result}\n"
        f"  Diags:    {[(d.code, d.message) for d in diags]}"
    )


# ── CSV structure golden tests ────────────────────────────────────────────────


@pytest.mark.parametrize("desc,csv_text,note_type,expected", CSV_CASES)
def test_csv_structure_golden(desc, csv_text, note_type, expected):
    diags = csv_structure.check_csv(csv_text, note_type)
    result = _highest(diags)
    assert result == expected, (
        f"[{desc}]\n"
        f"  Expected: {expected}\n"
        f"  Got:      {result}\n"
        f"  Diags:    {[(d.code, d.message) for d in diags]}"
    )


# ── Image field golden tests ──────────────────────────────────────────────────


@pytest.mark.parametrize("desc,html,catalog,expected", IMAGE_CASES)
def test_images_golden(desc, html, catalog, expected):
    # Under the AnkiMBR CSS variables contract, the legacy fixture containing
    # inline style attributes now correctly triggers IMG.INLINE_STYLE (WARN).
    if desc == "GOOD: properly namespaced image with caption":
        expected = "WARN"
        clean_html = (
            '<figure><img src="statdx_ms_img_p9_1.jpeg" '
            'alt="Figura sagital de placas venosas de EM (RM T2, corte axial).">'
            "<figcaption>Figura sagital de placas venosas de EM (RM T2, corte axial).</figcaption></figure>"
        )
        clean_diags = images_mod.check_images_field(clean_html, catalog)
        assert _highest(clean_diags) == "PASS"

    diags = images_mod.check_images_field(html, catalog)
    result = _highest(diags)
    assert result == expected, (
        f"[{desc}]\n"
        f"  Expected: {expected}\n"
        f"  Got:      {result}\n"
        f"  Diags:    {[(d.code, d.message) for d in diags]}"
    )


# ── Cloze Anti-Leak & Length Enhancements ───────────────────────────────────


def test_cloze_acronym_leak():
    # Full name in cloze, visible acronym immediately follows
    s1 = "Paciente com {{c1::esclerose lateral amiotrófica}} (ELA) em fase avançada."
    diags1 = cloze_mod.check_cloze(s1)
    assert any(d.code == "CLOZE.ACRONYM_LEAK" for d in diags1)

    # Acronym in cloze, visible full name immediately precedes
    s2 = "Paciente com esclerose lateral amiotrófica ({{c1::ELA}}) em fase avançada."
    diags2 = cloze_mod.check_cloze(s2)
    assert any(d.code == "CLOZE.ACRONYM_LEAK" for d in diags2)

    # Clean: both full name and acronym are occluded together
    s3 = "Paciente com {{c1::esclerose lateral amiotrófica (ELA)}} em fase avançada."
    diags3 = cloze_mod.check_cloze(s3)
    assert not any(d.code == "CLOZE.ACRONYM_LEAK" for d in diags3)


def test_cloze_too_long():
    long_target = "a" * 125
    s = f"Definição clínica: {{{{c1::{long_target}}}}}"
    diags = cloze_mod.check_cloze(s)
    assert any(d.code == "CLOZE.TOO_LONG" for d in diags)


def test_cloze_duplicate_index_emits_nonseq():
    """Repeating the same cloze index c1..c1 in one note violates non-sequential constraint."""
    s = "Tratamento de primeira linha é {{c1::ceftriaxona}} e segunda linha é {{c1::vancomicina}}."
    diags = cloze_mod.check_cloze(s)
    nonseq = [d for d in diags if d.code == "CLOZE.NONSEQ"]
    assert any("Duplicate cloze index" in d.message for d in nonseq)


def test_autofix_preview_on_clinical_inequality():
    """H1-4: FMT.ENTITY_MISSING on clinical comparisons must offer auto-fix preview."""
    qa = (
        "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas\n"
        "Qual o limiar?|Administrar dose > 50 mg/dia em idosos.|Medicina|Cuidado se TFG < 30 mL/min.||Diretriz|Neuro"
    )
    diags = csv_structure.check_csv(qa, "QA")
    missing = [d for d in diags if d.code == "FMT.ENTITY_MISSING"]
    assert missing, "Expected FMT.ENTITY_MISSING for raw > and <"
    for d in missing:
        assert d.auto_fix_available is True
        assert d.fix_preview is not None
        assert ("&gt;" in d.fix_preview) or ("&lt;" in d.fix_preview)
