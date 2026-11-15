//! Fail-closed active-content boundary for Rust-first outputs.
//! Legacy formatting helpers remain unchanged for differential compatibility.
use regex::Regex;
use std::sync::LazyLock;

static TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<([a-zA-Z][a-zA-Z0-9]*)((?:"[^"]*"|'[^']*'|[^<>"'])*)>"#)
        .expect("HTML boundary regex")
});

fn safe_style(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    !["url", "expression", "import", "behavior", "binding"]
        .iter()
        .any(|s| lower.contains(s))
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || ":;#%.,()-".contains(c)
        })
}

fn safe_attribute(tag: &str, name: &str, value: &str) -> bool {
    match name {
        "class" | "title" | "alt" => !value.contains(['<', '>']),
        "style" => safe_style(value),
        "colspan" | "rowspan" => !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()),
        "scope" => matches!(value, "row" | "col" | "rowgroup" | "colgroup"),
        // No remote, data, protocol-relative or traversal sources, encoded or literal.
        "src" if tag == "img" => {
            !value.is_empty()
                && !value.contains("..")
                && value
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-.".contains(&c))
        }
        _ => false,
    }
}

pub(crate) fn has_unsafe_html(value: &str) -> bool {
    for caps in TAG.captures_iter(value) {
        let tag = caps[1].to_ascii_lowercase();
        if matches!(
            tag.as_str(),
            "script"
                | "iframe"
                | "object"
                | "embed"
                | "svg"
                | "math"
                | "link"
                | "meta"
                | "base"
                | "form"
        ) {
            return true;
        }
        let attrs = caps[2].as_bytes();
        let mut i = 0;
        while i < attrs.len() {
            while i < attrs.len() && attrs[i].is_ascii_whitespace() {
                i += 1;
            }
            if i == attrs.len() || (attrs[i] == b'/' && i + 1 == attrs.len()) {
                break;
            }
            let start = i;
            while i < attrs.len() && (attrs[i].is_ascii_alphanumeric() || b"_-".contains(&attrs[i]))
            {
                i += 1;
            }
            if i == start {
                return true;
            }
            let name = caps[2][start..i].to_ascii_lowercase();
            while i < attrs.len() && attrs[i].is_ascii_whitespace() {
                i += 1;
            }
            if i == attrs.len() || attrs[i] != b'=' {
                return true;
            }
            i += 1;
            while i < attrs.len() && attrs[i].is_ascii_whitespace() {
                i += 1;
            }
            if i == attrs.len() {
                return true;
            }
            let val;
            if matches!(attrs[i], b'\'' | b'"') {
                let quote = attrs[i];
                i += 1;
                let start = i;
                while i < attrs.len() && attrs[i] != quote {
                    i += 1;
                }
                if i == attrs.len() {
                    return true;
                }
                val = &caps[2][start..i];
                i += 1;
            } else {
                let start = i;
                while i < attrs.len() && !attrs[i].is_ascii_whitespace() {
                    i += 1;
                }
                val = &caps[2][start..i];
            }
            if !safe_attribute(&tag, &name, val) {
                return true;
            }
        }
    }
    false
}
