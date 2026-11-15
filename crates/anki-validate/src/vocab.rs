//! Semantic HTML/CSS vocabulary (`core/vocab.py`).

/// Tags allowed in any text field.
pub const ALLOWED_TAGS: &[&str] = &[
    "b",
    "strong",
    "i",
    "em",
    "u",
    "s",
    "small",
    "span",
    "div",
    "p",
    "blockquote",
    "br",
    "hr",
    "sub",
    "sup",
    "code",
    "mark",
    "table",
    "caption",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "th",
    "td",
    "ul",
    "ol",
    "li",
];

/// Tags allowed only inside `"Imagens Relacionadas"`.
pub const IMAGES_ONLY_TAGS: &[&str] = &["img", "figure", "figcaption"];

/// Paired (non-void) tags tracked for matching open/close.
pub const PAIRED_TAGS: &[&str] = &[
    "b",
    "strong",
    "i",
    "em",
    "u",
    "s",
    "small",
    "span",
    "div",
    "p",
    "blockquote",
    "sub",
    "sup",
    "code",
    "mark",
    "table",
    "caption",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "th",
    "td",
    "ul",
    "ol",
    "li",
    "figure",
    "figcaption",
];

/// Closed semantic CSS class vocabulary.
pub const ALLOWED_CLASSES: &[&str] = &[
    "alerta",
    "anki-figure",
    "anki-table-figure",
    "crit",
    "crit-i",
    "dose",
    "faixa",
    "fig-context",
    "fig-title",
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
