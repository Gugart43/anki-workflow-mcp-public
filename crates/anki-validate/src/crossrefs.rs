use std::collections::HashSet;
use std::sync::LazyLock;

use anki_domain::{Diagnostic, Severity};
use regex::Regex;

static XREF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(B\d{2}-(?:QA|CL)-\d{3}[a-z]?)\b").expect("valid xref regex"));

#[must_use]
pub fn check_crossrefs(
    text: &str,
    active_ids: Option<&HashSet<String>>,
    current_batch_ids: Option<&HashSet<String>>,
    row: Option<usize>,
    field: Option<&str>,
) -> Vec<Diagnostic> {
    if active_ids.is_none() && current_batch_ids.is_none() {
        return Vec::new();
    }

    let mut diags = Vec::new();

    for caps in XREF_RE.captures_iter(text) {
        let Some(m) = caps.get(1) else {
            continue;
        };
        let ref_id = m.as_str();

        let in_active = active_ids.is_some_and(|ids| ids.contains(ref_id));
        let in_current = current_batch_ids.is_some_and(|ids| ids.contains(ref_id));

        if !in_active && !in_current {
            let mut diag = Diagnostic::new(
                anki_domain::DiagnosticCode::from_static("XREF.BROKEN"),
                Severity::Critical,
                "anki-export-validator",
                format!(
                    "Cross-reference '{ref_id}' does not resolve to an active or current-batch ID"
                ),
            )
            .with_suggestion(
                "Check the ID is active in the Manifesto or being introduced in this batch",
            );

            if let Some(r) = row {
                diag = diag.with_row(r);
            }
            if let Some(f) = field {
                diag = diag.with_field(f);
            }

            diags.push(diag);
        }
    }

    diags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_no_ledger_return_empty() {
        let active = HashSet::from(["B01-QA-001".to_string()]);
        assert!(check_crossrefs("", Some(&active), None, None, None).is_empty());
        assert!(check_crossrefs("See [B01-QA-999]", None, None, Some(1), Some("Notas")).is_empty());
    }

    #[test]
    fn resolves_active_and_current_batch_ids() {
        let active = HashSet::from(["B01-QA-001".to_string()]);
        let current = HashSet::from(["B01-CL-003a".to_string()]);
        let text = "Compare [B01-QA-001] with (B01-CL-003a).";
        let diags = check_crossrefs(
            text,
            Some(&active),
            Some(&current),
            Some(2),
            Some("Resposta"),
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn broken_and_duplicate_crossrefs_are_reported() {
        let active = HashSet::from(["B01-QA-001".to_string()]);
        let text = "See [B01-QA-999] and again B01-QA-999.";
        let diags = check_crossrefs(text, Some(&active), None, Some(4), Some("Enunciado"));
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0].code, "XREF.BROKEN");
        assert_eq!(diags[0].severity, Severity::Critical);
        assert_eq!(diags[0].row, Some(4));
        assert_eq!(diags[0].field.as_deref(), Some("Enunciado"));
        assert!(diags[0].suggestion.is_some());
    }

    #[test]
    fn case_sensitive_and_word_boundary() {
        let active = HashSet::new();
        let text = "lowercase b01-qa-001 and embedded XB01-QA-001Y should not match";
        let diags = check_crossrefs(text, Some(&active), None, None, None);
        assert!(diags.is_empty());
    }
}
