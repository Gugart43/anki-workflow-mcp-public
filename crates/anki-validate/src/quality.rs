//! Source-grounded information quality gate (`prepare_quality` / `evaluate_quality`).
//!
//! Validates structural/heuristic batch health via `review_batch_information`,
//! enforces a deterministic source -> obligation -> binding -> evidence graph,
//! computes a review-independent `snapshot_hash` via `compute_receipt_v1`,
//! and verifies per-retrieval editorial attestations without ever claiming
//! clinical correctness or export authorization.

use anki_domain::{Diagnostic, Severity, canonical_csv_text, compute_receipt_v1};
use anki_parser::{find_cloze_spans, iter_data_lines, py_is_blank, split_row};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::{BatchInformationReview, review_batch_information};

pub const QUALITY_SCHEMA_VERSION: &str = "quality_v1";
const OWNER_SKILL: &str = "anki-post-curator";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityPrepareInput {
    pub qa_csv: String,
    pub cloze_csv: String,
    #[serde(default)]
    pub sources: Vec<QualitySource>,
    #[serde(default)]
    pub obligations: Vec<QualityObligation>,
    #[serde(default)]
    pub bindings: Vec<QualityBinding>,
}

pub struct QualityRequestRef<'a> {
    pub qa_csv: &'a str,
    pub cloze_csv: &'a str,
    pub sources: &'a [QualitySource],
    pub obligations: &'a [QualityObligation],
    pub bindings: &'a [QualityBinding],
    pub reviews: Option<&'a [QualityReview]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityInput {
    pub qa_csv: String,
    pub cloze_csv: String,
    #[serde(default)]
    pub sources: Vec<QualitySource>,
    #[serde(default)]
    pub obligations: Vec<QualityObligation>,
    #[serde(default)]
    pub bindings: Vec<QualityBinding>,
    #[serde(default)]
    pub reviews: Vec<QualityReview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualitySource {
    pub id: String,
    pub title: String,
    pub version: String,
    pub locator: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityObligation {
    pub id: String,
    pub description: String,
    pub source_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityBinding {
    pub note_type: String,
    pub source_row: usize,
    #[serde(default)]
    pub cloze_index: Option<usize>,
    pub obligation_ids: Vec<String>,
    pub evidence: Vec<QualityEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityEvidence {
    pub source_id: String,
    pub quote: String,
    pub locator: String,
    pub scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityDimensions {
    pub source_fidelity: bool,
    pub atomic_retrieval: bool,
    pub context_sufficient: bool,
    pub no_answer_leak: bool,
    pub clinically_relevant: bool,
    pub notes_appropriate: bool,
    pub media_appropriate: bool,
}

impl QualityDimensions {
    fn false_dimension_names(&self) -> Vec<&'static str> {
        let mut failed = Vec::new();
        if !self.source_fidelity {
            failed.push("source_fidelity");
        }
        if !self.atomic_retrieval {
            failed.push("atomic_retrieval");
        }
        if !self.context_sufficient {
            failed.push("context_sufficient");
        }
        if !self.no_answer_leak {
            failed.push("no_answer_leak");
        }
        if !self.clinically_relevant {
            failed.push("clinically_relevant");
        }
        if !self.notes_appropriate {
            failed.push("notes_appropriate");
        }
        if !self.media_appropriate {
            failed.push("media_appropriate");
        }
        failed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityReview {
    pub note_type: String,
    pub source_row: usize,
    #[serde(default)]
    pub cloze_index: Option<usize>,
    pub snapshot_hash: String,
    pub reviewer: String,
    pub verdict: String,
    pub rationale: String,
    pub dimensions: QualityDimensions,
}

#[derive(Debug, Serialize)]
pub struct QualityReport {
    pub schema_version: String,
    pub mode: String,
    pub snapshot_hash: String,
    pub content_hash: String,
    pub verdict: String,
    pub clinical_correctness_certified: bool,
    pub export_authorized: bool,
    pub retrieval_count: usize,
    pub reviewed_count: usize,
    pub obligation_count: usize,
    pub covered_obligation_count: usize,
    pub diagnostics: Vec<Diagnostic>,
    pub guidance: Vec<String>,
    pub review: BatchInformationReview,
}

#[derive(Serialize)]
struct SnapshotPayload<'a> {
    schema_version: &'static str,
    qa_csv: String,
    cloze_csv: String,
    sources: &'a [QualitySource],
    obligations: &'a [QualityObligation],
    bindings: &'a [QualityBinding],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NoteTypeTag {
    Qa,
    Cl,
}

impl NoteTypeTag {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Qa => "QA",
            Self::Cl => "CL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct RetrievalKey {
    note_type: NoteTypeTag,
    source_row: usize,
    cloze_index: Option<usize>,
}

impl RetrievalKey {
    fn label(self) -> String {
        match self.cloze_index {
            Some(idx) => format!("{}:row{}:c{idx}", self.note_type.as_str(), self.source_row),
            None => format!("{}:row{}", self.note_type.as_str(), self.source_row),
        }
    }
}

fn parse_retrieval_key(
    note_type: &str,
    source_row: usize,
    cloze_index: Option<usize>,
) -> Result<RetrievalKey, String> {
    if source_row < 2 {
        return Err(format!(
            "source_row {source_row} is invalid (CSV header is line 1; data rows start at 2)"
        ));
    }
    match note_type {
        "QA" => {
            if let Some(idx) = cloze_index {
                Err(format!(
                    "QA address at row {source_row} must have cloze_index = null, got {idx}"
                ))
            } else {
                Ok(RetrievalKey {
                    note_type: NoteTypeTag::Qa,
                    source_row,
                    cloze_index: None,
                })
            }
        }
        "CL" => match cloze_index {
            Some(idx) if idx >= 1 => Ok(RetrievalKey {
                note_type: NoteTypeTag::Cl,
                source_row,
                cloze_index: Some(idx),
            }),
            Some(0) => Err(format!(
                "CL address at row {source_row} has invalid cloze_index 0 (must be >= 1)"
            )),
            _ => Err(format!(
                "CL address at row {source_row} requires an explicit cloze_index >= 1"
            )),
        },
        other => Err(format!(
            "Invalid note_type '{other}' (expected literal 'QA' or 'CL')"
        )),
    }
}

fn normalize_crlf_only(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn compute_snapshot_hash(req: &QualityRequestRef<'_>) -> String {
    let payload = SnapshotPayload {
        schema_version: QUALITY_SCHEMA_VERSION,
        qa_csv: canonical_csv_text(req.qa_csv),
        cloze_csv: canonical_csv_text(req.cloze_csv),
        sources: req.sources,
        obligations: req.obligations,
        bindings: req.bindings,
    };
    let canonical_json =
        serde_json::to_string(&payload).expect("QualityInput types are always JSON-serializable");
    compute_receipt_v1("anki-quality-v1", &canonical_json).into_inner()
}

fn has_unclosed_cloze_marker(sentence: &str) -> bool {
    let spans = find_cloze_spans(sentence);
    let chars: Vec<char> = sentence.chars().collect();
    let n = chars.len();
    let mut i = 0usize;

    while i + 3 < n {
        if chars[i] == '{'
            && chars[i + 1] == '{'
            && chars[i + 2] == 'c'
            && chars[i + 3].is_ascii_digit()
            && !spans.iter().any(|s| i >= s.start && i < s.end)
        {
            return true;
        }
        i += 1;
    }
    false
}

fn diag_critical(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::from_static_code(code, Severity::Critical, OWNER_SKILL, message)
}

fn diag_warn(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::from_static_code(code, Severity::Warn, OWNER_SKILL, message)
}

fn check_batch_structural_and_card_criticals(
    qa_csv: &str,
    cloze_csv: &str,
    review: &BatchInformationReview,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeSet<RetrievalKey> {
    for d in &review.diagnostics {
        if d.severity == Severity::Critical {
            let mut qd = diag_critical(
                "QUALITY.BATCH_CRITICAL",
                format!("Critical batch diagnostic [{}]: {}", d.code, d.message),
            );
            if let Some(row) = d.row {
                qd = qd.with_row(row);
            }
            if let Some(field) = &d.field {
                qd = qd.with_field(field.clone());
            }
            diagnostics.push(qd);
        }
    }

    let mut expected_retrievals = BTreeSet::new();
    for card in &review.cards {
        for d in &card.diagnostics {
            if d.severity == Severity::Critical {
                let mut qd = diag_critical(
                    "QUALITY.CARD_CRITICAL",
                    format!("Critical card diagnostic [{}]: {}", d.code, d.message),
                )
                .with_row(card.source_row);
                if let Some(field) = &d.field {
                    qd = qd.with_field(field.clone());
                }
                diagnostics.push(qd);
            }
        }

        match card.note_type {
            "QA" => {
                if card.cloze_index.is_none() {
                    expected_retrievals.insert(RetrievalKey {
                        note_type: NoteTypeTag::Qa,
                        source_row: card.source_row,
                        cloze_index: None,
                    });
                }
            }
            "CL" => {
                if let Some(idx) = card.cloze_index.filter(|&i| i >= 1) {
                    expected_retrievals.insert(RetrievalKey {
                        note_type: NoteTypeTag::Cl,
                        source_row: card.source_row,
                        cloze_index: Some(idx),
                    });
                } else {
                    diagnostics.push(
                        diag_critical(
                            "QUALITY.MALFORMED_CLOZE",
                            format!(
                                "Cloze row {} has no valid {{cN::...}} index and cannot pass",
                                card.source_row
                            ),
                        )
                        .with_row(card.source_row)
                        .with_field("Sentença"),
                    );
                }
            }
            _ => {}
        }
    }

    for (row, line) in iter_data_lines(qa_csv) {
        let split = split_row(line, 7);
        if split.structural_ok {
            let prompt = split.cols.first().map_or("", String::as_str);
            let answer = split.cols.get(1).map_or("", String::as_str);
            if py_is_blank(prompt) || py_is_blank(answer) {
                diagnostics.push(
                    diag_critical(
                        "QUALITY.MALFORMED_QA",
                        format!("QA row {row} has blank Enunciado or Resposta"),
                    )
                    .with_row(row),
                );
            }
        }
    }

    for (row, line) in iter_data_lines(cloze_csv) {
        let split = split_row(line, 6);
        if split.structural_ok {
            let sentence = split.cols.first().map_or("", String::as_str);
            if has_unclosed_cloze_marker(sentence) {
                diagnostics.push(
                    diag_critical(
                        "QUALITY.MALFORMED_CLOZE",
                        format!("Cloze row {row} contains an unclosed or malformed cloze marker"),
                    )
                    .with_row(row)
                    .with_field("Sentença"),
                );
            }
        }
    }

    expected_retrievals
}

fn validate_sources<'a>(
    sources: &'a [QualitySource],
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<&'a str, &'a QualitySource> {
    let mut source_map = BTreeMap::new();
    for (idx, src) in sources.iter().enumerate() {
        if py_is_blank(&src.id) {
            diagnostics.push(diag_critical(
                "QUALITY.INVALID_SOURCE_ID",
                format!("Source at index {idx} has a blank id"),
            ));
        } else if source_map.insert(src.id.as_str(), src).is_some() {
            diagnostics.push(diag_critical(
                "QUALITY.DUPLICATE_SOURCE_ID",
                format!("Duplicate source id '{}'", src.id),
            ));
        }

        if py_is_blank(&src.title)
            || py_is_blank(&src.version)
            || py_is_blank(&src.locator)
            || py_is_blank(&src.text)
        {
            diagnostics.push(diag_warn(
                "QUALITY.INCOMPLETE_SOURCE",
                format!(
                    "Source '{}' has blank title, version, locator, or text",
                    src.id
                ),
            ));
        }
    }
    source_map
}

fn validate_obligations<'a>(
    obligations: &'a [QualityObligation],
    source_map: &BTreeMap<&'a str, &'a QualitySource>,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<&'a str, &'a QualityObligation> {
    if obligations.is_empty() {
        diagnostics.push(diag_warn(
            "QUALITY.NO_OBLIGATIONS",
            "Quality obligations list is empty; at least one sourced obligation is required",
        ));
    }

    let mut obligation_map = BTreeMap::new();
    for (idx, obl) in obligations.iter().enumerate() {
        if py_is_blank(&obl.id) {
            diagnostics.push(diag_critical(
                "QUALITY.INVALID_OBLIGATION_ID",
                format!("Obligation at index {idx} has a blank id"),
            ));
        } else if obligation_map.insert(obl.id.as_str(), obl).is_some() {
            diagnostics.push(diag_critical(
                "QUALITY.DUPLICATE_OBLIGATION_ID",
                format!("Duplicate obligation id '{}'", obl.id),
            ));
        }

        if py_is_blank(&obl.description) {
            diagnostics.push(diag_warn(
                "QUALITY.INCOMPLETE_OBLIGATION",
                format!("Obligation '{}' has blank description", obl.id),
            ));
        }

        if obl.source_ids.is_empty() {
            diagnostics.push(diag_warn(
                "QUALITY.OBLIGATION_NO_SOURCES",
                format!("Obligation '{}' cites no source_ids", obl.id),
            ));
        }

        for sid in &obl.source_ids {
            if py_is_blank(sid) || !source_map.contains_key(sid.as_str()) {
                diagnostics.push(diag_critical(
                    "QUALITY.ORPHAN_SOURCE_REF",
                    format!(
                        "Obligation '{}' references unknown or blank source_id '{}'",
                        obl.id, sid
                    ),
                ));
            }
        }
    }
    obligation_map
}

fn validate_bindings<'a>(
    bindings: &'a [QualityBinding],
    expected_retrievals: &BTreeSet<RetrievalKey>,
    source_map: &BTreeMap<&'a str, &'a QualitySource>,
    obligation_map: &BTreeMap<&'a str, &'a QualityObligation>,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeSet<&'a str> {
    let mut seen_bindings = BTreeSet::new();
    let mut covered_obligations = BTreeSet::new();

    for (idx, binding) in bindings.iter().enumerate() {
        let addr_label = match parse_retrieval_key(
            &binding.note_type,
            binding.source_row,
            binding.cloze_index,
        ) {
            Ok(key) => {
                if !seen_bindings.insert(key) {
                    diagnostics.push(
                        diag_critical(
                            "QUALITY.DUPLICATE_BINDING",
                            format!("Duplicate binding for retrieval {}", key.label()),
                        )
                        .with_row(binding.source_row),
                    );
                } else if !expected_retrievals.contains(&key) {
                    diagnostics.push(
                        diag_critical(
                            "QUALITY.ORPHAN_BINDING",
                            format!(
                                "Binding {} does not match any retrieval in CSV inputs",
                                key.label()
                            ),
                        )
                        .with_row(binding.source_row),
                    );
                }
                key.label()
            }
            Err(reason) => {
                diagnostics.push(
                    diag_critical(
                        "QUALITY.INVALID_BINDING_ADDRESS",
                        format!("Binding[{idx}] invalid address: {reason}"),
                    )
                    .with_row(binding.source_row),
                );
                format!("binding[{idx}]")
            }
        };

        if binding.obligation_ids.is_empty() {
            diagnostics.push(
                diag_warn(
                    "QUALITY.BINDING_NO_OBLIGATIONS",
                    format!("Binding {addr_label} cites no obligation_ids"),
                )
                .with_row(binding.source_row),
            );
        }

        for oid in &binding.obligation_ids {
            if py_is_blank(oid) || !obligation_map.contains_key(oid.as_str()) {
                diagnostics.push(
                    diag_critical(
                        "QUALITY.ORPHAN_OBLIGATION_REF",
                        format!(
                            "Binding {addr_label} references unknown or blank obligation_id '{oid}'"
                        ),
                    )
                    .with_row(binding.source_row),
                );
            } else {
                covered_obligations.insert(oid.as_str());
            }
        }

        if binding.evidence.is_empty() {
            diagnostics.push(
                diag_warn(
                    "QUALITY.MISSING_EVIDENCE",
                    format!("Binding {addr_label} has no evidence items"),
                )
                .with_row(binding.source_row),
            );
        }

        let mut has_valid_retrieval_evidence = false;
        let mut retrieval_evidence_sources: BTreeSet<&str> = BTreeSet::new();

        for (ev_idx, ev) in binding.evidence.iter().enumerate() {
            let valid_scope = matches!(ev.scope.as_str(), "retrieval" | "context" | "notes");
            if !valid_scope {
                diagnostics.push(
                    diag_critical(
                        "QUALITY.INVALID_EVIDENCE_SCOPE",
                        format!(
                            "Binding {addr_label} evidence[{ev_idx}] has invalid scope '{}' (expected retrieval/context/notes)",
                            ev.scope
                        ),
                    )
                    .with_row(binding.source_row),
                );
            }

            let src_opt = if py_is_blank(&ev.source_id) {
                None
            } else {
                source_map.get(ev.source_id.as_str()).copied()
            };
            if src_opt.is_none() {
                diagnostics.push(
                    diag_critical(
                        "QUALITY.ORPHAN_EVIDENCE_SOURCE",
                        format!(
                            "Binding {addr_label} evidence[{ev_idx}] references unknown or blank source_id '{}'",
                            ev.source_id
                        ),
                    )
                    .with_row(binding.source_row),
                );
            }

            let locator_ok = !py_is_blank(&ev.locator);
            if !locator_ok {
                diagnostics.push(
                    diag_warn(
                        "QUALITY.INCOMPLETE_EVIDENCE",
                        format!("Binding {addr_label} evidence[{ev_idx}] has blank locator"),
                    )
                    .with_row(binding.source_row),
                );
            }

            let quote_blank = py_is_blank(&ev.quote);
            let mut quote_matched = false;
            if quote_blank {
                diagnostics.push(
                    diag_warn(
                        "QUALITY.INCOMPLETE_EVIDENCE",
                        format!("Binding {addr_label} evidence[{ev_idx}] has blank quote"),
                    )
                    .with_row(binding.source_row),
                );
            } else if let Some(src) = src_opt {
                let norm_src = normalize_crlf_only(&src.text);
                let norm_quote = normalize_crlf_only(&ev.quote);
                if norm_src.contains(&norm_quote) {
                    quote_matched = true;
                } else {
                    diagnostics.push(
                        diag_critical(
                            "QUALITY.EVIDENCE_QUOTE_NOT_FOUND",
                            format!(
                                "Binding {addr_label} evidence[{ev_idx}] quote is not an exact substring of source '{}'",
                                src.id
                            ),
                        )
                        .with_row(binding.source_row),
                    );
                }
            }

            if valid_scope
                && ev.scope == "retrieval"
                && locator_ok
                && quote_matched
                && let Some(src) = src_opt
            {
                has_valid_retrieval_evidence = true;
                retrieval_evidence_sources.insert(src.id.as_str());
            }
        }

        if !binding.evidence.is_empty() && !has_valid_retrieval_evidence {
            diagnostics.push(
                diag_warn(
                    "QUALITY.MISSING_RETRIEVAL_EVIDENCE",
                    format!("Binding {addr_label} has no valid scope='retrieval' evidence excerpt"),
                )
                .with_row(binding.source_row),
            );
        }

        let mut checked_req_sources: BTreeSet<&str> = BTreeSet::new();
        for oid in &binding.obligation_ids {
            if let Some(obl) = obligation_map.get(oid.as_str()) {
                for req_sid in &obl.source_ids {
                    if source_map.contains_key(req_sid.as_str())
                        && checked_req_sources.insert(req_sid.as_str())
                        && !retrieval_evidence_sources.contains(req_sid.as_str())
                    {
                        diagnostics.push(
                            diag_warn(
                                "QUALITY.MISSING_OBLIGATION_SOURCE_EVIDENCE",
                                format!(
                                    "Binding {addr_label} cites obligation '{}' requiring source '{}' without matching retrieval evidence from '{}'",
                                    obl.id, req_sid, req_sid
                                ),
                            )
                            .with_row(binding.source_row),
                        );
                    }
                }
            }
        }
    }

    for key in expected_retrievals {
        if !seen_bindings.contains(key) {
            diagnostics.push(
                diag_warn(
                    "QUALITY.MISSING_BINDING",
                    format!("Missing quality binding for retrieval {}", key.label()),
                )
                .with_row(key.source_row),
            );
        }
    }

    for obl in obligation_map.values() {
        if !covered_obligations.contains(obl.id.as_str()) {
            diagnostics.push(diag_warn(
                "QUALITY.UNCOVERED_OBLIGATION",
                format!(
                    "Obligation '{}' is not covered by any binding in the batch",
                    obl.id
                ),
            ));
        }
    }

    covered_obligations
}

fn validate_reviews(
    reviews: &[QualityReview],
    expected_retrievals: &BTreeSet<RetrievalKey>,
    snapshot_hash: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> usize {
    let mut seen_reviews = BTreeSet::new();
    let mut fulfilled_reviews = BTreeSet::new();

    for (idx, rev) in reviews.iter().enumerate() {
        let mut address_ok = false;
        let parsed_key = parse_retrieval_key(&rev.note_type, rev.source_row, rev.cloze_index);
        let addr_label = match &parsed_key {
            Ok(key) => {
                if !seen_reviews.insert(*key) {
                    diagnostics.push(
                        diag_critical(
                            "QUALITY.DUPLICATE_REVIEW",
                            format!("Duplicate editorial review for retrieval {}", key.label()),
                        )
                        .with_row(rev.source_row),
                    );
                } else if !expected_retrievals.contains(key) {
                    diagnostics.push(
                        diag_critical(
                            "QUALITY.ORPHAN_REVIEW",
                            format!(
                                "Editorial review {} does not match any retrieval in CSV inputs",
                                key.label()
                            ),
                        )
                        .with_row(rev.source_row),
                    );
                } else {
                    address_ok = true;
                }
                key.label()
            }
            Err(reason) => {
                diagnostics.push(
                    diag_critical(
                        "QUALITY.INVALID_REVIEW_ADDRESS",
                        format!("Review[{idx}] invalid address: {reason}"),
                    )
                    .with_row(rev.source_row),
                );
                format!("review[{idx}]")
            }
        };

        let snapshot_ok = rev.snapshot_hash == snapshot_hash;
        if !snapshot_ok {
            diagnostics.push(
                diag_critical(
                    "QUALITY.STALE_REVIEW_SNAPSHOT",
                    format!(
                        "Review {addr_label} snapshot_hash '{}' does not match current snapshot '{snapshot_hash}'",
                        rev.snapshot_hash
                    ),
                )
                .with_row(rev.source_row),
            );
        }

        let reviewer_ok = !py_is_blank(&rev.reviewer);
        if !reviewer_ok {
            diagnostics.push(
                diag_warn(
                    "QUALITY.EMPTY_REVIEWER",
                    format!("Review {addr_label} has blank reviewer"),
                )
                .with_row(rev.source_row),
            );
        }

        let rationale_ok = !py_is_blank(&rev.rationale);
        if !rationale_ok {
            diagnostics.push(
                diag_warn(
                    "QUALITY.EMPTY_RATIONALE",
                    format!("Review {addr_label} has blank rationale"),
                )
                .with_row(rev.source_row),
            );
        }

        let verdict_ok = match rev.verdict.as_str() {
            "approve" => true,
            "revise" => {
                diagnostics.push(
                    diag_critical(
                        "QUALITY.REVIEW_REVISE",
                        format!("Review {addr_label} returned verdict 'revise'"),
                    )
                    .with_row(rev.source_row),
                );
                false
            }
            other => {
                diagnostics.push(
                    diag_critical(
                        "QUALITY.INVALID_REVIEW_VERDICT",
                        format!(
                            "Review {addr_label} has invalid verdict '{other}' (expected 'approve' or 'revise')"
                        ),
                    )
                    .with_row(rev.source_row),
                );
                false
            }
        };

        let failed_dims = rev.dimensions.false_dimension_names();
        let dims_ok = failed_dims.is_empty();
        if !dims_ok {
            diagnostics.push(
                diag_critical(
                    "QUALITY.REVIEW_DIMENSION_FAILED",
                    format!(
                        "Review {addr_label} has false editorial dimension(s): {}",
                        failed_dims.join(", ")
                    ),
                )
                .with_row(rev.source_row),
            );
        }

        if address_ok
            && snapshot_ok
            && reviewer_ok
            && rationale_ok
            && verdict_ok
            && dims_ok
            && let Ok(key) = parsed_key
        {
            fulfilled_reviews.insert(key);
        }
    }

    for key in expected_retrievals {
        if !seen_reviews.contains(key) {
            diagnostics.push(
                diag_warn(
                    "QUALITY.MISSING_REVIEW",
                    format!("Missing editorial review for retrieval {}", key.label()),
                )
                .with_row(key.source_row),
            );
        }
    }

    fulfilled_reviews.len()
}

fn build_guidance(mode: &str) -> Vec<String> {
    let mut guidance = vec![
        "Excerpt matching verifies literal substring inclusion in caller-supplied QualitySource.text only; it does not prove semantic entailment or clinical correctness.".to_owned(),
        "QualityReview records are self-reported editorial attestations bound to snapshot_hash, not trusted identities or server-issued export authorizations (clinical_correctness_certified and export_authorized remain false).".to_owned(),
        "Advisory length warnings (e.g., NOTE.LENGTH_WARN, CARD.TOO_LONG_*) remain non-blocking review signals in report.review.".to_owned(),
    ];
    if mode == "prepare" {
        guidance.push(
            "Prepared snapshot_hash excludes reviews; collect one current QualityReview per retrieval bound to this snapshot_hash and call evaluate_quality."
                .to_owned(),
        );
    }
    guidance
}

fn run_quality(req: &QualityRequestRef<'_>) -> QualityReport {
    let evaluate_reviews = req.reviews.is_some();
    let mode = if evaluate_reviews {
        "evaluate"
    } else {
        "prepare"
    };
    let content_hash = compute_receipt_v1(req.qa_csv, req.cloze_csv).into_inner();
    let snapshot_hash = compute_snapshot_hash(req);
    let review = review_batch_information(req.qa_csv, req.cloze_csv);
    let mut diagnostics = Vec::new();
    for (csv, columns) in [(req.qa_csv, 7), (req.cloze_csv, 6)] {
        for (row, line) in iter_data_lines(csv) {
            for field in split_row(line, columns).cols {
                if crate::html_security::has_unsafe_html(&field) {
                    diagnostics.push(
                        diag_critical(
                            "QUALITY.UNSAFE_HTML",
                            "Active HTML or unsafe attributes are not allowed in card fields",
                        )
                        .with_row(row),
                    );
                }
            }
        }
    }

    let expected_retrievals = check_batch_structural_and_card_criticals(
        req.qa_csv,
        req.cloze_csv,
        &review,
        &mut diagnostics,
    );
    diagnostics.extend(crate::quality_content::review_content(
        req.qa_csv,
        req.cloze_csv,
    ));

    let retrieval_count = review.total_cards;
    if retrieval_count == 0 {
        diagnostics.push(diag_warn(
            "QUALITY.EMPTY_BATCH",
            "Batch contains zero retrievals (empty or headers-only CSV inputs)",
        ));
    }

    let source_map = validate_sources(req.sources, &mut diagnostics);
    let obligation_map = validate_obligations(req.obligations, &source_map, &mut diagnostics);
    let covered_obligations = validate_bindings(
        req.bindings,
        &expected_retrievals,
        &source_map,
        &obligation_map,
        &mut diagnostics,
    );

    let reviewed_count = if let Some(reviews) = req.reviews {
        validate_reviews(
            reviews,
            &expected_retrievals,
            &snapshot_hash,
            &mut diagnostics,
        )
    } else {
        diagnostics.push(diag_warn(
            "QUALITY.REVIEWS_REQUIRED",
            "prepare_quality ignores review state; run evaluate_quality with current per-retrieval reviews",
        ));
        0
    };

    let verdict = if diagnostics.iter().any(|d| d.severity == Severity::Critical) {
        "FAIL"
    } else if diagnostics.iter().any(|d| d.severity == Severity::Warn) {
        "INCOMPLETE"
    } else {
        "PASS"
    };

    QualityReport {
        schema_version: QUALITY_SCHEMA_VERSION.to_owned(),
        mode: mode.to_owned(),
        snapshot_hash,
        content_hash,
        verdict: verdict.to_owned(),
        clinical_correctness_certified: false,
        export_authorized: false,
        retrieval_count,
        reviewed_count,
        obligation_count: req.obligations.len(),
        covered_obligation_count: covered_obligations.len(),
        diagnostics,
        guidance: build_guidance(mode),
        review,
    }
}

/// Validate batch integrity, source/obligation/binding graph, and compute the
/// review-independent `snapshot_hash` using `QualityPrepareInput` without reviews.
#[must_use]
pub fn prepare_quality_input(input: &QualityPrepareInput) -> QualityReport {
    let req = QualityRequestRef {
        qa_csv: &input.qa_csv,
        cloze_csv: &input.cloze_csv,
        sources: &input.sources,
        obligations: &input.obligations,
        bindings: &input.bindings,
        reviews: None,
    };
    run_quality(&req)
}

/// Validate batch integrity, source/obligation/binding graph, and compute the
/// review-independent `snapshot_hash` while ignoring `input.reviews`.
#[must_use]
pub fn prepare_quality(input: &QualityInput) -> QualityReport {
    let req = QualityRequestRef {
        qa_csv: &input.qa_csv,
        cloze_csv: &input.cloze_csv,
        sources: &input.sources,
        obligations: &input.obligations,
        bindings: &input.bindings,
        reviews: None,
    };
    run_quality(&req)
}

/// Validate batch integrity, source/obligation/binding graph, and current
/// per-retrieval editorial reviews against the computed `snapshot_hash`.
#[must_use]
pub fn evaluate_quality(input: &QualityInput) -> QualityReport {
    let req = QualityRequestRef {
        qa_csv: &input.qa_csv,
        cloze_csv: &input.cloze_csv,
        sources: &input.sources,
        obligations: &input.obligations,
        bindings: &input.bindings,
        reviews: Some(&input.reviews),
    };
    run_quality(&req)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_quality_input_matches_prepare_quality_snapshot() {
        let prep_input = QualityPrepareInput {
            qa_csv: "Enunciado|Resposta\nPergunta?|Resposta.\n".to_owned(),
            cloze_csv: "Sentença|Notas Adicionais\n{{c1::Item}} teste|Nota.\n".to_owned(),
            sources: vec![QualitySource {
                id: "S1".to_owned(),
                title: "T".to_owned(),
                version: "1.0".to_owned(),
                locator: "p1".to_owned(),
                text: "Pergunta? Resposta. Item teste".to_owned(),
            }],
            obligations: vec![QualityObligation {
                id: "O1".to_owned(),
                description: "Obl".to_owned(),
                source_ids: vec!["S1".to_owned()],
            }],
            bindings: vec![],
        };

        let legacy_input = QualityInput {
            qa_csv: prep_input.qa_csv.clone(),
            cloze_csv: prep_input.cloze_csv.clone(),
            sources: prep_input.sources.clone(),
            obligations: prep_input.obligations.clone(),
            bindings: prep_input.bindings.clone(),
            reviews: vec![],
        };

        let rep1 = prepare_quality_input(&prep_input);
        let rep2 = prepare_quality(&legacy_input);
        assert_eq!(rep1.snapshot_hash, rep2.snapshot_hash);
        assert_eq!(rep1.mode, "prepare");
        assert_eq!(rep1.verdict, rep2.verdict);
    }

    #[test]
    fn quality_prepare_input_rejects_unknown_reviews_field() {
        let json_with_reviews = serde_json::json!({
            "qa_csv": "",
            "cloze_csv": "",
            "reviews": []
        });
        let result = serde_json::from_value::<QualityPrepareInput>(json_with_reviews);
        assert!(
            result.is_err(),
            "QualityPrepareInput must deny unknown fields (reviews)"
        );
    }
}
