use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};

static FIG_PARSE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<figure\b([^>]*)>\s*<img\b[^>]*src=["']([^"']+)["'][^>]*>\s*(?:<figcaption\b[^>]*>(.*?)</figcaption>)?\s*</figure>"#,
    )
    .expect("valid figure regex")
});

static ANKI_FIGURE_CLASS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)\bclass\s*=\s*(?:["'][^"']*\banki-figure\b[^"']*["']|\banki-figure\b)"#)
        .expect("valid anki-figure class regex")
});

static FIG_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<figure\b[^>]*>.*?</figure>").expect("valid figure block regex")
});

static UNCLOSED_FIG_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<figure\b[^>]*>.*$").expect("valid unclosed figure block regex")
});

static BARE_IMG_PARSE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<img\b[^>]*src=["']([^"']+)["'][^>]*>"#).expect("valid bare img regex")
});

static HTML_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<[^>]+>").expect("valid html tag regex"));

static IMAGE_EXT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\.(?:png|jpe?g|gif|webp|svg)$").expect("valid image extension regex")
});

fn make_figure_json(filename: &str, caption: &str, style: &str) -> Value {
    json!({
        "filename": filename,
        "caption": caption,
        "style": style,
    })
}

pub fn parse_images_html_to_figures(value: &Value) -> Vec<Value> {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => Vec::new(),
        Value::Object(_) => vec![value.clone()],
        Value::Array(items) => items
            .iter()
            .flat_map(parse_images_html_to_figures)
            .collect(),
        Value::String(raw) => {
            let s = raw.trim();
            if s.is_empty() {
                return Vec::new();
            }

            let mut figures = Vec::new();
            for caps in FIG_PARSE_RE.captures_iter(s) {
                let open_attrs = caps.get(1).map_or("", |m| m.as_str());
                let filename = caps.get(2).map_or("", |m| m.as_str()).trim();
                let caption = caps.get(3).map_or("", |m| m.as_str()).trim();
                let style = if ANKI_FIGURE_CLASS_RE.is_match(open_attrs) {
                    "clinical"
                } else {
                    ""
                };
                if !filename.is_empty() {
                    figures.push(make_figure_json(filename, caption, style));
                }
            }

            // Exclude figure blocks (including malformed and unclosed) before loose img scan
            let outside = FIG_BLOCK_RE.replace_all(s, "");
            let outside = UNCLOSED_FIG_BLOCK_RE.replace_all(&outside, "");

            let bare_filenames: Vec<&str> = BARE_IMG_PARSE_RE
                .captures_iter(&outside)
                .filter_map(|caps| {
                    caps.get(1)
                        .map(|m| m.as_str().trim())
                        .filter(|fn_str| !fn_str.is_empty())
                })
                .collect();

            if !bare_filenames.is_empty() {
                let rem_text = if bare_filenames.len() == 1 {
                    HTML_TAG_RE.replace_all(&outside, " ").trim().to_string()
                } else {
                    String::new()
                };
                for filename in bare_filenames {
                    figures.push(make_figure_json(filename, &rem_text, ""));
                }
            }

            if figures.is_empty()
                && !s.contains('<')
                && !s.contains('>')
                && IMAGE_EXT_RE.is_match(s)
            {
                figures.push(make_figure_json(s, "", ""));
            }

            figures
        }
    }
}

pub fn unique_figures(figura: Option<&Value>, figuras: &[Value]) -> Vec<Value> {
    let mut candidates = Vec::new();
    if let Some(fig) = figura
        && !fig.is_null()
    {
        candidates.extend(parse_images_html_to_figures(fig));
    }
    for fig in figuras {
        if !fig.is_null() {
            candidates.extend(parse_images_html_to_figures(fig));
        }
    }

    let mut result = Vec::new();
    let mut seen_filenames = HashSet::new();

    for fig in candidates {
        let trimmed = fig
            .get("filename")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if !trimmed.is_empty() && seen_filenames.insert(trimmed.to_string()) {
            result.push(fig);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_non_image_inputs_return_empty() {
        assert!(parse_images_html_to_figures(&Value::Null).is_empty());
        assert!(parse_images_html_to_figures(&json!("")).is_empty());
        assert!(parse_images_html_to_figures(&json!("   ")).is_empty());
        assert!(parse_images_html_to_figures(&json!("notes.txt")).is_empty());
        assert!(parse_images_html_to_figures(&json!("<p>no image here</p>")).is_empty());
        assert!(parse_images_html_to_figures(&json!(42)).is_empty());
    }

    #[test]
    fn parses_figure_with_case_sensitive_class_and_bare_img() {
        let html = json!(
            r#"<figure class="anki-figure"><img src=" rx.png "><figcaption> Torax </figcaption></figure>
               <figure class="ANKI-FIGURE"><img src="ct.jpg"></figure>
               Texto <img src="bare.webp"> extra"#
        );
        let parsed = parse_images_html_to_figures(&html);
        assert_eq!(parsed.len(), 3);
        assert_eq!(
            parsed[0],
            json!({"filename": "rx.png", "caption": "Torax", "style": "clinical"})
        );
        assert_eq!(
            parsed[1],
            json!({"filename": "ct.jpg", "caption": "", "style": ""})
        );
        assert_eq!(
            parsed[2],
            json!({"filename": "bare.webp", "caption": "Texto   extra", "style": ""})
        );
    }

    #[test]
    fn parses_standalone_filename_and_nested_arrays() {
        let input = json!([
            "  lesao.PNG ",
            {"filename": "canon.jpg", "caption": "Cap", "style": "clinical"}
        ]);
        let parsed = parse_images_html_to_figures(&input);
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0],
            json!({"filename": "lesao.PNG", "caption": "", "style": ""})
        );
        assert_eq!(
            parsed[1],
            json!({"filename": "canon.jpg", "caption": "Cap", "style": "clinical"})
        );
    }

    #[test]
    fn unique_figures_dedups_by_trimmed_filename_preserving_first_untouched() {
        let figura = json!({"filename": "  a.png ", "caption": "First", "style": "clinical"});
        let figuras = vec![
            json!({"filename": "a.png", "caption": "Duplicate", "style": ""}),
            json!("<img src=\"b.jpg\">"),
            json!({"filename": "   ", "caption": "Empty", "style": ""}),
            json!({"filename": "b.jpg", "caption": "Dup B", "style": ""}),
        ];
        let deduped = unique_figures(Some(&figura), &figuras);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0]["filename"], json!("  a.png "));
        assert_eq!(deduped[0]["caption"], json!("First"));
        assert_eq!(deduped[1]["filename"], json!("b.jpg"));
    }

    #[test]
    fn figure_class_only_in_opening_attributes_and_malicious_text() {
        // Valid AnkiMBR standard figure
        let html_valid = json!(
            r#"<figure class="anki-figure"><img src="ilae-2025_fig-1.png"><figcaption>Legenda clínica explicativa</figcaption></figure>"#
        );
        let parsed_valid = parse_images_html_to_figures(&html_valid);
        assert_eq!(parsed_valid.len(), 1);
        assert_eq!(
            parsed_valid[0],
            json!({
                "filename": "ilae-2025_fig-1.png",
                "caption": "Legenda clínica explicativa",
                "style": "clinical"
            })
        );

        // Multiple classes in opening attribute
        let html_multi_class = json!(
            r#"<figure class="card-media anki-figure responsive"><img src="multi.png"><figcaption>Legenda</figcaption></figure>"#
        );
        let parsed_multi = parse_images_html_to_figures(&html_multi_class);
        assert_eq!(parsed_multi.len(), 1);
        assert_eq!(parsed_multi[0]["style"], "clinical");

        // Malicious / ambiguous text inside figcaption should NOT grant "clinical" style
        let html_fake_caption = json!(
            r#"<figure><img src="fake_cap.png"><figcaption>Texto com class="anki-figure" na legenda</figcaption></figure>"#
        );
        let parsed_fake_cap = parse_images_html_to_figures(&html_fake_caption);
        assert_eq!(parsed_fake_cap.len(), 1);
        assert_eq!(parsed_fake_cap[0]["style"], "");

        // Malicious non-class attribute in opening tag should NOT grant "clinical" style
        let html_fake_attr = json!(
            r#"<figure data-note="anki-figure" title="anki-figure"><img src="fake_attr.png"><figcaption>Legenda</figcaption></figure>"#
        );
        let parsed_fake_attr = parse_images_html_to_figures(&html_fake_attr);
        assert_eq!(parsed_fake_attr.len(), 1);
        assert_eq!(parsed_fake_attr[0]["style"], "");

        // Malicious alt attribute in img should NOT grant "clinical" style
        let html_fake_alt = json!(
            r#"<figure><img src="fake_alt.png" alt='class="anki-figure"'><figcaption>Legenda</figcaption></figure>"#
        );
        let parsed_fake_alt = parse_images_html_to_figures(&html_fake_alt);
        assert_eq!(parsed_fake_alt.len(), 1);
        assert_eq!(parsed_fake_alt[0]["style"], "");
    }

    #[test]
    fn no_duplicated_remainder_captions_across_multiple_loose_images() {
        // Multiple loose images: remainder text must not be replicated
        let html_multi_loose = json!(
            r#"Texto inicial <img src="loose_a.png"> texto intermediário <img src="loose_b.png"> texto final"#
        );
        let parsed_multi = parse_images_html_to_figures(&html_multi_loose);
        assert_eq!(parsed_multi.len(), 2);
        assert_eq!(
            parsed_multi[0],
            json!({"filename": "loose_a.png", "caption": "", "style": ""})
        );
        assert_eq!(
            parsed_multi[1],
            json!({"filename": "loose_b.png", "caption": "", "style": ""})
        );

        // Single loose image: remainder text is preserved
        let html_single_loose = json!(r#"Texto único explicativo <img src="single.png">"#);
        let parsed_single = parse_images_html_to_figures(&html_single_loose);
        assert_eq!(parsed_single.len(), 1);
        assert_eq!(
            parsed_single[0],
            json!({"filename": "single.png", "caption": "Texto único explicativo", "style": ""})
        );
    }

    #[test]
    fn malformed_figure_blocks_excluded_before_loose_img_scan() {
        // Malformed figure with broken content should not leak inner images into bare scan
        let html_malformed = json!(
            r#"<figure class="broken"><p>broken table</p><img src="inner_broken.png">trailing text</figure> Fora <img src="real_loose.png">"#
        );
        let parsed = parse_images_html_to_figures(&html_malformed);
        // inner_broken.png was inside a <figure> block (malformed) so it is excluded from loose scan
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed[0],
            json!({"filename": "real_loose.png", "caption": "Fora", "style": ""})
        );
    }
}
