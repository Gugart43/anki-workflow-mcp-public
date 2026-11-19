//! Structural CSV, HTML tag/class, entity, and MathJax checks (`checks/csv_structure.py`).

use crate::sanitize::sanitize_field;
use crate::vocab::{
    ALLOWED_CLASSES, is_always_allowed_tag, is_images_only_tag, is_paired_tag, unknown_classes,
};
use anki_domain::{
    CLOZE_FIELDS, CLOZE_HEADER_CANONICAL, Diagnostic, NoteType, QA_FIELDS, QA_HEADER_CANONICAL,
    Severity,
};
use anki_parser::{py_is_blank, py_splitlines, split_row};
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

static ENTITY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&(#\d+|#x[0-9a-fA-F]+|[a-zA-Z]+);").expect("valid entity regex"));

static TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<([a-zA-Z/?!](?:"[^"]*"|'[^']*'|[^>\"'])*)>"#).expect("valid tag regex")
});

static TAG_NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^/?([a-zA-Z][a-zA-Z0-9]*)").expect("valid tag name regex"));

static COMMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->").expect("valid comment regex"));

static CLASS_ATTR_INNER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)class\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+))"#)
        .expect("valid class attr regex")
});

fn mask_comments(value: &str) -> (String, usize) {
    let mut count = 0usize;
    let masked = COMMENT_RE
        .replace_all(value, |caps: &regex::Captures<'_>| {
            count += 1;
            "#".repeat(caps[0].chars().count())
        })
        .into_owned();
    (masked, count)
}

/// Check UTF-8 BOM prefix (`FMT.BOM`).
#[must_use]
pub fn check_bom(raw_bytes: &[u8]) -> Vec<Diagnostic> {
    if raw_bytes.starts_with(b"\xef\xbb\xbf") {
        vec![Diagnostic::new(
            anki_domain::DiagnosticCode::from_static("FMT.BOM"),
            Severity::Critical,
            "anki-formatting-guide",
            "UTF-8 BOM detected at start of file",
        )]
    } else {
        Vec::new()
    }
}

fn extract_tag_name(tag_str: &str) -> String {
    let s = tag_str.trim();
    let is_close = s.starts_with('/');
    let Some(caps) = TAG_NAME_RE.captures(s) else {
        return String::new();
    };
    let name = caps[1].to_ascii_lowercase();
    if is_close { format!("/{name}") } else { name }
}

fn has_dollar_math(value: &str) -> bool {
    // Equivalent to: r"(?<![a-zA-Z])\$(?!\$)"
    let bytes = value.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'$' {
            let prev_ascii_alpha = i > 0 && bytes[i - 1].is_ascii_alphabetic();
            let next_dollar = i + 1 < bytes.len() && bytes[i + 1] == b'$';
            if !prev_ascii_alpha && !next_dollar {
                return true;
            }
        }
    }
    false
}

fn find_class_attrs(tag_inner: &str) -> Vec<&str> {
    // Equivalent to: r"(?<![-\w])class\s*=\s*(...)"
    let mut out = Vec::new();
    for caps in CLASS_ATTR_INNER_RE.captures_iter(tag_inner) {
        let full = caps.get(0).expect("group 0");
        let preceded_by_word_or_dash = tag_inner[..full.start()]
            .chars()
            .next_back()
            .is_some_and(|ch| ch == '-' || ch == '_' || ch.is_alphanumeric());
        if preceded_by_word_or_dash {
            continue;
        }
        if let Some(val) = caps.get(1).or_else(|| caps.get(2)).or_else(|| caps.get(3)) {
            out.push(val.as_str());
        }
    }
    out
}

fn check_entities_and_brackets(
    value: &str,
    row: usize,
    field: &str,
    is_images_field: bool,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    let no_entities = ENTITY_RE.replace_all(value, "");
    let stripped = TAG_RE.replace_all(&no_entities, "");

    let (sanitized, _) = sanitize_field(value, is_images_field, true);
    let auto_fix_available = sanitized != value;
    let fix_preview = if auto_fix_available {
        Some(sanitized.chars().take(200).collect())
    } else {
        None
    };

    if stripped.contains('<') || stripped.contains('>') {
        diags.push(
            Diagnostic::new(
                anki_domain::DiagnosticCode::from_static("FMT.ENTITY_MISSING"),
                Severity::Critical,
                "anki-formatting-guide",
                format!("Row {row}, field '{field}': raw '<' or '>' — use &lt; / &gt;"),
            )
            .with_row(row)
            .with_field(field)
            .with_suggestion("Replace < with &lt; and > with &gt;")
            .with_auto_fix(auto_fix_available, fix_preview.clone()),
        );
    }

    if no_entities.contains('&') {
        diags.push(
            Diagnostic::new(
                anki_domain::DiagnosticCode::from_static("FMT.AMP_RAW"),
                Severity::Critical,
                "anki-formatting-guide",
                format!("Row {row}, field '{field}': raw '&' not part of a valid HTML entity"),
            )
            .with_row(row)
            .with_field(field)
            .with_suggestion("Replace & with &amp;")
            .with_auto_fix(auto_fix_available, fix_preview),
        );
    }

    diags
}

fn check_classes(value: &str, row: usize, field: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let mut allowed_list: Vec<&str> = ALLOWED_CLASSES.to_vec();
    allowed_list.sort_unstable();
    let allowed_suggestion = format!(
        "Use one of: {}",
        allowed_list
            .iter()
            .map(|c| format!(".{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    for caps in TAG_RE.captures_iter(value) {
        let tag_inner = &caps[1];
        if tag_inner.starts_with('!') || tag_inner.starts_with('?') {
            continue;
        }
        for tokens in find_class_attrs(tag_inner) {
            for bad in unknown_classes(tokens) {
                if !seen.insert(bad.to_owned()) {
                    continue;
                }
                diags.push(
                    Diagnostic::new(
                        anki_domain::DiagnosticCode::from_static("FMT.CLASS_UNKNOWN"),
                        Severity::Warn,
                        "anki-formatting-guide",
                        format!(
                            "Row {row}, field '{field}': class '{bad}' is not in the semantic vocabulary (renders unstyled)"
                        ),
                    )
                    .with_row(row)
                    .with_field(field)
                    .with_suggestion(allowed_suggestion.clone()),
                );
            }
        }
    }

    diags
}

fn check_tags(value: &str, row: usize, field: &str, is_images_field: bool) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut tag_stack: Vec<String> = Vec::new();

    for caps in TAG_RE.captures_iter(value) {
        let tag_raw = caps[1].trim();
        if tag_raw.starts_with('!') || tag_raw.starts_with('?') {
            diags.push(
                Diagnostic::new(
                    anki_domain::DiagnosticCode::from_static("FMT.TAG_FORBIDDEN"),
                    Severity::Critical,
                    "anki-formatting-guide",
                    format!(
                        "Row {row}, field '{field}': HTML declaration is not allowed in a card field (never rendered)"
                    ),
                )
                .with_row(row)
                .with_field(field)
                .with_suggestion("Delete the declaration"),
            );
            continue;
        }

        let tag_name = extract_tag_name(tag_raw);
        let allows_figure_tags = is_images_field || field == "Notas Adicionais";
        if is_always_allowed_tag(&tag_name) || (is_images_only_tag(&tag_name) && allows_figure_tags)
        {
            // allowed
        } else if is_images_only_tag(&tag_name) && !allows_figure_tags {
            diags.push(
                Diagnostic::new(
                    anki_domain::DiagnosticCode::from_static("FMT.TAG_FORBIDDEN"),
                    Severity::Critical,
                    "anki-formatting-guide",
                    format!(
                        "Row {row}, field '{field}': <{tag_name}> only allowed in 'Imagens Relacionadas'"
                    ),
                )
                .with_row(row)
                .with_field(field),
            );
        } else {
            diags.push(
                Diagnostic::new(
                    anki_domain::DiagnosticCode::from_static("FMT.TAG_FORBIDDEN"),
                    Severity::Critical,
                    "anki-formatting-guide",
                    format!(
                        "Row {row}, field '{field}': tag <{tag_name}> is not in the allowed whitelist"
                    ),
                )
                .with_row(row)
                .with_field(field)
                .with_suggestion(
                    "Allowed: <b> <i> <strong> <em> <br>; in Imagens field also <figure> <img> <figcaption>",
                ),
            );
        }

        if let Some(close_name) = tag_name.strip_prefix('/') {
            if is_paired_tag(close_name) {
                if tag_stack.last().is_some_and(|top| top == close_name) {
                    tag_stack.pop();
                } else {
                    diags.push(
                        Diagnostic::new(
                            anki_domain::DiagnosticCode::from_static("FMT.TAG_UNCLOSED"),
                            Severity::Warn,
                            "anki-formatting-guide",
                            format!(
                                "Row {row}, field '{field}': unmatched closing tag </{close_name}>"
                            ),
                        )
                        .with_row(row)
                        .with_field(field)
                        .with_suggestion(format!("Remove or match </{close_name}>")),
                    );
                }
            }
        } else if is_paired_tag(&tag_name) {
            tag_stack.push(tag_name);
        }
    }

    for unclosed in tag_stack {
        diags.push(
            Diagnostic::new(
                anki_domain::DiagnosticCode::from_static("FMT.TAG_UNCLOSED"),
                Severity::Warn,
                "anki-formatting-guide",
                format!("Row {row}, field '{field}': unclosed tag <{unclosed}>"),
            )
            .with_row(row)
            .with_field(field)
            .with_suggestion(format!("Close tag with </{unclosed}>")),
        );
    }

    diags
}

fn check_mathjax(value: &str, row: usize, field: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    let inline_opens = value.matches(r"\(").count();
    let inline_closes = value.matches(r"\)").count();
    if inline_opens != inline_closes {
        diags.push(
            Diagnostic::new(
                anki_domain::DiagnosticCode::from_static("MATH.UNBALANCED"),
                Severity::Critical,
                "anki-formatting-guide",
                format!(
                    r"Row {row}, field '{field}': unbalanced \( \) — {inline_opens} open, {inline_closes} close"
                ),
            )
            .with_row(row)
            .with_field(field),
        );
    }

    let block_opens = value.matches(r"\[").count();
    let block_closes = value.matches(r"\]").count();
    if block_opens != block_closes {
        diags.push(
            Diagnostic::new(
                anki_domain::DiagnosticCode::from_static("MATH.UNBALANCED"),
                Severity::Critical,
                "anki-formatting-guide",
                format!(
                    r"Row {row}, field '{field}': unbalanced \[ \] — {block_opens} open, {block_closes} close"
                ),
            )
            .with_row(row)
            .with_field(field),
        );
    }

    if has_dollar_math(value) {
        diags.push(
            Diagnostic::new(
                anki_domain::DiagnosticCode::from_static("MATH.DOLLAR"),
                Severity::Critical,
                "anki-formatting-guide",
                format!(r"Row {row}, field '{field}': raw '$' — use \( \) for math"),
            )
            .with_row(row)
            .with_field(field),
        );
    }

    diags
}

/// Full structural check of a CSV block (`checks/csv_structure.py::check_csv`).
#[must_use]
pub fn check_csv(csv_text: &str, note_type: NoteType, enforce_header: bool) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let lines = py_splitlines(csv_text);
    if lines.is_empty() {
        return diags;
    }

    let (expected_fields, canonical): (&[&str], &str) = match note_type {
        NoteType::Qa => (&QA_FIELDS, QA_HEADER_CANONICAL),
        NoteType::Cloze | NoteType::ClozeLong => (&CLOZE_FIELDS, CLOZE_HEADER_CANONICAL),
    };
    let expected_count = expected_fields.len();

    let header_line = lines[0];
    if enforce_header && header_line.trim() != canonical {
        diags.push(
            Diagnostic::new(
                anki_domain::DiagnosticCode::from_static("FMT.HEADER_MISMATCH"),
                Severity::Critical,
                "anki-formatting-guide",
                format!("Expected: '{canonical}' | Got: '{}'", header_line.trim()),
            )
            .with_row(1usize)
            .with_suggestion(canonical),
        );
    }

    for (idx, line) in lines.iter().enumerate().skip(1) {
        let lineno = idx + 1;
        if py_is_blank(line) {
            continue;
        }

        let row = split_row(line, expected_count);
        if !row.structural_ok {
            diags.push(
                Diagnostic::new(
                    anki_domain::DiagnosticCode::from_static("FMT.COLUMN_COUNT"),
                    Severity::Critical,
                    "anki-formatting-guide",
                    format!(
                        "Row {lineno}: expected {expected_count} columns, got {}",
                        row.count()
                    ),
                )
                .with_row(lineno),
            );
            diags.push(
                Diagnostic::new(
                    anki_domain::DiagnosticCode::from_static("FMT.FIELD_SHIFTED"),
                    Severity::Critical,
                    "anki-formatting-guide",
                    format!(
                        "Row {lineno}: field checks SKIPPED_STRUCTURAL (found {} columns; a raw '|' inside a field shifts the rest)",
                        row.count()
                    ),
                )
                .with_row(lineno)
                .with_suggestion("Escape every raw '|' inside a field as &#124;"),
            );
            continue;
        }

        for (&field_name, value) in expected_fields.iter().zip(row.cols.iter()) {
            let is_images_field = field_name == "Imagens Relacionadas";

            if value.contains('\n') || value.contains('\r') {
                diags.push(
                    Diagnostic::new(
                        anki_domain::DiagnosticCode::from_static("FMT.RAW_NEWLINE"),
                        Severity::Critical,
                        "anki-formatting-guide",
                        format!("Row {lineno}, field '{field_name}': raw newline character"),
                    )
                    .with_row(lineno)
                    .with_field(field_name)
                    .with_suggestion("Replace with <br>"),
                );
            }

            let (masked, n_comments) = mask_comments(value);
            for _ in 0..n_comments {
                diags.push(
                    Diagnostic::new(
                        anki_domain::DiagnosticCode::from_static("FMT.TAG_FORBIDDEN"),
                        Severity::Critical,
                        "anki-formatting-guide",
                        format!(
                            "Row {lineno}, field '{field_name}': HTML comment is not allowed in a card field (never rendered)"
                        ),
                    )
                    .with_row(lineno)
                    .with_field(field_name)
                    .with_suggestion("Delete the comment"),
                );
            }

            diags.extend(check_entities_and_brackets(
                &masked,
                lineno,
                field_name,
                is_images_field,
            ));
            diags.extend(check_tags(&masked, lineno, field_name, is_images_field));
            diags.extend(check_classes(&masked, lineno, field_name));
            diags.extend(check_mathjax(&masked, lineno, field_name));
        }
    }

    diags
}
