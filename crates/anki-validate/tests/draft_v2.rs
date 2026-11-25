use anki_domain::NoteType;
use anki_validate::draft_compile::{
    compile_drafts, compile_drafts_partial, compile_drafts_partial_with_mode,
    compile_drafts_with_mode,
};
use anki_validate::draft_normalize::normalize_draft;
use anki_validate::draft_patch::patch_draft;
use anki_validate::{CompileMode, check_csv, compile_drafts_strict};
use serde_json::{Map, Value, json};

#[test]
fn test_normalize_draft_canonical_v2_document_preserved() {
    let canonical_draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Qual o manejo imediato do pneumotórax hipertensivo?",
        "resposta": "Toracocentese com agulha no 2º espaço intercostal ou 5º espaço na linha axilar anterior.",
        "campo_estudo": "Cirurgia",
        "notas_adicionais": {
            "blocos": [
                {
                    "tipo": "callout",
                    "variante": "pearl",
                    "rotulo": "Conduta imediata",
                    "texto": "Não aguardar confirmação radiológica."
                },
                {
                    "tipo": "paragrafo",
                    "titulo": "Fisiopatologia",
                    "texto": "Mecanismo valvular unidirecional colapsando o retorno venoso."
                }
            ],
            "fontes": [
                {
                    "titulo": "ATLS 10ª Ed",
                    "localizacao": {
                        "formato": "pdf",
                        "pagina": 68
                    },
                    "trecho": "Needle decompression must be performed immediately."
                }
            ]
        },
        "referencias": ["Diretriz Trauma"],
        "etiquetas": ["trauma", "emergencia"]
    });

    let normalized = normalize_draft(&canonical_draft).expect("canonical draft should normalize");
    let notas = &normalized["notas_adicionais"];
    assert!(notas.get("blocos").is_some());
    assert!(notas.get("fontes").is_some());

    let blocos = notas["blocos"].as_array().expect("blocos array");
    assert_eq!(blocos.len(), 2);
    assert_eq!(blocos[0]["tipo"], "callout");
    assert_eq!(blocos[0]["variante"], "pearl");
    assert_eq!(blocos[1]["tipo"], "paragrafo");
}

#[test]
fn test_normalize_draft_rejects_malformed_canonical_v2() {
    // Malformed canonical block: empty text in callout
    let bad_canonical = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Pergunta?",
        "resposta": "Resposta.",
        "notas_adicionais": {
            "blocos": [
                {
                    "tipo": "callout",
                    "variante": "pearl",
                    "texto": ""
                }
            ]
        }
    });

    let res = normalize_draft(&bad_canonical);
    assert!(
        res.is_err(),
        "malformed canonical document must fail validation and not fall back silently to legacy"
    );
}

#[test]
fn test_normalize_and_compile_reject_unsupported_tipo() {
    let bad_draft = json!({
        "tipo": "MULTIPLE_CHOICE",
        "enunciado_ou_sentenca": "Qual a opção correta?",
        "resposta": "A"
    });

    assert!(normalize_draft(&bad_draft).is_err());
    assert!(compile_drafts(&[bad_draft], true, true, "").is_err());
}

#[test]
fn test_compile_drafts_single_path_with_canonical_notes() {
    let draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Qual a tríade de Beck?",
        "resposta": "Hipotensão, hipofonese de bulhas e turgência jugular.",
        "campo_estudo": "Cardiologia",
        "notas_adicionais": {
            "blocos": [
                {
                    "tipo": "callout",
                    "variante": "trap",
                    "rotulo": "Armadilha",
                    "texto": "Nem sempre a tríade completa está presente no tamponamento cardíaco."
                }
            ],
            "fontes": [
                {
                    "titulo": "Braunwald Cardiologia",
                    "localizacao": {
                        "formato": "pdf",
                        "pagina": 112
                    },
                    "trecho": "Beck triad is classically described."
                }
            ]
        },
        "referencias": ["SBC 2024"],
        "etiquetas": ["cardio", "tamponamento"]
    });

    let result = compile_drafts(&[draft], true, true, "").expect("compile should succeed");
    let qa_csv = result["qa_csv"].as_str().unwrap();
    let lines: Vec<&str> = qa_csv.lines().collect();
    assert_eq!(lines.len(), 2);

    let fields: Vec<&str> = lines[1].split('|').collect();
    assert_eq!(fields.len(), 7);

    let notes = fields[3];
    assert!(notes.contains("anki-callout--trap"));
    assert!(notes.contains("Nem sempre a tríade completa está presente"));
    assert!(!notes.contains("Beck triad is classically described")); // Trecho de evidência não vaza

    let refs = fields[5];
    assert!(refs.contains("SBC 2024"));
    assert!(refs.contains("Braunwald Cardiologia, p. PDF 112."));
}

#[test]
fn test_compile_drafts_tag_inference_whitespace_splitting() {
    let draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Achado ECG?",
        "resposta": "Supradesnível de ST.",
        "campo_estudo": "",
        "etiquetas": "id::B02-QA-1 Cardiologia::Coronariopatia urgencia"
    });

    let result = compile_drafts(&[draft], true, true, "").expect("compile should succeed");
    let qa_csv = result["qa_csv"].as_str().unwrap();
    let fields: Vec<&str> = qa_csv.lines().nth(1).unwrap().split('|').collect();

    // Campo de Estudo deve ser inferido a partir dos tokens separados por whitespace
    assert_eq!(fields[2], "Medicina::Cardiologia::Coronariopatia");
    assert!(fields[6].contains("id::B02-QA-1"));
    assert!(fields[6].contains("Cardiologia::Coronariopatia"));
}

#[test]
fn test_compile_drafts_exact_source_dedup() {
    let draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Pergunta?",
        "resposta": "Resposta.",
        "referencias": ["Diretriz A, p. PDF 10.", "Diretriz B"],
        "notas_adicionais": {
            "blocos": [
                {
                    "tipo": "paragrafo",
                    "texto": "Texto clínico."
                }
            ],
            "fontes": [
                {
                    "titulo": "Diretriz A",
                    "localizacao": {
                        "formato": "pdf",
                        "pagina": 10
                    }
                }
            ]
        }
    });

    let result = compile_drafts(&[draft], true, true, "").expect("compile should succeed");
    let qa_csv = result["qa_csv"].as_str().unwrap();
    let refs = qa_csv.lines().nth(1).unwrap().split('|').nth(5).unwrap();

    // "Diretriz A, p. PDF 10." já existia nas referências iniciais e não deve ser duplicada
    let matches: Vec<&str> = refs.split("&#124;").map(str::trim).collect();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0], "Diretriz A, p. PDF 10.");
    assert_eq!(matches[1], "Diretriz B");
}

#[test]
fn test_patch_draft_alias_canonicalization_and_duplicate_prevention() {
    let draft = json!({
        "note_type": "qa",
        "pergunta": "Pergunta antiga?",
        "resposta": "Resposta antiga.",
        "campo": "Pediatria",
        "tags": ["pediatria", "neonatologia"]
    });

    let mut updates = Map::new();
    updates.insert("sentenca".to_string(), json!("Pergunta atualizada?"));
    updates.insert("especialidade".to_string(), json!("Neonatologia"));
    updates.insert("etiquetas".to_string(), json!(["neo"]));

    let patched = patch_draft(&draft, &updates).expect("patch should succeed");
    assert_eq!(patched["tipo"], json!("QA"));
    assert_eq!(
        patched["enunciado_ou_sentenca"],
        json!("Pergunta atualizada?")
    );
    assert_eq!(patched["campo_estudo"], json!("Neonatologia"));
    assert_eq!(patched["etiquetas"], json!(["neo"]));

    // Nenhum alias residual deve persistir no JSON
    assert!(patched.get("pergunta").is_none());
    assert!(patched.get("sentenca").is_none());
    assert!(patched.get("campo").is_none());
    assert!(patched.get("especialidade").is_none());
    assert!(patched.get("tags").is_none());
}

#[test]
fn test_patch_draft_preserves_images_alias() {
    let draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Pergunta?",
        "resposta": "Resposta."
    });

    let mut updates = Map::new();
    updates.insert(
        "imagens_relacionadas".to_string(),
        json!("<figure class='anki-figure'><img src='cxr_pneumonia.png'><figcaption>Consolidação alveolar</figcaption></figure>"),
    );

    let patched = patch_draft(&draft, &updates).expect("patch should succeed");
    let figuras = patched["figuras"].as_array().expect("figuras array");
    assert_eq!(figuras.len(), 1);
    assert_eq!(figuras[0]["filename"], "cxr_pneumonia.png");
    assert_eq!(figuras[0]["caption"], "Consolidação alveolar");
    assert_eq!(figuras[0]["style"], "clinical");
}

#[test]
fn test_canonical_repeated_normalization() {
    let initial_canonical = json!({
        "tipo": "CLOZE",
        "enunciado_ou_sentenca": "O achado clássico é {{c1::derrame pleural}}.",
        "campo_estudo": "Pneumologia",
        "notas_adicionais": {
            "blocos": [
                {
                    "tipo": "callout",
                    "variante": "pearl",
                    "rotulo": "Perola",
                    "texto": "Curva de Damoiseau observada na radiografia."
                }
            ],
            "fontes": [
                {
                    "titulo": "Pneumo Brasil",
                    "localizacao": {
                        "formato": "secao",
                        "referencia": "Capítulo 5"
                    }
                }
            ]
        },
        "referencias": ["SBPT"],
        "etiquetas": ["pneumo", "pleura"]
    });

    let first_pass = normalize_draft(&initial_canonical).expect("first pass should succeed");
    let second_pass = normalize_draft(&first_pass).expect("second pass should succeed");

    assert_eq!(first_pass, second_pass);
}

// --- CompileMode no caminho de compilação de drafts (P0-3) --------------------

fn base_qa_draft(extra: &Value) -> Value {
    let mut draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Pergunta sintética?",
        "resposta": "Resposta sintética."
    });
    let obj = draft.as_object_mut().expect("draft object");
    for (key, value) in extra.as_object().expect("extra object") {
        obj.insert(key.clone(), value.clone());
    }
    draft
}

fn notes_field(result: &Value) -> String {
    let qa_csv = result["qa_csv"].as_str().expect("qa_csv string");
    qa_csv
        .lines()
        .nth(1)
        .and_then(|line| line.split('|').nth(3))
        .expect("notes field")
        .to_string()
}

#[test]
fn test_default_compile_mode_is_legacy_compat() {
    assert_eq!(CompileMode::default(), CompileMode::LegacyCompat);
    let parsed: CompileMode =
        serde_json::from_value(json!("legacy_compat")).expect("legacy_compat");
    assert_eq!(parsed, CompileMode::LegacyCompat);
}

#[test]
fn test_compile_drafts_with_mode_strict_v2_rejects_legacy_notes_fields() {
    let cases = [
        (
            "fisiopatologia",
            json!({"notas_adicionais": {"fisiopatologia": "Mecanismo legado."}}),
        ),
        (
            "texto_livre",
            json!({"notas_adicionais": {"texto_livre": "Texto legado."}}),
        ),
        (
            "tabela_notas",
            json!({"tabela_notas": {"headers": ["Critério"], "rows": [["Valor"]]}}),
        ),
        (
            "tabelas_notas",
            json!({"tabelas_notas": [
                {"headers": ["Critério"], "rows": [["Valor"]]},
                {"headers": ["Conduta"], "rows": [["Imediata"]]}
            ]}),
        ),
        (
            "figuras_notas",
            json!({"figuras_notas": [{"filename": "fluxo_sintetico.png", "caption": "Fluxo."}]}),
        ),
    ];

    for (field, extra) in cases {
        let draft = base_qa_draft(&extra);
        let err = compile_drafts_with_mode(
            std::slice::from_ref(&draft),
            true,
            true,
            "",
            true,
            CompileMode::StrictV2,
        )
        .expect_err("strict_v2 must reject populated legacy notes fields");

        assert!(
            err.contains(&format!("drafts[0]: campo legado `{field}`")),
            "unexpected error for {field}: {err}"
        );
    }
}

#[test]
fn test_compile_drafts_with_mode_strict_v2_reports_indexed_errors() {
    let legacy = base_qa_draft(&json!({"notas_adicionais": {"fisiopatologia": "Legado."}}));
    let canonical = base_qa_draft(&json!({"notas_adicionais": {
        "blocos": [{"tipo": "paragrafo", "texto": "Texto canônico."}],
        "fontes": [{"titulo": "Fonte Canônica"}]
    }}));

    let err = compile_drafts_with_mode(
        &[canonical, legacy],
        true,
        true,
        "",
        true,
        CompileMode::StrictV2,
    )
    .expect_err("second draft must be rejected");

    assert!(
        err.contains("drafts[1]: campo legado `fisiopatologia`"),
        "error must carry the failing draft index: {err}"
    );
}

#[test]
fn test_compile_drafts_with_mode_strict_v2_accepts_canonical_note_document() {
    let draft = base_qa_draft(&json!({"notas_adicionais": {
        "blocos": [
            {"tipo": "paragrafo", "titulo": "Fisiopatologia", "texto": "Texto canônico preservado."},
            {"tipo": "callout", "variante": "pearl", "rotulo": "Regra prática", "texto": "Conduta imediata."}
        ],
        "fontes": [{"titulo": "Fonte Canônica", "localizacao": {"formato": "pdf", "pagina": 12}}]
    }}));

    let strict = compile_drafts_with_mode(
        std::slice::from_ref(&draft),
        true,
        true,
        "",
        true,
        CompileMode::StrictV2,
    )
    .expect("canonical NoteDocument must compile in strict_v2");
    let strict_notes = notes_field(&strict);

    assert!(strict_notes.contains("<section class=\"anki-note-section\">"));
    assert!(strict_notes.contains("Texto canônico preservado."));
    assert!(strict_notes.contains("anki-callout--pearl"));
    assert!(strict_notes.contains("<cite>Fonte Canônica, p. PDF 12.</cite>"));

    // HTML idêntico ao caminho legado: o modo só muda o que é aceito, não o render.
    let legacy = compile_drafts(&[draft], true, true, "").expect("legacy_compat must compile");
    assert_eq!(strict_notes, notes_field(&legacy));
    assert_eq!(strict["stats"], legacy["stats"]);
}

#[test]
fn test_compile_drafts_with_mode_strict_v2_allows_absent_or_null_notes() {
    for extra in [json!({}), json!({"notas_adicionais": null})] {
        let draft = base_qa_draft(&extra);
        let result = compile_drafts_with_mode(
            std::slice::from_ref(&draft),
            true,
            true,
            "",
            true,
            CompileMode::StrictV2,
        )
        .expect("absent or null notas_adicionais must compile in strict_v2");

        assert_eq!(result["stats"]["qa_count"], json!(1));
        assert_eq!(notes_field(&result), "");
    }
}

#[test]
fn test_compile_drafts_with_mode_strict_v2_rejects_invalid_canonical_without_fallback() {
    let draft = base_qa_draft(&json!({"notas_adicionais": {
        "blocos": [{"tipo": "paragrafo", "texto": "   "}]
    }}));

    let err = compile_drafts_with_mode(
        std::slice::from_ref(&draft),
        true,
        true,
        "",
        true,
        CompileMode::StrictV2,
    )
    .expect_err("invalid canonical document must not fall back to legacy");

    assert!(
        err.contains("drafts[0]:") && err.contains("texto"),
        "unexpected error: {err}"
    );

    // Documento texto (não canônico) também é recusado: strict_v2 exige NoteDocument.
    let text_draft = base_qa_draft(&json!({"notas_adicionais": "Texto legado livre."}));
    let err = compile_drafts_with_mode(
        std::slice::from_ref(&text_draft),
        true,
        true,
        "",
        true,
        CompileMode::StrictV2,
    )
    .expect_err("legacy string notes must be rejected in strict_v2");
    assert!(
        err.contains("notas_adicionais") && err.contains("StrictV2"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_compile_drafts_with_mode_legacy_compat_preserves_current_behavior() {
    let cases = [
        (
            "fisiopatologia",
            json!({"notas_adicionais": {"fisiopatologia": "Mecanismo legado."}}),
            "Mecanismo legado.",
        ),
        (
            "texto_livre",
            json!({"notas_adicionais": {"texto_livre": "Texto legado."}}),
            "Texto legado.",
        ),
        (
            "tabela_notas",
            json!({"tabela_notas": {"headers": ["Critério"], "rows": [["Valor"]]}}),
            "<table class=\"anki-table\">",
        ),
    ];

    for (field, extra, expected) in cases {
        let draft = base_qa_draft(&extra);

        // O wrapper histórico e o novo parâmetro explícito continuam aceitando legado.
        let default_mode = compile_drafts(std::slice::from_ref(&draft), true, true, "")
            .unwrap_or_else(|err| panic!("default mode must accept {field}: {err}"));
        let explicit_mode = compile_drafts_with_mode(
            std::slice::from_ref(&draft),
            true,
            true,
            "",
            false,
            CompileMode::LegacyCompat,
        )
        .unwrap_or_else(|err| panic!("legacy_compat must accept {field}: {err}"));

        assert_eq!(default_mode, explicit_mode);
        assert!(
            notes_field(&default_mode).contains(expected),
            "{field} content must be rendered: {}",
            notes_field(&default_mode)
        );
    }

    // Os dois eixos são independentes: draft tolerante + strict_v2 aceita notas canônicas.
    let tolerant_with_unknown = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Pergunta sintética?",
        "resposta": "Resposta sintética.",
        "campo_extra_desconhecido": 1,
        "notas_adicionais": {"blocos": [{"tipo": "paragrafo", "texto": "Texto canônico."}]}
    });
    assert!(
        compile_drafts_with_mode(
            std::slice::from_ref(&tolerant_with_unknown),
            true,
            true,
            "",
            false,
            CompileMode::StrictV2,
        )
        .is_ok(),
        "tolerant drafts plus strict_v2 notes must compile"
    );

    let legacy_notes = base_qa_draft(&json!({"notas_adicionais": {"fisiopatologia": "Legado."}}));
    assert!(
        compile_drafts_strict(std::slice::from_ref(&legacy_notes), true, true, "").is_ok(),
        "historical strict wrapper keeps compiling legacy notes in legacy_compat"
    );
}

#[test]
fn test_compile_drafts_strict_is_idempotent() {
    // A compilação estrita não pode depender de estado global nem de ordem: rodar
    // duas vezes sobre a mesma entrada deve produzir JSON byte-idêntico.
    let drafts = vec![
        json!({
            "tipo": "QA",
            "enunciado_ou_sentenca": "Pergunta com | pipe e \n quebra?",
            "resposta": "Resposta com CRLF\r\n e tab\t aqui.",
            "notas_adicionais": {
                "blocos": [{"tipo": "paragrafo", "texto": "Nota com | pipe."}]
            },
            "referencias": ["Ref A", "Ref A"],
            "etiquetas": ["tag_um", "tag dois"]
        }),
        json!({
            "tipo": "CLOZE",
            "enunciado_ou_sentenca": "O {{c1::pneumotórax}} hipertensivo exige {{c3::descompressão}}.",
            "notas_adicionais": {"fisiopatologia": "Mecanismo valvular."}
        }),
    ];

    let first = compile_drafts_strict(&drafts, true, true, "Clínica")
        .expect("strict compile must succeed on first run");
    let second = compile_drafts_strict(&drafts, true, true, "Clínica")
        .expect("strict compile must succeed on second run");

    assert_eq!(
        first, second,
        "compile_drafts_strict must be idempotent on the output JSON"
    );
    assert_eq!(
        first["qa_csv"], second["qa_csv"],
        "qa_csv must be byte-identical across runs"
    );
    assert_eq!(
        first["cloze_csv"], second["cloze_csv"],
        "cloze_csv must be byte-identical across runs"
    );
}

#[test]
fn test_compile_drafts_strict_adversarial_round_trip_is_structurally_clean() {
    // Round-trip adversarial: caracteres que normalmente quebram o CSV (`|`),
    // quebras de linha (LF, CRLF), tabulação e emoji em enunciado/notas devem
    // sobreviver ao compile e o CSV resultante passa por check_csv sem diagnóstico.
    let adversarial_stem = "Enunciado com | pipe, LF\n aqui, CRLF\r\n aqui, tab\t e emoji 🫀🧠 ok?";
    let adversarial_answer = "Resposta com | pipe e linha dupla:\n\r\nfim.";
    let adversarial_note = "Nota com | pipe, LF\n, CRLF\r\n, tab\te emoji 🩺.";

    let draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": adversarial_stem,
        "resposta": adversarial_answer,
        "campo_estudo": "Cardiologia",
        "notas_adicionais": {
            "blocos": [{"tipo": "paragrafo", "texto": adversarial_note}]
        },
        "referencias": ["Diretriz | Cardio"],
        "etiquetas": ["emoji_🫀", "pipe_ok"]
    });

    let compiled = compile_drafts_strict(std::slice::from_ref(&draft), true, true, "")
        .expect("adversarial draft must compile in strict mode");

    let qa_csv = compiled["qa_csv"].as_str().expect("qa_csv string");

    // O CSV compilado materializa as 7 colunas do NoteType::Qa.
    let row = qa_csv.lines().nth(1).expect("data row must exist");
    assert_eq!(
        row.split('|').count(),
        7,
        "escaped row must keep 7 columns: {row}"
    );
    assert!(
        !qa_csv.contains('\r'),
        "compiled CSV must not carry raw CR: {qa_csv}"
    );

    let diagnostics = check_csv(qa_csv, NoteType::Qa, true);
    assert!(
        diagnostics.is_empty(),
        "compiled qa_csv must pass check_csv without diagnostics, got: {diagnostics:?}"
    );

    // O conteúdo adversarial preservado não pode ser mutilado (pipe escapado,
    // quebras convertidas em <br>, emoji preservado).
    assert!(
        qa_csv.contains("&#124;"),
        "raw pipe must be escaped as entity: {qa_csv}"
    );
    assert!(
        qa_csv.contains("<br>"),
        "physical newlines must become <br>: {qa_csv}"
    );
    assert!(qa_csv.contains('🫀'), "emoji must survive: {qa_csv}");
}

#[test]
fn test_compile_drafts_partial_keeps_valid_rows_and_stats_when_middle_draft_fails() {
    // B16: um draft inválido no meio do lote não pode descartar o trabalho dos
    // demais. O resultado parcial deve trazer o erro do draft inválido, as linhas
    // dos dois drafts válidos e estatísticas coerentes.
    let valid_qa = base_qa_draft(&json!({"enunciado_ou_sentenca": "Pergunta válida QA?"}));
    let invalid = json!({"tipo": "INVALID", "enunciado_ou_sentenca": "Texto sem tipo suportado"});
    let valid_cloze = json!({
        "tipo": "CLOZE",
        "enunciado_ou_sentenca": "O {{c1::pneumotórax}} hipertensivo exige descompressão.",
    });
    let drafts = vec![valid_qa, invalid, valid_cloze];

    let partial = compile_drafts_partial(&drafts, true, true, "");

    assert_eq!(partial["partial"], json!(true), "partial flag must be true");
    let errors = partial["errors"].as_array().expect("errors array");
    assert_eq!(errors.len(), 1, "only the middle draft failed: {errors:?}");
    let error = errors[0].as_str().expect("error string");
    assert!(
        error.starts_with("drafts[1]:"),
        "error must point at the failing draft index: {error}"
    );

    let qa_csv = partial["qa_csv"].as_str().expect("qa_csv string");
    let cloze_csv = partial["cloze_csv"].as_str().expect("cloze_csv string");
    assert_eq!(
        qa_csv.lines().count(),
        2,
        "header + the single valid QA row: {qa_csv}"
    );
    assert_eq!(
        cloze_csv.lines().count(),
        2,
        "header + the single valid Cloze row: {cloze_csv}"
    );
    assert!(
        qa_csv.contains("Pergunta válida QA?"),
        "valid QA draft must be compiled: {qa_csv}"
    );
    assert!(
        cloze_csv.contains("pneumotórax"),
        "valid Cloze draft must be compiled: {cloze_csv}"
    );

    let stats = &partial["stats"];
    assert_eq!(stats["total_drafts"], json!(3));
    assert_eq!(stats["qa_count"], json!(1));
    assert_eq!(stats["cloze_count"], json!(1));
    assert_eq!(
        stats["total_drafts"].as_u64().unwrap()
            - (stats["qa_count"].as_u64().unwrap() + stats["cloze_count"].as_u64().unwrap()),
        1,
        "attempted minus compiled must equal the failure count"
    );

    // O contrato histórico continua "tudo ou nada" e a mensagem agregada é
    // exatamente `errors.join("; ")`.
    let legacy_err = compile_drafts(&drafts, true, true, "")
        .expect_err("historical contract still rejects partially invalid batches");
    assert_eq!(legacy_err, error, "legacy message equals errors.join");
}

#[test]
fn test_compile_drafts_partial_full_valid_batch_matches_success_contract() {
    // Sem regressão no caso 100% válido: nada de `partial: true` e as linhas,
    // estatísticas e CSV idênticos ao payload de sucesso.
    let drafts = vec![
        base_qa_draft(&json!({"etiquetas": ["cardio"], "campo_estudo": "Cardiologia"})),
        json!({
            "tipo": "CLOZE",
            "enunciado_ou_sentenca": "A {{c1::sístole}} segue a diástole.",
        }),
    ];

    let success = compile_drafts(&drafts, true, true, "Clínica").expect("valid batch compiles");
    let partial = compile_drafts_partial(&drafts, true, true, "Clínica");

    assert_eq!(partial["partial"], json!(false), "partial must be false");
    assert_eq!(partial["errors"], json!([]), "no errors on a valid batch");
    assert_eq!(partial["qa_csv"], success["qa_csv"], "qa_csv must match");
    assert_eq!(
        partial["cloze_csv"], success["cloze_csv"],
        "cloze_csv must match"
    );
    assert_eq!(partial["stats"], success["stats"], "stats must match");
}

#[test]
fn test_compile_drafts_partial_fully_invalid_batch_keeps_headers_and_zero_stats() {
    // Sem regressão no caso 100% inválido: nada compila, o payload traz apenas
    // cabeçalhos e estatísticas zeradas (mas `total_drafts` reflete a tentativa).
    let drafts = vec![
        json!({"tipo": "INVALID", "enunciado_ou_sentenca": "Um"}),
        json!({"tipo": "QA", "enunciado_ou_sentenca": "Sem resposta"}),
        json!({"tipo": "CLOZE"}),
    ];

    let partial = compile_drafts_partial(&drafts, true, true, "");

    assert_eq!(partial["partial"], json!(true));
    let errors = partial["errors"].as_array().expect("errors array");
    assert_eq!(
        errors.len(),
        3,
        "every draft must report its error: {errors:?}"
    );
    for (idx, error) in errors.iter().enumerate() {
        let error = error.as_str().expect("error string");
        assert!(
            error.starts_with(&format!("drafts[{idx}]:")),
            "errors must be ordered by draft index: {error}"
        );
    }

    assert_eq!(
        partial["qa_csv"].as_str().expect("qa_csv string"),
        anki_domain::QA_FIELDS.join("|"),
        "fully invalid batch keeps only the QA header"
    );
    assert_eq!(
        partial["cloze_csv"].as_str().expect("cloze_csv string"),
        anki_domain::CLOZE_FIELDS.join("|"),
        "fully invalid batch keeps only the Cloze header"
    );

    let stats = &partial["stats"];
    assert_eq!(stats["total_drafts"], json!(3));
    assert_eq!(stats["qa_count"], json!(0));
    assert_eq!(stats["cloze_count"], json!(0));
    assert_eq!(stats["renumbered_cloze"], json!(0));
    assert_eq!(stats["purged_id_leaks"], json!(0));
    assert_eq!(stats["illustrated_cards"], json!(0));
    assert_eq!(stats["tables_rendered"], json!(0));
}

#[test]
fn test_compile_drafts_partial_with_mode_isolates_strict_notes_failure() {
    // O modo estrito do compilador de notas falha por draft: o draft com campo
    // legado populado vira erro e o draft canônico continua compilando.
    let canonical = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Pergunta canônica?",
        "resposta": "Resposta canônica.",
        "notas_adicionais": {
            "blocos": [{"tipo": "paragrafo", "texto": "Nota canônica."}]
        }
    });
    let legacy = base_qa_draft(&json!({
        "notas_adicionais": {"fisiopatologia": "Campo legado populado."}
    }));
    let drafts = vec![canonical, legacy];

    let tolerant =
        compile_drafts_partial_with_mode(&drafts, true, true, "", false, CompileMode::StrictV2);
    assert_eq!(tolerant["partial"], json!(true), "legacy notes must fail");
    let errors = tolerant["errors"].as_array().expect("errors array");
    assert_eq!(errors.len(), 1, "single failing draft: {errors:?}");
    assert!(
        errors[0].as_str().unwrap().starts_with("drafts[1]:"),
        "the legacy-notes draft is the failure: {errors:?}"
    );
    assert_eq!(tolerant["stats"]["qa_count"], json!(1));
    let qa_csv = tolerant["qa_csv"].as_str().expect("qa_csv string");
    assert!(
        qa_csv.contains("Pergunta canônica?"),
        "canonical draft still compiles: {qa_csv}"
    );

    // All-strict-v2 batch: nenhum erro e `partial` falso.
    let canonical_only = compile_drafts_partial_with_mode(
        std::slice::from_ref(&drafts[0]),
        true,
        true,
        "",
        false,
        CompileMode::StrictV2,
    );
    assert_eq!(canonical_only["partial"], json!(false));
    assert_eq!(canonical_only["errors"], json!([]));
}
