//! Deterministic notes field checks (`checks/notes.py`).
//!
//! Checks:
//! - `NOTE.METADATA` — forbidden metadata headers (ID:, Âncora:, Cross-ref:, Relevância:) (CRITICAL)
//! - `NOTE.DUP_ANSWER_WARN` — notes verbatim-repeat the answer without pedagogical explanation (WARN)
//! - `NOTE.EXAM_ONLY_WARN` — notes contain only exam-appearance mention without explanation (WARN)
//! - `NOTE.LENGTH_WARN` — notes field outside recommended length (WARN, configurable)
//! - `NOTE.STYLE_WARN` — list items not using `• <b>…</b>` pattern (WARN)

use anki_domain::{Diagnostic, Severity};
use regex::Regex;
use std::sync::LazyLock;

pub const DEFAULT_MAX_LENGTH: usize = 1200;

static METADATA_HEADERS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?im)^\s*(ID\s*:|Âncora\s*:|Anchor\s*:|Cross-ref\s*:|Cross\s+ref\s*:|Relevância\s*:|Relevancia\s*:)",
    )
    .expect("valid metadata headers regex")
});

static RESTATEMENT_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:a\s+resposta\s+(?:correta\s+)?(?:é|e)|resposta(?:\s+correta)?)\s*:?\s*")
        .expect("valid restatement prefix regex")
});

static LEADING_BULLET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[•\-–—*\s]+").expect("valid leading bullet regex"));

static TRAILING_PUNCT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[.!?…:;\s]+$").expect("valid trailing punct regex"));

static SEGMENT_SPLIT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\n.!?]+").expect("valid segment split regex"));

static TRAILING_SEGMENT_PUNCT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[,:;()\s]+$").expect("valid trailing segment punct regex"));

static TABLE_BLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<table.*?>.*?</table>").expect("valid table block regex"));

static WHITESPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("valid whitespace regex"));

static EXAM_BOILERPLATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    let known_labels = "USP|UNIFESP|UNICAMP|UNESP|IAMSPE|FCMSCSP|SUS-SP|ENARE|PSU-MG|PSU|AMRIGS|AMP|SURCE|SES-DF|SES-PE|SES-RJ|UFRJ|UERJ|UFMG|UFPR|UFRGS|UFBA|UFC|UFPE|HCFMUSP|HC-FMUSP|HC-UFMG|INCA|INTO|FHEMIG|SCMSP|FAMERP|FAMEMA|REVALIDA|ABN|TE-ABN|AMB|CFM|CBR|SBC|SBN";
    let pattern = format!(
        r"(?i)^(?:(?:aten[çc][ãa]o|dica|nota|obs\.?|observa[çc][ãa]o|prova|alerta)\s*[:\-–—]\s*)?(?:(?:j[áa]\s+)?(?:foi\s+)?(?:caiu|cobrad[oa]|pedid[oa]|perguntad[oa]|apareceu|costuma\s+cair|cai(?:\s+(?:muito|sempre|frequentemente))?)\s+(?:em|na|no|nas|nos)\s+(?:provas?|concursos?|resid[êe]ncia(?:\s+m[ée]dica)?|quest[õo]es|exames?|bancas?)|(?:tema|assunto|conceito|t[óo]pico|detalhe|quest[ãa]o)\s+(?:cl[áa]ssic[oa]|frequente|recorrente|quente|(?:muito\s+)?cobrad[oa])\s+(?:em|na|no|nas|nos|de)\s+(?:provas?|concursos?|resid[êe]ncia(?:\s+m[ée]dica)?|bancas?|exames?)|(?:muito\s+)?(?:cobrad[oa]|frequente|recorrente|cl[áa]ssic[oa])\s+(?:em|na|no|nas|nos)\s+(?:provas?|concursos?|resid[êe]ncia(?:\s+m[ée]dica)?|bancas?)|prova(?:\s+(?:d[eao]\s+)?(?:resid[êe]ncia(?:\s+m[ée]dica)?|(?-i:(?:{known_labels}))))?(?:\s+(?:de|em))?\s+(?:19|20)\d{{2}})(?:\s+d[eao]\s+resid[êe]ncia(?:\s+m[ée]dica)?|\s+(?:(?:d[aeo]|n[ao]|em)\s+)?(?-i:(?:{known_labels}))\b|\s+(?:(?:d[aeo]|n[ao]|em)\s+)?(?:19|20)\d{{2}}(?:\s*[/\\-]\s*(?:19|20)?\d{{2,4}})?){{0,3}}$"
    );
    Regex::new(&pattern).expect("valid exam boilerplate regex")
});

fn decode_entity_token(token: &str) -> Option<char> {
    if let Some(rest) = token.strip_prefix('#') {
        if let Some(hex) = rest.strip_prefix('x').or_else(|| rest.strip_prefix('X')) {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        } else {
            rest.parse::<u32>().ok().and_then(char::from_u32)
        }
    } else {
        match token {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "ge" => Some('≥'),
            "le" => Some('≤'),
            "bull" => Some('•'),
            "deg" => Some('°'),
            "plusmn" => Some('±'),
            "times" => Some('×'),
            "divide" => Some('÷'),
            "micro" => Some('µ'),
            _ => None,
        }
    }
}

fn is_block_break_tag(tag: &str) -> bool {
    matches!(
        tag,
        "br" | "hr"
            | "p"
            | "div"
            | "li"
            | "tr"
            | "blockquote"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "ul"
            | "ol"
    )
}

/// Parse HTML text preserving block line breaks and decoding entities (`checks/information.py::parse_html_text`).
#[must_use]
pub fn parse_html_text(raw: &str) -> (String, usize, usize) {
    if raw.is_empty() {
        return (String::new(), 0, 0);
    }
    let mut chunks = String::with_capacity(raw.len());
    let mut li_count = 0usize;
    let mut br_count = 0usize;
    let mut skip_depth = 0usize;

    let bytes = raw.as_bytes();
    let n = bytes.len();
    let mut i = 0;

    while i < n {
        if bytes[i] == b'<' {
            if raw[i..].starts_with("<!--")
                && let Some(end_comment) = raw[i + 4..].find("-->")
            {
                i += 4 + end_comment + 3;
                continue;
            }
            if let Some(close_pos) = bytes[i + 1..].iter().position(|&b| b == b'>') {
                let tag_content = raw[i + 1..i + 1 + close_pos].trim();
                let is_close = tag_content.starts_with('/');
                let bare_tag = if is_close {
                    tag_content[1..].trim_start()
                } else {
                    tag_content
                };
                let tag_name: String = bare_tag
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .flat_map(char::to_lowercase)
                    .collect();

                if tag_name == "script" || tag_name == "style" {
                    if is_close {
                        skip_depth = skip_depth.saturating_sub(1);
                    } else {
                        skip_depth += 1;
                    }
                    i += 2 + close_pos;
                    continue;
                }

                if skip_depth == 0 {
                    if tag_name == "li" {
                        if !is_close {
                            li_count += 1;
                        }
                        chunks.push('\n');
                    } else if tag_name == "br" {
                        br_count += 1;
                        chunks.push('\n');
                    } else if is_block_break_tag(&tag_name) {
                        chunks.push('\n');
                    } else if tag_name == "td" || tag_name == "th" {
                        chunks.push(' ');
                    }
                }

                i += 2 + close_pos;
                continue;
            }
        }

        if skip_depth == 0 {
            if bytes[i] == b'&'
                && let Some(semi_pos) = bytes[i + 1..].iter().position(|&b| b == b';')
            {
                let entity = &raw[i + 1..i + 1 + semi_pos];
                if let Some(decoded) = decode_entity_token(entity) {
                    chunks.push(if decoded == '\u{00a0}' { ' ' } else { decoded });
                    i += 2 + semi_pos;
                    continue;
                }
            }
            let ch = raw[i..].chars().next().expect("valid utf8");
            chunks.push(if ch == '\u{00a0}' { ' ' } else { ch });
            i += ch.len_utf8();
        } else {
            let ch = raw[i..].chars().next().expect("valid utf8");
            i += ch.len_utf8();
        }
    }

    let lines: Vec<String> = chunks
        .lines()
        .map(|ln| {
            let collapsed = WHITESPACE_RE.replace_all(ln, " ");
            collapsed.trim().to_owned()
        })
        .filter(|ln| !ln.is_empty())
        .collect();

    (lines.join("\n"), li_count, br_count)
}

/// Return collapsed single-line visible text with HTML tags removed and entities decoded.
#[must_use]
pub fn html_to_visible_text(raw: &str) -> String {
    let (text, _, _) = parse_html_text(raw);
    let collapsed = WHITESPACE_RE.replace_all(&text, " ");
    collapsed.trim().to_owned()
}

/// Count visible characters (`checks/information.py::visible_char_count`).
#[must_use]
pub fn visible_char_count(raw: &str) -> usize {
    html_to_visible_text(raw).chars().count()
}

/// Normalize text for exact repetition comparison.
#[must_use]
pub fn normalize_for_exact_match(raw: &str) -> String {
    let vis = html_to_visible_text(raw).to_lowercase();
    let trimmed_leading = LEADING_BULLET_RE.replace(&vis, "");
    let trimmed_trailing = TRAILING_PUNCT_RE.replace(&trimmed_leading, "");
    trimmed_trailing.trim().to_owned()
}

/// Return True iff notes only repeats or restates answer without adding pedagogical explanation.
#[must_use]
pub fn is_exact_answer_restatement(notes: &str, answer: &str) -> bool {
    let norm_answer = normalize_for_exact_match(answer);
    let norm_notes = normalize_for_exact_match(notes);
    if norm_answer.is_empty() || norm_notes.is_empty() {
        return false;
    }
    if norm_notes == norm_answer {
        return true;
    }
    if let Some(m) = RESTATEMENT_PREFIX_RE.find(&norm_notes)
        && m.start() == 0
    {
        let stripped = norm_notes[m.end()..].trim();
        return !stripped.is_empty() && stripped == norm_answer && stripped != norm_notes;
    }
    false
}

/// Return True iff non-empty notes contain ONLY exam-appearance mentions without pedagogical explanation.
#[must_use]
pub fn is_exam_only_note(notes: &str) -> bool {
    let (text, _, _) = parse_html_text(notes);
    if text.is_empty() {
        return false;
    }
    let raw_segments: Vec<&str> = SEGMENT_SPLIT_RE
        .split(&text)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if raw_segments.is_empty() {
        return false;
    }
    let mut non_empty_segments = 0;
    for seg in raw_segments {
        let cleaned1 = LEADING_BULLET_RE.replace(seg, "");
        let cleaned = TRAILING_SEGMENT_PUNCT_RE.replace(cleaned1.trim(), "");
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            continue;
        }
        non_empty_segments += 1;
        if !EXAM_BOILERPLATE_RE.is_match(cleaned) {
            return false;
        }
    }
    non_empty_segments > 0
}

fn has_style_item_violation(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let ch = chars[i];
        if ch == '•' || ch == '-' || ch == '*' {
            let ws_count = chars[i + 1..]
                .iter()
                .take_while(|c| c.is_whitespace())
                .count();
            if ws_count > 0 {
                let after_ws: String = chars[i + 1 + ws_count..].iter().collect();
                if !after_ws.starts_with("<b>") {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// Check the `Notas Adicionais` field matching `checks/notes.py::check_notes`.
#[must_use]
pub fn check_notes(notes: &str, answer: &str, row: Option<usize>) -> Vec<Diagnostic> {
    check_notes_ext(notes, answer, row, "Notas Adicionais", DEFAULT_MAX_LENGTH)
}

/// Check the notes field with custom field name and max visible character limit.
#[must_use]
pub fn check_notes_ext(
    notes: &str,
    answer: &str,
    row: Option<usize>,
    field: &str,
    max_length: usize,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    // ── NOTE.METADATA ─────────────────────────────────────────────────────────
    for m in METADATA_HEADERS_RE.find_iter(notes) {
        let mut d = Diagnostic::new(
            "NOTE.METADATA",
            Severity::Critical,
            "anki-notes-enricher",
            format!("Forbidden metadata header found: '{}'", m.as_str().trim()),
        )
        .with_field(field)
        .with_suggestion("Remove metadata — Notes field is for pedagogical explanations only");
        if let Some(r) = row {
            d = d.with_row(r);
        }
        diags.push(d);
    }

    // ── NOTE.DUP_ANSWER_WARN ─────────────────────────────────────────────────
    if !answer.is_empty() && !notes.is_empty() && is_exact_answer_restatement(notes, answer) {
        let mut d = Diagnostic::new(
            "NOTE.DUP_ANSWER_WARN",
            Severity::Warn,
            "anki-notes-enricher",
            "Notes repeat the answer verbatim — deepen the explanation instead",
        )
        .with_field(field)
        .with_suggestion(
            "Explain *why* the answer is correct or add related nuance from the source",
        );
        if let Some(r) = row {
            d = d.with_row(r);
        }
        diags.push(d);
    }

    // ── NOTE.EXAM_ONLY_WARN ──────────────────────────────────────────────────
    if !notes.is_empty() && is_exam_only_note(notes) {
        let mut d = Diagnostic::new(
            "NOTE.EXAM_ONLY_WARN",
            Severity::Warn,
            "anki-notes-enricher",
            "Notes contain only an exam mention without pedagogical explanation",
        )
        .with_field(field)
        .with_suggestion(
            "Menção isolada a prova não explica a unidade; acrescente explicação útil da fonte ou deixe Notas Adicionais vazio.",
        );
        if let Some(r) = row {
            d = d.with_row(r);
        }
        diags.push(d);
    }

    // ── NOTE.LENGTH_WARN ─────────────────────────────────────────────────────
    let has_table = notes.to_lowercase().contains("<table");
    let effective_max = if has_table {
        max_length.max(2500)
    } else {
        max_length
    };
    let visible_length = visible_char_count(notes);
    if visible_length > effective_max {
        let mut d = Diagnostic::new(
            "NOTE.LENGTH_WARN",
            Severity::Warn,
            "anki-notes-enricher",
            format!(
                "Notes field is {visible_length} visible chars (recommended ≤ {effective_max}; advisory only)"
            ),
        )
        .with_field(field);
        if let Some(r) = row {
            d = d.with_row(r);
        }
        diags.push(d);
    }

    // ── NOTE.STYLE_WARN ──────────────────────────────────────────────────────
    let notes_outside_tables = TABLE_BLOCK_RE.replace_all(notes, "");
    if has_style_item_violation(&notes_outside_tables) {
        let mut d = Diagnostic::new(
            "NOTE.STYLE_WARN",
            Severity::Warn,
            "anki-notes-enricher",
            "List items found without <b>…</b> wrapper — preferred: '• <b>Tópico:</b> …'",
        )
        .with_field(field);
        if let Some(r) = row {
            d = d.with_row(r);
        }
        diags.push(d);
    }

    diags
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
}
