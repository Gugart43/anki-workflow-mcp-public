"""Integration tests for information_profile/profiles, MCP tool schemas, and phase_rules prompts."""

from __future__ import annotations

import asyncio
import json

from fastmcp import FastMCP

from anki_mcp.checks.information import BASE_LIMITATIONS, EDITORIAL_CHECKLIST, INFO_POLICY
from anki_mcp.core.evaluate import evaluate_batch
from anki_mcp.core.mcp_meta import get_phase_rules, register_prompts_and_resources, tag_tools
from anki_mcp.core.receipt import compute_receipt
from anki_mcp.tools.pipeline import register_pipeline_tools
from anki_mcp.tools.validate import register_validate_tools


def _make_server() -> FastMCP:
    mcp = FastMCP("test-anki")
    register_validate_tools(mcp)
    register_pipeline_tools(mcp)
    register_prompts_and_resources(mcp)
    tag_tools(mcp)
    return mcp


async def _call_tool(mcp: FastMCP, name: str, args: dict):
    tool = await mcp.get_tool(name)
    res = await tool.run(args)
    return json.loads(res.content[0].text)


async def _list_tools_map(mcp: FastMCP) -> dict[str, str]:
    tools = await mcp.list_tools()
    return {t.name: (t.description or "") for t in tools}


async def _get_prompt_text(mcp: FastMCP, phase: str) -> str:
    prompt = await mcp.get_prompt("phase_rules")
    res = await prompt.render({"phase": phase})
    msg = res.messages[0]
    content = msg.content
    return content.text if hasattr(content, "text") else str(content)


def test_validate_single_tools_expose_information_profile():
    mcp = _make_server()

    qa_res = asyncio.run(
        _call_tool(
            mcp,
            "validate_qa",
            {
                "enunciado": "Em cenário com contexto detalhado " + "x " * 170 + "qual a conduta?",
                "resposta": "Conduta Alfa",
            },
        )
    )
    assert qa_res["result"] == "WARN"  # length-only never FAIL
    assert qa_res["information_profile"]["note_type"] == "QA"
    assert qa_res["information_profile"]["semantic_review_required"] is True
    assert qa_res["information_profile"]["layers"]["retrieved_target"]["candidate_item_count"] == 1
    assert len(qa_res["information_profile"]["editorial_checklist"]) == 6

    cl_res = asyncio.run(
        _call_tool(
            mcp,
            "validate_cloze",
            {"sentence": "Os itens são {{c1::alfa; beta; gama}} e o contexto é {{c2::delta}}."},
        )
    )
    assert cl_res["card_count"] == 2
    cl_prof = cl_res["information_profile"]
    assert cl_prof["note_type"] == "CL"
    assert len(cl_prof["layers"]["retrieved_target"]["per_index"]) == 2
    assert cl_prof["layers"]["retrieved_target"]["per_index"][0]["hidden_list_signal"] is True

    notes_res = asyncio.run(
        _call_tool(
            mcp,
            "validate_notes",
            {"notes": "Caiu na prova de 2024.", "answer": "Conduta Alfa"},
        )
    )
    assert notes_res["result"] == "WARN"
    assert any(d["code"] == "NOTE.EXAM_ONLY_WARN" for d in notes_res["diagnostics"])
    n_prof = notes_res["information_profile"]
    assert n_prof["note_type"] == "QA"
    assert n_prof["prompt_available"] is False
    assert n_prof["layers"]["prompt_context"]["available"] is False
    assert any("enunciado" in lim.lower() for lim in n_prof["limitations"])


def test_batch_tools_and_evaluate_keep_receipt_unchanged_and_no_cl_as_qa():
    qa_csv = (
        "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas\n"
        "Em contexto clínico específico, qual a opção principal?|Opção Alfa|Campo::Sub|Mecanismo resumido.||Ref|Tag::Sub"
    )
    cl_csv = (
        "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas\n"
        "O marcador {{c1::beta}} associa-se a {{c2::gama}}.|Nota curta.|Campo::Sub||Ref|Tag::Sub"
    )

    ev = evaluate_batch(qa_csv, cl_csv)
    assert ev.receipt == compute_receipt(qa_csv, cl_csv)
    assert len(ev.information_profiles) == 2
    assert [(p["note_type"], p["row"]) for p in ev.information_profiles] == [("QA", 2), ("CL", 2)]
    assert ev.information_profiles[1]["layers"]["retrieved_target"]["card_count"] == 2

    mcp = _make_server()
    vb = asyncio.run(
        _call_tool(
            mcp,
            "validate_batch",
            {"batch_id": "B01", "qa_csv": qa_csv, "cloze_csv": cl_csv},
        )
    )
    assert vb["content_hash"] == compute_receipt(qa_csv, cl_csv)
    assert len(vb["information_profiles"]) == 2
    assert [p["note_type"] for p in vb["information_profiles"]] == ["QA", "CL"]
    assert vb["information_guidance"]["editorial_checklist"] == list(EDITORIAL_CHECKLIST)
    assert vb["information_guidance"]["policy"] == dict(INFO_POLICY)
    assert vb["information_guidance"]["limitations"] == list(BASE_LIMITATIONS)
    for row_prof in vb["information_profiles"]:
        assert "editorial_checklist" not in row_prof
        assert "policy" not in row_prof
        assert "limitations" not in row_prof
        assert row_prof["semantic_review_required"] is True
        assert row_prof["descriptive_only"] is True
        assert {"quality_score", "score", "approved", "clinical_approval"}.isdisjoint(row_prof)

    rb_compact = asyncio.run(
        _call_tool(
            mcp,
            "review_batch",
            {"qa_csv": qa_csv, "cloze_csv": cl_csv, "detail": "compact"},
        )
    )
    assert rb_compact["content_hash"] == compute_receipt(qa_csv, cl_csv)
    assert len(rb_compact["information_profiles"]) == 2
    assert rb_compact["information_guidance"]["editorial_checklist"] == list(EDITORIAL_CHECKLIST)
    assert rb_compact["information_guidance"]["limitations"] == list(BASE_LIMITATIONS)
    for row_prof in rb_compact["information_profiles"]:
        assert "editorial_checklist" not in row_prof
        assert "policy" not in row_prof
        assert row_prof["semantic_review_required"] is True

    rb_full = asyncio.run(
        _call_tool(
            mcp,
            "review_batch",
            {"qa_csv": qa_csv, "cloze_csv": cl_csv, "detail": "full"},
        )
    )
    assert len(rb_full["information_profiles"]) == 2
    assert rb_full["information_profiles"][0]["editorial_checklist"] == list(EDITORIAL_CHECKLIST)


def test_compact_batch_preserves_row_specific_limitation_without_repeating_base():
    qa_csv = (
        "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas\n"
        "Em contexto X, qual o achado?|Achado Y|Campo::Sub|Nota.||Ref|Tag::Sub"
    )
    cl_csv = (
        "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas\n"
        "Sentença sem nenhuma omissão marcada.|Nota.|Campo::Sub||Ref|Tag::Sub"
    )
    mcp = _make_server()
    vb = asyncio.run(
        _call_tool(
            mcp, "validate_batch", {"batch_id": "B01", "qa_csv": qa_csv, "cloze_csv": cl_csv}
        )
    )
    qa_row, cl_row = vb["information_profiles"]
    assert "limitations" not in qa_row
    assert any("nenhuma omissão" in lim.lower() for lim in cl_row.get("limitations", []))
    assert set(cl_row.get("limitations", [])).isdisjoint(BASE_LIMITATIONS)


def test_pipeline_gate_check_exposes_information_profiles(tmp_path):
    from anki_mcp.core.deck import save_manifesto
    from anki_mcp.models import Manifesto, TaxonomyNode

    m = Manifesto(
        deck_slug="neuro",
        campo_estudo="Medicina::Neurologia",
        created="2026-01-01",
        updated="2026-01-01",
        taxonomy=TaxonomyNode(root="Neuro", nodes=["Geral"]),
    )
    save_manifesto(tmp_path, m)
    qa_csv = (
        "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas\n"
        "Qual o fármaco de escolha na condição X?|Fármaco X|Medicina::Neurologia|||Ref|Neuro::Geral"
    )
    cl_csv = (
        "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas\n"
        "O gene {{c1::HTT}} localiza-se no braço {{c2::4p}}.|Explicação.|Medicina::Neurologia||Ref|Neuro::Geral"
    )
    mcp = _make_server()
    gate = asyncio.run(
        _call_tool(
            mcp,
            "pipeline_gate_check",
            {
                "deck_dir": str(tmp_path),
                "batch_id": "B01",
                "phase": "curated",
                "qa_csv": qa_csv,
                "cloze_csv": cl_csv,
                "detail": "compact",
            },
        )
    )
    assert gate["content_hash"] == compute_receipt(qa_csv, cl_csv)
    assert len(gate["information_profiles"]) == 2
    assert [(p["note_type"], p["row"]) for p in gate["information_profiles"]] == [
        ("QA", 2),
        ("CL", 2),
    ]


def test_tool_descriptions_and_phase_rules_prompts_reflect_three_layers_policy():
    mcp = _make_server()
    descs = asyncio.run(_list_tools_map(mcp))

    for name in (
        "validate_qa",
        "validate_cloze",
        "validate_notes",
        "validate_batch",
        "pipeline_gate_check",
        "review_batch",
    ):
        assert "information_profile" in descs[name], name

    qa_desc = descs["validate_qa"].lower()
    assert "hand off to cloze" not in qa_desc
    assert "context" in qa_desc

    for phase in ("drafted", "curated", "enriched"):
        direct = get_phase_rules(phase)
        rendered = asyncio.run(_get_prompt_text(mcp, phase))
        assert direct == rendered
        low = rendered.lower()
        assert "camada" in low or ("carga recuperada" in low and "contexto" in low)
        assert (
            "auto" in low or "automátic" in low or "não converter" in low or "sem conversão" in low
        )
        assert "lacuna" in low
        assert "mínimo" in low
        if phase in ("curated", "enriched"):
            assert "prova" in low
            assert "fonte" in low
