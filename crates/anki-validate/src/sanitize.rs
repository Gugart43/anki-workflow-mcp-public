//! Deterministic field sanitization (`core/sanitize.py`).

use crate::vocab::is_allowed_tag;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

static TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<(/?)([a-zA-Z][a-zA-Z0-9]*)((\s+[^>]*?)?)(/?)>").expect("valid tag regex")
});

static COMMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->").expect("valid comment regex"));

static ENTITY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&(#\d+|#x[0-9a-fA-F]+|[a-zA-Z]+);").expect("valid entity regex"));

static BOLD_STAR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\*\*(.+?)\*\*").expect("valid bold star regex"));

static BOLD_UNDER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"__(.+?)__").expect("valid bold under regex"));

const AUTO_CLOSE_TAGS: &[&str] = &[
    "b", "strong", "i", "em", "u", "s", "small", "span", "code", "mark", "sub", "sup",
];

fn convert_markdown_italic(input: &str, fixes: &mut Vec<String>) -> String {
    // Equivalent to: r"(?<!\*)\*([^\*\s][^\*]*?[^\*\s]|[^\*\s])\*(?!\*)"
    let bytes = input.as_bytes();
    let n = bytes.len();
    let mut out = String::with_capacity(n);
    let mut i = 0;

    while i < n {
        if bytes[i] == b'*' {
            let prev_is_star = i > 0 && bytes[i - 1] == b'*';
            let next_is_star = i + 1 < n && bytes[i + 1] == b'*';
            if !prev_is_star && !next_is_star && i + 1 < n {
                // Find first closing single '*' (not '**') without crossing whitespace-only boundaries
                let mut j = i + 1;
                let mut found = None;
                while j < n {
                    if bytes[j] == b'*' {
                        let close_next_star = j + 1 < n && bytes[j + 1] == b'*';
                        if close_next_star {
                            // Hit '**' before closing '*' -> abort this candidate
                            break;
                        }
                        if j > i + 1 {
                            let inner = &input[i + 1..j];
                            let first_ws = inner.chars().next().is_some_and(char::is_whitespace);
                            let last_ws = inner.chars().last().is_some_and(char::is_whitespace);
                            if !first_ws && !last_ws {
                                found = Some(j);
                                break;
                            }
                        }
                        break;
                    }
                    j += 1;
                }
                if let Some(end_star) = found {
                    fixes.push("converted_markdown_italic".to_owned());
                    out.push_str("<i>");
                    out.push_str(&input[i + 1..end_star]);
                    out.push_str("</i>");
                    i = end_star + 1;
                    continue;
                }
            }
        }
        let ch = input[i..].chars().next().expect("valid utf8 boundary");
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

fn escape_bare_ampersands(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for m in ENTITY_RE.find_iter(text) {
        if m.start() > last {
            out.push_str(&text[last..m.start()].replace('&', "&amp;"));
        }
        out.push_str(m.as_str());
        last = m.end();
    }
    if last < text.len() {
        out.push_str(&text[last..].replace('&', "&amp;"));
    }
    out
}

/// Sanitize a single card field value, matching `core/sanitize.py::sanitize_field`.
#[must_use]
pub fn sanitize_field(
    raw_value: &str,
    is_images_field: bool,
    convert_markdown: bool,
) -> (String, Vec<String>) {
    if raw_value.is_empty() {
        return (String::new(), Vec::new());
    }

    let mut value = raw_value.to_owned();
    let mut fixes: Vec<String> = Vec::new();

    // 1. Pipe escape
    if value.contains('|') {
        value = value.replace('|', "&#124;");
        fixes.push("escaped_pipe".to_owned());
    }

    // 2. Normalize raw physical newlines to <br>
    if value.contains('\r') || value.contains('\n') {
        value = value.replace("\r\n", "<br>").replace(['\r', '\n'], "<br>");
        fixes.push("normalized_newlines_to_br".to_owned());
    }

    // 3. Optional Markdown bold and italic conversion
    if convert_markdown {
        value = BOLD_STAR_RE
            .replace_all(&value, |caps: &regex::Captures<'_>| {
                fixes.push("converted_markdown_bold".to_owned());
                format!("<b>{}</b>", &caps[1])
            })
            .into_owned();
        value = BOLD_UNDER_RE
            .replace_all(&value, |caps: &regex::Captures<'_>| {
                fixes.push("converted_markdown_bold".to_owned());
                format!("<b>{}</b>", &caps[1])
            })
            .into_owned();
        value = convert_markdown_italic(&value, &mut fixes);
    }

    // 4. Mask comments temporarily
    let mut comments: Vec<String> = Vec::new();
    value = COMMENT_RE
        .replace_all(&value, |caps: &regex::Captures<'_>| {
            let idx = comments.len();
            comments.push(caps[0].to_owned());
            format!("__COMMENT_TOKEN_{idx}__")
        })
        .into_owned();

    // Tokenize allowed HTML tags vs plain text
    let mut tokens: Vec<(bool, String)> = Vec::new();
    let mut last_idx = 0;

    for caps in TAG_RE.captures_iter(&value) {
        let full = caps.get(0).expect("group 0");
        if full.start() > last_idx {
            tokens.push((false, value[last_idx..full.start()].to_owned()));
        }
        let tag_name = caps[2].to_ascii_lowercase();
        let is_allowed = is_allowed_tag(&tag_name, is_images_field);
        tokens.push((is_allowed, full.as_str().to_owned()));
        last_idx = full.end();
    }
    if last_idx < value.len() {
        tokens.push((false, value[last_idx..].to_owned()));
    }

    // 5. Process non-tag tokens and track paired tags
    let mut out_tokens: Vec<String> = Vec::new();
    let mut tag_stack: Vec<String> = Vec::new();

    for (is_tag, mut text) in tokens {
        if is_tag {
            out_tokens.push(text.clone());
            if let Some(caps) = TAG_RE.captures(&text) {
                let is_close = !caps[1].is_empty();
                let tname = caps[2].to_ascii_lowercase();
                let is_self_closing =
                    !caps[5].is_empty() || matches!(tname.as_str(), "br" | "hr" | "img");
                if !is_self_closing {
                    if is_close {
                        if tag_stack.last().is_some_and(|top| top == &tname) {
                            tag_stack.pop();
                        } else if tag_stack.contains(&tname) {
                            while let Some(top) = tag_stack.last() {
                                if top == &tname {
                                    tag_stack.pop();
                                    break;
                                }
                                tag_stack.pop();
                            }
                        }
                    } else if AUTO_CLOSE_TAGS.contains(&tname.as_str()) {
                        tag_stack.push(tname);
                    }
                }
            }
        } else {
            if text.contains("<=") {
                text = text.replace("<=", "&le;");
                fixes.push("escaped_inequality_le".to_owned());
            }
            if text.contains(">=") {
                text = text.replace(">=", "&ge;");
                fixes.push("escaped_inequality_ge".to_owned());
            }
            if text.contains('<') {
                text = text.replace('<', "&lt;");
                fixes.push("escaped_inequality_lt".to_owned());
            }
            if text.contains('>') {
                text = text.replace('>', "&gt;");
                fixes.push("escaped_inequality_gt".to_owned());
            }
            if text.contains('&') {
                let stripped = ENTITY_RE.replace_all(&text, "");
                if stripped.contains('&') {
                    text = escape_bare_ampersands(&text);
                    fixes.push("escaped_bare_amp".to_owned());
                }
            }
            out_tokens.push(text);
        }
    }

    // 6. Auto-close remaining open tags in reverse order
    while let Some(unclosed) = tag_stack.pop() {
        out_tokens.push(format!("</{unclosed}>"));
        fixes.push(format!("auto_closed_tag_{unclosed}"));
    }

    let mut result = out_tokens.concat();
    for (i, comm) in comments.into_iter().enumerate() {
        result = result.replace(&format!("__COMMENT_TOKEN_{i}__"), &comm);
    }

    let mut seen = HashSet::new();
    let deduped_fixes: Vec<String> = fixes
        .into_iter()
        .filter(|f| seen.insert(f.clone()))
        .collect();

    (result, deduped_fixes)
}
