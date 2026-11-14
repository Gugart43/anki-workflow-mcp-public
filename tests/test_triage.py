"""tests/test_triage.py — checks/triage.py scoring engine tests.

Covers: candidate extraction (headings, bullets, numbered, paragraphs, short
drop), each of the five scoring dimensions with positive and negative samples,
three verdict thresholds and the composite-descending sort order.
"""

from __future__ import annotations

from anki_mcp.checks import triage
from anki_mcp.checks.triage import (
    CARD_THRESHOLD,
    MAYBE_THRESHOLD,
    extract_candidates,
    score_atomicity,
    score_candidate,
    score_factual_density,
    score_non_triviality,
    score_stability,
    score_yield,
    triage_text,
)

# ── Sample texts ──────────────────────────────────────────────────────────────

CARD_TEXT = (
    "O tratamento de primeira linha da meningite bacteriana no adulto e "
    "ceftriaxona 2 g IV a cada 12 horas."
)
CARD_TEXT_CRITERIA = (
    "Segundo os criterios de Light, o exsudato pleural apresenta relacao "
    "proteina/soro maior que 0,5."
)
MAYBE_TEXT = "A anatomia do hipocampo e descrita em detalhes nos textos classicos de neuroanatomia."
DISCARD_TEXT = (
    "E importante entender que o contexto geral deste capitulo e fundamental neste texto."
)
UNSTABLE_TEXT = "A prevalencia da doenca e de 12% em estudos recentes publicados em 2023."

MD_SOURCE = """# Capitulo 1 - Introducao

Este capitulo apresenta os conceitos gerais da neurologia clinica.

## Epilepsia

- O estado de mal epileptico e definido como crise por 5 minutos ou mais.
- A fenitoina tem dose de ataque de 20 mg/kg IV.

1) O mecanismo envolve internalizacao de receptores GABA-A.
2) Outra droga util e o levetiracetam.

Veremos a seguir os conceitos.
"""


# ── Extraction ────────────────────────────────────────────────────────────────


def test_extract_from_headings_and_bullets():
    """Headings become section labels; bullets yield candidates."""
    candidates = extract_candidates(MD_SOURCE)
    labels = {c["source_label"] for c in candidates}
    assert "Epilepsia" in labels
    texts = [c["text"] for c in candidates]
    assert any("epileptico" in t.lower() for t in texts)


def test_extract_from_numbered_list():
    candidates = extract_candidates(MD_SOURCE)
    texts = [c["text"] for c in candidates]
    assert any("GABA" in t or "levetiracetam" in t for t in texts)


def test_extract_drops_short_candidates():
    short = "ABC.\nXYZ.\nUma frase com conteudo suficiente para passar o filtro de tamanho."
    candidates = extract_candidates(short)
    for c in candidates:
        assert len(c["text"]) >= 25


def test_extract_paragraph_sentences():
    text = "Primeira frase com conteudo adequado. Segunda frase tambem com conteudo relevante."
    candidates = extract_candidates(text)
    assert len(candidates) >= 2


def test_extract_heading_only_not_candidate():
    """Heading lines are section labels, not candidates themselves."""
    text = "# Apenas um titulo\n\nConteudo real com informacoes clinicas relevantes."
    candidates = extract_candidates(text)
    texts = [c["text"] for c in candidates]
    assert all("titulo" not in t.lower() for t in texts)


# ── Dimension 1: atomicity ────────────────────────────────────────────────────


def test_atomicity_single_fact():
    assert score_atomicity("A fenitoina tem dose de ataque de 20 mg/kg IV.") == 1.0


def test_atomicity_multiple_facts():
    multi = "Os sintomas incluem cefaleia, nausea, vomitos, rigidez de nuca e fotofobia."
    assert score_atomicity(multi) < 0.6


# ── Dimension 2: factual density ─────────────────────────────────────────────


def test_factual_density_high():
    assert score_factual_density(CARD_TEXT) >= 0.45


def test_factual_density_low():
    assert score_factual_density(DISCARD_TEXT) <= 0.2


def test_factual_density_technical_vocab_bonus():
    """Mechanisms / definitions get a density bonus even without numbers."""
    assert (
        score_factual_density(
            "O mecanismo fisiopatologico envolve a internalizacao de receptores GABA-A."
        )
        >= 0.2
    )


# ── Dimension 3: stability ───────────────────────────────────────────────────


def test_stability_rewards_mechanisms():
    assert score_stability("O mecanismo fisiopatologico envolve perda de inibitorio GABA.") >= 0.7


def test_stability_penalises_statistics():
    assert score_stability(UNSTABLE_TEXT) < 0.5


# ── Dimension 4: yield ───────────────────────────────────────────────────────


def test_yield_high():
    assert score_yield(CARD_TEXT) >= 0.5


def test_yield_low():
    assert score_yield("A curiosidade historica sobre esta sindrome remonta ao seculo XIX.") < 0.5


# ── Dimension 5: non-triviality ──────────────────────────────────────────────


def test_non_triviality_rewards_substantive():
    assert (
        score_non_triviality("O neuronio motor superior origina-se no cortex motor primario.")
        == 1.0
    )


def test_non_triviality_penalises_filler():
    assert score_non_triviality(DISCARD_TEXT) <= 0.2


def test_non_triviality_penalises_meta_and_tautology():
    assert score_non_triviality("Como foi dito neste capitulo, a figura 3 resume a tabela.") <= 0.2
    assert score_non_triviality("O choque cardiogenico e um tipo de choque.") <= 0.35


# ── Verdict thresholds ────────────────────────────────────────────────────────


def test_verdict_thresholds_are_inclusive_at_the_boundary():
    assert triage._verdict(CARD_THRESHOLD) == "CARD"
    assert triage._verdict(CARD_THRESHOLD - 0.01) == "MAYBE"
    assert triage._verdict(MAYBE_THRESHOLD) == "MAYBE"
    assert triage._verdict(MAYBE_THRESHOLD - 0.01) == "DISCARD"


def test_card_text_scores_card():
    score = score_candidate(CARD_TEXT)
    assert score["verdict"] == "CARD"
    assert score["composite"] >= CARD_THRESHOLD


def test_maybe_text_scores_maybe():
    score = score_candidate(MAYBE_TEXT)
    assert MAYBE_THRESHOLD <= score["composite"] < CARD_THRESHOLD


def test_discard_text_scores_discard():
    score = score_candidate(DISCARD_TEXT)
    assert score["verdict"] == "DISCARD"


# ── triage_text integration ──────────────────────────────────────────────────


def test_triage_text_sorted_desc():
    results = triage_text(MD_SOURCE, source_label="test")
    composites = [r["composite"] for r in results]
    assert composites == sorted(composites, reverse=True)


def test_triage_text_has_code_key():
    results = triage_text(MD_SOURCE)
    for r in results:
        assert "code" in r


def test_triage_text_source_label_propagated():
    results = triage_text(
        "Conteudo clinico relevante sobre diagnostico diferencial.", source_label="meu_label"
    )
    for r in results:
        assert r["source_label"] in ("meu_label", "")
