"""The Rust v2 batch review sizes by cards, not a fixed batch ceiling."""

from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
QA_HEADER = (
    "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas"
)
CL_HEADER = "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas"

try:
    from tests.differential.conftest import OracleRunner
except ImportError:
    from conftest import OracleRunner


def test_over_twenty_cards_no_longer_fail_v2(run_oracle: OracleRunner) -> None:
    qa = (
        QA_HEADER
        + "\n"
        + "\n".join(f"Pergunta {i}?|Resposta {i}|Geral|Explicação {i}|||" for i in range(21))
    )
    result = run_oracle("review_batch_information", qa_csv=qa, cloze_csv=CL_HEADER)
    assert result["total_cards"] == 21
    assert result["qa_cards"] == 21
    assert result["cloze_cards"] == 0
    assert len(result["cards"]) == 21
    assert result["semantic_review_required"] is True
    assert result.get("batch_size_limit") is None
    assert all(
        not any(d["code"] == "BATCH.OVER_LIMIT" for d in card["diagnostics"])
        for card in result["cards"]
    )


def test_default_count_operation_does_not_enforce_legacy_ceiling(run_oracle: OracleRunner) -> None:
    qa = QA_HEADER + "\n" + "\n".join(f"Pergunta {i}?|Resposta {i}|||||" for i in range(21))
    count = run_oracle("count_batch_cards", qa_csv=qa, cloze_csv=CL_HEADER, batch_id="B01")
    assert count["total"] == 21
    assert count["within_limit"] is True
    assert count["excess"] == 0
    assert count["ext_batch_suggested"] is None


def test_explicit_legacy_profile_preserves_twenty_card_ceiling(run_oracle: OracleRunner) -> None:
    qa = QA_HEADER + "\n" + "\n".join(f"Pergunta {i}?|Resposta {i}|||||" for i in range(21))
    count = run_oracle(
        "count_batch_cards",
        qa_csv=qa,
        cloze_csv=CL_HEADER,
        batch_id="B01",
        profile="legacy_compat",
    )
    assert count["total"] == 21
    assert count["within_limit"] is False
    assert count["excess"] == 1
    assert count["ext_batch_suggested"] == "B01-EXT"


def test_each_cloze_index_gets_its_own_retrieval_profile(run_oracle: OracleRunner) -> None:
    cl = CL_HEADER + "\n{{c1::alfa}} e {{c2::beta}} e {{c1::gama}}|Contexto|||Fonte|Tag"
    result = run_oracle("review_batch_information", qa_csv=QA_HEADER, cloze_csv=cl)
    assert result["qa_cards"] == 0
    assert result["cloze_cards"] == 2
    assert result["total_cards"] == 2
    assert [c["cloze_index"] for c in result["cards"]] == [1, 2]
    assert [c["retrieved_target"] for c in result["cards"]] == ["alfa ; gama", "beta"]
    assert all(c["source_row"] == 2 for c in result["cards"])
    assert all(c["notes_visible_chars"] > 0 for c in result["cards"])


def test_cloze_context_and_hidden_items_are_reviewed_per_index(run_oracle: OracleRunner) -> None:
    cl = (
        CL_HEADER + "\nO processo usa {{c1::alfa; beta; gama::etapas}} e "
        "{{c2::delta}}.|Contexto|||Fonte|Tag"
    )
    cards = run_oracle("review_batch_information", qa_csv=QA_HEADER, cloze_csv=cl)["cards"]
    assert [c["cloze_index"] for c in cards] == [1, 2]
    assert cards[0]["candidate_item_count"] == 3
    assert cards[0]["candidate_items"] == ["alfa", "beta", "gama"]
    assert cards[0]["hidden_list_signal"] is True
    assert "INFO.CLOZE_HIDDEN_LIST" in {d["code"] for d in cards[0]["diagnostics"]}
    assert cards[1]["candidate_item_count"] == 1
    assert cards[1]["hidden_list_signal"] is False
    assert "INFO.CLOZE_HIDDEN_LIST" not in {d["code"] for d in cards[1]["diagnostics"]}
    assert "[etapas]" in cards[0]["visible_context"]
    assert "delta" in cards[0]["visible_context"]
    assert "[...]." in cards[1]["visible_context"]
    assert "alfa; beta; gama" in cards[1]["visible_context"]


def test_qa_hidden_list_is_flagged_without_a_batch_ceiling(run_oracle: OracleRunner) -> None:
    qa = QA_HEADER + "\nEm população P, quais etapas?|alfa; beta; gama|Geral||||"
    card = run_oracle("review_batch_information", qa_csv=qa, cloze_csv=CL_HEADER)["cards"][0]
    assert card["candidate_items"] == ["alfa", "beta", "gama"]
    assert card["candidate_item_count"] == 3
    assert card["hidden_list_signal"] is True
    assert "QA.CARDINALITY" in {d["code"] for d in card["diagnostics"]}


def test_cloze_context_length_excludes_markup_and_tracks_each_view(
    run_oracle: OracleRunner,
) -> None:
    cl = CL_HEADER + "\nA {{c1::alfa::pista}} e {{c2::beta}}.|Explicação|||Fonte|Tag"
    cards = run_oracle("review_batch_information", qa_csv=QA_HEADER, cloze_csv=cl)["cards"]
    assert [c["visible_context"] for c in cards] == ["A [pista] e beta.", "A alfa e [...]."]
    assert [c["visible_context_chars"] for c in cards] == [len(c["visible_context"]) for c in cards]
    assert [c["prompt_visible_chars"] for c in cards] == [len("A alfa e beta.")] * 2


def test_empty_cloze_row_is_exposed_as_a_review_slot(run_oracle: OracleRunner) -> None:
    cl = CL_HEADER + "\nsem omissão|||||"
    result = run_oracle("review_batch_information", qa_csv=QA_HEADER, cloze_csv=cl)
    assert result["cloze_cards"] == 1
    assert result["cards"][0]["retrieved_target"] == ""
    assert "INFO.CLOZE_NO_INDEX" in {d["code"] for d in result["cards"][0]["diagnostics"]}


def test_cloze_target_length_warning_applies_only_to_its_card(run_oracle: OracleRunner) -> None:
    cl = CL_HEADER + "\nA {{c1::alfa}} e {{c2::" + "x" * 130 + "}}.|Explicação|||Fonte|Tag"
    cards = run_oracle("review_batch_information", qa_csv=QA_HEADER, cloze_csv=cl)["cards"]
    assert "CLOZE.TOO_LONG" not in {d["code"] for d in cards[0]["diagnostics"]}
    assert "CLOZE.TOO_LONG" in {d["code"] for d in cards[1]["diagnostics"]}


def test_cloze_notes_repetition_is_scoped_to_the_retrieved_index(run_oracle: OracleRunner) -> None:
    cl = CL_HEADER + "\nA {{c1::alfa}} e {{c2::beta}}.|alfa|||Fonte|Tag"
    cards = run_oracle("review_batch_information", qa_csv=QA_HEADER, cloze_csv=cl)["cards"]
    assert "NOTE.DUP_ANSWER_WARN" in {d["code"] for d in cards[0]["diagnostics"]}
    assert "NOTE.DUP_ANSWER_WARN" not in {d["code"] for d in cards[1]["diagnostics"]}


def test_qa_reports_required_answer_separately_from_optional_notes(
    run_oracle: OracleRunner,
) -> None:
    qa = QA_HEADER + "\nQual unidade?|Resposta|Geral|<p>Explicação opcional.</p>|||"
    result = run_oracle("review_batch_information", qa_csv=qa, cloze_csv=CL_HEADER)
    card = result["cards"][0]
    assert card["note_type"] == "QA"
    assert card["source_row"] == 2
    assert card["retrieved_target"] == "Resposta"
    assert card["retrieved_visible_chars"] > 0
    assert card["notes_visible_chars"] > 0
    assert isinstance(card["diagnostics"], list)
