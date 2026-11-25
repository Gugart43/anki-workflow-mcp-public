//! Gate de qualidade — diagnósticos de Notas Adicionais (namespace `NOTE`).
//!
//! Dois canais distintos e complementares (decisão de produto: Warn de nota é
//! consultivo, nunca bloqueia). O primeiro é `report.notes_diagnostics`, agregado
//! verbatim, ordenado e deduplicado. O segundo é a promoção de `NOTE.*` Critical
//! para `QUALITY.CARD_CRITICAL` com `cause_code` preservando a identidade original.
//! Nenhum dos dois altera o cálculo de `verdict`, que continua olhando apenas os
//! diagnósticos de topo.

use anki_domain::{QA_HEADER_CANONICAL, Severity};
use anki_validate::quality::{QualityMode, evaluate_quality, prepare_quality};

use super::common::{approve_qa_review, make_cloze_csv, make_qa_csv, sample_valid_qa_input};

const PROMPT: &str = "Em paciente adulto com síndrome X sintética, segundo o Manual Sintético 2026, qual é o marcador de primeira linha?";
const ANSWER: &str = "Marcador Alfa";
const LEGACY_LIST_NOTES: &str = "• Avaliação prática: olhos abertos não garantem consciência.";
const METADATA_NOTES: &str = "ID: 4242 — explicação clínica sintética sobre o marcador.";

fn notes_codes(report: &anki_validate::quality::QualityReport) -> Vec<&str> {
    report
        .notes_diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect()
}

#[test]
fn note_metadata_critical_promoted_with_cause_code() {
    let mut input = sample_valid_qa_input();
    input.qa_csv = make_qa_csv(&[(PROMPT, ANSWER, METADATA_NOTES)]);

    let rep = prepare_quality(&input);
    assert_eq!(rep.verdict, "FAIL");
    assert!(
        rep.notes_diagnostics
            .iter()
            .any(|d| d.code == "NOTE.METADATA" && d.severity == Severity::Critical),
        "notes_diagnostics: {:?}",
        rep.notes_diagnostics
    );

    let promoted: Vec<_> = rep
        .diagnostics
        .iter()
        .filter(|d| d.code == "QUALITY.CARD_CRITICAL")
        .collect();
    assert_eq!(promoted.len(), 1, "promoted: {promoted:?}");
    let d = promoted[0];
    assert_eq!(d.severity, Severity::Critical);
    assert_eq!(d.cause_code.as_deref(), Some("NOTE.METADATA"));
    assert_eq!(d.row, Some(2));
    assert_eq!(d.field.as_deref(), Some("Notas Adicionais"));
    assert!(
        d.suggestion.is_some(),
        "a suggestion consultiva do check de notas deve sobreviver à promoção"
    );
    assert!(d.message.contains("NOTE.METADATA"));
    assert_eq!(d.owner_skill, "anki-post-curator");
}

#[test]
fn note_style_warn_reaches_report_without_changing_verdict() {
    let mut input = sample_valid_qa_input();
    input.qa_csv = make_qa_csv(&[(PROMPT, ANSWER, LEGACY_LIST_NOTES)]);

    let prep = prepare_quality(&input);
    input.reviews = vec![approve_qa_review(&prep.snapshot_hash, 2)];
    let eval = evaluate_quality(&input);

    // Warn de nota é consultivo: com review aprovada o veredicto continua PASS.
    assert_eq!(eval.verdict, "PASS", "diagnostics: {:?}", eval.diagnostics);
    let warn = eval
        .notes_diagnostics
        .iter()
        .find(|d| d.code == "NOTE.STYLE_WARN")
        .expect("NOTE.STYLE_WARN deve chegar ao relatório");
    assert_eq!(warn.severity, Severity::Warn);
    assert_eq!(warn.owner_skill, "anki-notes-enricher");
    assert_eq!(warn.row, Some(2));
    assert_eq!(warn.field.as_deref(), Some("Notas Adicionais"));
    // Warn nunca é promovido ao canal bloqueante.
    assert!(
        !eval
            .diagnostics
            .iter()
            .any(|d| d.code.namespace() == "NOTE"),
        "diagnostics top-level não deve conter NOTE.*: {:?}",
        eval.diagnostics
    );
}

#[test]
fn notes_aggregate_deduplicated_across_cloze_indices() {
    let mut input = sample_valid_qa_input();
    input.cloze_csv = make_cloze_csv(&[(
        "No protocolo sintético, {{c1::Alfa}} precede {{c2::Beta}} na fase aguda.",
        METADATA_NOTES,
    )]);

    let rep = prepare_quality(&input);
    assert_eq!(rep.verdict, "FAIL");
    let cloze_cards = rep
        .review
        .cards
        .iter()
        .filter(|c| c.note_type == "CL")
        .count();
    assert_eq!(cloze_cards, 2, "duas recuperações para a mesma nota Cloze");

    // O check de notas roda uma vez por card, mas o agregado deduplica.
    assert_eq!(
        rep.notes_diagnostics
            .iter()
            .filter(|d| d.code == "NOTE.METADATA")
            .count(),
        1,
        "notes_diagnostics: {:?}",
        rep.notes_diagnostics
    );
    assert_eq!(
        rep.diagnostics
            .iter()
            .filter(|d| d.code == "QUALITY.CARD_CRITICAL")
            .count(),
        1,
        "diagnostics: {:?}",
        rep.diagnostics
    );
}

#[test]
fn batch_critical_keeps_cause_code() {
    let mut input = sample_valid_qa_input();
    input.qa_csv = format!("{QA_HEADER_CANONICAL}\nPergunta|Resposta");

    let rep = prepare_quality(&input);
    assert_eq!(rep.verdict, "FAIL");
    let batch: Vec<_> = rep
        .diagnostics
        .iter()
        .filter(|d| d.code == "QUALITY.BATCH_CRITICAL")
        .collect();
    assert!(!batch.is_empty(), "diagnostics: {:?}", rep.diagnostics);

    for d in batch {
        let cause = d.cause_code.as_deref().expect("cause_code obrigatório");
        assert!(
            rep.review
                .diagnostics
                .iter()
                .any(|rd| rd.code == cause && rd.severity == Severity::Critical),
            "cause_code '{cause}' deve apontar para um diagnóstico crítico do batch"
        );
    }
}

#[test]
fn clean_notes_produce_empty_notes_diagnostics() {
    let mut input = sample_valid_qa_input();
    input.cloze_csv = make_cloze_csv(&[(
        "No protocolo sintético, {{c1::Alfa}} precede Beta na fase aguda.",
        "Nota explicativa curta e limpa.",
    )]);

    let rep = prepare_quality(&input);
    assert!(
        rep.notes_diagnostics.is_empty(),
        "notes_diagnostics deve ficar vazio: {:?}",
        rep.notes_diagnostics
    );
}

#[test]
fn note_length_warn_stays_advisory() {
    let long_notes = "Explicação clínica detalhada sobre o mecanismo sintético. ".repeat(30);
    let mut input = sample_valid_qa_input();
    input.qa_csv = make_qa_csv(&[(PROMPT, ANSWER, &long_notes)]);

    let prep = prepare_quality(&input);
    input.reviews = vec![approve_qa_review(&prep.snapshot_hash, 2)];
    let eval = evaluate_quality(&input);

    assert_eq!(eval.verdict, "PASS", "diagnostics: {:?}", eval.diagnostics);
    assert!(
        eval.notes_diagnostics
            .iter()
            .any(|d| d.code == "NOTE.LENGTH_WARN" && d.severity == Severity::Warn),
        "notes_diagnostics: {:?}",
        eval.notes_diagnostics
    );
    // Nenhum Warn de nota foi promovido a diagnóstico bloqueante.
    assert!(
        !eval
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warn && d.code.namespace() == "NOTE"),
        "diagnostics: {:?}",
        eval.diagnostics
    );
}

#[test]
fn notes_diagnostics_deterministic_order() {
    // A ordenação real é (note_type, source_row, code, message, field). Como este
    // fixture tem apenas cartões QA, a monotonicidade por (row, code, message, field)
    // observável abaixo é consequência disso — ver
    // notes_diagnostics_order_is_deterministic_across_note_types para o caso misto.
    let long_metadata_notes = format!("ID: 4242 — {}", "Explicação clínica extensa. ".repeat(60));
    let mut input = sample_valid_qa_input();
    // Duas linhas QA: a primeira acumula METADATA + LENGTH_WARN, a segunda STYLE_WARN.
    input.qa_csv = make_qa_csv(&[
        (PROMPT, ANSWER, &long_metadata_notes),
        (PROMPT, ANSWER, LEGACY_LIST_NOTES),
    ]);

    let rep = prepare_quality(&input);
    let again = prepare_quality(&input);
    assert_eq!(
        rep.notes_diagnostics, again.notes_diagnostics,
        "a ordem deve ser determinística entre execuções"
    );

    let keys: Vec<(Option<usize>, &str, &str, Option<&str>)> = rep
        .notes_diagnostics
        .iter()
        .map(|d| {
            (
                d.row,
                d.code.as_str(),
                d.message.as_str(),
                d.field.as_deref(),
            )
        })
        .collect();
    assert!(
        keys.windows(2).all(|w| w[0] <= w[1]),
        "ordem esperada (row, code, message, field): {keys:?}"
    );
    assert_eq!(
        notes_codes(&rep),
        vec!["NOTE.LENGTH_WARN", "NOTE.METADATA", "NOTE.STYLE_WARN"],
        "diagnostics: {:?}",
        rep.notes_diagnostics
    );
    assert!(keys.iter().all(|k| k.0.is_some()));
}

#[test]
fn quick_mode_still_summarizes_notes() {
    let mut input = sample_valid_qa_input();
    input.mode = QualityMode::Quick;
    input.qa_csv = make_qa_csv(&[(PROMPT, ANSWER, METADATA_NOTES)]);
    input.sources.clear();
    input.obligations.clear();
    input.bindings.clear();

    let rep = prepare_quality(&input);
    assert_eq!(rep.mode, "prepare");
    assert!(
        rep.notes_diagnostics
            .iter()
            .any(|d| d.code == "NOTE.METADATA" && d.severity == Severity::Critical),
        "notes_diagnostics: {:?}",
        rep.notes_diagnostics
    );
    // Comportamento inalterado: Critical de nota já bloqueava no modo quick.
    assert_eq!(rep.verdict, "FAIL");
    assert!(rep.diagnostics.iter().any(|d| {
        d.code == "QUALITY.CARD_CRITICAL" && d.cause_code.as_deref() == Some("NOTE.METADATA")
    }));
}

#[test]
fn notes_aggregate_keeps_qa_and_cloze_cards_on_the_same_row_distinct() {
    // source_row é único apenas dentro de cada CSV: um cartão QA e um Cloze de
    // mesma linha são cartões DIFERENTES e não podem colapsar um no outro. A
    // réplica intra-cartão (mesma nota Cloze em vários índices) continua colapsada.
    let mut input = sample_valid_qa_input();
    input.qa_csv = make_qa_csv(&[(PROMPT, ANSWER, METADATA_NOTES)]);
    input.cloze_csv = make_cloze_csv(&[(
        "No protocolo sintético, {{c1::Alfa}} precede {{c2::Beta}} na fase aguda.",
        METADATA_NOTES,
    )]);

    let rep = prepare_quality(&input);
    assert_eq!(rep.verdict, "FAIL");

    let cloze_cards = rep
        .review
        .cards
        .iter()
        .filter(|c| c.note_type == "CL")
        .count();
    assert_eq!(cloze_cards, 2, "duas recuperações para a mesma nota Cloze");

    // Uma entrada agregada por cartão (QA row 2 e CL row 2), sem colapsar entre CSVs.
    assert_eq!(
        rep.notes_diagnostics
            .iter()
            .filter(|d| d.code == "NOTE.METADATA")
            .count(),
        2,
        "notes_diagnostics: {:?}",
        rep.notes_diagnostics
    );
    assert_eq!(
        rep.diagnostics
            .iter()
            .filter(|d| {
                d.code == "QUALITY.CARD_CRITICAL"
                    && d.cause_code.as_deref() == Some("NOTE.METADATA")
            })
            .count(),
        2,
        "um promovido por cartão distinto: {:?}",
        rep.diagnostics
    );
}

#[test]
fn notes_diagnostics_order_groups_by_note_type_before_row() {
    // Cartões QA e Cloze na MESMA linha não podem colapsar; a ordem agrupa por tipo de
    // nota (CL antes de QA), então a sequência de `row` deixa de ser monotônica — este
    // teste fixa exatamente isso, além do determinismo entre execuções.
    let mut input = sample_valid_qa_input();
    input.qa_csv = make_qa_csv(&[(PROMPT, ANSWER, METADATA_NOTES)]);
    input.cloze_csv = make_cloze_csv(&[
        ("Linha 2 sem metadados: {{c1::Alfa}}.", "Nota limpa."),
        ("Linha 3 com metadados: {{c1::Beta}}.", METADATA_NOTES),
    ]);

    let first = prepare_quality(&input);
    let second = prepare_quality(&input);
    assert_eq!(first.notes_diagnostics, second.notes_diagnostics);

    let rows: Vec<usize> = first
        .notes_diagnostics
        .iter()
        .filter_map(|d| d.row)
        .collect();
    // CL row 3 vem antes de QA row 2 (ordenação por tipo de nota): não é monotônica.
    assert_eq!(
        rows,
        vec![3, 2],
        "notes_diagnostics: {:?}",
        first.notes_diagnostics
    );
}
