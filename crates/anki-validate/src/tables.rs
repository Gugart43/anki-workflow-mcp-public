use std::collections::HashSet;
use std::sync::OnceLock;

use anki_domain::{Diagnostic, Severity};
use regex::Regex;

pub const MAX_COLS: usize = 4;
pub const MAX_BODY_ROWS: usize = 8;
pub const MAX_CELL_CHARS: usize = 120;

const OWNER_SKILL: &str = "anki-formatting-guide";

fn sep_cell_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^:?-+:?$").expect("valid sep_cell regex"))
}

fn line_break_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)<br\s*/?>\s*\n?|\r?\n").expect("valid line_break regex"))
}

fn is_table_tag(tag_lower: &str) -> bool {
    matches!(
        tag_lower,
        "table" | "thead" | "tbody" | "tfoot" | "tr" | "th" | "td" | "caption"
    )
}

fn is_forbidden_attr(attr_lower: &str) -> bool {
    matches!(
        attr_lower,
        "style" | "width" | "height" | "bgcolor" | "align" | "border" | "cellpadding"
    )
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
    if let Some(rest) = s.strip_prefix(delim) {
        s = rest;
    }
    if let Some(rest) = s.strip_suffix(delim) {
        s = rest;
    }
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    let re = sep_cell_re();
    let cells: Vec<&str> = s.split(delim).map(str::trim).collect();
    !cells.is_empty() && cells.iter().all(|c| re.is_match(c))
}

struct TableHtmlParser<'a> {
    field: &'a str,
    row: Option<usize>,
    face: bool,
    diags: Vec<Diagnostic>,
    tag_stack: Vec<String>,
    table_depth: usize,
    has_table: bool,
    current_has_thead: bool,
    row_cols: Vec<usize>,
    row_is_body: Vec<bool>,
    first_row_th: bool,
    current_row_cols: usize,
    current_row_has_th: bool,
    current_row_in_thead: bool,
    current_cell_chars: usize,
    in_cell: bool,
}

impl<'a> TableHtmlParser<'a> {
    fn new(field: &'a str, row: Option<usize>, face: bool) -> Self {
        Self {
            field,
            row,
            face,
            diags: Vec::new(),
            tag_stack: Vec::new(),
            table_depth: 0,
            has_table: false,
            current_has_thead: false,
            row_cols: Vec::new(),
            row_is_body: Vec::new(),
            first_row_th: false,
            current_row_cols: 0,
            current_row_has_th: false,
            current_row_in_thead: false,
            current_cell_chars: 0,
            in_cell: false,
        }
    }

    fn emit(
        &mut self,
        code: &'static str,
        severity: Severity,
        message: String,
        suggestion: Option<String>,
    ) {
        let mut diag = Diagnostic::from_static_code(code, severity, OWNER_SKILL, message)
            .with_field(self.field);
        if let Some(r) = self.row {
            diag = diag.with_row(r);
        }
        if let Some(s) = suggestion {
            diag = diag.with_suggestion(s);
        }
        self.diags.push(diag);
    }

    fn handle_starttag(&mut self, tag: &str, attrs: &[(String, Option<String>)]) {
        let tag_lower = tag.to_ascii_lowercase();
        if !is_table_tag(&tag_lower) {
            return;
        }

        for (attr, _) in attrs {
            if is_forbidden_attr(&attr.to_ascii_lowercase()) {
                self.emit(
                    "TBL.INLINE_STYLE",
                    Severity::Warn,
                    format!(
                        "Inline presentation attribute '{attr}' on <{tag_lower}> overrides note type theme"
                    ),
                    Some(
                        "Remove inline style/width/bgcolor attributes and rely on the AnkiMBR CSS"
                            .to_string(),
                    ),
                );
            }
        }

        match tag_lower.as_str() {
            "table" => {
                self.has_table = true;
                if self.face {
                    self.emit(
                        "TBL.IN_CARD_FACE",
                        Severity::Warn,
                        "Table in Enunciado/Resposta/Sentença (move to Notas Adicionais or split the card)"
                            .to_string(),
                        Some("Move table to Notas Adicionais or split the card".to_string()),
                    );
                }
                if self.table_depth > 0 {
                    self.emit(
                        "TBL.NESTED",
                        Severity::Critical,
                        "Nested <table> is not allowed".to_string(),
                        Some(
                            "Flatten nested tables into separate tables or linear text".to_string(),
                        ),
                    );
                }
                self.table_depth += 1;
                self.current_has_thead = false;
                self.row_cols.clear();
                self.row_is_body.clear();
                self.first_row_th = false;
                self.tag_stack.push(tag_lower);
            }
            "thead" | "tbody" | "tfoot" | "caption" => {
                if self.tag_stack.last().map(String::as_str) != Some("table") {
                    self.emit(
                        "TBL.MALFORMED",
                        Severity::Critical,
                        format!("<{tag_lower}> must be directly inside <table>"),
                        Some("Check table structure and nesting".to_string()),
                    );
                }
                if tag_lower == "thead" {
                    self.current_has_thead = true;
                }
                self.tag_stack.push(tag_lower);
            }
            "tr" => {
                let valid_parent = matches!(
                    self.tag_stack.last().map(String::as_str),
                    Some("table" | "thead" | "tbody" | "tfoot")
                );
                if !valid_parent {
                    self.emit(
                        "TBL.MALFORMED",
                        Severity::Critical,
                        "<tr> must be inside <table>, <thead>, <tbody>, or <tfoot>".to_string(),
                        Some("Check table row placement".to_string()),
                    );
                }
                self.current_row_cols = 0;
                self.current_row_has_th = false;
                self.current_row_in_thead = self.tag_stack.iter().any(|t| t == "thead");
                self.tag_stack.push(tag_lower);
            }
            "th" | "td" => {
                if self.tag_stack.last().map(String::as_str) != Some("tr") {
                    self.emit(
                        "TBL.MALFORMED",
                        Severity::Critical,
                        format!("<{tag_lower}> must be directly inside <tr>"),
                        Some("Check table cell placement".to_string()),
                    );
                }
                self.in_cell = true;
                self.current_cell_chars = 0;
                if tag_lower == "th" {
                    self.current_row_has_th = true;
                }

                let mut colspan = 1usize;
                for (attr, val) in attrs {
                    if attr.eq_ignore_ascii_case("colspan")
                        && let Some(v) = val
                        && let Ok(cs) = v.trim().parse::<usize>()
                        && cs > 0
                    {
                        colspan = cs;
                    }
                }
                self.current_row_cols += colspan;
                self.tag_stack.push(tag_lower);
            }
            _ => {}
        }
    }

    fn handle_data(&mut self, data: &str) {
        if self.in_cell {
            self.current_cell_chars += data.chars().count();
        }
    }

    fn handle_endtag(&mut self, tag: &str) {
        let tag_lower = tag.to_ascii_lowercase();
        if !is_table_tag(&tag_lower) {
            return;
        }

        if self.tag_stack.last().map(String::as_str) != Some(tag_lower.as_str()) {
            self.emit(
                "TBL.MALFORMED",
                Severity::Critical,
                format!("Unbalanced or unexpected closing tag </{tag_lower}>"),
                Some("Ensure all table tags are matched and closed in proper order".to_string()),
            );
            if self.tag_stack.iter().any(|t| t == &tag_lower) {
                while let Some(top) = self.tag_stack.last() {
                    if top == &tag_lower {
                        break;
                    }
                    self.tag_stack.pop();
                }
                self.tag_stack.pop();
            }
            return;
        }

        self.tag_stack.pop();

        match tag_lower.as_str() {
            "th" | "td" => {
                self.in_cell = false;
                if self.current_cell_chars > MAX_CELL_CHARS {
                    self.emit(
                        "TBL.CELL_TOO_LONG",
                        Severity::Warn,
                        format!(
                            "Table cell text too long ({} chars, max {MAX_CELL_CHARS})",
                            self.current_cell_chars
                        ),
                        Some(format!(
                            "Shorten table cell content to <= {MAX_CELL_CHARS} visible characters"
                        )),
                    );
                }
                self.current_cell_chars = 0;
            }
            "tr" => {
                let row_idx = self.row_cols.len();
                if row_idx == 0 {
                    self.first_row_th = self.current_row_has_th;
                }
                self.row_cols.push(self.current_row_cols);
                let mut is_body = !self.current_row_in_thead;
                if !self.current_has_thead && row_idx == 0 && self.first_row_th {
                    is_body = false;
                }
                self.row_is_body.push(is_body);
            }
            "table" => {
                self.table_depth = self.table_depth.saturating_sub(1);
                let unique_cols: HashSet<usize> = self.row_cols.iter().copied().collect();
                if self.row_cols.len() > 1 && unique_cols.len() > 1 {
                    self.emit(
                        "TBL.RAGGED",
                        Severity::Critical,
                        format!("Rows have inconsistent column counts: {:?}", self.row_cols),
                        Some(
                            "Ensure all rows have the same number of columns (matching the header)"
                                .to_string(),
                        ),
                    );
                }
                if !self.current_has_thead && !self.first_row_th {
                    self.emit(
                        "TBL.NO_HEADER",
                        Severity::Warn,
                        "Table without a header row (<thead>/<th>)".to_string(),
                        Some(
                            "Add a <thead><tr><th>...</th></tr></thead> row to provide context for columns"
                                .to_string(),
                        ),
                    );
                }
                let max_cols = self.row_cols.iter().copied().max().unwrap_or(0);
                if max_cols > MAX_COLS {
                    self.emit(
                        "TBL.TOO_WIDE",
                        Severity::Warn,
                        format!("Table has {max_cols} columns (maximum recommended: {MAX_COLS})"),
                        Some("Dividir a tabela ou transpor".to_string()),
                    );
                }
                let body_rows = self.row_is_body.iter().filter(|&&b| b).count();
                if body_rows > MAX_BODY_ROWS {
                    self.emit(
                        "TBL.TOO_LONG",
                        Severity::Warn,
                        format!(
                            "Table has {body_rows} body rows (maximum recommended: {MAX_BODY_ROWS})"
                        ),
                        Some(format!(
                            "Keep table body to <= {MAX_BODY_ROWS} rows to preserve card atomicity"
                        )),
                    );
                }
            }
            _ => {}
        }
    }

    fn finalize(&mut self) {
        if !self.tag_stack.is_empty() {
            let stack_repr = format!(
                "[{}]",
                self.tag_stack
                    .iter()
                    .map(|tag| format!("'{tag}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            self.emit(
                "TBL.MALFORMED",
                Severity::Critical,
                format!("Unclosed table tags at end of field: {stack_repr}"),
                Some("Ensure all table tags (table, tr, td, th) are properly closed".to_string()),
            );
        }
    }

    fn feed(&mut self, html: &str) {
        let bytes = html.as_bytes();
        let len = bytes.len();
        let mut pos = 0usize;

        while pos < len {
            if let Some(rel_lt) = html[pos..].find('<') {
                if rel_lt > 0 {
                    self.handle_data(&html[pos..pos + rel_lt]);
                    pos += rel_lt;
                }

                if html[pos..].starts_with("<!--") {
                    if let Some(end_comment) = html[pos + 4..].find("-->") {
                        pos += 4 + end_comment + 3;
                    } else {
                        self.handle_data(&html[pos..]);
                        break;
                    }
                    continue;
                }

                if let Some(parsed) = parse_tag_at(&html[pos..]) {
                    match parsed.kind {
                        TagKind::Start {
                            name,
                            attrs,
                            self_closing,
                        } => {
                            self.handle_starttag(&name, &attrs);
                            if self_closing {
                                self.handle_endtag(&name);
                            }
                        }
                        TagKind::End { name } => {
                            self.handle_endtag(&name);
                        }
                    }
                    pos += parsed.consumed;
                } else {
                    self.handle_data("<");
                    pos += 1;
                }
            } else {
                self.handle_data(&html[pos..]);
                break;
            }
        }
    }
}

enum TagKind {
    Start {
        name: String,
        attrs: Vec<(String, Option<String>)>,
        self_closing: bool,
    },
    End {
        name: String,
    },
}

struct ParsedTag {
    kind: TagKind,
    consumed: usize,
}

fn parse_tag_at(s: &str) -> Option<ParsedTag> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'<') {
        return None;
    }

    let mut i = 1usize;
    let is_end = if bytes.get(i) == Some(&b'/') {
        i += 1;
        true
    } else {
        false
    };

    let name_start = i;
    let first_ch = *bytes.get(i)?;
    if !first_ch.is_ascii_alphabetic() {
        return None;
    }
    while i < bytes.len()
        && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b':')
    {
        i += 1;
    }
    let name = s[name_start..i].to_string();

    if is_end {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if bytes.get(i) == Some(&b'>') {
            return Some(ParsedTag {
                kind: TagKind::End { name },
                consumed: i + 1,
            });
        }
        return None;
    }

    let mut attrs = Vec::new();
    let mut self_closing = false;

    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        if bytes[i] == b'>' {
            return Some(ParsedTag {
                kind: TagKind::Start {
                    name,
                    attrs,
                    self_closing,
                },
                consumed: i + 1,
            });
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'>') {
            self_closing = true;
            return Some(ParsedTag {
                kind: TagKind::Start {
                    name,
                    attrs,
                    self_closing,
                },
                consumed: i + 2,
            });
        }

        let attr_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
            && bytes[i] != b'>'
            && bytes[i] != b'/'
        {
            i += 1;
        }
        if i == attr_start {
            i += 1;
            continue;
        }
        let attr_name = s[attr_start..i].to_string();

        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }

        let attr_val = if bytes.get(i) == Some(&b'=') {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() {
                return None;
            }
            if bytes[i] == b'"' || bytes[i] == b'\'' {
                let quote = bytes[i];
                i += 1;
                let val_start = i;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                if i >= bytes.len() {
                    return None;
                }
                let val = s[val_start..i].to_string();
                i += 1;
                Some(val)
            } else {
                let val_start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                    i += 1;
                }
                Some(s[val_start..i].to_string())
            }
        } else {
            None
        };

        attrs.push((attr_name, attr_val));
    }
}

#[must_use]
pub fn check_tables_field(
    html: &str,
    field: &str,
    row: Option<usize>,
    face: bool,
) -> Vec<Diagnostic> {
    if html.trim().is_empty() {
        return Vec::new();
    }

    let mut diags = Vec::new();

    let lines: Vec<&str> = line_break_re().split(html).collect();
    let pipe_lines_count = lines
        .iter()
        .filter(|line| line.matches('|').count() + line.matches("&#124;").count() >= 3)
        .count();
    let has_sep = lines.iter().any(|line| is_separator_row(line));
    if has_sep || pipe_lines_count >= 2 {
        let mut diag = Diagnostic::new(
            anki_domain::DiagnosticCode::from_static("TBL.MARKDOWN_PIPE"),
            Severity::Critical,
            OWNER_SKILL,
            "Markdown table or raw pipe syntax detected in field (collides with CSV delimiter)",
        )
        .with_field(field)
        .with_suggestion("Convert markdown tables to HTML via table_from_markdown or table_build");
        if let Some(r) = row {
            diag = diag.with_row(r);
        }
        diags.push(diag);
    }

    let mut parser = TableHtmlParser::new(field, row, face);
    parser.feed(html);
    parser.finalize();
    diags.extend(parser.diags);
    diags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_field_returns_no_diagnostics() {
        assert!(check_tables_field("", "Notas Adicionais", Some(1), false).is_empty());
        assert!(check_tables_field("   \n\t ", "Notas Adicionais", None, true).is_empty());
    }

    #[test]
    fn valid_table_with_literal_less_than_returns_no_diagnostics() {
        let html = "<table><thead><tr><th>A</th><th>B</th></tr></thead><tbody><tr><td>x &lt; 5 and y < 10</td><td>ok</td></tr></tbody></table>";
        let diags = check_tables_field(html, "Notas Adicionais", Some(2), false);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn detects_markdown_pipe_table() {
        let md = "| A | B | C |\n|---|---|---|\n| 1 | 2 | 3 |";
        let diags = check_tables_field(md, "Notas Adicionais", Some(3), false);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "TBL.MARKDOWN_PIPE");
        assert_eq!(diags[0].severity, Severity::Critical);
        assert_eq!(diags[0].row, Some(3));
        assert_eq!(diags[0].field.as_deref(), Some("Notas Adicionais"));
    }

    #[test]
    fn detects_ragged_nested_inline_style_and_face() {
        let html =
            r#"<table style="width:100%"><tr><th>A</th><th>B</th></tr><tr><td>1</td></tr></table>"#;
        let diags = check_tables_field(html, "Enunciado", None, true);
        let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
        assert!(codes.contains(&"TBL.INLINE_STYLE"));
        assert!(codes.contains(&"TBL.IN_CARD_FACE"));
        assert!(codes.contains(&"TBL.RAGGED"));
        assert!(diags.iter().all(|d| d.row.is_none()));
    }

    #[test]
    fn detects_malformed_unclosed_table() {
        let html = "<table><tr><td>Unclosed";
        let diags = check_tables_field(html, "Notas Adicionais", Some(5), false);
        assert!(diags.iter().any(|d| d.code == "TBL.MALFORMED"));
    }
}
