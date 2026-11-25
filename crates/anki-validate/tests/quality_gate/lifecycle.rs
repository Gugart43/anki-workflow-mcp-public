use anki_domain::{Severity, compute_receipt_v1};
use anki_validate::quality::{
    QualityMode, QualityObligation, QualityReport, evaluate_quality, prepare_quality,
};

use super::common::{approve_qa_review, has_quality_diag, make_qa_csv, sample_valid_qa_input};

#[test]
fn full_valid_qa_passes_after_snapshot_and_review() {
    let mut input = sample_valid_qa_input();
    // Even if a stale/fake review is passed to prepare_quality, prepare ignores reviews.
    input.reviews.push(approve_qa_review("sha256:stale", 2));

    let prep = prepare_quality(&input);
    assert_eq!(prep.schema_version, "quality_v1");
    assert_eq!(prep.verdict, "INCOMPLETE");
    assert!(!prep.clinical_correctness_certified);
    assert!(!prep.export_authorized);
    assert!(prep.snapshot_hash.starts_with("sha256:"));
    assert_eq!(prep.snapshot_hash.len(), 71);
    assert_eq!(
        prep.content_hash,
        compute_receipt_v1(&input.qa_csv, &input.cloze_csv).into_inner()
    );
    assert_eq!(prep.retrieval_count, 1);
    assert_eq!(prep.reviewed_count, 0);
    assert_eq!(prep.obligation_count, 1);
    assert_eq!(prep.covered_obligation_count, 1);
    assert!(!prep.guidance.is_empty());

    input.reviews = vec![approve_qa_review(&prep.snapshot_hash, 2)];
    let eval = evaluate_quality(&input);
    assert_eq!(eval.verdict, "PASS");
    assert_eq!(eval.snapshot_hash, prep.snapshot_hash);
    assert!(!eval.clinical_correctness_certified);
    assert!(!eval.export_authorized);
    assert_eq!(eval.retrieval_count, 1);
    assert_eq!(eval.reviewed_count, 1);
    assert_eq!(eval.obligation_count, 1);
    assert_eq!(eval.covered_obligation_count, 1);
    assert!(
        !eval
            .diagnostics
            .iter()
            .any(|d| matches!(d.severity, Severity::Critical | Severity::Warn))
    );
}

#[test]
fn no_review_is_incomplete() {
    let input = sample_valid_qa_input();
    let eval = evaluate_quality(&input);
    assert_eq!(eval.verdict, "INCOMPLETE");
    assert_eq!(eval.reviewed_count, 0);
    assert!(has_quality_diag(
        &eval,
        "QUALITY.MISSING_REVIEW",
        Severity::Warn
    ));
}

#[test]
fn mutated_csv_source_or_evidence_invalidates_prior_reviews() {
    let mut base = sample_valid_qa_input();
    let prep = prepare_quality(&base);
    base.reviews = vec![approve_qa_review(&prep.snapshot_hash, 2)];
    assert_eq!(evaluate_quality(&base).verdict, "PASS");

    // 1. Mutate CSV
    let mut mutated_csv = base.clone();
    mutated_csv.qa_csv = make_qa_csv(&[(
        "Em paciente adulto com síndrome X sintética, segundo o Manual Sintético 2026, qual é o marcador principal?",
        "Marcador Alfa",
        "Explicação clínica sintética sobre o marcador.",
    )]);
    let r_csv = evaluate_quality(&mutated_csv);
    assert_ne!(r_csv.snapshot_hash, prep.snapshot_hash);
    assert_eq!(r_csv.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_csv,
        "QUALITY.STALE_REVIEW_SNAPSHOT",
        Severity::Critical
    ));

    // 2. Mutate source text (quote still present)
    let mut mutated_src = base.clone();
    mutated_src.sources[0].text.push_str(" Nota adicional.");
    let r_src = evaluate_quality(&mutated_src);
    assert_ne!(r_src.snapshot_hash, prep.snapshot_hash);
    assert_eq!(r_src.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_src,
        "QUALITY.STALE_REVIEW_SNAPSHOT",
        Severity::Critical
    ));

    // 3. Mutate evidence locator
    let mut mutated_ev = base;
    mutated_ev.bindings[0].evidence[0].locator = "Seção 2.1, p. 5".to_owned();
    let r_ev = evaluate_quality(&mutated_ev);
    assert_ne!(r_ev.snapshot_hash, prep.snapshot_hash);
    assert_eq!(r_ev.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_ev,
        "QUALITY.STALE_REVIEW_SNAPSHOT",
        Severity::Critical
    ));
}

#[test]
fn missing_obligation_coverage_is_incomplete() {
    let mut input = sample_valid_qa_input();
    input.obligations.push(QualityObligation {
        id: "O2".to_owned(),
        description: "Obrigação ainda não coberta por nenhum cartão.".to_owned(),
        source_ids: vec!["S1".to_owned()],
    });
    let prep = prepare_quality(&input);
    input.reviews = vec![approve_qa_review(&prep.snapshot_hash, 2)];

    let eval = evaluate_quality(&input);
    assert_eq!(eval.verdict, "INCOMPLETE");
    assert_eq!(eval.obligation_count, 2);
    assert_eq!(eval.covered_obligation_count, 1);
    assert!(has_quality_diag(
        &eval,
        "QUALITY.UNCOVERED_OBLIGATION",
        Severity::Warn
    ));

    // Empty obligations list -> incomplete
    let mut empty_obl = sample_valid_qa_input();
    empty_obl.obligations.clear();
    empty_obl.bindings.clear();
    let r_empty = prepare_quality(&empty_obl);
    assert_eq!(r_empty.verdict, "INCOMPLETE");
    assert!(has_quality_diag(
        &r_empty,
        "QUALITY.NO_OBLIGATIONS",
        Severity::Warn
    ));
}

#[test]
fn crlf_and_lf_produce_consistent_hashes_and_quote_match() {
    let lf_input = sample_valid_qa_input();
    let prep_lf = prepare_quality(&lf_input);

    let mut crlf_input = lf_input.clone();
    crlf_input.qa_csv = lf_input.qa_csv.replace('\n', "\r\n");
    crlf_input.cloze_csv = lf_input.cloze_csv.replace('\n', "\r\n");
    let prep_crlf = prepare_quality(&crlf_input);

    assert_eq!(prep_lf.snapshot_hash, prep_crlf.snapshot_hash);
    assert_eq!(prep_lf.content_hash, prep_crlf.content_hash);

    // Also CRLF in source text vs LF in evidence quote
    crlf_input.sources[0].text =
        "Primeira linha.\r\nMarcador Alfa constitui o parâmetro de primeira linha.".to_owned();
    crlf_input.bindings[0].evidence[0].quote = "Primeira linha.\nMarcador Alfa".to_owned();
    let prep_src_crlf = prepare_quality(&crlf_input);
    crlf_input.reviews = vec![approve_qa_review(&prep_src_crlf.snapshot_hash, 2)];
    let eval_crlf = evaluate_quality(&crlf_input);
    assert_eq!(eval_crlf.verdict, "PASS");
}

#[test]
fn revise_verdict_and_false_dimension_fail() {
    let mut input = sample_valid_qa_input();
    let prep = prepare_quality(&input);

    // Verdict revise fails even when all dimensions are true
    let mut rev_revise = approve_qa_review(&prep.snapshot_hash, 2);
    rev_revise.verdict = "revise".to_owned();
    input.reviews = vec![rev_revise];
    let r_revise = evaluate_quality(&input);
    assert_eq!(r_revise.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_revise,
        "QUALITY.REVIEW_REVISE",
        Severity::Critical
    ));

    // Any false dimension fails even with verdict approve
    let mut rev_false_dim = approve_qa_review(&prep.snapshot_hash, 2);
    rev_false_dim.dimensions.source_fidelity = false;
    input.reviews = vec![rev_false_dim];
    let r_dim = evaluate_quality(&input);
    assert_eq!(r_dim.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_dim,
        "QUALITY.REVIEW_DIMENSION_FAILED",
        Severity::Critical
    ));
}

/// Conjunto ordenado e deduplicado de `(code, severity)` do relatório: é o que
/// deve permanecer idêntico sob permutação da lista de reviews.
fn diag_code_set(report: &QualityReport) -> Vec<String> {
    let mut set: Vec<String> = report
        .diagnostics
        .iter()
        .map(|d| format!("{}:{:?}", d.code.as_str(), d.severity))
        .collect();
    set.sort();
    set.dedup();
    set
}

fn has_code(report: &QualityReport, code: &str) -> bool {
    report.diagnostics.iter().any(|d| d.code == code)
}

#[test]
fn receipt_hash_is_the_domain_receipt_accepted_by_export() {
    let input = sample_valid_qa_input();
    let rep = prepare_quality(&input);
    let expected = compute_receipt_v1(&input.qa_csv, &input.cloze_csv).into_inner();

    assert_eq!(rep.receipt_hash, expected);
    assert_eq!(rep.receipt_hash, rep.content_hash);
    assert!(rep.receipt_hash.starts_with("sha256:"));
}

#[test]
fn quick_evaluate_fails_closed_instead_of_passing_without_reviews() {
    let mut input = sample_valid_qa_input();
    input.mode = QualityMode::Quick;

    // quick + prepare: comportamento inalterado (structural/HTML only, sem o novo código).
    let prep = prepare_quality(&input);
    assert_eq!(prep.mode, "prepare");
    assert_eq!(prep.verdict, "PASS", "diagnostics: {:?}", prep.diagnostics);
    assert_eq!(prep.reviewed_count, 0);
    assert!(!has_code(&prep, "QUALITY.QUICK_SKIPS_REVIEWS"));

    // quick + evaluate: não valida reviews -> Critical explícito e FAIL (antes: PASS
    // com reviewed_count = 0, um passe livre silencioso sobre lote não revisado).
    input.reviews = vec![approve_qa_review(&prep.snapshot_hash, 2)];
    let eval_quick = evaluate_quality(&input);
    assert_eq!(eval_quick.mode, "evaluate");
    assert_eq!(eval_quick.verdict, "FAIL");
    assert_eq!(eval_quick.reviewed_count, 0);
    assert!(has_quality_diag(
        &eval_quick,
        "QUALITY.QUICK_SKIPS_REVIEWS",
        Severity::Critical
    ));

    // strict + evaluate: inalterado (PASS com a review aprovada).
    input.mode = QualityMode::Strict;
    let eval_strict = evaluate_quality(&input);
    assert_eq!(
        eval_strict.verdict, "PASS",
        "diagnostics: {:?}",
        eval_strict.diagnostics
    );
    assert_eq!(eval_strict.reviewed_count, 1);
    assert!(!has_code(&eval_strict, "QUALITY.QUICK_SKIPS_REVIEWS"));
}

#[test]
fn reviewed_count_and_code_set_are_invariant_to_review_order() {
    // Medido antes: [approve, revise] => reviewed_count = 1; [revise, approve] => 0.
    let mut input = sample_valid_qa_input();
    let prep = prepare_quality(&input);

    let approve = approve_qa_review(&prep.snapshot_hash, 2);
    let mut revise = approve_qa_review(&prep.snapshot_hash, 2);
    revise.verdict = "revise".to_owned();

    input.reviews = vec![approve.clone(), revise.clone()];
    let first = evaluate_quality(&input);
    input.reviews = vec![revise, approve];
    let second = evaluate_quality(&input);

    assert_eq!(
        first.reviewed_count, 1,
        "diagnostics: {:?}",
        first.diagnostics
    );
    assert_eq!(second.reviewed_count, first.reviewed_count);
    assert_eq!(diag_code_set(&second), diag_code_set(&first));
    assert_eq!(first.verdict, second.verdict);
    // Um único DUPLICATE_REVIEW por chave repetida, apesar das duas reviews.
    assert_eq!(
        first
            .diagnostics
            .iter()
            .filter(|d| d.code == "QUALITY.DUPLICATE_REVIEW")
            .count(),
        1,
        "diagnostics: {:?}",
        first.diagnostics
    );
    assert!(has_quality_diag(
        &first,
        "QUALITY.REVIEW_REVISE",
        Severity::Critical
    ));
}

#[test]
fn one_valid_review_fulfills_the_key_regardless_of_sibling_quality() {
    // Duas reviews para a MESMA linha: uma válida e uma com snapshot defasado.
    // A chave é cumprida porque ALGUMA review dela é válida, em qualquer ordem.
    let mut input = sample_valid_qa_input();
    input.qa_csv = make_qa_csv(&[
        (
            "Em paciente adulto com síndrome X sintética, qual é o marcador de primeira linha?",
            "Marcador Alfa",
            "Explicação clínica sintética sobre o marcador.",
        ),
        (
            "Em paciente adulto com síndrome X sintética, qual é o marcador de segunda linha?",
            "Marcador Beta",
            "Explicação clínica sintética sobre o segundo marcador.",
        ),
    ]);
    let prep = prepare_quality(&input);

    let current = approve_qa_review(&prep.snapshot_hash, 2);
    let stale_other_row = approve_qa_review("sha256:stale", 3);

    input.reviews = vec![current.clone(), stale_other_row.clone()];
    let first = evaluate_quality(&input);
    input.reviews = vec![stale_other_row, current];
    let second = evaluate_quality(&input);

    assert_eq!(
        first.reviewed_count, 1,
        "diagnostics: {:?}",
        first.diagnostics
    );
    assert_eq!(second.reviewed_count, 1);
    assert_eq!(diag_code_set(&second), diag_code_set(&first));
    assert!(has_quality_diag(
        &first,
        "QUALITY.STALE_REVIEW_SNAPSHOT",
        Severity::Critical
    ));
}

#[test]
fn snapshot_hash_ignores_csv_formatting_noise_but_tracks_content() {
    let base = sample_valid_qa_input();
    let base_hash = prepare_quality(&base).snapshot_hash;

    let qa_variants = [
        ("newline final", format!("{}\n", base.qa_csv)),
        ("linha em branco extra", format!("{}\n\n", base.qa_csv)),
        (
            "espaços à direita",
            format!("{}   ", base.qa_csv.replace('\n', "  \n")),
        ),
    ];
    for (label, qa_csv) in qa_variants {
        let mut variant = base.clone();
        variant.qa_csv = qa_csv;
        // Ruído equivalente no CSV de cloze também não pode contar.
        variant.cloze_csv = format!("{}\n", base.cloze_csv);
        assert_eq!(
            prepare_quality(&variant).snapshot_hash,
            base_hash,
            "formatação '{label}' não deveria mudar o snapshot_hash"
        );
    }

    // Controle: conteúdo realmente diferente muda o hash.
    let mut changed = base.clone();
    changed.qa_csv = make_qa_csv(&[(
        "Em paciente adulto com síndrome X sintética, qual é o marcador de última linha?",
        "Marcador Gama",
        "Explicação clínica sintética sobre o marcador.",
    )]);
    assert_ne!(prepare_quality(&changed).snapshot_hash, base_hash);
}
