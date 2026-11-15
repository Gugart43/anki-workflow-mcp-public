use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};

static FIG_PARSE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<figure\b[^>]*>\s*<img\b[^>]*src=["']([^"']+)["'][^>]*>\s*(?:<figcaption\b[^>]*>(.*?)</figcaption>)?\s*</figure>"#,
    )
    .expect("valid figure regex")
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
                let full_tag = caps.get(0).map_or("", |m| m.as_str());
                let filename = caps.get(1).map_or("", |m| m.as_str()).trim();
                let caption = caps.get(2).map_or("", |m| m.as_str()).trim();
                let style = if full_tag.contains("anki-figure") {
                    "clinical"
                } else {
                    ""
                };
                if !filename.is_empty() {
                    figures.push(make_figure_json(filename, caption, style));
                }
            }

            let outside = FIG_PARSE_RE.replace_all(s, "");
            for caps in BARE_IMG_PARSE_RE.captures_iter(&outside) {
                let filename = caps.get(1).map_or("", |m| m.as_str()).trim();
                if !filename.is_empty() {
                    let rem_text = HTML_TAG_RE.replace_all(&outside, " ").trim().to_string();
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
}
