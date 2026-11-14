"""Tests for top-down topic budget planning and batch chunking enforcement."""

from pathlib import Path

from anki_mcp.checks.decisions import plan_topic_budget
from anki_mcp.core.deck import load_manifesto, save_manifesto
from anki_mcp.core.render import render_manifesto_md
from anki_mcp.models import Manifesto, TopicPlan


def test_plan_topic_budget_chunking():
    # Source text triggering multiple clinical decisions
    text = (
        "O diagnóstico da doença de Huntington baseia-se na presença de coreia "
        "e teste genético com expansão CAG no gene HTT maior que 35 repetições. "
        "No diagnóstico diferencial, deve-se considerar HDL2 e coreia-acantocitose. "
        "O tratamento de primeira linha para coreia incapacitante inclui tetrabenazina ou deutetrabenazina, "
        "que inibem VMAT2. A depressão com ideação suicida é uma contraindicação e requer cautela extrema. "
        "O estadiamento funcional pela escala UHDRS e TFC classifica a incapacidade. "
        "Aconselhamento genético pré-teste e diretivas antecipadas de vontade são indispensáveis."
    )
    plan = plan_topic_budget(text, batch_size=20)
    assert plan["total_planned"] >= 15
    assert plan["batch_size"] == 20
    assert plan["batches_needed"] >= 1
    assert "B01" in plan["batches_planned"]
    assert "applicable_decisions" in plan


def test_manifesto_topic_plan_persistence(tmp_path: Path):
    manifesto = Manifesto(
        deck_slug="test-deck",
        campo_estudo="Medicina::Test",
        created="2026-09-30",
        updated="2026-09-30",
        topic_plan=TopicPlan(
            total_planned=40,
            batch_size=20,
            batches_planned=["B01", "B02"],
            breakdown={"confirmacao": 6, "tratamento_1a_linha": 6},
            rationale="Topic requires 40 cards split in two batches of 20.",
        ),
    )
    save_manifesto(tmp_path, manifesto)

    loaded = load_manifesto(tmp_path)
    assert loaded.topic_plan is not None
    assert loaded.topic_plan.total_planned == 40
    assert loaded.topic_plan.batch_size == 20
    assert loaded.topic_plan.batches_planned == ["B01", "B02"]

    md = render_manifesto_md(loaded)
    assert "40 cartões planejados" in md
    assert "B01, B02" in md
