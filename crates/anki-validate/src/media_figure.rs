use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

static EXTRACTOR_WRAPPER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)</?(?:div|span)\b[^>]*(?:statdx-fig-body|anki-fig-title|table-title|table-subinfo|arrow-pill)[^>]*>",
    )
    .expect("valid extractor wrapper regex")
});

static BASE64_IMG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<img\b[^>]*src=["']data:image/[^"']+["'][^>]*>"#)
        .expect("valid base64 img regex")
});

static TABLE_SUBINFO_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)<small>\s*Tabela\s*/\s*Quadro\s+estruturado\s*</small>")
        .expect("valid table subinfo type regex")
});

static TABLE_SUBINFO_DIM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)<small>\s*\d+\s*linhas?\s*[x×]\s*\d+\s*colunas?\s*</small>")
        .expect("valid table subinfo dim regex")
});

static WHITESPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("valid whitespace regex"));

static HTML_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<[^>]+>").expect("valid html tag regex"));

fn default_item_type() -> String {
    "figure".to_string()
}

fn semantic_style_class(style_key: &str) -> Option<&'static str> {
    match style_key {
        "clinical" | "figure" => Some("anki-figure"),
        "table" | "quadro" => Some("anki-figure anki-table-figure"),
        _ => None,
    }
}

fn html_escape_attr(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn clean_extractor_caption(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let s = BASE64_IMG_RE.replace_all(text, "");
    let s = EXTRACTOR_WRAPPER_RE.replace_all(&s, " ");
    let s = TABLE_SUBINFO_TYPE_RE.replace_all(&s, "");
    let s = TABLE_SUBINFO_DIM_RE.replace_all(&s, "");
    let s = WHITESPACE_RE.replace_all(&s, " ");
    s.trim().to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MediaFigure {
    pub filename: String,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub context_link: String,
    #[serde(default = "default_item_type")]
    pub item_type: String,
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub caption_style: String,
}

impl MediaFigure {
    pub fn render_html(&self) -> String {
        let fn_trimmed = self.filename.trim();
        if fn_trimmed.is_empty() {
            return String::new();
        }

        let raw_cap = clean_extractor_caption(&self.caption);
        let title_clean = clean_extractor_caption(&self.title);
        let ctx_clean = clean_extractor_caption(&self.context_link);

        let raw_cap_lower = raw_cap.to_lowercase();
        let mut cap_parts: Vec<String> = Vec::new();
        if !title_clean.is_empty() && !raw_cap_lower.contains(&title_clean.to_lowercase()) {
            cap_parts.push(format!("<b>{title_clean}:</b>"));
        }
        if !raw_cap.is_empty() {
            cap_parts.push(raw_cap);
        }
        if !ctx_clean.is_empty() && !raw_cap_lower.contains(&ctx_clean.to_lowercase()) {
            cap_parts.push(format!("<i>({ctx_clean})</i>"));
        }
        let cap = cap_parts.join(" ").trim().to_string();

        let clean_alt = HTML_TAG_RE.replace_all(&cap, "");
        let escaped_alt = html_escape_attr(clean_alt.trim());
        let alt_attr = format!(r#" alt="{escaped_alt}""#);

        let style_key = self.style.trim().to_lowercase();
        let fig_attr = if style_key.is_empty() {
            if self.item_type == "table" {
                r#" class="anki-figure anki-table-figure""#.to_string()
            } else {
                String::new()
            }
        } else if let Some(cls) = semantic_style_class(&style_key) {
            format!(r#" class="{cls}""#)
        } else {
            format!(r#" style="{}""#, self.style)
        };

        let cap_tag = if cap.is_empty() {
            String::new()
        } else {
            let cap_style = if self.caption_style.is_empty() {
                String::new()
            } else {
                format!(r#" style="{}""#, self.caption_style)
            };
            format!("<figcaption{cap_style}>{cap}</figcaption>")
        };

        format!(r#"<figure{fig_attr}><img src="{fn_trimmed}"{alt_attr}>{cap_tag}</figure>"#)
    }
}

pub fn render_media_figure(fig: &MediaFigure) -> String {
    fig.render_html()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_or_whitespace_filename_returns_empty() {
        let fig = MediaFigure {
            filename: "   \t\n ".to_string(),
            caption: "A caption".to_string(),
            title: "Title".to_string(),
            context_link: String::new(),
            item_type: "figure".to_string(),
            style: String::new(),
            caption_style: String::new(),
        };
        assert_eq!(render_media_figure(&fig), "");
    }

    #[test]
    fn clean_extractor_caption_removes_wrappers_base64_and_subinfo() {
        assert_eq!(clean_extractor_caption(""), "");

        let raw = r#"<div class="statdx-fig-body"><img src="data:image/png;base64,AAAA">  <span class="arrow-pill">Hallmark</span>   finding <small>Tabela / Quadro estruturado</small> <small>4 linhas × 3 colunas</small></div>"#;
        assert_eq!(
            clean_extractor_caption(raw),
            "Hallmark</span> finding </div>"
        );
    }

    #[test]
    fn render_media_figure_escapes_alt_dedups_title_and_maps_styles() {
        let fig = MediaFigure {
            filename: " fig_01.png ".to_string(),
            caption: r#"Shows "A" & 'B' <test>"#.to_string(),
            title: "Figure 1".to_string(),
            context_link: "Section 2".to_string(),
            item_type: "figure".to_string(),
            style: "Clinical".to_string(),
            caption_style: "font-size: 90%".to_string(),
        };
        let rendered = render_media_figure(&fig);
        assert_eq!(
            rendered,
            r#"<figure class="anki-figure"><img src="fig_01.png" alt="Figure 1: Shows &quot;A&quot; &amp; &#x27;B&#x27;  (Section 2)"><figcaption style="font-size: 90%"><b>Figure 1:</b> Shows "A" & 'B' <test> <i>(Section 2)</i></figcaption></figure>"#
        );

        let table_fig = MediaFigure {
            filename: "tbl.png".to_string(),
            caption: "Summary of Table 1".to_string(),
            title: "table 1".to_string(),
            context_link: String::new(),
            item_type: "table".to_string(),
            style: String::new(),
            caption_style: String::new(),
        };
        assert_eq!(
            render_media_figure(&table_fig),
            r#"<figure class="anki-figure anki-table-figure"><img src="tbl.png" alt="Summary of Table 1"><figcaption>Summary of Table 1</figcaption></figure>"#
        );
    }
}
