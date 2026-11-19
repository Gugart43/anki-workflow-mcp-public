//! Semantic HTML/CSS vocabulary (`core/vocab.py`).

/// Tags allowed in any text field.
pub const ALLOWED_TAGS: &[&str] = &[
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
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
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

/// Tags allowed only inside `"Imagens Relacionadas"`.
pub const IMAGES_ONLY_TAGS: &[&str] = &["img", "figure", "figcaption"];

/// Paired (non-void) tags tracked for matching open/close.
pub const PAIRED_TAGS: &[&str] = &[
    "abbr",
    "b",
    "blockquote",
    "caption",
    "cite",
    "code",
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
    "i",
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
];

/// Closed semantic CSS class vocabulary.
pub const ALLOWED_CLASSES: &[&str] = &[
    "alerta",
    "ambr-accent",
    "ambr-badge",
    "ambr-callout",
    "ambr-card",
    "ambr-details",
    "ambr-muted",
    "ambr-note",
    "ambr-pill",
    "ambr-section",
    "ambr-summary",
    "ambr-table",
    "ambr-table-wrap",
    "ambr-tag",
    "anki-callout",
    "anki-callout--diff",
    "anki-callout--pearl",
    "anki-callout--trap",
    "anki-detail",
    "anki-details",
    "anki-figure",
    "anki-flow",
    "anki-kv",
    "anki-kv-section",
    "anki-list",
    "anki-note",
    "anki-note-section",
    "anki-sources",
    "anki-steps",
    "anki-table",
    "anki-table-figure",
    "cloze",
    "cloze-inactive",
    "crit",
    "crit-i",
    "destaque",
    "dose",
    "faixa",
    "fig-context",
    "fig-title",
    "hint-details",
    "hint-summary",
    "hints",
    "lab",
    "media",
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
];

#[must_use]
pub fn is_always_allowed_tag(tag_name: &str) -> bool {
    let bare = tag_name.strip_prefix('/').unwrap_or(tag_name);
    ALLOWED_TAGS.contains(&bare)
}

#[must_use]
pub fn is_images_only_tag(tag_name: &str) -> bool {
    let bare = tag_name.strip_prefix('/').unwrap_or(tag_name);
    IMAGES_ONLY_TAGS.contains(&bare)
}

#[must_use]
pub fn is_paired_tag(tag_name: &str) -> bool {
    PAIRED_TAGS.contains(&tag_name)
}

#[must_use]
pub fn is_allowed_tag(name: &str, is_images_field: bool) -> bool {
    ALLOWED_TAGS.contains(&name) || (is_images_field && IMAGES_ONLY_TAGS.contains(&name))
}

/// Split a `class="..."` attribute value and return tokens not in `ALLOWED_CLASSES`.
#[must_use]
pub fn unknown_classes(class_attr: &str) -> Vec<&str> {
    class_attr
        .split_whitespace()
        .filter(|t| !t.is_empty() && !ALLOWED_CLASSES.contains(t))
        .collect()
}
