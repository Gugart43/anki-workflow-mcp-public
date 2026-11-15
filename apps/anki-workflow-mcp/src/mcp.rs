use std::future::Future;
use std::sync::Arc;

use anki_validate::quality::{QualityInput, evaluate_quality, prepare_quality};
use anki_validate::{compile_drafts, patch_draft, review_batch_information};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ErrorCode, Implementation, JsonObject,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

const MAX_ERROR_MESSAGE_CHARS: usize = 400;

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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompileDraftsArgs {
    drafts: Vec<Value>,
    #[serde(default = "default_true")]
    auto_renumber_cloze: bool,
    #[serde(default = "default_true")]
    auto_purge_id_leaks: bool,
    #[serde(default)]
    default_campo_estudo: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewBatchInformationArgs {
    qa_csv: String,
    cloze_csv: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchPatchDraftArgs {
    draft: Map<String, Value>,
    updates: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
enum QualityNoteTypeArg {
    QA,
    CL,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum EvidenceScopeArg {
    Retrieval,
    Context,
    Notes,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ReviewVerdictArg {
    Approve,
    Revise,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct QualitySourceArg {
    id: String,
    title: String,
    version: String,
    locator: String,
    text: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct QualityObligationArg {
    id: String,
    description: String,
    source_ids: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct QualityEvidenceArg {
    source_id: String,
    quote: String,
    locator: String,
    scope: EvidenceScopeArg,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct QualityBindingArg {
    note_type: QualityNoteTypeArg,
    source_row: i64,
    #[serde(default)]
    cloze_index: Option<i64>,
    obligation_ids: Vec<String>,
    evidence: Vec<QualityEvidenceArg>,
}

#[derive(Debug, Deserialize, Serialize)]
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

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct QualityReviewArg {
    note_type: QualityNoteTypeArg,
    source_row: i64,
    #[serde(default)]
    cloze_index: Option<i64>,
    snapshot_hash: String,
    reviewer: String,
    verdict: ReviewVerdictArg,
    rationale: String,
    dimensions: ReviewDimensionsArg,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct QualityInputArgs {
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

impl QualityInputArgs {
    fn validate_ranges(&self) -> Result<(), String> {
        for (idx, binding) in self.bindings.iter().enumerate() {
            if binding.source_row < 2 {
                return Err(format!(
                    "bindings[{idx}].source_row must be an integer >= 2"
                ));
            }
            if let Some(cloze_idx) = binding.cloze_index
                && cloze_idx < 1
            {
                return Err(format!(
                    "bindings[{idx}].cloze_index must be null or an integer >= 1"
                ));
            }
        }

        for (idx, review) in self.reviews.iter().enumerate() {
            if review.source_row < 2 {
                return Err(format!("reviews[{idx}].source_row must be an integer >= 2"));
            }
            if let Some(cloze_idx) = review.cloze_index
                && cloze_idx < 1
            {
                return Err(format!(
                    "reviews[{idx}].cloze_index must be null or an integer >= 1"
                ));
            }
        }

        Ok(())
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
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_ERROR_MESSAGE_CHARS {
        return collapsed;
    }
    let truncated: String = collapsed.chars().take(MAX_ERROR_MESSAGE_CHARS).collect();
    format!("{truncated}...")
}

fn tool_error(message: impl AsRef<str>) -> CallToolResponse {
    CallToolResult::structured_error(json!({
        "error": sanitize_error_text(message.as_ref()),
    }))
    .into()
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

fn parse_quality_input(arguments: Option<JsonObject>) -> Result<QualityInput, String> {
    let validated: QualityInputArgs = parse_args(arguments)?;
    validated.validate_ranges()?;
    let canonical_value = serde_json::to_value(&validated).map_err(|err| {
        format!(
            "Failed to normalize quality input: {}",
            sanitize_error_text(&err.to_string())
        )
    })?;
    serde_json::from_value::<QualityInput>(canonical_value).map_err(|err| {
        format!(
            "Invalid quality input: {}",
            sanitize_error_text(&err.to_string())
        )
    })
}

fn schema_object(value: Value) -> Arc<JsonObject> {
    match value {
        Value::Object(map) => Arc::new(map),
        _ => unreachable!("tool schema must be a JSON object"),
    }
}

fn readonly_annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(false)
}

fn quality_input_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["qa_csv", "cloze_csv"],
        "properties": {
            "qa_csv": {
                "type": "string",
                "description": "Pipe-delimited QA CSV content (header plus 0 or more rows)."
            },
            "cloze_csv": {
                "type": "string",
                "description": "Pipe-delimited Cloze CSV content (header plus 0 or more rows)."
            },
            "sources": {
                "type": "array",
                "default": [],
                "description": "Authoritative source excerpts with version and locator metadata.",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "title", "version", "locator", "text"],
                    "properties": {
                        "id": { "type": "string" },
                        "title": { "type": "string" },
                        "version": { "type": "string" },
                        "locator": { "type": "string" },
                        "text": { "type": "string" }
                    }
                }
            },
            "obligations": {
                "type": "array",
                "default": [],
                "description": "Clinical or decision obligations inventoried before card generation.",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "description", "source_ids"],
                    "properties": {
                        "id": { "type": "string" },
                        "description": { "type": "string" },
                        "source_ids": {
                            "type": "array",
                            "items": { "type": "string" }
                        }
                    }
                }
            },
            "bindings": {
                "type": "array",
                "default": [],
                "description": "Bindings linking each QA row or Cloze index to obligations and source evidence.",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["note_type", "source_row", "obligation_ids", "evidence"],
                    "properties": {
                        "note_type": {
                            "type": "string",
                            "enum": ["QA", "CL"]
                        },
                        "source_row": {
                            "type": "integer",
                            "minimum": 2
                        },
                        "cloze_index": {
                            "type": ["integer", "null"],
                            "minimum": 1
                        },
                        "obligation_ids": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "evidence": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["source_id", "quote", "locator", "scope"],
                                "properties": {
                                    "source_id": { "type": "string" },
                                    "quote": { "type": "string" },
                                    "locator": { "type": "string" },
                                    "scope": {
                                        "type": "string",
                                        "enum": ["retrieval", "context", "notes"]
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "reviews": {
                "type": "array",
                "default": [],
                "description": "Independent editorial review records bound to snapshot_hash per QA row or Cloze index.",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": [
                        "note_type",
                        "source_row",
                        "snapshot_hash",
                        "reviewer",
                        "verdict",
                        "rationale",
                        "dimensions"
                    ],
                    "properties": {
                        "note_type": {
                            "type": "string",
                            "enum": ["QA", "CL"]
                        },
                        "source_row": {
                            "type": "integer",
                            "minimum": 2
                        },
                        "cloze_index": {
                            "type": ["integer", "null"],
                            "minimum": 1
                        },
                        "snapshot_hash": { "type": "string" },
                        "reviewer": { "type": "string" },
                        "verdict": {
                            "type": "string",
                            "enum": ["approve", "revise"]
                        },
                        "rationale": { "type": "string" },
                        "dimensions": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": [
                                "source_fidelity",
                                "atomic_retrieval",
                                "context_sufficient",
                                "no_answer_leak",
                                "clinically_relevant",
                                "notes_appropriate",
                                "media_appropriate"
                            ],
                            "properties": {
                                "source_fidelity": { "type": "boolean" },
                                "atomic_retrieval": { "type": "boolean" },
                                "context_sufficient": { "type": "boolean" },
                                "no_answer_leak": { "type": "boolean" },
                                "clinically_relevant": { "type": "boolean" },
                                "notes_appropriate": { "type": "boolean" },
                                "media_appropriate": { "type": "boolean" }
                            }
                        }
                    }
                }
            }
        }
    })
}

fn build_tools() -> Vec<Tool> {
    let ann = readonly_annotations();

    let cards_compile_drafts = Tool::new(
        "cards_compile_drafts",
        "Normalize and compile structured QA/Cloze draft objects in memory into canonical pipe-delimited `qa_csv` and `cloze_csv` with structural diagnostics and statistics.",
        schema_object(json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["drafts"],
            "properties": {
                "drafts": {
                    "type": "array",
                    "description": "List of card draft objects (QA or Cloze) to normalize and compile.",
                    "items": {
                        "type": "object",
                        "description": "Dynamic card draft object supporting canonical fields and aliases (`tipo`/`note_type`, `enunciado_ou_sentenca`, `resposta`, `campo_estudo`, `notas_adicionais`, `referencias`, `etiquetas`, `figuras`, `tabela_notas`, etc.)."
                    }
                },
                "auto_renumber_cloze": {
                    "type": "boolean",
                    "default": true,
                    "description": "Sequentially renumber Cloze indices starting at c1 (default true)."
                },
                "auto_purge_id_leaks": {
                    "type": "boolean",
                    "default": true,
                    "description": "Strip leaked internal draft/card identifiers from text fields (default true)."
                },
                "default_campo_estudo": {
                    "type": "string",
                    "default": "",
                    "description": "Fallback `Campo de Estudo` when omitted on individual drafts (default empty string)."
                }
            }
        })),
    )
    .with_annotations(ann.clone());

    let review_batch_information = Tool::new(
        "review_batch_information",
        "Inspect compiled `qa_csv` and `cloze_csv` in memory and return per-card information profiles, token/character counts, and card totals without a fixed 20-card ceiling.",
        schema_object(json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["qa_csv", "cloze_csv"],
            "properties": {
                "qa_csv": {
                    "type": "string",
                    "description": "Pipe-delimited QA CSV string."
                },
                "cloze_csv": {
                    "type": "string",
                    "description": "Pipe-delimited Cloze CSV string."
                }
            }
        })),
    )
    .with_annotations(ann.clone());

    let quality_prepare = Tool::new(
        "quality_prepare",
        "Prepare a deterministic quality snapshot (`snapshot_hash`) and per-retrieval inventory from compiled CSVs and optional quality bindings/sources/obligations/reviews.",
        schema_object(quality_input_schema()),
    )
    .with_annotations(ann.clone());

    let quality_evaluate = Tool::new(
        "quality_evaluate",
        "Evaluate source fidelity, obligation coverage, evidence bindings, and independent editorial reviews per QA row and Cloze index against the batch snapshot.",
        schema_object(quality_input_schema()),
    )
    .with_annotations(ann.clone());

    let batch_patch_draft = Tool::new(
        "batch_patch_draft",
        "Pure in-memory draft patcher: accepts `{draft, updates}` JSON objects and returns `{card}` with updated fields normalized. NOTE: This is NOT the path-based Python staging file API (`staging_path`/`card_index`/`patch_json`); it performs no disk reads or writes.",
        schema_object(json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["draft", "updates"],
            "properties": {
                "draft": {
                    "type": "object",
                    "description": "Existing single card draft JSON object to patch in memory."
                },
                "updates": {
                    "type": "object",
                    "description": "Field updates to merge into the draft object in memory (empty strings clear optional fields)."
                }
            }
        })),
    )
    .with_annotations(ann.clone());

    let generation_contract_get = Tool::new(
        "generation_contract_get",
        "Return the normative `quality_v1` generation, review, media, field-ordering, limitation, and repair contract. Accepts `{}` only.",
        schema_object(json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {}
        })),
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
        std::future::ready(Ok(ListToolsResult::with_all_items(build_tools())))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        build_tools().into_iter().find(|tool| tool.name == name)
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
                    "Failed to serialize batch information review: {}",
                    err
                ))),
            }
        }
        "quality_prepare" => {
            let input = match parse_quality_input(request.arguments) {
                Ok(parsed) => parsed,
                Err(msg) => return Ok(tool_error(msg)),
            };
            let report = prepare_quality(&input);
            match serde_json::to_value(&report) {
                Ok(val) => Ok(tool_ok(val)),
                Err(err) => Ok(tool_error(format!(
                    "Failed to serialize quality_prepare result: {}",
                    err
                ))),
            }
        }
        "quality_evaluate" => {
            let input = match parse_quality_input(request.arguments) {
                Ok(parsed) => parsed,
                Err(msg) => return Ok(tool_error(msg)),
            };
            let report = evaluate_quality(&input);
            match serde_json::to_value(&report) {
                Ok(val) => Ok(tool_ok(val)),
                Err(err) => Ok(tool_error(format!(
                    "Failed to serialize quality_evaluate result: {}",
                    err
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

    fn assert_typed_schemas_strict(schema: &Value, path: &str) {
        let Some(obj) = schema.as_object() else {
            return;
        };
        if obj.contains_key("properties") {
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

        for tool in &tools {
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
    fn quality_validation_rejects_unknown_nested_fields_and_invalid_ranges() {
        let unknown_top = dispatch_tool_call(make_call(
            "quality_prepare",
            json!({ "qa_csv": "", "cloze_csv": "", "bogus": 1 }),
        ))
        .expect("returns isError result");
        assert!(is_error_response(&unknown_top));

        let bad_row = dispatch_tool_call(make_call(
            "quality_evaluate",
            json!({
                "qa_csv": "",
                "cloze_csv": "",
                "bindings": [{
                    "note_type": "QA",
                    "source_row": 1,
                    "obligation_ids": [],
                    "evidence": []
                }]
            }),
        ))
        .expect("returns isError result");
        assert!(is_error_response(&bad_row));

        let bad_cloze_idx = dispatch_tool_call(make_call(
            "quality_prepare",
            json!({
                "qa_csv": "",
                "cloze_csv": "",
                "reviews": [{
                    "note_type": "CL",
                    "source_row": 2,
                    "cloze_index": 0,
                    "snapshot_hash": "h",
                    "reviewer": "rev",
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
                }]
            }),
        ))
        .expect("returns isError result");
        assert!(is_error_response(&bad_cloze_idx));
    }

    #[test]
    fn unknown_tool_returns_method_not_found_error() {
        let err = dispatch_tool_call(make_call("does_not_exist", json!({})))
            .expect_err("unknown tool must return protocol error");
        assert_eq!(err.code, ErrorCode::METHOD_NOT_FOUND);
    }
}
