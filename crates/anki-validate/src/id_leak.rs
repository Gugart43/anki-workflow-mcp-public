use regex::Regex;
use std::sync::LazyLock;

static LEAKED_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:[A-Z]{2,4}-)?B\d{2}-(?:QA|CL|CLOZE)-\d{2,3}\b|\b[A-Z]{2,4}-B\d{2}-\d{3}\b",
    )
    .expect("valid leaked ID regex")
});

static MULTI_SPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r" {2,}").expect("valid multi-space regex"));

pub fn purge_id_leak(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let cleaned = LEAKED_ID_RE.replace_all(text, "");
    MULTI_SPACE_RE.replace_all(&cleaned, " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::purge_id_leak;

    #[test]
    fn empty_and_whitespace_inputs() {
        assert_eq!(purge_id_leak(""), "");
        assert_eq!(purge_id_leak("   "), "");
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
    fn negative_and_word_boundary_cases() {
        assert_eq!(
            purge_id_leak("Sem IDs: XB01-QA-01, B01-QA-01X, A-B01-001"),
            "Sem IDs: XB01-QA-01, B01-QA-01X, A-B01-001"
        );
    }
}
