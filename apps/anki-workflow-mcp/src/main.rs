//! `anki-workflow-mcp` CLI & differential oracle entrypoint (v2 bootstrap).

use anki_domain::{Diagnostic, NoteType, compute_receipt_v1};
use anki_parser::{count_batch_cards, split_row};
use anki_validate::{
    check_cloze, check_crossrefs, check_csv, check_hints, check_images_field, check_notes,
    check_qa, check_tables_field, check_tags, escape_cell, markdown_tables_to_html, sanitize_field,
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

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum OracleResponse {
    ReceiptV1 {
        receipt: String,
    },
    SplitRow(anki_parser::SplitRow),
    CountBatchCards(anki_domain::BatchCount),
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
            } => OracleResponse::CountBatchCards(count_batch_cards(
                &qa_csv,
                &cloze_csv,
                batch_id.as_deref(),
            )),
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
        };
        responses.push(resp);
    }

    let out = serde_json::to_string(&responses).map_err(|e| format!("serialize error: {e}"))?;
    println!("{out}");
    Ok(())
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("version") | Some("--version") | Some("-V") => {
            println!("anki-workflow-mcp {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("health") => {
            println!(
                "{{\"status\":\"ok\",\"version\":\"{}\",\"profile\":\"bootstrap\"}}",
                env!("CARGO_PKG_VERSION")
            );
            ExitCode::SUCCESS
        }
        Some("oracle") => match run_oracle() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{err}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("Usage: anki-workflow-mcp <version|health|oracle>");
            ExitCode::from(2)
        }
    }
}
