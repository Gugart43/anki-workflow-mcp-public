//! Advisory, per-card information review for the v2 workflow.
//!
//! No numeric batch ceiling: count for reporting, and review each retrieval
//! separately. Heuristics cannot certify source fidelity or semantic atomicity.
use anki_domain::{Diagnostic, NoteType};
use anki_parser::{count_cloze_indices, find_cloze_spans, iter_data_lines, split_row};
use schemars::JsonSchema;
use serde::Serialize;

use crate::cloze::check_cloze;
use crate::csv_structure::check_csv;
use crate::hints::check_hints;
use crate::note_metrics::check_cloze_target_restatement;
use crate::notes::{check_notes, visible_char_count};
use crate::qa::{check_qa, extract_candidate_items};

fn visible_context_for_index(sentence: &str, index: usize) -> String {
    let mut context = String::with_capacity(sentence.len());
    let mut cursor = 0usize;
    for span in find_cloze_spans(sentence) {
        if span.start < cursor {
            continue;
        }
        if span.start > cursor {
            context.push_str(&sentence[cursor..span.start]);
        }
        if span.index == index {
            match &span.hint {
                Some(hint) if !hint.is_empty() => {
                    context.push('[');
                    context.push_str(hint);
                    context.push(']');
                }
                _ => context.push_str("[...]"),
            }
        } else {
            context.push_str(&span.target);
        }
        cursor = span.end;
    }
    if cursor < sentence.len() {
        context.push_str(&sentence[cursor..]);
    }
    context
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CardInformationReview {
    pub note_type: &'static str,
    pub source_row: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloze_index: Option<usize>,
    pub retrieved_target: String,
    pub retrieved_visible_chars: usize,
    pub candidate_items: Vec<String>,
    pub candidate_item_count: usize,
    pub hidden_list_signal: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_context_chars: Option<usize>,
    pub prompt_visible_chars: usize,
    pub notes_visible_chars: usize,
    pub semantic_review_required: bool,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BatchInformationReview {
    pub qa_cards: usize,
    pub cloze_cards: usize,
    pub total_cards: usize,
    pub semantic_review_required: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub cards: Vec<CardInformationReview>,
}

/// Count QA records and distinct Cloze indices without rejecting a fixed batch size.
/// Malformed rows remain visible in structural diagnostics (only), never silently dropped.
#[must_use]
pub fn review_batch_information(qa_csv: &str, cloze_csv: &str) -> BatchInformationReview {
    let mut cards = Vec::new();
    let mut diagnostics = check_csv(qa_csv, NoteType::Qa, true);
    diagnostics.extend(check_csv(cloze_csv, NoteType::Cloze, true));

    for (row, line) in iter_data_lines(qa_csv) {
        let row = row.get();
        let split = split_row(line, 7);
        if !split.structural_ok {
            // Uma linha deslocada (por exemplo `|` cru dentro do Enunciado) já
            // recebe `FMT.COLUMN_COUNT`/`FMT.FIELD_SHIFTED`: ler as colunas por
            // índice aqui atribuiria o texto de uma coluna ao diagnóstico da
            // outra (SKIPPED_STRUCTURAL, igual a `checks/csv_structure.py`).
            continue;
        }
        let fields = split.cols;
        let prompt = fields.first().map_or("", String::as_str);
        let answer = fields.get(1).map_or("", String::as_str);
        let notes = fields.get(3).map_or("", String::as_str);
        let mut issues = check_qa(prompt, answer, Some(row));
        issues.extend(check_notes(notes, answer, Some(row)));
        let candidate_items = extract_candidate_items(answer);
        let candidate_item_count = candidate_items.len();
        if candidate_item_count >= 3 {
            issues.push(
                Diagnostic::new(
                    anki_domain::DiagnosticCode::from_static("INFO.QA_HIDDEN_LIST"),
                    anki_domain::Severity::Warn,
                    "anki-qa-designer",
                    format!("Answer hides {candidate_item_count} candidate items; review semantic independence"),
                )
                .with_row(row)
                .with_field("Resposta"),
            );
        }
        cards.push(CardInformationReview {
            note_type: "QA",
            source_row: row,
            cloze_index: None,
            retrieved_target: answer.to_owned(),
            retrieved_visible_chars: visible_char_count(answer),
            candidate_items,
            candidate_item_count,
            hidden_list_signal: candidate_item_count >= 3,
            visible_context: None,
            visible_context_chars: None,
            prompt_visible_chars: visible_char_count(prompt),
            notes_visible_chars: visible_char_count(notes),
            semantic_review_required: true,
            diagnostics: issues,
        });
    }
    let qa_cards = cards.len();

    for (row, line) in iter_data_lines(cloze_csv) {
        let row = row.get();
        let split = split_row(line, 6);
        if !split.structural_ok {
            // Mesmo motivo do laço QA: o deslocamento já é reportado por `check_csv`.
            continue;
        }
        let fields = split.cols;
        let sentence = fields.first().map_or("", String::as_str);
        let notes = fields.get(1).map_or("", String::as_str);
        let indices = count_cloze_indices(sentence);
        let mut issues = check_cloze(sentence, Some(row), Some("Sentença"));
        issues.extend(check_hints(sentence, Some(row), Some("Sentença")));
        issues.extend(check_notes(notes, "", Some(row)));
        let spans = find_cloze_spans(sentence);
        // Invariantes da linha: não dependem do índice cloze, então são
        // calculados uma única vez por linha (antes: uma vez por índice).
        let notes_visible_chars = visible_char_count(notes);
        // Preserve the legacy count for malformed no-index Cloze notes, but
        // expose an empty target and its diagnostics rather than inventing one.
        if indices.is_empty() {
            cards.push(CardInformationReview {
                note_type: "CL",
                source_row: row,
                cloze_index: None,
                retrieved_target: String::new(),
                retrieved_visible_chars: 0,
                candidate_items: Vec::new(),
                candidate_item_count: 0,
                hidden_list_signal: false,
                visible_context: Some(sentence.to_owned()),
                visible_context_chars: Some(visible_char_count(sentence)),
                prompt_visible_chars: visible_char_count(sentence),
                notes_visible_chars,
                semantic_review_required: true,
                diagnostics: {
                    let mut diagnostics = issues;
                    diagnostics.push(
                        Diagnostic::new(
                            anki_domain::DiagnosticCode::from_static("INFO.CLOZE_NO_INDEX"),
                            anki_domain::Severity::Warn,
                            "anki-cloze-creator",
                            "No valid Cloze index found; review the sentence before export",
                        )
                        .with_row(row)
                        .with_field("Sentença"),
                    );
                    diagnostics
                },
            });
            continue;
        }
        let prompt_visible_chars = visible_char_count(&crate::qa::strip_cloze_markers(sentence));
        for index in indices {
            let targets: Vec<&str> = spans
                .iter()
                .filter(|span| span.index == index)
                .map(|span| span.target.as_str())
                .collect();
            let retrieved_target = targets.join(" ; ");
            let retrieved_visible_chars = targets.iter().map(|s| visible_char_count(s)).sum();
            let candidate_items: Vec<String> = targets
                .iter()
                .flat_map(|target| extract_candidate_items(target))
                .collect();
            let candidate_item_count = candidate_items.len();
            let index_prefix = format!("c{index}:");
            // Note-global checks belong to every view, but a length warning about
            // cN must not mark unrelated cards. Cloning only happens when the
            // note actually carries diagnostics.
            let mut card_issues: Vec<Diagnostic> = Vec::new();
            for diag in &issues {
                if matches!(
                    diag.code.as_str(),
                    "CLOZE.TOO_LONG" | "CARD.TOO_LONG_DELETION"
                ) && !diag.message.starts_with(&index_prefix)
                {
                    continue;
                }
                card_issues.push(diag.clone());
            }
            if let Some(diag) = check_cloze_target_restatement(notes, &retrieved_target, index, row)
            {
                card_issues.push(diag);
            }
            if candidate_item_count >= 3 {
                card_issues.push(
                    Diagnostic::new(
                        anki_domain::DiagnosticCode::from_static("INFO.CLOZE_HIDDEN_LIST"),
                        anki_domain::Severity::Warn,
                        "anki-cloze-creator",
                        format!("c{index} hides {candidate_item_count} candidate items; review semantic independence"),
                    )
                    .with_row(row)
                    .with_field("Sentença"),
                );
            }
            let visible_context = visible_context_for_index(sentence, index);
            cards.push(CardInformationReview {
                note_type: "CL",
                source_row: row,
                cloze_index: Some(index),
                retrieved_target,
                retrieved_visible_chars,
                candidate_items,
                candidate_item_count,
                hidden_list_signal: candidate_item_count >= 3,
                visible_context_chars: Some(visible_char_count(&visible_context)),
                visible_context: Some(visible_context),
                prompt_visible_chars,
                notes_visible_chars,
                semantic_review_required: true,
                diagnostics: card_issues,
            });
        }
    }
    let cloze_cards = cards.len() - qa_cards;
    BatchInformationReview {
        qa_cards,
        cloze_cards,
        total_cards: cards.len(),
        semantic_review_required: true,
        diagnostics,
        cards,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_cloze_keeps_one_review_slot_and_diagnostics() {
        let result = review_batch_information(
            anki_domain::QA_HEADER_CANONICAL,
            "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas\nsem omissão|||||",
        );
        assert_eq!(result.total_cards, 1);
        assert_eq!(result.cards[0].cloze_index, None);
        assert!(!result.cards[0].diagnostics.is_empty());
    }

    #[test]
    fn structural_diagnostics_are_reported_without_a_semantic_review_slot() {
        let qa = format!("{}\npergunta|resposta", anki_domain::QA_HEADER_CANONICAL);
        let result = review_batch_information(&qa, anki_domain::CLOZE_HEADER_CANONICAL);
        // A linha deslocada continua visível nos diagnósticos estruturais, mas
        // não vira cartão: sem colunas confiáveis não há revisão semântica possível.
        assert_eq!(result.total_cards, 0);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code.as_str() == "FMT.COLUMN_COUNT" && d.row == Some(2))
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code.as_str() == "FMT.FIELD_SHIFTED" && d.row == Some(2))
        );
    }

    #[test]
    fn shifted_qa_row_reports_only_structural_diagnostics() {
        // `|` cru dentro do Enunciado => 8 colunas. Antes da correção a leitura
        // por índice atribuía ao campo "Enunciado" o texto da coluna Resposta.
        let qa = format!(
            "{}\nQual o diagnóstico do paciente?|Cardiologia sintética|Resposta curta|Geral|Notas|Imagens|Manual Sintético (2026), Seção 1|Tag",
            anki_domain::QA_HEADER_CANONICAL
        );
        let result = review_batch_information(&qa, anki_domain::CLOZE_HEADER_CANONICAL);
        assert_eq!(result.total_cards, 0, "shifted row must not become a card");
        let codes: Vec<&str> = result.diagnostics.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(codes.len(), 2, "only structural diagnostics: {codes:?}");
        assert!(codes.contains(&"FMT.COLUMN_COUNT"));
        assert!(codes.contains(&"FMT.FIELD_SHIFTED"));
        assert!(!codes.contains(&"QA.CONTEXT_WARN"));
    }

    #[test]
    fn shifted_row_does_not_poison_the_next_well_formed_row() {
        let qa = format!(
            "{}\nQual o diagnóstico?|Cardiologia|Resposta|Campo de Estudo|Notas com quatro palavras|Imagens|Manual Sintético (2026), Seção 1|Tag\nEm adulto com síndrome sintética, qual o marcador?|Marcador alfa|Campo de Estudo|Nota|Imagens|Manual Sintético (2026), Seção 2|Tag",
            anki_domain::QA_HEADER_CANONICAL
        );
        let result = review_batch_information(&qa, anki_domain::CLOZE_HEADER_CANONICAL);
        assert_eq!(result.total_cards, 1);
        let card = &result.cards[0];
        assert_eq!(card.source_row, 3);
        // As colunas do cartão bem formado seguem os valores reais da linha 3.
        assert_eq!(card.retrieved_target, "Marcador alfa");
        assert_eq!(card.notes_visible_chars, "Nota".chars().count());
    }

    #[test]
    fn shifted_cloze_row_reports_only_structural_diagnostics() {
        let cloze = format!(
            "{}\nSentença com | cru {{{{c1::alfa}}}}|Notas|Campo de Estudo|Imagens|Manual Sintético (2026), Seção 1|Tag",
            anki_domain::CLOZE_HEADER_CANONICAL
        );
        let result = review_batch_information(anki_domain::QA_HEADER_CANONICAL, &cloze);
        assert_eq!(result.total_cards, 0);
        let codes: Vec<&str> = result.diagnostics.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(codes.len(), 2, "only structural diagnostics: {codes:?}");
        assert!(codes.contains(&"FMT.COLUMN_COUNT"));
        assert!(codes.contains(&"FMT.FIELD_SHIFTED"));
    }

    #[test]
    fn qa_hidden_list_warning_emitted_for_three_or_more_items() {
        let qa = format!(
            "{}\nQual a tríade clínica?|febre; icterícia; dor em HCD|Geral||||",
            anki_domain::QA_HEADER_CANONICAL
        );
        let result = review_batch_information(&qa, anki_domain::CLOZE_HEADER_CANONICAL);
        assert_eq!(result.qa_cards, 1);
        let card = &result.cards[0];
        assert_eq!(card.candidate_item_count, 3);
        assert!(card.hidden_list_signal);
        let hidden_diag = card
            .diagnostics
            .iter()
            .find(|d| d.code.as_str() == "INFO.QA_HIDDEN_LIST");
        assert!(
            hidden_diag.is_some(),
            "Expected INFO.QA_HIDDEN_LIST in card diagnostics"
        );
        let d = hidden_diag.unwrap();
        assert_eq!(d.severity, anki_domain::Severity::Warn);
        assert_eq!(d.field.as_deref(), Some("Resposta"));
        assert_eq!(d.owner_skill, "anki-qa-designer");
    }

    #[test]
    fn visible_context_utf8_accents_and_emoji() {
        let sentence = "Atenção 🩺: {{c1::ação}} e {{c2::reação::pista}} fim 🎉.";
        let ctx1 = visible_context_for_index(sentence, 1);
        assert_eq!(ctx1, "Atenção 🩺: [...] e reação fim 🎉.");

        let ctx2 = visible_context_for_index(sentence, 2);
        assert_eq!(ctx2, "Atenção 🩺: ação e [pista] fim 🎉.");
    }

    #[test]
    fn batch_and_card_information_json_schema() {
        let schema_batch = schemars::schema_for!(BatchInformationReview);
        let json_batch = serde_json::to_string(&schema_batch).expect("schema serialize");
        assert!(json_batch.contains("BatchInformationReview"));
        assert!(!json_batch.contains("batch_size_limit"));

        let schema_card = schemars::schema_for!(CardInformationReview);
        let json_card = serde_json::to_string(&schema_card).expect("schema serialize");
        assert!(json_card.contains("CardInformationReview"));
    }
}
