use regex::Regex;
use std::sync::LazyLock;

static LEAKED_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\((?:\s*(?:(?:[A-Z]{2,4}-)?B\d{2,}-(?:QA|CL|CLOZE)-\d{2,4}|[A-Z]{2,4}-B\d{2,}-\d{2,4}|ANKI-\d+)\s*)\)|\[(?:\s*(?:(?:[A-Z]{2,4}-)?B\d{2,}-(?:QA|CL|CLOZE)-\d{2,4}|[A-Z]{2,4}-B\d{2,}-\d{2,4}|ANKI-\d+)\s*)\]|\b(?:[A-Z]{2,4}-)?B\d{2,}-(?:QA|CL|CLOZE)-\d{2,4}\b|\b[A-Z]{2,4}-B\d{2,}-\d{2,4}\b|\bANKI-\d+\b",
    )
    .expect("valid leaked ID regex")
});

static LEADING_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*[\(\[]?\s*(?:ANKI-\d+|(?:[A-Z]{2,4}-)?B\d{2,}(?:-[A-Z0-9]+)?-\d{2,4}|B\d+-[A-Z]+-\d+)\b\s*[\)\]]?",
    )
    .expect("valid leading id regex")
});

static LEADING_ORPHAN_PUNCT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*[:,\-–—;.]+\s*").expect("valid leading orphan punct regex"));

static TRAILING_ORPHAN_PUNCT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*[:,\-–—;]+\s*$").expect("valid trailing orphan punct regex"));

static SPACE_BEFORE_PUNCT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+([,;:?.!])").expect("valid space before punct regex"));

static MULTI_SPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r" {2,}").expect("valid multi-space regex"));

/// Check if text starts with a leaked card ID (e.g. `ANKI-123`, `B01-QA-01`, `[ANKI-1]`, `(B02-CL-03)`).
#[must_use]
pub fn has_leading_id_leak(text: &str) -> bool {
    LEADING_ID_RE.is_match(text)
}

/// Strip leaked card IDs and clean up orphaned punctuation and whitespace.
pub fn purge_id_leak(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let cleaned = LEAKED_ID_RE.replace_all(text, "");
    let no_orphan_lead = LEADING_ORPHAN_PUNCT_RE.replace(&cleaned, "");
    let no_orphan_trail = TRAILING_ORPHAN_PUNCT_RE.replace(&no_orphan_lead, "");
    let spaced = SPACE_BEFORE_PUNCT_RE.replace_all(&no_orphan_trail, "$1");
    MULTI_SPACE_RE.replace_all(&spaced, " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{has_leading_id_leak, purge_id_leak};

    #[test]
    fn empty_and_whitespace_inputs() {
        assert_eq!(purge_id_leak(""), "");
        assert_eq!(purge_id_leak("   "), "");
        assert!(!has_leading_id_leak(""));
        assert!(!has_leading_id_leak("   "));
    }

    #[test]
    fn strips_leaked_ids_and_collapses_spaces() {
        assert_eq!(
            purge_id_leak("Card B01-QA-01 sobre coração"),
            "Card sobre coração"
        );
        assert_eq!(
            purge_id_leak("of-b01-001  Resumo b02-cl-03 e ABCD-B12-CLOZE-123 "),
            "Resumo e"
        );
    }

    #[test]
    fn strips_anki_ids_and_enclosing_brackets_and_orphan_punct() {
        assert_eq!(
            purge_id_leak("ANKI-123: Qual o tratamento do choque?"),
            "Qual o tratamento do choque?"
        );
        assert_eq!(
            purge_id_leak("[B01-QA-01] - Tratamento inicial"),
            "Tratamento inicial"
        );
        assert_eq!(
            purge_id_leak("(ANKI-101) No manejo da hipertensão"),
            "No manejo da hipertensão"
        );
        assert_eq!(
            purge_id_leak("Manejo da crise (B01-QA-01) , segundo protocolo"),
            "Manejo da crise, segundo protocolo"
        );
        assert_eq!(
            purge_id_leak("Qual a conduta? - B01-QA-01"),
            "Qual a conduta?"
        );
        assert_eq!(
            purge_id_leak("Qual a conduta? : [ANKI-42]"),
            "Qual a conduta?"
        );
    }

    #[test]
    fn detects_leading_id_leak() {
        assert!(has_leading_id_leak("ANKI-123: Qual o tratamento?"));
        assert!(has_leading_id_leak("[B01-QA-01]: Qual o tratamento?"));
        assert!(has_leading_id_leak("(ANKI-101) No manejo..."));
        assert!(has_leading_id_leak("B01-QA-005 Qual o achado?"));
        assert!(has_leading_id_leak("B12-CLOZE-123 Qual o achado?"));
        assert!(has_leading_id_leak("OF-B01-001 Resumo"));
        assert!(has_leading_id_leak("b01-qa-01 algo"));

        assert!(!has_leading_id_leak("Qual o tratamento? ANKI-123"));
        assert!(!has_leading_id_leak("Qual o tratamento? [B01-QA-01]"));
        assert!(!has_leading_id_leak("Sem IDs: XB01-QA-01"));
    }

    #[test]
    fn negative_and_word_boundary_cases() {
        assert_eq!(
            purge_id_leak("Sem IDs: XB01-QA-01, B01-QA-01X, A-B01-001"),
            "Sem IDs: XB01-QA-01, B01-QA-01X, A-B01-001"
        );
    }
}
