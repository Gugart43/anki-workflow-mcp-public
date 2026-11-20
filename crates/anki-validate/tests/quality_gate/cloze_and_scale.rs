use anki_domain::{CLOZE_HEADER_CANONICAL, QA_HEADER_CANONICAL, Severity};
use anki_validate::quality::{
    QualityBinding, QualityEvidence, QualityInput, QualityMode, QualityObligation, QualityReview,
    QualitySource, evaluate_quality, prepare_quality,
};

use super::common::{
    all_true_dimensions, approve_qa_review, has_quality_diag, make_cloze_csv, make_qa_csv,
    sample_valid_qa_input,
};

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
fn cloze_c1_and_c2_require_separate_bindings_and_reviews() {
    let cloze_csv = make_cloze_csv(&[(
        "Na condicao sintetica beta em adultos segundo o Manual 2026, o achado inicial e {{c1::sinal alfa}} e a confirmacao usa {{c2::exame gama}}.",
        "Nota explicativa sintetica.",
    )]);
    let mut input = QualityInput {
        qa_csv: QA_HEADER_CANONICAL.to_owned(),
        cloze_csv,
        mode: QualityMode::default(),
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
        mode: QualityMode::default(),
        sources,
        obligations,
        bindings,
        reviews: Vec::new(),
    };

    let prep = prepare_quality(&input);
    assert_eq!(prep.retrieval_count, 25);
    assert_eq!(prep.review.total_cards, 25);

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
        mode: QualityMode::default(),
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
        mode: QualityMode::default(),
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
fn malformed_cloze_and_csv_never_pass() {
    // Malformed cloze without index
    let mut no_idx = QualityInput {
        qa_csv: QA_HEADER_CANONICAL.to_owned(),
        cloze_csv: make_cloze_csv(&[("Sentença clínica sintética sem nenhuma omissão.", "Nota.")]),
        mode: QualityMode::default(),
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
        mode: QualityMode::default(),
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
        mode: QualityMode::default(),
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
