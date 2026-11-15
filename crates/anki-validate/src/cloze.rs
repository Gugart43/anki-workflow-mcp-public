//! Cloze deletion validation rules and heuristics (`checks/cloze.py`).

use anki_domain::{Diagnostic, Severity};
use anki_parser::{ClozeSpan, find_cloze_spans};
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// Maximum number of distinct Cloze indices allowed per note before requiring a split.
pub const CLOZE_MAX_INDICES: usize = 3;

/// Advisory length limit threshold (visible characters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    pub warn: usize,
    pub error: usize,
}

/// Length limit for the cloze carrier sentence (Sentença: context + lacunas).
pub const LIMIT_CLOZE_CARRIER: Limit = Limit {
    warn: 220,
    error: 320,
};

/// Length limit for a single cloze deletion target (uma lacuna atômica).
pub const LIMIT_CLOZE_DELETION: Limit = Limit {
    warn: 60,
    error: 80,
};

static PREFIX_WORDS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(de|do|da|dos|das|em|no|na|nos|nas|por|para|ao|à|até|com|entre|sobre|of|in|at|to|for|by|with|from|the|a|an)\s+",
    )
    .expect("valid prefix words regex")
});

static SUFFIX_PUNCT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[,;:\.]\s*$").expect("valid suffix punct regex"));

static NESTED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{c\d+::[^}]*\{\{c\d+").expect("valid nested cloze regex"));

static ID_IN_TEXT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*[\(\[]?(?:ANKI-\d+|B\d+-[A-Z]+-\d+)[\)\]]?").expect("valid id in text regex")
});

static EXT_HINT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\{\{c\d+::[^}]+\}\}\s*\((?:ex|exemplo|ex\.|e\.g\.)\s*:[^)]+\)")
        .expect("valid ext hint regex")
});

static COTEXT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\{\{c\d+::[^}]*?(?:classe|grupo|categoria)[^}]*?\}\}\s*,\s*(?:como|a exemplo de|tais como)\s+.*?\bou\b",
    )
    .expect("valid cotext regex")
});

static POST_ACRONYM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\(([A-Z0-9]{2,8})\)").expect("valid post acronym regex"));

static TARGET_ACRONYM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z0-9]{2,8}$").expect("valid target acronym regex"));

static CLOZE_DEL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{c\d+::.*?\}\}").expect("valid cloze del regex"));

static BR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").expect("valid br regex"));

static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("valid tag regex"));

static WS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("valid ws regex"));

static ENTITY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&(#x[0-9a-fA-F]+|#\d+|[a-zA-Z]+);").expect("valid entity regex"));

fn unescape_html_entities(s: &str) -> String {
    ENTITY_RE
        .replace_all(s, |caps: &regex::Captures<'_>| {
            let ent = &caps[1];
            if let Some(hex) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X")) {
                if let Some(ch) = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
                    return ch.to_string();
                }
            } else if let Some(dec) = ent.strip_prefix('#') {
                if let Some(ch) = dec.parse::<u32>().ok().and_then(char::from_u32) {
                    return ch.to_string();
                }
            } else {
                match ent {
                    "amp" => return "&".to_string(),
                    "lt" => return "<".to_string(),
                    "gt" => return ">".to_string(),
                    "quot" => return "\"".to_string(),
                    "apos" => return "'".to_string(),
                    "nbsp" => return "\u{00A0}".to_string(),
                    "le" => return "≤".to_string(),
                    "ge" => return "≥".to_string(),
                    "times" => return "×".to_string(),
                    "divide" => return "÷".to_string(),
                    "deg" => return "°".to_string(),
                    "plusmn" => return "±".to_string(),
                    "micro" => return "µ".to_string(),
                    _ => {}
                }
            }
            caps[0].to_string()
        })
        .into_owned()
}

/// Unwrap `{{cN::target(::hint)?}}` keeping `target`, preserving nested LaTeX braces.
#[must_use]
pub fn strip_cloze_markers(text: &str) -> String {
    if !text.contains("{{c") {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out: Vec<char> = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        if i + 6 < n
            && chars[i] == '{'
            && chars[i + 1] == '{'
            && chars[i + 2] == 'c'
            && chars[i + 3].is_ascii_digit()
        {
            let mut j = i + 3;
            while j < n && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j + 1 < n && chars[j] == ':' && chars[j + 1] == ':' {
                let content_start = j + 2;
                let mut k = content_start;
                let mut brace_depth: usize = 0;
                let mut sep_pos: Option<usize> = None;
                let mut closed = false;
                while k + 1 < n {
                    if chars[k] == '}' && chars[k + 1] == '}' && brace_depth == 0 {
                        let target_end = sep_pos.unwrap_or(k);
                        out.extend_from_slice(&chars[content_start..target_end]);
                        i = k + 2;
                        closed = true;
                        break;
                    } else if chars[k] == ':'
                        && chars[k + 1] == ':'
                        && brace_depth == 0
                        && sep_pos.is_none()
                    {
                        sep_pos = Some(k);
                        k += 2;
                        continue;
                    } else if chars[k] == '{' && (k == 0 || chars[k - 1] != '\\') {
                        brace_depth += 1;
                    } else if chars[k] == '}' && (k == 0 || chars[k - 1] != '\\') {
                        brace_depth = brace_depth.saturating_sub(1);
                    }
                    k += 1;
                }
                if closed {
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out.into_iter().collect()
}

/// Calculate the visible length of text as seen by the reader
/// (tags, entities, and cloze syntax stripped; whitespace normalized).
#[must_use]
pub fn visible_len(text: &str) -> usize {
    let s = strip_cloze_markers(text);
    let s = BR_RE.replace_all(&s, " ");
    let s = TAG_RE.replace_all(&s, "");
    let s = unescape_html_entities(&s);
    let s = WS_RE.replace_all(&s, " ");
    s.trim().chars().count()
}

fn make_cloze_diag(
    code: &'static str,
    message: impl Into<String>,
    row: Option<usize>,
    field: &str,
    severity: Severity,
    owner: &'static str,
    suggestion: Option<String>,
) -> Diagnostic {
    let mut diag = Diagnostic::new(code, severity, owner, message).with_field(field);
    if let Some(r) = row {
        diag = diag.with_row(r);
    }
    if let Some(s) = suggestion {
        diag = diag.with_suggestion(s);
    }
    diag
}

/// Helper to generate advisory length diagnostics matching `core/vocab.py::limit_diag`.
#[must_use]
pub fn limit_diag(
    code: &'static str,
    message_prefix: &str,
    limit: Limit,
    value: usize,
    row: Option<usize>,
    field: &str,
    suggestion: Option<&str>,
) -> Option<Diagnostic> {
    if value <= limit.warn {
        return None;
    }
    let threshold = if value > limit.error {
        limit.error
    } else {
        limit.warn
    };
    let default_suggestion = "Sinal de revisão: preserve o contexto necessário para tornar a resposta única \
        e mova explicação opcional para Notas Adicionais.";
    let msg = format!(
        "{message_prefix} ({value} > {threshold} caracteres; sinal de revisão, não prova de múltipla cobrança)"
    );
    Some(make_cloze_diag(
        code,
        msg,
        row,
        field,
        Severity::Warn,
        "anki-card-generator",
        Some(suggestion.unwrap_or(default_suggestion).to_owned()),
    ))
}

fn py_is_upper(s: &str) -> bool {
    let mut has_cased = false;
    for c in s.chars() {
        if c.is_lowercase() {
            return false;
        }
        if c.is_uppercase() {
            has_cased = true;
        }
    }
    has_cased
}

/// Return the set of distinct `cN` indices found in *sentence*.
#[must_use]
pub fn count_cloze_indices(sentence: &str) -> std::collections::BTreeSet<usize> {
    anki_parser::count_cloze_indices(sentence)
}

/// Validate Cloze note sentence field against structural rules and heuristic quality gates.
#[must_use]
pub fn check_cloze(sentence: &str, row: Option<usize>, field: Option<&str>) -> Vec<Diagnostic> {
    let field_name = field.unwrap_or("Sentença");
    let mut diags = Vec::new();
    let spans: Vec<ClozeSpan> = find_cloze_spans(sentence);

    if spans.is_empty() {
        return diags;
    }

    // ── CLOZE.NESTED ─────────────────────────────────────────────────────────
    if NESTED_RE.is_match(sentence) {
        diags.push(make_cloze_diag(
            "CLOZE.NESTED",
            "Nested cloze deletions found",
            row,
            field_name,
            Severity::Critical,
            "anki-cloze-creator",
            None,
        ));
    }

    // ── CLOZE.ID_IN_TEXT ─────────────────────────────────────────────────────
    if ID_IN_TEXT_RE.is_match(sentence) {
        diags.push(make_cloze_diag(
            "CLOZE.ID_IN_TEXT",
            "Visible card ID found in carrier sentence. IDs must reside in tags or metadata, not in the prompt.",
            row,
            field_name,
            Severity::Critical,
            "anki-cloze-creator",
            None,
        ));
    }

    // ── CLOZE.EXT_HINT_LEAK ──────────────────────────────────────────────────
    if EXT_HINT_RE.is_match(sentence) {
        diags.push(make_cloze_diag(
            "CLOZE.EXT_HINT_LEAK",
            "Parenthetical example immediately following cloze deletion may leak/spoil the answer. Use canonical hint syntax {{cN::target::hint}} instead.",
            row,
            field_name,
            Severity::Warn,
            "anki-cloze-creator",
            None,
        ));
    }

    // ── CLOZE.COTEXT_LEAK ───────────────────────────────────────────────────
    if COTEXT_RE.is_match(sentence) {
        diags.push(make_cloze_diag(
            "CLOZE.COTEXT_LEAK",
            "Visible exemplar tokens following a class deletion may leak the answer. Separate into independent cards or omit visible exemplars.",
            row,
            field_name,
            Severity::Warn,
            "anki-cloze-creator",
            None,
        ));
    }

    // ── CLOZE.ACRONYM_LEAK & CLOZE.TOO_LONG ──────────────────────────────────
    let sentence_chars: Vec<char> = sentence.chars().collect();
    for span in &spans {
        let target_clean = span.target.trim();
        let target_vis_len = visible_len(target_clean);
        if target_vis_len > 120 {
            diags.push(make_cloze_diag(
                "CLOZE.TOO_LONG",
                format!(
                    "c{}: Deletion target is long ({target_vis_len} visible chars > 120; sinal de revisão, não prova de não atomicidade).",
                    span.index
                ),
                row,
                field_name,
                Severity::Warn,
                "anki-cloze-creator",
                Some(
                    "Revise se a lacuna oculta itens independentes ou detalhe opcional; \
                    preserve o contexto necessário na sentença e mova explicações para Notas Adicionais."
                        .to_owned(),
                ),
            ));
        }

        // Check acronym leak: Target followed by (ACRONYM) where acronym matches target
        let post: String = sentence_chars[span.end..(span.end + 25).min(sentence_chars.len())]
            .iter()
            .collect();
        let post_trimmed = post.trim();
        if let Some(caps) = POST_ACRONYM_RE.captures(post_trimmed) {
            let sigla = &caps[1];
            if target_clean.split_whitespace().count() >= 2 {
                let words: Vec<&str> = target_clean
                    .split(|c: char| c.is_whitespace() || c == '-')
                    .filter(|w| !w.is_empty() && (w.chars().count() > 2 || py_is_upper(w)))
                    .collect();
                let initials: String = words
                    .iter()
                    .filter_map(|w| w.chars().next().map(|c| c.to_ascii_uppercase()))
                    .collect();

                if !initials.is_empty() {
                    let sigla_first = sigla.chars().next();
                    let init_first = initials.chars().next();
                    if sigla_first == init_first {
                        let sigla_set: HashSet<char> = sigla.chars().collect();
                        let init_set: HashSet<char> = initials.chars().collect();
                        let common = sigla_set.intersection(&init_set).count();
                        if initials.contains(sigla) || sigla.contains(&initials) || common >= 2 {
                            diags.push(make_cloze_diag(
                                "CLOZE.ACRONYM_LEAK",
                                format!(
                                    "c{}: Target '{target_clean}' is followed by visible parenthetical acronym '({sigla})', which spoils the answer. Enclose both in the deletion: '{{{{c{}::{target_clean} ({sigla})}}}}'.",
                                    span.index, span.index
                                ),
                                row,
                                field_name,
                                Severity::Critical,
                                "anki-cloze-creator",
                                None,
                            ));
                        }
                    }
                }
            }
        }

        // Check acronym leak: Target is ACRONYM preceded by Full Name (
        if TARGET_ACRONYM_RE.is_match(target_clean) {
            let start_idx = span.start.saturating_sub(70);
            let pre: String = sentence_chars[start_idx..span.start].iter().collect();
            let pre_trimmed = pre.trim();
            if let Some(name_with_trailing) = pre_trimmed.strip_suffix('(') {
                let name_part = name_with_trailing.trim();
                let words: Vec<&str> = name_part
                    .split(|c: char| c.is_whitespace() || c == '-')
                    .filter(|w| !w.is_empty() && w.chars().count() > 2)
                    .collect();
                if words.len() >= 2 {
                    let target_len = target_clean.chars().count();
                    let slice_start = words.len().saturating_sub(target_len);
                    let selected_words = &words[slice_start..];
                    let initials: String = selected_words
                        .iter()
                        .filter_map(|w| w.chars().next().map(|c| c.to_ascii_uppercase()))
                        .collect();
                    if !initials.is_empty()
                        && target_clean.chars().next() == initials.chars().next()
                    {
                        diags.push(make_cloze_diag(
                            "CLOZE.ACRONYM_LEAK",
                            format!(
                                "c{}: Acronym target '{target_clean}' is preceded by visible full name '{name_part}', which spoils the answer. Enclose both in the deletion.",
                                span.index
                            ),
                            row,
                            field_name,
                            Severity::Critical,
                            "anki-cloze-creator",
                            None,
                        ));
                    }
                }
            }
        }
    }

    // ── CLOZE.NONSEQ ─────────────────────────────────────────────────────────
    let indices: Vec<usize> = spans.iter().map(|s| s.index).collect();
    let mut distinct: Vec<usize> = indices.clone();
    distinct.sort_unstable();
    distinct.dedup();

    if indices.len() != distinct.len() {
        diags.push(make_cloze_diag(
            "CLOZE.NONSEQ",
            "Duplicate cloze index found in the same note (each cN index must appear exactly once)",
            row,
            field_name,
            Severity::Critical,
            "anki-cloze-creator",
            None,
        ));
    }

    if distinct.first().copied().is_some_and(|first| first != 1) {
        let first = distinct[0];
        diags.push(make_cloze_diag(
            "CLOZE.NONSEQ",
            format!("Cloze indices must start at c1, but first index is c{first}"),
            row,
            field_name,
            Severity::Critical,
            "anki-cloze-creator",
            None,
        ));
    }

    for pair in distinct.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if b != a + 1 {
            diags.push(make_cloze_diag(
                "CLOZE.NONSEQ",
                format!(
                    "Gap in cloze indices: c{a} followed by c{b} (c{} missing)",
                    a + 1
                ),
                row,
                field_name,
                Severity::Critical,
                "anki-cloze-creator",
                None,
            ));
        }
    }

    // ── CLOZE.BOUNDARY (heuristic WARN) ──────────────────────────────────────
    for span in &spans {
        if PREFIX_WORDS_RE.is_match(&span.target) {
            let slice: String = span.target.chars().take(20).collect();
            diags.push(make_cloze_diag(
                "CLOZE.BOUNDARY",
                format!(
                    "c{}: deletion starts with a preposition/article ('{slice}…')",
                    span.index
                ),
                row,
                field_name,
                Severity::Warn,
                "anki-cloze-creator",
                Some(format!(
                    "Move the preposition outside the deletion: 'de {{{{c{}::...}}}}'",
                    span.index
                )),
            ));
        }
        if SUFFIX_PUNCT_RE.is_match(&span.target) {
            diags.push(make_cloze_diag(
                "CLOZE.BOUNDARY",
                format!(
                    "c{}: deletion ends with punctuation — move it outside",
                    span.index
                ),
                row,
                field_name,
                Severity::Warn,
                "anki-cloze-creator",
                None,
            ));
        }
    }

    // ── CLOZE.CONTEXT_WARN (heuristic WARN) ──────────────────────────────────
    let cleaned = CLOZE_DEL_RE.replace_all(sentence, "BLANK");
    if cleaned.chars().count() < 40 {
        diags.push(make_cloze_diag(
            "CLOZE.CONTEXT_WARN",
            "Carrier sentence is very short (<40 chars) — may lack clinical context",
            row,
            field_name,
            Severity::Warn,
            "anki-cloze-creator",
            Some("Provide full clinical scenario/guideline/population in the sentence".to_owned()),
        ));
    }

    // ── CARD.SPLIT_CLOZE_INDICES / CARD.TOO_LONG_* ───────────────────────────
    if distinct.len() > CLOZE_MAX_INDICES {
        diags.push(make_cloze_diag(
            "CARD.SPLIT_CLOZE_INDICES",
            format!(
                "{} índices cloze (máximo operacional {CLOZE_MAX_INDICES}): cada índice gera um cartão separado no Anki e contar índices não prova atomicidade",
                distinct.len()
            ),
            row,
            field_name,
            Severity::Critical,
            "anki-card-generator",
            Some(format!(
                "SPLIT: mantenha no máximo {CLOZE_MAX_INDICES} índices c1..c{CLOZE_MAX_INDICES} sequenciais por nota e revise cada lacuna quanto a listas ocultas ou obrigações independentes."
            )),
        ));
    }

    if let Some(diag) = limit_diag(
        "CARD.TOO_LONG_SENTENCE",
        "Sentença (carrier) extensa",
        LIMIT_CLOZE_CARRIER,
        visible_len(sentence),
        row,
        field_name,
        Some(
            "Revise se há fatos independentes; preserve o contexto necessário na sentença \
            e mova explicação opcional para Notas Adicionais.",
        ),
    ) {
        diags.push(diag);
    }

    for span in &spans {
        if let Some(diag) = limit_diag(
            "CARD.TOO_LONG_DELETION",
            &format!("c{}: deleção extensa", span.index),
            LIMIT_CLOZE_DELETION,
            visible_len(&span.target),
            row,
            field_name,
            Some(
                "Revise se a lacuna oculta uma lista ou explicação opcional; preserve o contexto necessário \
                na sentença e mova detalhes para Notas Adicionais.",
            ),
        ) {
            diags.push(diag);
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
    fn test_good_sentence_passes() {
        let s = "No manejo do Status Epilepticus refratário a benzodiazepínicos em adultos segundo a AES/ILAE, \
                 a terapia de 2ª linha inclui {{c1::levetiracetam}}, {{c2::fosfenitoína}} ou {{c3::valproato de sódio}} IV.";
        let diags = check_cloze(s, None, None);
        assert_eq!(worst_severity(&diags), "PASS");
    }

    #[test]
    fn test_nonseq_gap_fails() {
        let s = "O paciente apresentou {{c1::febre}} e depois {{c3::convulsão}}.";
        let diags = check_cloze(s, None, None);
        assert!(has_code(&diags, "CLOZE.NONSEQ"));
        assert_eq!(worst_severity(&diags), "FAIL");
    }

    #[test]
    fn test_nonseq_start_c2_fails() {
        let s = "A droga de primeira linha é {{c2::carbamazepina}}.";
        let diags = check_cloze(s, None, None);
        assert!(has_code(&diags, "CLOZE.NONSEQ"));
        assert_eq!(worst_severity(&diags), "FAIL");
    }

    #[test]
    fn test_nonseq_duplicate_fails() {
        let s = "Tratamento de primeira linha é {{c1::ceftriaxona}} e segunda linha é {{c1::vancomicina}}.";
        let diags = check_cloze(s, None, None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == "CLOZE.NONSEQ" && d.message.contains("Duplicate cloze index"))
        );
    }

    #[test]
    fn test_nested_cloze_fails() {
        let s = "Texto com {{c1::deleção {{c2::aninhada}} aqui}} presente no exame.";
        let diags = check_cloze(s, None, None);
        assert!(has_code(&diags, "CLOZE.NESTED"));
        assert_eq!(worst_severity(&diags), "FAIL");
    }

    #[test]
    fn test_id_in_text_fails() {
        let s = "ANKI-101 No manejo da hipertensão resistente, prescrever {{c1::espironolactona}}.";
        let diags = check_cloze(s, None, None);
        assert!(has_code(&diags, "CLOZE.ID_IN_TEXT"));
        assert_eq!(worst_severity(&diags), "FAIL");
    }

    #[test]
    fn test_ext_hint_leak_warns() {
        let s =
            "Administrar {{c1::antagonista dos canais de cálcio}} (ex: anlodipino) diariamente.";
        let diags = check_cloze(s, None, None);
        assert!(has_code(&diags, "CLOZE.EXT_HINT_LEAK"));
    }

    #[test]
    fn test_cotext_leak_warns() {
        let s = "Prescrever fármaco da {{c1::classe dos betabloqueadores}}, como atenolol ou metoprolol.";
        let diags = check_cloze(s, None, None);
        assert!(has_code(&diags, "CLOZE.COTEXT_LEAK"));
    }

    #[test]
    fn test_acronym_leak_post_and_pre() {
        // Case 1: Post acronym
        let s1 = "Paciente com {{c1::esclerose lateral amiotrófica}} (ELA) em fase avançada.";
        let diags1 = check_cloze(s1, None, None);
        assert!(has_code(&diags1, "CLOZE.ACRONYM_LEAK"));

        // Case 2: Pre full name
        let s2 = "Paciente com esclerose lateral amiotrófica ({{c1::ELA}}) em fase avançada.";
        let diags2 = check_cloze(s2, None, None);
        assert!(has_code(&diags2, "CLOZE.ACRONYM_LEAK"));

        // Case 3: Occluded together clean
        let s3 = "Paciente com {{c1::esclerose lateral amiotrófica (ELA)}} em fase avançada.";
        let diags3 = check_cloze(s3, None, None);
        assert!(!has_code(&diags3, "CLOZE.ACRONYM_LEAK"));
    }

    #[test]
    fn test_boundary_preposition_and_punct() {
        let s1 = "A PA deve ser {{c1::de pelo menos 140}} mmHg.";
        let diags1 = check_cloze(s1, None, None);
        assert!(has_code(&diags1, "CLOZE.BOUNDARY"));

        let s2 = "A pressão alvo de pulso deve ser {{c1::140;}} para estabilidade clínica.";
        let diags2 = check_cloze(s2, None, None);
        assert!(has_code(&diags2, "CLOZE.BOUNDARY"));
    }

    #[test]
    fn test_context_warn_short_sentence() {
        let s = "Dose de {{c1::5 mg}}.";
        let diags = check_cloze(s, None, None);
        assert!(has_code(&diags, "CLOZE.CONTEXT_WARN"));
    }

    #[test]
    fn test_card_split_cloze_indices_at_four() {
        let s = "No paciente em choque, administrar {{c1::a}}, {{c2::b}}, {{c3::c}} e {{c4::d}} conforme protocolo.";
        let diags = check_cloze(s, None, None);
        assert!(has_code(&diags, "CARD.SPLIT_CLOZE_INDICES"));
    }

    #[test]
    fn test_too_long_deletion_and_sentence() {
        let long_del = "a".repeat(125);
        let s = format!(
            "Definição clínica de quadro neurológico crônico extenso: {{{{c1::{long_del}}}}}"
        );
        let diags = check_cloze(&s, None, None);
        assert!(has_code(&diags, "CLOZE.TOO_LONG"));
        assert!(has_code(&diags, "CARD.TOO_LONG_DELETION"));
    }

    #[test]
    fn test_visible_len_stripping() {
        assert_eq!(visible_len("<b>abc</b>"), 3);
        assert_eq!(visible_len("{{c1::abc::dica}}"), 3);
        assert_eq!(visible_len("a&amp;b"), 3);
        assert_eq!(visible_len("a<br>b"), 3);
        assert_eq!(visible_len("  espaços   múltiplos  "), 17);
        assert_eq!(visible_len(r"{{c1::\frac{a}{b}::dica longa}}"), 11);
    }
}
