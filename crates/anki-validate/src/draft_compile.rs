use std::collections::HashSet;

use serde_json::{Value, json};

use crate::cloze_renumber::renumber_cloze;
use crate::draft_normalize::normalize_draft;
use crate::id_leak::purge_id_leak;
use crate::image_alias::unique_figures;
use crate::legacy_adapter::{DraftNotesInput, compile_extra_notes};
use crate::media_figure::render_media_figure;
use crate::note_model::CompileMode;
use crate::sanitize::sanitize_field;
use crate::study_field::infer_campo_estudo;

use anki_domain::{CLOZE_FIELDS, QA_FIELDS};

fn write_csv(headers: &[&str], rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return headers.join("|");
    }
    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(headers.join("|"));
    for row in rows {
        lines.push(row.join("|"));
    }
    lines.join("\n")
}

pub fn compile_drafts(
    drafts: &[Value],
    auto_renumber_cloze: bool,
    auto_purge_id_leaks: bool,
    default_campo_estudo: &str,
) -> Result<Value, String> {
    let mut qa_rows: Vec<Vec<String>> = Vec::new();
    let mut cloze_rows: Vec<Vec<String>> = Vec::new();

    let total_drafts = drafts.len();
    let mut qa_count: usize = 0;
    let mut cloze_count: usize = 0;
    let mut renumbered_cloze: usize = 0;
    let mut purged_id_leaks: usize = 0;
    let mut illustrated_cards: usize = 0;
    let mut tables_rendered: usize = 0;

    for raw_draft in drafts {
        let card = normalize_draft(raw_draft)?;
        fn unsafe_value(value: &Value) -> bool {
            match value {
                Value::String(text) => crate::html_security::has_unsafe_html(text),
                Value::Array(items) => items.iter().any(unsafe_value),
                Value::Object(items) => items.values().any(unsafe_value),
                _ => false,
            }
        }
        if unsafe_value(&card) {
            return Err("Unsafe HTML attributes or active content in draft".to_owned());
        }
        let obj = card
            .as_object()
            .ok_or_else(|| "Normalized draft must be a JSON object".to_string())?;

        let tipo = obj
            .get("tipo")
            .and_then(Value::as_str)
            .ok_or_else(|| "Draft missing string field 'tipo'".to_string())?;
        if tipo != "QA" && tipo != "CLOZE" {
            return Err(format!("Unsupported draft tipo: {tipo}"));
        }

        let mut stem = obj
            .get("enunciado_ou_sentenca")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let mut resposta = obj
            .get("resposta")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        // 1. Purge ID leaks if requested
        if auto_purge_id_leaks {
            let purged_stem = purge_id_leak(&stem);
            if purged_stem != stem {
                purged_id_leaks += 1;
                stem = purged_stem;
            }
            if !resposta.is_empty() {
                let purged_resp = purge_id_leak(&resposta);
                if purged_resp != resposta {
                    purged_id_leaks += 1;
                    resposta = purged_resp;
                }
            }
        }

        // 2. Sequential Cloze renumbering
        if auto_renumber_cloze && tipo == "CLOZE" {
            let renumbered = renumber_cloze(&stem);
            if renumbered != stem {
                renumbered_cloze += 1;
                stem = renumbered;
            }
        }

        // 3. Format 'Notas Adicionais' via single compiler path (F2 / legacy_adapter)
        let notes_input = DraftNotesInput {
            notas_adicionais: obj.get("notas_adicionais"),
            tabela_notas: obj.get("tabela_notas"),
            tabelas_notas: obj.get("tabelas_notas"),
            figuras_notas: obj.get("figuras_notas"),
        };
        let compiled_notes = compile_extra_notes(&notes_input, CompileMode::LegacyCompat)
            .map_err(|e| e.to_string())?;

        tables_rendered += compiled_notes.rendered.table_count;

        // Exactly one sanitization allowing notes figures and tables
        let (clean_notes, _) = sanitize_field(&compiled_notes.rendered.html, true, false);

        // 4. Format 'Imagens Relacionadas'
        let figs = unique_figures(
            obj.get("figura"),
            obj.get("figuras")
                .and_then(Value::as_array)
                .map_or(&[], Vec::as_slice),
        );
        let rendered_img = if !figs.is_empty() {
            illustrated_cards += 1;
            let mut buf = String::new();
            for fig in &figs {
                let figure: crate::media_figure::MediaFigure =
                    serde_json::from_value(fig.clone()).map_err(|e| e.to_string())?;
                buf.push_str(&render_media_figure(&figure));
            }
            buf
        } else {
            String::new()
        };
        let (clean_img, _) = sanitize_field(&rendered_img, true, false);

        // 5. Format References and Tags with exact deduplication
        let mut all_refs: Vec<String> = Vec::new();
        let mut seen_refs: HashSet<String> = HashSet::new();

        match obj.get("referencias") {
            Some(Value::Array(arr)) => {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        let trimmed = s.trim();
                        if !trimmed.is_empty() && seen_refs.insert(trimmed.to_string()) {
                            all_refs.push(trimmed.to_string());
                        }
                    }
                }
            }
            Some(Value::String(s)) => {
                for part in s.split("&#124;") {
                    let trimmed = part.trim();
                    if !trimmed.is_empty() && seen_refs.insert(trimmed.to_string()) {
                        all_refs.push(trimmed.to_string());
                    }
                }
            }
            Some(Value::Null) | None => {}
            Some(_) => return Err("referencias must be an array, string, or null".to_string()),
        }

        for r in compiled_notes.rendered.source_refs {
            let trimmed = r.trim().to_string();
            if !trimmed.is_empty() && seen_refs.insert(trimmed.clone()) {
                all_refs.push(trimmed);
            }
        }

        let refs_str = all_refs.join(" &#124; ");
        let (clean_refs, _) = sanitize_field(&refs_str, false, false);

        let mut tags_vec: Vec<String> = Vec::new();
        let mut tags_str = match obj.get("etiquetas") {
            Some(Value::Array(arr)) => {
                let mut parts = Vec::new();
                for item in arr {
                    if let Some(s) = item.as_str() {
                        let trimmed = s.trim();
                        if !trimmed.is_empty() {
                            tags_vec.push(trimmed.to_string());
                            parts.push(trimmed.replace(' ', "_"));
                        }
                    }
                }
                parts.join(" ")
            }
            Some(Value::String(s)) => {
                let trimmed = s.trim();
                let mut parts = Vec::new();
                for word in trimmed.split_whitespace() {
                    tags_vec.push(word.to_string());
                    parts.push(word.to_string());
                }
                parts.join(" ")
            }
            Some(Value::Null) | None => String::new(),
            Some(_) => return Err("etiquetas must be an array, string, or null".to_string()),
        };

        if let Some(id_val) = obj.get("id").and_then(Value::as_str) {
            let id_trimmed = id_val.trim();
            if !id_trimmed.is_empty() {
                let card_id_tag = format!("id::{id_trimmed}");
                if !tags_str.split_whitespace().any(|t| t == card_id_tag) {
                    if tags_str.is_empty() {
                        tags_str = card_id_tag;
                    } else {
                        tags_str = format!("{tags_str} {card_id_tag}");
                    }
                }
            }
        }

        // 6. Sanitize main fields
        let (clean_stem, _) = sanitize_field(&stem, false, false);

        let mut raw_campo = obj
            .get("campo_estudo")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if raw_campo.is_empty() && !default_campo_estudo.trim().is_empty() {
            raw_campo = default_campo_estudo.trim().to_string();
        }
        if raw_campo.is_empty() {
            raw_campo = infer_campo_estudo(&tags_vec);
        }
        let (clean_field_study, _) = sanitize_field(&raw_campo, false, false);

        if tipo == "QA" {
            qa_count += 1;
            let (clean_resp, _) = sanitize_field(&resposta, false, false);
            qa_rows.push(vec![
                clean_stem,
                clean_resp,
                clean_field_study,
                clean_notes,
                clean_img,
                clean_refs,
                tags_str,
            ]);
        } else {
            cloze_count += 1;
            cloze_rows.push(vec![
                clean_stem,
                clean_notes,
                clean_field_study,
                clean_img,
                clean_refs,
                tags_str,
            ]);
        }
    }

    let qa_csv = write_csv(&QA_FIELDS, &qa_rows);
    let cloze_csv = write_csv(&CLOZE_FIELDS, &cloze_rows);
    if qa_rows
        .iter()
        .chain(&cloze_rows)
        .flatten()
        .any(|field| crate::html_security::has_unsafe_html(field))
    {
        return Err("Unsafe HTML attributes or active content in compiled draft".to_owned());
    }

    Ok(json!({
        "qa_csv": qa_csv,
        "cloze_csv": cloze_csv,
        "stats": {
            "total_drafts": total_drafts,
            "qa_count": qa_count,
            "cloze_count": cloze_count,
            "renumbered_cloze": renumbered_cloze,
            "purged_id_leaks": purged_id_leaks,
            "illustrated_cards": illustrated_cards,
            "tables_rendered": tables_rendered,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_drafts_returns_headers_only() {
        let res = compile_drafts(&[], true, true, "").expect("empty drafts should succeed");
        assert_eq!(
            res["qa_csv"].as_str().unwrap(),
            "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas"
        );
        assert_eq!(
            res["cloze_csv"].as_str().unwrap(),
            "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas"
        );
        assert_eq!(res["stats"]["total_drafts"], 0);
        assert_eq!(res["stats"]["qa_count"], 0);
        assert_eq!(res["stats"]["cloze_count"], 0);
    }

    #[test]
    fn malformed_table_spec_returns_err() {
        let bad_draft = json!({
            "tipo": "QA",
            "enunciado_ou_sentenca": "Pergunta?",
            "resposta": "Resposta.",
            "tabela_notas": {
                "caption": "Tabela",
                "headers": [],
                "rows": []
            }
        });
        let err = compile_drafts(&[bad_draft], true, true, "Clínica")
            .expect_err("empty headers in TableSpec must return Err");
        assert!(err.contains("headers"));
    }

    #[test]
    fn unsupported_tipo_returns_err() {
        let draft = json!({"tipo": "INVALID", "enunciado_ou_sentenca": "Texto"});
        assert!(compile_drafts(&[draft], true, true, "").is_err());
    }

    #[test]
    fn id_tag_check_uses_exact_token_matching() {
        let draft = json!({
            "tipo": "QA",
            "id": "B01-QA-1",
            "enunciado_ou_sentenca": "Pergunta sintética?",
            "resposta": "Resposta sintética.",
            "etiquetas": ["id::B01-QA-10", "Neurologia"]
        });
        let result = compile_drafts(&[draft], true, true, "").expect("compile should succeed");
        let qa_csv = result["qa_csv"].as_str().unwrap();
        let tags = qa_csv.lines().nth(1).unwrap().split('|').nth(6).unwrap();
        assert!(tags.split_whitespace().any(|t| t == "id::B01-QA-10"));
        assert!(tags.split_whitespace().any(|t| t == "id::B01-QA-1"));
    }

    #[test]
    fn figuras_notas_and_tabelas_notas_render_safely_in_notes() {
        let draft = json!({
            "tipo": "QA",
            "enunciado_ou_sentenca": "Pergunta sintética?",
            "resposta": "Resposta sintética.",
            "notas_adicionais": "Resumo clínico sintético.",
            "tabelas_notas": [
                {
                    "caption": "Tabela A",
                    "headers": ["Critério", "Valor"],
                    "rows": [["X", "1"]]
                },
                {
                    "caption": "Tabela B",
                    "headers": ["Conduta", "Prazo"],
                    "rows": [["Y", "Imediato"]]
                }
            ],
            "figuras_notas": [
                {
                    "filename": "neuro_fluxo_sintetico.png",
                    "caption": "Fluxo sintético de decisão diagnóstica.",
                    "alt": "Diagrama de decisão diagnóstica sintético"
                }
            ]
        });
        let result = compile_drafts(&[draft], true, true, "").expect("compile should succeed");
        assert_eq!(result["stats"]["tables_rendered"], 2);
        let qa_csv = result["qa_csv"].as_str().unwrap();
        let notes = qa_csv.lines().nth(1).unwrap().split('|').nth(3).unwrap();
        assert!(notes.contains("<table class=\"anki-table\">"));
        assert!(notes.contains("<figure"));
        assert!(notes.contains("<figcaption>"));
        assert!(notes.contains("neuro_fluxo_sintetico.png"));
    }
}
