use std::collections::HashSet;
use std::sync::LazyLock;

use anki_domain::{CsvRowNumber, Diagnostic, DiagnosticCode, Severity};
use regex::Regex;

pub const CODE_TAG_FORMAT: DiagnosticCode = DiagnosticCode::from_static("TAG.FORMAT");
pub const CODE_TAG_DEPTH: DiagnosticCode = DiagnosticCode::from_static("TAG.DEPTH");
pub const CODE_TAG_UNREGISTERED: DiagnosticCode = DiagnosticCode::from_static("TAG.UNREGISTERED");

const OWNER_SKILL: &str = "anki-registry";
const SEPARATOR: &str = "::";

// Latin-1 alphanumeric + underscore, explicitly rejecting multiplication sign (×, \u{00D7})
// and division sign (÷, \u{00F7}) that fall inside the naive \u{00C0}-\u{00FF} range.
static VALID_TAG_SEGMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_À-ÖØ-öø-ÿ]+$").expect("valid tag segment regex"));

fn depth(tag: &str) -> usize {
    tag.matches(SEPARATOR).count() + 1
}

/// Normaliza texto PT-BR para minúsculas sem acentos.
///
/// Delega para [`crate::text_norm::fold`], a primitiva única de dobra do crate (Onda 2, B24).
/// Compartilhado por `qa_clinical` e `interference` (heurísticas Wave 2).
#[must_use]
pub fn normalize_ptbr(s: &str) -> String {
    crate::text_norm::fold(s)
}

fn attach_location(mut diag: Diagnostic, row: Option<usize>, field: &str) -> Diagnostic {
    if let Some(r) = row {
        if let Some(row_num) = CsvRowNumber::new(r) {
            diag = diag.with_row(row_num);
        } else {
            diag = diag.with_row(r);
        }
    }
    diag.with_field(field)
}

#[must_use]
pub fn check_tags(
    tags_field: &str,
    taxonomy_nodes: Option<&HashSet<String>>,
    row: Option<usize>,
    field: &str,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    if tags_field.trim().is_empty() {
        return diags;
    }

    for tag in tags_field.split_whitespace() {
        if tag.starts_with("id::") {
            continue;
        }

        let is_valid = tag
            .split(SEPARATOR)
            .all(|seg| !seg.is_empty() && VALID_TAG_SEGMENT.is_match(seg));
        if !is_valid {
            let diag = Diagnostic::new(
                CODE_TAG_FORMAT,
                Severity::Critical,
                OWNER_SKILL,
                format!("Tag '{tag}' contains invalid characters"),
            )
            .with_suggestion("Use only letters, digits, underscores, and :: as separator");
            diags.push(attach_location(diag, row, field));
        }

        let tag_depth = depth(tag);
        if tag_depth > 4 {
            let diag = Diagnostic::new(
                CODE_TAG_DEPTH,
                Severity::Critical,
                OWNER_SKILL,
                format!("Tag '{tag}' has depth {tag_depth} (maximum is 4 levels)"),
            );
            diags.push(attach_location(diag, row, field));
        }

        if let Some(nodes) = taxonomy_nodes
            && !nodes.contains(tag)
        {
            let diag = Diagnostic::new(
                CODE_TAG_UNREGISTERED,
                Severity::Critical,
                OWNER_SKILL,
                format!("Tag '{tag}' is not registered in the Manifesto taxonomy"),
            )
            .with_suggestion("Register the tag first, or check for typos");
            diags.push(attach_location(diag, row, field));
        }
    }

    diags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_whitespace_tags_produce_no_diagnostics() {
        assert!(check_tags("", None, Some(1), "Etiquetas").is_empty());
        assert!(check_tags("   \t\n  ", None, Some(1), "Etiquetas").is_empty());
    }

    #[test]
    fn id_tags_are_exempt_from_all_checks() {
        let taxonomy = HashSet::from(["Medicina::Cardiologia".to_string()]);
        let diags = check_tags(
            "id::CARD-001::extra::deep::level Medicina::Cardiologia",
            Some(&taxonomy),
            Some(2),
            "Etiquetas",
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn valid_latin1_tags_pass_with_and_without_taxonomy() {
        let taxonomy = HashSet::from(["Educação::Física_1::À_ÿ".to_string()]);
        assert!(check_tags("Educação::Física_1::À_ÿ", None, None, "Etiquetas").is_empty());
        assert!(
            check_tags(
                "Educação::Física_1::À_ÿ",
                Some(&taxonomy),
                Some(3),
                "Etiquetas"
            )
            .is_empty()
        );
    }

    #[test]
    fn rejects_empty_hierarchy_segments() {
        for bad in &["::", "::Medicina", "Medicina::", "A::::B"] {
            let diags = check_tags(bad, None, Some(2), "Etiquetas");
            assert!(
                diags.iter().any(|d| d.code == CODE_TAG_FORMAT),
                "Tag '{bad}' should be rejected for having empty hierarchy segments"
            );
        }
    }

    #[test]
    fn rejects_multiplication_and_division_glyphs() {
        for bad in &["Medicina×Cirurgia", "Dose÷2", "A::B×C", "X÷Y::Z"] {
            let diags = check_tags(bad, None, Some(2), "Etiquetas");
            assert!(
                diags.iter().any(|d| d.code == CODE_TAG_FORMAT),
                "Tag '{bad}' should be rejected for containing × or ÷"
            );
        }
    }

    #[test]
    fn accepts_all_portuguese_diacritics() {
        let tag = "Atenção::Ação::Órgão::Saúde";
        let diags = check_tags(tag, None, Some(2), "Etiquetas");
        assert!(
            diags.is_empty(),
            "Tag with Portuguese diacritics should pass: {diags:?}"
        );
    }

    #[test]
    fn detects_invalid_chars_depth_and_unregistered() {
        let taxonomy = HashSet::from(["A::B".to_string()]);
        let diags = check_tags(
            "bad-tag! A::B::C::D::E",
            Some(&taxonomy),
            Some(5),
            "Etiquetas",
        );
        assert_eq!(diags.len(), 4);

        assert_eq!(diags[0].code, "TAG.FORMAT");
        assert_eq!(diags[0].severity, Severity::Critical);
        assert_eq!(diags[0].owner_skill, "anki-registry");
        assert_eq!(diags[0].row, Some(5));
        assert_eq!(diags[0].field.as_deref(), Some("Etiquetas"));
        assert!(diags[0].suggestion.is_some());

        assert_eq!(diags[1].code, "TAG.UNREGISTERED");
        assert_eq!(diags[1].severity, Severity::Critical);

        assert_eq!(diags[2].code, "TAG.DEPTH");
        assert_eq!(diags[2].severity, Severity::Critical);
        assert_eq!(diags[2].suggestion, None);

        assert_eq!(diags[3].code, "TAG.UNREGISTERED");
    }
}
