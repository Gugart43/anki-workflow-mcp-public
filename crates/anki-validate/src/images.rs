use std::collections::HashSet;
use std::sync::LazyLock;

use anki_domain::{Diagnostic, Severity};
use regex::Regex;

static FIG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<figure[^>]*>(.*?)</figure>").expect("valid FIG_RE"));
static FIG_FULL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<figure\b[^>]*>.*?</figure>").expect("valid FIG_FULL_RE"));
static IMG_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<img\b[^>]*>").expect("valid IMG_TAG"));
static IMG_SRC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<img\b[^>]*?\bsrc\s*=\s*["']([^"']+)["']"#).expect("valid IMG_SRC")
});
static ALT_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\balt\s*=").expect("valid ALT_ATTR"));
static INLINE_STYLE_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)<(figure|img|figcaption)\b[^>]*?\bstyle\s*=").expect("valid INLINE_STYLE_ATTR")
});
static FIGCAP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<figcaption[^>]*>(.*?)</figcaption>").expect("valid FIGCAP")
});
static NS_VALID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9]*_[a-z0-9_.]+$").expect("valid NS_VALID"));
static NOISE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:ISBN|Copyright|©|\bReceived\s*:|ISSN|\bDOI\s*:|\bCatalog\s+No\b|Todos\s+os\s+direitos\s+reservados)",
    )
    .expect("valid NOISE_RE")
});
static GENERIC_CAPTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:fig(?:ura)?|imagem?|photo|foto|quadro|tabela)\.?\s*(?:\d+|[ivxlcdm]+)?(?:\s+da\s+p[áa]gina\s+\d+)?\s*[:.-]?\s*$|^tabela\s*/\s*quadro\s+estruturado$|^\d+\s*linhas?\s*[x×]\s*\d+\s*colunas?$",
    )
    .expect("valid GENERIC_CAPTION_RE")
});
static FILE_EXT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.(?:png|jpe?g|gif|webp|svg)$").expect("valid FILE_EXT_RE"));
static STYLE_ATTR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bstyle\s*=").expect("valid STYLE_ATTR_RE"));
static HTML_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<[^>]+>").expect("valid HTML_TAG_RE"));

const OWNER_SKILL: &str = "anki-image-injector";

fn make_diag(
    code: &'static str,
    severity: Severity,
    message: impl Into<String>,
    row: Option<usize>,
    field: &str,
    suggestion: Option<&str>,
) -> Diagnostic {
    let mut d = Diagnostic::new(
        anki_domain::DiagnosticCode::from_static(code),
        severity,
        OWNER_SKILL,
        message,
    )
    .with_field(field);
    if let Some(r) = row {
        d = d.with_row(r);
    }
    if let Some(s) = suggestion {
        d = d.with_suggestion(s);
    }
    d
}

/// Validate the `Imagens Relacionadas` HTML field.
#[must_use]
pub fn check_images_field(
    html: &str,
    catalog_filenames: Option<&HashSet<String>>,
    row: Option<usize>,
    field: &str,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    if html.trim().is_empty() {
        return diags;
    }

    // ── IMG.BARE_IMG ─────────────────────────────────────────────────────────
    let outside_figures = FIG_FULL_RE.replace_all(html, "");
    for bare_m in IMG_TAG.find_iter(&outside_figures) {
        let bare_str = bare_m.as_str();
        diags.push(make_diag(
            "IMG.BARE_IMG",
            Severity::Critical,
            format!("Bare <img> tag found outside <figure>: '{bare_str}'"),
            row,
            field,
            Some(
                r#"Wrap all images in semantic <figure class="anki-figure"><img ...><figcaption>...</figcaption></figure>"#,
            ),
        ));
        if STYLE_ATTR_RE.is_match(bare_str) {
            diags.push(make_diag(
                "IMG.INLINE_STYLE",
                Severity::Warn,
                format!("Bare <img> tag contains inline style '{bare_str}'"),
                row,
                field,
                Some("Remove inline style attribute; styling is handled by AnkiMBR CSS variables"),
            ));
        }
    }

    // ── IMG.CAP_OVER ─────────────────────────────────────────────────────────
    let img_src_count = IMG_SRC.captures_iter(html).count();
    if img_src_count > 3 {
        diags.push(make_diag(
            "IMG.CAP_OVER",
            Severity::Critical,
            format!("Card has {img_src_count} images (maximum 3)"),
            row,
            field,
            Some("Remove lower-ranked images to stay within the 3-image cap"),
        ));
    }

    let catalog_lower: Option<HashSet<String>> =
        catalog_filenames.map(|cat| cat.iter().map(|c| c.to_lowercase()).collect());

    let mut seen_srcs: HashSet<String> = HashSet::new();

    for (i, block) in FIG_RE.captures_iter(html).enumerate() {
        let idx = i + 1;
        let full_block = block.get(0).map_or("", |m| m.as_str());
        let inner = block.get(1).map_or("", |m| m.as_str());
        let img_m = IMG_SRC.captures(inner);
        let cap_m = FIGCAP.captures(inner);

        // ── IMG.INLINE_STYLE ─────────────────────────────────────────────────
        for style_m in INLINE_STYLE_ATTR.captures_iter(full_block) {
            let tag = style_m.get(1).map_or("", |m| m.as_str());
            diags.push(make_diag(
                "IMG.INLINE_STYLE",
                Severity::Warn,
                format!("Figure #{idx}: inline style in <{tag}> tag breaks theme CSS"),
                row,
                field,
                Some(
                    r#"Remove inline style attribute; use class="anki-figure" and AnkiMBR CSS variables"#,
                ),
            ));
        }

        // ── IMG.STRUCTURE (img) ──────────────────────────────────────────────
        let Some(img_cap) = img_m else {
            diags.push(make_diag(
                "IMG.STRUCTURE",
                Severity::Critical,
                format!("Figure #{idx}: no <img src=...> found inside <figure>"),
                row,
                field,
                None,
            ));
            continue;
        };

        let src = img_cap.get(1).map_or("", |m| m.as_str());

        // ── IMG.ALT_MISSING ──────────────────────────────────────────────────
        if let Some(img_tag_m) = IMG_TAG.find(inner)
            && !ALT_ATTR.is_match(img_tag_m.as_str())
        {
            diags.push(make_diag(
                "IMG.ALT_MISSING",
                Severity::Log,
                format!("Figure #{idx}: <img> tag missing alt attribute"),
                row,
                field,
                Some(r#"Add an alt attribute describing the image: <img src="..." alt="...">"#),
            ));
        }

        // ── IMG.NAMESPACE ────────────────────────────────────────────────────
        if !NS_VALID.is_match(src) {
            diags.push(make_diag(
                "IMG.NAMESPACE",
                Severity::Critical,
                format!(
                    "Figure #{idx}: src '{src}' does not match required lowercase namespace pattern <slug>_<filename>"
                ),
                row,
                field,
                Some(
                    "Run image_namespace(slug, filename) so the filename is lowercase and prefixed (e.g. 'ceratite_p14_1.png')",
                ),
            ));
        }

        // ── IMG.PHANTOM ──────────────────────────────────────────────────────
        if let Some(cat) = catalog_filenames {
            let in_cat = cat.contains(src)
                || catalog_lower
                    .as_ref()
                    .is_some_and(|cl| cl.contains(&src.to_lowercase()));
            if !in_cat {
                diags.push(make_diag(
                    "IMG.PHANTOM",
                    Severity::Critical,
                    format!("Figure #{idx}: src '{src}' not found in the active catalog"),
                    row,
                    field,
                    Some(
                        "Only inject images whose filenames exist in the loaded catalog (use image_candidates + image_namespace)",
                    ),
                ));
            }
        }

        // ── IMG.DUP_IN_CARD ──────────────────────────────────────────────────
        if seen_srcs.contains(src) {
            diags.push(make_diag(
                "IMG.DUP_IN_CARD",
                Severity::Critical,
                format!("Image '{src}' appears more than once in this card"),
                row,
                field,
                None,
            ));
        }
        seen_srcs.insert(src.to_string());

        // ── IMG.STRUCTURE (figcaption) ───────────────────────────────────────
        let Some(cap_match) = cap_m else {
            diags.push(make_diag(
                "IMG.STRUCTURE",
                Severity::Critical,
                format!("Figure #{idx}: missing <figcaption>"),
                row,
                field,
                Some("Add a contextual clinical caption inside <figcaption>"),
            ));
            continue;
        };

        let caption = cap_match.get(1).map_or("", |m| m.as_str()).trim();

        // ── IMG.CAPTION & IMG.CAPTION_WEAK ───────────────────────────────────
        if caption.is_empty() {
            diags.push(make_diag(
                "IMG.CAPTION",
                Severity::Warn,
                format!("Figure #{idx}: empty caption"),
                row,
                field,
                None,
            ));
        } else if NOISE_RE.is_match(caption) {
            diags.push(make_diag(
                "IMG.CAPTION",
                Severity::Warn,
                format!(
                    "Figure #{idx}: caption appears to contain bibliographic noise (ISBN/Copyright/…)"
                ),
                row,
                field,
                Some("Remove noise and keep only the descriptive clinical text"),
            ));
        } else {
            let text_caption = HTML_TAG_RE.replace_all(caption, "").trim().to_string();
            if text_caption.is_empty() {
                diags.push(make_diag(
                    "IMG.CAPTION",
                    Severity::Warn,
                    format!("Figure #{idx}: empty caption"),
                    row,
                    field,
                    None,
                ));
            } else if text_caption.chars().count() < 20
                || GENERIC_CAPTION_RE.is_match(&text_caption)
                || FILE_EXT_RE.is_match(&text_caption)
            {
                diags.push(make_diag(
                    "IMG.CAPTION_WEAK",
                    Severity::Warn,
                    format!(
                        "Figure #{idx}: caption '{text_caption}' is weak (too short, generic, or raw extractor placeholder)"
                    ),
                    row,
                    field,
                    Some(
                        "Provide a contextual clinical caption (>= 20 chars) explaining the visual finding and its connection to the card",
                    ),
                ));
            }
        }
    }

    diags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_html_returns_no_diagnostics() {
        assert!(check_images_field("", None, Some(1), "Imagens Relacionadas").is_empty());
        assert!(check_images_field("   \n\t ", None, Some(1), "Imagens Relacionadas").is_empty());
    }

    #[test]
    fn valid_figure_with_catalog_passes() {
        let mut catalog = HashSet::new();
        catalog.insert("ceratite_p14_1.png".to_string());
        let html = r#"<figure class="anki-figure"><img src="ceratite_p14_1.png" alt="Úlcera dendrítica"><figcaption>Úlcera dendrítica corada por fluoresceína na ceratite herpética.</figcaption></figure>"#;
        let diags = check_images_field(html, Some(&catalog), Some(2), "Imagens Relacionadas");
        assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    }

    #[test]
    fn bare_img_with_inline_style_and_phantom_in_figure() {
        let mut catalog = HashSet::new();
        catalog.insert("other_p1_1.png".to_string());
        let html = r#"<img src="bare.png" style="width:100px"><figure><img src="BadName.png"><figcaption>Figura 1</figcaption></figure>"#;
        let diags = check_images_field(html, Some(&catalog), Some(3), "Imagens Relacionadas");
        let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(
            codes,
            vec![
                "IMG.BARE_IMG",
                "IMG.INLINE_STYLE",
                "IMG.ALT_MISSING",
                "IMG.NAMESPACE",
                "IMG.PHANTOM",
                "IMG.CAPTION_WEAK",
            ]
        );
        assert!(diags.iter().all(|d| d.row == Some(3)));
        assert!(
            diags
                .iter()
                .all(|d| d.field.as_deref() == Some("Imagens Relacionadas"))
        );
    }

    #[test]
    fn cap_over_and_dup_and_noise_caption() {
        let html = concat!(
            r#"<figure><img src="a_1.png" alt="a"><figcaption>Copyright 2024 Elsevier</figcaption></figure>"#,
            r#"<figure><img src="a_1.png" alt="a"><figcaption></figcaption></figure>"#,
            r#"<figure><img src="a_2.png" alt="b"></figure>"#,
            r#"<figure><img src="a_3.png" alt="c"><figcaption>Descrição clínica detalhada suficiente.</figcaption></figure>"#,
        );
        let diags = check_images_field(html, None, None, "Imagens Relacionadas");
        let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
        assert!(codes.contains(&"IMG.CAP_OVER"));
        assert!(codes.contains(&"IMG.CAPTION"));
        assert!(codes.contains(&"IMG.DUP_IN_CARD"));
        assert!(codes.contains(&"IMG.STRUCTURE"));
    }
}
