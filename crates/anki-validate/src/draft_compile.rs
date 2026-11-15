use std::collections::HashSet;

use serde_json::{Value, json};

use crate::cloze_renumber::renumber_cloze;
use crate::draft_normalize::normalize_draft;
use crate::id_leak::purge_id_leak;
use crate::image_alias::unique_figures;
use crate::media_figure::render_media_figure;
use crate::notes_render::render_extra_notes;
use crate::sanitize::sanitize_field;
use crate::study_field::infer_campo_estudo;
use crate::table_render::{TableSpec, markdown_tables_to_html};

const QA_FIELDS: [&str; 7] = [
    "Enunciado",
    "Resposta",
    "Campo de Estudo",
    "Notas Adicionais",
    "Imagens Relacionadas",
    "Referências",
    "Etiquetas",
];

const CLOZE_FIELDS: [&str; 6] = [
    "Sentença",
    "Notas Adicionais",
    "Campo de Estudo",
    "Imagens Relacionadas",
    "Referências",
    "Etiquetas",
];

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

fn parse_table_spec(value: &Value) -> Result<TableSpec, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "tabela_notas must be an object".to_string())?;

    let caption = match obj.get("caption") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(_) => return Err("tabela_notas.caption must be a string".to_string()),
    };

    let headers_val = obj
        .get("headers")
        .and_then(Value::as_array)
        .ok_or_else(|| "tabela_notas.headers must be an array".to_string())?;
    let mut headers = Vec::with_capacity(headers_val.len());
    for h in headers_val {
        let s = h
            .as_str()
            .ok_or_else(|| "tabela_notas.headers items must be strings".to_string())?;
        headers.push(s.to_string());
    }

    let rows_val = obj
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| "tabela_notas.rows must be an array".to_string())?;
    let mut rows = Vec::with_capacity(rows_val.len());
    for r in rows_val {
        let row_arr = r
            .as_array()
            .ok_or_else(|| "tabela_notas.rows items must be arrays".to_string())?;
        let mut row = Vec::with_capacity(row_arr.len());
        for cell in row_arr {
            let s = cell
                .as_str()
                .ok_or_else(|| "tabela_notas.rows cells must be strings".to_string())?;
            row.push(s.to_string());
        }
        rows.push(row);
    }

    Ok(TableSpec {
        caption,
        headers,
        rows,
    })
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

        // 3. Format 'Notas Adicionais'
        let mut extra_source_refs: Vec<String> = Vec::new();
        let mut rendered_notes = match obj.get("notas_adicionais") {
            Some(Value::Object(_)) => {
                let notes_val = obj.get("notas_adicionais").unwrap();
                let (html, refs, tbl_count) = render_extra_notes(notes_val)?;
                tables_rendered += tbl_count;
                extra_source_refs = refs;
                html
            }
            Some(Value::String(s)) => {
                let (html, tbl_count) = markdown_tables_to_html(s);
                tables_rendered += tbl_count;
                html
            }
            Some(Value::Null) | None => String::new(),
            Some(_) => {
                return Err("notas_adicionais must be an object, string, or null".to_string());
            }
        };

        if let Some(tbl_val) = obj.get("tabela_notas")
            && !tbl_val.is_null()
        {
            let spec = parse_table_spec(tbl_val)?;
            let tbl_html = spec.render_html()?;
            if rendered_notes.is_empty() {
                rendered_notes = tbl_html;
            } else {
                rendered_notes = format!("{rendered_notes}<br>{tbl_html}");
            }
            tables_rendered += 1;
        }

        if let Some(tbls_val) = obj.get("tabelas_notas")
            && !tbls_val.is_null()
        {
            let arr = tbls_val
                .as_array()
                .ok_or_else(|| "tabelas_notas must be an array".to_string())?;
            for tbl_val in arr {
                let spec = parse_table_spec(tbl_val)?;
                let tbl_html = spec.render_html()?;
                if rendered_notes.is_empty() {
                    rendered_notes = tbl_html;
                } else {
                    rendered_notes = format!("{rendered_notes}<br>{tbl_html}");
                }
                tables_rendered += 1;
            }
        }

        let (mut clean_notes, _) = sanitize_field(&rendered_notes, false, true);

        if let Some(note_figs_val) = obj.get("figuras_notas")
            && !note_figs_val.is_null()
        {
            let note_figs = match note_figs_val {
                Value::Array(arr) => unique_figures(None, arr),
                Value::Object(_) => unique_figures(Some(note_figs_val), &[]),
                _ => return Err("figuras_notas must be an object, array, or null".to_string()),
            };
            if !note_figs.is_empty() {
                let mut figs_buf = String::new();
                for fig in &note_figs {
                    let figure: crate::media_figure::MediaFigure =
                        serde_json::from_value(fig.clone()).map_err(|e| e.to_string())?;
                    figs_buf.push_str(&render_media_figure(&figure));
                }
                let (clean_note_figs, _) = sanitize_field(&figs_buf, true, false);
                if !clean_note_figs.is_empty() {
                    if clean_notes.is_empty() {
                        clean_notes = clean_note_figs;
                    } else {
                        clean_notes = format!("{clean_notes}<br>{clean_note_figs}");
                    }
                }
            }
        }

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

        // 5. Format References and Tags
        let mut refs_str = match obj.get("referencias") {
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" &#124; "),
            Some(Value::String(s)) => s.trim().to_string(),
            Some(Value::Null) | None => String::new(),
            Some(_) => return Err("referencias must be an array, string, or null".to_string()),
        };

        if !extra_source_refs.is_empty() {
            let mut known_refs: HashSet<String> = refs_str
                .split(" &#124; ")
                .map(|s| s.trim().to_string())
                .collect();
            for r in extra_source_refs {
                if !known_refs.contains(&r) {
                    if refs_str.is_empty() {
                        refs_str = r.clone();
                    } else {
                        refs_str = format!("{refs_str} &#124; {r}");
                    }
                    known_refs.insert(r);
                }
            }
        }
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
                if !trimmed.is_empty() {
                    tags_vec.push(trimmed.to_string());
                }
                trimmed.to_string()
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
    fn unsupported_tipo_falls_back_to_qa_like_legacy() {
        let draft = json!({"tipo": "INVALID", "enunciado_ou_sentenca": "Texto"});
        let result =
            compile_drafts(&[draft], true, true, "").expect("legacy normalizes unknown type");
        assert_eq!(result["stats"]["qa_count"], 1);
        assert_eq!(result["stats"]["cloze_count"], 0);
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
        assert!(notes.contains("<table>"));
        assert!(notes.contains("<figure"));
        assert!(notes.contains("<figcaption>"));
        assert!(notes.contains("neuro_fluxo_sintetico.png"));
    }
}
