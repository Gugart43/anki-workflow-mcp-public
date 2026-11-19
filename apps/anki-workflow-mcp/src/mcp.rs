use std::future::Future;
use std::sync::{Arc, LazyLock};

use anki_domain::{EvidenceScope, ReviewVerdict};
use anki_validate::draft_normalize::RawCardDraft;
use anki_validate::quality::{
    QualityBinding, QualityDimensions, QualityEvidence, QualityInput, QualityObligation,
    QualityPrepareInput, QualityReview, QualitySource, evaluate_quality, prepare_quality_input,
};
use anki_validate::{compile_drafts, patch_draft, review_batch_information};
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

const SERVER_INSTRUCTIONS: &str = "\
Stateless MCP stdio validation and quality evaluation server for Anki QA/Cloze batches. \
Call `generation_contract_get` first with `{}` to read the full `quality_v1` workflow, \
per-retrieval rubric, field ordering, repair protocol, and limitations. \
Use `cards_compile_drafts` to compile normalized drafts into canonical QA/Cloze CSVs, \
`review_batch_information` for per-card information profiles (`no_count_only_limit: true`, no fixed 20-card ceiling), \
`quality_prepare` to build a snapshot hash and per-retrieval inventory, \
`quality_evaluate` to evaluate bindings and independent editorial reviews against that snapshot, \
and `batch_patch_draft` for pure in-memory `{draft, updates} -> {card}` patching \
(distinct from the path-based Python staging file API). \
Technical validation or `PASS` is never proof of clinical truth or permission to export; \
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
    let running = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|err| {
            format!(
                "MCP stdio initialization failed: {}",
                sanitize_error_text(&err.to_string())
            )
        })?;

    running.waiting().await.map_err(|err| {
        format!(
            "MCP stdio task failed: {}",
            sanitize_error_text(&err.to_string())
        )
    })?;

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct InputViolation {
    code: &'static str,
    path: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct ToolErrorPayload {
    error: String,
    code: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    violations: Vec<InputViolation>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CompileDraftsArgs {
    /// List of card draft objects (QA or Cloze) to normalize and compile.
    #[schemars(with = "Vec<RawCardDraft>")]
    drafts: Vec<Value>,
    #[serde(default = "default_true")]
    auto_renumber_cloze: bool,
    #[serde(default = "default_true")]
    auto_purge_id_leaks: bool,
    #[serde(default)]
    default_campo_estudo: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReviewBatchInformationArgs {
    qa_csv: String,
    cloze_csv: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BatchPatchDraftArgs {
    draft: Map<String, Value>,
    updates: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
enum StrictNoteTypeArg {
    QA,
    CL,
}

impl StrictNoteTypeArg {
    const fn as_str(self) -> &'static str {
        match self {
            Self::QA => "QA",
            Self::CL => "CL",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QualitySourceArg {
    id: String,
    title: String,
    version: String,
    locator: String,
    text: String,
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QualityObligationArg {
    id: String,
    description: String,
    source_ids: Vec<String>,
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QualityEvidenceArg {
    source_id: String,
    quote: String,
    locator: String,
    scope: EvidenceScope,
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QualityBindingArg {
    note_type: StrictNoteTypeArg,
    source_row: i64,
    #[serde(default)]
    cloze_index: Option<i64>,
    obligation_ids: Vec<String>,
    evidence: Vec<QualityEvidenceArg>,
}

impl QualityBindingArg {
    fn into_binding(self) -> QualityBinding {
        QualityBinding {
            note_type: self.note_type.as_str().to_owned(),
            source_row: self.source_row as usize,
            cloze_index: self.cloze_index.map(|idx| idx as usize),
            obligation_ids: self.obligation_ids,
            evidence: self.evidence.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReviewDimensionsArg {
    source_fidelity: bool,
    atomic_retrieval: bool,
    context_sufficient: bool,
    no_answer_leak: bool,
    clinically_relevant: bool,
    notes_appropriate: bool,
    media_appropriate: bool,
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QualityReviewArg {
    note_type: StrictNoteTypeArg,
    source_row: i64,
    #[serde(default)]
    cloze_index: Option<i64>,
    snapshot_hash: String,
    reviewer: String,
    verdict: ReviewVerdict,
    rationale: String,
    dimensions: ReviewDimensionsArg,
}

impl QualityReviewArg {
    fn into_review(self) -> QualityReview {
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QualityPrepareArgs {
    qa_csv: String,
    cloze_csv: String,
    #[serde(default)]
    sources: Vec<QualitySourceArg>,
    #[serde(default)]
    obligations: Vec<QualityObligationArg>,
    #[serde(default)]
    bindings: Vec<QualityBindingArg>,
}

impl QualityPrepareArgs {
    fn collect_range_violations(&self, violations: &mut Vec<InputViolation>) {
        for (idx, binding) in self.bindings.iter().enumerate() {
            if binding.source_row < 2 {
                violations.push(InputViolation {
                    code: "INVALID_SOURCE_ROW",
                    path: format!("bindings[{idx}].source_row"),
                    message: format!(
                        "bindings[{idx}].source_row must be an integer >= 2, got {}",
                        binding.source_row
                    ),
                });
            }
            if let Some(cloze_idx) = binding.cloze_index
                && cloze_idx < 1
            {
                violations.push(InputViolation {
                    code: "INVALID_CLOZE_INDEX",
                    path: format!("bindings[{idx}].cloze_index"),
                    message: format!(
                        "bindings[{idx}].cloze_index must be null or an integer >= 1, got {cloze_idx}"
                    ),
                });
            }
        }
    }

    fn into_prepare_input(self) -> QualityPrepareInput {
        QualityPrepareInput {
            qa_csv: self.qa_csv,
            cloze_csv: self.cloze_csv,
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QualityEvaluateArgs {
    qa_csv: String,
    cloze_csv: String,
    #[serde(default)]
    sources: Vec<QualitySourceArg>,
    #[serde(default)]
    obligations: Vec<QualityObligationArg>,
    #[serde(default)]
    bindings: Vec<QualityBindingArg>,
    #[serde(default)]
    reviews: Vec<QualityReviewArg>,
}

impl QualityEvaluateArgs {
    fn collect_range_violations(&self, violations: &mut Vec<InputViolation>) {
        for (idx, binding) in self.bindings.iter().enumerate() {
            if binding.source_row < 2 {
                violations.push(InputViolation {
                    code: "INVALID_SOURCE_ROW",
                    path: format!("bindings[{idx}].source_row"),
                    message: format!(
                        "bindings[{idx}].source_row must be an integer >= 2, got {}",
                        binding.source_row
                    ),
                });
            }
            if let Some(cloze_idx) = binding.cloze_index
                && cloze_idx < 1
            {
                violations.push(InputViolation {
                    code: "INVALID_CLOZE_INDEX",
                    path: format!("bindings[{idx}].cloze_index"),
                    message: format!(
                        "bindings[{idx}].cloze_index must be null or an integer >= 1, got {cloze_idx}"
                    ),
                });
            }
        }

        for (idx, review) in self.reviews.iter().enumerate() {
            if review.source_row < 2 {
                violations.push(InputViolation {
                    code: "INVALID_SOURCE_ROW",
                    path: format!("reviews[{idx}].source_row"),
                    message: format!(
                        "reviews[{idx}].source_row must be an integer >= 2, got {}",
                        review.source_row
                    ),
                });
            }
            if let Some(cloze_idx) = review.cloze_index
                && cloze_idx < 1
            {
                violations.push(InputViolation {
                    code: "INVALID_CLOZE_INDEX",
                    path: format!("reviews[{idx}].cloze_index"),
                    message: format!(
                        "reviews[{idx}].cloze_index must be null or an integer >= 1, got {cloze_idx}"
                    ),
                });
            }
        }
    }

    fn into_evaluate_input(self) -> QualityInput {
        QualityInput {
            qa_csv: self.qa_csv,
            cloze_csv: self.cloze_csv,
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
            "error": err.to_string(),
            "code": "SERIALIZE_ERROR"
        }))
        .into(),
    }
}

fn tool_error(message: impl AsRef<str>) -> CallToolResponse {
    tool_error_payload(ToolErrorPayload {
        error: sanitize_error_text(message.as_ref()),
        code: "INVALID_ARGUMENTS",
        violations: Vec::new(),
    })
}

fn tool_ok(value: Value) -> CallToolResponse {
    CallToolResult::structured(value).into()
}

fn parse_args<T: for<'de> Deserialize<'de>>(arguments: Option<JsonObject>) -> Result<T, String> {
    let map = arguments.unwrap_or_default();
    serde_json::from_value(Value::Object(map)).map_err(|err| {
        format!(
            "Invalid tool arguments: {}",
            sanitize_error_text(&err.to_string())
        )
    })
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
        "Normalize and compile structured QA/Cloze draft objects in memory into canonical pipe-delimited `qa_csv` and `cloze_csv` with structural diagnostics and statistics.",
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
        "Prepare a deterministic quality snapshot (`snapshot_hash`) and per-retrieval inventory from compiled CSVs and optional quality bindings/sources/obligations.",
        schema_for_args::<QualityPrepareArgs>(),
    )
    .with_annotations(ann.clone());

    let quality_evaluate = Tool::new(
        "quality_evaluate",
        "Evaluate source fidelity, obligation coverage, evidence bindings, and independent editorial reviews per QA row and Cloze index against the batch snapshot.",
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

    vec![
        cards_compile_drafts,
        review_batch_information,
        quality_prepare,
        quality_evaluate,
        batch_patch_draft,
        generation_contract_get,
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
            "Pure stateless stdio binary: no disk persistence, no manifesto/SQLite state, no network calls, and no `.apkg`/file export.",
            "`batch_patch_draft` operates purely in memory (`{draft, updates} -> {card}`) and is NOT the path-based Python staging file API.",
            "Technical `PASS` from compilation, structural checks, or information review is NOT proof of clinical truth, source fidelity, or permission to export.",
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
        let result = dispatch_tool_call(request);
        std::future::ready(result)
    }
}

fn dispatch_tool_call(request: CallToolRequestParams) -> Result<CallToolResponse, ErrorData> {
    let tool_name = request.name.as_ref();

    match tool_name {
        "cards_compile_drafts" => {
            let args: CompileDraftsArgs = match parse_args(request.arguments) {
                Ok(parsed) => parsed,
                Err(msg) => return Ok(tool_error(msg)),
            };
            match compile_drafts(
                &args.drafts,
                args.auto_renumber_cloze,
                args.auto_purge_id_leaks,
                &args.default_campo_estudo,
            ) {
                Ok(val) => Ok(tool_ok(val)),
                Err(msg) => Ok(tool_error(msg)),
            }
        }
        "review_batch_information" => {
            let args: ReviewBatchInformationArgs = match parse_args(request.arguments) {
                Ok(parsed) => parsed,
                Err(msg) => return Ok(tool_error(msg)),
            };
            let review = review_batch_information(&args.qa_csv, &args.cloze_csv);
            match serde_json::to_value(&review) {
                Ok(val) => Ok(tool_ok(val)),
                Err(err) => Ok(tool_error(format!(
                    "Failed to serialize batch information review: {err}"
                ))),
            }
        }
        "quality_prepare" => {
            let args: QualityPrepareArgs = match parse_args(request.arguments) {
                Ok(parsed) => parsed,
                Err(msg) => return Ok(tool_error(msg)),
            };
            let mut violations = Vec::new();
            args.collect_range_violations(&mut violations);
            if !violations.is_empty() {
                return Ok(tool_error_payload(ToolErrorPayload {
                    error: format!(
                        "Invalid tool arguments: {} range violation(s) encountered",
                        violations.len()
                    ),
                    code: "INVALID_INPUT_RANGES",
                    violations,
                }));
            }
            let input = args.into_prepare_input();
            let report = prepare_quality_input(&input);
            match serde_json::to_value(&report) {
                Ok(val) => Ok(tool_ok(val)),
                Err(err) => Ok(tool_error(format!(
                    "Failed to serialize quality_prepare result: {err}"
                ))),
            }
        }
        "quality_evaluate" => {
            let args: QualityEvaluateArgs = match parse_args(request.arguments) {
                Ok(parsed) => parsed,
                Err(msg) => return Ok(tool_error(msg)),
            };
            let mut violations = Vec::new();
            args.collect_range_violations(&mut violations);
            if !violations.is_empty() {
                return Ok(tool_error_payload(ToolErrorPayload {
                    error: format!(
                        "Invalid tool arguments: {} range violation(s) encountered",
                        violations.len()
                    ),
                    code: "INVALID_INPUT_RANGES",
                    violations,
                }));
            }
            let input = args.into_evaluate_input();
            let report = evaluate_quality(&input);
            match serde_json::to_value(&report) {
                Ok(val) => Ok(tool_ok(val)),
                Err(err) => Ok(tool_error(format!(
                    "Failed to serialize quality_evaluate result: {err}"
                ))),
            }
        }
        "batch_patch_draft" => {
            let args: BatchPatchDraftArgs = match parse_args(request.arguments) {
                Ok(parsed) => parsed,
                Err(msg) => return Ok(tool_error(msg)),
            };
            match patch_draft(&Value::Object(args.draft), &args.updates) {
                Ok(card) => Ok(tool_ok(json!({ "card": card }))),
                Err(msg) => Ok(tool_error(msg)),
            }
        }
        "generation_contract_get" => {
            if let Err(msg) = parse_args::<EmptyArgs>(request.arguments) {
                return Ok(tool_error(msg));
            }
            Ok(tool_ok(generation_contract_payload()))
        }
        unknown => Err(ErrorData::new(
            ErrorCode::METHOD_NOT_FOUND,
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
        if obj.contains_key("properties") && !path.contains("drafts") {
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
    }

    #[test]
    fn exposes_six_tools_with_strict_schemas_and_readonly_annotations() {
        let tools = build_tools();
        assert_eq!(tools.len(), 6);
        let names: BTreeSet<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        let expected: BTreeSet<&str> = [
            "cards_compile_drafts",
            "review_batch_information",
            "quality_prepare",
            "quality_evaluate",
            "batch_patch_draft",
            "generation_contract_get",
        ]
        .into_iter()
        .collect();
        assert_eq!(names, expected);

        for tool in tools {
            let schema_val = tool.schema_as_json_value();
            assert_eq!(schema_val.get("additionalProperties"), Some(&json!(false)));
            assert_typed_schemas_strict(&schema_val, tool.name.as_ref());
            let ann = tool.annotations.as_ref().expect("annotations required");
            assert_eq!(ann.read_only_hint, Some(true));
            assert_eq!(ann.destructive_hint, Some(false));
            assert_eq!(ann.idempotent_hint, Some(true));
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
        assert_eq!(payload["code"], json!("INVALID_ARGUMENTS"));
        assert!(
            payload["error"]
                .as_str()
                .unwrap_or_default()
                .contains("CLOZ"),
            "error should mention unsupported tipo CLOZ"
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
    fn unknown_tool_returns_method_not_found_error() {
        let err = dispatch_tool_call(make_call("does_not_exist", json!({})))
            .expect_err("unknown tool must return protocol error");
        assert_eq!(err.code, ErrorCode::METHOD_NOT_FOUND);
    }
}
