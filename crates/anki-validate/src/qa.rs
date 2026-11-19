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

static ID_IN_TEXT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*[\(\[]?(?:ANKI-\d+|B\d+-[A-Z]+-\d+)[\)\]]?").expect("valid id in text regex")
});

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
    Regex::new(r"(?is)[(\[]\s*[a-d]\s*[)\]]|\ba\s*\)\s*\S+.{0,40}\bb\s*\)")
        .expect("valid mc embed regex")
});

static CONTEXT_MARKERS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(paciente|patient|segundo|conforme|diretriz|guideline|critério|criteria|portador|com\s+(diagnóstico|suspeita)|população|adulto|criança|pediátrico|gestante|idoso|IRC|DRC|ICC|ELA|EM|AVC|SNC)\b",
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

static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("valid tag regex"));

static BR_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").expect("valid br tag regex"));

static WHITESPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("valid whitespace regex"));

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

static BULLET_INLINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:\A|\s)[•]\s+").expect("valid bullet inline regex"));

static TRAILING_CONJUNCTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(.+?)\s+(?:e|and)\s+(.+)$").expect("valid trailing conjunction regex")
});

static LEADING_MARKER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[•\-–—*]|\(\d+\)|\d+\))\s*").expect("valid leading marker regex")
});

/// Decode common HTML entity name or numeric token.
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

/// Decode HTML character and entity references in text.
#[must_use]
pub fn decode_html_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let bytes = input.as_bytes();
    let n = bytes.len();

    while i < n {
        if bytes[i] == b'&'
            && let Some(semi_pos) = bytes[i + 1..].iter().position(|&b| b == b';')
        {
            let entity = &input[i + 1..i + 1 + semi_pos];
            if let Some(c) = decode_entity_token(entity) {
                out.push(c);
                i += 2 + semi_pos;
                continue;
            }
        }
        let ch = input[i..].chars().next().expect("valid utf8");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
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

/// Parse HTML text preserving block-level line breaks and decoding entities.
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

/// Return single-line visible text with HTML tags removed and entities decoded.
#[must_use]
pub fn html_to_visible_text(raw: &str) -> String {
    let (text, _, _) = parse_html_text(raw);
    let collapsed = WHITESPACE_RE.replace_all(&text, " ");
    collapsed.trim().to_owned()
}

/// Unwrap `{{cN::target(::hint)?}}` keeping target content and supporting nested LaTeX braces.
#[must_use]
pub fn strip_cloze_markers(text: &str) -> String {
    if !text.contains("{{c") {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    while i < n {
        if i + 3 < n
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
                        for &ch in &chars[content_start..target_end] {
                            out.push(ch);
                        }
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
                    } else if chars[k] == '}' && (k == 0 || chars[k - 1] != '\\') && brace_depth > 0
                    {
                        brace_depth -= 1;
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
    out
}

/// Visible character length matching `core/vocab.py::visible_len`.
#[must_use]
pub fn visible_len(text: &str) -> usize {
    let uncloaked = strip_cloze_markers(text);
    let br_replaced = BR_TAG_RE.replace_all(&uncloaked, " ");
    let tag_stripped = TAG_RE.replace_all(&br_replaced, "");
    let unescaped = decode_html_entities(&tag_stripped);
    let normalized = WHITESPACE_RE.replace_all(&unescaped, " ");
    normalized.trim().chars().count()
}

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
    if !seg.contains('•') {
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

/// Extract candidate recalled items deterministically (`checks/information.py::extract_candidate_items`).
#[must_use]
pub fn extract_candidate_items(raw_html: &str) -> Vec<String> {
    let (text, _, _) = parse_html_text(raw_html);
    if text.is_empty() {
        return Vec::new();
    }
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    // Stage 1: split each line by top-level semicolons
    let mut stage1 = Vec::new();
    for ln in lines {
        let semi_parts = split_top_level(ln, ';');
        if semi_parts.len() >= 2 {
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
            stage2.extend(bullet_parts);
            continue;
        }
        let num_parts = split_numbered_inline(&seg);
        if num_parts.len() >= 2 {
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
            let replaced = LEADING_MARKER_RE.replace(&item, "");
            replaced.trim().to_owned()
        })
        .filter(|s| !s.is_empty())
        .collect();

    if cleaned.is_empty() {
        cleaned.push(html_to_visible_text(raw_html));
    }
    cleaned
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

    // ── QA.ID_IN_TEXT ────────────────────────────────────────────────────────
    if ID_IN_TEXT_RE.is_match(enunciado) {
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
    if YESNO_ANSWER_RE.is_match(resposta) {
        let mut d = Diagnostic::new(
            anki_domain::DiagnosticCode::from_static("QA.YESNO"),
            Severity::Critical,
            "anki-qa-designer",
            format!("Answer is a yes/no value: '{}'", resposta.trim()),
        )
        .with_field("Resposta")
        .with_suggestion("Rephrase as a direct-recall question with a factual answer");
        if let Some(r) = row {
            d = d.with_row(r);
        }
        diags.push(d);
    }
    if YESNO_STEM_RE.is_match(enunciado) {
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
    if MC_RE.is_match(enunciado) {
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
    let candidate_items = extract_candidate_items(resposta);
    let item_count = candidate_items.len();
    if item_count >= 3 {
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
    if !CONTEXT_MARKERS_RE.is_match(enunciado) {
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
    let cleaned_ordinals = ORDINAL_CLASS_RE.replace_all(resposta, "");
    let cleaned_num = IDENTIFIER_RE.replace_all(&cleaned_ordinals, "");
    if cleaned_num.chars().any(|c| c.is_ascii_digit())
        && !DIRECTION_OP_RE.is_match(resposta)
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
    let stem_text = TAG_RE.replace_all(enunciado, " ");
    let question_marks = stem_text.chars().filter(|&c| c == '?').count();
    let coordinated_q = COORDINATED_Q_RE.is_match(&stem_text);
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
            "alfa, beta e gama",
            None,
        );
        assert!(d3.iter().any(|d| d.code == "QA.CARDINALITY"));
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
}
