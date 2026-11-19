//! Deterministic notes field checks (`checks/notes.py`).
//!
//! Checks:
//! - `NOTE.METADATA` — forbidden metadata headers (ID:, Âncora:, Cross-ref:, Relevância:) (CRITICAL)
//! - `NOTE.DUP_ANSWER_WARN` — notes verbatim-repeat the answer without pedagogical explanation (WARN)
//! - `NOTE.EXAM_ONLY_WARN` — notes contain only exam-appearance mention without explanation (WARN)
//! - `NOTE.LENGTH_WARN` — notes field outside recommended length (WARN, configurable)
//! - `NOTE.STYLE_WARN` — list items not using `• <b>…</b>` pattern (WARN)

use std::sync::LazyLock;

use anki_domain::{Diagnostic, Severity};
use regex::Regex;

// Re-exports preservam 100% das assinaturas públicas da Fase 1.
pub use crate::domain_profile::{DomainProfile, DomainProfileError};
pub use crate::html_text::{
    html_to_visible_text, normalize_for_exact_match, parse_html_text, strip_table_blocks,
    visible_char_count,
};
pub use crate::note_metrics::{
    NotesStructureMetrics, check_cloze_target_restatement, compute_notes_metrics,
    is_exact_answer_restatement,
};

pub const DEFAULT_MAX_LENGTH: usize = 1200;

const OWNER: &str = "anki-notes-enricher";
const DEFAULT_FIELD: &str = "Notas Adicionais";

static METADATA_HEADERS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?im)^\s*(ID\s*:|Âncora\s*:|Anchor\s*:|Cross-ref\s*:|Cross\s+ref\s*:|Relevância\s*:|Relevancia\s*:)",
    )
    .expect("valid METADATA_HEADERS_RE")
});

static HAS_TABLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<table\b").expect("valid HAS_TABLE_RE"));

static BULLET_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|<br\s*/?>|\n|<div[^>]*>|<p[^>]*>)\s*(?:[•▪▸►]|[-*]\s)\s*")
        .expect("valid BULLET_LINE_RE")
});

static LI_BLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<li\b[^>]*>(.*?)</li>").expect("valid LI_BLOCK_RE"));

static BOLD_OPEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)^(?:<(?:b|strong)\b|<span\b[^>]*\bclass\s*=\s*["'][^"']*\brot\b[^"']*["'])"#)
        .expect("valid BOLD_OPEN_RE")
});

#[must_use]
pub fn is_exam_only_note(notes: &str) -> bool {
    is_exam_only_note_with_profile(notes, DomainProfile::default_profile())
}

#[must_use]
pub fn is_exam_only_note_with_profile(notes: &str, profile: &DomainProfile) -> bool {
    profile.is_exam_only_note(notes)
}

#[must_use]
pub fn check_notes(notes: &str, answer: &str, row: Option<usize>) -> Vec<Diagnostic> {
    check_notes_ext(notes, answer, row, DEFAULT_FIELD, DEFAULT_MAX_LENGTH)
}

#[must_use]
pub fn check_notes_ext(
    notes: &str,
    answer: &str,
    row: Option<usize>,
    field: &str,
    max_length: usize,
) -> Vec<Diagnostic> {
    check_notes_with_profile(
        notes,
        answer,
        row,
        field,
        Some(max_length),
        DomainProfile::default_profile(),
    )
}

#[must_use]
pub fn check_notes_with_profile(
    notes: &str,
    answer: &str,
    row: Option<usize>,
    field: &str,
    max_length: Option<usize>,
    profile: &DomainProfile,
) -> Vec<Diagnostic> {
    if notes.trim().is_empty() {
        return Vec::new();
    }

    let mut diags = Vec::new();
    // Preserva quebras de linha (\n) para que (?im)^ capture cabeçalhos após <br>/<p>/<li>.
    let (block_text, _, _) = parse_html_text(notes);

    // 1. NOTE.METADATA (respeita InfoPolicy::forbid_metadata_headers)
    if profile.info_policy.forbid_metadata_headers {
        for m in METADATA_HEADERS_RE.find_iter(&block_text) {
            diags.push(build_diagnostic(
                "NOTE.METADATA",
                Severity::Critical,
                format!("Forbidden metadata header found: '{}'", m.as_str().trim()),
                row,
                field,
                Some("Remove metadata — Notes field is for pedagogical explanations only"),
            ));
        }
    }

    // 2. NOTE.DUP_ANSWER_WARN (respeita InfoPolicy::forbid_verbatim_answer_restatement)
    if profile.info_policy.forbid_verbatim_answer_restatement
        && !answer.trim().is_empty()
        && is_exact_answer_restatement(notes, answer)
    {
        diags.push(build_diagnostic(
            "NOTE.DUP_ANSWER_WARN",
            Severity::Warn,
            "Notes repeat the answer verbatim — deepen the explanation instead",
            row,
            field,
            Some("Explain *why* the answer is correct or add related nuance from the source"),
        ));
    }

    // 3. NOTE.EXAM_ONLY_WARN (respeita InfoPolicy::forbid_exam_only_mentions)
    if profile.info_policy.forbid_exam_only_mentions
        && is_exam_only_note_with_profile(notes, profile)
    {
        diags.push(build_diagnostic(
            "NOTE.EXAM_ONLY_WARN",
            Severity::Warn,
            "Notes contain only an exam mention without pedagogical explanation",
            row,
            field,
            Some("Menção isolada a prova não explica a unidade; acrescente explicação útil da fonte ou deixe Notas Adicionais vazio."),
        ));
    }

    // 4. NOTE.LENGTH_WARN
    let base_max = max_length.unwrap_or(profile.default_max_note_chars);
    let has_table = HAS_TABLE_RE.is_match(notes);
    let effective_max = if has_table {
        base_max.max(profile.table_max_note_chars)
    } else {
        base_max
    };
    let visible_length = visible_char_count(notes);
    if visible_length > effective_max {
        diags.push(build_diagnostic(
            "NOTE.LENGTH_WARN",
            Severity::Warn,
            format!(
                "Notes field is {visible_length} visible chars (recommended ≤ {effective_max}; advisory only)"
            ),
            row,
            field,
            None,
        ));
    }

    // 5. NOTE.STYLE_WARN
    if has_unbolded_list_item(notes) {
        let pref = &profile.info_policy.preferred_list_format;
        diags.push(build_diagnostic(
            "NOTE.STYLE_WARN",
            Severity::Warn,
            format!("List items found without <b>…</b> wrapper — preferred: '{pref}'"),
            row,
            field,
            None,
        ));
    }

    diags
}

fn has_unbolded_list_item(notes: &str) -> bool {
    let non_table = strip_table_blocks(notes);

    for m in BULLET_LINE_RE.find_iter(&non_table) {
        let rest = non_table[m.end()..].trim_start();
        if !rest.is_empty() && !BOLD_OPEN_RE.is_match(rest) {
            return true;
        }
    }

    for cap in LI_BLOCK_RE.captures_iter(&non_table) {
        let inner = cap[1]
            .trim_start()
            .trim_start_matches(['•', '▪', '▸', '►', '-', '*', ' ']);
        if !inner.is_empty() && !BOLD_OPEN_RE.is_match(inner) {
            return true;
        }
    }

    false
}

fn build_diagnostic(
    code: &'static str,
    severity: Severity,
    message: impl Into<String>,
    row: Option<usize>,
    field: &str,
    suggestion: Option<&str>,
) -> Diagnostic {
    let mut diag = Diagnostic::new(
        anki_domain::DiagnosticCode::from_static(code),
        severity,
        OWNER,
        message,
    )
    .with_field(field);
    if let Some(r) = row {
        diag = diag.with_row(r);
    }
    if let Some(s) = suggestion {
        diag = diag.with_suggestion(s);
    }
    diag
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_and_short_notes() {
        assert!(check_notes("", "", None).is_empty());
        assert!(check_notes("   ", "", None).is_empty());
        assert!(check_notes("Meia-vida longa.", "", None).is_empty());
        assert!(check_notes("Via renal.", "", None).is_empty());
    }

    #[test]
    fn test_forbidden_metadata_headers() {
        let d1 = check_notes("ID: B01-QA-001<br>Explicação detalhada.", "", Some(1));
        assert!(
            d1.iter()
                .any(|d| d.code == "NOTE.METADATA" && d.severity == Severity::Critical)
        );

        let d2 = check_notes("Âncora: p. 83<br>Texto explicativo.", "", None);
        assert!(d2.iter().any(|d| d.code == "NOTE.METADATA"));

        let d3 = check_notes("Cross-ref: B01-QA-002", "", None);
        assert!(d3.iter().any(|d| d.code == "NOTE.METADATA"));

        let d4 = check_notes("Relevância: Alta para provas", "", None);
        assert!(d4.iter().any(|d| d.code == "NOTE.METADATA"));
    }

    #[test]
    fn test_duplicate_answer_warn() {
        let d1 = check_notes("<b>Riluzol.</b>", "riluzol", None);
        assert!(
            d1.iter()
                .any(|d| d.code == "NOTE.DUP_ANSWER_WARN" && d.severity == Severity::Warn)
        );

        let d2 = check_notes("<p>&ge;&nbsp;140 mmHg!</p>", "<b>&ge; 140 mmHg</b>", None);
        assert!(d2.iter().any(|d| d.code == "NOTE.DUP_ANSWER_WARN"));

        let d3 = check_notes("A resposta correta é: IECA.", "<i>IECA</i>", None);
        assert!(d3.iter().any(|d| d.code == "NOTE.DUP_ANSWER_WARN"));

        // Does not warn when pedagogical explanation is present
        let d4 = check_notes(
            "<p><b>Riluzol</b> reduz a liberação de glutamato e prolonga moderadamente a sobrevida.</p>",
            "Riluzol",
            None,
        );
        assert!(!d4.iter().any(|d| d.code == "NOTE.DUP_ANSWER_WARN"));
    }

    #[test]
    fn test_exam_only_notes() {
        let cases = [
            "Já caiu em prova.",
            "Caiu na prova de 2024.",
            "<p>• <b>Prova:</b> Muito cobrado em provas de residência.</p>",
            "Caiu na prova da USP de 2024.",
            "Prova do ENARE 2024.",
        ];
        for txt in cases {
            assert!(is_exam_only_note(txt), "Should be exam-only: {txt}");
            let diags = check_notes(txt, "", None);
            assert!(
                diags
                    .iter()
                    .any(|d| d.code == "NOTE.EXAM_ONLY_WARN" && d.severity == Severity::Warn),
                "Missing NOTE.EXAM_ONLY_WARN on: {txt}"
            );
        }

        let non_exam_cases = [
            "Caiu na prova de 2024. A meia-vida longa explica o platô tardio.",
            "Tema cobrado em prova: segue cinética de ordem zero em doses terapêuticas.",
            "Prova calórica ausente indica arreflexia vestibular.",
            "Caiu na prova de 2024 por distinguir o grupo alfa do beta",
            "Caiu na prova de 2024 de mecanismo ainda desconhecido",
            "Caiu na prova de 2024 para distinguir o grupo alfa do beta",
            "Muito cobrado em provas na diferenciação entre crise focal e síncope",
            "Já caiu em prova em pacientes com hepatopatia crônica para evitar toxicidade",
        ];
        for txt in non_exam_cases {
            assert!(!is_exam_only_note(txt), "Should NOT be exam-only: {txt}");
            let diags = check_notes(txt, "", None);
            assert!(
                !diags.iter().any(|d| d.code == "NOTE.EXAM_ONLY_WARN"),
                "False positive NOTE.EXAM_ONLY_WARN on: {txt}"
            );
        }
    }

    #[test]
    fn test_length_warn() {
        let base = "Explicação clínica. ".repeat(60); // ~1200 chars
        let d_ok = check_notes(&base, "", None);
        assert!(!d_ok.iter().any(|d| d.code == "NOTE.LENGTH_WARN"));

        let huge = "Explicação clínica. ".repeat(300);
        let d_huge = check_notes(&huge, "", None);
        assert!(
            d_huge
                .iter()
                .any(|d| d.code == "NOTE.LENGTH_WARN" && d.severity == Severity::Warn)
        );
        assert!(!d_huge.iter().any(|d| d.severity == Severity::Critical));

        // Table with ~2000 chars: > 1200, but <= 2500 for tables -> no warning
        let table_ok = format!(
            "<table><tr><td>{}</td></tr></table>",
            "Explicação clínica. ".repeat(100)
        );
        let d_table_ok = check_notes(&table_ok, "", None);
        assert!(!d_table_ok.iter().any(|d| d.code == "NOTE.LENGTH_WARN"));

        // Table with ~3000 chars: > 2500 -> warning emitted (WARN, not CRITICAL)
        let table_huge = format!(
            "<table><tr><td>{}</td></tr></table>",
            "Explicação clínica. ".repeat(150)
        );
        let d_table_huge = check_notes(&table_huge, "", None);
        assert!(
            d_table_huge
                .iter()
                .any(|d| d.code == "NOTE.LENGTH_WARN" && d.severity == Severity::Warn)
        );
        assert!(
            !d_table_huge
                .iter()
                .any(|d| d.severity == Severity::Critical)
        );
    }

    #[test]
    fn test_style_warn() {
        let valid_style = "• <b>Avaliação prática:</b> Olhos abertos não garantem consciência.<br>• <b>Fisiopatologia:</b> Descarga paroxística.";
        let d_valid = check_notes(valid_style, "", None);
        assert!(!d_valid.iter().any(|d| d.code == "NOTE.STYLE_WARN"));

        let invalid_style = "• Avaliação prática: Olhos abertos não garantem consciência.";
        let d_invalid = check_notes(invalid_style, "", None);
        assert!(
            d_invalid
                .iter()
                .any(|d| d.code == "NOTE.STYLE_WARN" && d.severity == Severity::Warn)
        );

        let table_style = "<table><tr><td>• Item sem negrito</td></tr></table>";
        let d_table = check_notes(table_style, "", None);
        assert!(!d_table.iter().any(|d| d.code == "NOTE.STYLE_WARN"));
    }

    #[test]
    fn test_golden_notes_cases() {
        let good_note = "A atualização da ILAE aboliu os termos obsoletos 'parcial simples' e 'parcial complexa'.<br>• <b>Avaliação prática:</b> A manutenção de olhos abertos não garante consciência.<br>• <b>Fisiopatologia:</b> O comprometimento da consciência ocorre quando a descarga paroxística ultrapassa a zona cortical focal.";
        let d_good = check_notes(good_note, "Consciência preservada vs. comprometida", None);
        assert!(d_good.is_empty() || !d_good.iter().any(|d| d.severity == Severity::Critical));

        let bad_metadata = "ID: B01-QA-001<br>Âncora: p. 12<br>Explicação: ...";
        let d_meta = check_notes(bad_metadata, "", None);
        assert!(
            d_meta
                .iter()
                .any(|d| d.code == "NOTE.METADATA" && d.severity == Severity::Critical)
        );

        let good_mention = "Cefaleia intensa em salvas com rinorreia ipsilateral. A resposta correta é cefaleia em salvas.";
        let d_mention = check_notes(good_mention, "cefaleia em salvas", None);
        assert!(!d_mention.iter().any(|d| d.code == "NOTE.DUP_ANSWER_WARN"));

        let bad_verbatim = "A resposta correta é cefaleia em salvas.";
        let d_verbatim = check_notes(bad_verbatim, "cefaleia em salvas", None);
        assert!(
            d_verbatim
                .iter()
                .any(|d| d.code == "NOTE.DUP_ANSWER_WARN" && d.severity == Severity::Warn)
        );
    }

    #[test]
    fn test_policy_flags_honored() {
        let custom_json = r#"{
            "id": "lenient",
            "name": "Lenient Profile",
            "description": "desc",
            "language": "pt",
            "default_max_note_chars": 1200,
            "table_max_note_chars": 2500,
            "exam_boilerplate": { "enabled": true, "known_labels": ["USP"], "phrase_patterns": ["(?:USP)"] },
            "info_policy": {
                "source_fidelity": "ok",
                "notes_purpose": "ok",
                "forbid_metadata_headers": false,
                "forbid_verbatim_answer_restatement": false,
                "forbid_exam_only_mentions": false,
                "preferred_list_format": "• <b>Tópico:</b> ..."
            }
        }"#;

        let profile = DomainProfile::from_json_str(custom_json).expect("valid profile");

        // Metadata headers ignored under lenient policy
        let d_meta =
            check_notes_with_profile("ID: CARD-1", "Ans", None, DEFAULT_FIELD, None, &profile);
        assert!(!d_meta.iter().any(|d| d.code == "NOTE.METADATA"));

        // Verbatim restatement ignored under lenient policy
        let d_dup = check_notes_with_profile("Ans", "Ans", None, DEFAULT_FIELD, None, &profile);
        assert!(!d_dup.iter().any(|d| d.code == "NOTE.DUP_ANSWER_WARN"));

        // Exam only ignored under lenient policy
        let d_exam = check_notes_with_profile("USP", "Ans", None, DEFAULT_FIELD, None, &profile);
        assert!(!d_exam.iter().any(|d| d.code == "NOTE.EXAM_ONLY_WARN"));
    }

    #[test]
    fn test_general_profile_ignores_exam_mentions() {
        let general = DomainProfile::general();
        assert!(!is_exam_only_note_with_profile(
            "Cobrado na prova da USP 2023.",
            general
        ));
        let diags = check_notes_with_profile(
            "Cobrado na prova da USP 2023.",
            "Ans",
            None,
            DEFAULT_FIELD,
            None,
            general,
        );
        assert!(!diags.iter().any(|d| d.code == "NOTE.EXAM_ONLY_WARN"));
    }
}
