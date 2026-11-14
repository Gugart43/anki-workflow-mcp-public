"""tests/test_compiler.py — Unit tests for JSON draft compiler, surgical repair, and quarantine."""

from anki_mcp.core.compiler import (
    apply_patch_to_draft,
    compile_drafts_to_csv,
    partition_quarantine,
    purge_id_leak,
)
from anki_mcp.models.draft import CardDraft, CardPatch, ExtraNotes, MediaFigure


def test_purge_id_leak():
    raw = "B01-QA-01 Em pacientes com miastenia grave, qual o teste?"
    assert purge_id_leak(raw) == "Em pacientes com miastenia grave, qual o teste?"


def test_extra_notes_rendering():
    notes = ExtraNotes(
        fisiopatologia="Dano imunomediado na junção neuromuscular.",
        raciocinio_clinico="Sensibilidade > 85% para anticorpos anti-AChR.",
        armadilhas_diferenciais="Diferenciar de síndrome de Lambert-Eaton.",
    )
    rendered = notes.render_html()
    assert "• <b>Fisiopatologia &amp; Mecanismo:</b> Dano imunomediado" in rendered
    assert "• <b>Raciocínio Clínico &amp; Prática:</b>" in rendered
    assert "• <b>Armadilhas &amp; Diferenciais:</b>" in rendered
    assert "<br>" in rendered


def test_media_figure_rendering():
    fig = MediaFigure(
        filename="catarata_senil.png",
        caption="Biomicroscopia mostrando esclerose nuclear com opacificação central.",
    )
    html = fig.render_html()
    assert (
        '<img src="catarata_senil.png" alt="Biomicroscopia mostrando esclerose nuclear com'
        ' opacificação central.">' in html
    )
    assert "<figcaption>" in html
    assert "Biomicroscopia mostrando esclerose" in html
    assert "style=" not in html


def test_cloze_auto_renumbering():
    draft = CardDraft(
        tipo="CLOZE",
        enunciado_ou_sentenca="No teste de tensilon, {{c4::edrofônio}} reverte {{c2::ptose palpebral}}.",
        campo_estudo="Medicina::Neurologia",
    )
    draft.renumber_cloze()
    assert "{{c1::edrofônio}}" in draft.enunciado_ou_sentenca
    assert "{{c2::ptose palpebral}}" in draft.enunciado_ou_sentenca


def test_compile_drafts_to_csv_qa_and_cloze():
    card_qa = CardDraft(
        id="B01-QA-01",
        tipo="QA",
        enunciado_ou_sentenca="B01-QA-01 Qual anticorpo mais comum na miastenia gravis generalizada?",
        resposta="Anticorpo anti-receptor de acetilcolina (anti-AChR em > 85% dos casos).",
        campo_estudo="Medicina::Neurologia::Doenças Neuromusculares",
        notas_adicionais=ExtraNotes(
            fisiopatologia="Bloqueio competitivo e degradação acelerada dos receptores pós-sinápticos.",
            raciocinio_clinico="Dosagem de anti-AChR é o primeiro exame sorológico de escolha.",
            armadilhas_diferenciais="Soronegatividade em ~15% dos casos generalizados; testar anti-MuSK.",
        ),
        referencias=["Diretrizes ABN 2024", "Lancet Neurol 2021"],
        etiquetas=["medicina", "neurologia", "miastenia"],
    )

    card_cloze = CardDraft(
        tipo="CLOZE",
        enunciado_ou_sentenca="Na esclerose múltipla, o tratamento de primeira linha no surto agudo é {{c5::metilprednisolona}} na dose de {{c2::1 g/dia IV por 3 a 5 dias}}.",
        campo_estudo="Medicina::Neurologia::Desmielinizantes",
        notas_adicionais="• <b>Raciocínio Clínico:</b> A pulsoterapia acelera a recuperação do surto.",
        etiquetas=["medicina", "neurologia", "esclerose_multipla"],
    )

    qa_csv, cloze_csv, stats = compile_drafts_to_csv([card_qa, card_cloze])

    assert stats["qa_count"] == 1
    assert stats["cloze_count"] == 1
    assert stats["purged_id_leaks"] >= 1
    assert stats["renumbered_cloze"] == 1

    # Verify QA columns
    qa_lines = qa_csv.splitlines()
    assert len(qa_lines) == 2
    qa_headers = qa_lines[0].split("|")
    assert qa_headers == [
        "Enunciado",
        "Resposta",
        "Campo de Estudo",
        "Notas Adicionais",
        "Imagens Relacionadas",
        "Referências",
        "Etiquetas",
    ]
    # Check that leaked ID was purged from prompt/answer
    assert "B01-QA-01" not in qa_lines[1].split("|")[0]
    # Check that ID was embedded as technical tag id::B01-QA-01 in Etiquetas
    assert "id::B01-QA-01" in qa_lines[1].split("|")[-1]
    # Check HTML entity escaping for clinical comparisons (e.g. > 85% -> &gt; 85%)
    assert "&gt; 85%" in qa_lines[1]

    # Verify Cloze columns
    cloze_lines = cloze_csv.splitlines()
    assert len(cloze_lines) == 2
    cloze_headers = cloze_lines[0].split("|")
    assert cloze_headers == [
        "Sentença",
        "Notas Adicionais",
        "Campo de Estudo",
        "Imagens Relacionadas",
        "Referências",
        "Etiquetas",
    ]
    assert "{{c1::metilprednisolona}}" in cloze_lines[1]
    assert "{{c2::1 g/dia IV por 3 a 5 dias}}" in cloze_lines[1]


def test_surgical_patch_application():
    draft = CardDraft(
        tipo="QA",
        enunciado_ou_sentenca="Enunciado com erro",
        resposta="Resposta correta",
    )
    patch = CardPatch(
        updates={"enunciado_ou_sentenca": "Enunciado corrigido cirurgicamente"},
        reason="Correção pontual sem regenerar lote",
    )
    patched = apply_patch_to_draft(draft, patch)
    assert patched.enunciado_ou_sentenca == "Enunciado corrigido cirurgicamente"
    assert patched.resposta == "Resposta correta"


def test_batch_patch_draft_staging(tmp_path):
    import asyncio
    import json

    from fastmcp import FastMCP

    from anki_mcp.tools.compiler import register_compiler_tools

    staging_file = tmp_path / "drafts.jsonl"
    card1 = {"tipo": "QA", "enunciado_ou_sentenca": "Q1 original", "resposta": "A1"}
    card2 = {"tipo": "QA", "enunciado_ou_sentenca": "Q2 original", "resposta": "A2"}
    staging_file.write_text(json.dumps(card1) + "\n" + json.dumps(card2) + "\n", encoding="utf-8")

    mcp = FastMCP("test-compiler")
    register_compiler_tools(mcp)
    tool = asyncio.run(mcp.get_tool("batch_patch_draft"))
    fn = tool.fn

    res = fn(str(staging_file), 1, json.dumps({"enunciado_ou_sentenca": "Q2 cirurgico"}))
    assert res["success"] is True
    assert res["patched_card"]["enunciado_ou_sentenca"] == "Q2 cirurgico"

    lines = [
        json.loads(line) for line in staging_file.read_text(encoding="utf-8").splitlines() if line
    ]
    assert lines[0]["enunciado_ou_sentenca"] == "Q1 original"
    assert lines[1]["enunciado_ou_sentenca"] == "Q2 cirurgico"


def test_partition_quarantine():
    card1 = CardDraft(tipo="QA", enunciado_ou_sentenca="Q1", resposta="R1")
    card2 = CardDraft(tipo="QA", enunciado_ou_sentenca="Q2 com falha", resposta="R2")
    card3 = CardDraft(tipo="QA", enunciado_ou_sentenca="Q3", resposta="R3")

    valid, quarantined = partition_quarantine([card1, card2, card3], failed_indices=[1])
    assert len(valid) == 2
    assert len(quarantined) == 1
    assert quarantined[0].card.enunciado_ou_sentenca == "Q2 com falha"
    assert [c.enunciado_ou_sentenca for c in valid] == ["Q1", "Q3"]


def test_campo_estudo_aliases_and_inference():
    # 1. Aliases resolution in CardDraft
    c1 = CardDraft.model_validate(
        {
            "tipo": "QA",
            "pergunta": "Pergunta 1",
            "resposta": "Resposta 1",
            "campo_de_estudo": "Medicina::Neurologia::Neuropatias Periféricas",
        }
    )
    assert c1.campo_estudo == "Medicina::Neurologia::Neuropatias Periféricas"

    # 2. Direct compilation with explicit campo_estudo
    qa_csv, _cloze_csv, _ = compile_drafts_to_csv([c1])
    qa_row = qa_csv.splitlines()[1].split("|")
    assert qa_row[2] == "Medicina::Neurologia::Neuropatias Periféricas"

    # 3. Auto-inference from taxonomy tags when campo_estudo is empty
    c2 = CardDraft.model_validate(
        {
            "tipo": "QA",
            "pergunta": "Pergunta 2",
            "resposta": "Resposta 2",
            "tags": "Neurologia::Neuropatias_Perifericas::Topografia FocoClinico::PSD",
        }
    )
    assert c2.campo_estudo == ""
    qa_csv2, _, _ = compile_drafts_to_csv([c2])
    qa_row2 = qa_csv2.splitlines()[1].split("|")
    assert qa_row2[2] == "Medicina::Neurologia::Neuropatias Perifericas"

    # 4. Fallback to default_campo_estudo
    c3 = CardDraft.model_validate(
        {"tipo": "CLOZE", "sentenca": "Sentença {{c1::teste}}", "tags": "Geral"}
    )
    _, cl_csv3, _ = compile_drafts_to_csv([c3], default_campo_estudo="Medicina::Clinica::Geral")
    cl_row3 = cl_csv3.splitlines()[1].split("|")
    assert cl_row3[2] == "Medicina::Clinica::Geral"
