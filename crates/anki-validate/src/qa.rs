//! Deterministic QA card checks (`checks/qa.py`).
//!
//! Checks:
//! - `QA.CARDINALITY` — answer contains 3+ atomic items (CRITICAL)
//! - `QA.YESNO` — yes/no question or single-word Sim/Não answer (CRITICAL)
//! - `QA.ID_IN_TEXT` — visible card ID in stem (CRITICAL)
//! - `QA.MC_EMBED` — multiple-choice options in stem (CRITICAL)
//! - `QA.CONTEXT_WARN` — stem lacks clinical scenario/population/source (WARN)
//! - `QA.NUMBER_UNIT` — numeric answer without unit or direction (WARN)
//! - `CARD.SPLIT_QA_MULTI_QUESTION` — stem requests two independent retrievals (WARN)
//! - `CARD.TOO_LONG_STEM` — advisory stem length warning (WARN)
//! - `CARD.TOO_LONG_ANSWER` — advisory answer length warning (WARN)

use anki_domain::{Diagnostic, Severity};
use regex::Regex;
use std::sync::LazyLock;

pub use crate::cloze::{strip_cloze_markers, visible_len};
pub use crate::html_text::{html_to_visible_text, parse_html_text};

static YESNO_ANSWER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(sim|não|yes|no|verdadeiro|falso|correto|incorreto)\s*$")
        .expect("valid yesno answer regex")
});

static YESNO_STEM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(?:é\s+verdadeiro|é\s+falso|é\s+correto|é\s+incorreto|concordo|discordo|é\s+contraindicado)\b|^.*\b(?:confirma[mr]?\s*(?:ou|e)\s|is\s+(?:true|false|correct|indicated))\b",
    )
    .expect("valid yesno stem regex")
});

static MC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?is)(?:(?:\A|[\s;:])\(\s*a\s*\).+?(?:\A|[\s;:])\(\s*b\s*\)|(?:\A|[\s;:])\[\s*a\s*\].+?(?:\A|[\s;:])\[\s*b\s*\]|(?:\A|[\s;:])a\s*\).+?(?:\A|[\s;:])b\s*\))",
    )
    .expect("valid mc embed regex")
});

static CONTEXT_MARKERS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(paciente|patient|segundo|conforme|diretriz|guideline|critério|criteria|portador|com\s+(?:diagnóstico|suspeita)|população|adulto|criança|pediátrico|gestante|idoso)\b|\b(?-i:IRC|DRC|ICC|ELA|EM|AVC|SNC)\b",
    )
    .expect("valid context regex")
});

static ORDINAL_CLASS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:tipo|fase|classe|est[áa]gio|grau)\s+\d+\b").expect("valid ordinal regex")
});

static IDENTIFIER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[A-Za-z]+\d+[A-Za-z\d]*|\b\d+[pq]\d+(?:\.\d+)?\b")
        .expect("valid identifier regex")
});

static DIRECTION_OP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[><≥≤=]|&[gl]t;|&[gl]e;").expect("valid direction op regex"));

static NUMBER_TOKEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d+(?:[.,]\d+)?\b").expect("valid number token regex"));

static ALLOWED_UNIT_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:mg|mcg|g|kg|mL|L|mmHg|%|UI|U|°C|anos|dias|horas|h\b|m\b)")
        .expect("valid allowed unit prefix regex")
});

static COORDINATED_Q_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:e|ou)\s+(?:qual|quais|quando|onde|como|por\s+que)\b")
        .expect("valid coordinated question regex")
});

static APPOSITIVE_AFTER_COMMA_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:caracterizad[oa]s?|definid[oa]s?|conhecid[oa]s?|denominad[oa]s?|chamad[oa]s?|ou\s+seja|isto\s+[ée]|i\.e\.|e\.g\.|p\.ex\.|como\s+tamb[ée]m|bem\s+como|assim\s+como|manifestando-se|apresentando-se|cursando\s+com|podendo|que\s+|o\s+qual|a\s+qual|os\s+quais|as\s+quais|cuj[oa]s?|onde|na\s+qual|no\s+qual|com\s+(?:incapacidade|aus[êe]ncia|presen[çc]a|dura[çc][ãa]o|predom[íi]nio|exce[çc][ãa]o|risco|desvio|melhora|piora)|sem\s+|principalmente|especialmente|sobretudo|geralmente|tipicamente|frequentemente|habitualmente|usualmente|em\s+geral|em\s+especial|ap[óo]s|devido\s+a|associad[oa]s?\s+a|secund[áa]ri[oa]s?\s+a|pois\s+|porque\s+|visto\s+que|embora\s+|por[ée]m\b|contudo\b|entretanto\b|todavia\b|mas\b|incluindo\b|exceto\b|salvo\b|inclusive\b|segundo\b|conforme\b)\b",
    )
    .expect("valid appositive regex")
});

static DOSE_REGIMEN_AFTER_COMMA_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:(?:VO|IV|IM|SC|EV|SL|VR|ID|inalat[óo]rio|t[óo]pico|oral|intravenos[oa]|subcut[âa]ne[oa])\b|\d+x\s*/\s*(?:dia|semana|m[êe]s)|a\s+cada\s+\d+|(?:de\s+)?\d+\s*/\s*\d+\s*h\b|em\s+(?:bolus|dose\s+[úu]nica|doses?\s+divididas?|infus[ãa]o)\b|(?:bolus|dose\s+[úu]nica|m[áa]ximo|m[íi]nimo)\b|at[ée]\s+\d+|por\s+\d+\s+(?:dias?|horas?|semanas?|meses?)|durante\s+\d+|se\s+necess[áa]rio|dividid[oa]\s+em|dilu[íi]d[oa]|(?:mg|mcg|ug|g|kg|ml|mL|L|UI|mEq|mmol)\b)",
    )
    .expect("valid dose regimen regex")
});

static COMPARISON_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:vs\.?|versus)\b").expect("valid comparison regex"));

// A classe de glifos vem da união única (`text_norm::BULLET_GLYPHS`, Onda 2/B24); só os
// glifos tipográficos entram, como antes — `-`/`*` inline são pontuação comum em resposta
// clínica ("anti-inflamatório", "p < 0,05") e não separam itens.
static BULLET_INLINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    let glyphs: String = crate::text_norm::BULLET_GLYPHS
        .iter()
        .copied()
        .filter(|c| crate::text_norm::is_glyph_bullet(*c))
        .collect();
    Regex::new(&format!(r"(?:\A|\s)[{glyphs}]\s+")).expect("valid bullet inline regex")
});

static TRAILING_CONJUNCTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(.+?)\s+(?:e|and)\s+(.+)$").expect("valid trailing conjunction regex")
});

// Marcador **numerado** no início do item (`(1)`, `1)`). Os bullets em si vêm de
// [`crate::text_norm::strip_leading_marker`] — a lista de glifos não se repete aqui.
static LEADING_NUMBERED_MARKER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:\(\d+\)|\d+\))").expect("valid leading numbered marker regex")
});

fn split_top_level(text: &str, sep_char: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut buf = String::with_capacity(text.len());
    let mut paren: usize = 0;
    let mut bracket: usize = 0;
    let mut in_appositive = false;

    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        let ch = chars[i];
        if ch == '\\'
            && i + 1 < n
            && (chars[i + 1] == '('
                || chars[i + 1] == ')'
                || chars[i + 1] == '['
                || chars[i + 1] == ']')
        {
            let nxt = chars[i + 1];
            if nxt == '(' || nxt == '[' {
                paren += 1;
            } else {
                paren = paren.saturating_sub(1);
            }
            buf.push(ch);
            buf.push(nxt);
            i += 2;
            continue;
        }

        if ch == '(' {
            paren += 1;
        } else if ch == ')' && paren > 0 {
            paren -= 1;
        } else if ch == '[' {
            bracket += 1;
        } else if ch == ']' && bracket > 0 {
            bracket -= 1;
        } else if ch == sep_char && paren == 0 && bracket == 0 {
            if sep_char == ',' {
                if in_appositive {
                    buf.push(ch);
                    i += 1;
                    continue;
                }
                let prev_ch = if i > 0 { chars[i - 1] } else { '\0' };
                let next_ch = if i + 1 < n { chars[i + 1] } else { '\0' };
                let rest: String = chars[i + 1..].iter().collect();
                let rest_trimmed = rest.trim_start();
                let is_decimal = prev_ch.is_ascii_digit() && next_ch.is_ascii_digit();
                let is_app = APPOSITIVE_AFTER_COMMA_RE.is_match(rest_trimmed);
                let is_dose = DOSE_REGIMEN_AFTER_COMMA_RE.is_match(rest_trimmed);
                if is_app {
                    in_appositive = true;
                }
                if is_decimal || is_app || is_dose {
                    buf.push(ch);
                    i += 1;
                    continue;
                }
            }
            let trimmed = buf.trim().to_owned();
            if !trimmed.is_empty() {
                parts.push(trimmed);
            }
            buf.clear();
            i += 1;
            continue;
        }

        buf.push(ch);
        i += 1;
    }

    let trimmed = buf.trim().to_owned();
    if !trimmed.is_empty() {
        parts.push(trimmed);
    }
    parts
}

fn split_bullet_inline(seg: &str) -> Vec<String> {
    // Atalho: sem nenhum glifo de bullet da união não há o que separar.
    if !seg.chars().any(crate::text_norm::is_glyph_bullet) {
        return vec![seg.to_owned()];
    }
    let parts: Vec<String> = BULLET_INLINE_RE
        .split(seg)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    if parts.len() >= 2 {
        parts
    } else {
        vec![seg.to_owned()]
    }
}

fn split_numbered_inline(seg: &str) -> Vec<String> {
    let chars: Vec<char> = seg.chars().collect();
    let n = chars.len();
    let mut split_indices = Vec::new();
    let mut i = 0;

    while i < n {
        let is_start_or_ws = i == 0 || chars[i - 1].is_whitespace();
        if is_start_or_ws {
            let mut matched_len = None;
            // Case 1: ( \d{1,2} ) \s+
            if chars[i] == '(' && i + 2 < n && chars[i + 1].is_ascii_digit() {
                let mut d_len = 1;
                if chars[i + 2].is_ascii_digit() {
                    d_len = 2;
                }
                if i + 1 + d_len < n && chars[i + 1 + d_len] == ')' {
                    let after_close = i + 2 + d_len;
                    let ws_count = chars[after_close..]
                        .iter()
                        .take_while(|c| c.is_whitespace())
                        .count();
                    if ws_count > 0 {
                        matched_len = Some(2 + d_len + ws_count);
                    }
                }
            }
            // Case 2: \d{1,2} ) \s+
            if matched_len.is_none() && chars[i].is_ascii_digit() {
                let mut d_len = 1;
                if i + 1 < n && chars[i + 1].is_ascii_digit() {
                    d_len = 2;
                }
                if i + d_len < n && chars[i + d_len] == ')' {
                    let after_close = i + d_len + 1;
                    let ws_count = chars[after_close..]
                        .iter()
                        .take_while(|c| c.is_whitespace())
                        .count();
                    if ws_count > 0 {
                        matched_len = Some(d_len + 1 + ws_count);
                    }
                }
            }
            // Case 3: \d{1,2} . \s+ [letter]
            if matched_len.is_none() && chars[i].is_ascii_digit() {
                let mut d_len = 1;
                if i + 1 < n && chars[i + 1].is_ascii_digit() {
                    d_len = 2;
                }
                if i + d_len < n && chars[i + d_len] == '.' {
                    let after_dot = i + d_len + 1;
                    let ws_count = chars[after_dot..]
                        .iter()
                        .take_while(|c| c.is_whitespace())
                        .count();
                    if ws_count > 0
                        && after_dot + ws_count < n
                        && chars[after_dot + ws_count].is_alphabetic()
                    {
                        matched_len = Some(d_len + 1 + ws_count);
                    }
                }
            }

            if let Some(m_len) = matched_len {
                split_indices.push((i, i + m_len));
                i += m_len;
                continue;
            }
        }
        i += 1;
    }

    if split_indices.is_empty() {
        return vec![seg.to_owned()];
    }

    let mut parts = Vec::new();
    let mut last = 0;
    for (start, end) in split_indices {
        if start > last {
            let part: String = chars[last..start].iter().collect();
            let trimmed = part.trim().to_owned();
            if !trimmed.is_empty() {
                parts.push(trimmed);
            }
        }
        last = end;
    }
    if last < n {
        let part: String = chars[last..n].iter().collect();
        let trimmed = part.trim().to_owned();
        if !trimmed.is_empty() {
            parts.push(trimmed);
        }
    }
    parts
}

fn split_trailing_conjunction(mut items: Vec<String>) -> Vec<String> {
    if items.len() < 2 {
        return items;
    }
    if let Some(last) = items.last()
        && let Some(caps) = TRAILING_CONJUNCTION_RE.captures(last)
    {
        let g1 = caps.get(1).map_or("", |m| m.as_str()).trim();
        let g2 = caps.get(2).map_or("", |m| m.as_str()).trim();
        if !g1.is_empty() && !g2.is_empty() {
            let g1_str = g1.to_owned();
            let g2_str = g2.to_owned();
            items.pop();
            items.push(g1_str);
            items.push(g2_str);
        }
    }
    items
}

/// Evidência **estrutural** de enumeração, coletada junto com
/// [`extract_candidate_items`].
///
/// Espelha a lista `evidence` que `checks/information.py::extract_candidate_items` já
/// devolve ao Python: a decisão de `QA.CARDINALITY` precisa olhar para marcadores
/// explícitos, não para a quantidade de vírgulas (Onda 2, B25).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct EnumerationEvidence {
    /// Duas ou mais linhas visíveis — separadas por `<li>`, `<br>` ou quebra de linha.
    multi_line: bool,
    /// Ponto e vírgula de topo separando itens.
    semicolon: bool,
    /// Glifo de bullet (ver [`crate::text_norm::is_glyph_bullet`]) separando itens na linha.
    bullet_marker: bool,
    /// Numeração inline (`1.`, `2)`, `(3)`) separando itens na linha.
    numbered_marker: bool,
}

impl EnumerationEvidence {
    /// `true` quando há marcador ou separador **explícito** de enumeração.
    ///
    /// Vírgula nunca conta: `"Dor retroesternal, em aperto, com irradiação"` é uma resposta
    /// de um único conceito com vírgulas de aposição, não uma lista.
    fn is_explicit(self) -> bool {
        self.multi_line || self.semicolon || self.bullet_marker || self.numbered_marker
    }
}

/// Extract candidate recalled items deterministically (`checks/information.py::extract_candidate_items`).
#[must_use]
pub fn extract_candidate_items(raw_html: &str) -> Vec<String> {
    let (items, _) = extract_with_evidence(raw_html);
    items
}

/// Extração de candidatos + evidência estrutural (ver [`EnumerationEvidence`]).
fn extract_with_evidence(raw_html: &str) -> (Vec<String>, EnumerationEvidence) {
    let mut evidence = EnumerationEvidence::default();
    let (text, _, _) = parse_html_text(raw_html);
    if text.is_empty() {
        return (Vec::new(), evidence);
    }
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    evidence.multi_line = lines.len() >= 2;

    // Stage 1: split each line by top-level semicolons
    let mut stage1 = Vec::new();
    for ln in lines {
        let semi_parts = split_top_level(ln, ';');
        if semi_parts.len() >= 2 {
            evidence.semicolon = true;
            stage1.extend(semi_parts);
        } else {
            stage1.push(ln.to_owned());
        }
    }

    // Stage 2: inline bullet / numbered markers
    let mut stage2 = Vec::new();
    for seg in stage1 {
        let bullet_parts = split_bullet_inline(&seg);
        if bullet_parts.len() >= 2 {
            evidence.bullet_marker = true;
            stage2.extend(bullet_parts);
            continue;
        }
        let num_parts = split_numbered_inline(&seg);
        if num_parts.len() >= 2 {
            evidence.numbered_marker = true;
            stage2.extend(num_parts);
            continue;
        }
        stage2.push(seg);
    }

    // Stage 3: safe top-level commas
    if stage2.len() == 1 && !COMPARISON_RE.is_match(&stage2[0]) {
        let comma_parts = split_top_level(&stage2[0], ',');
        if comma_parts.len() >= 2 {
            stage2 = split_trailing_conjunction(comma_parts);
        }
    }

    let mut cleaned: Vec<String> = stage2
        .into_iter()
        .map(|item| {
            // Uma única passada, alternando bullet **ou** marcador numerado — mesma
            // semântica da antiga regex `^(?:[•\-–—*]|\(\d+\)|\d+\))\s*`.
            let (rest, marker) = crate::text_norm::strip_leading_marker(&item);
            match marker {
                Some(_) => rest.trim().to_owned(),
                None => LEADING_NUMBERED_MARKER_RE
                    .replace(rest, "")
                    .trim()
                    .to_owned(),
            }
        })
        .filter(|s| !s.is_empty())
        .collect();

    if cleaned.is_empty() {
        cleaned.push(html_to_visible_text(raw_html));
    }
    (cleaned, evidence)
}

fn has_bare_number(cleaned_num: &str) -> bool {
    for mat in NUMBER_TOKEN_RE.find_iter(cleaned_num) {
        let rest = cleaned_num[mat.end()..].trim_start();
        if !ALLOWED_UNIT_PREFIX_RE.is_match(rest) {
            return true;
        }
    }
    false
}

/// Check a QA card (stem + answer pair), matching `checks/qa.py::check_qa`.
#[must_use]
pub fn check_qa(enunciado: &str, resposta: &str, row: Option<usize>) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let visible_stem = html_to_visible_text(enunciado);
    let visible_answer = html_to_visible_text(resposta);

    // ── QA.ID_IN_TEXT ────────────────────────────────────────────────────────
    if crate::id_leak::has_leading_id_leak(&visible_stem) {
        let mut d = Diagnostic::new(
            anki_domain::DiagnosticCode::from_static("QA.ID_IN_TEXT"),
            Severity::Critical,
            "anki-qa-designer",
            "Visible card ID found in question stem. IDs must reside in tags or metadata, not in the prompt.",
        )
        .with_field("Enunciado");
        if let Some(r) = row {
            d = d.with_row(r);
        }
        diags.push(d);
    }

    // ── QA.YESNO ─────────────────────────────────────────────────────────────
    if YESNO_ANSWER_RE.is_match(&visible_answer) {
        let mut d = Diagnostic::new(
            anki_domain::DiagnosticCode::from_static("QA.YESNO"),
            Severity::Critical,
            "anki-qa-designer",
            format!("Answer is a yes/no value: '{}'", visible_answer.trim()),
        )
        .with_field("Resposta")
        .with_suggestion("Rephrase as a direct-recall question with a factual answer");
        if let Some(r) = row {
            d = d.with_row(r);
        }
        diags.push(d);
    }
    if YESNO_STEM_RE.is_match(&visible_stem) {
        let mut d = Diagnostic::new(
            anki_domain::DiagnosticCode::from_static("QA.YESNO"),
            Severity::Critical,
            "anki-qa-designer",
            "Stem appears to be a true/false or yes/no question",
        )
        .with_field("Enunciado");
        if let Some(r) = row {
            d = d.with_row(r);
        }
        diags.push(d);
    }

    // ── QA.MC_EMBED ──────────────────────────────────────────────────────────
    if MC_RE.is_match(&visible_stem) {
        let mut d = Diagnostic::new(
            anki_domain::DiagnosticCode::from_static("QA.MC_EMBED"),
            Severity::Critical,
            "anki-qa-designer",
            "Stem contains embedded multiple-choice options (a)/b)/c) pattern)",
        )
        .with_field("Enunciado")
        .with_suggestion("Remove options — the stem must be a free-recall question");
        if let Some(r) = row {
            d = d.with_row(r);
        }
        diags.push(d);
    }

    // ── QA.CARDINALITY ───────────────────────────────────────────────────────
    let (candidate_items, evidence) = extract_with_evidence(resposta);
    let item_count = candidate_items.len();
    // ≥3 itens **e** marcador/separador enumerativo explícito (Onda 2, B25). Contagem de
    // vírgulas não basta: "Dor retroesternal, em aperto, com irradiação para o braço
    // esquerdo" é uma resposta de um único conceito com vírgulas de aposição.
    if item_count >= 3 && evidence.is_explicit() {
        let mut d = Diagnostic::new(
            anki_domain::DiagnosticCode::from_static("QA.CARDINALITY"),
            Severity::Critical,
            "anki-qa-designer",
            format!("Answer appears to enumerate ~{item_count} items (≥3) — review unit granularity"),
        )
        .with_field("Resposta")
        .with_suggestion(
            "Revise a unidade: separe recuperações independentes em cartões próprios ou mova detalhes opcionais para Notas Adicionais (uma lacuna Cloze única pode ocultar lista).",
        );
        if let Some(r) = row {
            d = d.with_row(r);
        }
        diags.push(d);
    }

    // ── QA.CONTEXT_WARN ──────────────────────────────────────────────────────
    if !CONTEXT_MARKERS_RE.is_match(&visible_stem) {
        let mut d = Diagnostic::new(
            anki_domain::DiagnosticCode::from_static("QA.CONTEXT_WARN"),
            Severity::Warn,
            "anki-qa-designer",
            "Stem lacks explicit clinical context (patient/guideline/condition/population)",
        )
        .with_field("Enunciado")
        .with_suggestion("Add: population, source guideline, or clinical scenario");
        if let Some(r) = row {
            d = d.with_row(r);
        }
        diags.push(d);
    }

    // ── QA.NUMBER_UNIT ───────────────────────────────────────────────────────
    let cleaned_ordinals = ORDINAL_CLASS_RE.replace_all(&visible_answer, "");
    let cleaned_num = IDENTIFIER_RE.replace_all(&cleaned_ordinals, "");
    if cleaned_num.chars().any(|c| c.is_ascii_digit())
        && !DIRECTION_OP_RE.is_match(&visible_answer)
        && has_bare_number(&cleaned_num)
    {
        let mut d = Diagnostic::new(
            anki_domain::DiagnosticCode::from_static("QA.NUMBER_UNIT"),
            Severity::Warn,
            "anki-qa-designer",
            "Answer contains a bare number without unit or direction (≥/≤/>/< + unit)",
        )
        .with_field("Resposta")
        .with_suggestion("e.g. '≥ 140 mmHg' not '140'");
        if let Some(r) = row {
            d = d.with_row(r);
        }
        diags.push(d);
    }

    // ── CARD.SPLIT_QA_MULTI_QUESTION (one obligation per stem) ───────────────
    let question_marks = visible_stem.chars().filter(|&c| c == '?').count();
    let coordinated_q = COORDINATED_Q_RE.is_match(&visible_stem);
    if question_marks > 1 || coordinated_q {
        let mut d = Diagnostic::new(
            anki_domain::DiagnosticCode::from_static("CARD.SPLIT_QA_MULTI_QUESTION"),
            Severity::Warn,
            "anki-card-generator",
            "Enunciado com mais de uma pergunta: o cartão cobra duas obrigações",
        )
        .with_field("Enunciado")
        .with_suggestion(
            "SPLIT: uma recuperação por cartão; separe perguntas independentes e mova detalhes opcionais para Notas Adicionais.",
        );
        if let Some(r) = row {
            d = d.with_row(r);
        }
        diags.push(d);
    }

    // ── CARD.TOO_LONG_STEM / CARD.TOO_LONG_ANSWER (advisory length signal) ───
    let stem_len = visible_len(enunciado);
    if stem_len > 200 {
        let threshold = if stem_len > 350 { 350 } else { 200 };
        let mut d = Diagnostic::new(
            anki_domain::DiagnosticCode::from_static("CARD.TOO_LONG_STEM"),
            Severity::Warn,
            "anki-card-generator",
            format!(
                "Enunciado extenso ({stem_len} > {threshold} caracteres; sinal de revisão, não prova de múltipla cobrança)"
            ),
        )
        .with_field("Enunciado")
        .with_suggestion(
            "Revise se há duas cobranças independentes; preserve o contexto necessário para tornar a resposta única e mova explicação opcional para Notas Adicionais.",
        );
        if let Some(r) = row {
            d = d.with_row(r);
        }
        diags.push(d);
    }

    let ans_len = visible_len(resposta);
    if ans_len > 240 {
        let threshold = if ans_len > 400 { 400 } else { 240 };
        let mut d = Diagnostic::new(
            anki_domain::DiagnosticCode::from_static("CARD.TOO_LONG_ANSWER"),
            Severity::Warn,
            "anki-card-generator",
            format!(
                "Resposta extenso ({ans_len} > {threshold} caracteres; sinal de revisão, não prova de múltipla cobrança)"
            ),
        )
        .with_field("Resposta")
        .with_suggestion(
            "Revise se a resposta agrupa itens independentes ou detalhe opcional; preserve o contexto necessário e mova explicação opcional para Notas Adicionais.",
        );
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
    fn test_id_in_text() {
        let diags = check_qa(
            "ANKI-123: Qual o tratamento do choque?",
            "Noradrenalina",
            Some(1),
        );
        assert!(
            diags
                .iter()
                .any(|d| d.code == "QA.ID_IN_TEXT" && d.severity == Severity::Critical)
        );

        let diags2 = check_qa("B01-QA-005 Qual o achado?", "Sinal de Kernig", None);
        assert!(diags2.iter().any(|d| d.code == "QA.ID_IN_TEXT"));
    }

    #[test]
    fn test_yesno_answer_and_stem() {
        let diags = check_qa("O paciente deve receber alta?", "Sim", Some(2));
        assert!(
            diags
                .iter()
                .any(|d| d.code == "QA.YESNO" && d.field.as_deref() == Some("Resposta"))
        );

        let diags_stem = check_qa(
            "É verdadeiro afirmar que o paciente tem DM?",
            "Glicemia de jejum elevada",
            None,
        );
        assert!(
            diags_stem
                .iter()
                .any(|d| d.code == "QA.YESNO" && d.field.as_deref() == Some("Enunciado"))
        );
    }

    #[test]
    fn test_mc_embed() {
        let diags = check_qa(
            "Qual o diagnóstico? a) AVC b) AIT c) Enxaqueca",
            "AVC",
            Some(3),
        );
        assert!(
            diags
                .iter()
                .any(|d| d.code == "QA.MC_EMBED" && d.severity == Severity::Critical)
        );
    }

    #[test]
    fn test_cardinality_three_or_more() {
        let d1 = check_qa(
            "Em paciente adulto, quais os achados?",
            "Item A; Item B; Item C",
            None,
        );
        assert!(
            d1.iter()
                .any(|d| d.code == "QA.CARDINALITY" && d.severity == Severity::Critical)
        );

        let d2 = check_qa(
            "Em paciente adulto, quais os achados?",
            "<ul><li>A</li><li>B</li><li>C</li></ul>",
            None,
        );
        assert!(d2.iter().any(|d| d.code == "QA.CARDINALITY"));

        let d3 = check_qa(
            "Em paciente adulto, quais os achados?",
            "1. alfa; 2. beta; 3. gama",
            None,
        );
        assert!(d3.iter().any(|d| d.code == "QA.CARDINALITY"));
    }

    /// B25: contagem de vírgulas não sustenta um diagnóstico CRITICAL.
    ///
    /// `"alfa, beta e gama"` era positivo até a Onda 2; passa a negativo porque não há
    /// marcador enumerativo explícito (nem `;`, nem bullet, nem numeração, nem quebra de
    /// linha) — é exatamente a forma das respostas de um único conceito com vírgulas de
    /// aposição descritas no defeito.
    #[test]
    fn test_cardinality_requires_explicit_enumerative_marker() {
        let negative = [
            "alfa, beta e gama",
            "Dor retroesternal, em aperto, com irradiação para o braço esquerdo",
            "Cefaleia unilateral, pulsátil, intensidade moderada a grave, piora com atividade física, náusea e fotofobia",
        ];
        for answer in negative {
            let diags = check_qa("Em paciente adulto, quais os achados?", answer, None);
            assert!(
                !diags.iter().any(|d| d.code == "QA.CARDINALITY"),
                "vírgula não é marcador enumerativo: {answer}"
            );
        }

        let positive = [
            "alfa; beta; gama",
            "alfa • beta • gama",
            "• alfa<br>• beta<br>• gama",
            "1. alfa 2. beta 3. gama",
            "alfa<br>beta<br>gama",
        ];
        for answer in positive {
            let diags = check_qa("Em paciente adulto, quais os achados?", answer, None);
            assert!(
                diags.iter().any(|d| d.code == "QA.CARDINALITY"),
                "marcador enumerativo explícito deve emitir: {answer}"
            );
        }
    }

    #[test]
    fn test_cardinality_continuations_not_false_positive() {
        let answers = [
            "5 mg, IV, a cada 8 horas",
            "500 mg, 12/12h, por 7 dias",
            "Cefaleia (unilateral, pulsátil, moderada a grave)",
            "Glossoplegia, caracterizada por paralisia da língua, com desvio ipsilateral, sem disartria",
            "glossoplegia, caracterizada por atrofia, fraqueza e desvio",
            "Pressão 140&#124;90 mmHg &#124; pulso 80 bpm",
        ];
        for ans in answers {
            let diags = check_qa("Em paciente adulto, qual a conduta ou achado?", ans, None);
            assert!(
                !diags.iter().any(|d| d.code == "QA.CARDINALITY"),
                "False positive QA.CARDINALITY on: {ans}"
            );
        }
    }

    #[test]
    fn test_context_warn() {
        let diags_no_ctx = check_qa("Qual é a dose inicial?", "10 mg", None);
        assert!(
            diags_no_ctx
                .iter()
                .any(|d| d.code == "QA.CONTEXT_WARN" && d.severity == Severity::Warn)
        );

        let diags_with_ctx = check_qa(
            "Em paciente adulto com DRC estágio 3, qual a conduta?",
            "IECA",
            None,
        );
        assert!(!diags_with_ctx.iter().any(|d| d.code == "QA.CONTEXT_WARN"));
    }

    #[test]
    fn test_number_unit() {
        let diags_bare = check_qa(
            "Em paciente com suspeita de HAS, qual a PA sistólica?",
            "140",
            None,
        );
        assert!(
            diags_bare
                .iter()
                .any(|d| d.code == "QA.NUMBER_UNIT" && d.severity == Severity::Warn)
        );

        let diags_unit = check_qa(
            "Em paciente com suspeita de HAS, qual a PA sistólica?",
            "&gt;= 140 mmHg",
            None,
        );
        assert!(!diags_unit.iter().any(|d| d.code == "QA.NUMBER_UNIT"));

        let diags_id = check_qa(
            "Em paciente com neoplasia, qual o gene afetado?",
            "CYP2D6",
            None,
        );
        assert!(!diags_id.iter().any(|d| d.code == "QA.NUMBER_UNIT"));
    }

    #[test]
    fn test_split_qa_multi_question() {
        let diags_multi = check_qa(
            "Em paciente adulto, qual o diagnóstico? E qual o tratamento?",
            "AIT e AAS",
            None,
        );
        assert!(
            diags_multi
                .iter()
                .any(|d| d.code == "CARD.SPLIT_QA_MULTI_QUESTION" && d.severity == Severity::Warn)
        );

        let diags_single = check_qa(
            "Em paciente adulto com coreia, qual o fármaco de primeira linha?",
            "Tetrabenazina",
            None,
        );
        assert!(
            !diags_single
                .iter()
                .any(|d| d.code == "CARD.SPLIT_QA_MULTI_QUESTION")
        );
    }

    #[test]
    fn test_too_long_stem_and_answer() {
        let long_stem = format!(
            "Em paciente adulto com DRC, {} qual a conduta?",
            "x ".repeat(150)
        );
        let diags = check_qa(&long_stem, "IECA", None);
        assert!(
            diags
                .iter()
                .any(|d| d.code == "CARD.TOO_LONG_STEM" && d.severity == Severity::Warn)
        );
        assert!(
            !diags
                .iter()
                .any(|d| d.code == "CARD.TOO_LONG_STEM" && d.severity == Severity::Critical)
        );
    }

    #[test]
    fn test_golden_qa_cases() {
        // GOOD: self-contained stem with population and source
        let d_good = check_qa(
            "Em paciente com DRC estágio 3b, segundo a Diretriz SBC 2025, qual é o anti-hipertensivo de primeira linha?",
            "IECA",
            None,
        );
        assert!(d_good.is_empty() || !d_good.iter().any(|d| d.severity == Severity::Critical));

        // BAD: answer has 3+ items
        let d_card = check_qa(
            "Em paciente adulto, quais são os três critérios diagnósticos da síndrome X?",
            "Critério A; Critério B; Critério C",
            None,
        );
        assert!(
            d_card
                .iter()
                .any(|d| d.code == "QA.CARDINALITY" && d.severity == Severity::Critical)
        );

        // BAD: yes/no answer
        let d_yesno = check_qa(
            "Em paciente gestante, o IECA é contraindicado?",
            "Sim",
            None,
        );
        assert!(
            d_yesno
                .iter()
                .any(|d| d.code == "QA.YESNO" && d.severity == Severity::Critical)
        );

        // BAD: multiple choice options embedded in stem
        let d_mc = check_qa(
            "Em paciente adulto, qual o mecanismo de ação? a) IECA b) BRA c) BCC",
            "BRA",
            None,
        );
        assert!(
            d_mc.iter()
                .any(|d| d.code == "QA.MC_EMBED" && d.severity == Severity::Critical)
        );
    }

    #[test]
    fn test_mc_embed_negative_portador_and_hepatite() {
        let d1 = check_qa(
            "Em paciente portador(a) de hepatite (B) crônica, qual o antiviral de escolha?",
            "Tenofovir",
            None,
        );
        assert!(!d1.iter().any(|d| d.code == "QA.MC_EMBED"));

        let d2 = check_qa(
            "Paciente portador(a) de síndrome nefrótica, qual o primeiro passo?",
            "Corticoide",
            None,
        );
        assert!(!d2.iter().any(|d| d.code == "QA.MC_EMBED"));

        let d3 = check_qa(
            "Sorologia para hepatite (B) positiva em adulto jovem...",
            "Vacinação prévia",
            None,
        );
        assert!(!d3.iter().any(|d| d.code == "QA.MC_EMBED"));

        // Sequential options must trigger QA.MC_EMBED
        let d_bracket = check_qa(
            "Qual a conduta? [a] Furosemida [b] Espironolactona",
            "Furosemida",
            None,
        );
        assert!(d_bracket.iter().any(|d| d.code == "QA.MC_EMBED"));

        let d_paren = check_qa(
            "Qual a conduta? (a) Furosemida (b) Espironolactona",
            "Furosemida",
            None,
        );
        assert!(d_paren.iter().any(|d| d.code == "QA.MC_EMBED"));
    }

    #[test]
    fn test_context_case_sensitive_acronyms() {
        // Lowercase "em" must NOT satisfy clinical context requirement
        let d_em = check_qa("Qual é a dose em jejum?", "10 mg", None);
        assert!(d_em.iter().any(|d| d.code == "QA.CONTEXT_WARN"));

        // Uppercase "EM" (Esclerose Múltipla) MUST satisfy clinical context requirement
        let d_em_upper = check_qa(
            "Na EM remitente-recorrente, qual a terapia de manutenção?",
            "Ocrelizumabe",
            None,
        );
        assert!(!d_em_upper.iter().any(|d| d.code == "QA.CONTEXT_WARN"));

        // Uppercase "AVC", "DRC", "ICC" satisfy context
        let d_avc = check_qa(
            "No AVC isquêmico agudo, qual a janela de trombólise?",
            "4,5 horas",
            None,
        );
        assert!(!d_avc.iter().any(|d| d.code == "QA.CONTEXT_WARN"));
    }

    #[test]
    fn test_visible_stem_and_answer_evaluations() {
        // QA.ID_IN_TEXT inside HTML tags
        let d_id = check_qa("<p>ANKI-123: Qual o tratamento?</p>", "Oxigênio", None);
        assert!(d_id.iter().any(|d| d.code == "QA.ID_IN_TEXT"));

        // QA.YESNO inside HTML tags
        let d_yesno = check_qa(
            "Em paciente adulto com dor, indicar repouso?",
            "<b>Sim</b>",
            None,
        );
        assert!(d_yesno.iter().any(|d| d.code == "QA.YESNO"));

        // QA.NUMBER_UNIT inside HTML tags
        let d_num = check_qa(
            "Em paciente com suspeita de HAS, qual a PA?",
            "<b>140</b>",
            None,
        );
        assert!(d_num.iter().any(|d| d.code == "QA.NUMBER_UNIT"));

        // QA.NUMBER_UNIT with entity inside HTML tags should pass
        let d_num_pass = check_qa(
            "Em paciente com suspeita de HAS, qual a PA?",
            "<p>&gt;= 140 mmHg</p>",
            None,
        );
        assert!(!d_num_pass.iter().any(|d| d.code == "QA.NUMBER_UNIT"));

        // CARD.SPLIT_QA_MULTI_QUESTION inside HTML tags
        let d_split = check_qa(
            "<p>Em paciente adulto, qual o diagnóstico? E qual o tratamento?</p>",
            "AIT",
            None,
        );
        assert!(
            d_split
                .iter()
                .any(|d| d.code == "CARD.SPLIT_QA_MULTI_QUESTION")
        );
    }
}
