use anki_domain::Severity;
use anki_validate::quality::{
    QualityBinding, QualityEvidence, QualityInput, evaluate_quality, prepare_quality,
};

use super::common::{approve_qa_review, has_quality_diag, sample_valid_qa_input};

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
