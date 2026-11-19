use std::num::NonZeroU64;

use anki_validate::legacy_adapter::{
    AdapterError, DraftNotesInput, compile_extra_notes, normalize_to_document,
};
use anki_validate::media_figure::MediaFigure;
use anki_validate::note_flow::{FlowEdge, FlowNode};
use anki_validate::note_model::{
    CalloutKind, CompileMode, KeyValueItem, NoteBlock, NoteDocument, NoteModelError, NoteSource,
    SourceLocation,
};
use anki_validate::note_render::render_document;
use anki_validate::validated_flow::{
    FlowError, FlowTopology, RawFlow, ValidatedFlow, render_validated_flow,
};
use serde_json::json;

fn sample_figure(filename: &str, caption: &str) -> MediaFigure {
    serde_json::from_value(json!({
        "filename": filename,
        "caption": caption
    }))
    .expect("valid MediaFigure")
}

#[test]
fn test_canonical_document_serialization_roundtrip_and_deny_unknown_fields() {
    let doc = NoteDocument {
        blocos: vec![
            NoteBlock::Paragrafo {
                titulo: Some("Resumo".into()),
                texto: "Texto do resumo".into(),
                colapsavel: None,
            },
            NoteBlock::Callout {
                variante: CalloutKind::Pearl,
                rotulo: Some("Regra de ouro".into()),
                texto: "Sempre verificar vias aéreas".into(),
            },
        ],
        fontes: vec![NoteSource {
            titulo: "ATLS".into(),
            localizacao: Some(SourceLocation::Pdf {
                pagina: NonZeroU64::new(42).unwrap(),
            }),
            trecho: Some("Capítulo 1".into()),
        }],
    };

    let serialized = serde_json::to_string(&doc).expect("serialize canonical doc");
    let deserialized: NoteDocument =
        serde_json::from_str(&serialized).expect("deserialize canonical doc");

    assert_eq!(deserialized.blocos.len(), 2);
    assert_eq!(deserialized.fontes.len(), 1);

    // Desconhecido em NoteDocument
    assert!(
        serde_json::from_value::<NoteDocument>(json!({
            "blocos": [],
            "extra_campo": 123
        }))
        .is_err()
    );

    // Desconhecido em bloco específico
    assert!(
        serde_json::from_value::<NoteBlock>(json!({
            "tipo": "paragrafo",
            "texto": "ok",
            "campo_fantasma": true
        }))
        .is_err()
    );
}

#[test]
fn test_flow_iterative_cycle_and_orphan_checks() {
    // 1. Ciclo simples a -> b -> a
    let cycle2 = RawFlow {
        inicio: "n1".into(),
        nos: vec![
            FlowNode {
                id: "n1".into(),
                texto: "Passo 1".into(),
            },
            FlowNode {
                id: "n2".into(),
                texto: "Passo 2".into(),
            },
        ],
        arestas: vec![
            FlowEdge {
                origem: "n1".into(),
                destino: "n2".into(),
                rotulo: "Avançar".into(),
            },
            FlowEdge {
                origem: "n2".into(),
                destino: "n1".into(),
                rotulo: "Voltar".into(),
            },
        ],
    };
    assert!(matches!(
        ValidatedFlow::try_from(cycle2),
        Err(FlowError::CycleDetected(_))
    ));

    // 2. Ciclo de 3 nós a -> b -> c -> a
    let cycle3 = RawFlow {
        inicio: "a".into(),
        nos: vec![
            FlowNode {
                id: "a".into(),
                texto: "A".into(),
            },
            FlowNode {
                id: "b".into(),
                texto: "B".into(),
            },
            FlowNode {
                id: "c".into(),
                texto: "C".into(),
            },
        ],
        arestas: vec![
            FlowEdge {
                origem: "a".into(),
                destino: "b".into(),
                rotulo: "".into(),
            },
            FlowEdge {
                origem: "b".into(),
                destino: "c".into(),
                rotulo: "".into(),
            },
            FlowEdge {
                origem: "c".into(),
                destino: "a".into(),
                rotulo: "".into(),
            },
        ],
    };
    assert!(matches!(
        ValidatedFlow::try_from(cycle3),
        Err(FlowError::CycleDetected(_))
    ));

    // 3. Nó inalcançável (órfão)
    let orphan = RawFlow {
        inicio: "a".into(),
        nos: vec![
            FlowNode {
                id: "a".into(),
                texto: "A".into(),
            },
            FlowNode {
                id: "isolado".into(),
                texto: "Isolado".into(),
            },
        ],
        arestas: vec![],
    };
    assert_eq!(
        ValidatedFlow::try_from(orphan).unwrap_err(),
        FlowError::UnreachableNode("isolado".into())
    );
}

#[test]
fn test_flow_all_three_topologies_rendering() {
    // Linear
    let linear = ValidatedFlow::try_from(RawFlow {
        inicio: "n1".into(),
        nos: vec![
            FlowNode {
                id: "n1".into(),
                texto: "Avaliar".into(),
            },
            FlowNode {
                id: "n2".into(),
                texto: "Tratar".into(),
            },
        ],
        arestas: vec![FlowEdge {
            origem: "n1".into(),
            destino: "n2".into(),
            rotulo: "Se grave".into(),
        }],
    })
    .unwrap();
    assert_eq!(linear.topology(), FlowTopology::Linear);
    let html_linear = render_validated_flow(&linear, Some("Algoritmo Linear"));
    assert!(html_linear.starts_with("<section class=\"anki-steps\">"));
    assert!(html_linear.contains("<b>Se grave:</b> Tratar"));
    assert!(!html_linear.contains("<br"));
    assert!(!html_linear.contains('|'));

    // ShallowTree
    let tree = ValidatedFlow::try_from(RawFlow {
        inicio: "root".into(),
        nos: vec![
            FlowNode {
                id: "root".into(),
                texto: "Choque?".into(),
            },
            FlowNode {
                id: "sim".into(),
                texto: "Drogas vasoativas".into(),
            },
            FlowNode {
                id: "nao".into(),
                texto: "Hidratação".into(),
            },
        ],
        arestas: vec![
            FlowEdge {
                origem: "root".into(),
                destino: "sim".into(),
                rotulo: "Sim".into(),
            },
            FlowEdge {
                origem: "root".into(),
                destino: "nao".into(),
                rotulo: "Não".into(),
            },
        ],
    })
    .unwrap();
    assert_eq!(tree.topology(), FlowTopology::ShallowTree);
    let html_tree = render_validated_flow(&tree, None);
    assert!(html_tree.contains("<b>Sim →</b> Drogas vasoativas"));
    assert!(html_tree.contains("<b>Não →</b> Hidratação"));

    // ConvergentDag
    let dag = ValidatedFlow::try_from(RawFlow {
        inicio: "in".into(),
        nos: vec![
            FlowNode {
                id: "in".into(),
                texto: "Suspeita".into(),
            },
            FlowNode {
                id: "a".into(),
                texto: "Via A".into(),
            },
            FlowNode {
                id: "b".into(),
                texto: "Via B".into(),
            },
            FlowNode {
                id: "fim".into(),
                texto: "Desfecho comum".into(),
            },
        ],
        arestas: vec![
            FlowEdge {
                origem: "in".into(),
                destino: "a".into(),
                rotulo: "Risco alto".into(),
            },
            FlowEdge {
                origem: "in".into(),
                destino: "b".into(),
                rotulo: "Risco baixo".into(),
            },
            FlowEdge {
                origem: "a".into(),
                destino: "fim".into(),
                rotulo: "".into(),
            },
            FlowEdge {
                origem: "b".into(),
                destino: "fim".into(),
                rotulo: "".into(),
            },
        ],
    })
    .unwrap();
    assert_eq!(dag.topology(), FlowTopology::ConvergentDag);
    let html_dag = render_validated_flow(&dag, Some("DAG"));
    assert!(html_dag.contains("Passo 4"));
}

#[test]
fn test_figure_filename_safety_enforcement() {
    let safe_doc = NoteDocument {
        blocos: vec![NoteBlock::Figura {
            figura: sample_figure("ecg_paciente_12.png", "ECG de 12 derivações"),
            colapsavel: None,
        }],
        fontes: vec![],
    };
    assert!(safe_doc.validate().is_ok());

    // Rejeição de esquemas remotos e data URI
    for bad_scheme in [
        "https://cdn.example.com/img.png",
        "http://local/img.png",
        "data:image/png;base64,AAAA",
        "ftp://server.org/pic.jpg",
        "javascript:alert(1)",
    ] {
        let doc = NoteDocument {
            blocos: vec![NoteBlock::Figura {
                figura: sample_figure(bad_scheme, "Legenda"),
                colapsavel: None,
            }],
            fontes: vec![],
        };
        assert!(matches!(
            doc.validate(),
            Err(NoteModelError::ForbiddenFigureFilenamePrefix { .. })
        ));
    }

    // Rejeição de travessia de diretório e caracteres inseguros
    for unsafe_fn in [
        "../../etc/passwd.png",
        "/absolute/path.png",
        "sub/folder.png",
        "C:\\Windows\\img.png",
        "img<script>.png",
        "quote\"name.png",
    ] {
        let doc = NoteDocument {
            blocos: vec![NoteBlock::Figura {
                figura: sample_figure(unsafe_fn, "Legenda"),
                colapsavel: None,
            }],
            fontes: vec![],
        };
        assert!(matches!(
            doc.validate(),
            Err(NoteModelError::UnsafeFigureFilename { .. })
        ));
    }
}

#[test]
fn test_render_preserves_safe_inline_html_without_double_escaping() {
    let doc = NoteDocument {
        blocos: vec![NoteBlock::Paragrafo {
            titulo: Some("Mecanismo".into()),
            texto: "<b>Ativação</b> via <i>JAK2</i> &amp; H<sub>2</sub>O & leigo <script>alert(1)</script>".into(),
            colapsavel: None,
        }],
        fontes: vec![],
    };

    let rendered = render_document(&doc).expect("render deve ter sucesso");
    // Preserva tags inline semânticas seguras
    assert!(rendered.html.contains("<b>Ativação</b>"));
    assert!(rendered.html.contains("<i>JAK2</i>"));
    assert!(rendered.html.contains("H<sub>2</sub>O"));
    assert!(rendered.html.contains("&amp;"));
    // Neutraliza script e tags ativas
    assert!(
        rendered
            .html
            .contains("&lt;script&gt;alert(1)&lt;/script&gt;")
    );
    assert!(!rendered.html.contains("<script>"));
    // Sem quebras brutas ou barras
    assert!(!rendered.html.contains('\n'));
    assert!(!rendered.html.contains('|'));
}

#[test]
fn test_collapsible_title_preservation() {
    let doc = NoteDocument {
        blocos: vec![
            NoteBlock::Paragrafo {
                titulo: Some("Diagnóstico Diferencial".into()),
                texto: "Pneumonia vs Tromboembolismo".into(),
                colapsavel: Some("Ver diagnósticos".into()),
            },
            NoteBlock::ChaveValor {
                titulo: Some("Marcadores".into()),
                pares: vec![KeyValueItem {
                    chave: "D-Dímero".into(),
                    valor: "Elevado".into(),
                }],
                colapsavel: Some("Ver marcadores".into()),
            },
            NoteBlock::Lista {
                titulo: Some("Critérios".into()),
                ordenada: true,
                itens: vec!["Critério 1".into(), "Critério 2".into()],
                colapsavel: Some("Ver critérios".into()),
            },
        ],
        fontes: vec![],
    };

    let rendered = render_document(&doc).expect("render com colapsável");
    assert!(
        rendered
            .html
            .contains("<summary><b>Ver diagnósticos</b></summary>")
    );
    assert!(rendered.html.contains("<b>Diagnóstico Diferencial</b>"));

    assert!(
        rendered
            .html
            .contains("<summary><b>Ver marcadores</b></summary>")
    );
    assert!(rendered.html.contains("<b>Marcadores</b>"));

    assert!(
        rendered
            .html
            .contains("<summary><b>Ver critérios</b></summary>")
    );
    assert!(rendered.html.contains("<b>Critérios</b>"));
}

#[test]
fn test_source_deduplication_retains_all_locations() {
    let doc = NoteDocument {
        blocos: vec![],
        fontes: vec![
            NoteSource {
                titulo: "Diretriz SBC".into(),
                localizacao: Some(SourceLocation::Secao {
                    referencia: "Capítulo 1".into(),
                }),
                trecho: None,
            },
            NoteSource {
                titulo: "Diretriz SBC".into(),
                localizacao: Some(SourceLocation::Secao {
                    referencia: "Capítulo 2".into(),
                }),
                trecho: None,
            },
            NoteSource {
                titulo: "Diretriz SBC".into(),
                localizacao: Some(SourceLocation::Url {
                    href: "https://sbc.org.br/dir".into(),
                }),
                trecho: None,
            },
            // Duplicata exata de uma existente
            NoteSource {
                titulo: " Diretriz SBC ".into(),
                localizacao: Some(SourceLocation::Secao {
                    referencia: " capítulo 1 ".into(),
                }),
                trecho: None,
            },
        ],
    };

    let rendered = render_document(&doc).expect("render com fontes");
    assert_eq!(rendered.source_refs.len(), 3);
    assert!(
        rendered
            .source_refs
            .contains(&"Diretriz SBC, Capítulo 1.".to_string())
    );
    assert!(
        rendered
            .source_refs
            .contains(&"Diretriz SBC, Capítulo 2.".to_string())
    );
    assert!(
        rendered
            .source_refs
            .contains(&"Diretriz SBC (https://sbc.org.br/dir).".to_string())
    );
}

#[test]
fn test_legacy_compat_rejects_invalid_canonical_document_without_fallback() {
    let invalid_canonical = json!({
        "blocos": [
            {
                "tipo": "paragrafo",
                "texto": "   "
            }
        ]
    });

    let res = normalize_to_document(
        &DraftNotesInput {
            notas_adicionais: Some(&invalid_canonical),
            ..Default::default()
        },
        CompileMode::LegacyCompat,
    );

    assert!(
        res.is_err(),
        "Documento canônico com texto em branco não pode sofrer fallback silencioso para legado"
    );
}

#[test]
fn test_legacy_compat_rejects_competing_canonical_and_legacy_payloads() {
    let competing = json!({
        "fisiopatologia": "Mecanismo legado",
        "blocos": [
            {
                "tipo": "paragrafo",
                "texto": "Texto canônico"
            }
        ]
    });

    let res = normalize_to_document(
        &DraftNotesInput {
            notas_adicionais: Some(&competing),
            ..Default::default()
        },
        CompileMode::LegacyCompat,
    );

    assert!(matches!(res, Err(AdapterError::CompetingPayloads(_))));
}

#[test]
fn test_legacy_compat_unification_and_markdown_table_preservation() {
    let plain_with_table = json!(
        "Introdução clínica\n\n| Droga | Dose |\n|---|---|\n| Adrenalina | 1 mg |\n\nConduta pós-PCR"
    );

    let res = compile_extra_notes(
        &DraftNotesInput {
            notas_adicionais: Some(&plain_with_table),
            ..Default::default()
        },
        CompileMode::LegacyCompat,
    )
    .expect("LegacyCompat deve compilar nota com tabela markdown");

    assert!(res.rendered.html.contains("<table"));
    assert!(res.rendered.html.contains("Adrenalina"));
    assert_eq!(res.rendered.table_count, 1);
    assert!(!res.rendered.html.contains("<br"));
}

#[test]
fn test_strict_v2_strictly_forbids_legacy_fields() {
    let legacy_notes = json!({
        "fisiopatologia": "Mecanismo antigo"
    });

    let err = normalize_to_document(
        &DraftNotesInput {
            notas_adicionais: Some(&legacy_notes),
            ..Default::default()
        },
        CompileMode::StrictV2,
    )
    .unwrap_err();

    assert!(matches!(
        err,
        AdapterError::LegacyFieldNotAllowedInStrictV2("fisiopatologia")
    ));
}
