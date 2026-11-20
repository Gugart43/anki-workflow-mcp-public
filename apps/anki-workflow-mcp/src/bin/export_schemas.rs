//! Deterministic JSON Schema exporter for `anki-workflow-mcp` v2 contracts.
//!
//! Emits canonical UTF-8 JSON Schema Draft 2020-12 files with LF (`\n`) line endings
//! into `contracts/v2/schemas/` (or a custom directory passed via `--out-dir <path>`).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anki_domain::{
    BatchCount, CompatibilityProfile, ContentDigest, Diagnostic, EvidenceScope, NoteType,
    ReviewVerdict, Severity, ValidationResult,
};
use anki_validate::{
    BatchInformationReview, CompileMode, DomainProfile, NoteDocument, QualityInput, QualityMode,
    QualityPrepareInput, QualityReport,
};
use anki_workflow_mcp::export::{ExportReviewedFilesArgs, ExportReviewedFilesResult};
use anki_workflow_mcp::mcp::{
    BatchPatchDraftArgs, CompileDraftsArgs, EmptyArgs, QualityEvaluateArgs, QualityPrepareArgs,
    ReviewBatchInformationArgs, ToolErrorPayload,
};
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value};

pub struct SchemaEntry {
    pub filename: &'static str,
    pub build: fn() -> Value,
}

pub fn build_schema<T: JsonSchema>() -> Value {
    let settings = SchemaSettings::draft2020_12();
    let raw = settings
        .into_generator()
        .into_root_schema_for::<T>()
        .to_value();
    canonicalize_value(raw)
}

/// Recursively sorts every JSON object's keys lexicographically so output is
/// byte-identical regardless of `serde_json` map features or platform.
pub fn canonicalize_value(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<String, Value> = map
                .into_iter()
                .map(|(k, v)| (k, canonicalize_value(v)))
                .collect();
            let mut out = Map::with_capacity(sorted.len());
            for (k, v) in sorted {
                out.insert(k, v);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize_value).collect()),
        other => other,
    }
}

pub fn default_out_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("contracts")
        .join("v2")
        .join("schemas")
}

pub fn parse_out_dir_from_iter<I>(mut args: I) -> Result<PathBuf, String>
where
    I: Iterator<Item = String>,
{
    let Some(first) = args.next() else {
        return Ok(default_out_dir());
    };
    if first == "--out-dir" {
        let path = args
            .next()
            .ok_or_else(|| "missing value after --out-dir".to_owned())?;
        if let Some(extra) = args.next() {
            return Err(format!("unexpected argument: {extra}"));
        }
        return Ok(PathBuf::from(path));
    }
    Err(format!(
        "usage: export-schemas [--out-dir <path>] (got unexpected argument '{first}')"
    ))
}

pub fn parse_out_dir() -> Result<PathBuf, String> {
    parse_out_dir_from_iter(std::env::args().skip(1))
}

pub fn schema_registry() -> Vec<SchemaEntry> {
    vec![
        // anki-domain
        SchemaEntry {
            filename: "batch_count.schema.json",
            build: build_schema::<BatchCount>,
        },
        SchemaEntry {
            filename: "compatibility_profile.schema.json",
            build: build_schema::<CompatibilityProfile>,
        },
        SchemaEntry {
            filename: "content_digest.schema.json",
            build: build_schema::<ContentDigest>,
        },
        SchemaEntry {
            filename: "diagnostic.schema.json",
            build: build_schema::<Diagnostic>,
        },
        SchemaEntry {
            filename: "evidence_scope.schema.json",
            build: build_schema::<EvidenceScope>,
        },
        SchemaEntry {
            filename: "note_type.schema.json",
            build: build_schema::<NoteType>,
        },
        SchemaEntry {
            filename: "review_verdict.schema.json",
            build: build_schema::<ReviewVerdict>,
        },
        SchemaEntry {
            filename: "severity.schema.json",
            build: build_schema::<Severity>,
        },
        SchemaEntry {
            filename: "validation_result.schema.json",
            build: build_schema::<ValidationResult>,
        },
        // anki-validate
        SchemaEntry {
            filename: "batch_information_review.schema.json",
            build: build_schema::<BatchInformationReview>,
        },
        SchemaEntry {
            filename: "compile_mode.schema.json",
            build: build_schema::<CompileMode>,
        },
        SchemaEntry {
            filename: "domain_profile.schema.json",
            build: build_schema::<DomainProfile>,
        },
        SchemaEntry {
            filename: "note_document.schema.json",
            build: build_schema::<NoteDocument>,
        },
        SchemaEntry {
            filename: "quality_input.schema.json",
            build: build_schema::<QualityInput>,
        },
        SchemaEntry {
            filename: "quality_mode.schema.json",
            build: build_schema::<QualityMode>,
        },
        SchemaEntry {
            filename: "quality_prepare_input.schema.json",
            build: build_schema::<QualityPrepareInput>,
        },
        SchemaEntry {
            filename: "quality_report.schema.json",
            build: build_schema::<QualityReport>,
        },
        // anki-workflow-mcp (MCP tool args & error payloads)
        SchemaEntry {
            filename: "mcp_batch_patch_draft_args.schema.json",
            build: build_schema::<BatchPatchDraftArgs>,
        },
        SchemaEntry {
            filename: "mcp_compile_drafts_args.schema.json",
            build: build_schema::<CompileDraftsArgs>,
        },
        SchemaEntry {
            filename: "mcp_empty_args.schema.json",
            build: build_schema::<EmptyArgs>,
        },
        SchemaEntry {
            filename: "mcp_export_reviewed_files_args.schema.json",
            build: build_schema::<ExportReviewedFilesArgs>,
        },
        SchemaEntry {
            filename: "mcp_export_reviewed_files_result.schema.json",
            build: build_schema::<ExportReviewedFilesResult>,
        },
        SchemaEntry {
            filename: "mcp_quality_evaluate_args.schema.json",
            build: build_schema::<QualityEvaluateArgs>,
        },
        SchemaEntry {
            filename: "mcp_quality_prepare_args.schema.json",
            build: build_schema::<QualityPrepareArgs>,
        },
        SchemaEntry {
            filename: "mcp_review_batch_information_args.schema.json",
            build: build_schema::<ReviewBatchInformationArgs>,
        },
        SchemaEntry {
            filename: "mcp_tool_error_payload.schema.json",
            build: build_schema::<ToolErrorPayload>,
        },
    ]
}

pub fn serialize_lf(value: &Value) -> Result<String, serde_json::Error> {
    let pretty = serde_json::to_string_pretty(value)?;
    let normalized = pretty.replace("\r\n", "\n");
    Ok(format!("{normalized}\n"))
}

pub fn export_entries(out_dir: &Path, mut entries: Vec<SchemaEntry>) -> Result<usize, String> {
    fs::create_dir_all(out_dir).map_err(|err| {
        format!(
            "failed to create output directory {}: {err}",
            out_dir.display()
        )
    })?;

    entries.sort_by_key(|entry| entry.filename);

    let mut expected_files = BTreeSet::new();
    for entry in &entries {
        if !expected_files.insert(entry.filename) {
            return Err(format!("duplicate schema filename: {}", entry.filename));
        }
        let schema = (entry.build)();
        let content = serialize_lf(&schema)
            .map_err(|err| format!("failed to serialize {}: {err}", entry.filename))?;
        let target = out_dir.join(entry.filename);
        fs::write(&target, content.as_bytes())
            .map_err(|err| format!("failed to write {}: {err}", target.display()))?;
    }

    // Remove stale *.schema.json files so renamed/removed contracts cannot linger.
    let dir_entries = fs::read_dir(out_dir)
        .map_err(|err| format!("failed to read {}: {err}", out_dir.display()))?;
    for item in dir_entries {
        let item = item.map_err(|err| format!("failed to inspect directory entry: {err}"))?;
        let path = item.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name.ends_with(".schema.json")
            && !expected_files.contains(name)
        {
            fs::remove_file(&path).map_err(|err| {
                format!("failed to remove stale schema {}: {err}", path.display())
            })?;
        }
    }

    Ok(entries.len())
}

pub fn export_all(out_dir: &Path) -> Result<usize, String> {
    export_entries(out_dir, schema_registry())
}

fn main() -> ExitCode {
    let out_dir = match parse_out_dir() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("export-schemas error: {err}");
            return ExitCode::from(2);
        }
    };

    match export_all(&out_dir) {
        Ok(count) => {
            println!(
                "exported {count} deterministic JSON schemas to {}",
                out_dir.display()
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("export-schemas error: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_canonicalize_value_sorts_keys_recursively() {
        let input = json!({
            "z": 1,
            "b": { "y": 2, "a": 1 },
            "a": [3, { "d": 4, "c": 5 }]
        });
        let canonical = canonicalize_value(input);
        if let Value::Object(map) = canonical {
            let keys: Vec<_> = map.keys().map(String::as_str).collect();
            assert_eq!(keys, vec!["a", "b", "z"]);
            if let Value::Object(b_map) = &map["b"] {
                let b_keys: Vec<_> = b_map.keys().map(String::as_str).collect();
                assert_eq!(b_keys, vec!["a", "y"]);
            } else {
                panic!("expected object for b");
            }
        } else {
            panic!("expected object");
        }
    }

    #[test]
    fn test_serialize_lf_produces_only_lf() {
        let val = json!({ "hello": "world", "nested": { "foo": "bar" } });
        let out = serialize_lf(&val).expect("serialization succeeds");
        assert!(!out.contains("\r\n"), "must not contain CRLF");
        assert!(out.ends_with('\n'), "must end with LF");
    }

    #[test]
    fn test_parse_out_dir_custom_and_errors() {
        let args = vec!["--out-dir".to_string(), "my/custom/path".to_string()];
        let parsed = parse_out_dir_from_iter(args.into_iter()).unwrap();
        assert_eq!(parsed, PathBuf::from("my/custom/path"));

        let args_unexpected = vec!["--unknown".to_string()];
        let err = parse_out_dir_from_iter(args_unexpected.into_iter()).unwrap_err();
        assert!(err.contains("unexpected argument"));

        let args_extra = vec![
            "--out-dir".to_string(),
            "path".to_string(),
            "extra".to_string(),
        ];
        let err_extra = parse_out_dir_from_iter(args_extra.into_iter()).unwrap_err();
        assert!(err_extra.contains("unexpected argument: extra"));
    }

    #[test]
    fn test_export_stale_deletion_and_retention() {
        let temp_dir = tempfile::tempdir().expect("tempdir creates");
        let path = temp_dir.path();

        let stale_file = path.join("old_contract.schema.json");
        let non_schema = path.join("README.txt");
        fs::write(&stale_file, b"stale").unwrap();
        fs::write(&non_schema, b"keep me").unwrap();

        let dummy_entries = vec![SchemaEntry {
            filename: "valid.schema.json",
            build: || json!({ "title": "Valid", "type": "string" }),
        }];

        let count = export_entries(path, dummy_entries).expect("export succeeds");
        assert_eq!(count, 1);

        assert!(!stale_file.exists(), "stale .schema.json must be deleted");
        assert!(non_schema.exists(), "non-schema file must be preserved");
        assert!(
            path.join("valid.schema.json").exists(),
            "exported schema must exist"
        );
    }

    #[test]
    fn test_duplicate_schema_filename_detection() {
        let temp_dir = tempfile::tempdir().expect("tempdir creates");
        let duplicate_entries = vec![
            SchemaEntry {
                filename: "duplicate.schema.json",
                build: || json!({ "title": "First" }),
            },
            SchemaEntry {
                filename: "duplicate.schema.json",
                build: || json!({ "title": "Second" }),
            },
        ];

        let err = export_entries(temp_dir.path(), duplicate_entries).unwrap_err();
        assert!(err.contains("duplicate schema filename"));
    }
}
