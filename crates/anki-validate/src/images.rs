use std::collections::HashSet;
use std::sync::LazyLock;

pub use anki_domain::CsvRowNumber;
use anki_domain::{Diagnostic, DiagnosticCode, Severity};
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
    LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]*_[a-z0-9_.-]+$").expect("valid NS_VALID"));
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

pub const CODE_IMG_BARE_IMG: DiagnosticCode = DiagnosticCode::from_static("IMG.BARE_IMG");
pub const CODE_IMG_INLINE_STYLE: DiagnosticCode = DiagnosticCode::from_static("IMG.INLINE_STYLE");
pub const CODE_IMG_CAP_OVER: DiagnosticCode = DiagnosticCode::from_static("IMG.CAP_OVER");
pub const CODE_IMG_STRUCTURE: DiagnosticCode = DiagnosticCode::from_static("IMG.STRUCTURE");
pub const CODE_IMG_ALT_MISSING: DiagnosticCode = DiagnosticCode::from_static("IMG.ALT_MISSING");
pub const CODE_IMG_NAMESPACE: DiagnosticCode = DiagnosticCode::from_static("IMG.NAMESPACE");
pub const CODE_IMG_PHANTOM: DiagnosticCode = DiagnosticCode::from_static("IMG.PHANTOM");
pub const CODE_IMG_DUP_IN_CARD: DiagnosticCode = DiagnosticCode::from_static("IMG.DUP_IN_CARD");
pub const CODE_IMG_CAPTION: DiagnosticCode = DiagnosticCode::from_static("IMG.CAPTION");
pub const CODE_IMG_CAPTION_WEAK: DiagnosticCode = DiagnosticCode::from_static("IMG.CAPTION_WEAK");

fn make_diag(
    code: DiagnosticCode,
    severity: Severity,
    message: impl Into<String>,
    row: Option<usize>,
    field: &str,
    suggestion: Option<&str>,
) -> Diagnostic {
    let mut d = Diagnostic::new(code, severity, OWNER_SKILL, message).with_field(field);
    if let Some(r) = row {
        d = d.with_row(r);
    }
    if let Some(s) = suggestion {
        d = d.with_suggestion(s);
    }
    d
}

/// Validate the `Imagens Relacionadas` HTML field with a typed `CsvRowNumber`.
#[must_use]
pub fn check_images_field_row(
    html: &str,
    catalog_filenames: Option<&HashSet<String>>,
    row: Option<CsvRowNumber>,
    field: &str,
) -> Vec<Diagnostic> {
    check_images_field(html, catalog_filenames, row.map(usize::from), field)
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
            CODE_IMG_BARE_IMG.clone(),
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
                CODE_IMG_INLINE_STYLE.clone(),
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
            CODE_IMG_CAP_OVER.clone(),
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
                CODE_IMG_INLINE_STYLE.clone(),
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
                CODE_IMG_STRUCTURE.clone(),
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
                CODE_IMG_ALT_MISSING.clone(),
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
                CODE_IMG_NAMESPACE.clone(),
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
                    CODE_IMG_PHANTOM.clone(),
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
                CODE_IMG_DUP_IN_CARD.clone(),
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
                CODE_IMG_STRUCTURE.clone(),
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
                CODE_IMG_CAPTION.clone(),
                Severity::Warn,
                format!("Figure #{idx}: empty caption"),
                row,
                field,
                None,
            ));
        } else if NOISE_RE.is_match(caption) {
            diags.push(make_diag(
                CODE_IMG_CAPTION.clone(),
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
                    CODE_IMG_CAPTION.clone(),
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
                    CODE_IMG_CAPTION_WEAK.clone(),
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

    #[test]
    fn namespaced_pattern_with_hyphens_and_leading_hyphen_rejection() {
        let mut catalog = HashSet::new();
        catalog.insert("ilae-2025_fig-1.png".to_string());
        catalog.insert("slug-with-hyphen_image-2.jpeg".to_string());

        // Valid hyphenated slug and filename
        let html_valid = r#"<figure class="anki-figure"><img src="ilae-2025_fig-1.png" alt="Alt"><figcaption>Descrição clínica longa o suficiente para passar no teste.</figcaption></figure>"#;
        let diags_valid =
            check_images_field(html_valid, Some(&catalog), Some(2), "Imagens Relacionadas");
        assert!(
            diags_valid.is_empty(),
            "expected valid hyphenated namespaced filename to pass, got: {diags_valid:?}"
        );

        // Leading hyphen in slug must be rejected
        let html_leading_hyphen = r#"<figure class="anki-figure"><img src="-invalid-slug_fig.png" alt="Alt"><figcaption>Descrição clínica longa o suficiente para passar no teste.</figcaption></figure>"#;
        let diags_leading =
            check_images_field(html_leading_hyphen, None, Some(3), "Imagens Relacionadas");
        assert!(
            diags_leading.iter().any(|d| d.code == CODE_IMG_NAMESPACE),
            "expected leading hyphen in slug to trigger CODE_IMG_NAMESPACE"
        );

        // Leading underscore in slug must be rejected
        let html_leading_underscore = r#"<figure class="anki-figure"><img src="_invalid_slug_fig.png" alt="Alt"><figcaption>Descrição clínica longa o suficiente para passar no teste.</figcaption></figure>"#;
        let diags_underscore = check_images_field(
            html_leading_underscore,
            None,
            Some(4),
            "Imagens Relacionadas",
        );
        assert!(
            diags_underscore
                .iter()
                .any(|d| d.code == CODE_IMG_NAMESPACE),
            "expected leading underscore in slug to trigger CODE_IMG_NAMESPACE"
        );
    }

    #[test]
    fn typed_csv_row_number_preserves_row_and_callers() {
        let mut catalog = HashSet::new();
        catalog.insert("test_p1.png".to_string());
        let html = r#"<figure class="anki-figure"><img src="test_p1.png"><figcaption></figcaption></figure>"#;

        // Called via check_images_field_row with CsvRowNumber
        let row_num = CsvRowNumber::new(10);
        let diags_typed =
            check_images_field_row(html, Some(&catalog), row_num, "Imagens Relacionadas");
        assert!(!diags_typed.is_empty());
        assert!(diags_typed.iter().all(|d| d.row == Some(10)));
        assert!(diags_typed.iter().any(|d| d.code == CODE_IMG_CAPTION));

        // Called via check_images_field with Option<usize> (preserving callers)
        let diags_legacy =
            check_images_field(html, Some(&catalog), Some(10), "Imagens Relacionadas");
        assert_eq!(diags_typed, diags_legacy);

        // Constant comparison
        assert_eq!(CODE_IMG_BARE_IMG.as_str(), "IMG.BARE_IMG");
        assert_eq!(CODE_IMG_INLINE_STYLE.as_str(), "IMG.INLINE_STYLE");
        assert_eq!(CODE_IMG_CAP_OVER.as_str(), "IMG.CAP_OVER");
        assert_eq!(CODE_IMG_STRUCTURE.as_str(), "IMG.STRUCTURE");
        assert_eq!(CODE_IMG_ALT_MISSING.as_str(), "IMG.ALT_MISSING");
        assert_eq!(CODE_IMG_NAMESPACE.as_str(), "IMG.NAMESPACE");
        assert_eq!(CODE_IMG_PHANTOM.as_str(), "IMG.PHANTOM");
        assert_eq!(CODE_IMG_DUP_IN_CARD.as_str(), "IMG.DUP_IN_CARD");
        assert_eq!(CODE_IMG_CAPTION.as_str(), "IMG.CAPTION");
        assert_eq!(CODE_IMG_CAPTION_WEAK.as_str(), "IMG.CAPTION_WEAK");
    }
}
