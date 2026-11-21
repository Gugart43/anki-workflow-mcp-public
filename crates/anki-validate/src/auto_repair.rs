//! Deterministic in-memory surgical auto-repair engine for `batch_patch_draft`.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

use crate::cloze_renumber::renumber_cloze;

static BREAKING_ATTR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\s+(?:style|bgcolor|border|color|align|cellpadding|cellspacing|background|valign|on[a-z]+)\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)"#)
        .expect("valid breaking_attr_re regex")
});

static TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)<([a-zA-Z0-9]+)(\s+[^>]*)?>"#).expect("valid tag_re regex"));

/// Individual repair action record describing what changed and under which rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairAction {
    pub field: String,
    pub rule_code: String,
    pub before_excerpt: String,
    pub after_excerpt: String,
    pub safe_auto_apply: bool,
}

/// Aggregated repair report for a card in the batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardRepairReport {
    pub card_index: usize,
    pub applied_actions: Vec<RepairAction>,
    pub manual_review_required: Vec<String>,
}

/// Auto-repair a retrieval field (Sentence for Cloze, or Prompt/Answer for QA).
#[must_use]
pub fn auto_repair_retrieval_field(input: &str, is_cloze: bool) -> (String, Vec<RepairAction>) {
    let mut current = input.to_string();
    let mut actions = Vec::new();
    let field = if is_cloze {
        "Sentença"
    } else {
        "Enunciado/Resposta"
    };

    // 1. Quebra física -> <br> e colapso <br><br>+
    // CRLF must collapse to a single <br>, so these replacements cannot be
    // folded into one character-set replace.
    #[allow(clippy::collapsible_str_replace)]
    let step1 = current
        .replace("\r\n", "<br>")
        .replace('\n', "<br>")
        .replace('\r', "<br>");
    let mut step1_clean = step1.replace("<br/>", "<br>").replace("<br />", "<br>");
    while step1_clean.contains("<br><br>") {
        step1_clean = step1_clean.replace("<br><br>", "<br>");
    }
    record_if_changed(
        &mut current,
        step1_clean,
        field,
        "FMT.RAW_NEWLINE",
        &mut actions,
    );

    // 2. Escape de pipe cru
    let step2 = current.replace('|', "&#124;");
    record_if_changed(&mut current, step2, field, "FMT.RAW_PIPE", &mut actions);

    // 3. MathJax $...$ -> \(...\) poupando moedas
    let step3 = repair_mathjax_dollars(&current);
    record_if_changed(&mut current, step3, field, "MATH.DOLLAR", &mut actions);

    // 4. Normalização de unidades clínicas
    let step4 = normalize_clinical_units(&current);
    record_if_changed(&mut current, step4, field, "VOCAB.UNIT_NORM", &mut actions);

    // 5. Remoção de prefixos decorativos redundantes
    let step5 = strip_decorative_prefix(&current);
    record_if_changed(
        &mut current,
        step5,
        field,
        "PRES.RETRIEVAL_DECORATION_WARN",
        &mut actions,
    );

    // 6. Renumeração sequencial Cloze sem lacunas
    if is_cloze {
        let step6 = renumber_cloze_sequential(&current);
        record_if_changed(&mut current, step6, field, "CLOZE.NONSEQ", &mut actions);
    }

    (current.trim().to_string(), actions)
}

/// Auto-repair notes HTML field (Notas Adicionais), stripping dark-mode breaking attributes
/// and escaping raw pipes without altering valid semantic classes or table structures.
#[must_use]
pub fn auto_repair_notes_html(input: &str) -> (String, Vec<RepairAction>) {
    let mut current = input.to_string();
    let mut actions = Vec::new();

    let step1 = current.split_whitespace().collect::<Vec<_>>().join(" ");
    record_if_changed(
        &mut current,
        step1,
        "Notas Adicionais",
        "FMT.RAW_NEWLINE",
        &mut actions,
    );

    let step2 = current.replace('|', "&#124;");
    record_if_changed(
        &mut current,
        step2,
        "Notas Adicionais",
        "FMT.RAW_PIPE",
        &mut actions,
    );

    let step3 = repair_mathjax_dollars(&current);
    record_if_changed(
        &mut current,
        step3,
        "Notas Adicionais",
        "MATH.DOLLAR",
        &mut actions,
    );

    let step4 = strip_dark_mode_breaking_attrs(&current);
    record_if_changed(
        &mut current,
        step4,
        "Notas Adicionais",
        "TBL.INLINE_STYLE_FORBIDDEN",
        &mut actions,
    );

    (current, actions)
}

/// Converts `$$...$$` -> `\[...\]` and `$...$` -> `\(...\)`, safely preserving currencies
/// (`R$ 50`, `r$ 20`, `US$ 10`, `C$ 30`, `BRL$ 100`) and avoiding dollar collisions.
#[must_use]
pub fn repair_mathjax_dollars(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // 1. Display Math: $$...$$ -> \[...\]
        if i + 1 < len
            && bytes[i] == b'$'
            && bytes[i + 1] == b'$'
            && let Some(rel) = input[i + 2..].find("$$")
        {
            let inner = &input[i + 2..i + 2 + rel];
            out.push_str(r"\[");
            out.push_str(inner);
            out.push_str(r"\]");
            i = i + 2 + rel + 2;
            continue;
        }

        // 2. Inline Math: $...$ -> \(...\)
        if bytes[i] == b'$' {
            // Spares currency symbols (R$, US$, etc.)
            if is_currency_prefix(input, i) {
                out.push('$');
                i += 1;
                continue;
            }

            // Opening dollar cannot be followed by space, tab, newline, or another dollar
            if i + 1 < len {
                let next_byte = bytes[i + 1];
                if next_byte != b' '
                    && next_byte != b'\t'
                    && next_byte != b'\n'
                    && next_byte != b'\r'
                    && next_byte != b'$'
                {
                    let mut j = i + 1;
                    let mut found = None;

                    while j < len {
                        // Math inline cannot cross newlines or <br>
                        if bytes[j] == b'\n' || input[j..].starts_with("<br>") {
                            break;
                        }

                        if bytes[j] == b'$' {
                            // Cannot be escaped: \$
                            if j > 0 && bytes[j - 1] == b'\\' {
                                j += 1;
                                continue;
                            }
                            // Cannot be preceded by whitespace: $ math $
                            if j > 0 && matches!(bytes[j - 1], b' ' | b'\t' | b'\r') {
                                j += 1;
                                continue;
                            }
                            // Cannot be a currency prefix like " ... R$", unless it's a single variable "$R$"
                            if is_currency_prefix(input, j) {
                                let inner_slice = &input[i + 1..j];
                                let is_enclosed_var =
                                    matches!(inner_slice, "R" | "r" | "U" | "u" | "C" | "c");
                                if !is_enclosed_var {
                                    j += 1;
                                    continue;
                                }
                            }
                            found = Some(j);
                            break;
                        }
                        let ch = input[j..].chars().next().expect("valid utf-8 char");
                        j += ch.len_utf8();
                    }

                    if let Some(end_idx) = found {
                        let inner = &input[i + 1..end_idx];
                        if !inner.contains("R$") && !inner.contains("US$") && !inner.contains("r$")
                        {
                            out.push_str(r"\(");
                            out.push_str(inner);
                            out.push_str(r"\)");
                            i = end_idx + 1;
                            continue;
                        }
                    }
                }
            }
        }

        let ch = input[i..].chars().next().expect("valid utf-8 char");
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

/// Identifies if a `$` at byte index `idx` is part of a currency symbol (`R$`, `US$`, `C$`, etc.).
fn is_currency_prefix(s: &str, idx: usize) -> bool {
    const CURRENCY_PREFIXES: &[&str] = &[
        "BRL", "brl", "US", "us", "AU", "au", "NZ", "nz", "R", "r", "U", "u", "C", "c",
    ];
    for pfx in CURRENCY_PREFIXES {
        let plen = pfx.len();
        if idx >= plen && s.get(idx - plen..idx) == Some(*pfx) {
            let before_idx = idx - plen;
            if before_idx == 0 {
                return true;
            }
            if let Some(c) = s[..before_idx].chars().last()
                && !c.is_alphanumeric()
            {
                return true;
            }
        }
    }
    false
}

/// Normalizes clinical units to canonical casing (e.g. `mg/dl` -> `mg/dL`, `meq/l` -> `mEq/L`).
#[must_use]
pub fn normalize_clinical_units(input: &str) -> String {
    let mut s = input.to_string();
    let pairs = [
        ("mg/dl", "mg/dL"),
        ("MG/DL", "mg/dL"),
        ("Mg/dl", "mg/dL"),
        ("mg/Dl", "mg/dL"),
        ("meq/l", "mEq/L"),
        ("MEQ/L", "mEq/L"),
        ("Meq/l", "mEq/L"),
        ("meq/L", "mEq/L"),
        ("mEq/l", "mEq/L"),
        ("mmol/l", "mmol/L"),
        ("MMOL/L", "mmol/L"),
        ("Mmol/l", "mmol/L"),
        ("mmhg", "mmHg"),
        ("MMHG", "mmHg"),
        ("Mmhg", "mmHg"),
        ("ml/kg/h", "mL/kg/h"),
        ("ML/KG/H", "mL/kg/h"),
        ("g/dl", "g/dL"),
        ("G/DL", "g/dL"),
        ("g/Dl", "g/dL"),
        ("G/dl", "g/dL"),
        ("ug/kg/min", "µg/kg/min"),
        ("UG/KG/MIN", "µg/kg/min"),
        ("ug/", "µg/"),
        ("UG/", "µg/"),
    ];
    for (from, to) in pairs {
        s = s.replace(from, to);
    }
    s
}

/// Strips decorative prompt prefixes like `Pergunta:`, `Questão:`, `Resposta:`, `Gabarito:`.
#[must_use]
pub fn strip_decorative_prefix(input: &str) -> String {
    let trimmed = input.trim_start();
    let lower = trimmed.to_lowercase();
    for pfx in [
        "pergunta:",
        "questão:",
        "questao:",
        "resposta:",
        "gabarito:",
    ] {
        if lower.starts_with(pfx) {
            let char_count = pfx.chars().count();
            let stripped: String = trimmed.chars().skip(char_count).collect();
            return stripped.trim_start().to_string();
        }
    }
    trimmed.to_string()
}

/// Automatically renumbers Cloze deletions strictly sequentially (`c1..cK`),
/// preserving LaTeX formulas, hints, and Unicode text.
#[must_use]
pub fn renumber_cloze_sequential(input: &str) -> String {
    let normalized = if input.contains("{{C") {
        let mut s = String::with_capacity(input.len());
        let mut i = 0;
        let bytes = input.as_bytes();
        while i < bytes.len() {
            if input[i..].starts_with("{{C") && i + 3 < bytes.len() && bytes[i + 3].is_ascii_digit()
            {
                s.push_str("{{c");
                i += 3;
            } else {
                let ch = input[i..].chars().next().expect("valid utf-8 char");
                s.push(ch);
                i += ch.len_utf8();
            }
        }
        s
    } else {
        input.to_string()
    };
    renumber_cloze(&normalized)
}

/// Strips forbidden and dark-mode breaking attributes (`style`, `bgcolor`, `border`, `color`,
/// `align`, `cellpadding`, `cellspacing`, `background`, `valign`, and script event handlers)
/// from HTML tags without stripping semantic classes or altering safe content.
#[must_use]
pub fn strip_dark_mode_breaking_attrs(html: &str) -> String {
    TAG_RE
        .replace_all(html, |caps: &regex::Captures<'_>| {
            let tag_name = &caps[1];
            if let Some(attrs_match) = caps.get(2) {
                let attrs = attrs_match.as_str();
                let cleaned = BREAKING_ATTR_RE.replace_all(attrs, "").trim().to_string();
                if cleaned.is_empty() {
                    format!("<{tag_name}>")
                } else {
                    format!("<{tag_name} {cleaned}>")
                }
            } else {
                caps[0].to_string()
            }
        })
        .into_owned()
}

fn record_if_changed(
    current: &mut String,
    candidate: String,
    field: &str,
    rule_code: &str,
    actions: &mut Vec<RepairAction>,
) {
    if candidate != *current {
        actions.push(RepairAction {
            field: field.to_string(),
            rule_code: rule_code.to_string(),
            before_excerpt: current.chars().take(60).collect(),
            after_excerpt: candidate.chars().take(60).collect(),
            safe_auto_apply: true,
        });
        *current = candidate;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repair_mathjax_dollars_preserves_currency() {
        // Standard integration case
        assert_eq!(
            repair_mathjax_dollars("Custo R$ 50,00; fórmula $pH = 7.4$"),
            r"Custo R$ 50,00; fórmula \(pH = 7.4\)"
        );

        // Multiple currencies
        assert_eq!(
            repair_mathjax_dollars("Valores: R$ 100,00, US$ 25 e C$ 10"),
            "Valores: R$ 100,00, US$ 25 e C$ 10"
        );

        // Standalone prices without math pairing
        assert_eq!(
            repair_mathjax_dollars("Preço $10 e R$ 50"),
            "Preço $10 e R$ 50"
        );
        assert_eq!(repair_mathjax_dollars("Preço $10 e $20"), "Preço $10 e $20");

        // Display math
        assert_eq!(
            repair_mathjax_dollars("$$E = mc^2$$ e R$ 500"),
            r"\[E = mc^2\] e R$ 500"
        );

        // Single math variable matching currency letter
        assert_eq!(
            repair_mathjax_dollars("Resistência $R$ na lei de Ohm"),
            r"Resistência \(R\) na lei de Ohm"
        );
    }

    #[test]
    fn test_auto_repair_retrieval_field_cloze_and_units() {
        let (fixed, actions) =
            auto_repair_retrieval_field("Pergunta: {{c1::Na}} e {{c3::K}} 4 meq/l\n\n", true);
        assert!(fixed.contains("{{c2::K}}"));
        assert!(fixed.contains("mEq/L"));
        assert!(!fixed.starts_with("Pergunta:"));
        assert!(!actions.is_empty());

        let rule_codes: Vec<&str> = actions.iter().map(|a| a.rule_code.as_str()).collect();
        assert!(rule_codes.contains(&"FMT.RAW_NEWLINE"));
        assert!(rule_codes.contains(&"VOCAB.UNIT_NORM"));
        assert!(rule_codes.contains(&"PRES.RETRIEVAL_DECORATION_WARN"));
        assert!(rule_codes.contains(&"CLOZE.NONSEQ"));
    }

    #[test]
    fn test_auto_repair_notes_html_preserves_pipe_and_strips_styles() {
        let input = r##"<table bgcolor="#fff"><tr><td style="color:#000">A | B</td></tr></table>"##;
        let (notes_fixed, actions) = auto_repair_notes_html(input);

        assert!(!notes_fixed.contains("bgcolor="));
        assert!(!notes_fixed.contains("style="));
        assert!(notes_fixed.contains("&#124;"));
        assert_eq!(notes_fixed, "<table><tr><td>A &#124; B</td></tr></table>");

        let rule_codes: Vec<&str> = actions.iter().map(|a| a.rule_code.as_str()).collect();
        assert!(rule_codes.contains(&"FMT.RAW_PIPE"));
        assert!(rule_codes.contains(&"TBL.INLINE_STYLE_FORBIDDEN"));
    }

    #[test]
    fn test_normalize_clinical_units() {
        let input = "Glicemia 180 mg/dl, K 4,0 meq/l, PA 120 mmhg, noradrenalina 0,2 ug/kg/min";
        let expected = "Glicemia 180 mg/dL, K 4,0 mEq/L, PA 120 mmHg, noradrenalina 0,2 µg/kg/min";
        assert_eq!(normalize_clinical_units(input), expected);
    }

    #[test]
    fn test_strip_decorative_prefix() {
        assert_eq!(
            strip_decorative_prefix("Pergunta: Qual o diagnóstico?"),
            "Qual o diagnóstico?"
        );
        assert_eq!(
            strip_decorative_prefix("Questão: Qual a conduta?"),
            "Qual a conduta?"
        );
        assert_eq!(strip_decorative_prefix("Gabarito: IAM de VD"), "IAM de VD");
        assert_eq!(
            strip_decorative_prefix("Resposta: Reposição volêmica"),
            "Reposição volêmica"
        );
    }

    #[test]
    fn test_card_repair_report_serialization() {
        let report = CardRepairReport {
            card_index: 0,
            applied_actions: vec![RepairAction {
                field: "Sentença".to_string(),
                rule_code: "CLOZE.NONSEQ".to_string(),
                before_excerpt: "{{c3::K}}".to_string(),
                after_excerpt: "{{c2::K}}".to_string(),
                safe_auto_apply: true,
            }],
            manual_review_required: vec![],
        };

        let serialized = serde_json::to_string(&report).expect("serializes correctly");
        let deserialized: CardRepairReport =
            serde_json::from_str(&serialized).expect("deserializes correctly");
        assert_eq!(report, deserialized);
    }
}
