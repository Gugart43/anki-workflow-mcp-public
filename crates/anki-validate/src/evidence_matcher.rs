//! Verificação de citações literais tolerante a artefatos de PDF e cobertura de obrigações (`quality_v1`).

use std::collections::BTreeSet;

use anki_domain::{Diagnostic, DiagnosticCode, Severity};
use serde::{Deserialize, Serialize};

pub const CODE_GENERIC_REFERENCE: DiagnosticCode =
    DiagnosticCode::from_static("QUALITY.GENERIC_REFERENCE");
pub const MIN_QUOTE_VISIBLE_CHARS: usize = 15;
pub const OWNER_SKILL: &str = "anki-export-validator";

const GENERIC_REF_ONLY: &[&str] = &[
    "pdf",
    "aula",
    "apostila",
    "resumo",
    "livro",
    "diretriz",
    "harrison",
    "cecil",
    "sabiston",
    "up to date",
    "uptodate",
    "notas",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuoteMatchResult {
    ExactMatch { byte_offset: usize },
    NormalizedPdfMatch { normalized_offset: usize },
    TooShort { visible_len: usize },
    NotFound,
}

impl QuoteMatchResult {
    #[must_use]
    pub fn is_match(&self) -> bool {
        matches!(
            self,
            Self::ExactMatch { .. } | Self::NormalizedPdfMatch { .. }
        )
    }

    #[must_use]
    pub fn offset(&self) -> Option<usize> {
        match self {
            Self::ExactMatch { byte_offset } => Some(*byte_offset),
            Self::NormalizedPdfMatch { normalized_offset } => Some(*normalized_offset),
            Self::TooShort { .. } | Self::NotFound => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObligationSpec {
    pub id: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingSpec {
    pub unit_id: String,
    pub obligation_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObligationCoverageReport {
    pub total_obligations: usize,
    pub covered_count: usize,
    pub coverage_ratio: f32,
    pub uncovered_obligations: Vec<String>,
    pub orphan_obligation_refs: Vec<String>,
}

impl ObligationCoverageReport {
    #[must_use]
    pub fn is_fully_covered(&self) -> bool {
        self.uncovered_obligations.is_empty() && self.orphan_obligation_refs.is_empty()
    }
}

/// Normaliza texto extraído de PDF para busca e verificação de citações literais,
/// retornando tanto o texto normalizado quanto o mapa de byte offsets do texto original.
///
/// Trata:
/// - Quebras de linha CRLF (`\r\n`), CR (`\r`) e LF (`\n`)
/// - Hifenização de fim de linha em português (`hiper-\ntensão` -> `hipertensão`),
///   suportando múltiplos espaços e quebras (`-\r\n  `), preservando hífen intra-linha (`beta-bloqueador`)
/// - Hifens Unicode (U+002D, U+00AD soft hyphen, U+2010, U+2011)
/// - Aspas curvas (`“`, `”`, `„`, `‟` -> `"`) e apóstrofos curvos (`‘`, `’`, `‚`, `‛` -> `'`)
/// - Travessões e traços (`–`, `—`, `−` -> `-`)
/// - Espaços sem quebra (U+00A0, U+2007, U+202F) e caracteres invisíveis de largura zero (U+200B, U+FEFF)
/// - Colapso de múltiplos espaços em branco sequenciais em um único espaço ASCII
#[must_use]
pub fn normalize_pdf_text_with_offsets(input: &str) -> (String, Vec<usize>) {
    // 1. Unificação de quebras e caracteres especiais mantendo os byte offsets originais de cada char
    let mut unified: Vec<(usize, char)> = Vec::with_capacity(input.len());
    let mut chars_iter = input.char_indices().peekable();

    while let Some((byte_idx, c)) = chars_iter.next() {
        match c {
            '\r' => {
                if let Some(&(_, '\n')) = chars_iter.peek() {
                    chars_iter.next();
                }
                unified.push((byte_idx, '\n'));
            }
            '\u{00A0}' | '\u{2007}' | '\u{202F}' => {
                unified.push((byte_idx, ' '));
            }
            '“' | '”' | '„' | '‟' => {
                unified.push((byte_idx, '"'));
            }
            '‘' | '’' | '‚' | '‛' => {
                unified.push((byte_idx, '\''));
            }
            '–' | '—' | '−' => {
                unified.push((byte_idx, '-'));
            }
            '\u{200B}' | '\u{FEFF}' => {
                // Caracteres invisíveis de largura zero são ignorados
            }
            _ => {
                unified.push((byte_idx, c));
            }
        }
    }

    // 2. Remoção de hifenização em quebra de linha e soft hyphens
    let mut joined: Vec<(usize, char)> = Vec::with_capacity(unified.len());
    let mut i = 0;
    while i < unified.len() {
        let (byte_idx, c) = unified[i];

        // Soft hyphen no meio de uma palavra (ex: hiper\u{00AD}tensão)
        if c == '\u{00AD}' {
            let prev_alpha = i > 0 && unified[i - 1].1.is_alphabetic();
            let next_alpha = i + 1 < unified.len() && unified[i + 1].1.is_alphabetic();
            if prev_alpha && next_alpha {
                i += 1;
                continue;
            }
        }

        // Hífen em quebra de linha (ex: hiper-\ntensão)
        if matches!(c, '-' | '\u{00AD}' | '\u{2010}' | '\u{2011}') {
            let prev_alpha = i > 0 && unified[i - 1].1.is_alphabetic();
            let mut k = i + 1;
            while k < unified.len() && (unified[k].1 == ' ' || unified[k].1 == '\t') {
                k += 1;
            }
            if k < unified.len() && unified[k].1 == '\n' {
                k += 1;
                while k < unified.len()
                    && (unified[k].1 == ' ' || unified[k].1 == '\t' || unified[k].1 == '\n')
                {
                    k += 1;
                }
                let next_lower = k < unified.len() && unified[k].1.is_lowercase();
                if prev_alpha && next_lower {
                    i = k;
                    continue;
                }
            }
        }

        if c == '\n' || c == '\t' {
            joined.push((byte_idx, ' '));
        } else {
            joined.push((byte_idx, c));
        }
        i += 1;
    }

    // 3. Colapso de múltiplos espaços em branco e trimming (idêntico a split_whitespace().join(" "))
    let mut result_str = String::with_capacity(joined.len());
    let mut byte_offsets = Vec::with_capacity(joined.len());

    let mut idx = 0;
    while idx < joined.len() && joined[idx].1.is_whitespace() {
        idx += 1;
    }

    while idx < joined.len() {
        let (orig_byte, ch) = joined[idx];
        if ch.is_whitespace() {
            let space_len = ' '.len_utf8();
            result_str.push(' ');
            for _ in 0..space_len {
                byte_offsets.push(orig_byte);
            }
            while idx < joined.len() && joined[idx].1.is_whitespace() {
                idx += 1;
            }
        } else {
            let ch_len = ch.len_utf8();
            result_str.push(ch);
            for _ in 0..ch_len {
                byte_offsets.push(orig_byte);
            }
            idx += 1;
        }
    }

    if result_str.ends_with(' ') {
        result_str.pop();
        byte_offsets.pop();
    }

    (result_str, byte_offsets)
}

/// Reconstrói palavras hifenizadas em quebra de linha de PDF (`hiper-\ntensão` -> `hipertensão`),
/// preservando hifens intra-linha (`beta-bloqueador`), e normaliza aspas/travessões/espaços.
#[must_use]
pub fn normalize_pdf_text(input: &str) -> String {
    normalize_pdf_text_with_offsets(input).0
}

/// Mapeia um byte offset no texto normalizado de volta ao byte offset no texto fonte original.
#[must_use]
pub fn map_normalized_offset_to_source(
    source_text: &str,
    normalized_offset: usize,
) -> Option<usize> {
    let (_, offset_map) = normalize_pdf_text_with_offsets(source_text);
    offset_map.get(normalized_offset).copied()
}

/// Verifica a presença de uma citação literal no texto fonte, tolerando hifenização de PDF.
/// Retorna `QuoteMatchResult` indicando se a citação foi encontrada de forma exata,
/// por normalização de PDF, se é curta demais, ou se não foi encontrada.
#[must_use]
pub fn verify_quote_in_source(quote: &str, source_text: &str) -> QuoteMatchResult {
    let visible_len = quote
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .count();
    if visible_len < MIN_QUOTE_VISIBLE_CHARS {
        return QuoteMatchResult::TooShort { visible_len };
    }
    if let Some(byte_offset) = source_text.find(quote) {
        return QuoteMatchResult::ExactMatch { byte_offset };
    }
    let norm_q = normalize_pdf_text(quote);
    let norm_s = normalize_pdf_text(source_text);
    if let Some(normalized_offset) = norm_s.find(&norm_q) {
        QuoteMatchResult::NormalizedPdfMatch { normalized_offset }
    } else {
        QuoteMatchResult::NotFound
    }
}

/// Verifica e retorna o span de byte `(start, end)` no texto original da citação localizada,
/// mesmo quando a correspondência dependeu de normalização de quebras ou hifenizações de PDF.
#[must_use]
pub fn verify_quote_in_source_with_source_span(
    quote: &str,
    source_text: &str,
) -> Option<(usize, usize)> {
    match verify_quote_in_source(quote, source_text) {
        QuoteMatchResult::ExactMatch { byte_offset } => {
            Some((byte_offset, byte_offset + quote.len()))
        }
        QuoteMatchResult::NormalizedPdfMatch { normalized_offset } => {
            let norm_q = normalize_pdf_text(quote);
            let (_norm_s, offset_map) = normalize_pdf_text_with_offsets(source_text);
            if normalized_offset + norm_q.len() <= offset_map.len() {
                let start_byte = offset_map.get(normalized_offset).copied()?;
                let end_char_byte = offset_map
                    .get(normalized_offset + norm_q.len() - 1)
                    .copied()?;
                let remaining = &source_text[end_char_byte..];
                let next_char_len = remaining.chars().next().map_or(1, |c| c.len_utf8());
                Some((start_byte, end_char_byte + next_char_len))
            } else {
                None
            }
        }
        QuoteMatchResult::TooShort { .. } | QuoteMatchResult::NotFound => None,
    }
}

/// Avalia a especificidade de uma referência bibliográfica (`Referências`),
/// sinalizando referências genéricas vazias, monopalavras conhecidas ou ausentes de localizador (ano/capítulo/página/seção).
#[must_use]
pub fn check_reference_specificity(reference: &str, row: Option<usize>) -> Option<Diagnostic> {
    let trimmed = reference.trim();
    let lower = trimmed.to_lowercase();
    let has_locator = lower.contains("cap")
        || lower.contains("pág")
        || lower.contains("pag")
        || lower.contains("p.")
        || lower.contains("seç")
        || lower.contains("sec")
        || trimmed.chars().filter(|c| c.is_ascii_digit()).count() >= 4;

    let clean_lower = lower.trim_matches(|c: char| c.is_ascii_punctuation() || c.is_whitespace());

    if trimmed.is_empty()
        || GENERIC_REF_ONLY.contains(&lower.as_str())
        || GENERIC_REF_ONLY.contains(&clean_lower)
        || (!has_locator && trimmed.split_whitespace().count() <= 2)
    {
        let mut d = Diagnostic::new(
            CODE_GENERIC_REFERENCE,
            Severity::Warn,
            OWNER_SKILL,
            format!("Referência genérica ('{trimmed}'): informe obra/diretriz + ano, capítulo, seção ou página."),
        )
        .with_field("Referências");
        if let Some(r) = row {
            d = d.with_row(r);
        }
        return Some(d);
    }
    None
}

/// Avalia o grau de cobertura de um conjunto de obrigações de aprendizado clínico (`ObligationSpec`)
/// por unidades/flashcards vinculados (`BindingSpec`), identificando lacunas e referências órfãs.
#[must_use]
pub fn check_obligation_coverage(
    obligations: &[ObligationSpec],
    bindings: &[BindingSpec],
) -> ObligationCoverageReport {
    let known: BTreeSet<String> = obligations.iter().map(|o| o.id.clone()).collect();
    let mut covered = BTreeSet::new();
    let mut orphans = BTreeSet::new();

    for b in bindings {
        for oid in &b.obligation_ids {
            if known.contains(oid) {
                covered.insert(oid.clone());
            } else {
                orphans.insert(oid.clone());
            }
        }
    }

    let uncovered_obligations: Vec<String> = known.difference(&covered).cloned().collect();
    let total = known.len();
    let covered_count = covered.len();
    let coverage_ratio = if total == 0 {
        1.0
    } else {
        covered_count as f32 / total as f32
    };

    ObligationCoverageReport {
        total_obligations: total,
        covered_count,
        coverage_ratio,
        uncovered_obligations,
        orphan_obligation_refs: orphans.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_pdf_hyphenation_and_whitespace() {
        let pdf_src = "Na cetoacidose diabética, a hiper-\ntensão intracraniana é complicação temida do edema cerebral.";
        let norm = normalize_pdf_text(pdf_src);
        assert!(norm.contains("hipertensão intracraniana"));
        assert!(!norm.contains("hiper-"));

        // Hífen com CRLF e múltiplos espaços
        let src_crlf = "Manejo do trombo-\r\n  embolismo pulmonar maciço.";
        assert_eq!(
            normalize_pdf_text(src_crlf),
            "Manejo do tromboembolismo pulmonar maciço."
        );

        // Hífen com espaço antes de newline
        let src_trailing_space = "Uso de glico- \n corticoide sistêmico.";
        assert_eq!(
            normalize_pdf_text(src_trailing_space),
            "Uso de glicocorticoide sistêmico."
        );

        // Preservação de hífen intra-linha (não quebra de linha)
        let src_intraline = "Paciente em uso de beta-bloqueador e alfa-1-antitripsina.";
        let norm_intra = normalize_pdf_text(src_intraline);
        assert!(norm_intra.contains("beta-bloqueador"));
        assert!(norm_intra.contains("alfa-1-antitripsina"));

        // Aspas curvas, travessões e espaços não quebráveis
        let src_quotes = "“Tratamento de primeira linha” – diretriz SBC (pág. 12).";
        assert_eq!(
            normalize_pdf_text(src_quotes),
            "\"Tratamento de primeira linha\" - diretriz SBC (pág. 12)."
        );

        // Soft hyphen (U+00AD) dentro de palavra
        let src_soft = "hiper\u{00AD}tensão";
        assert_eq!(normalize_pdf_text(src_soft), "hipertensão");
    }

    #[test]
    fn test_verify_quote_exact_and_normalized() {
        let pdf_src = "Na cetoacidose diabética, a hiper-\ntensão intracraniana é complicação temida do edema cerebral.";

        // Citação literal contendo a palavra que foi quebrada no PDF
        let res = verify_quote_in_source("hipertensão intracraniana é complicação temida", pdf_src);
        assert!(matches!(res, QuoteMatchResult::NormalizedPdfMatch { .. }));
        assert!(res.is_match());
        assert!(res.offset().is_some());

        // Citação exata (presente sem necessidade de desfazer quebra de linha)
        let exact_quote = "complicação temida do edema cerebral";
        let res_exact = verify_quote_in_source(exact_quote, pdf_src);
        assert!(matches!(res_exact, QuoteMatchResult::ExactMatch { .. }));
        assert!(res_exact.is_match());

        // Citação muito curta (< 15 caracteres visíveis)
        let res_short = verify_quote_in_source("curto demais", pdf_src);
        assert!(
            matches!(res_short, QuoteMatchResult::TooShort { visible_len } if visible_len < MIN_QUOTE_VISIBLE_CHARS)
        );
        assert!(!res_short.is_match());

        // Citação inexistente
        let res_not_found =
            verify_quote_in_source("infarto agudo do miocárdio com supradesnível", pdf_src);
        assert_eq!(res_not_found, QuoteMatchResult::NotFound);
        assert!(!res_not_found.is_match());
    }

    #[test]
    fn test_offset_preservation_and_source_span() {
        let pdf_src = "Na cetoacidose diabética, a hiper-\ntensão intracraniana é complicação temida do edema cerebral.";
        let quote = "hipertensão intracraniana é complicação temida";

        let span = verify_quote_in_source_with_source_span(quote, pdf_src);
        assert!(span.is_some());
        let (start, end) = span.unwrap();
        let source_slice = &pdf_src[start..end];
        assert_eq!(
            source_slice,
            "hiper-\ntensão intracraniana é complicação temida"
        );

        // Mapeamento de offset direto
        let mapped = map_normalized_offset_to_source(pdf_src, 0);
        assert_eq!(mapped, Some(0));
    }

    #[test]
    fn test_reference_specificity() {
        // Genérica monopalavra
        let diag1 = check_reference_specificity("Harrison", Some(2));
        assert!(diag1.is_some());
        let d1 = diag1.unwrap();
        assert_eq!(d1.code, CODE_GENERIC_REFERENCE);
        assert_eq!(d1.row, Some(2));
        assert_eq!(d1.field.as_deref(), Some("Referências"));

        // Genérica com pontuação
        assert!(check_reference_specificity("UpToDate.", Some(3)).is_some());
        assert!(check_reference_specificity("diretriz", None).is_some());
        assert!(check_reference_specificity("   ", None).is_some());

        // Específica completa com edição, capítulo e página
        let ok1 = check_reference_specificity("Harrison 21ª ed., Cap. 137, p. 1042", Some(2));
        assert!(ok1.is_none());

        // Específica com ano e seção
        let ok2 = check_reference_specificity(
            "ADA Consensus Report on Hyperglycemic Crises (2024), Sec. Potassium.",
            Some(4),
        );
        assert!(ok2.is_none());
    }

    #[test]
    fn test_obligation_coverage() {
        let obligations = vec![
            ObligationSpec {
                id: "OB-CAD-01".into(),
                summary: "Conduta inicial com K+ < 3,3".into(),
            },
            ObligationSpec {
                id: "OB-CAD-02".into(),
                summary: "Tríade diagnóstica da CAD".into(),
            },
            ObligationSpec {
                id: "OB-IAM-01".into(),
                summary: "Confirmação de IAM de VD".into(),
            },
        ];

        let bindings = vec![
            BindingSpec {
                unit_id: "QA-1".into(),
                obligation_ids: vec!["OB-CAD-01".into()],
            },
            BindingSpec {
                unit_id: "CL-1".into(),
                obligation_ids: vec!["OB-CAD-02".into(), "OB-ORPHAN-99".into()],
            },
        ];

        let rep = check_obligation_coverage(&obligations, &bindings);
        assert_eq!(rep.total_obligations, 3);
        assert_eq!(rep.covered_count, 2);
        assert!((rep.coverage_ratio - (2.0 / 3.0)).abs() < 1e-4);
        assert_eq!(rep.uncovered_obligations, vec!["OB-IAM-01"]);
        assert_eq!(rep.orphan_obligation_refs, vec!["OB-ORPHAN-99"]);
        assert!(!rep.is_fully_covered());

        // Cobertura completa
        let full_bindings = vec![BindingSpec {
            unit_id: "QA-1".into(),
            obligation_ids: vec!["OB-CAD-01".into(), "OB-CAD-02".into(), "OB-IAM-01".into()],
        }];
        let full_rep = check_obligation_coverage(&obligations, &full_bindings);
        assert_eq!(full_rep.uncovered_obligations, Vec::<String>::new());
        assert_eq!(full_rep.orphan_obligation_refs, Vec::<String>::new());
        assert_eq!(full_rep.coverage_ratio, 1.0);
        assert!(full_rep.is_fully_covered());
    }
}
