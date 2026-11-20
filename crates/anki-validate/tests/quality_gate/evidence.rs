use anki_domain::Severity;
use anki_validate::quality::{QualityEvidence, QualitySource, evaluate_quality, prepare_quality};

use super::common::{approve_qa_review, has_quality_diag, sample_valid_qa_input};

#[test]
fn bogus_quote_fails_and_no_case_whitespace_html_normalization() {
    // Completely absent quote
    let mut bogus = sample_valid_qa_input();
    bogus.bindings[0].evidence[0].quote = "Texto inventado que não existe na fonte".to_owned();
    let r_bogus = prepare_quality(&bogus);
    assert_eq!(r_bogus.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_bogus,
        "QUALITY.EVIDENCE_QUOTE_NOT_FOUND",
        Severity::Critical
    ));

    // Case mismatch must FAIL
    let mut case_diff = sample_valid_qa_input();
    case_diff.bindings[0].evidence[0].quote =
        "marcador alfa constitui o parâmetro de primeira linha".to_owned();
    assert_eq!(prepare_quality(&case_diff).verdict, "FAIL");

    // Whitespace mismatch must FAIL
    let mut ws_diff = sample_valid_qa_input();
    ws_diff.bindings[0].evidence[0].quote =
        "Marcador Alfa  constitui o parâmetro de primeira linha".to_owned();
    assert_eq!(prepare_quality(&ws_diff).verdict, "FAIL");

    // HTML tag mismatch must FAIL
    let mut html_diff = sample_valid_qa_input();
    html_diff.bindings[0].evidence[0].quote =
        "<b>Marcador Alfa</b> constitui o parâmetro de primeira linha".to_owned();
    assert_eq!(prepare_quality(&html_diff).verdict, "FAIL");
}

#[test]
fn wrong_evidence_source_is_not_accepted() {
    let mut input = sample_valid_qa_input();
    input.sources.push(QualitySource {
        id: "S2".to_owned(),
        title: "Outro Manual".to_owned(),
        version: "2026.1".to_owned(),
        locator: "Seção 9".to_owned(),
        text: "Outro excerto válido apenas na fonte S2.".to_owned(),
    });
    // Obligation O1 requires S1, but binding only supplies retrieval evidence from S2!
    input.bindings[0].evidence = vec![QualityEvidence {
        source_id: "S2".to_owned(),
        quote: "Outro excerto válido apenas na fonte S2.".to_owned(),
        locator: "Seção 9".to_owned(),
        scope: "retrieval".to_owned(),
    }];

    let prep = prepare_quality(&input);
    input.reviews = vec![approve_qa_review(&prep.snapshot_hash, 2)];
    let eval = evaluate_quality(&input);
    assert_eq!(eval.verdict, "INCOMPLETE");
    assert!(has_quality_diag(
        &eval,
        "QUALITY.MISSING_OBLIGATION_SOURCE_EVIDENCE",
        Severity::Warn
    ));

    // Context-only evidence (not retrieval) for S1 also cannot pass
    input.bindings[0].evidence = vec![QualityEvidence {
        source_id: "S1".to_owned(),
        quote: "Marcador Alfa constitui o parâmetro de primeira linha".to_owned(),
        locator: "Seção 2.1".to_owned(),
        scope: "context".to_owned(),
    }];
    let prep_ctx = prepare_quality(&input);
    input.reviews = vec![approve_qa_review(&prep_ctx.snapshot_hash, 2)];
    let eval_ctx = evaluate_quality(&input);
    assert_eq!(eval_ctx.verdict, "INCOMPLETE");
    assert!(has_quality_diag(
        &eval_ctx,
        "QUALITY.MISSING_RETRIEVAL_EVIDENCE",
        Severity::Warn
    ));
}

#[test]
fn invalid_evidence_scope_and_multi_source_obligation_enforced() {
    let mut input = sample_valid_qa_input();
    input.sources.push(QualitySource {
        id: "S2".to_owned(),
        title: "Manual Complementar".to_owned(),
        version: "2026.2".to_owned(),
        locator: "Seção 4".to_owned(),
        text: "Critério secundário confirmado no Manual Complementar.".to_owned(),
    });
    input.obligations[0].source_ids = vec!["S1".to_owned(), "S2".to_owned()];

    // Only S1 retrieval evidence provided -> incomplete because S2 is missing
    let prep1 = prepare_quality(&input);
    input.reviews = vec![approve_qa_review(&prep1.snapshot_hash, 2)];
    let eval1 = evaluate_quality(&input);
    assert_eq!(eval1.verdict, "INCOMPLETE");
    assert!(has_quality_diag(
        &eval1,
        "QUALITY.MISSING_OBLIGATION_SOURCE_EVIDENCE",
        Severity::Warn
    ));

    // Invalid evidence scope -> FAIL
    let mut bad_scope = input.clone();
    bad_scope.bindings[0].evidence[0].scope = "invalid_scope".to_owned();
    assert_eq!(prepare_quality(&bad_scope).verdict, "FAIL");
    assert!(has_quality_diag(
        &prepare_quality(&bad_scope),
        "QUALITY.INVALID_EVIDENCE_SCOPE",
        Severity::Critical
    ));

    // Supply S2 retrieval evidence too -> PASS
    input.bindings[0].evidence.push(QualityEvidence {
        source_id: "S2".to_owned(),
        quote: "Critério secundário confirmado no Manual Complementar.".to_owned(),
        locator: "Seção 4".to_owned(),
        scope: "retrieval".to_owned(),
    });
    let prep2 = prepare_quality(&input);
    input.reviews = vec![approve_qa_review(&prep2.snapshot_hash, 2)];
    let eval2 = evaluate_quality(&input);
    assert_eq!(eval2.verdict, "PASS");
}
