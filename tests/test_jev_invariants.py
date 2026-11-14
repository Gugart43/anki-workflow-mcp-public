"""tests/test_jev_invariants.py — precisao dos UNIVERSAL_INVARIANTS (jev_triage).

Regressoes medidas: as regex de `padrao_ouro` e `red_flags` usavam termos
genericos ("exame", "alerta") que davam match em semiologia basal, inflando o
numero de eixos clinicos detectados e portanto o EV da secao.
"""

from __future__ import annotations

from anki_mcp.checks.jev_triage import (
    UNIVERSAL_INVARIANTS,
    heuristic_section_density,
)


def _matched(text: str) -> list[str]:
    import re

    low = text.lower()
    return [slug for slug, _label, pattern in UNIVERSAL_INVARIANTS if re.search(pattern, low)]


def test_physical_exam_alone_is_not_pattern_of_reference() -> None:
    """ "exame fisico" e semiologia (reconhecimento), nao confirmacao diagnostica."""
    matched = _matched("Ao exame fisico observa-se palidez cutanea e ausculta limpa.")
    assert "padrao_ouro" not in matched
    assert "reconhecimento" in matched


def test_complementary_exam_still_counts_as_pattern_of_reference() -> None:
    """O eixo de confirmacao diagnostica continua detectavel quando e real."""
    assert "padrao_ouro" in _matched("Exames complementares: PCR no LCR confirma o agente.")
    assert "padrao_ouro" in _matched("O padrao-ouro e a biopsia de arteria temporal.")


def test_alert_level_of_consciousness_is_not_a_red_flag() -> None:
    """ "vigil e alerta" e exame neurologico normal, nao sinal de alarme."""
    matched = _matched("Paciente vigil, alerta e orientado em tempo e espaco.")
    assert "red_flags" not in matched


def test_true_red_flags_still_detected() -> None:
    assert "red_flags" in _matched("Sinal de alerta: cefaleia em trovoada exige investigacao.")
    assert "red_flags" in _matched("Perda subita de visao e emergencia oftalmologica.")


def test_semiology_section_does_not_inflate_ev() -> None:
    """Duas regex frouxas elevavam o EV de 1.1 para 2.8 em texto puramente semiologico."""
    ev, _conf, primary, _probs = heuristic_section_density(
        "Paciente vigil e alerta, orientado no tempo e no espaco. "
        "Exame fisico: ausculta pulmonar limpa, abdomen indolor.",
        "Semiologia",
    )
    assert primary == "reconhecimento"
    assert ev <= 1.1
