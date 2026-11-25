//! Deterministic JSON Schema exporter for `anki-workflow-mcp` v2 contracts.
//!
//! Emits canonical UTF-8 JSON Schema Draft 2020-12 files with LF (`\n`) line endings
//! into `contracts/v2/schemas/` (or a custom directory passed via `--out-dir <path>`).
//!
//! Next to the schema directory (its parent, `contracts/v2/` for the default layout) the
//! same run also writes two byte-stable artefacts from the exact same bytes:
//!
//! - `manifest.json`: MCP tool → `args_schema` → `result_schema` index (CT-04);
//! - `version.lock`: `contract_version` + sha256 digest of the schema set (CT-05).
//!
//! Both are compared byte by byte against the committed copies by
//! `apps/anki-workflow-mcp/tests/contract_drift.rs`.

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
use sha2::{Digest, Sha256};

/// Major version of the contract directory this exporter writes (`contracts/v2`).
pub const CONTRACT_VERSION: &str = "2";

/// Command that reproduces every generated artefact in `contracts/v2`.
pub const PRODUCER: &str = "cargo run --locked -p anki-workflow-mcp --bin export-schemas";

/// Exact recipe of `schema_digest`, embedded in `version.lock` so the digest is reproducible.
pub const SCHEMA_DIGEST_INPUT: &str = "sha256 over every *.schema.json of this directory, ordered by \
     filename (code point), each fed as: filename + LF + file bytes + LF";

/// Policy carried inside the generated `version.lock` (JSON has no comments).
pub const CONTRACT_POLICY: [&str; 5] = [
    "contracts/v2 is generated, never hand-edited: run `just codegen-contracts` (export-schemas + \
     codegen-ts) and commit the diff together with the Rust struct change that caused it.",
    "contract_version is the major version of this directory; any breaking change to a schema, a \
     manifest entry, or a TypeScript type must bump it and land in a new contracts/vN directory.",
    "schema_digest is the sha256 of the whole schema set (see digest_input); any single byte of any \
     *.schema.json changes it, so it pins exactly what a consumer of contracts/v2 was built against.",
    "manifest.json and this lock are produced by the same run as the schemas; \
     apps/anki-workflow-mcp/tests/contract_drift.rs fails when the committed copies diverge from a \
     fresh run, including the digest.",
    "Consumers pin the digest rather than a hand-written version: a digest change without a \
     contract_version bump is only allowed for additive, non-breaking edits and must be reviewed as \
     such in the diff.",
];

/// Description of a generated file inside the contract directory.
pub struct GeneratedFile {
    /// File name relative to the v2 root (`version.lock`, `manifest.json`).
    pub filename: &'static str,
    pub content: String,
}

pub struct SchemaEntry {
    pub filename: &'static str,
    pub build: fn() -> Value,
}

/// One MCP tool exposed by this server, mapped to the generated schema files that describe it.
pub struct ToolContract {
    pub name: &'static str,
    /// `*.schema.json` holding the JSON Schema of the tool arguments.
    pub args_schema: &'static str,
    /// `*.schema.json` holding the JSON Schema of the tool result, when the result is typed.
    pub result_schema: Option<&'static str>,
}

/// The seven MCP tools served by `anki-workflow-mcp`, in the same shape the server advertises
/// them (`apps/anki-workflow-mcp/src/mcp.rs::init_tools`). `result_schema` is `None` for tools
/// that answer with an untyped JSON value (`generation_contract_get`) or with a free-form object
/// (`cards_compile_drafts`, `review_batch_information`, `quality_prepare`, `quality_evaluate`,
/// `batch_patch_draft`), which is what `output_schema_present` reports.
pub fn tool_registry() -> Vec<ToolContract> {
    vec![
        ToolContract {
            name: "batch_patch_draft",
            args_schema: "mcp_batch_patch_draft_args.schema.json",
            result_schema: None,
        },
        ToolContract {
            name: "cards_compile_drafts",
            args_schema: "mcp_compile_drafts_args.schema.json",
            result_schema: None,
        },
        ToolContract {
            name: "export_reviewed_files",
            args_schema: "mcp_export_reviewed_files_args.schema.json",
            result_schema: Some("mcp_export_reviewed_files_result.schema.json"),
        },
        ToolContract {
            name: "generation_contract_get",
            args_schema: "mcp_empty_args.schema.json",
            result_schema: None,
        },
        ToolContract {
            name: "quality_evaluate",
            args_schema: "mcp_quality_evaluate_args.schema.json",
            result_schema: None,
        },
        ToolContract {
            name: "quality_prepare",
            args_schema: "mcp_quality_prepare_args.schema.json",
            result_schema: None,
        },
        ToolContract {
            name: "review_batch_information",
            args_schema: "mcp_review_batch_information_args.schema.json",
            result_schema: None,
        },
    ]
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

pub fn default_v2_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("contracts")
        .join("v2")
}

pub fn default_out_dir() -> PathBuf {
    default_v2_dir().join("schemas")
}

/// Contract root that owns `manifest.json` / `version.lock`: the parent of the schema
/// directory (`--out-dir <root>/schemas` ⇒ artefacts in `<root>`). Keeping the artefacts out of
/// the schema directory means `*.schema.json` globs stay schemas-only.
pub fn contract_root_for(schemas_dir: &Path) -> PathBuf {
    match schemas_dir.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => schemas_dir.to_path_buf(),
    }
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

/// Renders every entry into memory, keyed by filename, so the digest, the manifest and the files
/// written to disk are guaranteed to describe the exact same bytes.
pub fn render_schemas(
    mut entries: Vec<SchemaEntry>,
) -> Result<BTreeMap<&'static str, String>, String> {
    entries.sort_by_key(|entry| entry.filename);

    let mut rendered: BTreeMap<&'static str, String> = BTreeMap::new();
    for entry in &entries {
        if rendered.contains_key(entry.filename) {
            return Err(format!("duplicate schema filename: {}", entry.filename));
        }
        let schema = (entry.build)();
        let content = serialize_lf(&schema)
            .map_err(|err| format!("failed to serialize {}: {err}", entry.filename))?;
        rendered.insert(entry.filename, content);
    }
    Ok(rendered)
}

fn write_rendered(out_dir: &Path, rendered: &BTreeMap<&'static str, String>) -> Result<(), String> {
    fs::create_dir_all(out_dir).map_err(|err| {
        format!(
            "failed to create output directory {}: {err}",
            out_dir.display()
        )
    })?;

    for (filename, content) in rendered {
        let target = out_dir.join(filename);
        fs::write(&target, content.as_bytes())
            .map_err(|err| format!("failed to write {}: {err}", target.display()))?;
    }
    Ok(())
}

/// Removes stale `*.schema.json` files so renamed/removed contracts cannot linger.
fn remove_stale_schemas(
    out_dir: &Path,
    rendered: &BTreeMap<&'static str, String>,
) -> Result<(), String> {
    let dir_entries = fs::read_dir(out_dir)
        .map_err(|err| format!("failed to read {}: {err}", out_dir.display()))?;
    for item in dir_entries {
        let item = item.map_err(|err| format!("failed to inspect directory entry: {err}"))?;
        let path = item.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name.ends_with(".schema.json")
            && !rendered.contains_key(name)
        {
            fs::remove_file(&path).map_err(|err| {
                format!("failed to remove stale schema {}: {err}", path.display())
            })?;
        }
    }
    Ok(())
}

pub fn export_entries(out_dir: &Path, entries: Vec<SchemaEntry>) -> Result<usize, String> {
    let rendered = render_schemas(entries)?;
    write_rendered(out_dir, &rendered)?;
    remove_stale_schemas(out_dir, &rendered)?;
    Ok(rendered.len())
}

pub fn export_all(out_dir: &Path) -> Result<usize, String> {
    export_entries(out_dir, schema_registry())
}

/// sha256 over the rendered schema set, ordered by filename (CT-05).
pub fn schema_digest(rendered: &BTreeMap<&'static str, String>) -> String {
    let mut hasher = Sha256::new();
    for (filename, content) in rendered {
        hasher.update(filename.as_bytes());
        hasher.update(b"\n");
        hasher.update(content.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

/// `manifest.json` (CT-04): MCP tool → args schema file → result schema file.
pub fn build_manifest(entries: &[SchemaEntry]) -> Result<Value, String> {
    let known: BTreeSet<&str> = entries.iter().map(|entry| entry.filename).collect();

    let mut tools = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut registry = tool_registry();
    registry.sort_by_key(|tool| tool.name);

    for tool in &registry {
        if !seen.insert(tool.name) {
            return Err(format!("duplicate MCP tool name: {}", tool.name));
        }
        if !known.contains(tool.args_schema) {
            return Err(format!(
                "tool '{}' references unknown args schema '{}'",
                tool.name, tool.args_schema
            ));
        }
        if let Some(result_schema) = tool.result_schema
            && !known.contains(result_schema)
        {
            return Err(format!(
                "tool '{}' references unknown result schema '{}'",
                tool.name, result_schema
            ));
        }
        tools.push(serde_json::json!({
            "name": tool.name,
            "args_schema": tool.args_schema,
            "result_schema": tool.result_schema,
            "output_schema_present": tool.result_schema.is_some(),
        }));
    }

    Ok(canonicalize_value(serde_json::json!({
        "contract_version": CONTRACT_VERSION,
        "generated_by": PRODUCER,
        "tool_count": tools.len(),
        "tools": tools,
    })))
}

/// `version.lock` (CT-05): `contract_version` + digest of the schema set + the policy that keeps
/// the directory honest.
pub fn build_version_lock(rendered: &BTreeMap<&'static str, String>) -> Value {
    canonicalize_value(serde_json::json!({
        "contract_version": CONTRACT_VERSION,
        "digest_algorithm": "sha256",
        "digest_input": SCHEMA_DIGEST_INPUT,
        "policy": CONTRACT_POLICY,
        "producer": PRODUCER,
        "schema_count": rendered.len(),
        "schema_digest": schema_digest(rendered),
    }))
}

/// Everything a single exporter run wrote, for reporting and tests.
pub struct ExportSummary {
    pub schema_count: usize,
    pub schema_digest: String,
    pub files: Vec<GeneratedFile>,
}

/// Writes the schemas plus the two derived artefacts of the contract root.
pub fn export_bundle(schemas_dir: &Path) -> Result<ExportSummary, String> {
    let entries = schema_registry();
    // Build the manifest first: it validates that every referenced schema file exists, so a
    // refactor that renames a schema fails before anything is written.
    let manifest = build_manifest(&entries)?;
    let rendered = render_schemas(entries)?;

    write_rendered(schemas_dir, &rendered)?;
    remove_stale_schemas(schemas_dir, &rendered)?;

    let digest = schema_digest(&rendered);
    let version_lock = build_version_lock(&rendered);
    let contract_root = contract_root_for(schemas_dir);
    fs::create_dir_all(&contract_root).map_err(|err| {
        format!(
            "failed to create contract root {}: {err}",
            contract_root.display()
        )
    })?;

    let mut files = Vec::new();
    for (filename, value) in [
        ("manifest.json", &manifest),
        ("version.lock", &version_lock),
    ] {
        let content =
            serialize_lf(value).map_err(|err| format!("failed to serialize {filename}: {err}"))?;
        let target = contract_root.join(filename);
        fs::write(&target, content.as_bytes())
            .map_err(|err| format!("failed to write {}: {err}", target.display()))?;
        files.push(GeneratedFile { filename, content });
    }

    Ok(ExportSummary {
        schema_count: rendered.len(),
        schema_digest: digest,
        files,
    })
}

fn main() -> ExitCode {
    let out_dir = match parse_out_dir() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("export-schemas error: {err}");
            return ExitCode::from(2);
        }
    };

    match export_bundle(&out_dir) {
        Ok(summary) => {
            println!(
                "exported {} deterministic JSON schemas to {} (sha256 {})",
                summary.schema_count,
                out_dir.display(),
                summary.schema_digest
            );
            let artifacts: Vec<&str> = summary.files.iter().map(|file| file.filename).collect();
            println!(
                "wrote {} to {}",
                artifacts.join(" + "),
                contract_root_for(&out_dir).display()
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

    fn contracts_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("contracts")
    }

    fn load_json(path: &Path) -> Value {
        let text = fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        serde_json::from_str(&text)
            .unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()))
    }

    fn object_keys(value: Option<&Value>) -> BTreeSet<String> {
        value
            .and_then(Value::as_object)
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn required_names(schema: &Value) -> BTreeSet<String> {
        schema
            .get("required")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn test_tool_registry_covers_the_mcp_surface_and_known_schemas() {
        let registry = tool_registry();
        assert_eq!(
            registry.len(),
            7,
            "anki-workflow-mcp serves exactly 7 MCP tools; update this list with mcp.rs"
        );

        let known: BTreeSet<&str> = schema_registry().iter().map(|e| e.filename).collect();
        let mut names = BTreeSet::new();
        for tool in &registry {
            assert!(names.insert(tool.name), "duplicate tool name {}", tool.name);
            assert!(
                known.contains(tool.args_schema),
                "tool '{}' must reuse an existing SchemaEntry, got '{}'",
                tool.name,
                tool.args_schema
            );
            if let Some(result_schema) = tool.result_schema {
                assert!(
                    known.contains(result_schema),
                    "tool '{}' must reuse an existing SchemaEntry, got '{}'",
                    tool.name,
                    result_schema
                );
            }
        }

        // `output_schema_present` is only true where the result has a typed schema.
        let with_output: Vec<&str> = registry
            .iter()
            .filter(|tool| tool.result_schema.is_some())
            .map(|tool| tool.name)
            .collect();
        assert_eq!(with_output, vec!["export_reviewed_files"]);
    }

    #[test]
    fn test_manifest_is_byte_stable_and_sorted() {
        let entries = schema_registry();
        let first = build_manifest(&entries).expect("manifest builds");
        let second = build_manifest(&schema_registry()).expect("manifest builds twice");
        assert_eq!(first, second, "manifest must be deterministic");

        let text = serialize_lf(&first).expect("manifest serializes");
        assert_eq!(
            text,
            serialize_lf(&second).expect("manifest serializes"),
            "manifest bytes must be stable"
        );
        assert!(!text.contains("\r\n"), "manifest must be LF-only");
        assert!(text.ends_with('\n'), "manifest must end with LF");

        let tools = first["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 7);
        assert_eq!(first["tool_count"], json!(7));
        assert_eq!(first["contract_version"], json!(CONTRACT_VERSION));

        let names: Vec<&str> = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "manifest tools must be sorted by name");

        let by_name: BTreeMap<&str, &Value> = tools
            .iter()
            .map(|tool| (tool["name"].as_str().expect("tool name"), tool))
            .collect();
        let export = by_name["export_reviewed_files"];
        assert_eq!(
            export["args_schema"],
            json!("mcp_export_reviewed_files_args.schema.json")
        );
        assert_eq!(
            export["result_schema"],
            json!("mcp_export_reviewed_files_result.schema.json")
        );
        assert_eq!(export["output_schema_present"], json!(true));

        for tool in ["cards_compile_drafts", "generation_contract_get"] {
            assert_eq!(by_name[tool]["output_schema_present"], json!(false));
            assert_eq!(by_name[tool]["result_schema"], Value::Null);
        }
        assert_eq!(
            by_name["generation_contract_get"]["args_schema"],
            json!("mcp_empty_args.schema.json")
        );
    }

    #[test]
    fn test_manifest_rejects_unknown_schema_files() {
        let mut entries = schema_registry();
        let removed = entries
            .iter()
            .position(|entry| entry.filename == "mcp_empty_args.schema.json")
            .expect("mcp_empty_args entry present");
        entries.remove(removed);

        let err = build_manifest(&entries).expect_err("manifest must reject a missing args schema");
        assert!(
            err.contains("generation_contract_get") && err.contains("mcp_empty_args.schema.json"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_schema_digest_is_stable_and_sensitive_to_one_byte() {
        let rendered = render_schemas(schema_registry()).expect("schemas render");
        let digest = schema_digest(&rendered);
        assert_eq!(digest.len(), 64, "sha256 hex digest");
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(
            digest,
            schema_digest(&render_schemas(schema_registry()).unwrap())
        );

        let mut tampered = rendered.clone();
        let filename = *tampered.keys().next().expect("at least one schema");
        tampered.insert(filename, format!("{}\n", tampered[filename]));
        assert_ne!(
            digest,
            schema_digest(&tampered),
            "one extra byte must change the digest"
        );
    }

    #[test]
    fn test_version_lock_documents_policy_and_digest() {
        let rendered = render_schemas(schema_registry()).expect("schemas render");
        let lock = build_version_lock(&rendered);

        assert_eq!(lock["contract_version"], json!(CONTRACT_VERSION));
        assert_eq!(lock["digest_algorithm"], json!("sha256"));
        assert_eq!(lock["schema_digest"], json!(schema_digest(&rendered)));
        assert_eq!(lock["schema_count"], json!(rendered.len()));
        assert_eq!(CONTRACT_POLICY.len(), 5);
        let policy = lock["policy"].as_array().expect("policy array");
        assert_eq!(policy.len(), 5);
        assert!(
            policy
                .iter()
                .all(|line| line.as_str().is_some_and(|l| l.len() > 40)),
            "policy lines must be self-contained"
        );

        let text = serialize_lf(&lock).expect("lock serializes");
        assert!(!text.contains("\r\n"));
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn test_export_bundle_writes_artifacts_beside_the_schema_dir() {
        let temp_dir = tempfile::tempdir().expect("tempdir creates");
        let schemas_dir = temp_dir.path().join("v2").join("schemas");

        let first = export_bundle(&schemas_dir).expect("bundle exports");
        assert_eq!(first.files.len(), 2);

        let manifest_path = temp_dir.path().join("v2").join("manifest.json");
        let lock_path = temp_dir.path().join("v2").join("version.lock");
        assert!(manifest_path.is_file(), "manifest.json must be written");
        assert!(lock_path.is_file(), "version.lock must be written");
        assert!(
            !schemas_dir.join("manifest.json").exists(),
            "artifacts must stay outside the schema directory"
        );

        let manifest_bytes = fs::read(&manifest_path).expect("manifest readable");
        let lock_bytes = fs::read(&lock_path).expect("lock readable");

        let second = export_bundle(&schemas_dir).expect("bundle exports twice");
        assert_eq!(first.schema_digest, second.schema_digest);
        assert_eq!(
            manifest_bytes,
            fs::read(&manifest_path).expect("manifest readable"),
            "manifest must be byte-identical across runs"
        );
        assert_eq!(
            lock_bytes,
            fs::read(&lock_path).expect("lock readable"),
            "version.lock must be byte-identical across runs"
        );

        let lock: Value = serde_json::from_slice(&lock_bytes).expect("lock parses");
        assert_eq!(lock["schema_digest"], json!(first.schema_digest));

        // Every schema file in the temp tree must be byte-identical to the committed one.
        let committed = default_out_dir();
        for (filename, content) in render_schemas(schema_registry()).expect("schemas render") {
            let committed_bytes = fs::read(committed.join(filename))
                .unwrap_or_else(|err| panic!("failed to read {filename}: {err}"));
            assert_eq!(content.as_bytes(), committed_bytes.as_slice());
        }
    }

    /// B11: `contracts/legacy/tools.json` must state who serves each legacy tool name, and the
    /// two names that the Rust server took over from the Python server (with incompatible
    /// arguments) must be documented in the entry itself.
    #[test]
    fn test_legacy_tools_declare_served_by_and_document_incompatible_collisions() {
        let legacy = load_json(&contracts_dir().join("legacy").join("tools.json"));
        let tools = legacy["tools"].as_array().expect("legacy tools array");
        assert_eq!(legacy["count"], json!(tools.len()));

        let registry = tool_registry();
        let rust_names: BTreeSet<&str> = registry.iter().map(|tool| tool.name).collect();

        let mut collisions: Vec<String> = Vec::new();
        for tool in tools {
            let name = tool["name"].as_str().expect("legacy tool name");
            let served_by = tool["served_by"].as_str().unwrap_or_else(|| {
                panic!("legacy tool '{name}' is missing `served_by` (expected \"python-legacy\" or \"rust\")")
            });
            assert!(
                matches!(served_by, "python-legacy" | "rust"),
                "legacy tool '{name}' has invalid `served_by`: {served_by}"
            );

            let expected = if rust_names.contains(name) {
                "rust"
            } else {
                "python-legacy"
            };
            assert_eq!(
                served_by, expected,
                "`served_by` of '{name}' must be derived from the real implementations"
            );

            let Some(rust_tool) = registry.iter().find(|entry| entry.name == name) else {
                assert!(
                    tool.get("collision").is_none(),
                    "'{name}' is served by python-legacy only; a `collision` block makes no sense"
                );
                continue;
            };

            // Same name on both implementations: the argument contracts must either match, or
            // the divergence must be documented in the entry.
            collisions.push(name.to_owned());
            let rust_schema = load_json(&default_out_dir().join(rust_tool.args_schema));
            let legacy_parameters = &tool["parameters"];
            let compatible = required_names(legacy_parameters) == required_names(&rust_schema)
                && object_keys(legacy_parameters.get("properties"))
                    == object_keys(rust_schema.get("properties"));

            match tool.get("collision") {
                Some(documentation) if compatible => {
                    assert_eq!(
                        documentation["params_compatible"],
                        json!(true),
                        "stale `collision` block: '{name}' now has identical parameters on both servers"
                    );
                }
                Some(documentation) => {
                    assert_eq!(documentation["also_served_by"], json!("python-legacy"));
                    assert_eq!(
                        documentation["params_compatible"],
                        json!(false),
                        "'{name}' documents a collision but claims compatible parameters"
                    );
                    let reason = documentation["reason"].as_str().unwrap_or_default();
                    assert!(
                        reason.len() >= 40,
                        "'{name}' must explain the divergence in `collision.reason`, got: {reason:?}"
                    );
                }
                None if compatible => {}
                None => panic!(
                    "legacy tool '{name}' is also served by Rust with incompatible parameters \
                     (legacy requires {:?}, Rust requires {:?}); add a `collision` block \
                     documenting it or rename one of the implementations",
                    required_names(legacy_parameters),
                    required_names(&rust_schema)
                ),
            }
        }

        collisions.sort();
        assert_eq!(
            collisions,
            vec![
                "batch_patch_draft".to_owned(),
                "cards_compile_drafts".to_owned()
            ],
            "the set of names served by both implementations changed; review the collisions"
        );
    }
}
