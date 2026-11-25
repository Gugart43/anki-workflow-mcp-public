"""tests/test_differential_eval.py — Oracle differential test for evaluate_batch vs tools.

Ensures core/evaluate.py, validate_batch, and pipeline_gate_check agree on verdicts,
codes, and receipts across standard, edge, and failing corpora (Bug B2 elimination).
"""

from __future__ import annotations

from pathlib import Path

import pytest
from fastmcp import FastMCP

from anki_mcp.core.deck import save_manifesto
from anki_mcp.core.evaluate import evaluate_batch
from anki_mcp.models import Manifesto, TaxonomyNode
from anki_mcp.tools.pipeline import register_pipeline_tools
from anki_mcp.tools.validate import register_validate_tools

QA_HEADER = (
    "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas"
)
CL_HEADER = "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas"

GOOD_QA = (
    QA_HEADER
    + "\nQual a conduta inicial na crise convulsiva em adulto?|Administrar diazepam 10 mg IV lento.|Neurologia|Monitorar via aérea e saturação O2.||Diretrizes SBC 2024|Neuro::Epilepsia"
)
GOOD_CL = (
    CL_HEADER
    + "\nNo manejo do Status Epilepticus em adultos, a 1ª linha farmacológica é {{c1::diazepam IV}} ou {{c2::midazolam IM}}.|Pérola clínica: repetir após 5 min se persistir.|Neurologia||ILAE 2023|Neuro::Status"
)

BAD_CLOZE_NONSEQ = (
    CL_HEADER
    + "\nO paciente com TCE grave apresentou {{c1::anisocoria}} e evoluiu com {{c3::parada respiratória}}.|Herniação uncal.|Neurologia||ATLS 10|Neuro::Trauma"
)

BAD_QA_CARDINALITY = (
    QA_HEADER
    + "\nQuais os critérios diagnósticos de migrânea sem aura?|1. Cefaleia unilateral; 2. pulsátil; 3. intensidade moderada a grave; 4. piora com atividade física; 5. náusea e fotofobia.|Neurologia|Critérios ICHD-3.||ICHD-3|Neuro::Migranea"
)

# B25: a mesma resposta em prosa (só vírgulas) descreve UM conceito com vírgulas de
# aposição. Sem marcador enumerativo explícito não há QA.CARDINALITY — o veredito PASS é
# o esperado, e não uma regressão: era o falso positivo CRITICAL medido antes da Onda 2.
GOOD_QA_APPOSITIVE_COMMAS = (
    QA_HEADER
    + "\nQuais os critérios diagnósticos de migrânea sem aura?|Cefaleia unilateral, pulsátil, intensidade moderada a grave, piora com atividade física, náusea e fotofobia.|Neurologia|Critérios ICHD-3.||ICHD-3|Neuro::Migranea"
)

STRUCTURAL_SHIFTED = (
    QA_HEADER
    + "\nPergunta sem pipe sanitizado|Resposta|Medicina|Nota com | pipe solto|||Neuro::Geral"
)


@pytest.fixture
def test_deck(tmp_path: Path) -> Path:
    deck = tmp_path / "diff_deck"
    deck.mkdir()
    m = Manifesto(
        deck_slug="diff-deck",
        campo_estudo="Medicina",
        created="2026-01-01",
        updated="2026-01-01",
        taxonomy=TaxonomyNode(
            root="Neuro", nodes=["Epilepsia", "Status", "Trauma", "Migranea", "Geral"]
        ),
    )
    save_manifesto(deck, m)
    return deck


@pytest.fixture
def tool_runners():
    import asyncio

    mcp = FastMCP("test-differential")
    register_validate_tools(mcp)
    register_pipeline_tools(mcp)
    validate_fn = asyncio.run(mcp.get_tool("validate_batch")).fn
    gate_fn = asyncio.run(mcp.get_tool("pipeline_gate_check")).fn
    return validate_fn, gate_fn


@pytest.mark.parametrize(
    "case_name,qa_csv,cloze_csv,expected_verdict",
    [
        ("clean_pass", GOOD_QA, GOOD_CL, "PASS"),
        ("bad_cloze_nonseq", GOOD_QA, BAD_CLOZE_NONSEQ, "FAIL"),
        ("bad_qa_cardinality", BAD_QA_CARDINALITY, GOOD_CL, "FAIL"),
        ("good_qa_appositive_commas", GOOD_QA_APPOSITIVE_COMMAS, GOOD_CL, "PASS"),
        ("structural_shifted_pipe", STRUCTURAL_SHIFTED, GOOD_CL, "FAIL"),
    ],
)
def test_differential_oracle_eval(
    case_name: str,
    qa_csv: str,
    cloze_csv: str,
    expected_verdict: str,
    test_deck: Path,
    tool_runners,
):
    validate_fn, gate_fn = tool_runners

    # 1. Direct evaluate_batch
    ev = evaluate_batch(qa_csv, cloze_csv, deck_dir=str(test_deck))
    assert ev.verdict == expected_verdict, f"Direct evaluate_batch failed for {case_name}"

    # 2. validate_batch tool
    vb = validate_fn(batch_id="B01", qa_csv=qa_csv, cloze_csv=cloze_csv)
    assert vb["result"] == expected_verdict, f"validate_batch mismatch for {case_name}"

    # 3. pipeline_gate_check tool
    gate = gate_fn(
        deck_dir=str(test_deck),
        batch_id="B01",
        phase="drafted",
        qa_csv=qa_csv,
        cloze_csv=cloze_csv,
    )
    assert gate["status"] == expected_verdict, f"pipeline_gate_check mismatch for {case_name}"

    # 4. Receipt consistency on PASS
    if expected_verdict == "PASS":
        assert ev.receipt is not None
        assert vb["content_hash"] == ev.receipt
        assert gate["content_hash"] == ev.receipt
