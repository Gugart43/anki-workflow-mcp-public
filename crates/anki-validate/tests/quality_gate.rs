use anki_domain::{CLOZE_HEADER_CANONICAL, QA_HEADER_CANONICAL, Severity, compute_receipt_v1};
use anki_validate::quality::{
    QualityBinding, QualityDimensions, QualityEvidence, QualityInput, QualityObligation,
    QualityReport, QualityReview, QualitySource, evaluate_quality, prepare_quality,
};

fn all_true_dimensions() -> QualityDimensions {
    QualityDimensions {
        source_fidelity: true,
        atomic_retrieval: true,
        context_sufficient: true,
        no_answer_leak: true,
        clinically_relevant: true,
        notes_appropriate: true,
        media_appropriate: true,
    }
}

fn make_qa_csv(rows: &[(&str, &str, &str)]) -> String {
    let mut csv = String::from(QA_HEADER_CANONICAL);
    for (prompt, answer, notes) in rows {
        csv.push('\n');
        csv.push_str(&format!(
            "{prompt}|{answer}|Neurologia|{notes}||Manual Sintético (2026), Seção 1|neuro"
        ));
    }
    csv
}

fn make_cloze_csv(rows: &[(&str, &str)]) -> String {
    let mut csv = String::from(CLOZE_HEADER_CANONICAL);
    for (sentence, notes) in rows {
        csv.push('\n');
        csv.push_str(&format!(
            "{sentence}|{notes}|Neurologia||Manual Sintético (2026), Seção 1|neuro"
        ));
    }
    csv
}

fn has_quality_diag(report: &QualityReport, code: &str, severity: Severity) -> bool {
    report
        .diagnostics
        .iter()
        .any(|d| d.code == code && d.severity == severity && d.owner_skill == "anki-post-curator")
}

fn sample_valid_qa_input() -> QualityInput {
    let qa_csv = make_qa_csv(&[(
        "Em paciente adulto com síndrome X sintética, segundo o Manual Sintético 2026, qual é o marcador de primeira linha?",
        "Marcador Alfa",
        "Explicação clínica sintética sobre o marcador.",
    )]);
    QualityInput {
        qa_csv,
        cloze_csv: CLOZE_HEADER_CANONICAL.to_owned(),
        sources: vec![QualitySource {
            id: "S1".to_owned(),
            title: "Manual Sintético".to_owned(),
            version: "2026.1".to_owned(),
            locator: "Seção 2.1".to_owned(),
            text: "Em adultos com síndrome X sintética, o Marcador Alfa constitui o parâmetro de primeira linha.".to_owned(),
        }],
        obligations: vec![QualityObligation {
            id: "O1".to_owned(),
            description: "Identificar o marcador de primeira linha na síndrome X sintética em adultos.".to_owned(),
            source_ids: vec!["S1".to_owned()],
        }],
        bindings: vec![QualityBinding {
            note_type: "QA".to_owned(),
            source_row: 2,
            cloze_index: None,
            obligation_ids: vec!["O1".to_owned()],
            evidence: vec![QualityEvidence {
                source_id: "S1".to_owned(),
                quote: "Marcador Alfa constitui o parâmetro de primeira linha".to_owned(),
                locator: "Seção 2.1, p. 4".to_owned(),
                scope: "retrieval".to_owned(),
            }],
        }],
        reviews: Vec::new(),
    }
}

fn approve_qa_review(snapshot_hash: &str, row: usize) -> QualityReview {
    QualityReview {
        note_type: "QA".to_owned(),
        source_row: row,
        cloze_index: None,
        snapshot_hash: snapshot_hash.to_owned(),
        reviewer: "curador-sintetico".to_owned(),
        verdict: "approve".to_owned(),
        rationale: "Fiel ao excerto citado e recuperação atômica.".to_owned(),
        dimensions: all_true_dimensions(),
    }
}

#[test]
fn malformed_cloze_prefix_is_never_accepted_as_plain_context() {
    for malformed in ["{{c2:text}}", "{{c2 incomplete", "{{c2", "{{c2 text}}"] {
        let mut input = sample_valid_qa_input();
        input.qa_csv = QA_HEADER_CANONICAL.to_owned();
        input.cloze_csv = make_cloze_csv(&[(
            &format!("Synthetic context identifies {{{{c1::Alpha}}}} and {malformed}."),
            "Synthetic explanation.",
        )]);
        let report = prepare_quality(&input);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "QUALITY.MALFORMED_CLOZE"),
            "missed malformed marker: {malformed}"
        );
    }
}

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
fn cloze_c1_and_c2_require_separate_bindings_and_reviews() {
    let cloze_csv = make_cloze_csv(&[(
        "Na condição sintética beta em adultos segundo o Manual 2026, o achado inicial é {{c1::sinal alfa}} e a confirmação usa {{c2::exame gama}}.",
        "Nota explicativa sintética.",
    )]);
    let mut input = QualityInput {
        qa_csv: QA_HEADER_CANONICAL.to_owned(),
        cloze_csv,
        sources: vec![QualitySource {
            id: "S1".to_owned(),
            title: "Manual Sintético".to_owned(),
            version: "2026.1".to_owned(),
            locator: "Cap. 1".to_owned(),
            text: "Achado inicial: sinal alfa; confirmação diagnóstica: exame gama.".to_owned(),
        }],
        obligations: vec![
            QualityObligation {
                id: "O1".to_owned(),
                description: "Achado inicial da condição beta.".to_owned(),
                source_ids: vec!["S1".to_owned()],
            },
            QualityObligation {
                id: "O2".to_owned(),
                description: "Exame confirmatório da condição beta.".to_owned(),
                source_ids: vec!["S1".to_owned()],
            },
        ],
        bindings: vec![QualityBinding {
            note_type: "CL".to_owned(),
            source_row: 2,
            cloze_index: Some(1),
            obligation_ids: vec!["O1".to_owned()],
            evidence: vec![QualityEvidence {
                source_id: "S1".to_owned(),
                quote: "Achado inicial: sinal alfa".to_owned(),
                locator: "Cap. 1".to_owned(),
                scope: "retrieval".to_owned(),
            }],
        }],
        reviews: Vec::new(),
    };

    // Only c1 bound -> incomplete
    let prep1 = prepare_quality(&input);
    assert_eq!(prep1.verdict, "INCOMPLETE");
    assert_eq!(prep1.retrieval_count, 2);
    assert!(has_quality_diag(
        &prep1,
        "QUALITY.MISSING_BINDING",
        Severity::Warn
    ));

    // Bind c2 as well, but only review c1 -> incomplete
    input.bindings.push(QualityBinding {
        note_type: "CL".to_owned(),
        source_row: 2,
        cloze_index: Some(2),
        obligation_ids: vec!["O2".to_owned()],
        evidence: vec![QualityEvidence {
            source_id: "S1".to_owned(),
            quote: "confirmação diagnóstica: exame gama".to_owned(),
            locator: "Cap. 1".to_owned(),
            scope: "retrieval".to_owned(),
        }],
    });
    let prep2 = prepare_quality(&input);
    assert_eq!(prep2.verdict, "INCOMPLETE");
    assert_eq!(prep2.covered_obligation_count, 2);

    input.reviews = vec![QualityReview {
        note_type: "CL".to_owned(),
        source_row: 2,
        cloze_index: Some(1),
        snapshot_hash: prep2.snapshot_hash.clone(),
        reviewer: "curador".to_owned(),
        verdict: "approve".to_owned(),
        rationale: "c1 ok".to_owned(),
        dimensions: all_true_dimensions(),
    }];
    let eval_partial = evaluate_quality(&input);
    assert_eq!(eval_partial.verdict, "INCOMPLETE");
    assert_eq!(eval_partial.reviewed_count, 1);
    assert!(has_quality_diag(
        &eval_partial,
        "QUALITY.MISSING_REVIEW",
        Severity::Warn
    ));

    // Add c2 review -> PASS
    input.reviews.push(QualityReview {
        note_type: "CL".to_owned(),
        source_row: 2,
        cloze_index: Some(2),
        snapshot_hash: prep2.snapshot_hash.clone(),
        reviewer: "curador".to_owned(),
        verdict: "approve".to_owned(),
        rationale: "c2 ok".to_owned(),
        dimensions: all_true_dimensions(),
    });
    let eval_full = evaluate_quality(&input);
    assert_eq!(eval_full.verdict, "PASS");
    assert_eq!(eval_full.retrieval_count, 2);
    assert_eq!(eval_full.reviewed_count, 2);
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
fn duplicate_orphan_and_empty_identities_fail() {
    // Duplicate source id
    let mut dup_src = sample_valid_qa_input();
    let cloned_src = dup_src.sources[0].clone();
    dup_src.sources.push(cloned_src);
    let r_dup_src = prepare_quality(&dup_src);
    assert_eq!(r_dup_src.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_dup_src,
        "QUALITY.DUPLICATE_SOURCE_ID",
        Severity::Critical
    ));

    // Empty source id
    let mut empty_src_id = sample_valid_qa_input();
    empty_src_id.sources[0].id = "   ".to_owned();
    let r_empty_src = prepare_quality(&empty_src_id);
    assert_eq!(r_empty_src.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_empty_src,
        "QUALITY.INVALID_SOURCE_ID",
        Severity::Critical
    ));

    // Duplicate obligation id
    let mut dup_obl = sample_valid_qa_input();
    let cloned_obl = dup_obl.obligations[0].clone();
    dup_obl.obligations.push(cloned_obl);
    let r_dup_obl = prepare_quality(&dup_obl);
    assert_eq!(r_dup_obl.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_dup_obl,
        "QUALITY.DUPLICATE_OBLIGATION_ID",
        Severity::Critical
    ));

    // Empty obligation id
    let mut empty_obl_id = sample_valid_qa_input();
    empty_obl_id.obligations[0].id = "".to_owned();
    let r_empty_obl = prepare_quality(&empty_obl_id);
    assert_eq!(r_empty_obl.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_empty_obl,
        "QUALITY.INVALID_OBLIGATION_ID",
        Severity::Critical
    ));

    // Orphan source ref in obligation
    let mut orphan_src = sample_valid_qa_input();
    orphan_src.obligations[0].source_ids = vec!["S_MISSING".to_owned()];
    let r_orphan_src = prepare_quality(&orphan_src);
    assert_eq!(r_orphan_src.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_orphan_src,
        "QUALITY.ORPHAN_SOURCE_REF",
        Severity::Critical
    ));

    // Orphan obligation ref in binding
    let mut orphan_obl = sample_valid_qa_input();
    orphan_obl.bindings[0].obligation_ids = vec!["O_MISSING".to_owned()];
    let r_orphan_obl = prepare_quality(&orphan_obl);
    assert_eq!(r_orphan_obl.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_orphan_obl,
        "QUALITY.ORPHAN_OBLIGATION_REF",
        Severity::Critical
    ));

    // Duplicate binding & orphan binding
    let mut dup_bind = sample_valid_qa_input();
    let b0 = dup_bind.bindings[0].clone();
    dup_bind.bindings.push(b0);
    let r_dup_bind = prepare_quality(&dup_bind);
    assert_eq!(r_dup_bind.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_dup_bind,
        "QUALITY.DUPLICATE_BINDING",
        Severity::Critical
    ));

    let mut orphan_bind = sample_valid_qa_input();
    orphan_bind.bindings.push(QualityBinding {
        note_type: "QA".to_owned(),
        source_row: 99,
        cloze_index: None,
        obligation_ids: vec!["O1".to_owned()],
        evidence: vec![QualityEvidence {
            source_id: "S1".to_owned(),
            quote: "Marcador Alfa".to_owned(),
            locator: "Seção 2.1".to_owned(),
            scope: "retrieval".to_owned(),
        }],
    });
    let r_orphan_bind = prepare_quality(&orphan_bind);
    assert_eq!(r_orphan_bind.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_orphan_bind,
        "QUALITY.ORPHAN_BINDING",
        Severity::Critical
    ));

    // QA with non-null cloze_index -> invalid address -> FAIL
    let mut bad_qa_addr = sample_valid_qa_input();
    bad_qa_addr.bindings[0].cloze_index = Some(1);
    let r_bad_qa = prepare_quality(&bad_qa_addr);
    assert_eq!(r_bad_qa.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_bad_qa,
        "QUALITY.INVALID_BINDING_ADDRESS",
        Severity::Critical
    ));

    // Duplicate & orphan review
    let mut dup_rev = sample_valid_qa_input();
    let prep = prepare_quality(&dup_rev);
    dup_rev.reviews = vec![
        approve_qa_review(&prep.snapshot_hash, 2),
        approve_qa_review(&prep.snapshot_hash, 2),
    ];
    let r_dup_rev = evaluate_quality(&dup_rev);
    assert_eq!(r_dup_rev.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_dup_rev,
        "QUALITY.DUPLICATE_REVIEW",
        Severity::Critical
    ));

    let mut orphan_rev = sample_valid_qa_input();
    orphan_rev.reviews = vec![
        approve_qa_review(&prep.snapshot_hash, 2),
        approve_qa_review(&prep.snapshot_hash, 77),
    ];
    let r_orphan_rev = evaluate_quality(&orphan_rev);
    assert_eq!(r_orphan_rev.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_orphan_rev,
        "QUALITY.ORPHAN_REVIEW",
        Severity::Critical
    ));

    // Empty reviewer / rationale -> INCOMPLETE
    let mut empty_rev = sample_valid_qa_input();
    let mut r = approve_qa_review(&prep.snapshot_hash, 2);
    r.reviewer = "   ".to_owned();
    r.rationale = "".to_owned();
    empty_rev.reviews = vec![r];
    let r_empty_rev = evaluate_quality(&empty_rev);
    assert_eq!(r_empty_rev.verdict, "INCOMPLETE");
    assert!(has_quality_diag(
        &r_empty_rev,
        "QUALITY.EMPTY_REVIEWER",
        Severity::Warn
    ));
    assert!(has_quality_diag(
        &r_empty_rev,
        "QUALITY.EMPTY_RATIONALE",
        Severity::Warn
    ));
}

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
fn missing_explicit_dimension_and_unknown_fields_serde_rejected() {
    let missing_dim_json = r#"{
        "qa_csv": "",
        "cloze_csv": "",
        "reviews": [{
            "note_type": "QA",
            "source_row": 2,
            "cloze_index": null,
            "snapshot_hash": "sha256:abc",
            "reviewer": "curador",
            "verdict": "approve",
            "rationale": "ok",
            "dimensions": {
                "source_fidelity": true,
                "atomic_retrieval": true,
                "context_sufficient": true,
                "no_answer_leak": true,
                "clinically_relevant": true,
                "notes_appropriate": true
            }
        }]
    }"#;
    assert!(serde_json::from_str::<QualityInput>(missing_dim_json).is_err());

    let unknown_field_json = r#"{
        "qa_csv": "",
        "cloze_csv": "",
        "export_authorized": true
    }"#;
    assert!(serde_json::from_str::<QualityInput>(unknown_field_json).is_err());
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

#[test]
fn twenty_five_individually_sourced_and_reviewed_qa_cards_pass_without_ceiling() {
    let mut rows_owned: Vec<(String, String, String)> = Vec::new();
    let mut sources = Vec::new();
    let mut obligations = Vec::new();
    let mut bindings = Vec::new();

    for i in 1..=25 {
        let topic = format!("fenotipo{:02}k", i);
        let target = format!("Biomarcador{:02}Z", i);
        let prompt = format!(
            "Em paciente adulto com {topic}, segundo o Manual 2026, qual é o alvo diagnóstico?"
        );
        let answer = target.clone();
        let note = format!("Explicação sintética para {topic} e {target}.");
        let quote = format!("{topic} utiliza o {target}");
        let src_text = format!("Na população adulta, {quote} como referência.");

        rows_owned.push((prompt, answer, note));
        sources.push(QualitySource {
            id: format!("S{i}"),
            title: "Manual Sintético".to_owned(),
            version: "2026.1".to_owned(),
            locator: format!("Seção {i}"),
            text: src_text,
        });
        obligations.push(QualityObligation {
            id: format!("O{i}"),
            description: format!("Identificar {target} em {topic}."),
            source_ids: vec![format!("S{i}")],
        });
        bindings.push(QualityBinding {
            note_type: "QA".to_owned(),
            source_row: i + 1,
            cloze_index: None,
            obligation_ids: vec![format!("O{i}")],
            evidence: vec![QualityEvidence {
                source_id: format!("S{i}"),
                quote,
                locator: format!("Seção {i}"),
                scope: "retrieval".to_owned(),
            }],
        });
    }

    let row_refs: Vec<(&str, &str, &str)> = rows_owned
        .iter()
        .map(|(p, a, n)| (p.as_str(), a.as_str(), n.as_str()))
        .collect();
    let mut input = QualityInput {
        qa_csv: make_qa_csv(&row_refs),
        cloze_csv: CLOZE_HEADER_CANONICAL.to_owned(),
        sources,
        obligations,
        bindings,
        reviews: Vec::new(),
    };

    let prep = prepare_quality(&input);
    assert_eq!(prep.retrieval_count, 25);
    assert_eq!(prep.review.batch_size_limit, None);

    input.reviews = (2..=26)
        .map(|row| approve_qa_review(&prep.snapshot_hash, row))
        .collect();

    let eval = evaluate_quality(&input);
    assert_eq!(eval.verdict, "PASS");
    assert_eq!(eval.retrieval_count, 25);
    assert_eq!(eval.reviewed_count, 25);
    assert_eq!(eval.obligation_count, 25);
    assert_eq!(eval.covered_obligation_count, 25);
}

#[test]
fn zero_cards_both_empty_or_headers_only_incomplete() {
    let empty_both = QualityInput {
        qa_csv: String::new(),
        cloze_csv: String::new(),
        sources: Vec::new(),
        obligations: Vec::new(),
        bindings: Vec::new(),
        reviews: Vec::new(),
    };
    let r1 = evaluate_quality(&empty_both);
    assert_eq!(r1.verdict, "INCOMPLETE");
    assert_eq!(r1.retrieval_count, 0);
    assert!(has_quality_diag(&r1, "QUALITY.EMPTY_BATCH", Severity::Warn));

    let headers_only = QualityInput {
        qa_csv: QA_HEADER_CANONICAL.to_owned(),
        cloze_csv: CLOZE_HEADER_CANONICAL.to_owned(),
        sources: Vec::new(),
        obligations: Vec::new(),
        bindings: Vec::new(),
        reviews: Vec::new(),
    };
    let r2 = evaluate_quality(&headers_only);
    assert_eq!(r2.verdict, "INCOMPLETE");
    assert_eq!(r2.retrieval_count, 0);
    assert!(has_quality_diag(&r2, "QUALITY.EMPTY_BATCH", Severity::Warn));
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
fn malformed_cloze_and_csv_never_pass() {
    // Malformed cloze without index
    let mut no_idx = QualityInput {
        qa_csv: QA_HEADER_CANONICAL.to_owned(),
        cloze_csv: make_cloze_csv(&[("Sentença clínica sintética sem nenhuma omissão.", "Nota.")]),
        sources: vec![QualitySource {
            id: "S1".to_owned(),
            title: "Manual".to_owned(),
            version: "2026".to_owned(),
            locator: "p. 1".to_owned(),
            text: "Sentença clínica sintética sem nenhuma omissão.".to_owned(),
        }],
        obligations: vec![QualityObligation {
            id: "O1".to_owned(),
            description: "Obrigação".to_owned(),
            source_ids: vec!["S1".to_owned()],
        }],
        bindings: Vec::new(),
        reviews: Vec::new(),
    };
    let prep_no_idx = prepare_quality(&no_idx);
    assert_eq!(prep_no_idx.verdict, "FAIL");
    assert!(has_quality_diag(
        &prep_no_idx,
        "QUALITY.MALFORMED_CLOZE",
        Severity::Critical
    ));
    no_idx.reviews = vec![QualityReview {
        note_type: "CL".to_owned(),
        source_row: 2,
        cloze_index: None,
        snapshot_hash: prep_no_idx.snapshot_hash,
        reviewer: "curador".to_owned(),
        verdict: "approve".to_owned(),
        rationale: "ok".to_owned(),
        dimensions: all_true_dimensions(),
    }];
    assert_eq!(evaluate_quality(&no_idx).verdict, "FAIL");

    // Malformed CSV (wrong column count)
    let bad_csv = QualityInput {
        qa_csv: format!("{QA_HEADER_CANONICAL}\nPergunta|Resposta"),
        cloze_csv: CLOZE_HEADER_CANONICAL.to_owned(),
        sources: Vec::new(),
        obligations: Vec::new(),
        bindings: Vec::new(),
        reviews: Vec::new(),
    };
    let r_bad_csv = evaluate_quality(&bad_csv);
    assert_eq!(r_bad_csv.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_bad_csv,
        "QUALITY.BATCH_CRITICAL",
        Severity::Critical
    ));

    // Unclosed cloze marker with an otherwise valid c1 must FAIL
    let unclosed_cl = QualityInput {
        qa_csv: QA_HEADER_CANONICAL.to_owned(),
        cloze_csv: make_cloze_csv(&[(
            "Paciente adulto com {{c1::achado alfa}} e {{c2::incompleto",
            "Nota.",
        )]),
        sources: Vec::new(),
        obligations: Vec::new(),
        bindings: Vec::new(),
        reviews: Vec::new(),
    };
    let r_unclosed = prepare_quality(&unclosed_cl);
    assert_eq!(r_unclosed.verdict, "FAIL");
    assert!(has_quality_diag(
        &r_unclosed,
        "QUALITY.MALFORMED_CLOZE",
        Severity::Critical
    ));

    // Critical QA card diagnostic (QA.YESNO) fails even when fully sourced and reviewed
    let mut yesno_qa = sample_valid_qa_input();
    yesno_qa.qa_csv = make_qa_csv(&[(
        "Em paciente adulto com síndrome X sintética, o Marcador Alfa está indicado?",
        "Sim",
        "Nota.",
    )]);
    let prep_yesno = prepare_quality(&yesno_qa);
    assert_eq!(prep_yesno.verdict, "FAIL");
    yesno_qa.reviews = vec![approve_qa_review(&prep_yesno.snapshot_hash, 2)];
    let eval_yesno = evaluate_quality(&yesno_qa);
    assert_eq!(eval_yesno.verdict, "FAIL");
    assert!(has_quality_diag(
        &eval_yesno,
        "QUALITY.CARD_CRITICAL",
        Severity::Critical
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
