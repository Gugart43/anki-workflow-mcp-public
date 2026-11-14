"""Deterministic content filter, primary attribution and justified card budget.

The golden excerpt is a fictional pt-BR clinical text about Huntington's
disease, written for this suite. Expectations were derived from the rules in
``checks/decisions.py`` and checked by hand against each sentence.
"""

from __future__ import annotations

import asyncio
import math

import pytest

from anki_mcp.checks import decisions as dec
from anki_mcp.checks.decisions import (
    DECISION_TAXONOMY,
    DecisionType,
    budget_from_counts,
    classify_content,
    evaluate_plan,
    plan_topic_budget,
)

GOLDEN = """\
## Epidemiologia
A doença de Huntington afeta cerca de 5 a 10 pessoas por 100 mil habitantes no mundo.
Descrita por George Huntington em 1872, em um relato sobre famílias de Long Island.
Posso formular um caso clínico simulado para você praticar.

## Quadro clínico
A tríade clássica é coreia, alterações cognitivas e transtorno psiquiátrico.
Os sintomas iniciais costumam surgir entre os 30 e os 50 anos.

## Diagnóstico
O diagnóstico clínico exige coreia e história familiar compatível.
O critério diagnóstico é a presença de coreia e expansão de CAG igual ou maior que 36 repetições.
A ressonância magnética mostra atrofia do núcleo caudado e putame.

## Tratamento
O tratamento de primeira linha para coreia incapacitante inclui tetrabenazina, cujo efeito é inibir VMAT2.
Em caso de coreia, a dose inicial de tetrabenazina é 12,5 mg por dia.
A depressão com ideação suicida é uma contraindicação relativa ao uso de tetrabenazina e exige cautela.

## Diagnóstico diferencial
No diagnóstico diferencial, considerar coreia-acantocitose e doença de Wilson.
Sinais de alarme incluem início antes dos 10 anos ou piora rápida da marcha.

## Prognóstico
A sobrevida média após o início dos sintomas é de cerca de 15 a 20 anos.
A causa mais comum de morte é a pneumonia por aspiração.

## Aconselhamento genético
O aconselhamento genético pré-teste é indispensável para adultos assintomáticos.
Diretivas antecipadas de vontade devem ser discutidas com a família.

## Mecanismo
A expansão de repetições CAG no gene HTT produz huntingtina com ganho de função tóxica.
A prevalência da doença de Huntington é de 5 por 100.000 e foi estudada por décadas.
- Doença autossômica dominante, com herança de 50% para filhos.
"""

_CANDIDATES = [line for line in GOLDEN.splitlines() if line and not line.startswith("#")]


def _verdict(fragment: str):
    matches = [s for s in _CANDIDATES if fragment in s]
    assert len(matches) == 1, f"fragment {fragment!r} must identify one sentence"
    return classify_content(matches[0])


# ── Content filter: epidemiology/history/chatter are DISCARD ──────────────────


@pytest.mark.parametrize(
    "fragment, reason_fragment",
    [
        ("afeta cerca de 5 a 10", "epidemiologia descritiva"),
        ("Descrita por George Huntington", "história da medicina"),
        ("Posso formular um caso", "meta / conversa"),
        ("A prevalência da doença", "epidemiologia descritiva"),
    ],
)
def test_epidemiology_history_and_chatter_are_discarded(fragment, reason_fragment):
    v = _verdict(fragment)
    assert v.verdict == "DISCARD"
    assert reason_fragment in v.reason
    assert v.primary is None


def test_non_decisional_statement_without_trigger_is_maybe_not_card():
    v = _verdict("herança de 50% para filhos")
    assert v.verdict == "MAYBE"
    assert "decisão humana" in v.reason


# ── Content filter: criteria, cut-offs, therapy lines are CARD ────────────────


@pytest.mark.parametrize(
    "fragment, primary",
    [
        ("tríade clássica", "reconhecimento"),
        ("sintomas iniciais costumam", "reconhecimento"),
        ("O diagnóstico clínico exige", "marco_diagnostico"),
        ("maior que 36 repetições", "marco_diagnostico"),
        ("ressonância magnética mostra", "confirmacao"),
        ("tratamento de primeira linha", "tratamento_1a_linha"),
        ("dose inicial de tetrabenazina", "tratamento_1a_linha"),
        ("No diagnóstico diferencial", "diferencial"),
        ("Sinais de alarme incluem", "red_flag"),
        ("sobrevida média", "prognostico"),
        ("causa mais comum de morte", "prognostico"),
        ("aconselhamento genético pré-teste", "bioetica"),
    ],
)
def test_decision_sentences_are_card_with_primary_type(fragment, primary):
    v = _verdict(fragment)
    assert v.verdict == "CARD", v.reason
    assert v.primary == primary


def test_contraindication_is_card_and_negation_is_reported_not_demoted():
    v = _verdict("contraindicação relativa")
    assert v.verdict == "CARD"
    assert v.primary == "seguranca_farmacologica"
    assert v.polarity == "negated"
    assert "preservar negação" in v.reason


def test_mechanism_only_is_maybe_never_card():
    v = _verdict("expansão de repetições CAG no gene HTT")
    assert v.verdict == "MAYBE"
    assert "só mecanismo" in v.reason
    assert v.primary is None


def test_decision_trigger_overrides_discard_pattern_in_same_sentence():
    v = classify_content(
        "A prevalência é alta, mas o diagnóstico clínico exige ressonância magnética."
    )
    assert v.verdict == "CARD"
    assert "epidemiologia descritiva" in v.reason


def test_single_generic_word_does_not_create_a_decision():
    # "dose" and "tratamento" alone are not phrase triggers.
    assert classify_content("Nos dias de hoje, a dose é um tema de estudo.").verdict != "CARD"


# ── Applicability: phrase-level, per topic ────────────────────────────────────


def test_golden_applicability_contains_the_decisions_the_excerpt_raises():
    slugs = {dt.slug for dt in dec.applicable_types(GOLDEN)}
    assert {
        "reconhecimento",
        "marco_diagnostico",
        "tratamento_1a_linha",
        "seguranca_farmacologica",
        "diferencial",
        "red_flag",
        "prognostico",
        "bioetica",
        "confirmacao",
        "mecanismo",
    } <= slugs


def test_rare_topic_without_comorbidity_section_is_not_charged_for_it():
    slugs = {dt.slug for dt in dec.applicable_types(GOLDEN)}
    assert "cenario_especial" not in slugs
    assert "monitorizacao" not in slugs


# ── Attribution: one primary type per card, no multi-counting ────────────────


def test_each_card_is_counted_under_exactly_one_type():
    cands = [{"text": s, "verdict": "CARD"} for s in _CANDIDATES]
    coverage = dec.build_coverage(cands)
    counted = [idx for lst in coverage.values() for idx in lst]
    assert len(counted) == len(set(counted))  # no candidate appears twice
    # Every triage-CARD that the filter also calls CARD is counted once.
    expected = {i for i, c in enumerate(cands) if classify_content(c["text"]).verdict == "CARD"}
    assert set(counted) == expected


def test_maybe_candidates_do_not_count_toward_coverage():
    cands = [
        {"text": "expansão de repetições CAG no gene HTT produz huntingtina", "verdict": "CARD"}
    ]
    assert all(not lst for lst in dec.build_coverage(cands).values())


# ── Budget: justified, auditable, never invented ──────────────────────────────


def _applicable():
    return dec.applicable_types(GOLDEN)


def test_budget_subdimensionado_points_to_gap():
    applicable = _applicable()
    counts = {dt.slug: 2 for dt in applicable}
    counts["marco_diagnostico"] = 0  # no card for the diagnostic milestone
    b = budget_from_counts(applicable, counts)
    assert "marco_diagnostico" in b.gaps
    assert b.stopping["cobertura"] is False
    assert b.status == "ABERTO"
    assert "LACUNA" in next(r.justification for r in b.rows if r.slug == "marco_diagnostico")


def test_budget_superdimensionado_points_to_trimming():
    applicable = _applicable()
    counts = {dt.slug: 1 for dt in applicable}
    counts["marco_diagnostico"] = 5  # target_max is 2
    b = budget_from_counts(applicable, counts)
    assert b.excess == {"marco_diagnostico": 3}
    row = next(r for r in b.rows if r.slug == "marco_diagnostico")
    assert row.status == "EXCEDENTE"
    assert row.retained == 2
    assert "aparar 3" in row.justification


def test_budget_forty_cards_split_into_two_full_batches():
    types = tuple(
        DecisionType(
            slug=f"t{i}",
            label=f"T{i}",
            target_min=1,
            target_max=10,
            triggers=r"zzz",
            obrigatorio=True,
        )
        for i in range(4)
    )
    counts = {dt.slug: 10 for dt in types}
    b = budget_from_counts(types, counts)
    assert b.retained == 40
    assert b.batches_retained == math.ceil(40 / 20) == 2
    assert [batch["total"] for batch in b.batches] == [20, 20]
    assert b.batches[0]["types"] == {"t0": 10, "t1": 10}
    assert b.batches[1]["types"] == {"t2": 10, "t3": 10}
    assert b.stopping["orcamento"] is True


def test_budget_with_zero_candidates_invents_nothing():
    applicable = _applicable()
    counts = {dt.slug: 0 for dt in applicable}
    b = budget_from_counts(applicable, counts)
    assert b.retained == 0
    assert b.batches == []
    assert b.status == "SEM_CARTOES"
    assert b.stopping["orcamento"] is False
    assert "nada foi inventado" in b.justification()


def test_budget_without_source_decisions_plans_nothing():
    b = budget_from_counts((), {})
    assert b.total_max == 0 and b.status == "SEM_DECISOES"


def test_planning_without_cards_claims_no_coverage():
    plan = plan_topic_budget(GOLDEN, batch_size=20)
    assert plan["total_min"] <= plan["total_max"]
    assert plan["batches_needed"] == math.ceil(plan["total_max"] / 20)
    assert all(r["status"] == "NAO_AVALIADO" for r in plan["rows"])
    assert plan["stopping"] == {}


def test_plan_topic_budget_with_cards_reports_covered_over_target():
    cards = [s for s in _CANDIDATES if classify_content(s).verdict == "CARD"]
    plan = plan_topic_budget(GOLDEN, card_texts=cards)
    line = next(r for r in plan["rows"] if r["decision"] == "tratamento_1a_linha")
    assert line["covered"] == 2
    assert line["target"] == "2–6"
    assert "coberto 2 / alvo 2–6" in plan["justification"]
    expected_retained = sum(
        min(r["covered"], int(r["target"].split("–")[1]))
        for r in plan["rows"]
        if r["covered"] is not None
    )
    assert plan["retained"] == expected_retained == sum(b["total"] for b in plan["batches"])


def test_golden_budget_justification_names_each_decision_and_the_horizon():
    cards = [s for s in _CANDIDATES if classify_content(s).verdict == "CARD"]
    plan = plan_topic_budget(GOLDEN, card_texts=cards)
    text = plan["justification"]
    for row in plan["rows"]:
        assert row["justification"] in text
    assert "horizonte" in text and "lote(s) de até 20" in text
    # Cards-only entry point has no pending information: an uncovered type is
    # reported as LACUNA here. The evaluate_plan path distinguishes PENDENTE.
    mech = next(r for r in plan["rows"] if r["decision"] == "mecanismo")
    assert mech["status"] == "LACUNA"
    assert plan["budget_status"] == "ABERTO"


def test_evaluate_plan_maybe_is_pending_not_gap():
    cands = [{"text": s, "verdict": "CARD"} for s in _CANDIDATES]
    plan = evaluate_plan(GOLDEN, cands)
    assert "mecanismo" not in plan.gaps
    row = next(r for r in plan.budget.rows if r.slug == "mecanismo")
    assert row.status == "PENDENTE"
    assert plan.discarded  # epidemiology/history/chatter reported with reasons
    assert all("reason" in d for d in plan.discarded)


def test_evaluate_plan_zero_candidates_is_not_closed():
    plan = evaluate_plan(GOLDEN, [])
    assert plan.card_count == 0
    assert not plan.stopped


# ── MCP tool surface ─────────────────────────────────────────────────────────


def test_topic_budget_plan_tool_accepts_card_texts_and_keeps_signature():
    from fastmcp import FastMCP

    from anki_mcp.tools.corpus import register_corpus_tools

    mcp = FastMCP("test")
    register_corpus_tools(mcp)

    async def _run():
        tool = await mcp.get_tool("topic_budget_plan")
        return tool.fn

    fn = asyncio.run(_run())
    # First three golden lines are all DISCARD: no card can be counted, so the
    # budget must say so rather than report a closed one. The tool-level
    # "status" stays "OK" (the call succeeded); the budget state is budget_status.
    out = fn("C:/unused", text=GOLDEN, card_texts=_CANDIDATES[:3])
    assert out["status"] == "OK"
    assert out["budget_status"] == "SEM_CARTOES"
    assert "justification" in out and "rows" in out
    assert out["retained"] == 0
    old = fn("C:/unused", text=GOLDEN)  # pre-existing call shape still works
    assert old["status"] == "OK"
    assert old["budget_status"] == "ABERTO"
    assert old["stopping"] == {}  # no cards supplied -> no stop claim


def test_decision_taxonomy_is_frozen_and_ordered():
    assert len(DECISION_TAXONOMY) == 12
    assert next(dt.slug for dt in DECISION_TAXONOMY) == "marco_diagnostico"
