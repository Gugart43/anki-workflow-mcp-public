//! Advisory, per-card information review for the v2 workflow.
//!
//! No numeric batch ceiling: count for reporting, and review each retrieval
//! separately. Heuristics cannot certify source fidelity or semantic atomicity.
use anki_domain::{Diagnostic, NoteType};
use anki_parser::{count_cloze_indices, find_cloze_spans, iter_data_lines, split_row};
use serde::Serialize;

use crate::cloze::check_cloze;
use crate::csv_structure::check_csv;
use crate::hints::check_hints;
use crate::note_metrics::check_cloze_target_restatement;
use crate::notes::{check_notes, visible_char_count};
use crate::qa::{check_qa, extract_candidate_items};

fn visible_context_for_index(sentence: &str, index: usize) -> String {
    let chars: Vec<char> = sentence.chars().collect();
    let mut context = String::new();
    let mut cursor = 0;
    for span in find_cloze_spans(sentence) {
        if span.start < cursor {
            continue;
        }
        context.extend(&chars[cursor..span.start]);
        if span.index == index {
            match span.hint {
                Some(hint) if !hint.is_empty() => {
                    context.push('[');
                    context.push_str(&hint);
                    context.push(']');
                }
                _ => context.push_str("[...]"),
            }
        } else {
            context.push_str(&span.target);
        }
        cursor = span.end;
    }
    context.extend(&chars[cursor..]);
    context
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub struct BatchInformationReview {
    pub qa_cards: usize,
    pub cloze_cards: usize,
    pub total_cards: usize,
    pub batch_size_limit: Option<usize>,
    pub semantic_review_required: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub cards: Vec<CardInformationReview>,
}

/// Count QA records and distinct Cloze indices without rejecting a fixed batch size.
/// Malformed rows remain visible in structural diagnostics, not silently dropped.
#[must_use]
pub fn review_batch_information(qa_csv: &str, cloze_csv: &str) -> BatchInformationReview {
    let mut cards = Vec::new();
    let mut diagnostics = check_csv(qa_csv, NoteType::Qa, true);
    diagnostics.extend(check_csv(cloze_csv, NoteType::Cloze, true));

    for (row, line) in iter_data_lines(qa_csv) {
        let row = row.get();
        let fields = split_row(line, 7).cols;
        let prompt = fields.first().map_or("", String::as_str);
        let answer = fields.get(1).map_or("", String::as_str);
        let notes = fields.get(3).map_or("", String::as_str);
        let mut issues = check_qa(prompt, answer, Some(row));
        issues.extend(check_notes(notes, answer, Some(row)));
        let candidate_items = extract_candidate_items(answer);
        let candidate_item_count = candidate_items.len();
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
        let fields = split_row(line, 6).cols;
        let sentence = fields.first().map_or("", String::as_str);
        let notes = fields.get(1).map_or("", String::as_str);
        let indices = count_cloze_indices(sentence);
        let mut issues = check_cloze(sentence, Some(row), Some("Sentença"));
        issues.extend(check_hints(sentence, Some(row), Some("Sentença")));
        issues.extend(check_notes(notes, "", Some(row)));
        let spans = find_cloze_spans(sentence);
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
                notes_visible_chars: visible_char_count(notes),
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
            let mut card_issues: Vec<Diagnostic> = issues
                .iter()
                .filter(|d| {
                    // Note-global checks belong to every view, but a length
                    // warning about cN must not mark unrelated cards.
                    !matches!(d.code.as_str(), "CLOZE.TOO_LONG" | "CARD.TOO_LONG_DELETION")
                        || d.message.starts_with(&format!("c{index}:"))
                })
                .cloned()
                .collect();
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
                prompt_visible_chars: visible_char_count(&crate::qa::strip_cloze_markers(sentence)),
                notes_visible_chars: visible_char_count(notes),
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
        batch_size_limit: None,
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
    fn structural_diagnostics_are_reported_without_dropping_the_row() {
        let qa = format!("{}\npergunta|resposta", anki_domain::QA_HEADER_CANONICAL);
        let result = review_batch_information(&qa, anki_domain::CLOZE_HEADER_CANONICAL);
        assert_eq!(result.total_cards, 1);
        assert!(!result.diagnostics.is_empty());
        assert_eq!(result.cards[0].source_row, 2);
    }
}
