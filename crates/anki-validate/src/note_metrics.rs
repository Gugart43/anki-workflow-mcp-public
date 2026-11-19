//! Métricas estruturais e detecção de redundância para `Notas Adicionais`.
//!
//! Fornece:
//! - Detecção determinística de repetição literal de resposta (`is_exact_answer_restatement`);
//! - Verificação de repetição de alvo Cloze (`check_cloze_target_restatement`);
//! - Métricas estruturais com suporte a estruturas aninhadas (`compute_notes_metrics`).

use std::sync::LazyLock;

use anki_domain::{Diagnostic, Severity};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::html_text::{html_to_visible_text, normalize_for_exact_match, visible_char_count};

static RESTATEMENT_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:a\s+resposta\s+(?:correta\s+)?(?:é|e)|resposta(?:\s+correta)?)\s*:?\s*")
        .expect("valid RESTATEMENT_PREFIX_RE")
});

static FOOTER_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<footer\b[^>]*>.*?</footer>").expect("valid FOOTER_BLOCK_RE")
});

static FIGCAPTION_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<figcaption\b[^>]*>.*?</figcaption>").expect("valid FIGCAPTION_BLOCK_RE")
});

static LIST_ITEM_OPEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<(?:li|dt)\b[^>]*>").expect("valid LIST_ITEM_OPEN_RE"));

static PARAGRAPH_BREAK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)</?(?:p|div|h[1-6]|blockquote|section|article|header|hr|details|summary|figure|dl|dt|dd)\b[^>]*>|(?:<br\s*/?>\s*){2,}|\r?\n\s*\r?\n",
    )
    .expect("valid PARAGRAPH_BREAK_RE")
});

/// Métricas estruturais determinísticas calculadas sobre o HTML de `Notas Adicionais`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NotesStructureMetrics {
    pub total_visible_chars: usize,
    pub prose_chars: usize,
    pub table_chars: usize,
    pub list_chars: usize,
    pub visual_caption_chars: usize,
    pub citation_chars: usize,
    pub table_count: usize,
    pub list_item_count: usize,
    pub max_paragraph_chars: usize,
    pub effective_cognitive_load: usize,
}

/// Retorna `true` se `notes` apenas repete `answer` sem explicação pedagógica adicional.
#[must_use]
pub fn is_exact_answer_restatement(notes: &str, answer: &str) -> bool {
    let norm_notes = normalize_for_exact_match(notes);
    let norm_answer = normalize_for_exact_match(answer);

    if norm_notes.is_empty() || norm_answer.is_empty() {
        return false;
    }

    if norm_notes == norm_answer {
        return true;
    }

    if let Some(prefix) = RESTATEMENT_PREFIX_RE.find(&norm_notes)
        && prefix.start() == 0
    {
        let stripped = normalize_for_exact_match(&norm_notes[prefix.end()..]);
        if !stripped.is_empty() && stripped == norm_answer {
            return true;
        }
    }

    let visible_notes = html_to_visible_text(notes);
    let trimmed_visible = visible_notes.trim();
    if let Some(prefix) = RESTATEMENT_PREFIX_RE.find(trimmed_visible)
        && prefix.start() == 0
    {
        let stripped = normalize_for_exact_match(&trimmed_visible[prefix.end()..]);
        if !stripped.is_empty() && stripped == norm_answer {
            return true;
        }
    }

    false
}

/// Emite `NOTE.DUP_ANSWER_WARN` quando `Notas Adicionais` apenas repete o alvo
/// recuperado do cloze `c{cloze_index}` sem explicação.
#[must_use]
pub fn check_cloze_target_restatement(
    notes: &str,
    retrieved_target: &str,
    cloze_index: usize,
    row: usize,
) -> Option<Diagnostic> {
    if is_exact_answer_restatement(notes, retrieved_target) {
        Some(
            Diagnostic::new(
                anki_domain::DiagnosticCode::from_static("NOTE.DUP_ANSWER_WARN"),
                Severity::Warn,
                "anki-notes-enricher",
                format!("Notes repeat the c{cloze_index} target without explanation"),
            )
            .with_row(row)
            .with_field("Notas Adicionais"),
        )
    } else {
        None
    }
}

/// Calcula métricas estruturais e carga cognitiva efetiva das notas HTML,
/// com suporte a estruturas aninhadas (tabelas, listas e legendas).
///
/// Ponderação (excluindo `citation_chars`):
/// `prose * 1.0 + list * 0.75 + visual_caption * 0.60 + table * 0.55`.
#[must_use]
pub fn compute_notes_metrics(notes: &str) -> NotesStructureMetrics {
    let total_visible_chars = visible_char_count(notes);
    if total_visible_chars == 0 {
        return NotesStructureMetrics::default();
    }

    // 1. Extração de citações (footer)
    let citation_chars: usize = FOOTER_BLOCK_RE
        .find_iter(notes)
        .map(|m| visible_char_count(m.as_str()))
        .sum();
    let without_footer = FOOTER_BLOCK_RE.replace_all(notes, "\n");

    // 2. Extração de tabelas (com suporte a aninhamento)
    let (table_blocks, without_tables) = extract_balanced_blocks(&without_footer, &["table"]);
    let table_count = table_blocks.len();
    let table_chars: usize = table_blocks.iter().map(|t| visible_char_count(t)).sum();

    // 3. Extração de legendas de figuras
    let visual_caption_chars: usize = FIGCAPTION_BLOCK_RE
        .find_iter(&without_tables)
        .map(|m| visible_char_count(m.as_str()))
        .sum();
    let without_captions = FIGCAPTION_BLOCK_RE.replace_all(&without_tables, "\n");

    // 4. Extração de listas (ul, ol, dl — com suporte a aninhamento)
    let (list_blocks, without_lists) =
        extract_balanced_blocks(&without_captions, &["ul", "ol", "dl"]);
    let list_chars: usize = list_blocks.iter().map(|l| visible_char_count(l)).sum();
    let list_item_count: usize = list_blocks
        .iter()
        .map(|b| LIST_ITEM_OPEN_RE.find_iter(b).count())
        .sum();

    // 5. Prosa e parágrafos restantes
    let prose_html = without_lists;
    let prose_chars = visible_char_count(&prose_html);
    let max_paragraph_chars = PARAGRAPH_BREAK_RE
        .split(&prose_html)
        .map(visible_char_count)
        .max()
        .unwrap_or(0);

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let effective_cognitive_load = (prose_chars as f64
        + (list_chars as f64) * 0.75
        + (visual_caption_chars as f64) * 0.60
        + (table_chars as f64) * 0.55)
        .round() as usize;

    NotesStructureMetrics {
        total_visible_chars,
        prose_chars,
        table_chars,
        list_chars,
        visual_caption_chars,
        citation_chars,
        table_count,
        list_item_count,
        max_paragraph_chars,
        effective_cognitive_load,
    }
}

/// Extrai blocos balanceados (suportando aninhamento) correspondentes às tags informadas,
/// retornando os blocos encontrados e a string residual com os blocos substituídos por quebras de linha.
fn extract_balanced_blocks(html: &str, tags: &[&str]) -> (Vec<String>, String) {
    let bytes = html.as_bytes();
    let len = bytes.len();
    let mut blocks = Vec::new();
    let mut residual = String::with_capacity(len);
    let mut i = 0usize;

    while i < len {
        if bytes[i] == b'<'
            && let Some(close_pos) = find_tag_end_local(bytes, i + 1)
        {
            let tag_inner = html[i + 1..close_pos].trim();
            let is_closing = tag_inner.starts_with('/');
            let is_self_closing = !is_closing && tag_inner.ends_with('/');
            let name_slice = tag_inner.strip_prefix('/').unwrap_or(tag_inner);
            let tag_name = extract_tag_name_local(name_slice);

            if !is_closing
                && !is_self_closing
                && tags.iter().any(|t| t.eq_ignore_ascii_case(tag_name))
                && let Some(end_block) = find_balanced_block_end(html, i, tag_name)
            {
                blocks.push(html[i..end_block].to_string());
                residual.push('\n');
                i = end_block;
                continue;
            }
        }

        let ch = html[i..].chars().next().unwrap_or(' ');
        residual.push(ch);
        i += ch.len_utf8();
    }

    (blocks, residual)
}

fn find_balanced_block_end(raw: &str, start_pos: usize, target_tag: &str) -> Option<usize> {
    let bytes = raw.as_bytes();
    let len = bytes.len();
    let mut depth = 0usize;
    let mut i = start_pos;

    while i < len {
        if bytes[i] == b'<'
            && let Some(close_pos) = find_tag_end_local(bytes, i + 1)
        {
            let tag_inner = raw[i + 1..close_pos].trim();
            let is_closing = tag_inner.starts_with('/');
            let is_self_closing = !is_closing && tag_inner.ends_with('/');
            let name_slice = tag_inner.strip_prefix('/').unwrap_or(tag_inner);
            let tag_name = extract_tag_name_local(name_slice);

            if tag_name.eq_ignore_ascii_case(target_tag) {
                if is_closing {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(close_pos + 1);
                    }
                } else if !is_self_closing {
                    depth += 1;
                }
            }
            i = close_pos + 1;
            continue;
        }
        i += 1;
    }

    None
}

fn find_tag_end_local(bytes: &[u8], start: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    let mut idx = start;

    while idx < bytes.len() {
        let b = bytes[idx];
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'>' => return Some(idx),
                _ => {}
            },
        }
        idx += 1;
    }

    None
}

fn extract_tag_name_local(s: &str) -> &str {
    let trimmed = s.trim_start();
    let end = trimmed
        .bytes()
        .position(|b| !b.is_ascii_alphanumeric())
        .unwrap_or(trimmed.len());
    &trimmed[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restatement_detects_prefixes_and_ignores_explanations() {
        assert!(is_exact_answer_restatement(
            "A resposta correta é: <b>Hipertensão arterial</b>.",
            "Hipertensão arterial"
        ));
        assert!(is_exact_answer_restatement(
            "A resposta correta é Hipertensão arterial.",
            "Hipertensão arterial"
        ));
        assert!(is_exact_answer_restatement(
            "Resposta: Aldosterona",
            "aldosterona"
        ));
        assert!(!is_exact_answer_restatement(
            "A mitocôndria sintetiza ATP via fosforilação oxidativa.",
            "Mitocôndria"
        ));
    }

    #[test]
    fn check_cloze_target_restatement_emits_expected_diagnostic() {
        let diag =
            check_cloze_target_restatement("Resposta correta: Aldosterona", "Aldosterona", 2, 14)
                .expect("should emit NOTE.DUP_ANSWER_WARN");
        assert_eq!(diag.code, "NOTE.DUP_ANSWER_WARN");
        assert_eq!(diag.severity, Severity::Warn);
        assert_eq!(diag.owner_skill, "anki-notes-enricher");
    }

    #[test]
    fn compute_notes_metrics_weights_blocks_and_excludes_footer() {
        let html = "<p>Prosa curta.</p><table><tr><td>A</td><td>B</td></tr></table><ul><li>Um</li><li>Dois</li></ul><figure><figcaption>Legenda</figcaption></figure><footer>Fonte: Harrison</footer>";
        let metrics = compute_notes_metrics(html);
        assert_eq!(metrics.table_count, 1);
        assert_eq!(metrics.list_item_count, 2);
        assert!(metrics.citation_chars > 0);
        assert!(metrics.effective_cognitive_load < metrics.total_visible_chars);
    }

    #[test]
    fn compute_notes_metrics_handles_nested_lists_and_tables() {
        let html = "<p>Intro</p><table><tr><td><table><tr><td>Nested Cell</td></tr></table></td></tr></table><ul><li>Item 1<ul><li>Sub 1</li><li>Sub 2</li></ul></li><li>Item 2</li></ul>";
        let metrics = compute_notes_metrics(html);
        assert_eq!(metrics.table_count, 1);
        assert_eq!(metrics.list_item_count, 4);
        assert!(metrics.table_chars > 0);
        assert!(metrics.list_chars > 0);
    }
}
