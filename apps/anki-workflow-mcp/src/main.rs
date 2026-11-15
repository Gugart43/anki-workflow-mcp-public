//! `anki-workflow-mcp` CLI & differential oracle entrypoint (v2 bootstrap).

use anki_domain::compute_receipt_v1;
use anki_parser::{count_batch_cards, split_row};
use serde::{Deserialize, Serialize};
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
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum OracleResponse {
    ReceiptV1 { receipt: String },
    SplitRow(anki_parser::SplitRow),
    CountBatchCards(anki_domain::BatchCount),
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
