//! Additional quality-only checks. These do not alter legacy validator contracts.
use anki_domain::{Diagnostic, Severity};
use anki_parser::{count_cloze_indices, iter_data_lines, split_row};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

const OWNER_SKILL: &str = "anki-post-curator";

static YEAR_4_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:19|20)\d{2}\b").expect("valid year regex"));

static REF_LOCATOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:\b(?:se[çc][ãa]o|sec|cap[íi]tulo|cap|p[áa]gina|p[áa]g|pp?|ed|edi[çc][ãa]o|edition|vol|volume|tabela|quadro|figura|fig|artigo|art|ch|chapter|section|page)\.?\s*(?:\d+|[ivxlcdm]+)\b|\b(?:se[çc][ãa]o|cap[íi]tulo|edi[çc][ãa]o)\b|\d+[ªºa]\s*ed(?:i[çc][ãa]o|\.)?)",
    )
    .expect("valid ref locator regex")
});

static DOI_OR_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:https?://|www\.|\bdoi\s*:|\b10\.\d{4,9}/)").expect("valid doi/url regex")
});

static BINARY_CHOICE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[([^\[\]\n]{1,80})/([^\[\]\n]{1,80})\]").expect("valid binary choice regex")
});

static MC_COMMAND_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:\b[ée]\s+incorreto\s+afirmar\b|\bassinale\s+a\s+alternativa\b|\bmarque\s+a\s+alternativa\b|\bexceto\s*:)",
    )
    .expect("valid mc command regex")
});

static BINARY_VECTOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s)[01](?:\s+[01]){2,}(?:\s|$)").expect("valid binary vector regex")
});

static DUAL_TARGET_INTERROGATIVES_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:qual|quais|como)\b[^?.!\n]*\b(?:e|ou|versus|vs\.?)\s+(?:qual|quais|como)\b",
    )
    .expect("valid dual target interrogatives regex")
});

static DUAL_TARGET_MECHANISM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:qual|quais)\b[^?.!\n]*\be\s+(?:o|a)\s+(?:mecanismo|tratamento|diagn[óo]stico|conduta|dose|posologia|progn[óo]stico)\b",
    )
    .expect("valid dual target mechanism regex")
});

static UL_OPEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<ul\b").expect("valid ul open regex"));

static UL_BLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<ul\b[^>]*>(.*?)</ul>").expect("valid ul block regex"));

static LI_OPEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<li\b").expect("valid li open regex"));

static IMG_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<img\b[^>]*>").expect("valid img tag regex"));

static ALT_VALUE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\balt\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#)
        .expect("valid alt value regex")
});

static GENERIC_ALT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:img|imagem|image|foto|figura)(?:\s*\d+)?$")
        .expect("valid generic alt regex")
});

const GENERIC_REF_LABELS: &[&str] = &[
    "fluxograma",
    "tdc",
    "resumo",
    "aula",
    "apostila",
    "uptodate",
    "anotacao",
    "anotacoes",
    "livro",
    "diretriz",
];

const UNIT_TOKENS: &[&str] = &[
    "mg", "mcg", "ug", "g", "kg", "ml", "l", "dl", "ui", "u", "meq", "mmol", "mol", "min", "seg",
    "h", "dia", "dias", "semana", "semanas", "mes", "ano", "anos", "kcal", "cm", "mm", "bpm",
    "mmhg", "kpa", "ng", "pg",
];

fn normalized_visible(text: &str) -> String {
    crate::qa::html_to_visible_text(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn fold_accents_lower(text: &str) -> String {
    text.chars()
        .flat_map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => {
                vec!['a']
            }
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => vec!['e'],
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => vec!['i'],
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => {
                vec!['o']
            }
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => vec!['u'],
            'ç' | 'Ç' => vec!['c'],
            other => other.to_lowercase().collect(),
        })
        .collect()
}

fn is_generic_reference(vis_ref: &str) -> bool {
    if YEAR_4_RE.is_match(vis_ref)
        || REF_LOCATOR_RE.is_match(vis_ref)
        || DOI_OR_URL_RE.is_match(vis_ref)
    {
        return false;
    }
    let folded = fold_accents_lower(vis_ref);
    let words: Vec<&str> = folded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    !words.is_empty() && words.len() <= 5 && words.iter().any(|w| GENERIC_REF_LABELS.contains(w))
}

fn check_reference_field(ref_raw: &str, row: usize, diagnostics: &mut Vec<Diagnostic>) {
    let vis_ref = crate::qa::html_to_visible_text(ref_raw);
    let trimmed = vis_ref.trim();
    if trimmed.is_empty() {
        diagnostics.push(
            Diagnostic::new(
                "QUALITY.MISSING_REFERENCE",
                Severity::Warn,
                OWNER_SKILL,
                "Campo Referências vazio; adicione fonte verificável (autor/sociedade, ano e seção/página ou DOI/URL).",
            )
            .with_row(row)
            .with_field("Referências"),
        );
    } else if is_generic_reference(trimmed) {
        diagnostics.push(
            Diagnostic::new(
                "QUALITY.GENERIC_REFERENCE",
                Severity::Warn,
                OWNER_SKILL,
                format!(
                    "Referência genérica ('{trimmed}') sem ano, capítulo/seção/página ou DOI/URL verificável."
                ),
            )
            .with_row(row)
            .with_field("Referências"),
        );
    }
}

fn has_binary_choice_bracket(text: &str) -> bool {
    let bytes = text.as_bytes();
    for caps in BINARY_CHOICE_RE.captures_iter(text) {
        let Some(full) = caps.get(0) else {
            continue;
        };
        if full.start() > 0 && bytes[full.start() - 1] == b'\\' {
            continue;
        }
        let left = caps.get(1).map_or("", |m| m.as_str()).trim();
        let right = caps.get(2).map_or("", |m| m.as_str()).trim();
        if left.is_empty()
            || right.is_empty()
            || left.contains('/')
            || right.contains('/')
            || left.contains('<')
            || right.contains('<')
            || left.contains('>')
            || right.contains('>')
        {
            continue;
        }
        if !left.chars().any(char::is_alphabetic) || !right.chars().any(char::is_alphabetic) {
            continue;
        }
        if left.chars().any(|c| c.is_ascii_digit()) && right.chars().any(|c| c.is_ascii_digit()) {
            continue;
        }
        let l_low = left.to_lowercase();
        let r_low = right.to_lowercase();
        if UNIT_TOKENS.contains(&l_low.as_str()) && UNIT_TOKENS.contains(&r_low.as_str()) {
            continue;
        }
        return true;
    }
    false
}

fn check_binary_choice_field(
    text: &str,
    row: usize,
    field: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let visible = crate::qa::html_to_visible_text(text);
    if has_binary_choice_bracket(&visible) {
        diagnostics.push(
            Diagnostic::new(
                "QUALITY.BINARY_CHOICE_CLOZE",
                Severity::Critical,
                OWNER_SKILL,
                "Escolha binária fechada entre colchetes ([A / B]) compromete recuperação ativa; reescreva como evocação aberta.",
            )
            .with_row(row)
            .with_field(field),
        );
    }
}

fn check_allinone_mc_leak(
    text: &str,
    row: usize,
    field: &str,
    check_commands: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let uncloaked = crate::qa::strip_cloze_markers(text);
    let visible = crate::qa::html_to_visible_text(&uncloaked);
    let has_cmd = check_commands && MC_COMMAND_RE.is_match(&visible);
    let has_vec = BINARY_VECTOR_RE.is_match(&visible);
    if has_cmd || has_vec {
        diagnostics.push(
            Diagnostic::new(
                "QUALITY.ALLINONE_MC_LEAK",
                Severity::Critical,
                OWNER_SKILL,
                "Padrão de múltipla escolha importada sem conversão para recuperação ativa (comando MC ou vetor binário de gabarito).",
            )
            .with_row(row)
            .with_field(field),
        );
    }
}

fn count_sentences(visible: &str) -> usize {
    let chars: Vec<char> = visible.trim().chars().collect();
    let n = chars.len();
    let mut count = 0usize;
    let mut has_word_since_last = false;
    let mut i = 0usize;
    while i < n {
        let ch = chars[i];
        if ch.is_alphanumeric() {
            has_word_since_last = true;
            i += 1;
            continue;
        }
        if matches!(ch, '.' | '!' | '?') {
            let prev_digit = i > 0 && chars[i - 1].is_ascii_digit();
            let next_digit = i + 1 < n && chars[i + 1].is_ascii_digit();
            if ch == '.' && prev_digit && next_digit {
                i += 1;
                continue;
            }
            if has_word_since_last {
                count += 1;
                has_word_since_last = false;
            }
            while i + 1 < n && matches!(chars[i + 1], '.' | '!' | '?') {
                i += 1;
            }
        }
        i += 1;
    }
    count
}

fn check_missing_media_tag(tags: &str, row: usize, diagnostics: &mut Vec<Diagnostic>) {
    let has_tag = tags.split_whitespace().any(|tok| {
        let low = tok.to_lowercase();
        low == "mídia-ausente" || low == "midia-ausente"
    });
    if has_tag {
        diagnostics.push(
            Diagnostic::new(
                "QUALITY.MISSING_MEDIA_TAG",
                Severity::Critical,
                OWNER_SKILL,
                "Etiqueta 'mídia-ausente' presente; anexe a imagem clínica correspondente ou remova a dependência visual.",
            )
            .with_row(row)
            .with_field("Etiquetas"),
        );
    }
}

fn check_img_alts(html: &str, row: usize, field: &str, diagnostics: &mut Vec<Diagnostic>) {
    for m in IMG_TAG_RE.find_iter(html) {
        let tag = m.as_str();
        let is_bad_alt = match ALT_VALUE_RE.captures(tag) {
            None => true,
            Some(caps) => {
                let alt_val = caps
                    .get(1)
                    .or_else(|| caps.get(2))
                    .or_else(|| caps.get(3))
                    .map_or("", |g| g.as_str())
                    .trim();
                alt_val.is_empty() || GENERIC_ALT_RE.is_match(alt_val)
            }
        };
        if is_bad_alt {
            diagnostics.push(
                Diagnostic::new(
                    "QUALITY.GENERIC_IMG_ALT",
                    Severity::Warn,
                    OWNER_SKILL,
                    "Tag <img> com atributo alt ausente, vazio ou genérico; forneça descrição clínica específica em alt=\"...\".",
                )
                .with_row(row)
                .with_field(field),
            );
        }
    }
}

fn ul_has_at_least_three_li(sentence: &str) -> bool {
    if !UL_OPEN_RE.is_match(sentence) {
        return false;
    }
    let mut found_closed_ul = false;
    for caps in UL_BLOCK_RE.captures_iter(sentence) {
        found_closed_ul = true;
        let inner = caps.get(1).map_or("", |m| m.as_str());
        if LI_OPEN_RE.find_iter(inner).count() >= 3 {
            return true;
        }
    }
    !found_closed_ul && LI_OPEN_RE.find_iter(sentence).count() >= 3
}

fn normalized_tokens(prompt_visible: &str, target_visible: &str) -> BTreeSet<String> {
    let combined = format!("{prompt_visible} {target_visible}").to_lowercase();
    combined
        .split(|c: char| !c.is_alphanumeric())
        .filter(|tok| tok.chars().count() >= 4)
        .map(str::to_owned)
        .collect()
}

struct NearDupCandidate {
    note_type: &'static str,
    source_row: usize,
    cloze_index: Option<usize>,
    key: (String, String),
    tokens: BTreeSet<String>,
}

#[must_use]
pub fn review_content(qa_csv: &str, cloze_csv: &str) -> Vec<Diagnostic> {
    let review = crate::review_batch_information(qa_csv, cloze_csv);
    let mut seen = BTreeMap::new();
    let mut prompts = BTreeMap::new();
    let mut diagnostics = Vec::new();

    let mut qa_rows: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (row, line) in iter_data_lines(qa_csv) {
        let cols = split_row(line, 7).cols;
        let prompt = cols.first().map_or("", String::as_str);
        let answer = cols.get(1).map_or("", String::as_str);
        let notes = cols.get(3).map_or("", String::as_str);
        let images = cols.get(4).map_or("", String::as_str);
        let refs = cols.get(5).map_or("", String::as_str);
        let tags = cols.get(6).map_or("", String::as_str);

        if !images.is_empty() {
            diagnostics.extend(crate::check_images_field(
                images,
                None,
                Some(row),
                "Imagens Relacionadas",
            ));
        }
        check_reference_field(refs, row, &mut diagnostics);
        check_binary_choice_field(prompt, row, "Enunciado", &mut diagnostics);
        check_binary_choice_field(answer, row, "Resposta", &mut diagnostics);
        check_allinone_mc_leak(prompt, row, "Enunciado", true, &mut diagnostics);
        check_allinone_mc_leak(answer, row, "Resposta", false, &mut diagnostics);

        let vis_prompt = crate::qa::html_to_visible_text(prompt);
        if DUAL_TARGET_INTERROGATIVES_RE.is_match(&vis_prompt)
            || DUAL_TARGET_MECHANISM_RE.is_match(&vis_prompt)
        {
            diagnostics.push(
                Diagnostic::new(
                    "QUALITY.DUAL_TARGET_QA",
                    Severity::Warn,
                    OWNER_SKILL,
                    "Enunciado sugere duas cobranças distintas na mesma pergunta; separe em cartões atômicos.",
                )
                .with_row(row)
                .with_field("Enunciado"),
            );
        }

        let vis_answer = crate::qa::html_to_visible_text(answer);
        let vis_notes = crate::qa::html_to_visible_text(notes);
        if vis_notes.trim().is_empty()
            && (vis_answer.chars().count() > 220 || count_sentences(&vis_answer) >= 3)
        {
            diagnostics.push(
                Diagnostic::new(
                    "QUALITY.ANSWER_EXPLANATION_MIX",
                    Severity::Warn,
                    OWNER_SKILL,
                    "Resposta extensa ou multifrase com Notas Adicionais vazio; mantenha apenas o alvo direto em Resposta e mova a explicação para Notas Adicionais.",
                )
                .with_row(row)
                .with_field("Resposta")
                .with_suggestion("Mova a explicação complementar para Notas Adicionais."),
            );
        }

        check_missing_media_tag(tags, row, &mut diagnostics);
        check_img_alts(prompt, row, "Enunciado", &mut diagnostics);
        check_img_alts(images, row, "Imagens Relacionadas", &mut diagnostics);
        if notes.contains("<img") || notes.contains("<figure") {
            diagnostics.extend(crate::check_images_field(
                notes,
                None,
                Some(row),
                "Notas Adicionais",
            ));
            check_img_alts(notes, row, "Notas Adicionais", &mut diagnostics);
        }

        qa_rows.insert(row, cols);
    }

    let mut cloze_rows: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (row, line) in iter_data_lines(cloze_csv) {
        let cols = split_row(line, 6).cols;
        let sentence = cols.first().map_or("", String::as_str);
        let notes = cols.get(1).map_or("", String::as_str);
        let images = cols.get(3).map_or("", String::as_str);
        let refs = cols.get(4).map_or("", String::as_str);
        let tags = cols.get(5).map_or("", String::as_str);

        if !images.is_empty() {
            diagnostics.extend(crate::check_images_field(
                images,
                None,
                Some(row),
                "Imagens Relacionadas",
            ));
        }
        check_reference_field(refs, row, &mut diagnostics);
        check_binary_choice_field(sentence, row, "Sentença", &mut diagnostics);
        check_allinone_mc_leak(sentence, row, "Sentença", true, &mut diagnostics);

        if ul_has_at_least_three_li(sentence) && count_cloze_indices(sentence).len() == 1 {
            diagnostics.push(
                Diagnostic::new(
                    "QUALITY.LIST_SCAFFOLD_WARN",
                    Severity::Warn,
                    OWNER_SKILL,
                    "Sentença contém lista <ul> com >= 3 itens e apenas 1 índice Cloze; avalie decomposição ou múltiplos cartões independentes.",
                )
                .with_row(row)
                .with_field("Sentença"),
            );
        }

        check_missing_media_tag(tags, row, &mut diagnostics);
        check_img_alts(sentence, row, "Sentença", &mut diagnostics);
        check_img_alts(images, row, "Imagens Relacionadas", &mut diagnostics);
        if notes.contains("<img") || notes.contains("<figure") {
            diagnostics.extend(crate::check_images_field(
                notes,
                None,
                Some(row),
                "Notas Adicionais",
            ));
            check_img_alts(notes, row, "Notas Adicionais", &mut diagnostics);
        }

        cloze_rows.insert(row, cols);
    }

    let mut by_study_field: BTreeMap<String, Vec<NearDupCandidate>> = BTreeMap::new();

    for card in &review.cards {
        let (prompt, study_field) = if card.note_type == "QA" {
            let cols = qa_rows.get(&card.source_row);
            (
                cols.and_then(|c| c.first()).cloned().unwrap_or_default(),
                cols.and_then(|c| c.get(2)).map_or("", String::as_str),
            )
        } else {
            let cols = cloze_rows.get(&card.source_row);
            (
                card.visible_context.clone().unwrap_or_default(),
                cols.and_then(|c| c.get(2)).map_or("", String::as_str),
            )
        };
        let key = (
            normalized_visible(&prompt),
            normalized_visible(&card.retrieved_target),
        );
        if key.0.is_empty() || key.1.is_empty() {
            continue;
        }
        if let Some(first_target) = prompts.get(&key.0) {
            if first_target != &key.1 {
                diagnostics.push(Diagnostic::new(
                    "QUALITY.AMBIGUOUS_PROMPT", Severity::Warn, OWNER_SKILL,
                    "Same normalized visible prompt has different targets; review context, alternatives and potential interference.",
                ).with_row(card.source_row));
            }
        } else {
            prompts.insert(key.0.clone(), key.1.clone());
        }
        if let Some((kind, row, index)) = seen.get(&key) {
            diagnostics.push(Diagnostic::new(
                "QUALITY.DUPLICATE_RETRIEVAL", Severity::Critical, OWNER_SKILL,
                format!("Repeated visible prompt and target: {} row {} index {:?}; first at {kind} row {row} index {index:?}. Review duplicates instead of creating repeated obligations.",card.note_type,card.source_row,card.cloze_index),
            ).with_row(card.source_row));
        } else {
            seen.insert(
                key.clone(),
                (card.note_type, card.source_row, card.cloze_index),
            );
        }

        let norm_field = normalized_visible(study_field);
        if !norm_field.is_empty() {
            let tokens = normalized_tokens(&key.0, &key.1);
            if tokens.len() >= 4 {
                let candidates = by_study_field.entry(norm_field).or_default();
                for prev in candidates.iter() {
                    if prev.key == key {
                        continue;
                    }
                    if prev.note_type == "CL"
                        && card.note_type == "CL"
                        && prev.source_row == card.source_row
                    {
                        continue;
                    }
                    let inter = prev.tokens.intersection(&tokens).count();
                    let union = prev.tokens.union(&tokens).count();
                    if union > 0 && inter * 10 >= union * 7 {
                        diagnostics.push(
                            Diagnostic::new(
                                "QUALITY.NEAR_DUPLICATE_WARN",
                                Severity::Warn,
                                OWNER_SKILL,
                                format!(
                                    "Cartão quase duplicado no mesmo Campo de Estudo ({} row {} index {:?} vs {} row {} index {:?}; Jaccard = {inter}/{union}).",
                                    card.note_type,
                                    card.source_row,
                                    card.cloze_index,
                                    prev.note_type,
                                    prev.source_row,
                                    prev.cloze_index,
                                ),
                            )
                            .with_row(card.source_row),
                        );
                        break;
                    }
                }
                candidates.push(NearDupCandidate {
                    note_type: card.note_type,
                    source_row: card.source_row,
                    cloze_index: card.cloze_index,
                    key,
                    tokens,
                });
            }
        }
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use anki_domain::{CLOZE_HEADER_CANONICAL as CL, QA_HEADER_CANONICAL as QA};

    #[test]
    fn same_prompt_different_targets_requires_interference_review() {
        let csv = format!(
            "{QA}\nEm cenário alfa, qual símbolo?|delta|||||\nEm cenário alfa, qual símbolo?|epsilon|||||"
        );
        assert!(
            review_content(&csv, CL)
                .iter()
                .any(|d| d.code == "QUALITY.AMBIGUOUS_PROMPT")
        );
    }

    #[test]
    fn same_answer_with_different_context_is_not_a_duplicate() {
        let csv = format!(
            "{QA}\nEm cenário alfa, qual símbolo?|delta||||Manual Sintético (2026), Seção 1|\nEm cenário beta, qual símbolo?|delta||||Manual Sintético (2026), Seção 2|"
        );
        assert!(review_content(&csv, CL).is_empty());
    }

    #[test]
    fn images_require_semantic_figures_in_quality_gate() {
        let csv = format!("{QA}\nQual símbolo aparece?|delta|||<img src=\"demo_shape.png\">||");
        assert!(
            review_content(&csv, CL)
                .iter()
                .any(|d| d.code == "IMG.BARE_IMG")
        );
    }

    #[test]
    fn identical_visible_retrievals_detect_duplicates_despite_markup() {
        let csv = format!(
            "{QA}\nEm cenário alfa, qual símbolo?|<b>delta</b>|||||\nEm cenário alfa, qual símbolo?|delta|||||"
        );
        let diagnostics = review_content(&csv, CL);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == "QUALITY.DUPLICATE_RETRIEVAL" && d.row == Some(3))
        );
    }
}
