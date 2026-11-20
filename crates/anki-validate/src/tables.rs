use std::collections::HashSet;
use std::sync::OnceLock;

use anki_domain::{CsvRowNumber, Diagnostic, DiagnosticCode, Severity};
use regex::Regex;

use crate::html_text::{strip_table_blocks, visible_char_count};

pub const MAX_COLS: usize = 4;
pub const MAX_BODY_ROWS: usize = 8;
pub const MAX_CELL_CHARS: usize = 120;

pub const CODE_TBL_INLINE_STYLE: DiagnosticCode = DiagnosticCode::from_static("TBL.INLINE_STYLE");
pub const CODE_TBL_IN_CARD_FACE: DiagnosticCode = DiagnosticCode::from_static("TBL.IN_CARD_FACE");
pub const CODE_TBL_NESTED: DiagnosticCode = DiagnosticCode::from_static("TBL.NESTED");
pub const CODE_TBL_MALFORMED: DiagnosticCode = DiagnosticCode::from_static("TBL.MALFORMED");
pub const CODE_TBL_CELL_TOO_LONG: DiagnosticCode = DiagnosticCode::from_static("TBL.CELL_TOO_LONG");
pub const CODE_TBL_RAGGED: DiagnosticCode = DiagnosticCode::from_static("TBL.RAGGED");
pub const CODE_TBL_NO_HEADER: DiagnosticCode = DiagnosticCode::from_static("TBL.NO_HEADER");
pub const CODE_TBL_TOO_WIDE: DiagnosticCode = DiagnosticCode::from_static("TBL.TOO_WIDE");
pub const CODE_TBL_TOO_LONG: DiagnosticCode = DiagnosticCode::from_static("TBL.TOO_LONG");
pub const CODE_TBL_MARKDOWN_PIPE: DiagnosticCode = DiagnosticCode::from_static("TBL.MARKDOWN_PIPE");

const OWNER_SKILL: &str = "anki-formatting-guide";

fn sep_cell_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^:?-+:?$").expect("valid sep_cell regex"))
}

fn line_break_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)<br\s*/?>\s*\n?|\r?\n").expect("valid line_break regex"))
}

fn mathjax_spans_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?s)\\\((.*?)\\\)|\\\[(.*?)\\\]").expect("valid mathjax_spans regex")
    })
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
    !cells.is_empty()
        && (cells.len() >= 2 || cells[0].matches('-').count() >= 3)
        && cells.iter().all(|c| re.is_match(c))
}

#[derive(Debug, Default)]
struct TableState {
    has_thead: bool,
    row_cols: Vec<usize>,
    row_is_body: Vec<bool>,
    first_row_th: bool,
    pending_rowspans: Vec<(usize, usize)>,
    current_row_new_rowspans: Vec<(usize, usize)>,
    current_row_occupied_cols: HashSet<usize>,
    current_col_idx: usize,
    current_row_has_th: bool,
    current_row_in_thead: bool,
}

struct TableHtmlParser<'a> {
    field: &'a str,
    row: Option<usize>,
    face: bool,
    diags: Vec<Diagnostic>,
    tag_stack: Vec<String>,
    table_stack: Vec<TableState>,
    has_table: bool,
    cell_chars_stack: Vec<String>,
}

impl<'a> TableHtmlParser<'a> {
    fn new(field: &'a str, row: Option<usize>, face: bool) -> Self {
        Self {
            field,
            row,
            face,
            diags: Vec::new(),
            tag_stack: Vec::new(),
            table_stack: Vec::new(),
            has_table: false,
            cell_chars_stack: Vec::new(),
        }
    }

    fn emit(
        &mut self,
        code: DiagnosticCode,
        severity: Severity,
        message: String,
        suggestion: Option<String>,
    ) {
        let mut diag = Diagnostic::new(code, severity, OWNER_SKILL, message).with_field(self.field);
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
                    CODE_TBL_INLINE_STYLE,
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
                        CODE_TBL_IN_CARD_FACE,
                        Severity::Warn,
                        "Table in Enunciado/Resposta/Sentença (move to Notas Adicionais or split the card)"
                            .to_string(),
                        Some("Move table to Notas Adicionais or split the card".to_string()),
                    );
                }
                if !self.table_stack.is_empty() {
                    self.emit(
                        CODE_TBL_NESTED,
                        Severity::Critical,
                        "Nested <table> is not allowed".to_string(),
                        Some(
                            "Flatten nested tables into separate tables or linear text".to_string(),
                        ),
                    );
                }
                self.table_stack.push(TableState::default());
                self.tag_stack.push(tag_lower);
            }
            "thead" | "tbody" | "tfoot" | "caption" => {
                if self.tag_stack.last().map(String::as_str) != Some("table") {
                    self.emit(
                        CODE_TBL_MALFORMED,
                        Severity::Critical,
                        format!("<{tag_lower}> must be directly inside <table>"),
                        Some("Check table structure and nesting".to_string()),
                    );
                }
                if tag_lower == "thead"
                    && let Some(state) = self.table_stack.last_mut()
                {
                    state.has_thead = true;
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
                        CODE_TBL_MALFORMED,
                        Severity::Critical,
                        "<tr> must be inside <table>, <thead>, <tbody>, or <tfoot>".to_string(),
                        Some("Check table row placement".to_string()),
                    );
                }
                if let Some(state) = self.table_stack.last_mut() {
                    state.current_col_idx = 0;
                    state.current_row_occupied_cols.clear();
                    for &(col_idx, _) in &state.pending_rowspans {
                        state.current_row_occupied_cols.insert(col_idx);
                    }
                    state.current_row_new_rowspans.clear();
                    state.current_row_has_th = false;
                    state.current_row_in_thead = self.tag_stack.iter().any(|t| t == "thead");
                }
                self.tag_stack.push(tag_lower);
            }
            "th" | "td" => {
                if self.tag_stack.last().map(String::as_str) != Some("tr") {
                    self.emit(
                        CODE_TBL_MALFORMED,
                        Severity::Critical,
                        format!("<{tag_lower}> must be directly inside <tr>"),
                        Some("Check table cell placement".to_string()),
                    );
                }
                self.cell_chars_stack.push(String::new());

                let mut colspan = 1usize;
                let mut rowspan = 1usize;
                for (attr, val) in attrs {
                    if attr.eq_ignore_ascii_case("colspan")
                        && let Some(v) = val
                        && let Ok(cs) = v.trim().parse::<usize>()
                        && cs > 0
                    {
                        colspan = cs;
                    }
                    if attr.eq_ignore_ascii_case("rowspan")
                        && let Some(v) = val
                        && let Ok(rs) = v.trim().parse::<usize>()
                        && rs > 0
                    {
                        rowspan = rs;
                    }
                }

                let mut collided_col: Option<usize> = None;
                if let Some(state) = self.table_stack.last_mut() {
                    if tag_lower == "th" {
                        state.current_row_has_th = true;
                    }

                    while state
                        .current_row_occupied_cols
                        .contains(&state.current_col_idx)
                    {
                        state.current_col_idx += 1;
                    }
                    let cell_col_start = state.current_col_idx;
                    for c in 0..colspan {
                        let col = cell_col_start + c;
                        if !state.current_row_occupied_cols.insert(col) && collided_col.is_none() {
                            collided_col = Some(col);
                        }
                        if rowspan > 1 {
                            state.current_row_new_rowspans.push((col, rowspan - 1));
                        }
                    }
                    state.current_col_idx = cell_col_start + colspan;
                }
                if let Some(col) = collided_col {
                    self.emit(
                        CODE_TBL_MALFORMED,
                        Severity::Critical,
                        format!(
                            "Cell colspan={colspan} overlaps an active rowspan at column {}",
                            col + 1
                        ),
                        Some(
                            "Adjust colspan/rowspan attributes so cells do not overlap".to_string(),
                        ),
                    );
                }
                self.tag_stack.push(tag_lower);
            }
            _ => {}
        }
    }

    fn handle_data(&mut self, data: &str) {
        if let Some(cell_buf) = self.cell_chars_stack.last_mut() {
            cell_buf.push_str(data);
        }
    }

    fn handle_endtag(&mut self, tag: &str) {
        let tag_lower = tag.to_ascii_lowercase();
        if !is_table_tag(&tag_lower) {
            return;
        }

        if self.tag_stack.last().map(String::as_str) != Some(tag_lower.as_str()) {
            self.emit(
                CODE_TBL_MALFORMED,
                Severity::Critical,
                format!("Unbalanced or unexpected closing tag </{tag_lower}>"),
                Some("Ensure all table tags are matched and closed in proper order".to_string()),
            );
            if self.tag_stack.iter().any(|t| t == &tag_lower) {
                while let Some(top) = self.tag_stack.pop() {
                    if matches!(top.as_str(), "td" | "th") {
                        self.cell_chars_stack.pop();
                    } else if top == "table" {
                        self.table_stack.pop();
                    }
                    if top == tag_lower {
                        break;
                    }
                }
            }
            return;
        }

        self.tag_stack.pop();

        match tag_lower.as_str() {
            "th" | "td" => {
                if let Some(cell_text) = self.cell_chars_stack.pop() {
                    let chars = visible_char_count(&cell_text);
                    if chars > MAX_CELL_CHARS {
                        self.emit(
                            CODE_TBL_CELL_TOO_LONG,
                            Severity::Warn,
                            format!(
                                "Table cell text too long ({chars} chars, max {MAX_CELL_CHARS})"
                            ),
                            Some(format!(
                                "Shorten table cell content to <= {MAX_CELL_CHARS} visible characters"
                            )),
                        );
                    }
                }
            }
            "thead" | "tbody" | "tfoot" => {
                if let Some(state) = self.table_stack.last_mut() {
                    state.pending_rowspans.clear();
                }
            }
            "tr" => {
                if let Some(state) = self.table_stack.last_mut() {
                    let row_cols_count = state
                        .current_row_occupied_cols
                        .iter()
                        .max()
                        .map_or(0, |&max_c| max_c + 1);

                    let row_idx = state.row_cols.len();
                    if row_idx == 0 {
                        state.first_row_th = state.current_row_has_th;
                    }
                    state.row_cols.push(row_cols_count);
                    let mut is_body = !state.current_row_in_thead;
                    if !state.has_thead && row_idx == 0 && state.first_row_th {
                        is_body = false;
                    }
                    state.row_is_body.push(is_body);

                    for (_, remaining) in &mut state.pending_rowspans {
                        *remaining -= 1;
                    }
                    state
                        .pending_rowspans
                        .retain(|&(_, remaining)| remaining > 0);
                    state
                        .pending_rowspans
                        .append(&mut state.current_row_new_rowspans);
                }
            }
            "table" => {
                if let Some(state) = self.table_stack.pop() {
                    let unique_cols: HashSet<usize> = state.row_cols.iter().copied().collect();
                    if state.row_cols.len() > 1 && unique_cols.len() > 1 {
                        self.emit(
                            CODE_TBL_RAGGED,
                            Severity::Critical,
                            format!("Rows have inconsistent column counts: {:?}", state.row_cols),
                            Some(
                                "Ensure all rows have the same number of columns (matching the header)"
                                    .to_string(),
                            ),
                        );
                    }
                    if !state.has_thead && !state.first_row_th {
                        self.emit(
                            CODE_TBL_NO_HEADER,
                            Severity::Warn,
                            "Table without a header row (<thead>/<th>)".to_string(),
                            Some(
                                "Add a <thead><tr><th>...</th></tr></thead> row to provide context for columns"
                                    .to_string(),
                            ),
                        );
                    }
                    let max_cols = state.row_cols.iter().copied().max().unwrap_or(0);
                    if max_cols > MAX_COLS {
                        self.emit(
                            CODE_TBL_TOO_WIDE,
                            Severity::Warn,
                            format!(
                                "Table has {max_cols} columns (maximum recommended: {MAX_COLS})"
                            ),
                            Some("Dividir a tabela ou transpor".to_string()),
                        );
                    }
                    let body_rows = state.row_is_body.iter().filter(|&&b| b).count();
                    if body_rows > MAX_BODY_ROWS {
                        self.emit(
                            CODE_TBL_TOO_LONG,
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
                CODE_TBL_MALFORMED,
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
                    let raw_tag = &html[pos..pos + parsed.consumed];
                    match parsed.kind {
                        TagKind::Start {
                            name,
                            attrs,
                            self_closing,
                        } => {
                            if is_table_tag(&name.to_ascii_lowercase()) {
                                self.handle_starttag(&name, &attrs);
                                if self_closing {
                                    self.handle_endtag(&name);
                                }
                            } else {
                                self.handle_data(raw_tag);
                            }
                        }
                        TagKind::End { name } => {
                            if is_table_tag(&name.to_ascii_lowercase()) {
                                self.handle_endtag(&name);
                            } else {
                                self.handle_data(raw_tag);
                            }
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
    check_tables_field_internal(html, field, row, face)
}

#[must_use]
pub fn check_tables_field_csv(
    html: &str,
    field: &str,
    row: Option<CsvRowNumber>,
    face: bool,
) -> Vec<Diagnostic> {
    check_tables_field_internal(html, field, row.map(Into::into), face)
}

fn check_tables_field_internal(
    html: &str,
    field: &str,
    row: Option<usize>,
    face: bool,
) -> Vec<Diagnostic> {
    if html.trim().is_empty() {
        return Vec::new();
    }

    let mut diags = Vec::new();

    let clean_for_md = strip_table_blocks(html);
    let clean_for_md = mathjax_spans_re().replace_all(&clean_for_md, " ");
    let lines: Vec<&str> = line_break_re().split(&clean_for_md).collect();
    let pipe_lines_count = lines
        .iter()
        .filter(|line| line.matches('|').count() + line.matches("&#124;").count() >= 3)
        .count();
    let has_sep = lines.iter().any(|line| is_separator_row(line));
    if has_sep || pipe_lines_count >= 2 {
        let mut diag = Diagnostic::new(
            CODE_TBL_MARKDOWN_PIPE,
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
        assert_eq!(diags[0].code, CODE_TBL_MARKDOWN_PIPE);
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
        assert!(codes.contains(&CODE_TBL_INLINE_STYLE.as_str()));
        assert!(codes.contains(&CODE_TBL_IN_CARD_FACE.as_str()));
        assert!(codes.contains(&CODE_TBL_RAGGED.as_str()));
        assert!(diags.iter().all(|d| d.row.is_none()));
    }

    #[test]
    fn detects_malformed_unclosed_table() {
        let html = "<table><tr><td>Unclosed";
        let diags = check_tables_field(html, "Notas Adicionais", Some(5), false);
        assert!(diags.iter().any(|d| d.code == CODE_TBL_MALFORMED));
    }

    #[test]
    fn valid_rowspan_table_does_not_emit_ragged() {
        let html = r#"<table>
<thead><tr><th>A</th><th>B</th></tr></thead>
<tbody>
<tr><td rowspan="2">Merged</td><td>Row 1</td></tr>
<tr><td>Row 2</td></tr>
</tbody>
</table>"#;
        let diags = check_tables_field(html, "Notas Adicionais", Some(2), false);
        assert!(
            !diags.iter().any(|d| d.code == CODE_TBL_RAGGED),
            "unexpected ragged: {diags:?}"
        );
    }

    #[test]
    fn invalid_rowspan_table_emits_ragged() {
        let html = r#"<table>
<thead><tr><th>A</th><th>B</th></tr></thead>
<tbody>
<tr><td rowspan="2">Merged</td><td>Row 1</td></tr>
<tr><td>Row 2</td><td>Extra cell</td></tr>
</tbody>
</table>"#;
        let diags = check_tables_field(html, "Notas Adicionais", Some(2), false);
        assert!(diags.iter().any(|d| d.code == CODE_TBL_RAGGED));
    }

    #[test]
    fn rowspan_with_colspan_does_not_emit_ragged() {
        let html = r#"<table>
<thead><tr><th>A</th><th>B</th><th>C</th></tr></thead>
<tbody>
<tr><td rowspan="2" colspan="2">Merged 2x2</td><td>Row 1 Right</td></tr>
<tr><td>Row 2 Right</td></tr>
</tbody>
</table>"#;
        let diags = check_tables_field(html, "Notas Adicionais", Some(2), false);
        assert!(
            !diags.iter().any(|d| d.code == CODE_TBL_RAGGED),
            "unexpected ragged: {diags:?}"
        );
    }

    #[test]
    fn nested_table_state_does_not_contaminate_parent() {
        let html = r#"<table>
<thead><tr><th>Parent 1</th><th>Parent 2</th></tr></thead>
<tbody>
<tr>
<td>
<table>
<thead><tr><th>Sub 1</th><th>Sub 2</th><th>Sub 3</th></tr></thead>
<tbody><tr><td>1</td><td>2</td><td>3</td></tr></tbody>
</table>
</td>
<td>Parent Right</td>
</tr>
</tbody>
</table>"#;
        let diags = check_tables_field(html, "Notas Adicionais", Some(2), false);
        let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
        assert!(codes.contains(&CODE_TBL_NESTED.as_str()));
        assert!(
            !codes.contains(&CODE_TBL_RAGGED.as_str()),
            "parent table contaminated by nested table row count: {codes:?}"
        );
    }

    #[test]
    fn html_table_with_pipes_does_not_emit_markdown_pipe() {
        let html = r#"<table>
<thead><tr><th>Condição</th><th>Expressão</th></tr></thead>
<tbody>
<tr><td>Módulo</td><td>|x| &lt; 5 | y &gt; 3</td></tr>
<tr><td>Disjunção</td><td>A | B | C</td></tr>
</tbody>
</table>"#;
        let diags = check_tables_field(html, "Notas Adicionais", Some(2), false);
        assert!(
            !diags.iter().any(|d| d.code == CODE_TBL_MARKDOWN_PIPE),
            "unexpected markdown pipe diag: {diags:?}"
        );
    }

    #[test]
    fn mathjax_spans_with_pipes_do_not_emit_markdown_pipe() {
        let text = r#"Exemplo de equação com valor absoluto:
\( |x| \le 1 \)
e em bloco:
\[ |A| = \det(A) \]
com mais um:
\( P(A|B) = \frac{P(A \cap B)}{P(B)} \)"#;
        let diags = check_tables_field(text, "Notas Adicionais", Some(2), false);
        assert!(
            !diags.iter().any(|d| d.code == CODE_TBL_MARKDOWN_PIPE),
            "unexpected markdown pipe diag: {diags:?}"
        );
    }

    #[test]
    fn markdown_pipe_separator_rules() {
        let single_dash_not_sep = "Item A | - | Item B";
        assert!(
            !check_tables_field(single_dash_not_sep, "Notas Adicionais", Some(2), false)
                .iter()
                .any(|d| d.code == CODE_TBL_MARKDOWN_PIPE)
        );

        let single_col_three_dashes = "| A |\n| --- |\n| 1 |";
        assert!(
            check_tables_field(single_col_three_dashes, "Notas Adicionais", Some(2), false)
                .iter()
                .any(|d| d.code == CODE_TBL_MARKDOWN_PIPE)
        );

        let two_cells_separator = "| A | B |\n| - | - |\n| 1 | 2 |";
        assert!(
            check_tables_field(two_cells_separator, "Notas Adicionais", Some(2), false)
                .iter()
                .any(|d| d.code == CODE_TBL_MARKDOWN_PIPE)
        );
    }

    #[test]
    fn check_tables_field_csv_with_csv_row_number() {
        let row = CsvRowNumber::new(4).unwrap();
        let html = "<table><tr><td>Unclosed";
        let diags = check_tables_field_csv(html, "Notas Adicionais", Some(row), false);
        assert!(diags.iter().any(|d| d.code == CODE_TBL_MALFORMED));
        assert_eq!(diags[0].row, Some(4));
    }

    #[test]
    fn table_cell_too_long_emits_warning_above_120_chars() {
        let long_cell = "a".repeat(121);
        let ok_cell = "b".repeat(120);
        let html = format!(
            "<table><thead><tr><th>Col 1</th><th>Col 2</th></tr></thead><tbody><tr><td>{long_cell}</td><td>{ok_cell}</td></tr></tbody></table>"
        );
        let diags = check_tables_field(&html, "Notas Adicionais", Some(2), false);
        let long_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.code == CODE_TBL_CELL_TOO_LONG)
            .collect();
        assert_eq!(long_diags.len(), 1);
        assert_eq!(long_diags[0].severity, Severity::Warn);
    }

    #[test]
    fn table_cell_too_long_uses_visible_char_count_for_entities_and_inline_html() {
        let body = format!("   <b>{}</b>&rarr;   ", "a".repeat(115));
        let html = format!(
            "<table><thead><tr><th>Col</th></tr></thead><tbody><tr><td>{body}</td></tr></tbody></table>"
        );
        let diags = check_tables_field(&html, "Notas Adicionais", Some(2), false);
        assert!(
            !diags.iter().any(|d| d.code == CODE_TBL_CELL_TOO_LONG),
            "cell with <= 120 visible chars should not emit TBL.CELL_TOO_LONG: {diags:?}"
        );
    }

    #[test]
    fn tr_recovery_syncs_cell_chars_stack_on_unclosed_td() {
        let unclosed_text = "x".repeat(90);
        let next_cell_text = "y".repeat(50);
        let html = format!(
            "<table><thead><tr><th>Col</th></tr></thead><tbody><tr><td>{unclosed_text}</tr><tr><td>{next_cell_text}</td></tr></tbody></table>"
        );
        let diags = check_tables_field(&html, "Notas Adicionais", Some(2), false);
        assert!(diags.iter().any(|d| d.code == CODE_TBL_MALFORMED));
        assert!(
            !diags.iter().any(|d| d.code == CODE_TBL_CELL_TOO_LONG),
            "unclosed <td> text must not bleed into subsequent cells: {diags:?}"
        );
    }

    #[test]
    fn pending_rowspans_reset_at_section_boundary() {
        let html = r#"<table>
<thead><tr><th rowspan="2">A</th><th>B</th></tr></thead>
<tbody><tr><td>1</td><td>2</td></tr></tbody>
</table>"#;
        let diags = check_tables_field(html, "Notas Adicionais", Some(2), false);
        assert!(
            !diags.iter().any(|d| d.code == CODE_TBL_RAGGED),
            "rowspan in <thead> must not leak into <tbody>: {diags:?}"
        );
    }

    #[test]
    fn detects_colspan_overlapping_active_rowspan() {
        let html = r#"<table>
<thead><tr><th>A</th><th>B</th><th>C</th></tr></thead>
<tbody>
<tr><td>1</td><td rowspan="2">2</td><td>3</td></tr>
<tr><td colspan="2">Overlap</td><td>4</td></tr>
</tbody>
</table>"#;
        let diags = check_tables_field(html, "Notas Adicionais", Some(2), false);
        assert!(
            diags.iter().any(|d| d.code == CODE_TBL_MALFORMED
                && d.message.contains("overlaps an active rowspan")),
            "expected colspan/rowspan overlap diagnostic, got: {diags:?}"
        );
    }
}
