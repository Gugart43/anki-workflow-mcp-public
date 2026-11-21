//! Suíte de integração Wave 2 — casos clínicos PT-BR dos módulos portados.
//!
//! Cobre apenas o que esta branch adiciona sobre o core existente:
//! `auto_repair`, `evidence_matcher`, `interference` e `qa_clinical`.
//!
//! Todos os diagnósticos são advisories estruturais. Nada aqui certifica
//! correção clínica, e as fixtures são enunciados conceituais públicos.

use anki_validate::auto_repair::{
    auto_repair_notes_html, auto_repair_retrieval_field, repair_mathjax_dollars,
};
use anki_validate::evidence_matcher::{
    BindingSpec, ObligationSpec, QuoteMatchResult, check_obligation_coverage,
    check_reference_specificity, normalize_pdf_text, verify_quote_in_source,
};
use anki_validate::interference::{
    CODE_DUPLICATE_RETRIEVAL, CODE_INTERFERENCE_PAIR_WARN, RetrievalUnit,
    analyze_batch_interference,
};
use anki_validate::qa_clinical::{
    CODE_BARE_CLINICAL_NUMBER_WARN, CODE_CLINICAL_ANCHOR_MISSING_WARN,
    CODE_DRUG_PAIR_INCOMPLETE_WARN, CODE_REVERSE_DIRECTION_WARN, check_qa_clinical,
};

#[test]
fn t02_qa_clinical_checks_anchor_drug_pair_direction_and_units() {
    let d1 = check_qa_clinical("Qual a dose de amoxicilina?", "Amoxicilina 500", Some(2));
    assert!(
        d1.iter()
            .any(|d| d.code == CODE_CLINICAL_ANCHOR_MISSING_WARN)
    );
    assert!(d1.iter().any(|d| d.code == CODE_DRUG_PAIR_INCOMPLETE_WARN));
    assert!(d1.iter().any(|d| d.code == CODE_BARE_CLINICAL_NUMBER_WARN));

    let d2 = check_qa_clinical(
        "Para qual doença serve ceftriaxona 2 g EV?",
        "Meningite.",
        Some(3),
    );
    assert!(d2.iter().any(|d| d.code == CODE_REVERSE_DIRECTION_WARN));
}

#[test]
fn t04_interference_catches_cross_type_duplicate_and_crohn_rcu_pair() {
    let units = vec![
        RetrievalUnit {
            id: "Q1".into(),
            row: 2,
            is_cloze: false,
            prompt: "Na suspeita de doença inflamatória intestinal, qual o achado endoscópico típico da doença de Crohn?".into(),
            target: "Úlceras aftoides salteadas em pedra de calçamento".into(),
        },
        RetrievalUnit {
            id: "Q2".into(),
            row: 3,
            is_cloze: false,
            prompt: "Na suspeita de doença inflamatória intestinal, qual o achado endoscópico típico da retocolite ulcerativa?".into(),
            target: "Mucosa friável contínua a partir do reto".into(),
        },
        RetrievalUnit {
            id: "C1".into(),
            row: 4,
            is_cloze: true,
            prompt: "Na suspeita de doença inflamatória intestinal, achado endoscópico típico da doença de Crohn: {{c1::úlceras aftoides salteadas em pedra de calçamento}}.".into(),
            target: "Úlceras aftoides salteadas em pedra de calçamento".into(),
        },
    ];
    let rep = analyze_batch_interference(&units);
    assert!(
        rep.diagnostics
            .iter()
            .any(|d| d.code == CODE_DUPLICATE_RETRIEVAL)
    );
    assert!(
        rep.diagnostics
            .iter()
            .any(|d| d.code == CODE_INTERFERENCE_PAIR_WARN)
    );
}

#[test]
fn t07_evidence_matcher_handles_pdf_hyphenation_and_coverage() {
    let pdf_src = "Na cetoacidose diabética, a hiper-\ntensão intracraniana é complicação temida do edema cerebral.";
    assert!(normalize_pdf_text(pdf_src).contains("hipertensão intracraniana"));
    let res = verify_quote_in_source("hipertensão intracraniana é complicação temida", pdf_src);
    assert!(matches!(res, QuoteMatchResult::NormalizedPdfMatch { .. }));
    assert!(check_reference_specificity("Harrison", Some(2)).is_some());
    assert!(check_reference_specificity("Harrison 21ª ed., Cap. 137, p. 1042", Some(2)).is_none());

    let cov = check_obligation_coverage(
        &[
            ObligationSpec {
                id: "OB1".into(),
                summary: "K+ na CAD".into(),
            },
            ObligationSpec {
                id: "OB2".into(),
                summary: "Insulina".into(),
            },
        ],
        &[BindingSpec {
            unit_id: "QA-1".into(),
            obligation_ids: vec!["OB1".into()],
        }],
    );
    assert_eq!(cov.uncovered_obligations, vec!["OB2"]);
}

#[test]
fn t08_auto_repair_preserves_currency_and_fixes_cloze_and_tables() {
    assert_eq!(
        repair_mathjax_dollars("Custo R$ 50,00; fórmula $pH = 7.4$"),
        r"Custo R$ 50,00; fórmula \(pH = 7.4\)"
    );
    let (fixed, actions) =
        auto_repair_retrieval_field("Pergunta: {{c1::Na}} e {{c3::K}} 4 meq/l\n\n", true);
    assert!(fixed.contains("{{c2::K}}"));
    assert!(fixed.contains("mEq/L"));
    assert!(!actions.is_empty());

    let (notes_fixed, _) = auto_repair_notes_html(
        r##"<table bgcolor="#fff"><tr><td style="color:#000">A | B</td></tr></table>"##,
    );
    assert!(!notes_fixed.contains("bgcolor="));
    assert!(!notes_fixed.contains("style="));
    assert!(notes_fixed.contains("&#124;"));
}
