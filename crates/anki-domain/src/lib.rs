//! Core domain types, invariants, and v1 receipt computation for `anki-workflow-mcp`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Legacy note type representation matching Python `NoteType(StrEnum)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NoteType {
    #[serde(rename = "QA")]
    Qa,
    #[serde(rename = "CL")]
    Cloze,
    #[serde(rename = "CLOZE")]
    ClozeLong,
}

impl NoteType {
    /// Expected pipe-delimited column count for this note type.
    #[must_use]
    pub const fn expected_columns(self) -> usize {
        match self {
            Self::Qa => 7,
            Self::Cloze | Self::ClozeLong => 6,
        }
    }
}

/// Diagnostic severity levels matching Python `Severity(StrEnum)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    Critical,
    Warn,
    Info,
    Log,
}

/// Overall validation verdict matching Python `ValidationResult(StrEnum)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ValidationResult {
    Pass,
    Fail,
    Incomplete,
}

/// Validation policy profile (ADR-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityProfile {
    LegacyCompat,
    #[default]
    StrictV2,
}

/// Typed identifier newtype for decks.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeckId(pub String);

/// Typed identifier newtype for batches (e.g. `B01`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BatchId(pub String);

/// Result of counting QA + expanded Cloze cards in a batch (`models.BatchCount`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchCount {
    pub qa_cards: usize,
    pub cloze_notes: usize,
    pub cloze_indices: usize,
    pub total: usize,
    pub within_limit: bool,
    pub excess: usize,
    pub ext_batch_suggested: Option<String>,
}

/// Maximum Anki cards per batch in the legacy operational policy.
pub const MAX_BATCH: usize = 20;

/// Legacy v1 receipt token (`sha256:` + 64 lowercase hex chars).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentDigest(pub String);

impl fmt::Display for ContentDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Normalize line endings to canonical LF (`\n`), matching `core/receipt.py::canonical_csv_text`.
///
/// Replaces `\r\n` with `\n`, then strips trailing `\r` from each line.
#[must_use]
pub fn canonical_csv_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .split('\n')
        .map(|line| line.trim_end_matches('\r'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Compute the legacy v1 universal receipt hash (`sha256:<64 hex>`), byte-for-byte
/// identical to `core/receipt.py::compute_receipt`.
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

/// Verify a v1 receipt hash against QA and Cloze CSV inputs.
#[must_use]
pub fn verify_receipt_v1(receipt_hash: &str, qa_csv: &str, cloze_csv: &str) -> bool {
    compute_receipt_v1(qa_csv, cloze_csv).0 == receipt_hash
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
        assert!(digest.0.starts_with("sha256:"));
        assert_eq!(digest.0.len(), 71);
        assert!(verify_receipt_v1(&digest.0, qa_crlf, cl_crlf));
    }

    #[test]
    fn canonical_text_strips_trailing_cr_per_line() {
        assert_eq!(canonical_csv_text("a\r\r\nb\r\nc\r"), "a\nb\nc");
    }
}
