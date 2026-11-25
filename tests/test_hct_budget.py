"""tests/test_hct_budget.py — Automated regression suite for Hybrid Clinical Triage (HCT)

Tests:
1. Sectional slicing (markdown headers & paragraph clusters).
2. Pure noise & epidemiology rejection (EV = 0.0).
3. Universal Must-Cover clinical invariants (8 axes).
4. Real golden excerpts from user domains:
   - Catarata (Oftalmologia)
   - Ventilação Mecânica (Medicina Intensiva / UTI)
   - OACR (Emergência Oftalmológica)
   - ELA (Neurologia / Residência)
5. MCP tools: topic_triage_calibrate and augmented topic_budget_plan.
6. Resilient network fallback when JEV is offline or without key.
"""

from __future__ import annotations

from pathlib import Path
from unittest.mock import patch

from fastmcp import FastMCP

from anki_mcp.checks.jev_triage import (
    UNIVERSAL_INVARIANTS,
    evaluate_must_cover_invariants,
    evaluate_topic_budget,
    heuristic_section_density,
    split_into_sections,
)
from anki_mcp.models.triage import TopicBudgetEvaluation
from anki_mcp.tools.corpus import register_corpus_tools

# ── Golden Snippets ────────────────────────────────────────────────────────────

GOLDEN_CATARATA = """\
# Catarata Senil e Manejo Clínico-Cirúrgico

## Apresentação e Quadro Clínico
A catarata é a principal causa de cegueira reversível no mundo. Apresenta-se tipicamente com perda insidiosa, progressiva e indolor da acuidade visual bilateral, piora do contraste e ofuscamento (glare) sob luz forte. Ao exame físico com lâmpada de fenda, observa-se opacificação do cristalino e diminuição do reflexo vermelho na fundoscopia.

## Indicações Cirúrgicas e Procedimento Definitivo
A cirurgia de facoemulsificação com implante de lente intraocular (LIO) é o tratamento de escolha definitivo. As principais indicações formais incluem: comprometimento das atividades de vida diária, catarata induzindo hipertensão ocular (glaucoma facomórfico ou facolítico) e opacidades que impeçam o diagnóstico ou tratamento de doenças retinianas posteriores (como retinopatia diabética).

## Complicações e Conduta Tardia
A opacificação da cápsula posterior (OCP) é a complicação tardia mais comum após a facoemulsificação. O tratamento padrão-ouro é a capsulotomia posterior com YAG laser. Complicações agudas graves incluem endoftalmite infecciosa, que se manifesta com dor ocular intensa, hipópio e perda visual aguda nos primeiros dias de pós-operatório.
"""

GOLDEN_VENTILACAO_MECANICA = """\
# Princípios Fundamentais de Ventilação Mecânica

## Fisiologia Respiratória e Equação do Movimento
A pressão nas vias aéreas obedece à equação do movimento: P_total = Fluxo * Resistência + Volume / Complacência + PEEP total. A pressão de pico reflete componentes resistivos e elásticos, enquanto a pressão de platô (obtida após pausa inspiratória de 0,5 a 2 segundos) reflete puramente a elastância do parênquima pulmonar e da caixa torácica.

## Modos Ventilatórios Básicos
No modo Volume Controlado (VCV), o volume corrente (Vt) e o fluxo são fixos, sendo a pressão uma variável dependente. No modo Pressão Controlada (PCV), a pressão inspiratória e o tempo inspiratório são pré-ajustados, resultando em fluxo desacelerado variável. Na Pressão de Suporte (PSV), o disparo é deflagrado pelo esforço muscular do paciente e a ciclagem ocorre quando o fluxo inspiratório cai para uma porcentagem do pico (critério off de ciclagem).

## Ventilação Protetora na SDRA
Na Síndrome do Desconforto Respiratório Agudo (SDRA), a estratégia protetora salva vidas. As metas obrigatórias incluem: volume corrente baixo de 4 a 6 mL/kg de peso predito, pressão de platô menor ou igual a 30 cmH2O e driving pressure (Delta P = Platô - PEEP) menor ou igual a 15 cmH2O para minimizar lesão pulmonar induzida pela ventilação (VILI).
"""

GOLDEN_OACR_EMERGENCY = """\
# Oclusão da Artéria Central da Retina (OACR)

## Quadro Clínico e Semiologia
A OACR cursa com perda súbita, profunda e indolor da acuidade visual em um único olho, frequentemente reduzida a vultos ou percepção luminosa. Na fundoscopia imediata, observa-se palidez retiniana difusa por edema isquêmico e mancha vermelho-cereja na mácula (fóvea preservada nutrida pela coriocapilar).

## Conduta Imediata e Red Flags
Trata-se de uma emergência oftalmológica aguda tratada como equivalente a um AVC isquêmico. A janela para salvar a função retiniana é de até 4 a 6 horas. Medidas iniciais incluem massagem ocular para desalojar êmbolo, redução medicamentosa da pressão intraocular (acetazolamida venosa, colírios hipotensores) e encaminhamento neurológico urgente para investigação de fonte embólica carotídea ou cardíaca.
"""

GOLDEN_ELA = """\
# Esclerose Lateral Amiotrófica (ELA)

## Fisiopatologia e Mecanismos Moleculares
Doença neurodegenerativa fatal marcada pela perda concomitante de neurônios motores superiores (NMS) e inferiores (NMI). Inclusões citoplasmáticas da proteína TDP-43 estão presentes em > 95% dos pacientes. A expansão de hexanucleotídeos no gene C9orf72 é a causa genética mais frequente de ELA e demência frontotemporal.

## Quadro Clínico e Semiologia
Início insidioso e assimétrico, caracterizado por fraqueza progressiva, atrofia muscular e fasciculações (NMI) associadas a hiperreflexia, sinal de Babinski e espasticidade (NMS). Sensibilidade, motilidade ocular extrínseca e controle esfincteriano são classicamente preservados.

## Critérios Diagnósticos e Confirmação
O diagnóstico apoia-se nos critérios de Gold Coast: disfunção motora progressiva com sinais documentados de NMS e NMI em pelo menos uma região corporal, ou sinais de NMI em duas ou mais regiões. A eletroneuromiografia (ENMG) confirma desnervação ativa e crônica difusa. A ressonância magnética de encéfalo e medula é mandatória para descartar mimetizadores.

## Diagnóstico Diferencial
Mimetizadores graves incluem espondilopatia cervical mielopática, polineuropatia desmielinizante inflamatória crônica (CIDP) e síndrome pós-pólio. Fraqueza motora com alterações sensitivas proeminentes afasta o diagnóstico de ELA típica.

## Tratamento Modificador e Cuidados de Suporte
O riluzol (50 mg 2x/dia) prolonga a sobrevida em 2 a 3 meses e reduz a dependência de traqueostomia. O suporte ventilatório com ventilação não invasiva (VNI) é indicado precocemente quando a capacidade vital forçada (CVF) cai abaixo de 50%. A gastrostomia endoscópica percutânea (PEG) previne broncoaspiração e perda ponderal grave.
"""


# ── Tests ──────────────────────────────────────────────────────────────────────


def test_split_into_sections():
    """Verify markdown sections are parsed cleanly into SectionNodes with headings."""
    sections = split_into_sections(GOLDEN_CATARATA)
    assert len(sections) == 3
    assert sections[0].title == "Apresentação e Quadro Clínico"
    assert sections[1].title == "Indicações Cirúrgicas e Procedimento Definitivo"
    assert sections[2].title == "Complicações e Conduta Tardia"
    assert all(s.token_estimate > 0 for s in sections)
    assert sections[0].id == "sec_01"
    assert sections[1].id == "sec_02"
    assert sections[2].id == "sec_03"


def test_heuristic_density_rejects_pure_epidemiology_and_chatter():
    """Epidemiological trivia and assistant chatter receive EV = 0.0."""
    pure_epi = (
        "A doença de Huntington foi descrita por George Huntington em 1872 em Long Island. "
        "A prevalência varia na Europa entre 5 e 10 casos por 100 mil habitantes no mundo moderno."
    )
    score, conf, _primary, probs = heuristic_section_density(
        pure_epi, title="História e Epidemiologia"
    )
    assert score == 0.0
    assert conf >= 0.9
    assert probs.get("0", 0.0) == 1.0


def test_heuristic_density_scores_dense_treatment_section():
    """Treatment section with dosages, drugs and cutoffs gets elevated EV."""
    text = (
        "O tratamento de primeira linha inclui riluzol na dose de 50 mg a cada 12 horas. "
        "Ventilação não invasiva deve ser iniciada quando CVF estiver abaixo de 50% ou PIMax menor que 60 cmH2O. "
        "Efeitos adversos graves exigem monitorização de transaminases a cada 3 meses."
    )
    score, _conf, primary, _probs = heuristic_section_density(text, title="Terapêutica e Suporte")
    assert score >= 2.0
    assert primary in ("tratamento_1a_linha", "seguranca_farmacologica")


def test_universal_invariants_coverage_and_gap_detection():
    """Checks 8 universal clinical invariants, detecting COVERED axes and GAPs."""
    sections = split_into_sections(GOLDEN_OACR_EMERGENCY)
    invariants, gaps = evaluate_must_cover_invariants(sections)

    assert len(invariants) == len(UNIVERSAL_INVARIANTS)
    status_map = {inv.slug: inv.status for inv in invariants}

    assert status_map["reconhecimento"] == "COVERED"
    assert status_map["red_flags"] == "COVERED"
    assert status_map["tratamento_1a_linha"] == "COVERED"
    # Diferencial may be absent in this focused emergency snippet -> correctly reported as GAP
    assert "GAP.DIFERENCIAL_CRITICO" in gaps


def test_evaluate_topic_budget_catarata_golden():
    """Verify topic budget evaluation on Catarata produces balanced EV and batch plan."""
    evaluation = evaluate_topic_budget(
        text=GOLDEN_CATARATA,
        topic="Catarata",
        domain="Oftalmologia",
        use_jev=False,  # deterministic mode for CI
    )
    assert isinstance(evaluation, TopicBudgetEvaluation)
    assert evaluation.total_sections == 3
    assert evaluation.total_planned_cards >= 5
    assert evaluation.batches_needed == 1
    assert evaluation.batches_planned == ["B01"]
    assert evaluation.engine == "deterministic_fallback"
    assert "Orçamento Triangulado" in evaluation.raw_summary


def test_evaluate_topic_budget_ela_multibatch_partitioning():
    """Verify that a comprehensive topic (ELA) partitions properly into discrete batches <= 20."""
    evaluation = evaluate_topic_budget(
        text=GOLDEN_ELA,
        topic="Esclerose Lateral Amiotrófica",
        domain="Neurologia",
        batch_size=5,  # small batch size to test multi-batch horizon
        use_jev=False,
    )
    assert evaluation.total_sections == 5
    assert evaluation.total_planned_cards >= 8
    assert evaluation.batches_needed >= 2
    assert "B01" in evaluation.batches_planned
    assert "B02" in evaluation.batches_planned


def test_evaluate_topic_budget_umbrella_scope_and_audience():
    """Verify that an umbrella topic with multiple subentities calibrates to R1 core (16-24) vs Full."""
    umbrella_text = """# Neuropatias Periféricas
## Abordagem Sindrômica
Apresenta-se com perda sensitiva em bota e luva e arreflexia.
## Síndrome de Guillain-Barré
Paralisia flácida arreflexa aguda com dissociação proteinocitológica. Tratamento com IVIG ou plasmaférese. Corticoide contraindicado.
## PIDC
Polirradiculoneuropatia crônica desmielinizante com evolução maior que 8 semanas. Corticoterapia eficaz.
## Síndrome do Túnel do Carpo
Compressão do nervo mediano no punho. Manobras de Phalen e Tinel positivas. Parestesias nos quirodáctilos 1 a 3.
## Neuropatia Diabética
Polineuropatia simétrica distal comprimento-dependente. Rastreio com monofilamento de 10g e controle glicêmico.
## Neuropatia da Hanseníase
Perda térmica e dolorosa precoce com espessamento dos nervos ulnar e fibular comum. Tratamento com PQT-U no SUS.
"""
    # 1. R1 Generalista focus on brief summary (flags gaps against elastic range 16-32)
    r1_eval = evaluate_topic_budget(
        text=umbrella_text,
        topic="Neuropatias Periféricas",
        target_audience="R1_GENERALISTA",
        use_jev=False,
    )
    assert r1_eval.cognitive_zone == "ZONA_SINDROMICA"
    assert r1_eval.taxonomy_scope == "UMBRELLA_SYNDROME"
    assert len(r1_eval.suggested_splits) >= 4
    assert r1_eval.elastic_range == (16, 32)
    assert r1_eval.total_planned_cards == 10  # matches available content density in short snippet
    assert r1_eval.saturation_status == "DEFICIT"
    assert r1_eval.action_required == "request_more_source"
    assert len(r1_eval.gaps) >= 3  # identifies missing invariant axes
    assert r1_eval.batches_needed == 1
    assert "ZONA_SINDROMICA" in r1_eval.raw_summary

    # 1b. When full dense content is provided, stays in elastic range (16-32 cards) rather than bloating to 50+
    dense_umbrella_text = umbrella_text * 4
    dense_eval = evaluate_topic_budget(
        text=dense_umbrella_text,
        topic="Neuropatias Periféricas",
        target_audience="R1_GENERALISTA",
        use_jev=False,
    )
    assert 16 <= dense_eval.total_planned_cards <= 32
    assert dense_eval.batches_needed <= 2
    assert dense_eval.saturation_status == "OPTIMAL"

    # 2. Micro-acute emergency
    acute_eval = evaluate_topic_budget(
        text=GOLDEN_OACR_EMERGENCY,
        topic="OACR Aguda",
        use_jev=False,
    )
    assert acute_eval.cognitive_zone == "ZONA_FOCAL"
    assert acute_eval.taxonomy_scope == "MICRO_ACUTE"
    assert acute_eval.elastic_range == (5, 14)
    assert acute_eval.total_planned_cards <= 14


def _get_fn(mcp: FastMCP, name: str):
    import asyncio

    tool = asyncio.run(mcp.get_tool(name))
    return tool.fn


def test_topic_triage_calibrate_mcp_tool(tmp_path: Path):
    """Test FastMCP registration and execution of topic_triage_calibrate tool."""
    mcp = FastMCP("test-hct")
    register_corpus_tools(mcp)

    fn = _get_fn(mcp, "topic_triage_calibrate")
    assert fn is not None

    res = fn(text=GOLDEN_VENTILACAO_MECANICA, label="Ventilacao_Mecanica", use_jev=False)
    assert res["status"] == "OK"
    assert res["total_sections"] == 3
    assert res["total_planned_cards"] > 0
    assert "ev_decisions" in res
    assert "ev_structural" in res
    assert "convergence" in res
    assert "saturation_status" in res
    assert "action_required" in res
    assert len(res["must_cover_matrix"]) == 8
    assert "B01" in res["batches_planned"]
    assert res["domain"] == "Medicina"


def test_triangulated_median_and_convergence():
    """Verify that EV synthesis uses median of 3 estimators, floor guarantee, and computes convergence."""
    eval_res = evaluate_topic_budget(
        text=GOLDEN_ELA,
        topic="Esclerose Lateral Amiotrófica",
        domain="Neurologia",
        use_jev=False,
    )
    # Under deterministic fallback, candidates are [ev_decisions, ev_structural]
    assert eval_res.degraded is True
    assert eval_res.convergence == "DEGRADED_FALLBACK"
    assert eval_res.ev_decisions > 0
    assert eval_res.ev_structural > 0
    assert eval_res.ev_floor >= 5  # at least 5 universal invariants in ELA golden
    assert eval_res.total_ev >= eval_res.ev_floor


def test_a_cascade_and_pruned_sections_audit():
    """Verify that dense sections undergo A-Cascade subdivision and noise sections are auditably logged."""
    text_with_noise = """# Manejo Clínico Completo
## História e Epidemiologia Antiga
A prevalência varia de acordo com estudos epidemiológicos apontam que foi descrita em 1869 por Charcot. Afeta cerca de 2 pessoas por 100.000.
## Seção Muito Densa de Condutas e Critérios
Critério de diagnóstico: presença de sinais de primeiro e segundo neurônio motor com padrão ouro por eletroneuromiografia mostrando desnervação ativa e crônica com fibrilações.

Conduta imediata no suporte ventilatório: ventilação não invasiva com indicação quando CVF menor que 50% ou PImáx menor que 60 cmH2O. Reduz mortalidade e melhora sobrevida.

Manejo farmacológico específico: Riluzol 50 mg de 12 em 12 horas bloqueando liberação glutamatérgica com monitorização de transaminases hepáticas bimestrais nos primeiros 6 meses.
"""
    eval_res = evaluate_topic_budget(
        text=text_with_noise,
        topic="Teste ELA",
        domain="Neurologia",
        use_jev=False,
    )
    assert len(eval_res.pruned_sections) >= 1
    pruned = eval_res.pruned_sections[0]
    assert pruned.reason == "LOW_DENSITY_EPIDEMIOLOGY"
    assert "História" in pruned.title
    assert eval_res.actionability_pruned_count >= 1


def test_topic_budget_plan_augmented_with_must_cover(tmp_path: Path):
    """Test that legacy topic_budget_plan now returns augmented must_cover_matrix and gaps."""
    mcp = FastMCP("test-legacy-augmented")
    register_corpus_tools(mcp)

    fn = _get_fn(mcp, "topic_budget_plan")
    assert fn is not None

    res = fn(deck_dir=str(tmp_path), text=GOLDEN_CATARATA)
    assert res["status"] == "OK"
    assert "must_cover_matrix" in res
    assert "must_cover_gaps" in res
    assert "must_cover_covered_count" in res
    assert res["must_cover_covered_count"] >= 3


def test_jev_network_fallback_on_error():
    """Ensure network failures or API exceptions fall back gracefully without crashing."""
    # B30: o cliente SystemOne passou a usar um opener próprio (redirects
    # recusados), então o seam mockável é `_open_systemone`, não `urlopen`.
    with patch(
        "anki_mcp.checks.jev_triage._open_systemone",
        side_effect=Exception("Connection refused"),
    ):
        eval_res = evaluate_topic_budget(
            text=GOLDEN_OACR_EMERGENCY,
            topic="OACR",
            use_jev=True,
            api_key="dummy_key_to_force_call",
        )
        assert eval_res.total_sections > 0
        assert eval_res.total_planned_cards > 0
        assert eval_res.engine == "deterministic_fallback"
        assert eval_res.degraded is True


def test_ev_structural_isolated():
    """Verify that compute_structural_ev evaluates bullets, cutoffs and tables independently."""
    from anki_mcp.checks.jev_triage import compute_structural_ev

    text = """# Seção de Teste
* Conduta A: metilprednisolona 1 g/dia
* Conduta B: furosemida 40 mg IV
* Conduta C: manter PAM >= 65 mmHg
| Droga | Dose | Alvo |
| --- | --- | --- |
| Dopa | 5 mcg | 65 |
"""
    ev = compute_structural_ev(text, sections_count=1)
    assert ev >= 4.0  # 1 section + 3 bullets + 3 cutoffs + 1 table


def test_saturation_status_three_cases():
    """Verify that saturation_status correctly identifies DEFICIT, OPTIMAL, and SURPLUS."""
    # 1. Deficit (brief summary of Catarata resulting in 5 cards in ZONA_NUCLEAR 10-22)
    def_res = evaluate_topic_budget(GOLDEN_CATARATA, topic="Catarata Senil", use_jev=False)
    assert def_res.saturation_status == "DEFICIT"
    assert def_res.source_incomplete is True
    assert def_res.action_required == "request_more_source"

    # 2. Optimal (acute emergency OACR resulting in 5 cards within ZONA_FOCAL 5-14)
    opt_res = evaluate_topic_budget(GOLDEN_OACR_EMERGENCY, topic="OACR Aguda", use_jev=False)
    assert opt_res.saturation_status == "OPTIMAL"
    assert opt_res.action_required == "proceed_normal"

    # 3. Surplus (large dense text exceeding zone upper bound)
    dense_text = (GOLDEN_VENTILACAO_MECANICA + "\n\n" + GOLDEN_ELA) * 4
    surp_res = evaluate_topic_budget(
        dense_text,
        topic="Ventilação e ELA Multissistêmica",
        target_audience="FULL_COMPREHENSIVE",
        use_jev=False,
    )
    assert surp_res.saturation_status == "SURPLUS"
    assert surp_res.source_exceeds_zone is True
    assert surp_res.action_required in ("split_topic", "prioritize_core")


def test_offline_determinism_idempotence():
    """Verify that calling evaluate_topic_budget twice offline returns identical numbers."""
    r1 = evaluate_topic_budget(GOLDEN_ELA, topic="ELA", use_jev=False)
    r2 = evaluate_topic_budget(GOLDEN_ELA, topic="ELA", use_jev=False)
    assert r1.total_planned_cards == r2.total_planned_cards
    assert r1.total_ev == r2.total_ev
    assert r1.ev_decisions == r2.ev_decisions
    assert r1.ev_structural == r2.ev_structural
    assert r1.saturation_status == r2.saturation_status
    assert r1.action_required == r2.action_required
    assert len(r1.pruned_sections) == len(r2.pruned_sections)
