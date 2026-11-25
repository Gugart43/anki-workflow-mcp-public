use anki_domain::Severity;
use anki_validate::quality::{
    QualityEvidence, QualityInput, QualityReview, QualitySource, evaluate_quality, prepare_quality,
};

use super::common::{
    all_true_dimensions, approve_qa_review, has_quality_diag, make_cloze_csv, make_qa_csv,
    sample_valid_qa_input,
};

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

// ============================================================
// Evidência com scope="notes" deve ser conferida contra o campo
// 'Notas Adicionais' do próprio card (não apenas contra a fonte).
// ============================================================

const NOTES_PROMPT: &str = "Em paciente adulto com síndrome X sintética, segundo o Manual Sintético 2026, qual é o marcador de primeira linha?";
const NOTES_ANSWER: &str = "Marcador Alfa";
/// Substring literal da fonte padrão de `sample_valid_qa_input`.
const SOURCE_QUOTE: &str = "Marcador Alfa constitui o parâmetro de primeira linha";
const NOT_FOUND_CODE: &str = "QUALITY.NOTES_EVIDENCE_QUOTE_NOT_FOUND";

fn retrieval_evidence() -> QualityEvidence {
    QualityEvidence {
        source_id: "S1".to_owned(),
        quote: SOURCE_QUOTE.to_owned(),
        locator: "Seção 2.1, p. 4".to_owned(),
        scope: "retrieval".to_owned(),
    }
}

fn notes_evidence(quote: &str) -> QualityEvidence {
    QualityEvidence {
        source_id: "S1".to_owned(),
        quote: quote.to_owned(),
        locator: "Notas Adicionais".to_owned(),
        scope: "notes".to_owned(),
    }
}

fn qa_with_notes(notes: &str, notes_quote: &str) -> QualityInput {
    let mut input = sample_valid_qa_input();
    input.qa_csv = make_qa_csv(&[(NOTES_PROMPT, NOTES_ANSWER, notes)]);
    input.bindings[0].evidence = vec![retrieval_evidence(), notes_evidence(notes_quote)];
    input
}

fn cloze_review(snapshot_hash: &str) -> QualityReview {
    QualityReview {
        note_type: "CL".to_owned(),
        source_row: 2,
        cloze_index: Some(1),
        snapshot_hash: snapshot_hash.to_owned(),
        reviewer: "curador-sintetico".to_owned(),
        verdict: "approve".to_owned(),
        rationale: "Fiel ao excerto citado.".to_owned(),
        dimensions: all_true_dimensions(),
    }
}

#[test]
fn notes_evidence_quote_absent_from_notes_field_is_critical() {
    // O quote existe na FONTE, mas NÃO no campo 'Notas Adicionais'.
    let mut input = qa_with_notes(
        "Explicação clínica sintética sobre o marcador.",
        "constitui o parâmetro de primeira linha",
    );

    let prep = prepare_quality(&input);
    assert_eq!(prep.verdict, "FAIL");
    assert!(has_quality_diag(&prep, NOT_FOUND_CODE, Severity::Critical));
    let diag = prep
        .diagnostics
        .iter()
        .find(|d| d.code == NOT_FOUND_CODE)
        .expect("diagnóstico esperado");
    assert_eq!(diag.row, Some(2));
    assert_eq!(diag.field.as_deref(), Some("Notas Adicionais"));
    assert_eq!(diag.owner_skill, "anki-post-curator");
    // O check original de fonte não deve disparar: o quote existe na fonte.
    assert!(!has_quality_diag(
        &prep,
        "QUALITY.EVIDENCE_QUOTE_NOT_FOUND",
        Severity::Critical
    ));

    // Mesmo com aprovação editorial completa o furo não pode passar (fail-open).
    input.reviews = vec![approve_qa_review(&prep.snapshot_hash, 2)];
    let eval = evaluate_quality(&input);
    assert_eq!(eval.verdict, "FAIL", "diagnostics: {:?}", eval.diagnostics);
    assert!(has_quality_diag(&eval, NOT_FOUND_CODE, Severity::Critical));
}

#[test]
fn notes_evidence_quote_present_in_notes_html_passes() {
    // Marcador de negrito no meio do quote: só o texto visível casa.
    let mut input = qa_with_notes(
        "<b>Marcador Alfa</b> constitui o parâmetro de primeira linha.",
        "Marcador Alfa constitui o parâmetro",
    );

    let prep = prepare_quality(&input);
    assert!(
        !has_quality_diag(&prep, NOT_FOUND_CODE, Severity::Critical),
        "diagnostics: {:?}",
        prep.diagnostics
    );

    input.reviews = vec![approve_qa_review(&prep.snapshot_hash, 2)];
    let eval = evaluate_quality(&input);
    assert_eq!(eval.verdict, "PASS", "diagnostics: {:?}", eval.diagnostics);
}

#[test]
fn notes_evidence_quote_as_raw_html_passes() {
    // Quote em HTML cru, presente tanto na fonte quanto no campo de notas.
    let mut input = sample_valid_qa_input();
    input.sources[0].text =
        "<b>Marcador Alfa</b> constitui o parâmetro de primeira linha.".to_owned();
    input.qa_csv = make_qa_csv(&[(
        NOTES_PROMPT,
        NOTES_ANSWER,
        "<b>Marcador Alfa</b> constitui o parâmetro de primeira linha.",
    )]);
    input.bindings[0].evidence = vec![
        QualityEvidence {
            source_id: "S1".to_owned(),
            quote: "constitui o parâmetro de primeira linha".to_owned(),
            locator: "Seção 2.1".to_owned(),
            scope: "retrieval".to_owned(),
        },
        notes_evidence("<b>Marcador Alfa</b>"),
    ];

    let prep = prepare_quality(&input);
    assert!(
        !has_quality_diag(&prep, NOT_FOUND_CODE, Severity::Critical),
        "diagnostics: {:?}",
        prep.diagnostics
    );

    input.reviews = vec![approve_qa_review(&prep.snapshot_hash, 2)];
    let eval = evaluate_quality(&input);
    assert_eq!(eval.verdict, "PASS", "diagnostics: {:?}", eval.diagnostics);
}

#[test]
fn notes_evidence_with_blank_notes_field_fails() {
    let input = qa_with_notes("", "constitui o parâmetro de primeira linha");

    let rep = prepare_quality(&input);
    assert_eq!(rep.verdict, "FAIL");
    assert!(has_quality_diag(&rep, NOT_FOUND_CODE, Severity::Critical));
    let diag = rep
        .diagnostics
        .iter()
        .find(|d| d.code == NOT_FOUND_CODE)
        .expect("diagnóstico esperado");
    assert_eq!(diag.row, Some(2));
    assert_eq!(diag.field.as_deref(), Some("Notas Adicionais"));
}

/// Monta um input com QA válido (linha 2) + Cloze c1 (linha 2) cujo campo de
/// notas é `cloze_notes`. Notas do Cloze ficam na coluna 1 do CSV CL.
fn cloze_notes_input(cloze_notes: &str, notes_quote: &str) -> QualityInput {
    let mut input = sample_valid_qa_input();
    input.qa_csv = make_qa_csv(&[(
        NOTES_PROMPT,
        NOTES_ANSWER,
        "Explicação clínica sintética sobre o marcador.",
    )]);
    input.sources[0].text =
        "Na fase aguda do protocolo sintético, o Marcador Alfa constitui o parâmetro de primeira linha."
            .to_owned();
    input.cloze_csv = make_cloze_csv(&[(
        "Na fase aguda do protocolo, {{c1::Alfa}} precede Beta.",
        cloze_notes,
    )]);
    input.bindings.push(anki_validate::quality::QualityBinding {
        note_type: "CL".to_owned(),
        source_row: 2,
        cloze_index: Some(1),
        obligation_ids: vec!["O1".to_owned()],
        evidence: vec![retrieval_evidence(), notes_evidence(notes_quote)],
    });
    input
}

#[test]
fn cloze_notes_evidence_uses_column_one() {
    const NOTES_QUOTE: &str = "Na fase aguda";

    // Positivo: o quote aparece na coluna 1 (Notas Adicionais) do CSV CL.
    let mut ok = cloze_notes_input("Na fase aguda, aplica-se o item citado.", NOTES_QUOTE);
    let prep_ok = prepare_quality(&ok);
    assert!(
        !has_quality_diag(&prep_ok, NOT_FOUND_CODE, Severity::Critical),
        "diagnostics: {:?}",
        prep_ok.diagnostics
    );
    ok.reviews = vec![
        approve_qa_review(&prep_ok.snapshot_hash, 2),
        cloze_review(&prep_ok.snapshot_hash),
    ];
    let eval_ok = evaluate_quality(&ok);
    assert_eq!(
        eval_ok.verdict, "PASS",
        "diagnostics: {:?}",
        eval_ok.diagnostics
    );

    // Negativo: o mesmo quote não está no campo de notas do Cloze.
    let mut bad = cloze_notes_input("Sequência sintética diversa.", NOTES_QUOTE);
    let prep_bad = prepare_quality(&bad);
    assert_eq!(prep_bad.verdict, "FAIL");
    assert!(has_quality_diag(
        &prep_bad,
        NOT_FOUND_CODE,
        Severity::Critical
    ));
    bad.reviews = vec![
        approve_qa_review(&prep_bad.snapshot_hash, 2),
        cloze_review(&prep_bad.snapshot_hash),
    ];
    let eval_bad = evaluate_quality(&bad);
    assert_eq!(
        eval_bad.verdict, "FAIL",
        "diagnostics: {:?}",
        eval_bad.diagnostics
    );
}

#[test]
fn notes_evidence_crlf_quote_matches_lf_field() {
    // Um campo de CSV nunca contém quebra de linha crua (as linhas são divididas
    // antes), então a quebra "LF" do campo aparece como <br>. O quote usa CRLF:
    // ambos os lados são normalizados (CRLF -> LF e texto visível com \s+ colapsado).
    let mut input = sample_valid_qa_input();
    input.sources[0].text =
        "Primeira linha.\r\nMarcador Alfa\r\nconstitui o parâmetro de primeira linha.".to_owned();
    input.qa_csv = make_qa_csv(&[(
        NOTES_PROMPT,
        NOTES_ANSWER,
        "Marcador Alfa<br>constitui o parâmetro de primeira linha",
    )]);
    input.bindings[0].evidence = vec![
        QualityEvidence {
            source_id: "S1".to_owned(),
            quote: "constitui o parâmetro de primeira linha".to_owned(),
            locator: "Seção 2.1".to_owned(),
            scope: "retrieval".to_owned(),
        },
        notes_evidence("Marcador Alfa\r\nconstitui o parâmetro"),
    ];

    let prep = prepare_quality(&input);
    assert!(
        !has_quality_diag(&prep, NOT_FOUND_CODE, Severity::Critical),
        "diagnostics: {:?}",
        prep.diagnostics
    );
    assert!(
        !has_quality_diag(
            &prep,
            "QUALITY.EVIDENCE_QUOTE_NOT_FOUND",
            Severity::Critical
        ),
        "diagnostics: {:?}",
        prep.diagnostics
    );

    input.reviews = vec![approve_qa_review(&prep.snapshot_hash, 2)];
    let eval = evaluate_quality(&input);
    assert_eq!(eval.verdict, "PASS", "diagnostics: {:?}", eval.diagnostics);
}

#[test]
fn notes_evidence_skipped_when_notes_field_is_unavailable() {
    // Linha estruturalmente inválida (colunas a mais): o campo de notas não é
    // localizável, então o check NÃO pode afirmar que o quote está ausente dele.
    // O diagnóstico estrutural próprio continua sendo emitido.
    let mut input = sample_valid_qa_input();
    input.qa_csv = format!(
        "{}\n{}|{}|Neurologia|Notas| |Ref|tag|coluna-extra",
        anki_domain::QA_HEADER_CANONICAL,
        NOTES_PROMPT,
        NOTES_ANSWER
    );
    let prep = prepare_quality(&input);
    assert!(
        !has_quality_diag(&prep, NOT_FOUND_CODE, Severity::Critical),
        "campo inparseável não pode produzir diagnóstico de quote: {:?}",
        prep.diagnostics
    );
    assert!(
        has_quality_diag(&prep, "QUALITY.BATCH_CRITICAL", Severity::Critical),
        "o diagnóstico estrutural próprio deve continuar: {:?}",
        prep.diagnostics
    );

    // Binding órfão (linha inexistente): mesma regra.
    let mut orphan = sample_valid_qa_input();
    orphan.bindings[0].source_row = 99;
    let prep_orphan = prepare_quality(&orphan);
    assert!(
        !has_quality_diag(&prep_orphan, NOT_FOUND_CODE, Severity::Critical),
        "binding órfão não pode produzir diagnóstico de quote: {:?}",
        prep_orphan.diagnostics
    );
    assert!(has_quality_diag(
        &prep_orphan,
        "QUALITY.ORPHAN_BINDING",
        Severity::Critical
    ));
}

#[test]
fn notes_evidence_requires_the_quote_in_the_source_as_well() {
    // Decisão documentada: evidência scope="notes" exige o quote na fonte E no campo
    // de notas. Um trecho que só existe nas notas continua reportado contra a fonte,
    // mas NÃO recebe o código de "ausente das notas" (que seria falso).
    let mut input = sample_valid_qa_input();
    input.qa_csv = make_qa_csv(&[(
        NOTES_PROMPT,
        NOTES_ANSWER,
        "Frase exclusiva das notas que a fonte não contém.",
    )]);
    input.bindings[0].evidence = vec![
        retrieval_evidence(),
        notes_evidence("Frase exclusiva das notas que a fonte não contém."),
    ];

    let prep = prepare_quality(&input);
    assert_eq!(prep.verdict, "FAIL");
    assert!(has_quality_diag(
        &prep,
        "QUALITY.EVIDENCE_QUOTE_NOT_FOUND",
        Severity::Critical
    ));
    assert!(
        !has_quality_diag(&prep, NOT_FOUND_CODE, Severity::Critical),
        "o quote existe nas notas; só a fonte reprova: {:?}",
        prep.diagnostics
    );
}

#[test]
fn notes_evidence_is_not_checked_for_other_scopes() {
    // scope=context continua sem conferência de campo (follow-up conhecido):
    // nenhum código novo aparece para context.
    let mut input = sample_valid_qa_input();
    input.bindings[0].evidence = vec![
        retrieval_evidence(),
        QualityEvidence {
            source_id: "S1".to_owned(),
            quote: SOURCE_QUOTE.to_owned(),
            locator: "Enunciado".to_owned(),
            scope: "context".to_owned(),
        },
    ];
    let prep = prepare_quality(&input);
    assert!(
        !has_quality_diag(&prep, NOT_FOUND_CODE, Severity::Critical),
        "diagnostics: {:?}",
        prep.diagnostics
    );
}

#[test]
fn notes_evidence_accepts_canonical_list_markup_and_entities() {
    // HTML canônico do compilador, entidades e espaços nas pontas não podem punir o
    // quote: o ramo de texto visível existe exatamente para isso.
    let cases = [
        (
            "<section class=\"anki-list\"><ul><li>Marcador Beta prevalece</li></ul></section>",
            "Marcador Beta prevalece",
        ),
        (
            "Alfa &amp; Beta definem o perfil",
            "Alfa & Beta definem o perfil",
        ),
        ("&#160;Marcador Beta&#160;", "Marcador Beta"),
        ("  Marcador Beta  ", "Marcador Beta"),
        ("<b>Marcador</b> Beta", "Marcador Beta"),
    ];
    for (notes, quote) in cases {
        let input = qa_with_notes(notes, quote);
        let prep = prepare_quality(&input);
        assert!(
            !has_quality_diag(&prep, NOT_FOUND_CODE, Severity::Critical),
            "notas {notes:?} / quote {quote:?} não deveriam reprovar: {:?}",
            prep.diagnostics
        );
    }
}

#[test]
fn notes_evidence_does_not_affect_retrieval_scope() {
    // Guarda contra over-firing: o check é exclusivo de scope="notes". Um quote válido
    // na fonte mas ausente das notas, declarado como "retrieval", não recebe o código.
    let mut input = sample_valid_qa_input();
    input.bindings[0].evidence = vec![QualityEvidence {
        source_id: "S1".to_owned(),
        quote: "constitui o parâmetro de primeira linha".to_owned(),
        locator: "Seção 2.1".to_owned(),
        scope: "retrieval".to_owned(),
    }];
    let prep = prepare_quality(&input);
    assert!(
        !has_quality_diag(&prep, NOT_FOUND_CODE, Severity::Critical),
        "diagnostics: {:?}",
        prep.diagnostics
    );
    assert!(
        !has_quality_diag(
            &prep,
            "QUALITY.EVIDENCE_QUOTE_NOT_FOUND",
            Severity::Critical
        ),
        "o quote existe na fonte: {:?}",
        prep.diagnostics
    );
}

#[test]
fn notes_evidence_has_no_minimum_quote_length() {
    // Propriedade documentada (docs/information-quality.md): não há comprimento mínimo.
    // Um quote trivialmente presente passa; um de 1 caractere ausente reprova.
    let mut present = qa_with_notes("Marcador Alfa prevalece", "a");
    let prep_present = prepare_quality(&present);
    assert!(
        !has_quality_diag(&prep_present, NOT_FOUND_CODE, Severity::Critical),
        "quote de 1 caractere presente no campo deve passar: {:?}",
        prep_present.diagnostics
    );

    present.bindings[0].evidence[1].quote = "Z".to_owned();
    let prep_absent = prepare_quality(&present);
    assert!(has_quality_diag(
        &prep_absent,
        NOT_FOUND_CODE,
        Severity::Critical
    ));
}
