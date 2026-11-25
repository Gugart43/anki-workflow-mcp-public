use std::future::Future;
use std::sync::{Arc, LazyLock};

use anki_domain::{EvidenceScope, ReviewVerdict};
use anki_validate::draft_normalize::StrictRawCardDraft;
use anki_validate::quality::{
    QualityBinding, QualityDimensions, QualityEvidence, QualityInput, QualityMode,
    QualityObligation, QualityPrepareInput, QualityReview, QualitySource, evaluate_quality,
    prepare_quality_input,
};
use anki_validate::{CompileMode, compile_drafts_with_mode, patch_draft, review_batch_information};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ErrorCode, Implementation, JsonObject,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub use crate::export::ExportReviewedFilesArgs;
use crate::export::{ExportError, ExportReviewedFilesResult, execute_export_reviewed_files};

const SERVER_INSTRUCTIONS: &str = "\
MCP stdio validation, quality evaluation, and transactional export server for Anki QA/Cloze batches (7 tools). \
Call `generation_contract_get` first with `{}` to read the full `quality_v1` workflow, \
per-retrieval rubric, field ordering, repair protocol, and limitations. \
Use `cards_compile_drafts` to compile normalized drafts into canonical QA/Cloze CSVs, \
`review_batch_information` for per-card information profiles (`no_count_only_limit: true`, no fixed 20-card ceiling), \
`quality_prepare` to build a snapshot hash and per-retrieval inventory, \
`quality_evaluate` to evaluate bindings and independent editorial reviews against that snapshot, \
`batch_patch_draft` for pure in-memory `{draft, updates} -> {card}` patching \
(distinct from the path-based Python staging file API), \
and `export_reviewed_files` for local transactional CSV export with cross-process locking and idempotent state-ledger persistence \
(requires exact `PASS` verdict and verified SHA-256 `receipt_hash`). \
Technical validation or `PASS` is never proof of clinical truth; \
`INCOMPLETE` never authorizes export.";

#[derive(Debug, Clone, Default)]
pub struct AnkiQualityMcpServer;

impl AnkiQualityMcpServer {
    pub fn new() -> Self {
        Self
    }
}

pub async fn run_stdio() -> Result<(), String> {
    let server = AnkiQualityMcpServer::new();
    let running = match server.serve(rmcp::transport::stdio()).await {
        Ok(running) => running,
        Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => return Ok(()),
        Err(err) => {
            return Err(format!(
                "MCP stdio initialization failed: {}",
                sanitize_error_text(&err.to_string())
            ));
        }
    };

    running.waiting().await.map_err(|err| {
        format!(
            "MCP stdio task failed: {}",
            sanitize_error_text(&err.to_string())
        )
    })?;

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct InputViolation {
    pub code: &'static str,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ToolErrorPayload {
    /// Single-line, sanitized human-readable error message.
    pub error: String,
    /// Stable machine-readable error code: `INVALID_ARGUMENTS` (argument parse/decode),
    /// `COMPILE.INVALID_DRAFT` (draft decode/normalize/compile), `INVALID_INPUT_RANGES`
    /// (structured range violations), `INTERNAL.SERIALIZE` (result serialization),
    /// `EXPORT.*` (export domain errors).
    pub code: &'static str,
    /// Structured field-level violations, always present (empty when there are none).
    pub violations: Vec<InputViolation>,
    /// JSON path of the offending argument element, omitted when not applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyArgs {}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompileDraftsArgs {
    /// List of card draft objects (QA or Cloze) to normalize and compile.
    #[schemars(with = "Vec<StrictRawCardDraft>")]
    pub drafts: Vec<Value>,
    /// Rewrite Cloze deletions into sequential `c1..cN` order before compiling (default `true`).
    /// Set to `false` only when the draft already carries the final indices.
    #[serde(default = "default_true")]
    pub auto_renumber_cloze: bool,
    /// Strip leaked internal identifiers (deck/note/board ids) from rendered text (default `true`).
    /// Set to `false` only when the identifiers are intentional content.
    #[serde(default = "default_true")]
    pub auto_purge_id_leaks: bool,
    /// Fallback value for the `Campo de Estudo` column, applied to drafts that omit
    /// `campo_estudo` / `study_field` (default empty string).
    #[serde(default)]
    pub default_campo_estudo: String,
    #[serde(default)]
    pub compile_mode: CompileMode,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewBatchInformationArgs {
    pub qa_csv: String,
    pub cloze_csv: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchPatchDraftArgs {
    pub draft: Map<String, Value>,
    pub updates: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub enum StrictNoteTypeArg {
    QA,
    #[serde(alias = "CLOZE")]
    CL,
}

impl StrictNoteTypeArg {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::QA => "QA",
            Self::CL => "CL",
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualitySourceArg {
    pub id: String,
    pub title: String,
    pub version: String,
    pub locator: String,
    pub text: String,
}

impl From<QualitySourceArg> for QualitySource {
    fn from(arg: QualitySourceArg) -> Self {
        Self {
            id: arg.id,
            title: arg.title,
            version: arg.version,
            locator: arg.locator,
            text: arg.text,
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityObligationArg {
    pub id: String,
    pub description: String,
    pub source_ids: Vec<String>,
}

impl From<QualityObligationArg> for QualityObligation {
    fn from(arg: QualityObligationArg) -> Self {
        Self {
            id: arg.id,
            description: arg.description,
            source_ids: arg.source_ids,
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityEvidenceArg {
    pub source_id: String,
    /// Excerpt copied verbatim from the referenced `sources[].text`; it must be an exact
    /// substring of that source text (no paraphrasing, normalization, or ellipsis).
    pub quote: String,
    pub locator: String,
    pub scope: EvidenceScope,
}

impl From<QualityEvidenceArg> for QualityEvidence {
    fn from(arg: QualityEvidenceArg) -> Self {
        Self {
            source_id: arg.source_id,
            quote: arg.quote,
            locator: arg.locator,
            scope: arg.scope.as_str().to_owned(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityBindingArg {
    pub note_type: StrictNoteTypeArg,
    /// 1-based CSV data row of the retrieval, counting the header row; use `>= 2`
    /// (row 1 is the header). Rows beyond the compiled batch cannot bind any card.
    #[schemars(range(min = 2))]
    pub source_row: i64,
    /// 1-based Cloze deletion index (`cN`) inside the sentence; required with
    /// `>= 1` for `note_type: CL` and must be absent/null for `note_type: QA`.
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub cloze_index: Option<i64>,
    pub obligation_ids: Vec<String>,
    pub evidence: Vec<QualityEvidenceArg>,
}

impl QualityBindingArg {
    pub fn into_binding(self) -> QualityBinding {
        QualityBinding {
            note_type: self.note_type.as_str().to_owned(),
            source_row: self.source_row as usize,
            cloze_index: self.cloze_index.map(|idx| idx as usize),
            obligation_ids: self.obligation_ids,
            evidence: self.evidence.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewDimensionsArg {
    pub source_fidelity: bool,
    pub atomic_retrieval: bool,
    pub context_sufficient: bool,
    pub no_answer_leak: bool,
    pub clinically_relevant: bool,
    pub notes_appropriate: bool,
    pub media_appropriate: bool,
}

impl From<ReviewDimensionsArg> for QualityDimensions {
    fn from(arg: ReviewDimensionsArg) -> Self {
        Self {
            source_fidelity: arg.source_fidelity,
            atomic_retrieval: arg.atomic_retrieval,
            context_sufficient: arg.context_sufficient,
            no_answer_leak: arg.no_answer_leak,
            clinically_relevant: arg.clinically_relevant,
            notes_appropriate: arg.notes_appropriate,
            media_appropriate: arg.media_appropriate,
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityReviewArg {
    pub note_type: StrictNoteTypeArg,
    /// 1-based CSV data row of the reviewed retrieval, counting the header row; use `>= 2`
    /// (row 1 is the header).
    #[schemars(range(min = 2))]
    pub source_row: i64,
    /// 1-based Cloze deletion index (`cN`) inside the sentence; required with
    /// `>= 1` for `note_type: CL` and must be absent/null for `note_type: QA`.
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub cloze_index: Option<i64>,
    /// `snapshot_hash` returned by `quality_prepare` for the exact batch being reviewed;
    /// a stale or foreign hash is reported as `INCOMPLETE` and never authorizes export.
    pub snapshot_hash: String,
    pub reviewer: String,
    pub verdict: ReviewVerdict,
    pub rationale: String,
    pub dimensions: ReviewDimensionsArg,
}

impl QualityReviewArg {
    pub fn into_review(self) -> QualityReview {
        QualityReview {
            note_type: self.note_type.as_str().to_owned(),
            source_row: self.source_row as usize,
            cloze_index: self.cloze_index.map(|idx| idx as usize),
            snapshot_hash: self.snapshot_hash,
            reviewer: self.reviewer,
            verdict: self.verdict.as_str().to_owned(),
            rationale: self.rationale,
            dimensions: self.dimensions.into(),
        }
    }
}

fn validate_retrieval_locator(
    note_type: StrictNoteTypeArg,
    source_row: i64,
    cloze_index: Option<i64>,
    path_prefix: &str,
    violations: &mut Vec<InputViolation>,
) {
    if source_row < 2 {
        violations.push(InputViolation {
            code: "INVALID_SOURCE_ROW",
            path: format!("{path_prefix}.source_row"),
            message: format!("{path_prefix}.source_row must be an integer >= 2, got {source_row}"),
        });
    }
    match (note_type, cloze_index) {
        (StrictNoteTypeArg::QA, Some(cloze_idx)) => {
            violations.push(InputViolation {
                code: "INVALID_CLOZE_INDEX",
                path: format!("{path_prefix}.cloze_index"),
                message: format!(
                    "{path_prefix}.cloze_index must be null for QA note_type, got {cloze_idx}"
                ),
            });
        }
        (StrictNoteTypeArg::CL, None) => {
            violations.push(InputViolation {
                code: "MISSING_CLOZE_INDEX",
                path: format!("{path_prefix}.cloze_index"),
                message: format!("{path_prefix}.cloze_index must be provided for CL note_type"),
            });
        }
        (StrictNoteTypeArg::CL, Some(cloze_idx)) if cloze_idx < 1 => {
            violations.push(InputViolation {
                code: "INVALID_CLOZE_INDEX",
                path: format!("{path_prefix}.cloze_index"),
                message: format!(
                    "{path_prefix}.cloze_index must be an integer >= 1, got {cloze_idx}"
                ),
            });
        }
        _ => {}
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityPrepareArgs {
    #[serde(default)]
    pub mode: QualityMode,
    pub qa_csv: String,
    pub cloze_csv: String,
    #[serde(default)]
    pub sources: Vec<QualitySourceArg>,
    #[serde(default)]
    pub obligations: Vec<QualityObligationArg>,
    #[serde(default)]
    pub bindings: Vec<QualityBindingArg>,
}

impl QualityPrepareArgs {
    fn collect_range_violations(&self, violations: &mut Vec<InputViolation>) {
        for (idx, binding) in self.bindings.iter().enumerate() {
            validate_retrieval_locator(
                binding.note_type,
                binding.source_row,
                binding.cloze_index,
                &format!("bindings[{idx}]"),
                violations,
            );
        }
    }

    fn into_prepare_input(self) -> QualityPrepareInput {
        QualityPrepareInput {
            qa_csv: self.qa_csv,
            cloze_csv: self.cloze_csv,
            mode: self.mode,
            sources: self.sources.into_iter().map(Into::into).collect(),
            obligations: self.obligations.into_iter().map(Into::into).collect(),
            bindings: self
                .bindings
                .into_iter()
                .map(QualityBindingArg::into_binding)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityEvaluateArgs {
    #[serde(default)]
    pub mode: QualityMode,
    pub qa_csv: String,
    pub cloze_csv: String,
    #[serde(default)]
    pub sources: Vec<QualitySourceArg>,
    #[serde(default)]
    pub obligations: Vec<QualityObligationArg>,
    #[serde(default)]
    pub bindings: Vec<QualityBindingArg>,
    #[serde(default)]
    pub reviews: Vec<QualityReviewArg>,
}

impl QualityEvaluateArgs {
    fn collect_range_violations(&self, violations: &mut Vec<InputViolation>) {
        for (idx, binding) in self.bindings.iter().enumerate() {
            validate_retrieval_locator(
                binding.note_type,
                binding.source_row,
                binding.cloze_index,
                &format!("bindings[{idx}]"),
                violations,
            );
        }

        for (idx, review) in self.reviews.iter().enumerate() {
            validate_retrieval_locator(
                review.note_type,
                review.source_row,
                review.cloze_index,
                &format!("reviews[{idx}]"),
                violations,
            );
        }
    }

    fn into_evaluate_input(self) -> QualityInput {
        QualityInput {
            qa_csv: self.qa_csv,
            cloze_csv: self.cloze_csv,
            mode: self.mode,
            sources: self.sources.into_iter().map(Into::into).collect(),
            obligations: self.obligations.into_iter().map(Into::into).collect(),
            bindings: self
                .bindings
                .into_iter()
                .map(QualityBindingArg::into_binding)
                .collect(),
            reviews: self
                .reviews
                .into_iter()
                .map(QualityReviewArg::into_review)
                .collect(),
        }
    }
}

fn sanitize_error_text(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c == '\r' || c == '\n' || c == '\t' {
                ' '
            } else {
                c
            }
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn tool_error_payload(payload: ToolErrorPayload) -> CallToolResponse {
    match serde_json::to_value(payload) {
        Ok(val) => CallToolResult::structured_error(val).into(),
        Err(err) => CallToolResult::structured_error(json!({
            "error": sanitize_error_text(&format!("Failed to serialize tool error payload: {err}")),
            "code": "INTERNAL.SERIALIZE",
            "violations": []
        }))
        .into(),
    }
}

fn tool_error_with(
    code: &'static str,
    message: impl AsRef<str>,
    path: Option<String>,
    violations: Vec<InputViolation>,
) -> CallToolResponse {
    tool_error_payload(ToolErrorPayload {
        error: sanitize_error_text(message.as_ref()),
        code,
        violations,
        path,
    })
}

/// Argument parse/decode failure, carrying the JSON path of the offending element.
struct ArgsError {
    message: String,
    path: Option<String>,
}

/// `INVALID_ARGUMENTS`: the request arguments could not be parsed/decoded.
fn tool_invalid_arguments(err: &ArgsError) -> CallToolResponse {
    tool_error_with(
        "INVALID_ARGUMENTS",
        &err.message,
        err.path.clone(),
        Vec::new(),
    )
}

/// `COMPILE.INVALID_DRAFT`: a draft could not be decoded, normalized, or compiled.
/// The domain message already carries the `drafts[i]:` prefix.
fn tool_invalid_draft(message: impl AsRef<str>) -> CallToolResponse {
    tool_error_with("COMPILE.INVALID_DRAFT", message, None, Vec::new())
}

/// `INTERNAL.SERIALIZE`: the tool result could not be serialized.
fn tool_serialize_failure(context: &str, err: &serde_json::Error) -> CallToolResponse {
    tool_error_with(
        "INTERNAL.SERIALIZE",
        format!("Failed to serialize {context}: {err}"),
        None,
        Vec::new(),
    )
}

fn tool_ok(value: Value) -> CallToolResponse {
    CallToolResult::structured(value).into()
}

fn parse_args<T: for<'de> Deserialize<'de>>(arguments: Option<JsonObject>) -> Result<T, ArgsError> {
    let map = arguments.unwrap_or_default();
    let value = Value::Object(map);
    match serde_path_to_error::deserialize::<_, T>(&value) {
        Ok(parsed) => Ok(parsed),
        Err(err) => {
            let path = normalize_arg_path(&err.path().to_string());
            let detail = sanitize_error_text(&err.into_inner().to_string());
            let message = match path.as_deref() {
                Some(path) => format!("Invalid tool arguments at `{path}`: {detail}"),
                None => format!("Invalid tool arguments: {detail}"),
            };
            Err(ArgsError { message, path })
        }
    }
}

/// Normaliza o `serde_path_to_error` path para o payload de erro.
///
/// `serde_path_to_error` devolve `"."` quando o erro é na raiz do objeto (e `""` quando não há
/// caminho). Ambos são ruído: o esquema do `ToolErrorPayload` só admite `path` ausente ou um
/// caminho não vazio, então raiz vira `None` em vez de `"path": "."`.
fn normalize_arg_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "." {
        return None;
    }
    Some(trimmed.to_owned())
}

fn readonly_annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(false)
}

fn schema_for_args<T: JsonSchema>() -> Arc<JsonObject> {
    let settings = SchemaSettings::draft2020_12().with(|s| {
        s.inline_subschemas = true;
    });
    match settings
        .into_generator()
        .into_root_schema_for::<T>()
        .to_value()
    {
        Value::Object(map) => Arc::new(map),
        _ => unreachable!("tool schema must be a JSON object"),
    }
}

static TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(init_tools);

#[cfg(test)]
fn build_tools() -> &'static [Tool] {
    TOOLS.as_slice()
}

fn init_tools() -> Vec<Tool> {
    let ann = readonly_annotations();

    let cards_compile_drafts = Tool::new(
        "cards_compile_drafts",
        "Normalize and compile structured QA/Cloze draft objects in memory into canonical pipe-delimited `qa_csv` and `cloze_csv` with structural diagnostics and statistics. Arguments: `drafts` (required, one object per card) plus the optional knobs `auto_renumber_cloze` (default `true`, rewrites Cloze deletions into sequential `c1..cN`), `auto_purge_id_leaks` (default `true`, strips leaked internal identifiers from rendered text), `default_campo_estudo` (fallback `Campo de Estudo` for drafts without one, default `\"\"`) and `compile_mode`, which selects the `notas_adicionais` compiler: `legacy_compat` (default) accepts the legacy fields; `strict_v2` accepts only the canonical `NoteDocument` (`blocos`/`fontes`) and rejects populated legacy fields and fails with code `COMPILE.INVALID_DRAFT`.",
        schema_for_args::<CompileDraftsArgs>(),
    )
    .with_annotations(ann.clone());

    let review_batch_information = Tool::new(
        "review_batch_information",
        "Inspect compiled `qa_csv` and `cloze_csv` in memory and return per-card information profiles, token/character counts, and card totals without a fixed 20-card ceiling.",
        schema_for_args::<ReviewBatchInformationArgs>(),
    )
    .with_annotations(ann.clone());

    let quality_prepare = Tool::new(
        "quality_prepare",
        "Prepare a deterministic quality snapshot (`snapshot_hash`) and per-retrieval inventory from compiled CSVs and optional quality bindings/sources/obligations. `mode` (default `strict`): `quick` runs lightweight structural/HTML checks only, `strict` enforces full source grounding, obligation coverage, and independent editorial review. Each `bindings[i]` needs `source_row` (1-based data row, counting the header, so `>= 2`), `cloze_index` (`>= 1`, required for `note_type: CL`, absent for `QA`), `obligation_ids` and `evidence[]`, where `evidence[i].quote` must be a verbatim substring of the referenced `sources[i].text`.",
        schema_for_args::<QualityPrepareArgs>(),
    )
    .with_annotations(ann.clone());

    let quality_evaluate = Tool::new(
        "quality_evaluate",
        "Evaluate source fidelity, obligation coverage, evidence bindings, and independent editorial reviews per QA row and Cloze index against the batch snapshot. `mode` (default `strict`): `quick` runs lightweight structural/HTML checks only, `strict` enforces full source grounding, obligation coverage, and independent editorial review. Each `bindings[i]` uses `source_row` (1-based data row counting the header, so `>= 2`) plus `cloze_index` (`>= 1`, required for `note_type: CL`, absent for `QA`); each `reviews[i]` must echo the exact `snapshot_hash` from `quality_prepare` for the same batch, otherwise the result is `INCOMPLETE`.",
        schema_for_args::<QualityEvaluateArgs>(),
    )
    .with_annotations(ann.clone());

    let batch_patch_draft = Tool::new(
        "batch_patch_draft",
        "Pure in-memory draft patcher: accepts `{draft, updates}` JSON objects and returns `{card}` with updated fields normalized. NOTE: This is NOT the path-based Python staging file API (`staging_path`/`card_index`/`patch_json`); it performs no disk reads or writes.",
        schema_for_args::<BatchPatchDraftArgs>(),
    )
    .with_annotations(ann.clone());

    let generation_contract_get = Tool::new(
        "generation_contract_get",
        "Return the normative `quality_v1` generation, review, media, field-ordering, limitation, and repair contract. Accepts `{}` only.",
        schema_for_args::<EmptyArgs>(),
    )
    .with_annotations(ann);

    let export_reviewed_files = Tool::new(
        "export_reviewed_files",
        "Atomically export reviewed QA/Cloze CSV files to deck export directory after validating receipt hash, verdict PASS, CSV format, and HTML safety. Idempotent on identical payload; records exported state. Arguments: `deck_dir` (destination deck directory; files land in its `exports/` subdirectory, and `..`/symlinks/empty paths are rejected), `batch_id` (stable id used for the idempotency ledger; no path separators or reserved names), `qa_csv` and `cloze_csv` (exact compiled content to persist), `receipt_hash` (SHA-256 of that exact CSV pair), `evaluation_verdict` (must be exactly `PASS`; `INCOMPLETE`/`REVISE`/`FAIL` are rejected), `topic_slug` (optional filename stem, defaults to the sanitized `batch_id`) and `write_to_deck_root` (default `true`, also mirrors the written files into the deck root).",
        schema_for_args::<ExportReviewedFilesArgs>(),
    )
    .with_annotations(
        ToolAnnotations::new()
            .read_only(false)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    );

    vec![
        cards_compile_drafts,
        review_batch_information,
        quality_prepare,
        quality_evaluate,
        batch_patch_draft,
        generation_contract_get,
        export_reviewed_files,
    ]
}

fn generation_contract_payload() -> Value {
    json!({
        "schema_version": "quality_v1",
        "no_count_only_limit": true,
        "phases": [
            {
                "phase": 1,
                "name": "source_ingestion",
                "summary": "Ingest authoritative sources and record exact verbatim excerpts with stable source metadata, version, and precise locator (`id`, `title`, `version`, `locator`, `text`). Never invent or extrapolate unsupported medical claims."
            },
            {
                "phase": 2,
                "name": "obligation_inventory",
                "summary": "Inventory decision and diagnostic obligations (`id`, `description`, `source_ids`) before drafting cards. Focus coverage on essential clinical/decision obligations without flattening into shallow generalities."
            },
            {
                "phase": 3,
                "name": "card_drafting",
                "summary": "Draft concise QA and Cloze cards where each retrieval target tests one decision/fact with enough necessary front context to make the answer unique. Avoid answer leaks, synthetic splits by character length, or a fixed 20-card batch limit. Verify all facts including optional `Notas Adicionais`; compare alternatives only when pedagogically useful; never auto-enrich unsupported medical details. Render images with clinically descriptive `<figure>`/`<figcaption>`, avoid duplicate styling, and preserve AnkiMBR native containers (expandable Notes, Images, and References, watermark, and design tokens)."
            },
            {
                "phase": 4,
                "name": "compile_and_snapshot",
                "summary": "Compile drafts with `cards_compile_drafts`, inspect information profiles with `review_batch_information`, and generate the deterministic snapshot (`snapshot_hash`) and retrieval inventory with `quality_prepare`."
            },
            {
                "phase": 5,
                "name": "independent_editorial_review",
                "summary": "Perform an independent editorial review for every QA row and every individual Cloze `cN` index bound to the current `snapshot_hash`, attaching explicit evidence quotes (`retrieval`, `context`, `notes`) and all 7 boolean rubric dimensions."
            },
            {
                "phase": 6,
                "name": "quality_evaluation",
                "summary": "Run `quality_evaluate` with sources, obligations, bindings, and independent reviews. A technical pass is not clinical truth and does not grant export permission."
            },
            {
                "phase": 7,
                "name": "targeted_repair",
                "summary": "Patch failing cards only via `batch_patch_draft` (in-memory `{draft, updates}`), recompile, prepare a fresh snapshot, and re-review the affected retrievals against the new `snapshot_hash`."
            }
        ],
        "per_retrieval_rubric": {
            "unit_of_review": "Each QA row (`note_type: QA`, `source_row >= 2`) and each distinct Cloze index (`note_type: CL`, `source_row >= 2`, `cloze_index >= 1`).",
            "layers": {
                "retrieval": "Tests one clear, clinically meaningful decision or recall target; never bundle unrelated recalls or split artificially purely by text length.",
                "context": "Front/stem supplies sufficient discriminating context so only the intended answer fits without leaking the answer via synonyms, binary elimination, or morphological/root cues.",
                "notes": "Optional `Notas Adicionais` are verified against cited sources (empty or brief notes are acceptable); alternatives are compared only when useful for discrimination; no unsupported medical enrichment."
            },
            "dimensions": [
                {
                    "name": "source_fidelity",
                    "type": "boolean",
                    "requirement": "All claims in retrieval, context, and optional notes are directly supported by ingested source excerpts without fabrication."
                },
                {
                    "name": "atomic_retrieval",
                    "type": "boolean",
                    "requirement": "Targets one retrieval per QA card or per Cloze index without overloaded multi-part recall."
                },
                {
                    "name": "context_sufficient",
                    "type": "boolean",
                    "requirement": "Provides enough necessary context on the prompt side for an unambiguous answer."
                },
                {
                    "name": "no_answer_leak",
                    "type": "boolean",
                    "requirement": "Prompt, context, hints, and image captions/alt text do not leak the tested answer."
                },
                {
                    "name": "clinically_relevant",
                    "type": "boolean",
                    "requirement": "Addresses an inventoried decision or clinical obligation rather than trivia."
                },
                {
                    "name": "notes_appropriate",
                    "type": "boolean",
                    "requirement": "Optional notes are accurate, verified, non-bloated, and compare alternatives only when useful."
                },
                {
                    "name": "media_appropriate",
                    "type": "boolean",
                    "requirement": "Figures use clinically descriptive `<figure>`/`<figcaption>`, avoid duplicate styling, and preserve AnkiMBR native containers."
                }
            ],
            "evidence_scopes": ["retrieval", "context", "notes"],
            "verdicts": ["approve", "revise"]
        },
        "field_ordering": {
            "delimiter": "|",
            "qa": [
                "Enunciado",
                "Resposta",
                "Campo de Estudo",
                "Notas Adicionais",
                "Imagens Relacionadas",
                "Referências",
                "Etiquetas"
            ],
            "cloze": [
                "Sentença",
                "Notas Adicionais",
                "Campo de Estudo",
                "Imagens Relacionadas",
                "Referências",
                "Etiquetas"
            ]
        },
        "limitations": [
            "Local stdio binary exposing 7 tools: 6 pure in-memory validation/evaluation/contract tools and `export_reviewed_files` for local transactional CSV persistence (`exports/` + `.anki-workflow-export-state.json` ledger with cross-process locking); no SQLite/manifesto database, no network calls, and no `.apkg` packaging.",
            "`batch_patch_draft` operates purely in memory (`{draft, updates} -> {card}`) and is NOT the path-based Python staging file API.",
            "Technical `PASS` from compilation, structural checks, or information review is NOT proof of clinical truth or source fidelity; `export_reviewed_files` still requires exact `PASS` verdict, valid SHA-256 `receipt_hash`, and fail-closed CSV/HTML revalidation.",
            "`INCOMPLETE` means required inputs, bindings, or independent reviews are missing or stale and NEVER authorizes export.",
            "No invented medical examples or private deck data are embedded in this contract."
        ],
        "repair_protocol": {
            "steps": [
                "Identify failing QA rows or Cloze indices from `quality_evaluate` diagnostics or `revise` verdicts.",
                "Patch only failing drafts in memory using `batch_patch_draft` (`{draft, updates}`); preserve passing drafts unchanged.",
                "Recompile the full batch via `cards_compile_drafts` so CSV ordering and Cloze numbering remain consistent.",
                "Call `quality_prepare` to compute the new `snapshot_hash`.",
                "Update bindings and perform fresh independent editorial reviews bound to the new `snapshot_hash`, then re-run `quality_evaluate`."
            ]
        }
    })
}

impl ServerHandler for AnkiQualityMcpServer {
    fn get_info(&self) -> ServerConfig {
        let info = crate::build_info::BuildInfo::current();
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                info.implementation.into_owned(),
                info.package_version.into_owned(),
            ))
            .with_instructions(SERVER_INSTRUCTIONS)
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(TOOLS.clone())))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        TOOLS.iter().find(|tool| tool.name == name).cloned()
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResponse, ErrorData>> + Send + '_ {
        dispatch_tool_call_async(request)
    }
}

fn map_export_result(result: Result<ExportReviewedFilesResult, ExportError>) -> CallToolResponse {
    match result {
        Ok(res) => match serde_json::to_value(&res) {
            Ok(val) => tool_ok(val),
            Err(err) => tool_serialize_failure("export_reviewed_files result", &err),
        },
        Err(err) => tool_error_payload(ToolErrorPayload {
            error: sanitize_error_text(&err.message),
            code: err.code,
            violations: Vec::new(),
            path: None,
        }),
    }
}

async fn dispatch_tool_call_async(
    request: CallToolRequestParams,
) -> Result<CallToolResponse, ErrorData> {
    if request.name.as_ref() == "export_reviewed_files" {
        let args: ExportReviewedFilesArgs = match parse_args(request.arguments) {
            Ok(parsed) => parsed,
            Err(err) => return Ok(tool_invalid_arguments(&err)),
        };
        let export_result =
            tokio::task::spawn_blocking(move || execute_export_reviewed_files(&args)).await;
        return Ok(match export_result {
            Ok(res) => map_export_result(res),
            Err(join_err) => tool_error_payload(ToolErrorPayload {
                error: sanitize_error_text(&format!("Export blocking task failed: {join_err}")),
                code: "EXPORT.TASK_JOIN_ERROR",
                violations: Vec::new(),
                path: None,
            }),
        });
    }
    dispatch_tool_call(request)
}

fn dispatch_tool_call(request: CallToolRequestParams) -> Result<CallToolResponse, ErrorData> {
    let tool_name = request.name.as_ref();

    match tool_name {
        "cards_compile_drafts" => {
            let args: CompileDraftsArgs = match parse_args(request.arguments) {
                Ok(parsed) => parsed,
                Err(err) => return Ok(tool_invalid_arguments(&err)),
            };
            match compile_drafts_with_mode(
                &args.drafts,
                args.auto_renumber_cloze,
                args.auto_purge_id_leaks,
                &args.default_campo_estudo,
                true,
                args.compile_mode,
            ) {
                Ok(val) => Ok(tool_ok(val)),
                Err(msg) => Ok(tool_invalid_draft(msg)),
            }
        }
        "review_batch_information" => {
            let args: ReviewBatchInformationArgs = match parse_args(request.arguments) {
                Ok(parsed) => parsed,
                Err(err) => return Ok(tool_invalid_arguments(&err)),
            };
            let review = review_batch_information(&args.qa_csv, &args.cloze_csv);
            match serde_json::to_value(&review) {
                Ok(val) => Ok(tool_ok(val)),
                Err(err) => Ok(tool_serialize_failure("batch information review", &err)),
            }
        }
        "quality_prepare" => {
            let args: QualityPrepareArgs = match parse_args(request.arguments) {
                Ok(parsed) => parsed,
                Err(err) => return Ok(tool_invalid_arguments(&err)),
            };
            let mut violations = Vec::new();
            args.collect_range_violations(&mut violations);
            if !violations.is_empty() {
                return Ok(tool_error_with(
                    "INVALID_INPUT_RANGES",
                    format!(
                        "Invalid tool arguments: {} range violation(s) encountered",
                        violations.len()
                    ),
                    None,
                    violations,
                ));
            }
            let input = args.into_prepare_input();
            let report = prepare_quality_input(&input);
            match serde_json::to_value(&report) {
                Ok(val) => Ok(tool_ok(val)),
                Err(err) => Ok(tool_serialize_failure("quality_prepare result", &err)),
            }
        }
        "quality_evaluate" => {
            let args: QualityEvaluateArgs = match parse_args(request.arguments) {
                Ok(parsed) => parsed,
                Err(err) => return Ok(tool_invalid_arguments(&err)),
            };
            let mut violations = Vec::new();
            args.collect_range_violations(&mut violations);
            if !violations.is_empty() {
                return Ok(tool_error_with(
                    "INVALID_INPUT_RANGES",
                    format!(
                        "Invalid tool arguments: {} range violation(s) encountered",
                        violations.len()
                    ),
                    None,
                    violations,
                ));
            }
            let input = args.into_evaluate_input();
            let report = evaluate_quality(&input);
            match serde_json::to_value(&report) {
                Ok(val) => Ok(tool_ok(val)),
                Err(err) => Ok(tool_serialize_failure("quality_evaluate result", &err)),
            }
        }
        "batch_patch_draft" => {
            let args: BatchPatchDraftArgs = match parse_args(request.arguments) {
                Ok(parsed) => parsed,
                Err(err) => return Ok(tool_invalid_arguments(&err)),
            };
            match patch_draft(&Value::Object(args.draft), &args.updates) {
                Ok(card) => Ok(tool_ok(json!({ "card": card }))),
                Err(msg) => Ok(tool_invalid_draft(msg)),
            }
        }
        "generation_contract_get" => {
            if let Err(err) = parse_args::<EmptyArgs>(request.arguments) {
                return Ok(tool_invalid_arguments(&err));
            }
            Ok(tool_ok(generation_contract_payload()))
        }
        "export_reviewed_files" => {
            let args: ExportReviewedFilesArgs = match parse_args(request.arguments) {
                Ok(parsed) => parsed,
                Err(err) => return Ok(tool_invalid_arguments(&err)),
            };
            Ok(map_export_result(execute_export_reviewed_files(&args)))
        }
        unknown => Err(ErrorData::new(
            ErrorCode::INVALID_PARAMS,
            format!("Unknown tool: {}", sanitize_error_text(unknown)),
            None,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn make_call(name: &'static str, args: Value) -> CallToolRequestParams {
        let map = match args {
            Value::Object(m) => Some(m),
            _ => panic!("test args must be object"),
        };
        let mut params = CallToolRequestParams::new(name);
        params.arguments = map;
        params
    }

    fn is_error_response(resp: &CallToolResponse) -> bool {
        match resp {
            CallToolResponse::Complete(res) => res.is_error == Some(true),
            _ => false,
        }
    }

    fn extract_error_payload(resp: &CallToolResponse) -> Value {
        match resp {
            CallToolResponse::Complete(res) => res
                .structured_content
                .clone()
                .expect("structured error payload"),
            _ => panic!("expected complete response"),
        }
    }

    fn assert_typed_schemas_strict(schema: &Value, path: &str) {
        let Some(obj) = schema.as_object() else {
            return;
        };
        if obj.contains_key("properties")
            && !path.ends_with(".properties.draft")
            && !path.ends_with(".properties.updates")
        {
            assert_eq!(
                obj.get("additionalProperties"),
                Some(&Value::Bool(false)),
                "typed object at {path} missing additionalProperties: false"
            );
        }
        if let Some(props) = obj.get("properties").and_then(Value::as_object) {
            for (k, v) in props {
                assert_typed_schemas_strict(v, &format!("{path}.properties.{k}"));
            }
        }
        if let Some(items) = obj.get("items") {
            assert_typed_schemas_strict(items, &format!("{path}.items"));
        }
        for combinator in ["anyOf", "oneOf", "allOf"] {
            if let Some(arr) = obj.get(combinator).and_then(Value::as_array) {
                for (idx, sub) in arr.iter().enumerate() {
                    assert_typed_schemas_strict(sub, &format!("{path}.{combinator}[{idx}]"));
                }
            }
        }
        for defs_key in ["$defs", "definitions"] {
            if let Some(defs) = obj.get(defs_key).and_then(Value::as_object) {
                for (k, v) in defs {
                    assert_typed_schemas_strict(v, &format!("{path}.{defs_key}.{k}"));
                }
            }
        }
    }

    #[test]
    fn exposes_seven_tools_with_strict_schemas_and_annotations() {
        let tools = build_tools();
        assert_eq!(tools.len(), 7);
        let names: BTreeSet<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        let expected: BTreeSet<&str> = [
            "cards_compile_drafts",
            "review_batch_information",
            "quality_prepare",
            "quality_evaluate",
            "batch_patch_draft",
            "generation_contract_get",
            "export_reviewed_files",
        ]
        .into_iter()
        .collect();
        assert_eq!(names, expected);

        for tool in tools {
            let schema_val = tool.schema_as_json_value();
            assert_eq!(schema_val.get("additionalProperties"), Some(&json!(false)));
            assert_typed_schemas_strict(&schema_val, tool.name.as_ref());
            let ann = tool.annotations.as_ref().expect("annotations required");
            if tool.name == "export_reviewed_files" {
                assert_eq!(ann.read_only_hint, Some(false));
            } else {
                assert_eq!(ann.read_only_hint, Some(true));
            }
            assert_eq!(ann.destructive_hint, Some(false));
            assert_eq!(ann.idempotent_hint, Some(true));
            assert_eq!(ann.open_world_hint, Some(false));
        }
    }

    #[test]
    fn generation_contract_rejects_unknown_args_and_returns_quality_v1() {
        let bad = dispatch_tool_call(make_call("generation_contract_get", json!({ "extra": 1 })))
            .expect("should return tool error result");
        assert!(is_error_response(&bad));

        let ok = dispatch_tool_call(make_call("generation_contract_get", json!({})))
            .expect("contract should succeed");
        assert!(!is_error_response(&ok));
        if let CallToolResponse::Complete(res) = ok {
            let payload = res.structured_content.expect("structured content");
            assert_eq!(payload["schema_version"], json!("quality_v1"));
            assert_eq!(payload["no_count_only_limit"], json!(true));
        }
    }

    #[test]
    fn cards_compile_drafts_rejects_unknown_tipo() {
        let resp = dispatch_tool_call(make_call(
            "cards_compile_drafts",
            json!({
                "drafts": [{
                    "tipo": "CLOZ",
                    "enunciado_ou_sentenca": "Teste de cloze malformado"
                }]
            }),
        ))
        .expect("should return tool response");
        assert!(is_error_response(&resp));
        let payload = extract_error_payload(&resp);
        assert_eq!(payload["code"], json!("COMPILE.INVALID_DRAFT"));
        assert!(
            payload["error"]
                .as_str()
                .unwrap_or_default()
                .contains("CLOZ"),
            "error should mention unsupported tipo CLOZ"
        );
        assert!(
            payload["error"]
                .as_str()
                .unwrap_or_default()
                .contains("drafts[0]:"),
            "compile errors must preserve the drafts[i]: prefix"
        );
    }

    #[test]
    fn quality_prepare_rejects_reviews_and_unknown_fields() {
        let unknown_top = dispatch_tool_call(make_call(
            "quality_prepare",
            json!({ "qa_csv": "", "cloze_csv": "", "bogus": 1 }),
        ))
        .expect("returns isError result");
        assert!(is_error_response(&unknown_top));

        let with_reviews = dispatch_tool_call(make_call(
            "quality_prepare",
            json!({
                "qa_csv": "",
                "cloze_csv": "",
                "reviews": []
            }),
        ))
        .expect("quality_prepare must reject reviews");
        assert!(is_error_response(&with_reviews));
    }

    #[test]
    fn quality_evaluate_accumulates_all_row_and_cloze_range_violations() {
        let bad_eval = dispatch_tool_call(make_call(
            "quality_evaluate",
            json!({
                "qa_csv": "",
                "cloze_csv": "",
                "bindings": [
                    {
                        "note_type": "QA",
                        "source_row": 1,
                        "obligation_ids": [],
                        "evidence": []
                    },
                    {
                        "note_type": "CL",
                        "source_row": 3,
                        "cloze_index": 0,
                        "obligation_ids": [],
                        "evidence": []
                    }
                ],
                "reviews": [
                    {
                        "note_type": "QA",
                        "source_row": 0,
                        "snapshot_hash": "h",
                        "reviewer": "r",
                        "verdict": "approve",
                        "rationale": "ok",
                        "dimensions": {
                            "source_fidelity": true,
                            "atomic_retrieval": true,
                            "context_sufficient": true,
                            "no_answer_leak": true,
                            "clinically_relevant": true,
                            "notes_appropriate": true,
                            "media_appropriate": true
                        }
                    },
                    {
                        "note_type": "CL",
                        "source_row": 4,
                        "cloze_index": 0,
                        "snapshot_hash": "h",
                        "reviewer": "r",
                        "verdict": "revise",
                        "rationale": "ok",
                        "dimensions": {
                            "source_fidelity": true,
                            "atomic_retrieval": true,
                            "context_sufficient": true,
                            "no_answer_leak": true,
                            "clinically_relevant": true,
                            "notes_appropriate": true,
                            "media_appropriate": true
                        }
                    }
                ]
            }),
        ))
        .expect("returns isError result");
        assert!(is_error_response(&bad_eval));

        let payload = extract_error_payload(&bad_eval);
        assert_eq!(payload["code"], json!("INVALID_INPUT_RANGES"));
        let violations = payload["violations"].as_array().expect("violations array");
        assert_eq!(
            violations.len(),
            4,
            "all 4 range violations must be accumulated"
        );

        assert_eq!(violations[0]["path"], json!("bindings[0].source_row"));
        assert_eq!(violations[0]["code"], json!("INVALID_SOURCE_ROW"));

        assert_eq!(violations[1]["path"], json!("bindings[1].cloze_index"));
        assert_eq!(violations[1]["code"], json!("INVALID_CLOZE_INDEX"));

        assert_eq!(violations[2]["path"], json!("reviews[0].source_row"));
        assert_eq!(violations[2]["code"], json!("INVALID_SOURCE_ROW"));

        assert_eq!(violations[3]["path"], json!("reviews[1].cloze_index"));
        assert_eq!(violations[3]["code"], json!("INVALID_CLOZE_INDEX"));
    }

    #[test]
    fn unknown_tool_returns_invalid_params_error() {
        let err = dispatch_tool_call(make_call("does_not_exist", json!({})))
            .expect_err("unknown tool must return protocol error");
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn cards_compile_drafts_rejects_unknown_draft_fields_and_accepts_aliases() {
        let bad = dispatch_tool_call(make_call(
            "cards_compile_drafts",
            json!({
                "drafts": [{
                    "tipo": "QA",
                    "enunciado_ou_sentenca": "Pergunta?",
                    "resposta": "Resposta.",
                    "campo_desconhecido_invalido": "valor"
                }]
            }),
        ))
        .expect("should return tool response");
        assert!(is_error_response(&bad));
        let payload = extract_error_payload(&bad);
        assert_eq!(
            payload["code"],
            json!("COMPILE.INVALID_DRAFT"),
            "draft decode/normalize errors must not be reported as INVALID_ARGUMENTS"
        );
        assert!(
            payload["error"]
                .as_str()
                .unwrap_or_default()
                .contains("campo_desconhecido_invalido")
        );

        let ok = dispatch_tool_call(make_call(
            "cards_compile_drafts",
            json!({
                "drafts": [{
                    "note_type": "QA",
                    "pergunta": "Pergunta via alias?",
                    "resposta": "Resposta via alias.",
                    "study_field": "Neurologia",
                    "fonte": "Diretriz 2026",
                    "tags": ["neuro"]
                }]
            }),
        ))
        .expect("alias draft should succeed");
        assert!(!is_error_response(&ok));
    }

    #[test]
    fn strict_note_type_accepts_cloze_alias_and_validates_locator_combinations() {
        let bad_locators = dispatch_tool_call(make_call(
            "quality_prepare",
            json!({
                "qa_csv": "",
                "cloze_csv": "",
                "bindings": [
                    {
                        "note_type": "QA",
                        "source_row": 2,
                        "cloze_index": 1,
                        "obligation_ids": [],
                        "evidence": []
                    },
                    {
                        "note_type": "CLOZE",
                        "source_row": 2,
                        "obligation_ids": [],
                        "evidence": []
                    }
                ]
            }),
        ))
        .expect("should return range/locator violations");
        assert!(is_error_response(&bad_locators));
        let payload = extract_error_payload(&bad_locators);
        assert_eq!(payload["code"], json!("INVALID_INPUT_RANGES"));
        let vios = payload["violations"].as_array().expect("violations array");
        assert_eq!(vios.len(), 2);
        assert_eq!(vios[0]["code"], json!("INVALID_CLOZE_INDEX"));
        assert_eq!(vios[1]["code"], json!("MISSING_CLOZE_INDEX"));
    }

    #[test]
    fn server_instructions_and_contract_reflect_seven_tools_and_local_export() {
        assert!(SERVER_INSTRUCTIONS.contains("7 tools"));
        assert!(SERVER_INSTRUCTIONS.contains("export_reviewed_files"));
        assert!(SERVER_INSTRUCTIONS.contains("local transactional CSV export"));
        assert!(!SERVER_INSTRUCTIONS.contains("Stateless MCP stdio"));

        let server = AnkiQualityMcpServer::new();
        let info = server.get_info();
        let info_json = serde_json::to_value(&info).expect("serialize server info");
        assert_eq!(info_json["instructions"], json!(SERVER_INSTRUCTIONS));

        let contract = generation_contract_payload();
        let limitations = contract["limitations"]
            .as_array()
            .expect("limitations array");
        assert!(limitations.iter().any(|l| {
            l.as_str()
                .unwrap_or_default()
                .contains("Local stdio binary exposing 7 tools")
        }));
        assert!(limitations.iter().any(|l| {
            l.as_str()
                .unwrap_or_default()
                .contains("export_reviewed_files")
        }));
        assert!(!limitations.iter().any(|l| {
            l.as_str()
                .unwrap_or_default()
                .contains("Pure stateless stdio binary")
        }));
        assert!(limitations.iter().any(|l| {
            l.as_str()
                .unwrap_or_default()
                .contains("fail-closed CSV/HTML revalidation")
        }));
    }

    #[test]
    fn nested_enum_and_type_errors_expose_the_failing_json_path() {
        let dimensions = json!({
            "source_fidelity": true,
            "atomic_retrieval": true,
            "context_sufficient": true,
            "no_answer_leak": true,
            "clinically_relevant": true,
            "notes_appropriate": true,
            "media_appropriate": true
        });

        // (a) enum inválido em `reviews[0].verdict`
        let bad_verdict = dispatch_tool_call(make_call(
            "quality_evaluate",
            json!({
                "qa_csv": "",
                "cloze_csv": "",
                "reviews": [{
                    "note_type": "QA",
                    "source_row": 2,
                    "snapshot_hash": "h",
                    "reviewer": "r",
                    "verdict": "maybe",
                    "rationale": "ok",
                    "dimensions": dimensions.clone()
                }]
            }),
        ))
        .expect("should return tool response");
        assert!(is_error_response(&bad_verdict));
        let payload = extract_error_payload(&bad_verdict);
        assert_eq!(payload["code"], json!("INVALID_ARGUMENTS"));
        assert_eq!(
            payload["path"],
            json!("reviews[0].verdict"),
            "path must identify the offending element: {payload}"
        );
        let err = payload["error"].as_str().unwrap_or_default();
        assert!(
            err.contains("at `reviews[0].verdict`"),
            "error must name the offending path: {err}"
        );
        assert!(
            err.contains("maybe"),
            "error must quote the bad value: {err}"
        );

        // (b) enum inválido no segundo elemento de `bindings`
        let bad_note_type = dispatch_tool_call(make_call(
            "quality_prepare",
            json!({
                "qa_csv": "",
                "cloze_csv": "",
                "bindings": [
                    {
                        "note_type": "QA",
                        "source_row": 2,
                        "obligation_ids": [],
                        "evidence": []
                    },
                    {
                        "note_type": "QS",
                        "source_row": 2,
                        "obligation_ids": [],
                        "evidence": []
                    }
                ]
            }),
        ))
        .expect("should return tool response");
        assert!(is_error_response(&bad_note_type));
        let payload = extract_error_payload(&bad_note_type);
        assert_eq!(payload["code"], json!("INVALID_ARGUMENTS"));
        assert_eq!(payload["path"], json!("bindings[1].note_type"));

        // (c) tipo errado em campo aninhado de `sources`
        let bad_source_text = dispatch_tool_call(make_call(
            "quality_prepare",
            json!({
                "qa_csv": "",
                "cloze_csv": "",
                "sources": [{
                    "id": "s1",
                    "title": "T",
                    "version": "v1",
                    "locator": "p1",
                    "text": 123
                }]
            }),
        ))
        .expect("should return tool response");
        assert!(is_error_response(&bad_source_text));
        let payload = extract_error_payload(&bad_source_text);
        assert_eq!(payload["code"], json!("INVALID_ARGUMENTS"));
        assert_eq!(payload["path"], json!("sources[0].text"));

        // (d) campo desconhecido no próprio elemento aninhado
        let unknown_nested_field = dispatch_tool_call(make_call(
            "quality_prepare",
            json!({
                "qa_csv": "",
                "cloze_csv": "",
                "sources": [{
                    "id": "s1",
                    "title": "T",
                    "version": "v1",
                    "locator": "p1",
                    "text": "excerpt",
                    "extra_nested": 123
                }]
            }),
        ))
        .expect("should return tool response");
        assert!(is_error_response(&unknown_nested_field));
        let payload = extract_error_payload(&unknown_nested_field);
        assert_eq!(payload["code"], json!("INVALID_ARGUMENTS"));
        assert_eq!(payload["path"], json!("sources[0].extra_nested"));
        assert!(
            payload["error"]
                .as_str()
                .unwrap_or_default()
                .contains("extra_nested")
        );
    }

    #[test]
    fn tool_error_payload_always_emits_violations_and_omits_absent_path() {
        let plain = serde_json::to_value(ToolErrorPayload {
            error: "boom".to_owned(),
            code: "INVALID_ARGUMENTS",
            violations: Vec::new(),
            path: None,
        })
        .expect("serialize error payload");
        assert_eq!(
            plain["violations"],
            json!([]),
            "violations must always be emitted (schema requires it): {plain}"
        );
        assert_eq!(plain.as_object().map(serde_json::Map::len), Some(3));
        assert!(plain.get("path").is_none(), "absent path must be omitted");

        let located = serde_json::to_value(ToolErrorPayload {
            error: "boom".to_owned(),
            code: "INVALID_ARGUMENTS",
            violations: Vec::new(),
            path: Some("drafts[0].tipo".to_owned()),
        })
        .expect("serialize error payload");
        assert_eq!(located["violations"], json!([]));
        assert_eq!(located["path"], json!("drafts[0].tipo"));
    }

    #[test]
    fn normalize_arg_path_drops_the_root_markers() {
        // `serde_path_to_error` devolve "." para erros na raiz do objeto e "" quando não há
        // caminho; ambos são ruído no payload.
        assert_eq!(normalize_arg_path(""), None);
        assert_eq!(normalize_arg_path("."), None);
        assert_eq!(normalize_arg_path("  "), None);
        assert_eq!(
            normalize_arg_path("drafts[0].tipo"),
            Some("drafts[0].tipo".to_owned())
        );
        assert_eq!(normalize_arg_path("sources"), Some("sources".to_owned()));
    }

    #[test]
    fn root_level_argument_error_omits_the_dotted_path() {
        // `qa_csv`/`cloze_csv` são obrigatórios: faltando, o erro é na RAIZ do objeto e antes
        // saía como `"path": "."`.
        let resp = dispatch_tool_call(make_call("quality_prepare", json!({})))
            .expect("should return tool response");
        assert!(is_error_response(&resp));
        let payload = extract_error_payload(&resp);
        assert_eq!(payload["code"], json!("INVALID_ARGUMENTS"));
        assert!(
            payload.get("path").is_none(),
            "root-level argument errors must omit path: {payload}"
        );
        let err = payload["error"].as_str().unwrap_or_default();
        assert!(
            !err.contains("at `.`"),
            "root-level argument errors must not name `.` as a path: {err}"
        );
        assert!(
            err.contains("Invalid tool arguments:"),
            "message must stay the path-less form: {err}"
        );
    }

    #[test]
    fn tool_error_payload_schema_requires_violations_and_keeps_path_optional() {
        let schema = Value::Object((*schema_for_args::<ToolErrorPayload>()).clone());
        let required: Vec<&str> = schema["required"]
            .as_array()
            .expect("required array")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            required.contains(&"violations"),
            "violations must stay required in the contract: {schema}"
        );
        assert!(
            !required.contains(&"path"),
            "path must stay optional in the contract: {schema}"
        );
        assert!(
            schema["properties"]["path"].is_object(),
            "path must be declared: {schema}"
        );
    }

    #[test]
    fn serialize_failures_use_internal_serialize_code() {
        let err = serde_json::from_str::<Value>("{").expect_err("invalid JSON fixture");
        let resp = tool_serialize_failure("quality_prepare result", &err);
        assert!(is_error_response(&resp));
        let payload = extract_error_payload(&resp);
        assert_eq!(payload["code"], json!("INTERNAL.SERIALIZE"));
        assert_eq!(payload["violations"], json!([]));
        assert!(
            payload["error"]
                .as_str()
                .unwrap_or_default()
                .contains("Failed to serialize quality_prepare result")
        );
    }

    /// A `description` sibling next to `$ref` makes the TS codegen emit a duplicated
    /// type (`CompileMode1`/`QualityMode1`), so documented `$ref` properties are
    /// intentionally documented in the tool description instead. `$defs` subtrees are
    /// skipped: there the description belongs to the definition itself.
    fn assert_no_description_beside_ref(schema: &Value, path: &str) {
        match schema {
            Value::Object(obj) => {
                if obj.contains_key("$ref") {
                    assert!(
                        obj.get("description").is_none(),
                        "description beside $ref at {path} would duplicate the TS type"
                    );
                    return;
                }
                for (key, value) in obj {
                    if key == "$defs" || key == "definitions" {
                        continue;
                    }
                    assert_no_description_beside_ref(value, &format!("{path}.{key}"));
                }
            }
            Value::Array(items) => {
                for (idx, item) in items.iter().enumerate() {
                    assert_no_description_beside_ref(item, &format!("{path}[{idx}]"));
                }
            }
            _ => {}
        }
    }

    #[test]
    fn tool_argument_schemas_never_attach_description_to_a_ref() {
        for tool in build_tools() {
            let schema_val = tool.schema_as_json_value();
            assert_no_description_beside_ref(&schema_val, tool.name.as_ref());
        }
    }

    /// Builds the schema exactly like `export-schemas` does (draft 2020-12, defs kept as
    /// `$ref`, no inlining), so the assertion covers what lands in `contracts/v2`.
    fn exported_schema<T: JsonSchema>() -> Value {
        SchemaSettings::draft2020_12()
            .into_generator()
            .into_root_schema_for::<T>()
            .to_value()
    }

    #[test]
    fn exported_contract_schemas_never_attach_description_to_a_ref() {
        assert_no_description_beside_ref(
            &exported_schema::<CompileDraftsArgs>(),
            "CompileDraftsArgs",
        );
        assert_no_description_beside_ref(
            &exported_schema::<QualityPrepareArgs>(),
            "QualityPrepareArgs",
        );
        assert_no_description_beside_ref(
            &exported_schema::<QualityEvaluateArgs>(),
            "QualityEvaluateArgs",
        );
        assert_no_description_beside_ref(
            &exported_schema::<ToolErrorPayload>(),
            "ToolErrorPayload",
        );

        // `compile_mode`/`mode` are `$ref` properties: they must stay undocumented there,
        // otherwise the TS codegen emits `CompileMode1`/`QualityMode1`.
        for (label, schema, prop) in [
            (
                "CompileDraftsArgs",
                exported_schema::<CompileDraftsArgs>(),
                "compile_mode",
            ),
            (
                "QualityPrepareArgs",
                exported_schema::<QualityPrepareArgs>(),
                "mode",
            ),
            (
                "QualityEvaluateArgs",
                exported_schema::<QualityEvaluateArgs>(),
                "mode",
            ),
        ] {
            assert!(
                schema["properties"][prop].get("description").is_none(),
                "{label}.{prop} must not carry a description next to its $ref"
            );
            assert!(
                schema["properties"][prop].get("$ref").is_some(),
                "{label}.{prop} is expected to stay a $ref in the exported contract"
            );
        }
    }

    #[tokio::test]
    async fn export_reviewed_files_uses_spawn_blocking_and_sanitizes_error_messages() {
        let raw_err = ExportError::new(
            "EXPORT.VALIDATION_FAIL",
            "Error with\nembedded\r\nnewlines\tand    tabs",
        );
        let mapped = map_export_result(Err(raw_err));
        assert!(is_error_response(&mapped));
        let payload = extract_error_payload(&mapped);
        assert_eq!(payload["code"], json!("EXPORT.VALIDATION_FAIL"));
        assert_eq!(
            payload["error"],
            json!("Error with embedded newlines and tabs")
        );

        let call = make_call(
            "export_reviewed_files",
            json!({
                "deck_dir": "target/test_export_dir",
                "batch_id": "batch\nwith\r\nnewlines",
                "qa_csv": "qa",
                "cloze_csv": "cloze",
                "receipt_hash": "invalid_hash",
                "evaluation_verdict": "REVISE"
            }),
        );
        let async_resp = dispatch_tool_call_async(call)
            .await
            .expect("dispatch_tool_call_async should complete");
        assert!(is_error_response(&async_resp));
        let async_payload = extract_error_payload(&async_resp);
        assert_eq!(async_payload["code"], json!("EXPORT.INVALID_BATCH_ID"));
        let err_text = async_payload["error"].as_str().unwrap_or_default();
        assert!(!err_text.contains('\n'));
        assert!(!err_text.contains('\r'));
        assert!(!err_text.contains('\t'));
        assert!(err_text.contains("batch_id inválido 'batch with newlines'"));
    }
}
