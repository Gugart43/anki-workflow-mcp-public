use anki_domain::Severity;
use anki_validate::quality::{evaluate_quality, prepare_quality};

use super::common::{approve_qa_review, make_qa_csv, sample_valid_qa_input};

#[test]
fn bare_image_blocks_quality_even_with_complete_approvals() {
    let mut input = sample_valid_qa_input();
    input.qa_csv = input.qa_csv.replace(
        "||Manual Sintético",
        "|<img src=\"demo_shape.png\">|Manual Sintético",
    );
    let prep = prepare_quality(&input);
    input.reviews = vec![approve_qa_review(&prep.snapshot_hash, 2)];
    let result = evaluate_quality(&input);
    assert_eq!(result.verdict, "FAIL");
    assert!(result.diagnostics.iter().any(|d| d.code == "IMG.BARE_IMG"));
}

#[test]
fn repeated_retrieval_cannot_pass_by_adding_more_bindings() {
    let mut input = sample_valid_qa_input();
    let row = input.qa_csv.lines().nth(1).unwrap().to_owned();
    input.qa_csv.push('\n');
    input.qa_csv.push_str(&row);
    let mut second = input.bindings[0].clone();
    second.source_row = 3;
    input.bindings.push(second);
    let prep = prepare_quality(&input);
    input.reviews = vec![
        approve_qa_review(&prep.snapshot_hash, 2),
        approve_qa_review(&prep.snapshot_hash, 3),
    ];
    let result = evaluate_quality(&input);
    assert_eq!(result.verdict, "FAIL");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "QUALITY.DUPLICATE_RETRIEVAL")
    );
}

#[test]
fn long_optional_notes_not_count_only_blocker() {
    let long_notes = "Explicação clínica detalhada sobre o mecanismo sintético. ".repeat(30); // > 1200 chars -> NOTE.LENGTH_WARN
    let mut input = sample_valid_qa_input();
    input.qa_csv = make_qa_csv(&[(
        "Em paciente adulto com síndrome X sintética, segundo o Manual Sintético 2026, qual é o marcador de primeira linha?",
        "Marcador Alfa",
        &long_notes,
    )]);

    let prep = prepare_quality(&input);
    assert_eq!(prep.verdict, "INCOMPLETE");
    input.reviews = vec![approve_qa_review(&prep.snapshot_hash, 2)];

    let eval = evaluate_quality(&input);
    assert!(
        eval.review.cards[0]
            .diagnostics
            .iter()
            .any(|d| d.code == "NOTE.LENGTH_WARN" && d.severity == Severity::Warn)
    );
    assert_eq!(eval.verdict, "PASS");
}
