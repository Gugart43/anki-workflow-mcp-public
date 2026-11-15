use regex::Regex;
use std::sync::LazyLock;

static REOPEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&lt;(/?[a-zA-Z]+(?:\s*/?)?)&gt;").expect("valid reopen regex"));

static SEP_CELL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^:?-+:?$").expect("valid separator cell regex"));

static LINE_BREAK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>\s*\n?|\r?\n").expect("valid line break regex"));

const KNOWN_ENTITIES: [&str; 7] = ["amp;", "lt;", "gt;", "le;", "ge;", "#124;", "nbsp;"];

const ALLOWED_TAGS: [&str; 13] = [
    "b", "/b", "i", "/i", "em", "/em", "strong", "/strong", "sub", "/sub", "sup", "/sup", "br",
];

#[must_use]
pub fn escape_cell(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    let mut escaped_amp = String::with_capacity(text.len());
    let mut last = 0;
    for (idx, _) in text.match_indices('&') {
        escaped_amp.push_str(&text[last..idx]);
        let rest = &text[idx + 1..];
        if KNOWN_ENTITIES.iter().any(|ent| rest.starts_with(ent)) {
            escaped_amp.push('&');
        } else {
            escaped_amp.push_str("&amp;");
        }
        last = idx + 1;
    }
    escaped_amp.push_str(&text[last..]);

    let escaped_angles = escaped_amp.replace('<', "&lt;").replace('>', "&gt;");

    let reopened = REOPEN_RE.replace_all(&escaped_angles, |caps: &regex::Captures<'_>| {
        let inner = caps
            .get(1)
            .map_or("", |m| m.as_str())
            .trim()
            .to_ascii_lowercase();
        if inner == "br/" || inner == "br /" {
            "<br>".to_string()
        } else if ALLOWED_TAGS.contains(&inner.as_str()) {
            format!("<{inner}>")
        } else {
            caps.get(0).map_or("", |m| m.as_str()).to_string()
        }
    });

    reopened.replace('|', "&#124;")
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct TableSpec {
    #[serde(default)]
    pub caption: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl TableSpec {
    pub fn render_html(&self) -> Result<String, String> {
        if self.headers.is_empty() {
            return Err("headers must not be empty".to_string());
        }
        let expected_len = self.headers.len();
        for (idx, row) in self.rows.iter().enumerate() {
            if row.len() != expected_len {
                return Err(format!(
                    "Row {idx} length {} does not match headers length {expected_len}: {row:?}",
                    row.len()
                ));
            }
        }

        let mut out = String::from("<table>");
        let caption_trimmed = self.caption.trim();
        if !caption_trimmed.is_empty() {
            out.push_str("<caption>");
            out.push_str(&escape_cell(caption_trimmed));
            out.push_str("</caption>");
        }
        out.push_str("<thead><tr>");
        for h in &self.headers {
            out.push_str("<th>");
            out.push_str(&escape_cell(h));
            out.push_str("</th>");
        }
        out.push_str("</tr></thead><tbody>");
        for row in &self.rows {
            out.push_str("<tr>");
            for cell in row {
                out.push_str("<td>");
                out.push_str(&escape_cell(cell));
                out.push_str("</td>");
            }
            out.push_str("</tr>");
        }
        out.push_str("</tbody></table>");
        Ok(out)
    }
}

fn split_markdown_cells(line: &str) -> Vec<String> {
    let mut s = line.trim();
    let delim = if s.contains('|') {
        "|"
    } else if s.contains("&#124;") {
        "&#124;"
    } else {
        return vec![s.to_string()];
    };

    if let Some(stripped) = s.strip_prefix(delim) {
        s = stripped;
    }
    if let Some(stripped) = s.strip_suffix(delim) {
        s = stripped;
    }
    s.split(delim).map(|c| c.trim().to_string()).collect()
}

fn is_separator_row(line: &str) -> bool {
    let mut s = line.trim();
    if s.is_empty() {
        return false;
    }
    let delim = if s.contains('|') {
        "|"
    } else if s.contains("&#124;") {
        "&#124;"
    } else {
        return false;
    };

    if let Some(stripped) = s.strip_prefix(delim) {
        s = stripped;
    }
    if let Some(stripped) = s.strip_suffix(delim) {
        s = stripped;
    }
    s = s.trim();
    if s.is_empty() {
        return false;
    }

    let cells: Vec<&str> = s.split(delim).map(str::trim).collect();
    !cells.is_empty() && cells.iter().all(|c| SEP_CELL_RE.is_match(c))
}

fn is_table_row(line: &str) -> bool {
    let s = line.trim();
    if s.is_empty() {
        return false;
    }
    if !s.contains('|') && !s.contains("&#124;") {
        return false;
    }
    if is_separator_row(s) {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("<table") || lower.starts_with("<div") || lower.starts_with("<p") {
        return false;
    }
    true
}

fn normalize_cells(mut cells: Vec<String>, expected: usize) -> Vec<String> {
    if cells.len() < expected {
        cells.resize(expected, String::new());
        cells
    } else if cells.len() > expected {
        let surplus = cells.split_off(expected - 1).join(" ");
        cells.push(surplus);
        cells
    } else {
        cells
    }
}

#[must_use]
pub fn markdown_tables_to_html(text: &str) -> (String, usize) {
    if text.is_empty() {
        return (String::new(), 0);
    }

    let mut segments: Vec<(&str, &str)> = Vec::new();
    let mut last = 0;
    for m in LINE_BREAK_RE.find_iter(text) {
        segments.push((&text[last..m.start()], m.as_str()));
        last = m.end();
    }
    segments.push((&text[last..], ""));

    let mut out = String::with_capacity(text.len());
    let mut tables_count = 0usize;
    let mut i = 0usize;
    let n = segments.len();

    while i < n {
        let (line_content, line_sep) = segments[i];
        if i + 1 < n
            && (line_content.contains('|') || line_content.contains("&#124;"))
            && !is_separator_row(line_content)
            && is_separator_row(segments[i + 1].0)
        {
            let headers = split_markdown_cells(line_content);
            let expected_cols = headers.len();
            let mut rows: Vec<Vec<String>> = Vec::new();
            let mut j = i + 2;
            while j < n && is_table_row(segments[j].0) {
                let row_cells = normalize_cells(split_markdown_cells(segments[j].0), expected_cols);
                rows.push(row_cells);
                j += 1;
            }
            let spec = TableSpec {
                caption: String::new(),
                headers,
                rows,
            };
            if let Ok(table_html) = spec.render_html() {
                out.push_str(&table_html);
            }
            out.push_str(segments[j - 1].1);
            tables_count += 1;
            i = j;
        } else {
            out.push_str(line_content);
            out.push_str(line_sep);
            i += 1;
        }
    }

    if tables_count == 0 {
        return (text.to_string(), 0);
    }
    (out, tables_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_cell_preserves_entities_and_allowed_tags_and_escapes_pipes() {
        assert_eq!(escape_cell(""), "");
        let input = "A & B &amp; &le; <b>bold</b> <script>x</script> <br/> a|b";
        let expected =
            "A &amp; B &amp; &le; <b>bold</b> &lt;script&gt;x&lt;/script&gt; <br> a&#124;b";
        assert_eq!(escape_cell(input), expected);
    }

    #[test]
    fn table_spec_rejects_empty_headers_and_ragged_rows() {
        let empty_headers = TableSpec {
            caption: String::new(),
            headers: vec![],
            rows: vec![],
        };
        assert!(empty_headers.render_html().is_err());

        let ragged = TableSpec {
            caption: "Cap".to_string(),
            headers: vec!["A".to_string(), "B".to_string()],
            rows: vec![vec!["1".to_string()]],
        };
        assert!(ragged.render_html().is_err());
    }

    #[test]
    fn markdown_tables_to_html_handles_empty_and_no_tables() {
        assert_eq!(markdown_tables_to_html(""), (String::new(), 0));
        let plain = "Just some text\nwith no table";
        assert_eq!(markdown_tables_to_html(plain), (plain.to_string(), 0));
    }

    #[test]
    fn markdown_tables_to_html_converts_and_normalizes_rows() {
        let md = "Intro<br>| H1 | H2 |<br>| --- | :---: |<br>| a |<br>| x | y | z |<br>Outro";
        let (html, count) = markdown_tables_to_html(md);
        assert_eq!(count, 1);
        assert_eq!(
            html,
            "Intro<br><table><thead><tr><th>H1</th><th>H2</th></tr></thead><tbody><tr><td>a</td><td></td></tr><tr><td>x</td><td>y z</td></tr></tbody></table><br>Outro"
        );
    }
}
