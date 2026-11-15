//! Cloze hint validation rules and heuristics (`checks/hints.py`).

use anki_domain::{Diagnostic, Severity};
use anki_parser::find_cloze_spans;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// Common medical/Portuguese modifiers that should NOT trigger `HINT.ROOT_LEAK`.
///
/// These represent shared clinical categories rather than leaked content words.
pub const COMMON_MODIFIERS: &[&str] = &[
    "hepatica",
    "hepaticas",
    "serica",
    "sericas",
    "sistemica",
    "sistemicas",
    "cronica",
    "cronico",
    "aguda",
    "agudo",
    "primaria",
    "primario",
    "secundaria",
    "secundario",
    "severa",
    "grave",
    "monitorada",
    "monitoradas",
    "monitorado",
];

static ELIMINATION_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(ou|or|e/ou|and/or)\b.{0,20}\b(sim|não|yes|no|masculino|feminino|male|female|ieca|bra|bcc|ara|aines|aas)\b|\b(sim|não|yes|no|masculino|feminino|male|female)\b.{0,20}\b(ou|or)\b",
    )
    .expect("valid elimination regex")
});

static ALT_SLASH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b([A-Z]{2,6})\s*/\s*([A-Z]{2,6})\b").expect("valid alt_slash regex")
});

/// Pure Rust ASCII/Latin diacritic folding for NFD-like normalization without external dependencies.
///
/// Strips combining marks (Unicode `Mn`) and maps accented Latin characters to their ASCII bases.
fn fold_latin_char(c: char) -> Option<char> {
    match c {
        'a'..='z' | '0'..='9' | ' ' => Some(c),
        // Portuguese & Western European vowels with acute, grave, circumflex, tilde, umlaut/diaeresis, ring
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => Some('a'),
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => Some('c'),
        'ď' | 'đ' => Some('d'),
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => Some('e'),
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => Some('g'),
        'ĥ' | 'ħ' => Some('h'),
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => Some('i'),
        'ĵ' => Some('j'),
        'ķ' | 'ĸ' => Some('k'),
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => Some('l'),
        'ñ' | 'ń' | 'ņ' | 'ň' | 'ŉ' | 'ŋ' => Some('n'),
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => Some('o'),
        'ŕ' | 'ŗ' | 'ř' => Some('r'),
        'ś' | 'ŝ' | 'ş' | 'š' => Some('s'),
        'ţ' | 'ť' | 'ŧ' => Some('t'),
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => Some('u'),
        'ŵ' => Some('w'),
        'ý' | 'ÿ' | 'ŷ' => Some('y'),
        'ź' | 'ż' | 'ž' => Some('z'),
        // Unicode Combining Diacritical Marks (Mn: U+0300..=U+036F) are dropped
        '\u{0300}'..='\u{036F}' => None,
        // Any other non-alphanumeric punctuation/symbol maps to space
        _ => Some(' '),
    }
}

/// Normalize text for semantic token comparison: lowercase, strip accents, and remove punctuation.
///
/// Matches Python `checks/hints.py::_normalize(text)`:
/// `unicodedata.normalize("NFD", text.lower())` stripped of `Mn` marks and non-`[a-z0-9 ]`.
#[must_use]
pub fn normalize_hint_text(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    for ch in text.chars() {
        for lower_ch in ch.to_lowercase() {
            if let Some(mapped) = fold_latin_char(lower_ch) {
                folded.push(mapped);
            }
        }
    }
    let words: Vec<&str> = folded.split_whitespace().collect();
    words.join(" ")
}

/// Calculate Jaccard token overlap between two strings after normalization.
#[must_use]
pub fn token_overlap_ratio(a: &str, b: &str) -> f64 {
    let norm_a = normalize_hint_text(a);
    let norm_b = normalize_hint_text(b);
    let ta: HashSet<&str> = norm_a.split_whitespace().collect();
    let tb: HashSet<&str> = norm_b.split_whitespace().collect();
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let intersection_count = ta.intersection(&tb).count();
    let union_count = ta.union(&tb).count();
    if union_count == 0 {
        0.0
    } else {
        intersection_count as f64 / union_count as f64
    }
}

/// Check if `hint` literally contains a content word (≥5 characters) from `target`.
///
/// Excludes common medical/anatomical modifiers (`COMMON_MODIFIERS`).
#[must_use]
pub fn has_root_leak(target: &str, hint: &str) -> bool {
    let nt = normalize_hint_text(target);
    let nh = normalize_hint_text(hint);

    let target_tokens: HashSet<&str> = nt.split_whitespace().collect();
    let hint_tokens: HashSet<&str> = nh.split_whitespace().collect();
    let modifier_set: HashSet<&str> = COMMON_MODIFIERS.iter().copied().collect();

    let leaked: HashSet<&str> = target_tokens
        .intersection(&hint_tokens)
        .copied()
        .filter(|w| !modifier_set.contains(w) && w.chars().count() >= 5)
        .collect();

    !leaked.is_empty()
}

fn make_hint_diag(
    code: &'static str,
    message: impl Into<String>,
    row: Option<usize>,
    field: &str,
    severity: Severity,
    suggestion: Option<String>,
) -> Diagnostic {
    let mut diag = Diagnostic::new(code, severity, "anki-cloze-hints", message).with_field(field);
    if let Some(r) = row {
        diag = diag.with_row(r);
    }
    if let Some(s) = suggestion {
        diag = diag.with_suggestion(s);
    }
    diag
}

/// Validate Cloze hints in a sentence against quality and leakage rules (`checks/hints.py`).
///
/// Evaluates:
/// - `HINT.FORMAT`: Empty hint (`{{cN::target::}}`)
/// - `HINT.ROOT_LEAK`: Verbatim root word of target leaked in hint
/// - `HINT.SYNONYM`: High token overlap (> 50%) indicating a direct synonym
/// - `HINT.ELIMINATION`: Binary/small-category alternatives (e.g. `ou`, `/`)
#[must_use]
pub fn check_hints(sentence: &str, row: Option<usize>, field: Option<&str>) -> Vec<Diagnostic> {
    let field_name = field.unwrap_or("Sentença");
    let mut diags = Vec::new();
    let spans = find_cloze_spans(sentence);

    for span in spans {
        let Some(raw_hint) = span.hint else {
            continue;
        };

        let cn = span.index;
        let target = span.target.trim();
        let hint = raw_hint.trim();

        if hint.is_empty() {
            diags.push(make_hint_diag(
                "HINT.FORMAT",
                format!(
                    "c{cn}: hint is empty (either provide a valid hint or remove the :: separator)"
                ),
                row,
                field_name,
                Severity::Critical,
                None,
            ));
            continue;
        }

        // HINT.ROOT_LEAK
        if has_root_leak(target, hint) {
            diags.push(make_hint_diag(
                "HINT.ROOT_LEAK",
                format!("c{cn}: hint '{hint}' appears to contain a root/derivative of target '{target}'"),
                row,
                field_name,
                Severity::Critical,
                Some("Use a category label (e.g. 'classe farmacológica') not a derivative of the term".to_owned()),
            ));
        }

        // HINT.SYNONYM
        let overlap = token_overlap_ratio(target, hint);
        if overlap > 0.5 {
            diags.push(make_hint_diag(
                "HINT.SYNONYM",
                format!(
                    "c{cn}: hint '{hint}' has high token overlap ({:.0}%) with target '{target}' — likely a synonym",
                    overlap * 100.0
                ),
                row,
                field_name,
                Severity::Critical,
                Some("The hint must state the *type* of entity, not what it looks like".to_owned()),
            ));
        }

        // HINT.ELIMINATION
        if ELIMINATION_PATTERN.is_match(hint) || ALT_SLASH.is_match(hint) {
            diags.push(make_hint_diag(
                "HINT.ELIMINATION",
                format!("c{cn}: hint '{hint}' may enable binary/small-category elimination"),
                row,
                field_name,
                Severity::Warn,
                Some(
                    "Avoid alternatives (A ou B / A/B) in hints — use a broad category instead"
                        .to_owned(),
                ),
            ));
        }
    }

    diags
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_code(diags: &[Diagnostic], code: &str) -> bool {
        diags.iter().any(|d| d.code == code)
    }

    fn worst_severity(diags: &[Diagnostic]) -> &'static str {
        if diags.iter().any(|d| d.severity == Severity::Critical) {
            "FAIL"
        } else if diags.iter().any(|d| d.severity == Severity::Warn) {
            "WARN"
        } else {
            "PASS"
        }
    }

    #[test]
    fn test_normalization_and_folding() {
        assert_eq!(normalize_hint_text("DOR DE CABEÇA!"), "dor de cabeca");
        assert_eq!(
            normalize_hint_text("janela terapêutica"),
            "janela terapeutica"
        );
        assert_eq!(
            normalize_hint_text("doença da placa motora autoimune com miastenia"),
            "doenca da placa motora autoimune com miastenia"
        );
        assert_eq!(
            normalize_hint_text("  espaços   múltiplos  "),
            "espacos multiplos"
        );
    }

    #[test]
    fn test_token_overlap() {
        let t = "insuficiência cardíaca";
        let h = "insuficiência cardíaca congestiva";
        let overlap = token_overlap_ratio(t, h);
        // intersection: 2 ("insuficiencia", "cardiaca"), union: 3 ("insuficiencia", "cardiaca", "congestiva")
        assert!((overlap - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn test_empty_hint_format_fails() {
        let s = "O paciente tem {{c1::hipertensão::}} essencial.";
        let diags = check_hints(s, None, None);
        assert!(has_code(&diags, "HINT.FORMAT"));
        assert_eq!(worst_severity(&diags), "FAIL");

        let s_spaces = "O paciente tem {{c1::hipertensão::   }} essencial.";
        let diags_spaces = check_hints(s_spaces, None, None);
        assert!(has_code(&diags_spaces, "HINT.FORMAT"));
    }

    #[test]
    fn test_root_leak_detected() {
        let s = "O diagnóstico é {{c1::miastenia gravis::doença da placa motora autoimune com miastenia}}.";
        let diags = check_hints(s, None, None);
        assert!(has_code(&diags, "HINT.ROOT_LEAK"));
        assert_eq!(worst_severity(&diags), "FAIL");
    }

    #[test]
    fn test_root_leak_ignores_common_modifiers() {
        let s = "Exames laboratoriais revelam alteração de {{c1::transaminases hepáticas::enzimas hepáticas monitoradas}}.";
        let diags = check_hints(s, None, None);
        assert!(!has_code(&diags, "HINT.ROOT_LEAK"));
    }

    #[test]
    fn test_synonym_detected_on_high_overlap() {
        let s = "O achado é {{c1::insuficiência cardíaca congestiva::insuficiência cardíaca}} descompensada.";
        let diags = check_hints(s, None, None);
        assert!(has_code(&diags, "HINT.SYNONYM"));
    }

    #[test]
    fn test_elimination_alternation_pattern() {
        let s = "O anti-hipertensivo de escolha é {{c1::IECA::IECA ou BRA}}.";
        let diags = check_hints(s, None, None);
        assert!(has_code(&diags, "HINT.ELIMINATION"));
        assert_eq!(worst_severity(&diags), "WARN");
    }

    #[test]
    fn test_elimination_slash_pattern() {
        let s = "Prescrever terapia com {{c1::inibidor::IECA/BRA}} em dose baixa.";
        let diags = check_hints(s, None, None);
        assert!(has_code(&diags, "HINT.ELIMINATION"));
    }

    #[test]
    fn test_golden_cases() {
        // Known limitation: token-based checker cannot detect cross-language synonyms without root overlap
        let s_synonym_limit = "O paciente refere {{c1::cefaleia::dor de cabeça}} intensa.";
        let d_synonym = check_hints(s_synonym_limit, None, None);
        assert_eq!(worst_severity(&d_synonym), "PASS");

        // Sex binary elimination
        let s_sex = "O predomínio é no sexo {{c1::masculino::masculino ou feminino}}.";
        let d_sex = check_hints(s_sex, None, None);
        assert!(has_code(&d_sex, "HINT.ROOT_LEAK"));
        assert!(has_code(&d_sex, "HINT.ELIMINATION"));
        assert_eq!(worst_severity(&d_sex), "FAIL");

        // Approved hint: category label
        let s_good_cat =
            "A droga de escolha é {{c1::Pregabalina::gabapentinoide / ligante alfa-2-delta}}.";
        let d_good_cat = check_hints(s_good_cat, None, None);
        assert_eq!(worst_severity(&d_good_cat), "PASS");

        // Approved hint: temporal window
        let s_good_temp = "A janela terapêutica do rTPA é {{c1::4,5 horas::janela terapêutica}}.";
        let d_good_temp = check_hints(s_good_temp, None, None);
        assert_eq!(worst_severity(&d_good_temp), "PASS");

        // Approved hint: no hint at all
        let s_no_hint = "A droga de escolha para intubação em sequência rápida na suspeita de hipertensão intracraniana é o {{c1::etomidato}} por sua estabilidade hemodinâmica.";
        let d_no_hint = check_hints(s_no_hint, None, None);
        assert_eq!(worst_severity(&d_no_hint), "PASS");
    }
}
