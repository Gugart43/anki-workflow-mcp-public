use anki_validate::draft_compile::compile_drafts;
use anki_validate::draft_normalize::normalize_draft;
use anki_validate::draft_patch::patch_draft;
use serde_json::{Map, json};

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
