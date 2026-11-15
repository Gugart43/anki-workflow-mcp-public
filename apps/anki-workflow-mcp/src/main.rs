//! `anki-workflow-mcp` CLI & differential oracle entrypoint (v2 bootstrap).

mod build_info;
mod mcp;

use anki_domain::{Diagnostic, NoteType, compute_receipt_v1};
use anki_parser::{count_batch_cards_with_profile, split_row};
use anki_validate::{
    MediaFigure, NoteFlow, check_cloze, check_crossrefs, check_csv, check_hints,
    check_images_field, check_notes, check_qa, check_tables_field, check_tags, compile_drafts,
    escape_cell, infer_campo_estudo, markdown_tables_to_html, partition_quarantine, patch_draft,
    purge_id_leak, render_media_figure, render_note_flow, renumber_cloze, review_batch_information,
    sanitize_field,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{self, Read};
use std::process::ExitCode;

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OracleRequest {
    ReceiptV1 {
        qa_csv: String,
        cloze_csv: String,
    },
    SplitRow {
        line: String,
        expected: usize,
    },
    CountBatchCards {
        qa_csv: String,
        cloze_csv: String,
        #[serde(default)]
        batch_id: Option<String>,
        #[serde(default)]
        profile: anki_domain::CompatibilityProfile,
    },
    ReviewBatchInformation {
        qa_csv: String,
        cloze_csv: String,
    },
    SanitizeField {
        text: String,
        #[serde(default)]
        is_images_field: bool,
        #[serde(default = "default_true")]
        convert_markdown: bool,
    },
    CheckCsv {
        csv_text: String,
        note_type: NoteType,
        #[serde(default = "default_true")]
        enforce_header: bool,
    },
    CheckCloze {
        sentence: String,
        #[serde(default)]
        row: Option<usize>,
        #[serde(default)]
        field: Option<String>,
    },
    CheckHints {
        sentence: String,
        #[serde(default)]
        row: Option<usize>,
        #[serde(default)]
        field: Option<String>,
    },
    CheckQa {
        enunciado: String,
        resposta: String,
        #[serde(default)]
        row: Option<usize>,
    },
    CheckNotes {
        notes: String,
        #[serde(default)]
        answer: String,
        #[serde(default)]
        row: Option<usize>,
    },
    CheckTags {
        tags_field: String,
        taxonomy_nodes: Option<Vec<String>>,
        #[serde(default)]
        row: Option<usize>,
        #[serde(default = "default_tags_field")]
        field: String,
    },
    CheckCrossrefs {
        text: String,
        active_ids: Option<Vec<String>>,
        current_batch_ids: Option<Vec<String>>,
        #[serde(default)]
        row: Option<usize>,
        field: Option<String>,
    },
    CheckImagesField {
        html: String,
        catalog_filenames: Option<Vec<String>>,
        #[serde(default)]
        row: Option<usize>,
        #[serde(default = "default_images_field")]
        field: String,
    },
    CheckTablesField {
        html: String,
        #[serde(default = "default_notes_field")]
        field: String,
        #[serde(default)]
        row: Option<usize>,
        #[serde(default)]
        face: bool,
    },
    EscapeCell {
        text: String,
    },
    MarkdownTablesToHtml {
        text: String,
    },
    PurgeIdLeak {
        text: String,
    },
    InferCampoEstudo {
        etiquetas: serde_json::Value,
    },
    RenumberCloze {
        text: String,
    },
    RenderMediaFigure {
        figure: MediaFigure,
    },
    RenderNoteFlow {
        flow: NoteFlow,
    },
    CompileDrafts {
        drafts: Vec<serde_json::Value>,
        #[serde(default = "default_true")]
        auto_renumber_cloze: bool,
        #[serde(default = "default_true")]
        auto_purge_id_leaks: bool,
        #[serde(default)]
        default_campo_estudo: String,
    },
    PatchDraft {
        draft: serde_json::Value,
        updates: serde_json::Map<String, serde_json::Value>,
    },
    PartitionQuarantine {
        drafts: Vec<serde_json::Value>,
        failed_indices: Vec<usize>,
        #[serde(default)]
        diagnostics: Vec<serde_json::Value>,
        #[serde(default = "default_quarantine_reason")]
        reason: String,
    },
}

fn default_tags_field() -> String {
    "Etiquetas".to_owned()
}
fn default_images_field() -> String {
    "Imagens Relacionadas".to_owned()
}
fn default_notes_field() -> String {
    "Notas Adicionais".to_owned()
}

fn default_true() -> bool {
    true
}

fn default_quarantine_reason() -> String {
    "Validation failed after repair budget".to_owned()
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum OracleResponse {
    ReceiptV1 {
        receipt: String,
    },
    SplitRow(anki_parser::SplitRow),
    CountBatchCards(anki_domain::BatchCount),
    BatchInformationReview(anki_validate::BatchInformationReview),
    SanitizeField {
        sanitized: String,
        fixes: Vec<String>,
    },
    CheckCsv {
        diagnostics: Vec<Diagnostic>,
    },
    Diagnostics {
        diagnostics: Vec<Diagnostic>,
    },
    Text {
        text: String,
    },
    TextAndCount {
        text: String,
        count: usize,
    },
    DraftResult(serde_json::Value),
}

fn run_oracle() -> Result<(), String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("failed to read stdin: {e}"))?;

    let requests: Vec<OracleRequest> =
        serde_json::from_str(&input).map_err(|e| format!("invalid oracle JSON: {e}"))?;

    let mut responses = Vec::with_capacity(requests.len());
    for req in requests {
        let resp = match req {
            OracleRequest::ReceiptV1 { qa_csv, cloze_csv } => OracleResponse::ReceiptV1 {
                receipt: compute_receipt_v1(&qa_csv, &cloze_csv).0,
            },
            OracleRequest::SplitRow { line, expected } => {
                OracleResponse::SplitRow(split_row(&line, expected))
            }
            OracleRequest::CountBatchCards {
                qa_csv,
                cloze_csv,
                batch_id,
                profile,
            } => OracleResponse::CountBatchCards(count_batch_cards_with_profile(
                &qa_csv,
                &cloze_csv,
                batch_id.as_deref(),
                profile,
            )),
            OracleRequest::ReviewBatchInformation { qa_csv, cloze_csv } => {
                OracleResponse::BatchInformationReview(review_batch_information(
                    &qa_csv, &cloze_csv,
                ))
            }
            OracleRequest::SanitizeField {
                text,
                is_images_field,
                convert_markdown,
            } => {
                let (sanitized, fixes) = sanitize_field(&text, is_images_field, convert_markdown);
                OracleResponse::SanitizeField { sanitized, fixes }
            }
            OracleRequest::CheckCsv {
                csv_text,
                note_type,
                enforce_header,
            } => OracleResponse::CheckCsv {
                diagnostics: check_csv(&csv_text, note_type, enforce_header),
            },
            OracleRequest::CheckCloze {
                sentence,
                row,
                field,
            } => OracleResponse::Diagnostics {
                diagnostics: check_cloze(&sentence, row, field.as_deref()),
            },
            OracleRequest::CheckHints {
                sentence,
                row,
                field,
            } => OracleResponse::Diagnostics {
                diagnostics: check_hints(&sentence, row, field.as_deref()),
            },
            OracleRequest::CheckQa {
                enunciado,
                resposta,
                row,
            } => OracleResponse::Diagnostics {
                diagnostics: check_qa(&enunciado, &resposta, row),
            },
            OracleRequest::CheckNotes { notes, answer, row } => OracleResponse::Diagnostics {
                diagnostics: check_notes(&notes, &answer, row),
            },
            OracleRequest::CheckTags {
                tags_field,
                taxonomy_nodes,
                row,
                field,
            } => OracleResponse::Diagnostics {
                diagnostics: check_tags(
                    &tags_field,
                    taxonomy_nodes
                        .as_ref()
                        .map(|ids| ids.iter().cloned().collect::<HashSet<_>>())
                        .as_ref(),
                    row,
                    &field,
                ),
            },
            OracleRequest::CheckCrossrefs {
                text,
                active_ids,
                current_batch_ids,
                row,
                field,
            } => OracleResponse::Diagnostics {
                diagnostics: check_crossrefs(
                    &text,
                    active_ids
                        .as_ref()
                        .map(|ids| ids.iter().cloned().collect::<HashSet<_>>())
                        .as_ref(),
                    current_batch_ids
                        .as_ref()
                        .map(|ids| ids.iter().cloned().collect::<HashSet<_>>())
                        .as_ref(),
                    row,
                    field.as_deref(),
                ),
            },
            OracleRequest::CheckImagesField {
                html,
                catalog_filenames,
                row,
                field,
            } => OracleResponse::Diagnostics {
                diagnostics: check_images_field(
                    &html,
                    catalog_filenames
                        .as_ref()
                        .map(|ids| ids.iter().cloned().collect::<HashSet<_>>())
                        .as_ref(),
                    row,
                    &field,
                ),
            },
            OracleRequest::CheckTablesField {
                html,
                field,
                row,
                face,
            } => OracleResponse::Diagnostics {
                diagnostics: check_tables_field(&html, &field, row, face),
            },
            OracleRequest::EscapeCell { text } => OracleResponse::Text {
                text: escape_cell(&text),
            },
            OracleRequest::MarkdownTablesToHtml { text } => {
                let (text, count) = markdown_tables_to_html(&text);
                OracleResponse::TextAndCount { text, count }
            }
            OracleRequest::PurgeIdLeak { text } => OracleResponse::Text {
                text: purge_id_leak(&text),
            },
            OracleRequest::InferCampoEstudo { etiquetas } => {
                let tags = match etiquetas {
                    serde_json::Value::Array(items) => items
                        .into_iter()
                        .map(|item| item.as_str().unwrap_or("").to_owned())
                        .collect::<Vec<_>>(),
                    serde_json::Value::String(text) => text
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect::<Vec<_>>(),
                    _ => Vec::new(),
                };
                OracleResponse::Text {
                    text: infer_campo_estudo(&tags),
                }
            }
            OracleRequest::RenumberCloze { text } => OracleResponse::Text {
                text: renumber_cloze(&text),
            },
            OracleRequest::RenderMediaFigure { figure } => OracleResponse::Text {
                text: render_media_figure(&figure),
            },
            OracleRequest::RenderNoteFlow { flow } => OracleResponse::Text {
                text: render_note_flow(&flow),
            },
            OracleRequest::CompileDrafts {
                drafts,
                auto_renumber_cloze,
                auto_purge_id_leaks,
                default_campo_estudo,
            } => OracleResponse::DraftResult(compile_drafts(
                &drafts,
                auto_renumber_cloze,
                auto_purge_id_leaks,
                &default_campo_estudo,
            )?),
            OracleRequest::PatchDraft { draft, updates } => OracleResponse::DraftResult(
                serde_json::json!({"card": patch_draft(&draft, &updates)?}),
            ),
            OracleRequest::PartitionQuarantine {
                drafts,
                failed_indices,
                diagnostics,
                reason,
            } => {
                let (valid, quarantined) =
                    partition_quarantine(&drafts, &failed_indices, &diagnostics, &reason)?;
                OracleResponse::DraftResult(
                    serde_json::json!({"valid": valid, "quarantined": quarantined}),
                )
            }
        };
        responses.push(resp);
    }

    let out = serde_json::to_string(&responses).map_err(|e| format!("serialize error: {e}"))?;
    println!("{out}");
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("version") | Some("--version") | Some("-V") => {
            let info = build_info::BuildInfo::current();
            println!("{} {}", info.implementation, info.package_version);
            ExitCode::SUCCESS
        }
        Some("build-info") | Some("--build-info") => {
            let info = build_info::BuildInfo::current();
            match info.to_json() {
                Ok(json) => {
                    println!("{json}");
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("failed to serialize build-info: {err}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("health") => {
            let info = build_info::BuildInfo::current();
            println!(
                "{{\"status\":\"ok\",\"version\":\"{}\",\"profile\":\"bootstrap\"}}",
                info.package_version
            );
            ExitCode::SUCCESS
        }
        Some("serve") => match mcp::run_stdio().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{err}");
                ExitCode::FAILURE
            }
        },
        Some("oracle") => match run_oracle() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{err}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("Usage: anki-workflow-mcp <version|build-info|health|oracle|serve>");
            ExitCode::from(2)
        }
    }
}
