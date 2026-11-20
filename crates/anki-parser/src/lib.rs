//! Pipe-delimited CSV row splitter, Cloze span parser, and batch card counter.

use anki_domain::{BatchCount, CompatibilityProfile, CsvRowNumber, MAX_BATCH, NoteType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Result of splitting a single pipe-delimited CSV line (`core/rows.py::SplitRow`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitRow {
    pub cols: Vec<String>,
    pub expected: usize,
    pub structural_ok: bool,
}

impl SplitRow {
    #[must_use]
    pub fn count(&self) -> usize {
        self.cols.len()
    }
}

/// Split *line* on raw `|` without trimming or normalizing any column.
///
/// Only trailing `\r` / `\n` line terminators are stripped, matching `core/rows.py::split_row`.
#[must_use]
pub fn split_row(line: &str, expected: usize) -> SplitRow {
    let stripped = line.trim_end_matches(['\r', '\n']);
    let cols: Vec<String> = stripped.split('|').map(str::to_owned).collect();
    let structural_ok = cols.len() == expected;
    SplitRow {
        cols,
        expected,
        structural_ok,
    }
}

/// Split a row matching the expected columns for the given `NoteType`.
#[must_use]
pub fn split_row_for(line: &str, note_type: NoteType) -> SplitRow {
    split_row(line, note_type.expected_columns())
}

/// Split text into lines using Python `str.splitlines()` semantics.
///
/// Recognizes `\n`, `\r\n`, `\r`, `\x0b`, `\x0c`, `\x1c`, `\x1d`, `\x1e`, `\x85`, `\u{2028}`, `\u{2029}`
/// and omits a trailing empty element after a final line terminator.
#[must_use]
pub fn py_splitlines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    let mut iter = text.char_indices().peekable();

    while let Some((idx, ch)) = iter.next() {
        let is_sep = matches!(
            ch,
            '\n' | '\r'
                | '\x0b'
                | '\x0c'
                | '\x1c'
                | '\x1d'
                | '\x1e'
                | '\u{0085}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if is_sep {
            lines.push(&text[start..idx]);
            if ch == '\r' && iter.peek().is_some_and(|&(_, next_ch)| next_ch == '\n') {
                let (nl_idx, _) = iter.next().expect("peeked");
                start = nl_idx + 1;
            } else {
                start = idx + ch.len_utf8();
            }
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Python-compatible whitespace check (`str.strip()`), including ASCII control
/// separators `\x1c`..=`\x1f` which `char::is_whitespace` excludes.
#[must_use]
pub fn py_is_blank(s: &str) -> bool {
    s.chars()
        .all(|ch| ch.is_whitespace() || matches!(ch, '\x1c'..='\x1f'))
}

/// Return `(1-based line number, raw line)` for every non-blank line after the header.
#[must_use]
pub fn iter_data_lines(csv_text: &str) -> Vec<(CsvRowNumber, &str)> {
    py_splitlines(csv_text)
        .into_iter()
        .enumerate()
        .skip(1)
        .filter(|(_, line)| !py_is_blank(line))
        .map(|(idx, line)| {
            let row = CsvRowNumber::new(idx + 1)
                .expect("skip(1) guarantees 1-based data row number >= 2");
            (row, line)
        })
        .collect()
}

/// A parsed Cloze span `{{cN::target}}` or `{{cN::target::hint}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClozeSpan {
    pub index: usize,
    pub target: String,
    pub hint: Option<String>,
    /// Start UTF-8 byte offset in the sentence.
    pub start: usize,
    /// End UTF-8 byte offset (exclusive) in the sentence.
    pub end: usize,
}

fn is_escaped(bytes: &[u8], pos: usize) -> bool {
    let mut backslashes = 0usize;
    let mut idx = pos;
    while idx > 0 && bytes[idx - 1] == b'\\' {
        backslashes += 1;
        idx -= 1;
    }
    backslashes % 2 == 1
}

/// Extract Cloze spans supporting nested LaTeX braces `{a}/{b}`, matching
/// `checks/cloze.py::_find_cloze_spans`.
#[must_use]
pub fn find_cloze_spans(sentence: &str) -> Vec<ClozeSpan> {
    let bytes = sentence.as_bytes();
    let n = bytes.len();
    let mut results = Vec::new();
    let mut i = 0;

    while i + 6 < n {
        if bytes[i] == b'{'
            && bytes[i + 1] == b'{'
            && bytes[i + 2] == b'c'
            && bytes[i + 3].is_ascii_digit()
        {
            let mut j = i + 3;
            while j < n && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j + 1 >= n || bytes[j] != b':' || bytes[j + 1] != b':' {
                i += 1;
                continue;
            }
            let Ok(idx) = sentence[i + 3..j].parse::<usize>() else {
                i += 1;
                continue;
            };

            let content_start = j + 2;
            let mut k = content_start;
            let mut brace_depth: usize = 0;
            let mut sep_pos: Option<usize> = None;
            let mut closed = false;

            while k + 1 < n {
                if bytes[k] == b'}'
                    && bytes[k + 1] == b'}'
                    && brace_depth == 0
                    && !is_escaped(bytes, k)
                {
                    let (target, hint) = if let Some(sep) = sep_pos {
                        (
                            sentence[content_start..sep].to_owned(),
                            Some(sentence[sep + 2..k].to_owned()),
                        )
                    } else {
                        (sentence[content_start..k].to_owned(), None)
                    };
                    results.push(ClozeSpan {
                        index: idx,
                        target,
                        hint,
                        start: i,
                        end: k + 2,
                    });
                    i = k + 2;
                    closed = true;
                    break;
                } else if bytes[k] == b':'
                    && bytes[k + 1] == b':'
                    && brace_depth == 0
                    && sep_pos.is_none()
                {
                    sep_pos = Some(k);
                    k += 2;
                    continue;
                } else if bytes[k] == b'{' && !is_escaped(bytes, k) {
                    brace_depth += 1;
                } else if bytes[k] == b'}' && !is_escaped(bytes, k) {
                    brace_depth = brace_depth.saturating_sub(1);
                }
                k += 1;
            }

            if !closed {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    results
}

/// Return the set of distinct `cN` indices found in *sentence*.
#[must_use]
pub fn count_cloze_indices(sentence: &str) -> BTreeSet<usize> {
    find_cloze_spans(sentence)
        .into_iter()
        .map(|s| s.index)
        .collect()
}

fn suggest_ext_batch(batch_id: &str) -> Option<String> {
    let rest = batch_id.strip_prefix('B')?;
    let digits_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits_len == 0 {
        return None;
    }
    Some(format!("B{}-EXT", &rest[..digits_len]))
}

/// Count QA + expanded Cloze cards in a batch (`checks/batch.py::count_batch`).
///
/// - QA notes = 1 card each.
/// - Cloze notes with N distinct `cN` indices = N cards each (0-index note counts as 1).
#[must_use]
pub fn count_batch_cards(qa_csv: &str, cloze_csv: &str, batch_id: Option<&str>) -> BatchCount {
    let qa_cards = iter_data_lines(qa_csv).len();

    let mut cloze_notes = 0usize;
    let mut cloze_indices = 0usize;

    for (_, line) in iter_data_lines(cloze_csv) {
        let sentenca = line.split('|').next().unwrap_or("");
        let idxs = count_cloze_indices(sentenca);
        cloze_notes += 1;
        cloze_indices += if idxs.is_empty() { 1 } else { idxs.len() };
    }

    let total = qa_cards + cloze_indices;
    let within_limit = total <= MAX_BATCH;
    let excess = total.saturating_sub(MAX_BATCH);

    let ext_batch_suggested = if !within_limit {
        batch_id
            .filter(|s| !s.is_empty())
            .and_then(suggest_ext_batch)
    } else {
        None
    };

    BatchCount {
        qa_cards,
        cloze_notes,
        cloze_indices,
        total,
        within_limit,
        excess,
        ext_batch_suggested,
    }
}

/// Count in a specified policy profile; `strict_v2` never rejects a batch by size.
///
/// The legacy entry point remains available as a compatibility oracle.
/// `within_limit` and `excess` are legacy-shaped serialization fields:
/// in strict v2 they mean no *batch-size* violation, not approval of content.
#[must_use]
pub fn count_batch_cards_with_profile(
    qa_csv: &str,
    cloze_csv: &str,
    batch_id: Option<&str>,
    profile: CompatibilityProfile,
) -> BatchCount {
    let mut count = count_batch_cards(qa_csv, cloze_csv, batch_id);
    if profile == CompatibilityProfile::StrictV2 {
        count.within_limit = true;
        count.excess = 0;
        count.ext_batch_suggested = None;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_row_preserves_whitespace_and_checks_arity() {
        let r = split_row("  a  |b| c |d|e|f|g\r\n", 7);
        assert!(r.structural_ok);
        assert_eq!(r.count(), 7);
        assert_eq!(r.cols[0], "  a  ");
        assert_eq!(r.cols[2], " c ");

        let bad = split_row("a|b|c", 7);
        assert!(!bad.structural_ok);
        assert_eq!(bad.count(), 3);
    }

    #[test]
    fn cloze_spans_support_latex_braces_and_hints() {
        let s = r"A fórmula é {{c1::\frac{a}{b}::fração}} e {{c1::repetido}} mais {{c2::outro}}.";
        let spans = find_cloze_spans(s);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].index, 1);
        assert_eq!(spans[0].target, r"\frac{a}{b}");
        assert_eq!(spans[0].hint.as_deref(), Some("fração"));
        assert_eq!(count_cloze_indices(s), BTreeSet::from([1, 2]));
    }

    #[test]
    fn strict_v2_counts_without_rejecting_a_large_batch() {
        let qa_hdr = "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas";
        let qa = format!(
            "{qa_hdr}{}",
            (0..21)
                .map(|i| format!("\nQ{i}|R{i}|||||"))
                .collect::<String>()
        );
        let cl_hdr =
            "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas";
        let strict = count_batch_cards_with_profile(
            &qa,
            cl_hdr,
            Some("B03"),
            anki_domain::CompatibilityProfile::StrictV2,
        );
        assert_eq!(strict.total, 21);
        assert!(strict.within_limit);
        assert_eq!(strict.excess, 0);
        assert_eq!(strict.ext_batch_suggested, None);
        let legacy = count_batch_cards_with_profile(
            &qa,
            cl_hdr,
            Some("B03"),
            anki_domain::CompatibilityProfile::LegacyCompat,
        );
        assert!(!legacy.within_limit);
        assert_eq!(legacy.excess, 1);
        assert_eq!(legacy.ext_batch_suggested.as_deref(), Some("B03-EXT"));
    }

    #[test]
    fn batch_counter_expands_cloze_and_suggests_ext() {
        let qa_hdr = "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas";
        let mut qa = String::from(qa_hdr);
        for i in 0..18 {
            qa.push_str(&format!("\nQ{i}|R{i}|Neuro||||"));
        }
        let cl_hdr =
            "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas";
        let cl =
            format!("{cl_hdr}\n{{{{c1::alfa}}}} e {{{{c2::beta}}}} e {{{{c1::alfa2}}}}||Neuro|||");
        // 18 QA + 2 distinct cloze indices + 1 note = 20
        let ok = count_batch_cards(&qa, &cl, Some("B03-T1"));
        assert_eq!(ok.total, 20);
        assert!(ok.within_limit);
        assert_eq!(ok.ext_batch_suggested, None);

        let cl_over = format!("{cl}\n{{{{c1::extra}}}}||Neuro|||");
        let over = count_batch_cards(&qa, &cl_over, Some("B03-T1"));
        assert_eq!(over.total, 21);
        assert!(!over.within_limit);
        assert_eq!(over.excess, 1);
        assert_eq!(over.ext_batch_suggested.as_deref(), Some("B03-EXT"));
    }

    #[test]
    fn split_row_for_uses_expected_columns() {
        let qa_valid = "a|b|c|d|e|f|g";
        let r1 = split_row_for(qa_valid, NoteType::Qa);
        assert!(r1.structural_ok);
        assert_eq!(r1.expected, 7);
        assert_eq!(r1.count(), 7);

        let qa_invalid = "a|b|c|d|e|f";
        let r2 = split_row_for(qa_invalid, NoteType::Qa);
        assert!(!r2.structural_ok);
        assert_eq!(r2.expected, 7);
        assert_eq!(r2.count(), 6);

        let cl_valid = "a|b|c|d|e|f";
        let r3 = split_row_for(cl_valid, NoteType::Cloze);
        assert!(r3.structural_ok);
        assert_eq!(r3.expected, 6);
        assert_eq!(r3.count(), 6);

        let cl_long_valid = "a|b|c|d|e|f";
        let r4 = split_row_for(cl_long_valid, NoteType::ClozeLong);
        assert!(r4.structural_ok);
        assert_eq!(r4.expected, 6);
        assert_eq!(r4.count(), 6);
    }

    #[test]
    fn iter_data_lines_returns_typed_csv_row_numbers() {
        let csv = "Enunciado|Resposta|Campo|Notas|Imagens|Refs|Tags\n\
                   Q1|R1|Cardio||||\n\
                   \n\
                   Q3|R3|Neuro||||\n";

        let lines = iter_data_lines(csv);
        assert_eq!(lines.len(), 2);

        // First data line is line 2 (header is line 1)
        let (row1, line1) = &lines[0];
        assert_eq!(row1.get(), 2);
        assert_eq!(*row1, 2);
        assert_eq!(row1, &2);
        assert_eq!(*line1, "Q1|R1|Cardio||||");

        // Second data line is line 4 (line 3 is blank)
        let (row2, line2) = &lines[1];
        assert_eq!(row2.get(), 4);
        assert_eq!(*row2, 4);
        assert_eq!(row2, &4);
        assert_eq!(*line2, "Q3|R3|Neuro||||");
    }

    #[test]
    fn cloze_spans_byte_offsets_with_accents_and_emoji() {
        let text = "Coração 🩺: {{c1::ação}} e {{c2::reação::pista}} fim 🎉.";
        let spans = find_cloze_spans(text);
        assert_eq!(spans.len(), 2);

        // Verify span 0: byte offsets allow exact UTF-8 string slicing without panic
        let s0 = &spans[0];
        assert_eq!(s0.index, 1);
        assert_eq!(s0.target, "ação");
        assert_eq!(s0.hint, None);
        assert_eq!(&text[s0.start..s0.end], "{{c1::ação}}");
        assert_eq!(&text[..s0.start], "Coração 🩺: ");

        // Verify span 1
        let s1 = &spans[1];
        assert_eq!(s1.index, 2);
        assert_eq!(s1.target, "reação");
        assert_eq!(s1.hint.as_deref(), Some("pista"));
        assert_eq!(&text[s1.start..s1.end], "{{c2::reação::pista}}");
        assert_eq!(&text[s0.end..s1.start], " e ");
        assert_eq!(&text[s1.end..], " fim 🎉.");
    }

    #[test]
    fn cloze_spans_parity_aware_backslash_escapes() {
        // 1. Literal escaped brace \{ does not change depth:
        //    In LaTeX, \left\{ and \right\} are common
        let s_escaped_brace = r"Fórmula: {{c1::\left\{ x \mid x > 0 \right\}::conjunto}} ok";
        let spans = find_cloze_spans(s_escaped_brace);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].target, r"\left\{ x \mid x > 0 \right\}");
        assert_eq!(spans[0].hint.as_deref(), Some("conjunto"));

        // 2. Escaped backslash \\ before { DOES change depth (even backslashes = unescaped brace):
        let s_even_bs = r"Exemplo: {{c1::linha1 \\ {bloco} \\ fim::dica}} ok";
        let spans_even = find_cloze_spans(s_even_bs);
        assert_eq!(spans_even.len(), 1);
        assert_eq!(spans_even[0].target, r"linha1 \\ {bloco} \\ fim");

        // 3. Escaped closing brace \} does not close cloze; even backslashes \\}} does close cloze:
        let s_escaped_close = r"Teste {{c1::algo \} com chaves::dica}} fim";
        let spans_close = find_cloze_spans(s_escaped_close);
        assert_eq!(spans_close.len(), 1);
        assert_eq!(spans_close[0].target, r"algo \} com chaves");

        // 4. Parity helper direct verification
        assert!(is_escaped(b"\\", 1));
        assert!(!is_escaped(b"\\\\", 2));
        assert!(is_escaped(b"\\\\\\", 3));
        assert!(!is_escaped(b"\\\\\\\\", 4));
    }
}
