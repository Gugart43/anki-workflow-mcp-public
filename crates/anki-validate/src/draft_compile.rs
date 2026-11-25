use std::collections::HashSet;

use serde_json::{Value, json};

use crate::cloze_renumber::renumber_cloze;
use crate::draft_normalize::{normalize_draft, normalize_draft_strict};
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

fn has_unsafe_value(value: &Value) -> bool {
    match value {
        Value::String(text) => crate::html_security::has_unsafe_html(text),
        Value::Array(items) => items.iter().any(has_unsafe_value),
        Value::Object(items) => items.values().any(has_unsafe_value),
        _ => false,
    }
}

/// `<código>: <mensagem com o valor medido>` para uma violação de limite de campo HTML.
fn field_limit_error(text: &str) -> Option<String> {
    crate::html_security::html_field_limit_violation(text).map(|code| {
        format!(
            "{code}: {}",
            crate::html_security::html_field_limit_message(code, text)
        )
    })
}

/// Primeira violação de LIMITE de campo HTML (`HTML.FIELD_TOO_LARGE` / `HTML.NESTING_TOO_DEEP`)
/// na árvore, já formatada como `<código>: <mensagem com o valor medido>`.
///
/// O canal de erro deste compilador é `Vec<String>` (superfície pública, entregue como
/// `COMPILE.INVALID_DRAFT`), então o código específico viaja dentro da mensagem: sem isto um
/// draft legítimo e apenas grande era reportado como "Unsafe HTML attributes or active content",
/// o que é falso. Fail-closed: continua rejeitando.
fn field_limit_violation(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => field_limit_error(text),
        Value::Array(items) => items.iter().find_map(field_limit_violation),
        Value::Object(items) => items.values().find_map(field_limit_violation),
        _ => None,
    }
}

/// Primeira violação de limite entre campos já renderizados (linhas de CSV), formatada igual.
fn field_limit_violation_in_row<'a, I>(fields: I) -> Option<String>
where
    I: IntoIterator<Item = &'a String>,
{
    fields
        .into_iter()
        .find_map(|field| field_limit_error(field))
}

/// Chaves legadas aceitas pelo adaptador de notas (`legacy_adapter`).
const LEGACY_NOTES_KEYS: [&str; 6] = [
    "fisiopatologia",
    "raciocinio_clinico",
    "armadilhas_diferenciais",
    "correlacao_visual",
    "tabela",
    "texto_livre",
];

/// Compila drafts tolerantes (`normalize_draft`) com `CompileMode::LegacyCompat`.
pub fn compile_drafts(
    drafts: &[Value],
    auto_renumber_cloze: bool,
    auto_purge_id_leaks: bool,
    default_campo_estudo: &str,
) -> Result<Value, String> {
    compile_drafts_with_mode(
        drafts,
        auto_renumber_cloze,
        auto_purge_id_leaks,
        default_campo_estudo,
        false,
        CompileMode::LegacyCompat,
    )
}

/// Compila drafts estruturais (`normalize_draft_strict`) com `CompileMode::LegacyCompat`.
pub fn compile_drafts_strict(
    drafts: &[Value],
    auto_renumber_cloze: bool,
    auto_purge_id_leaks: bool,
    default_campo_estudo: &str,
) -> Result<Value, String> {
    compile_drafts_with_mode(
        drafts,
        auto_renumber_cloze,
        auto_purge_id_leaks,
        default_campo_estudo,
        true,
        CompileMode::LegacyCompat,
    )
}

/// Compila drafts controlando separadamente o rigor estrutural do draft
/// (`strict_drafts`) e o modo de compilação das notas adicionais (`compile_mode`).
///
/// `compile_mode = CompileMode::StrictV2` torna alcançável o contrato estrito do
/// `NoteDocument` (campos legados populados são rejeitados); `LegacyCompat`
/// preserva o comportamento histórico do compilador único.
pub fn compile_drafts_with_mode(
    drafts: &[Value],
    auto_renumber_cloze: bool,
    auto_purge_id_leaks: bool,
    default_campo_estudo: &str,
    strict_drafts: bool,
    compile_mode: CompileMode,
) -> Result<Value, String> {
    compile_drafts_inner(
        drafts,
        auto_renumber_cloze,
        auto_purge_id_leaks,
        default_campo_estudo,
        strict_drafts,
        compile_mode,
    )
}

/// Compila um lote de drafts tolerantes (`normalize_draft`) devolvendo o
/// resultado **parcial** em vez de descartar o trabalho dos drafts válidos.
///
/// O retorno é sempre um `Value` (nunca `Result`): os drafts que compilam
/// produzem linhas e estatísticas, e os que falham produzem entradas em
/// `errors`. Chaves devolvidas:
///
/// - `qa_csv` — CSV QA canônico com as linhas dos drafts compilados com sucesso;
///   traz somente o cabeçalho quando nenhum draft QA compila.
/// - `cloze_csv` — CSV Cloze canônico com as linhas dos drafts compilados com
///   sucesso; traz somente o cabeçalho quando nenhum draft Cloze compila.
/// - `stats` — mesmas chaves do payload de sucesso (`total_drafts`, `qa_count`,
///   `cloze_count`, `renumbered_cloze`, `purged_id_leaks`, `illustrated_cards`,
///   `tables_rendered`). `total_drafts` é o tamanho do lote tentado, portanto
///   `stats.qa_count + stats.cloze_count` é o número de drafts efetivamente
///   compilados e `stats.total_drafts - (qa_count + cloze_count)` é o número de
///   falhas.
/// - `errors` — erros por draft em ordem de índice, cada um no formato
///   `drafts[<índice>]: <motivo>`. `errors.join("; ")` reproduz exatamente a
///   mensagem que o contrato histórico devolvia como erro.
/// - `partial` — `true` quando `errors` não está vazio, ou seja, quando o lote
///   foi apenas parcialmente compilado; `false` em lote 100% válido.
pub fn compile_drafts_partial(
    drafts: &[Value],
    auto_renumber_cloze: bool,
    auto_purge_id_leaks: bool,
    default_campo_estudo: &str,
) -> Value {
    compile_drafts_partial_with_mode(
        drafts,
        auto_renumber_cloze,
        auto_purge_id_leaks,
        default_campo_estudo,
        false,
        CompileMode::LegacyCompat,
    )
}

/// Versão de [`compile_drafts_partial`] que controla o rigor estrutural dos
/// drafts (`strict_drafts`, ver `normalize_draft_strict`) e o modo do compilador
/// de notas (`compile_mode`).
///
/// O payload é o mesmo descrito em [`compile_drafts_partial`]; o resultado é
/// parcial (`partial: true`) quando qualquer draft falha na normalização, no
/// compilador de notas, na renderização de figuras ou na checagem de HTML
/// inseguro.
pub fn compile_drafts_partial_with_mode(
    drafts: &[Value],
    auto_renumber_cloze: bool,
    auto_purge_id_leaks: bool,
    default_campo_estudo: &str,
    strict_drafts: bool,
    compile_mode: CompileMode,
) -> Value {
    compile_batch_outcome(
        drafts,
        auto_renumber_cloze,
        auto_purge_id_leaks,
        default_campo_estudo,
        strict_drafts,
        compile_mode,
    )
    .partial_payload()
}

/// `true` quando o valor de uma chave legada não carrega conteúdo algum.
fn is_empty_notes_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        Value::Bool(_) | Value::Number(_) => false,
    }
}

/// Remove chaves legadas sem conteúdo antes de compilar em `CompileMode::StrictV2`.
///
/// O draft normalizado materializa *todas* as chaves legadas (`fisiopatologia`,
/// `texto_livre`, ...) como `null` mesmo quando o cliente não as enviou. O
/// adaptador estrito rejeita a mera presença da chave, então sem esta poda uma
/// nota canônica — ou a ausência de notas — seria recusada por ruído de
/// normalização. Chaves legadas populadas continuam sendo rejeitadas.
fn prune_empty_legacy_notes(notes: &Value) -> Value {
    let Value::Object(map) = notes else {
        return notes.clone();
    };

    let mut pruned = map.clone();
    for key in LEGACY_NOTES_KEYS {
        if matches!(pruned.get(key), Some(value) if is_empty_notes_value(value)) {
            pruned.remove(key);
        }
    }
    Value::Object(pruned)
}

/// Resultado bruto de uma compilação de lote, antes da decisão entre o contrato
/// histórico ("tudo ou nada", via [`compile_drafts_inner`]) e o contrato parcial
/// (via [`compile_drafts_partial_with_mode`]).
///
/// A compilação de um draft nunca interrompe o lote: cada draft que falha
/// acrescenta uma entrada em `errors` e os demais seguem sendo compilados. Os
/// dois payloads abaixo diferem apenas na forma de reportar essa falha.
struct BatchOutcome {
    /// CSV QA contendo somente as linhas dos drafts que compilaram com sucesso.
    qa_csv: String,
    /// CSV Cloze contendo somente as linhas dos drafts que compilaram com sucesso.
    cloze_csv: String,
    /// Estatísticas do lote: `total_drafts` conta os drafts tentados, enquanto as
    /// contagens de sucesso (`qa_count`, `cloze_count`) contam apenas os compilados.
    stats: Value,
    /// Erros por draft no formato `drafts[<índice>]: <motivo>`, na ordem dos drafts.
    errors: Vec<String>,
}

impl BatchOutcome {
    /// Payload histórico de sucesso: `qa_csv`, `cloze_csv` e `stats`, sem
    /// nenhuma chave adicional. Mantido byte-idêntico ao contrato anterior para
    /// não alterar o resultado de `cards_compile_drafts` quando todo o lote
    /// compila.
    fn success_payload(&self) -> Value {
        json!({
            "qa_csv": self.qa_csv,
            "cloze_csv": self.cloze_csv,
            "stats": self.stats,
        })
    }

    /// Payload parcial: o trabalho dos drafts válidos mais o relato das falhas.
    ///
    /// Chaves:
    /// - `qa_csv`: linhas QA compiladas com sucesso (só o cabeçalho se nenhuma).
    /// - `cloze_csv`: linhas Cloze compiladas com sucesso (só o cabeçalho se nenhuma).
    /// - `stats`: mesmas estatísticas do payload de sucesso; `total_drafts` é o
    ///   total tentado e `qa_count`/`cloze_count` somam apenas os compilados.
    /// - `errors`: lista de erros por draft em ordem de índice; `errors.join("; ")`
    ///   reproduz exatamente a mensagem do contrato histórico de erro.
    /// - `partial`: `true` sempre que `errors` não está vazio, isto é, sempre que o
    ///   lote foi apenas parcialmente compilado (`false` em lote 100% válido).
    fn partial_payload(&self) -> Value {
        json!({
            "qa_csv": self.qa_csv,
            "cloze_csv": self.cloze_csv,
            "stats": self.stats,
            "errors": self.errors,
            "partial": !self.errors.is_empty(),
        })
    }
}

/// Contrato histórico do compilador de lote: qualquer draft inválido descarta o
/// lote inteiro e vira `Err` com a mensagem agregada.
fn compile_drafts_inner(
    drafts: &[Value],
    auto_renumber_cloze: bool,
    auto_purge_id_leaks: bool,
    default_campo_estudo: &str,
    strict: bool,
    compile_mode: CompileMode,
) -> Result<Value, String> {
    let outcome = compile_batch_outcome(
        drafts,
        auto_renumber_cloze,
        auto_purge_id_leaks,
        default_campo_estudo,
        strict,
        compile_mode,
    );
    if !outcome.errors.is_empty() {
        return Err(outcome.errors.join("; "));
    }
    Ok(outcome.success_payload())
}

/// Compila o lote inteiro preservando o trabalho dos drafts válidos.
fn compile_batch_outcome(
    drafts: &[Value],
    auto_renumber_cloze: bool,
    auto_purge_id_leaks: bool,
    default_campo_estudo: &str,
    strict: bool,
    compile_mode: CompileMode,
) -> BatchOutcome {
    let mut qa_rows: Vec<Vec<String>> = Vec::new();
    let mut cloze_rows: Vec<Vec<String>> = Vec::new();

    let total_drafts = drafts.len();
    let mut qa_count: usize = 0;
    let mut cloze_count: usize = 0;
    let mut renumbered_cloze: usize = 0;
    let mut purged_id_leaks: usize = 0;
    let mut illustrated_cards: usize = 0;
    let mut tables_rendered: usize = 0;

    let mut errors: Vec<String> = Vec::new();

    for (idx, raw_draft) in drafts.iter().enumerate() {
        let card = if strict {
            normalize_draft_strict(raw_draft)
        } else {
            normalize_draft(raw_draft)
        };
        let card = match card {
            Ok(c) => c,
            Err(err) => {
                errors.push(format!("drafts[{idx}]: {err}"));
                continue;
            }
        };
        // Limite de campo tem diagnóstico PRÓPRIO (fail-closed): antes, código descartado e
        // campo legítimo de 17.007 bytes saía como "Unsafe HTML attributes".
        if let Some(limit_error) = field_limit_violation(&card) {
            errors.push(format!("drafts[{idx}]: {limit_error}"));
            continue;
        }
        if has_unsafe_value(&card) {
            errors.push(format!(
                "drafts[{idx}]: Unsafe HTML attributes or active content in draft"
            ));
            continue;
        }
        let Some(obj) = card.as_object() else {
            errors.push(format!(
                "drafts[{idx}]: Normalized draft must be a JSON object"
            ));
            continue;
        };

        let Some(tipo) = obj.get("tipo").and_then(Value::as_str) else {
            errors.push(format!("drafts[{idx}]: Draft missing string field 'tipo'"));
            continue;
        };
        if tipo != "QA" && tipo != "CLOZE" {
            errors.push(format!("drafts[{idx}]: Unsupported draft tipo: {tipo}"));
            continue;
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

        let mut draft_errors: Vec<String> = Vec::new();

        // 3. Format 'Notas Adicionais' via single compiler path (F2 / legacy_adapter)
        let pruned_notes = match compile_mode {
            CompileMode::StrictV2 => obj.get("notas_adicionais").map(prune_empty_legacy_notes),
            CompileMode::LegacyCompat => None,
        };
        let notas_adicionais = pruned_notes
            .as_ref()
            .or_else(|| obj.get("notas_adicionais"));
        let notes_input = DraftNotesInput {
            notas_adicionais,
            tabela_notas: obj.get("tabela_notas"),
            tabelas_notas: obj.get("tabelas_notas"),
            figuras_notas: obj.get("figuras_notas"),
        };
        let compiled_notes = match compile_extra_notes(&notes_input, compile_mode) {
            Ok(notes) => notes,
            Err(err) => {
                draft_errors.push(format!("drafts[{idx}]: {err}"));
                errors.extend(draft_errors);
                continue;
            }
        };

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
                match serde_json::from_value::<crate::media_figure::MediaFigure>(fig.clone()) {
                    Ok(figure) => buf.push_str(&render_media_figure(&figure)),
                    Err(err) => draft_errors.push(format!("drafts[{idx}]: {err}")),
                }
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
            Some(_) => {
                draft_errors.push(format!(
                    "drafts[{idx}]: referencias must be an array, string, or null"
                ));
            }
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
            Some(_) => {
                draft_errors.push(format!(
                    "drafts[{idx}]: etiquetas must be an array, string, or null"
                ));
                String::new()
            }
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
            let (clean_resp, _) = sanitize_field(&resposta, false, false);
            if clean_resp.trim().is_empty()
                || crate::html_text::html_to_visible_text(&clean_resp)
                    .trim()
                    .is_empty()
            {
                draft_errors.push(format!(
                    "drafts[{idx}]: QA draft must have a non-empty 'resposta'"
                ));
            }
            if !draft_errors.is_empty() {
                errors.extend(draft_errors);
                continue;
            }
            let row = vec![
                clean_stem,
                clean_resp,
                clean_field_study,
                clean_notes,
                clean_img,
                clean_refs,
                tags_str,
            ];
            if let Some(limit_error) = field_limit_violation_in_row(&row) {
                errors.push(format!("drafts[{idx}]: {limit_error}"));
                continue;
            }
            if row
                .iter()
                .any(|field| crate::html_security::has_unsafe_html(field))
            {
                errors.push(format!(
                    "drafts[{idx}]: Unsafe HTML attributes or active content in compiled draft"
                ));
                continue;
            }
            qa_count += 1;
            qa_rows.push(row);
        } else {
            if !draft_errors.is_empty() {
                errors.extend(draft_errors);
                continue;
            }
            let row = vec![
                clean_stem,
                clean_notes,
                clean_field_study,
                clean_img,
                clean_refs,
                tags_str,
            ];
            if let Some(limit_error) = field_limit_violation_in_row(&row) {
                errors.push(format!("drafts[{idx}]: {limit_error}"));
                continue;
            }
            if row
                .iter()
                .any(|field| crate::html_security::has_unsafe_html(field))
            {
                errors.push(format!(
                    "drafts[{idx}]: Unsafe HTML attributes or active content in compiled draft"
                ));
                continue;
            }
            cloze_count += 1;
            cloze_rows.push(row);
        }
    }

    let qa_csv = write_csv(&QA_FIELDS, &qa_rows);
    let cloze_csv = write_csv(&CLOZE_FIELDS, &cloze_rows);
    if let Some(limit_error) =
        field_limit_violation_in_row(qa_rows.iter().chain(&cloze_rows).flatten())
    {
        errors.push(limit_error);
    } else if qa_rows
        .iter()
        .chain(&cloze_rows)
        .flatten()
        .any(|field| crate::html_security::has_unsafe_html(field))
    {
        errors.push("Unsafe HTML attributes or active content in compiled draft".to_owned());
    }

    BatchOutcome {
        qa_csv,
        cloze_csv,
        stats: json!({
            "total_drafts": total_drafts,
            "qa_count": qa_count,
            "cloze_count": cloze_count,
            "renumbered_cloze": renumbered_cloze,
            "purged_id_leaks": purged_id_leaks,
            "illustrated_cards": illustrated_cards,
            "tables_rendered": tables_rendered,
        }),
        errors,
    }
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
                    "caption": "Fluxo sintético de decisão diagnóstica."
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

    #[test]
    fn compile_drafts_strict_rejects_unknown_fields() {
        let draft = json!({
            "tipo": "QA",
            "enunciado_ou_sentenca": "Pergunta?",
            "resposta": "Resposta.",
            "extra_field": 1
        });
        let err = compile_drafts_strict(std::slice::from_ref(&draft), true, true, "")
            .expect_err("strict should reject extra_field");
        assert!(err.contains("drafts[0]: unknown field `extra_field`"));

        // Tolerant compiler accepts it
        let ok = compile_drafts(&[draft], true, true, "");
        assert!(ok.is_ok());
    }

    #[test]
    fn compile_drafts_aggregates_multiple_indexed_errors() {
        let draft0 = json!({
            "tipo": "INVALID",
            "enunciado_ou_sentenca": "Texto 0"
        });
        let draft1 = json!({
            "tipo": "QA",
            "enunciado_ou_sentenca": "Texto 1",
            "resposta": "   "
        });
        let err = compile_drafts(&[draft0, draft1], true, true, "")
            .expect_err("multiple errors must be collected");
        assert!(err.contains("drafts[0]: Unsupported draft tipo: 'INVALID'"));
        assert!(err.contains("drafts[1]: QA draft must have a non-empty 'resposta'"));
        assert!(err.contains("; "));
    }

    #[test]
    fn compile_drafts_rejects_empty_and_whitespace_and_empty_html_qa_answers() {
        for empty_val in ["", "   ", "<b>  </b>", "<p></p>"] {
            let draft = json!({
                "tipo": "QA",
                "enunciado_ou_sentenca": "Pergunta?",
                "resposta": empty_val
            });
            let err = compile_drafts(&[draft], true, true, "")
                .expect_err("empty visible resposta must be rejected");
            assert!(
                err.contains("drafts[0]: QA draft must have a non-empty 'resposta'"),
                "failed for: {empty_val}"
            );
        }
    }

    #[test]
    fn compile_drafts_rejects_unsafe_html_before_and_after_compilation() {
        let unsafe_draft = json!({
            "tipo": "QA",
            "enunciado_ou_sentenca": "<script>alert('xss')</script>Pergunta?",
            "resposta": "Resposta."
        });
        let err = compile_drafts(&[unsafe_draft], true, true, "")
            .expect_err("unsafe draft must be rejected");
        assert!(err.contains("drafts[0]: Unsafe HTML attributes or active content in draft"));
    }
}
