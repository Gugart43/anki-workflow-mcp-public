use std::collections::HashMap;

use anki_parser::find_cloze_spans;

/// Automatically renumber cloze deletions `cN` to strictly sequential `c1..cK`
/// in order of first appearance, preserving hints, nested braces, and Unicode.
#[must_use]
pub fn renumber_cloze(text: &str) -> String {
    let spans = find_cloze_spans(text);
    if spans.is_empty() {
        return text.to_string();
    }

    let mut mapping: HashMap<usize, usize> = HashMap::with_capacity(spans.len());
    let mut next_idx: usize = 1;
    for span in &spans {
        mapping.entry(span.index).or_insert_with(|| {
            let assigned = next_idx;
            next_idx += 1;
            assigned
        });
    }

    let mut out = String::with_capacity(text.len());
    let mut cursor: usize = 0;

    for span in &spans {
        if span.start > cursor {
            out.push_str(&text[cursor..span.start]);
        }
        let new_idx = mapping[&span.index];
        out.push_str("{{c");
        out.push_str(&new_idx.to_string());
        out.push_str("::");
        out.push_str(&span.target);
        if let Some(hint) = &span.hint {
            out.push_str("::");
            out.push_str(hint);
        }
        out.push_str("}}");
        cursor = span.end;
    }

    if cursor < text.len() {
        out.push_str(&text[cursor..]);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::renumber_cloze;

    #[test]
    fn test_renumber_cloze_empty_and_negative() {
        assert_eq!(renumber_cloze(""), "");
        let plain = "No cloze here, just {{c::missing_num}} and {{c1::unclosed";
        assert_eq!(renumber_cloze(plain), plain);
    }

    #[test]
    fn test_renumber_cloze_repeated_first_appearance() {
        let input =
            "First {{c5::alpha}}, second {{c2::beta}}, again {{c5::gamma}}, third {{c9::delta}}.";
        let expected =
            "First {{c1::alpha}}, second {{c2::beta}}, again {{c1::gamma}}, third {{c3::delta}}.";
        assert_eq!(renumber_cloze(input), expected);
    }

    #[test]
    fn test_renumber_cloze_unicode_hints_and_nested_braces() {
        let input = "Ação: {{c4::\\frac{a}{b}::fração}} e {{c10::café::bebida}} fim.";
        let expected = "Ação: {{c1::\\frac{a}{b}::fração}} e {{c2::café::bebida}} fim.";
        assert_eq!(renumber_cloze(input), expected);
    }

    #[test]
    fn test_renumber_cloze_emoji_and_parity_escapes() {
        let input = "🩺 Diagnóstico: {{c9::\\left\\{ x \\right\\}::fórmula}} e 🎉 {{c3::imunidade::defesa}} 💉 {{c9::repetido}} fim.";
        let expected = "🩺 Diagnóstico: {{c1::\\left\\{ x \\right\\}::fórmula}} e 🎉 {{c2::imunidade::defesa}} 💉 {{c1::repetido}} fim.";
        assert_eq!(renumber_cloze(input), expected);
    }
}
