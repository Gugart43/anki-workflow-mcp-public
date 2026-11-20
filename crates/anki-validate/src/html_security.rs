//! Fail-closed HTML5 active-content boundary for Rust-first outputs.
//! Uses `ammonia` (`html5ever`) with a strict AnkiMBR + F2 semantic allowlist.

use ammonia::{Builder, UrlRelative};
use std::borrow::Cow;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

const ALLOWED_TAGS: &[&str] = &[
    "abbr",
    "b",
    "blockquote",
    "br",
    "caption",
    "cite",
    "code",
    "col",
    "colgroup",
    "dd",
    "del",
    "details",
    "div",
    "dl",
    "dt",
    "em",
    "figcaption",
    "figure",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "img",
    "ins",
    "kbd",
    "li",
    "mark",
    "ol",
    "p",
    "pre",
    "s",
    "section",
    "small",
    "span",
    "strong",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "u",
    "ul",
    "wbr",
];

const CLEAN_CONTENT_TAGS: &[&str] = &[
    "applet", "audio", "base", "button", "canvas", "embed", "form", "iframe", "input", "link",
    "math", "meta", "noscript", "object", "script", "select", "style", "svg", "template",
    "textarea", "title", "video", "xmp",
];

thread_local! {
    static ATTR_REJECTED: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn safe_style(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    ![
        "url",
        "expression",
        "import",
        "behavior",
        "binding",
        "javascript",
        "vbscript",
    ]
    .iter()
    .any(|s| lower.contains(s))
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || ":;#%.,()-".contains(c)
        })
}

pub(crate) fn safe_img_src(value: &str) -> bool {
    !value.is_empty()
        && !value.contains("..")
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-.".contains(&c))
}

static SANITIZER: LazyLock<Builder<'static>> = LazyLock::new(|| {
    let mut builder = Builder::default();

    builder.tags(ALLOWED_TAGS.iter().copied().collect());
    builder.clean_content_tags(CLEAN_CONTENT_TAGS.iter().copied().collect());
    builder.generic_attributes(HashSet::from(["title", "style"]));
    builder.tag_attributes(HashMap::from([
        ("img", HashSet::from(["src", "alt"])),
        ("th", HashSet::from(["colspan", "rowspan", "scope"])),
        ("td", HashSet::from(["colspan", "rowspan", "scope"])),
    ]));
    builder.allowed_classes(HashMap::from([
        (
            "figure",
            HashSet::from(["anki-figure", "anki-table-figure", "media"]),
        ),
        (
            "span",
            HashSet::from([
                "destaque",
                "cloze",
                "cloze-inactive",
                "ambr-badge",
                "ambr-pill",
                "ambr-tag",
                "ambr-muted",
                "ambr-accent",
                "alerta",
                "crit",
                "crit-i",
                "dose",
                "faixa",
                "fig-context",
                "fig-title",
                "hints",
                "lab",
                "nl",
                "nl-i",
                "nl-sub",
                "pearl",
                "rot",
                "sigla",
                "style-notas",
                "style-ref",
                "tbl",
                "termo",
                "uni",
            ]),
        ),
        (
            "details",
            HashSet::from([
                "hint-details",
                "anki-details",
                "ambr-details",
                "anki-detail",
            ]),
        ),
        ("summary", HashSet::from(["hint-summary", "ambr-summary"])),
        (
            "div",
            HashSet::from([
                "ambr-card",
                "ambr-section",
                "ambr-callout",
                "ambr-note",
                "ambr-table-wrap",
                "anki-note",
                "anki-callout",
                "alerta",
            ]),
        ),
        ("table", HashSet::from(["anki-table", "ambr-table", "tbl"])),
        (
            "p",
            HashSet::from(["ambr-note", "ambr-muted", "destaque", "alerta"]),
        ),
        (
            "blockquote",
            HashSet::from([
                "ambr-callout",
                "anki-callout",
                "anki-callout--trap",
                "anki-callout--pearl",
                "anki-callout--diff",
                "alerta",
            ]),
        ),
        (
            "section",
            HashSet::from([
                "anki-note-section",
                "anki-kv-section",
                "anki-list",
                "anki-steps",
                "anki-flow",
                "ambr-section",
            ]),
        ),
        ("footer", HashSet::from(["anki-sources"])),
        ("dl", HashSet::from(["anki-kv"])),
        ("ol", HashSet::from(["anki-steps"])),
        ("ul", HashSet::from(["anki-list", "anki-flow"])),
    ]));

    builder.url_schemes(HashSet::new());
    builder.url_relative(UrlRelative::PassThrough);
    builder.link_rel(None);
    builder.strip_comments(true);

    builder.attribute_filter(|tag, attr, val| {
        let allowed = match (tag, attr) {
            (_, "style") => safe_style(val),
            (_, "title") | ("img", "alt") => !val.contains(['<', '>']),
            ("img", "src") => safe_img_src(val),
            ("th" | "td", "colspan" | "rowspan") => {
                !val.is_empty() && val.bytes().all(|c| c.is_ascii_digit())
            }
            ("th" | "td", "scope") => matches!(val, "row" | "col" | "rowgroup" | "colgroup"),
            _ => true,
        };
        if allowed {
            Some(Cow::Borrowed(val))
        } else {
            ATTR_REJECTED.with(|flag| flag.set(true));
            None
        }
    });

    builder
});

fn is_benign_html5_diff(orig: &str, clean: &str) -> bool {
    let mut o = orig.as_bytes();
    let mut c = clean.as_bytes();
    let mut in_tag = false;

    while !o.is_empty() && !c.is_empty() {
        if !in_tag {
            // HTML5 parser auto-inserts <tbody> in tables
            if c.starts_with(b"<tbody>") && !o.starts_with(b"<tbody>") {
                c = &c[b"<tbody>".len()..];
                continue;
            }
            if c.starts_with(b"</tbody>") && !o.starts_with(b"</tbody>") {
                c = &c[b"</tbody>".len()..];
                continue;
            }

            // Entity decodings in text nodes (shared decoder with html_text::try_decode_entity)
            if o[0] == b'&'
                && let Ok(o_str) = std::str::from_utf8(o)
                && let Some((decoded, consumed)) = crate::html_text::try_decode_entity(o_str)
                && !matches!(decoded, '<' | '>' | '&')
            {
                let mut buf = [0u8; 4];
                let encoded = decoded.encode_utf8(&mut buf).as_bytes();
                if c.starts_with(encoded) {
                    o = &o[consumed..];
                    c = &c[encoded.len()..];
                    continue;
                }
            }

            if o.starts_with("\u{00A0}".as_bytes()) && c.starts_with(b"&nbsp;") {
                o = &o["\u{00A0}".len()..];
                c = &c[6..];
                continue;
            }

            // Clinical inequalities / raw entity serialization in text
            if o[0] == b'>' && c.starts_with(b"&gt;") {
                o = &o[1..];
                c = &c[4..];
                continue;
            }
            if o[0] == b'&' && c.starts_with(b"&amp;") {
                o = &o[1..];
                c = &c[5..];
                continue;
            }
            if o[0] == b'<'
                && c.starts_with(b"&lt;")
                && !o
                    .get(1)
                    .is_some_and(|b| b.is_ascii_alphabetic() || matches!(b, b'/' | b'!' | b'?'))
            {
                o = &o[1..];
                c = &c[4..];
                continue;
            }
        } else {
            // Self-closing void tag normalization (<br/> -> <br>, <hr /> -> <hr>)
            if o.starts_with(b"/>") && c.starts_with(b">") {
                o = &o[2..];
                c = &c[1..];
                in_tag = false;
                continue;
            }
            let ws_len = o.iter().take_while(|&&b| b == b' ' || b == b'\t').count();
            if ws_len > 0 && o[ws_len..].starts_with(b"/>") && c.starts_with(b">") {
                o = &o[ws_len + 2..];
                c = &c[1..];
                in_tag = false;
                continue;
            }

            // Normalization of attribute quote styles: 'value' -> "value"
            if o.starts_with(b"'") && c.starts_with(b"\"") {
                o = &o[1..];
                c = &c[1..];
                continue;
            }
        }

        if o[0] == c[0] {
            if o[0] == b'<' {
                in_tag = true;
            } else if o[0] == b'>' {
                in_tag = false;
            }
            o = &o[1..];
            c = &c[1..];
            continue;
        }

        return false;
    }

    // Trailing benign insertions (e.g. </tbody> before EOF)
    while !in_tag && c.starts_with(b"</tbody>") {
        c = &c[b"</tbody>".len()..];
    }

    o.is_empty() && c.is_empty()
}

pub fn has_unsafe_html(value: &str) -> bool {
    if !value.contains('<') {
        return false;
    }
    ATTR_REJECTED.with(|flag| flag.set(false));
    let cleaned = SANITIZER.clean(value).to_string();
    if ATTR_REJECTED.with(|flag| flag.get()) {
        return true;
    }
    cleaned != value && !is_benign_html5_diff(value, &cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_benign_semantic_tags_and_classes() {
        let safe_cases = [
            "<section class=\"anki-note-section\"><p><b>Título</b></p><p>Texto</p></section>",
            "<dl class=\"anki-kv\"><dt>Chave</dt><dd>Valor</dd></dl>",
            "<section class=\"anki-kv-section\"><p><b>Par</b></p><dl class=\"anki-kv\"><dt>K</dt><dd>V</dd></dl></section>",
            "<blockquote class=\"anki-callout anki-callout--trap\"><p><b>Cuidado:</b> Perigo</p></blockquote>",
            "<blockquote class=\"anki-callout anki-callout--pearl\"><p><b>Dica:</b> Peróla</p></blockquote>",
            "<blockquote class=\"anki-callout anki-callout--diff\"><p><b>Diferencial:</b> Comparação</p></blockquote>",
            "<section class=\"anki-list\"><ul><li>Item 1</li><li>Item 2</li></ul></section>",
            "<section class=\"anki-steps\"><ol><li>Passo 1</li><li>Passo 2</li></ol></section>",
            "<section class=\"anki-flow\"><ul><li>Etapa A</li></ul></section>",
            "<details class=\"anki-detail\"><summary><b>Expandir</b></summary><p>Detalhe</p></details>",
            "<details class=\"hint-details\"><summary class=\"hint-summary\">Dica</summary>Texto</details>",
            "<footer class=\"anki-sources\"><small><b>Fonte:</b> <cite>Harrison 21ed</cite></small></footer>",
            "<table class=\"anki-table\"><thead><tr><th>A</th></tr></thead><tbody><tr><td>1</td></tr></tbody></table>",
            "<figure class=\"anki-figure\"><img src=\"fig1.png\" alt=\"Foto\"><figcaption>Legenda</figcaption></figure>",
            "<figure class=\"anki-figure anki-table-figure\"><img src=\"tbl1.png\" alt=\"Tabela\"></figure>",
            "<span class=\"destaque\" style=\"color: red\">Destaque seguro</span>",
        ];

        for case in safe_cases {
            assert!(!has_unsafe_html(case), "should be accepted as safe: {case}");
        }
    }

    #[test]
    fn accepts_clinical_inequalities_and_table_normalization() {
        let benign_cases = [
            "<p>K+ > 5.5 mEq/L</p>",
            "<p>pH < 7.35</p>",
            "<p>Troponina <= 0.04</p>",
            "<p>Glicemia >= 126</p>",
            "<p>Item A & Item B</p>",
            "<p>Texto com &#124; pipe escapado</p>",
            "<p>Linha 1<br/>Linha 2</p>",
            "<p>Linha 1<br />Linha 2</p>",
            "<table><tr><th>Sem Tbody</th></tr><tr><td>Valor</td></tr></table>",
        ];

        for case in benign_cases {
            assert!(
                !has_unsafe_html(case),
                "benign normalization should not be marked unsafe: {case}"
            );
        }
    }

    #[test]
    fn accepts_decoded_clinical_entities() {
        let clinical_cases = [
            "<p>A &rarr; B &uarr; PA &darr; FC &harr; equilíbrio &rArr; conduta</p>",
            "<p>Temperatura 37,5 &plusmn; 0,5 &deg;C; dose 50 &mu;g</p>",
            "<p>Receptores &alpha;, &beta;, &gamma;, &delta; e &Delta;pH</p>",
            "<p>Faixa 10&ndash;20 &bull; p &ne; 0,05 &bull; IMC &asymp; 25 kg/m&sup2; &bull; 10 mm&sup3;</p>",
        ];

        for case in clinical_cases {
            assert!(
                !has_unsafe_html(case),
                "valid clinical entities must not trigger has_unsafe_html: {case}"
            );
        }
    }

    #[test]
    fn rejects_active_content_and_script_injection() {
        let unsafe_cases = [
            "<script>alert(1)</script>",
            "<script src=\"evil.js\"></script>",
            "<b onclick=\"alert(1)\">Texto</b>",
            "<b oNcLiCk='alert(1)'>Texto</b>",
            "<img src=\"x\" onerror=\"alert(1)\">",
            "<svg><circle onload=\"alert(1)\"/></svg>",
            "<iframe src=\"javascript:alert(1)\"></iframe>",
            "<a href=\"javascript:alert(1)\">Link</a>",
            "<object data=\"evil.swf\"></object>",
            "<embed src=\"evil.swf\">",
            "<form action=\"evil.com\"><input type=\"text\"></form>",
        ];

        for case in unsafe_cases {
            assert!(has_unsafe_html(case), "must be rejected as unsafe: {case}");
        }
    }

    #[test]
    fn rejects_dangerous_css_and_escapes() {
        let unsafe_styles = [
            "<span style=\"background:url(https://evil.com/x)\">Texto</span>",
            "<span style=\"background:u&#114;l(x)\">Texto</span>",
            "<span style=\"background:u\\72l(x)\">Texto</span>",
            "<span style=\"expression(alert(1))\">Texto</span>",
            "<span style=\"behavior:url(evil.htc)\">Texto</span>",
            "<span style=\"binding:url(evil.xml)\">Texto</span>",
            "<span style=\"/*comment*/color:red\">Texto</span>",
            "<span style=\"@import 'evil.css'\">Texto</span>",
        ];

        for case in unsafe_styles {
            assert!(
                has_unsafe_html(case),
                "dangerous CSS must be rejected: {case}"
            );
        }
    }

    #[test]
    fn rejects_media_traversal_and_remote_urls() {
        let unsafe_media = [
            "<img src=\"../secret.png\" alt=\"x\">",
            "<img src=\"sub/folder.png\" alt=\"x\">",
            "<img src=\"https://remote.com/pic.png\" alt=\"x\">",
            "<img src=\"data:image/png;base64,AAAA\" alt=\"x\">",
            "<img src=\"javascript:alert(1)\" alt=\"x\">",
        ];

        for case in unsafe_media {
            assert!(
                has_unsafe_html(case),
                "media traversal or remote URL must be rejected: {case}"
            );
        }
    }

    #[test]
    fn rejects_unknown_classes_and_attributes() {
        let unknown_cases = [
            "<div class=\"unregistered-class\">Texto</div>",
            "<span class=\"destaque evil-class\">Texto</span>",
            "<p id=\"custom-id\">Texto</p>",
            "<p data-bind=\"x\">Texto</p>",
        ];

        for case in unknown_cases {
            assert!(
                has_unsafe_html(case),
                "unknown classes and attributes must be rejected: {case}"
            );
        }
    }
}
