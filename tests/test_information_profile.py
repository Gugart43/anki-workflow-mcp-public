"""Tests for checks/information.py — deterministic read-only 3-layer information profiler."""

from __future__ import annotations

import json

from anki_mcp.checks.information import profile_information


def test_qa_separates_context_retrieved_and_notes_and_is_json_serializable():
    stem = (
        "Em paciente adulto com diagnóstico recente de condição X e contraindicação a Y, "
        "apresentando achado Z persistente apesar da primeira medida padrão, "
        "qual é a conduta de escolha segundo a diretriz?"
    )
    answer = "Conduta Alfa"
    notes = "Mecanismo detalhado e contraste com a segunda opção."

    prof = profile_information(note_type="QA", prompt=stem, answer=answer, notes=notes)
    # Must be JSON-serializable without custom encoders
    roundtrip = json.loads(json.dumps(prof))

    assert roundtrip["note_type"] == "QA"
    assert roundtrip["semantic_review_required"] is True
    assert roundtrip["descriptive_only"] is True

    # No forbidden quality/clinical approval/source verification claims
    forbidden_keys = {"quality_score", "score", "approved", "clinical_approval", "source_verified"}
    assert forbidden_keys.isdisjoint(roundtrip.keys())

    # 3 layers clearly separated
    layers = roundtrip["layers"]
    assert layers["prompt_context"]["available"] is True
    assert layers["prompt_context"]["visible_chars"] > 100
    assert layers["retrieved_target"]["visible_chars"] == len("Conduta Alfa")
    assert layers["retrieved_target"]["candidate_items"] == ["Conduta Alfa"]
    assert layers["retrieved_target"]["candidate_item_count"] == 1
    assert layers["retrieved_target"]["hidden_list_signal"] is False
    assert layers["optional_explanation"]["visible_chars"] == len(notes)

    # Checklist with 6 editorial questions + policy + limitations
    assert len(roundtrip["editorial_checklist"]) == 6
    assert roundtrip["policy"]
    assert roundtrip["limitations"]


def test_cloze_profiles_per_index_with_latex_and_no_cross_index_summing():
    sentence = (
        r"Na cinética de ordem 1, a constante é \(k = \frac{\ln(2)}{t_{1/2}}\) "
        "para {{c1::eliminação linear}} enquanto a meia-vida permanece {{c2::constante}}."
    )
    prof = profile_information(note_type="CL", prompt=sentence, notes="Explicação complementar.")

    assert prof["note_type"] == "CL"
    rt = prof["layers"]["retrieved_target"]
    assert rt["distinct_indices"] == [1, 2]
    assert rt["card_count"] == 2
    per_idx = rt["per_index"]
    assert len(per_idx) == 2

    c1, c2 = per_idx[0], per_idx[1]
    assert c1["index"] == 1 and c1["card"] == "c1"
    assert c2["index"] == 2 and c2["card"] == "c2"
    assert c1["visible_chars"] == len("eliminação linear")
    assert c2["visible_chars"] == len("constante")
    # Must NOT sum targets across c1 and c2
    assert rt["max_target_visible_chars"] == len("eliminação linear")
    assert rt["max_target_visible_chars"] < c1["visible_chars"] + c2["visible_chars"]
    # Other index stays visible in context of c1 (c2 target 'constante' is visible on card c1)
    assert c1["visible_context_chars"] > len("constante")


def test_cloze_short_hidden_list_in_c1_triggers_advisory_regardless_of_size():
    sentence = "Os três componentes básicos do modelo são {{c1::alfa; beta; gama}}."
    prof = profile_information(note_type="CL", prompt=sentence)

    c1 = prof["layers"]["retrieved_target"]["per_index"][0]
    assert c1["visible_chars"] < 30  # very short
    assert c1["candidate_items"] == ["alfa", "beta", "gama"]
    assert c1["candidate_item_count"] == 3
    assert "semicolon" in c1["evidence"]
    assert c1["hidden_list_signal"] is True

    codes = [s["code"] for s in prof["advisory_signals"]]
    assert "INFO.CLOZE_HIDDEN_LIST" in codes
    sig = next(s for s in prof["advisory_signals"] if s["code"] == "INFO.CLOZE_HIDDEN_LIST")
    assert "independ" in sig["message"].lower()


def test_html_parser_preserves_li_and_br_and_decodes_numeric_entities():
    qa_li = profile_information(
        note_type="QA",
        prompt="Quais os três elementos?",
        answer="<ul><li>Alfa</li><li>Beta</li><li>Gama</li></ul>",
    )
    rt_li = qa_li["layers"]["retrieved_target"]
    assert rt_li["candidate_items"] == ["Alfa", "Beta", "Gama"]
    assert rt_li["candidate_item_count"] == 3
    assert "html_li" in rt_li["evidence"]

    qa_br = profile_information(
        note_type="QA",
        prompt="Quais os elementos?",
        answer="Alfa<br>Beta<br/>Gama",
    )
    rt_br = qa_br["layers"]["retrieved_target"]
    assert rt_br["candidate_items"] == ["Alfa", "Beta", "Gama"]
    assert "html_br" in rt_br["evidence"]

    qa_ent = profile_information(
        note_type="QA",
        prompt="Quais?",
        answer="Alfa&#59; Beta&#59; Gama",
    )
    assert qa_ent["layers"]["retrieved_target"]["candidate_items"] == ["Alfa", "Beta", "Gama"]


def test_avoids_false_positives_on_decimals_doses_appositives_and_vs():
    cases = [
        "12,5 mg/dia VO, em dose única, máximo 50 mg",
        "0.5 mL SC, a cada 12 horas",
        "5 mg, IV, a cada 8 horas",
        "500 mg, 12/12h, por 7 dias",
        "Glossoplegia, caracterizada por paralisia da língua, com desvio ipsilateral, sem disartria",
        "glossoplegia, caracterizada por atrofia, fraqueza e desvio",
        "Consciência preservada vs. comprometida",
        "Cefaleia (unilateral, pulsátil, moderada a grave)",
        "Pressão 140&#124;90 mmHg &#124; pulso 80 bpm",
    ]
    for ans in cases:
        prof = profile_information(
            note_type="QA", prompt="Em contexto X, qual o achado?", answer=ans
        )
        rt = prof["layers"]["retrieved_target"]
        assert rt["candidate_item_count"] == 1, (ans, rt)
        assert rt["hidden_list_signal"] is False, (ans, rt)


def test_comma_enumeration_detection_preserved():
    prof = profile_information(
        note_type="QA",
        prompt="Segundo o exemplo, quais os rótulos?",
        answer="alfa, beta e gama",
    )
    rt = prof["layers"]["retrieved_target"]
    assert rt["candidate_items"] == ["alfa", "beta", "gama"]
    assert rt["candidate_item_count"] == 3
    assert rt["hidden_list_signal"] is True


def test_cloze_prompt_context_visible_chars_strips_cloze_markup_and_hints():
    sent = "O achado {{c1::alfa::dica longa aqui}} ocorre no lobo {{c2::frontal::região}}."
    prof = profile_information(note_type="CL", prompt=sent)
    assert prof["layers"]["prompt_context"]["visible_chars"] == len(
        "O achado alfa ocorre no lobo frontal."
    )

    latex_sent = r"Na cinética, \(k = {{c1::\frac{a}{b}::fração longa}}\)."
    prof_latex = profile_information(note_type="CL", prompt=latex_sent)
    assert prof_latex["layers"]["prompt_context"]["visible_chars"] == len(
        r"Na cinética, \(k = \frac{a}{b}\)."
    )
