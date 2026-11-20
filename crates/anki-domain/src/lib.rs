//! Core domain types, invariants, and v1 receipt computation for `anki-workflow-mcp`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum NoteType {
    #[serde(rename = "QA")]
    Qa,
    #[serde(rename = "CL")]
    Cloze,
    #[serde(rename = "CLOZE")]
    ClozeLong,
}

impl NoteType {
    #[must_use]
    pub const fn fields(self) -> &'static [&'static str] {
        match self {
            Self::Qa => &QA_FIELDS,
            Self::Cloze | Self::ClozeLong => &CLOZE_FIELDS,
        }
    }

    #[must_use]
    pub const fn expected_columns(self) -> usize {
        self.fields().len()
    }

    #[must_use]
    pub const fn canonical_header(self) -> &'static str {
        match self {
            Self::Qa => QA_HEADER_CANONICAL,
            Self::Cloze | Self::ClozeLong => CLOZE_HEADER_CANONICAL,
        }
    }

    #[must_use]
    pub const fn as_canonical_str(self) -> &'static str {
        match self {
            Self::Qa => "QA",
            Self::Cloze | Self::ClozeLong => "CL",
        }
    }

    #[must_use]
    pub const fn is_strict_tag(self) -> bool {
        matches!(self, Self::Qa | Self::Cloze)
    }

    #[must_use]
    pub fn parse_strict_tag(value: &str) -> Option<Self> {
        match value {
            "QA" => Some(Self::Qa),
            "CL" => Some(Self::Cloze),
            _ => None,
        }
    }
}

impl fmt::Display for NoteType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_canonical_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid evidence scope '{0}': expected 'retrieval', 'context', or 'notes'")]
pub struct InvalidEvidenceScope(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceScope {
    Retrieval,
    Context,
    Notes,
}

impl EvidenceScope {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Retrieval => "retrieval",
            Self::Context => "context",
            Self::Notes => "notes",
        }
    }
}

impl fmt::Display for EvidenceScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EvidenceScope {
    type Err = InvalidEvidenceScope;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "retrieval" => Ok(Self::Retrieval),
            "context" => Ok(Self::Context),
            "notes" => Ok(Self::Notes),
            _ => Err(InvalidEvidenceScope(s.to_owned())),
        }
    }
}

impl PartialEq<str> for EvidenceScope {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for EvidenceScope {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid review verdict '{0}': expected 'approve' or 'revise'")]
pub struct InvalidReviewVerdict(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Approve,
    Revise,
}

impl ReviewVerdict {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Revise => "revise",
        }
    }

    #[must_use]
    pub const fn is_approved(self) -> bool {
        matches!(self, Self::Approve)
    }
}

impl fmt::Display for ReviewVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ReviewVerdict {
    type Err = InvalidReviewVerdict;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "approve" => Ok(Self::Approve),
            "revise" => Ok(Self::Revise),
            _ => Err(InvalidReviewVerdict(s.to_owned())),
        }
    }
}

impl PartialEq<str> for ReviewVerdict {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for ReviewVerdict {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    Critical,
    Warn,
    Info,
    Log,
}

impl Severity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "CRITICAL",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Log => "LOG",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum ValidationResult {
    Pass,
    Fail,
    Incomplete,
}

impl ValidationResult {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Incomplete => "INCOMPLETE",
        }
    }
}

impl fmt::Display for ValidationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityProfile {
    LegacyCompat,
    #[default]
    StrictV2,
}

impl CompatibilityProfile {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LegacyCompat => "legacy_compat",
            Self::StrictV2 => "strict_v2",
        }
    }
}

impl fmt::Display for CompatibilityProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct DeckId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct BatchId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BatchCount {
    pub qa_cards: usize,
    pub cloze_notes: usize,
    pub cloze_indices: usize,
    pub total: usize,
    pub within_limit: bool,
    pub excess: usize,
    pub ext_batch_suggested: Option<String>,
}

pub const MAX_BATCH: usize = 20;

pub const QA_FIELDS: [&str; 7] = [
    "Enunciado",
    "Resposta",
    "Campo de Estudo",
    "Notas Adicionais",
    "Imagens Relacionadas",
    "Referências",
    "Etiquetas",
];

pub const CLOZE_FIELDS: [&str; 6] = [
    "Sentença",
    "Notas Adicionais",
    "Campo de Estudo",
    "Imagens Relacionadas",
    "Referências",
    "Etiquetas",
];

pub const QA_HEADER_CANONICAL: &str = "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas";
pub const CLOZE_HEADER_CANONICAL: &str =
    "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("CSV data row number must be >= 2 (row 1 is the header), got {0}")]
pub struct InvalidCsvRowNumber(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, JsonSchema)]
#[serde(try_from = "usize")]
#[schemars(with = "usize")]
pub struct CsvRowNumber(usize);

impl CsvRowNumber {
    pub const MIN: usize = 2;

    #[must_use]
    pub const fn new(row: usize) -> Option<Self> {
        if row >= Self::MIN {
            Some(Self(row))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }

    #[must_use]
    pub const fn as_usize(self) -> usize {
        self.0
    }
}

impl TryFrom<usize> for CsvRowNumber {
    type Error = InvalidCsvRowNumber;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        Self::new(value).ok_or(InvalidCsvRowNumber(value))
    }
}

impl From<CsvRowNumber> for usize {
    fn from(value: CsvRowNumber) -> Self {
        value.0
    }
}

impl std::ops::Deref for CsvRowNumber {
    type Target = usize;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq<usize> for CsvRowNumber {
    fn eq(&self, other: &usize) -> bool {
        self.0 == *other
    }
}

impl PartialEq<CsvRowNumber> for usize {
    fn eq(&self, other: &CsvRowNumber) -> bool {
        *self == other.0
    }
}

impl Serialize for CsvRowNumber {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl fmt::Display for CsvRowNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "invalid diagnostic code '{0}': expected dotted uppercase identifier like 'CSV.BAD_COLUMNS'"
)]
pub struct InvalidDiagnosticCode(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, JsonSchema)]
#[serde(try_from = "String")]
#[schemars(with = "String")]
pub struct DiagnosticCode(Cow<'static, str>);

impl DiagnosticCode {
    const fn is_valid_bytes(bytes: &[u8]) -> bool {
        if bytes.is_empty() {
            return false;
        }
        let mut i = 0;
        let mut seg_len = 0;
        let mut dot_count = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'.' {
                if seg_len == 0 {
                    return false;
                }
                dot_count += 1;
                seg_len = 0;
            } else if matches!(b, b'A'..=b'Z' | b'0'..=b'9')
                || (seg_len > 0 && matches!(b, b'_' | b'-'))
            {
                seg_len += 1;
            } else {
                return false;
            }
            i += 1;
        }
        dot_count >= 1 && seg_len > 0
    }

    #[must_use]
    pub const fn from_static(code: &'static str) -> Self {
        assert!(
            Self::is_valid_bytes(code.as_bytes()),
            "invalid DiagnosticCode: expected dotted uppercase identifier"
        );
        Self(Cow::Borrowed(code))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }

    #[must_use]
    pub fn namespace(&self) -> &str {
        self.as_str().split('.').next().unwrap_or("")
    }

    pub fn parse(code: &str) -> Result<Self, InvalidDiagnosticCode> {
        Self::try_from(code)
    }

    pub fn dynamic(code: impl Into<String>) -> Result<Self, InvalidDiagnosticCode> {
        Self::try_from(code.into())
    }
}

impl TryFrom<String> for DiagnosticCode {
    type Error = InvalidDiagnosticCode;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if Self::is_valid_bytes(value.as_bytes()) {
            Ok(Self(Cow::Owned(value)))
        } else {
            Err(InvalidDiagnosticCode(value))
        }
    }
}

impl TryFrom<&str> for DiagnosticCode {
    type Error = InvalidDiagnosticCode;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_owned())
    }
}

impl FromStr for DiagnosticCode {
    type Err = InvalidDiagnosticCode;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl AsRef<str> for DiagnosticCode {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::borrow::Borrow<str> for DiagnosticCode {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl std::ops::Deref for DiagnosticCode {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl PartialEq<str> for DiagnosticCode {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for DiagnosticCode {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<DiagnosticCode> for str {
    fn eq(&self, other: &DiagnosticCode) -> bool {
        self == other.as_str()
    }
}

impl PartialEq<DiagnosticCode> for &str {
    fn eq(&self, other: &DiagnosticCode) -> bool {
        *self == other.as_str()
    }
}

impl PartialEq<String> for DiagnosticCode {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<DiagnosticCode> for String {
    fn eq(&self, other: &DiagnosticCode) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Serialize for DiagnosticCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub owner_skill: String,
    pub message: String,
    pub row: Option<usize>,
    pub field: Option<String>,
    pub suggestion: Option<String>,
    pub auto_fix_available: bool,
    pub fix_preview: Option<String>,
}

impl Diagnostic {
    #[must_use]
    pub fn new(
        code: DiagnosticCode,
        severity: Severity,
        owner_skill: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity,
            owner_skill: owner_skill.into(),
            message: message.into(),
            row: None,
            field: None,
            suggestion: None,
            auto_fix_available: false,
            fix_preview: None,
        }
    }

    #[must_use]
    pub fn from_static_code(
        code: &'static str,
        severity: Severity,
        owner_skill: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(
            DiagnosticCode::from_static(code),
            severity,
            owner_skill,
            message,
        )
    }

    #[must_use]
    pub fn with_row(mut self, row: impl Into<usize>) -> Self {
        self.row = Some(row.into());
        self
    }

    #[must_use]
    pub fn with_row_opt(mut self, row: Option<usize>) -> Self {
        self.row = row;
        self
    }

    #[must_use]
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    #[must_use]
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    #[must_use]
    pub fn with_auto_fix(mut self, available: bool, preview: Option<String>) -> Self {
        self.auto_fix_available = available;
        self.fix_preview = preview;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid content digest: expected 'sha256:' followed by 64 lowercase hex characters")]
pub struct InvalidContentDigest;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, JsonSchema)]
#[serde(try_from = "String")]
pub struct ContentDigest(String);

impl ContentDigest {
    fn is_valid(s: &str) -> bool {
        let Some(hex) = s.strip_prefix("sha256:") else {
            return false;
        };
        hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn parse(s: &str) -> Result<Self, InvalidContentDigest> {
        s.parse()
    }
}

impl TryFrom<String> for ContentDigest {
    type Error = InvalidContentDigest;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if Self::is_valid(&value) {
            Ok(Self(value))
        } else {
            Err(InvalidContentDigest)
        }
    }
}

impl TryFrom<&str> for ContentDigest {
    type Error = InvalidContentDigest;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_owned())
    }
}

impl FromStr for ContentDigest {
    type Err = InvalidContentDigest;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl AsRef<str> for ContentDigest {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::ops::Deref for ContentDigest {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl PartialEq<str> for ContentDigest {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for ContentDigest {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<ContentDigest> for str {
    fn eq(&self, other: &ContentDigest) -> bool {
        self == other.as_str()
    }
}

impl PartialEq<ContentDigest> for &str {
    fn eq(&self, other: &ContentDigest) -> bool {
        *self == other.as_str()
    }
}

impl Serialize for ContentDigest {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl fmt::Display for ContentDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[must_use]
pub fn canonical_csv_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .split('\n')
        .map(|line| line.trim_end_matches('\r'))
        .collect::<Vec<_>>()
        .join("\n")
}

#[must_use]
pub fn compute_receipt_v1(qa_csv: &str, cloze_csv: &str) -> ContentDigest {
    let norm_qa = canonical_csv_text(qa_csv);
    let norm_cloze = canonical_csv_text(cloze_csv);

    let mut hasher = Sha256::new();
    hasher.update(norm_qa.as_bytes());
    hasher.update(b"\n---\n");
    hasher.update(norm_cloze.as_bytes());
    let digest = hasher.finalize();

    let mut hex = String::with_capacity(7 + 64);
    hex.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    ContentDigest(hex)
}

#[must_use]
pub fn verify_receipt_v1(receipt_hash: &str, qa_csv: &str, cloze_csv: &str) -> bool {
    match ContentDigest::try_from(receipt_hash) {
        Ok(parsed) => parsed == compute_receipt_v1(qa_csv, cloze_csv),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_normalizes_crlf_and_lf_identically() {
        let qa_lf = "Enunciado|Resposta\nQ1|A1\n";
        let qa_crlf = "Enunciado|Resposta\r\nQ1|A1\r\n";
        let cl_lf = "Sentença|Notas\n{{c1::x}}|n\n";
        let cl_crlf = "Sentença|Notas\r\n{{c1::x}}|n\r\n";

        assert_eq!(
            compute_receipt_v1(qa_lf, cl_lf),
            compute_receipt_v1(qa_crlf, cl_crlf)
        );
        let digest = compute_receipt_v1(qa_lf, cl_lf);
        assert!(digest.as_str().starts_with("sha256:"));
        assert_eq!(digest.as_str().len(), 71);
        assert!(verify_receipt_v1(digest.as_str(), qa_crlf, cl_crlf));
        // Test Deref and PartialEq<&str>
        assert!(digest.starts_with("sha256:"));
        assert_eq!(digest, digest.as_str());
    }

    #[test]
    fn canonical_text_strips_trailing_cr_per_line() {
        assert_eq!(canonical_csv_text("a\r\r\nb\r\nc\r"), "a\nb\nc");
    }

    #[test]
    fn note_type_fields_and_accessors() {
        assert_eq!(NoteType::Qa.expected_columns(), 7);
        assert_eq!(NoteType::Qa.fields(), &QA_FIELDS);
        assert_eq!(NoteType::Qa.canonical_header(), QA_HEADER_CANONICAL);
        assert_eq!(NoteType::Qa.as_canonical_str(), "QA");
        assert!(NoteType::Qa.is_strict_tag());

        assert_eq!(NoteType::Cloze.expected_columns(), 6);
        assert_eq!(NoteType::Cloze.fields(), &CLOZE_FIELDS);
        assert_eq!(NoteType::Cloze.canonical_header(), CLOZE_HEADER_CANONICAL);
        assert_eq!(NoteType::Cloze.as_canonical_str(), "CL");
        assert!(NoteType::Cloze.is_strict_tag());

        assert_eq!(NoteType::ClozeLong.expected_columns(), 6);
        assert_eq!(NoteType::ClozeLong.fields(), &CLOZE_FIELDS);
        assert_eq!(
            NoteType::ClozeLong.canonical_header(),
            CLOZE_HEADER_CANONICAL
        );
        assert_eq!(NoteType::ClozeLong.as_canonical_str(), "CL");
        assert!(!NoteType::ClozeLong.is_strict_tag());

        assert_eq!(NoteType::parse_strict_tag("QA"), Some(NoteType::Qa));
        assert_eq!(NoteType::parse_strict_tag("CL"), Some(NoteType::Cloze));
        assert_eq!(NoteType::parse_strict_tag("CLOZE"), None);
        assert_eq!(NoteType::parse_strict_tag("INVALID"), None);
    }

    #[test]
    fn note_type_serde_boundaries() {
        assert_eq!(
            serde_json::from_str::<NoteType>("\"QA\"").unwrap(),
            NoteType::Qa
        );
        assert_eq!(
            serde_json::from_str::<NoteType>("\"CL\"").unwrap(),
            NoteType::Cloze
        );
        assert_eq!(
            serde_json::from_str::<NoteType>("\"CLOZE\"").unwrap(),
            NoteType::ClozeLong
        );
        assert!(serde_json::from_str::<NoteType>("\"INVALID\"").is_err());
        assert!(serde_json::from_str::<NoteType>("\"qa\"").is_err());
        assert!(serde_json::from_str::<NoteType>("123").is_err());
    }

    #[test]
    fn evidence_scope_variants_and_serde() {
        for (scope, expected) in [
            (EvidenceScope::Retrieval, "retrieval"),
            (EvidenceScope::Context, "context"),
            (EvidenceScope::Notes, "notes"),
        ] {
            assert_eq!(scope.as_str(), expected);
            assert_eq!(format!("{scope}"), expected);
            assert_eq!(scope, expected);
            assert_eq!(expected.parse::<EvidenceScope>().unwrap(), scope);

            let json = serde_json::to_string(&scope).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
            assert_eq!(serde_json::from_str::<EvidenceScope>(&json).unwrap(), scope);
        }

        assert!("invalid".parse::<EvidenceScope>().is_err());
        assert!(serde_json::from_str::<EvidenceScope>("\"unknown\"").is_err());
    }

    #[test]
    fn review_verdict_variants_and_serde() {
        assert_eq!(ReviewVerdict::Approve.as_str(), "approve");
        assert!(ReviewVerdict::Approve.is_approved());
        assert_eq!(format!("{}", ReviewVerdict::Approve), "approve");
        assert_eq!(ReviewVerdict::Approve, "approve");
        assert_eq!(
            "approve".parse::<ReviewVerdict>().unwrap(),
            ReviewVerdict::Approve
        );

        assert_eq!(ReviewVerdict::Revise.as_str(), "revise");
        assert!(!ReviewVerdict::Revise.is_approved());
        assert_eq!(format!("{}", ReviewVerdict::Revise), "revise");
        assert_eq!(ReviewVerdict::Revise, "revise");
        assert_eq!(
            "revise".parse::<ReviewVerdict>().unwrap(),
            ReviewVerdict::Revise
        );

        assert!("other".parse::<ReviewVerdict>().is_err());
        assert!(serde_json::from_str::<ReviewVerdict>("\"reject\"").is_err());
    }

    #[test]
    fn csv_row_number_boundaries_and_serde() {
        assert_eq!(CsvRowNumber::new(0), None);
        assert_eq!(CsvRowNumber::new(1), None);
        assert!(CsvRowNumber::try_from(0).is_err());
        assert!(CsvRowNumber::try_from(1).is_err());

        let row2 = CsvRowNumber::new(2).expect("2 is min valid row");
        assert_eq!(row2.get(), 2);
        assert_eq!(row2.as_usize(), 2);
        assert_eq!(*row2, 2);
        assert_eq!(row2, 2);
        assert_eq!(2, row2);
        assert_eq!(usize::from(row2), 2);
        assert_eq!(format!("{row2}"), "2");

        let row100 = CsvRowNumber::try_from(100).expect("100 is valid");
        assert_eq!(row100.get(), 100);

        // Serde boundaries: >= 2 ok, 0 and 1 fail
        assert_eq!(serde_json::from_str::<CsvRowNumber>("2").unwrap(), row2);
        assert_eq!(serde_json::from_str::<CsvRowNumber>("100").unwrap(), row100);
        assert!(serde_json::from_str::<CsvRowNumber>("1").is_err());
        assert!(serde_json::from_str::<CsvRowNumber>("0").is_err());
        assert!(serde_json::from_str::<CsvRowNumber>("\"2\"").is_err());
        assert!(serde_json::from_str::<CsvRowNumber>("-5").is_err());

        // Serialize
        assert_eq!(serde_json::to_string(&row2).unwrap(), "2");
    }

    #[test]
    fn diagnostic_code_validation_and_methods() {
        let code = DiagnosticCode::from_static("CSV.BAD_COLUMNS");
        assert_eq!(code.as_str(), "CSV.BAD_COLUMNS");
        assert_eq!(code.namespace(), "CSV");
        assert_eq!(code, "CSV.BAD_COLUMNS");
        assert_eq!("CSV.BAD_COLUMNS", code);
        assert_eq!(code, "CSV.BAD_COLUMNS".to_string());
        assert_eq!(format!("{code}"), "CSV.BAD_COLUMNS");
        assert!(code.starts_with("CSV."));

        // Additional valid forms
        assert!(DiagnosticCode::parse("HINTS.EMPTY").is_ok());
        assert!(DiagnosticCode::parse("CARD.TOO_LONG_DELETION").is_ok());
        assert!(DiagnosticCode::parse("NOTE.DUP_ANSWER_WARN").is_ok());
        assert!(DiagnosticCode::parse("A.B").is_ok());
        assert!(DiagnosticCode::parse("A1.B2").is_ok());
        assert!(DiagnosticCode::parse("A-1.B_2").is_ok());
        assert!(DiagnosticCode::parse("A.B.C").is_ok());

        // Dynamic helper
        let dyn_code = DiagnosticCode::dynamic("CSV.BAD_COLUMNS".to_string()).unwrap();
        assert_eq!(dyn_code, code);

        // Invalid forms
        assert!(DiagnosticCode::parse("").is_err());
        assert!(DiagnosticCode::parse("CSV").is_err());
        assert!(DiagnosticCode::parse(".BAD").is_err());
        assert!(DiagnosticCode::parse("BAD.").is_err());
        assert!(DiagnosticCode::parse("CSV..BAD").is_err());
        assert!(DiagnosticCode::parse("csv.bad").is_err());
        assert!(DiagnosticCode::parse("CSV.bad").is_err());
        assert!(DiagnosticCode::parse("CSV._BAD").is_err());
        assert!(DiagnosticCode::parse("CSV.-BAD").is_err());
        assert!(DiagnosticCode::parse("CSV.BAD COLUMNS").is_err());
        assert!(DiagnosticCode::parse("CSV.BAD@").is_err());

        // Serde
        assert_eq!(
            serde_json::from_str::<DiagnosticCode>("\"CSV.BAD_COLUMNS\"").unwrap(),
            code
        );
        assert!(serde_json::from_str::<DiagnosticCode>("\"invalid\"").is_err());
        assert!(serde_json::from_str::<DiagnosticCode>("\"\"").is_err());
        assert!(serde_json::from_str::<DiagnosticCode>("123").is_err());
    }

    #[test]
    #[should_panic(expected = "invalid DiagnosticCode")]
    fn diagnostic_code_from_static_panics_on_invalid() {
        let _ = DiagnosticCode::from_static("invalid_code_without_dot");
    }

    #[test]
    fn content_digest_validation_and_boundaries() {
        let valid_hex = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let digest = ContentDigest::try_from(valid_hex).expect("valid 64 hex");
        assert_eq!(digest.as_str(), valid_hex);
        assert_eq!(digest, valid_hex);
        assert_eq!(valid_hex, digest);
        assert_eq!(format!("{digest}"), valid_hex);
        assert_eq!(digest.clone().into_inner(), valid_hex);

        // Serde roundtrip
        let serialized = serde_json::to_string(&digest).unwrap();
        assert_eq!(serialized, format!("\"{valid_hex}\""));
        assert_eq!(
            serde_json::from_str::<ContentDigest>(&serialized).unwrap(),
            digest
        );

        // Boundary rejections
        assert!(ContentDigest::parse("").is_err());
        assert!(ContentDigest::parse("sha256:").is_err());
        assert!(ContentDigest::parse("sha256:0123").is_err());
        // 63 chars (too short)
        assert!(
            ContentDigest::parse(
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde"
            )
            .is_err()
        );
        // 65 chars (too long)
        assert!(
            ContentDigest::parse(
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0"
            )
            .is_err()
        );
        // Uppercase hex rejected
        assert!(
            ContentDigest::parse(
                "sha256:0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef"
            )
            .is_err()
        );
        // Non-hex character 'g'
        assert!(
            ContentDigest::parse(
                "sha256:g123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            )
            .is_err()
        );
        // Wrong prefix
        assert!(
            ContentDigest::parse(
                "md5:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            )
            .is_err()
        );

        // Serde rejection
        assert!(serde_json::from_str::<ContentDigest>("\"sha256:invalid\"").is_err());
        assert!(serde_json::from_str::<ContentDigest>("123").is_err());
    }

    #[test]
    fn diagnostic_builder_and_with_row_convertible() {
        let code = DiagnosticCode::from_static("CSV.BAD_COLUMNS");
        let row = CsvRowNumber::new(5).unwrap();

        // with_row taking CsvRowNumber
        let d1 = Diagnostic::new(
            code.clone(),
            Severity::Critical,
            "csv-skill",
            "bad column count",
        )
        .with_row(row)
        .with_field("Enunciado")
        .with_suggestion("add delimiter")
        .with_auto_fix(true, Some("preview".into()));

        assert_eq!(d1.code, "CSV.BAD_COLUMNS");
        assert_eq!(d1.severity, Severity::Critical);
        assert_eq!(d1.row, Some(5));
        assert_eq!(d1.field.as_deref(), Some("Enunciado"));
        assert_eq!(d1.suggestion.as_deref(), Some("add delimiter"));
        assert!(d1.auto_fix_available);
        assert_eq!(d1.fix_preview.as_deref(), Some("preview"));

        // with_row taking usize directly
        let d2 =
            Diagnostic::from_static_code("CSV.BAD_COLUMNS", Severity::Warn, "csv-skill", "msg")
                .with_row(2usize);
        assert_eq!(d2.row, Some(2));

        // with_row_opt
        let d3 =
            Diagnostic::from_static_code("CSV.BAD_COLUMNS", Severity::Info, "csv-skill", "msg")
                .with_row_opt(Some(3));
        assert_eq!(d3.row, Some(3));

        // JSON serialization roundtrip
        let json = serde_json::to_string(&d1).unwrap();
        let de: Diagnostic = serde_json::from_str(&json).unwrap();
        assert_eq!(de, d1);
    }
}
