//! Gate `NOTE.STYLE_WARN` vs. saída canônica do compilador (regressão P0-1).
//!
//! O renderizador emite listas canônicas como
//! `<section class="anki-list"><p><b>Título</b></p><ul><li>item</li></ul></section>`
//! (ou `anki-steps`/`anki-flow`), com itens em texto puro. O gate de estilo deve punir
//! apenas formatação LEGADA (`•`/`-`/`*` em texto puro ou `<li>` fora das seções canônicas),
//! nunca a saída do próprio compilador.

use anki_domain::CLOZE_HEADER_CANONICAL;
use anki_validate::notes::check_notes;
use anki_validate::{compile_drafts, compile_drafts_strict, review_batch_information};
use serde_json::json;

fn style_warn_messages(diagnostics: &[anki_domain::Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .filter(|d| d.code == "NOTE.STYLE_WARN")
        .map(|d| d.message.clone())
        .collect()
}

/// Diagnósticos de estilo do lote inteiro (batch + por cartão).
fn review_style_warnings(qa_csv: &str) -> Vec<String> {
    let review = review_batch_information(qa_csv, CLOZE_HEADER_CANONICAL);
    let mut warnings = style_warn_messages(&review.diagnostics);
    for card in &review.cards {
        warnings.extend(style_warn_messages(&card.diagnostics));
    }
    warnings
}

fn qa_csv_from_compiled(compiled: &serde_json::Value) -> String {
    compiled["qa_csv"]
        .as_str()
        .expect("compiled QA CSV string")
        .to_owned()
}

#[test]
fn strict_compiled_list_block_does_not_trip_style_gate() {
    let draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Quais são os fatores de risco cardiovasculares clássicos?",
        "resposta": "Hipertensão arterial e diabetes mellitus tipo 2.",
        "campo_estudo": "Cardiologia",
        "notas_adicionais": {
            "blocos": [
                {
                    "tipo": "lista",
                    "titulo": "Riscos",
                    "itens": ["Hipertensão arterial sistêmica", "Diabetes mellitus tipo 2"]
                }
            ],
            "fontes": [
                {
                    "titulo": "Diretriz SBC 2024",
                    "localizacao": { "formato": "secao", "referencia": "cap. 3" },
                    "trecho": "Hipertensão e diabetes são fatores de risco clássicos."
                }
            ]
        }
    });

    let compiled =
        compile_drafts_strict(&[draft], true, true, "").expect("strict compile succeeds");
    let qa_csv = qa_csv_from_compiled(&compiled);

    assert!(
        qa_csv.contains(r#"<section class="anki-list">"#),
        "compiled notes must carry the canonical list section: {qa_csv}"
    );

    let warnings = review_style_warnings(&qa_csv);
    assert!(
        warnings.is_empty(),
        "compiler-emitted canonical list must not trip NOTE.STYLE_WARN: {warnings:?}\nqa_csv={qa_csv}"
    );
}

#[test]
fn strict_compiled_steps_and_flow_blocks_do_not_trip_style_gate() {
    let draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Qual a sequência inicial do atendimento ao politraumatizado?",
        "resposta": "Via aérea, respiração e circulação.",
        "campo_estudo": "Cirurgia",
        "notas_adicionais": {
            "blocos": [
                {
                    "tipo": "lista",
                    "titulo": "Sequência",
                    "ordenada": true,
                    "itens": ["Via aérea", "Respiração", "Circulação"]
                },
                {
                    "tipo": "fluxograma",
                    "titulo": "Triagem",
                    "fluxo": {
                        "inicio": "n1",
                        "nos": [
                            { "id": "n1", "texto": "Paciente instável?" },
                            { "id": "n2", "texto": "Sala vermelha" },
                            { "id": "n3", "texto": "Sala verde" }
                        ],
                        "arestas": [
                            { "origem": "n1", "destino": "n2", "rotulo": "Sim" },
                            { "origem": "n1", "destino": "n3", "rotulo": "Não" }
                        ]
                    }
                }
            ],
            "fontes": [
                {
                    "titulo": "ATLS 10ª Ed",
                    "localizacao": { "formato": "pdf", "pagina": 68 },
                    "trecho": "Airway, breathing, circulation."
                }
            ]
        }
    });

    let compiled =
        compile_drafts_strict(&[draft], true, true, "").expect("strict compile succeeds");
    let qa_csv = qa_csv_from_compiled(&compiled);

    assert!(
        qa_csv.contains(r#"<section class="anki-steps">"#),
        "{qa_csv}"
    );
    assert!(
        qa_csv.contains(r#"<section class="anki-flow">"#),
        "{qa_csv}"
    );

    let warnings = review_style_warnings(&qa_csv);
    assert!(
        warnings.is_empty(),
        "anki-steps/anki-flow output must not trip NOTE.STYLE_WARN: {warnings:?}\nqa_csv={qa_csv}"
    );
}

#[test]
fn legacy_plain_bullet_notes_still_trip_style_gate_end_to_end() {
    let draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Qual o mecanismo da hiponatremia dilucional?",
        "resposta": "Excesso de água livre em relação ao sódio corporal total.",
        "notas_adicionais": "• Mecanismo: retenção de água livre sem reposição de sódio."
    });

    let compiled = compile_drafts(&[draft], true, true, "").expect("legacy compile succeeds");
    let qa_csv = qa_csv_from_compiled(&compiled);

    let warnings = review_style_warnings(&qa_csv);
    assert!(
        !warnings.is_empty(),
        "legacy `• texto` bullet must still trip NOTE.STYLE_WARN\nqa_csv={qa_csv}"
    );
}

#[test]
fn canonical_section_exemption_is_scoped_to_its_own_section() {
    // Nota montada à mão: uma lista canônica e uma lista legada no mesmo campo.
    let notes = concat!(
        r#"<section class="anki-list"><ul><li>item canônico</li></ul></section>"#,
        r#"<ul><li>item legado sem negrito</li></ul>"#
    );

    let diags = check_notes(notes, "", None);
    assert!(
        diags.iter().any(|d| d.code == "NOTE.STYLE_WARN"),
        "legacy list outside the canonical section must still warn: {diags:?}"
    );
}
