use anki_domain::{CLOZE_HEADER_CANONICAL, QA_HEADER_CANONICAL, Severity};
use anki_validate::quality::{
    QualityBinding, QualityDimensions, QualityEvidence, QualityInput, QualityMode,
    QualityObligation, QualityReport, QualityReview, QualitySource,
};

pub fn all_true_dimensions() -> QualityDimensions {
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

pub fn make_qa_csv(rows: &[(&str, &str, &str)]) -> String {
    let mut csv = String::from(QA_HEADER_CANONICAL);
    for (prompt, answer, notes) in rows {
        csv.push('\n');
        csv.push_str(&format!(
            "{prompt}|{answer}|Neurologia|{notes}||Manual Sintético (2026), Seção 1|neuro"
        ));
    }
    csv
}

pub fn make_cloze_csv(rows: &[(&str, &str)]) -> String {
    let mut csv = String::from(CLOZE_HEADER_CANONICAL);
    for (sentence, notes) in rows {
        csv.push('\n');
        csv.push_str(&format!(
            "{sentence}|{notes}|Neurologia||Manual Sintético (2026), Seção 1|neuro"
        ));
    }
    csv
}

pub fn has_quality_diag(report: &QualityReport, code: &str, severity: Severity) -> bool {
    report
        .diagnostics
        .iter()
        .any(|d| d.code == code && d.severity == severity && d.owner_skill == "anki-post-curator")
}

pub fn sample_valid_qa_input() -> QualityInput {
    let qa_csv = make_qa_csv(&[(
        "Em paciente adulto com síndrome X sintética, segundo o Manual Sintético 2026, qual é o marcador de primeira linha?",
        "Marcador Alfa",
        "Explicação clínica sintética sobre o marcador.",
    )]);
    QualityInput {
        qa_csv,
        cloze_csv: CLOZE_HEADER_CANONICAL.to_owned(),
        mode: QualityMode::default(),
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

pub fn approve_qa_review(snapshot_hash: &str, row: usize) -> QualityReview {
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
