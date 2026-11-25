//! Deterministic notes field checks (`checks/notes.py`).
//!
//! Checks:
//! - `NOTE.METADATA` — forbidden metadata headers (ID:, Âncora:, Cross-ref:, Relevância:) (CRITICAL)
//! - `NOTE.DUP_ANSWER_WARN` — notes verbatim-repeat the answer without pedagogical explanation (WARN)
//! - `NOTE.EXAM_ONLY_WARN` — notes contain only exam-appearance mention without explanation (WARN)
//! - `NOTE.LENGTH_WARN` — notes field outside recommended length (WARN, configurable)
//! - `NOTE.STYLE_WARN` — LEGACY list formatting only (WARN): bare `•`/`-`/`*` bullets outside `<li>`,
//!   `<li>` without a bold lead-in outside the canonical list blocks emitted by the renderer
//!   (`<section class="anki-list|anki-steps|anki-flow">`), and `<li>` that keeps a literal `•` marker
//!   even inside a canonical section, unless the item itself starts with a bold lead-in.
//!   Canonical renderer output is exempt by design.
//!
//! Escolhas determinísticas do scanner de `<section>` (observáveis, portanto documentadas):
//! - um `<li>` sem `</li>` é fundido ao item seguinte (a regex de item vai da primeira abertura ao primeiro
//!   fechamento), o que pode suprimir um aviso legítimo — a regra falha para o lado leniente;
//! - atributo `class` repetido na mesma tag: a primeira ocorrência vence, como na leitura HTML;
//! - `</section>` dentro de comentário HTML é lido como tag de fechamento (limitação conhecida).

use std::sync::LazyLock;

use anki_domain::{Diagnostic, Severity};
use regex::Regex;

// Re-exports preservam 100% das assinaturas públicas da Fase 1.
pub use crate::domain_profile::{DomainProfile, DomainProfileError};
pub use crate::html_text::{
    html_to_visible_text, normalize_for_exact_match, parse_html_text, strip_table_blocks,
    visible_char_count,
};
pub use crate::note_metrics::{
    NotesStructureMetrics, check_cloze_target_restatement, compute_notes_metrics,
    is_exact_answer_restatement,
};

pub const DEFAULT_MAX_LENGTH: usize = 1200;

const OWNER: &str = "anki-notes-enricher";
const DEFAULT_FIELD: &str = "Notas Adicionais";

/// Rótulo de metadado proibido, ancorado no começo de uma linha visível.
///
/// `[*_]{0,2}` aceita o realce markdown que o autor digita (`**ID:**`, `__Âncora:__`) — sem
/// ele, um campo com `**ID:**` escapava da checagem (Onda 2, B26). O bullet inicial
/// (`• ID:`) é removido antes, por [`crate::text_norm::strip_leading_marker`].
/// Grupo 1 = rótulo, para a mensagem continuar igual à histórica (`'ID:'`).
static METADATA_HEADER_LABEL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^[*_]{0,2}\s*(ID\s*:|Âncora\s*:|Anchor\s*:|Cross-ref\s*:|Cross\s+ref\s*:|Relevância\s*:|Relevancia\s*:)",
    )
    .expect("valid METADATA_HEADER_LABEL_RE")
});

static HAS_TABLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<table\b").expect("valid HAS_TABLE_RE"));

static BULLET_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    // A classe de glifos vem da união única (`text_norm::BULLET_GLYPHS`, Onda 2/B24); `-` e
    // `*` ficam de fora da classe e são exigidos com espaço à direita (`[-*]\s`), como antes —
    // sem isso `**negrito**` e `-item` passariam por bullet.
    let glyphs: String = crate::text_norm::BULLET_GLYPHS
        .iter()
        .copied()
        .filter(|c| crate::text_norm::is_glyph_bullet(*c))
        .collect();
    Regex::new(&format!(
        r"(?i)(?:^|<br\s*/?>|\n|<div[^>]*>|<p[^>]*>)\s*(?:[{glyphs}]|[-*]\s)\s*"
    ))
    .expect("valid BULLET_LINE_RE")
});

static LI_BLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<li\b[^>]*>(.*?)</li>").expect("valid LI_BLOCK_RE"));

static BOLD_OPEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)^(?:<(?:b|strong)\b|<span\b[^>]*\bclass\s*=\s*["'][^"']*\brot\b[^"']*["'])"#)
        .expect("valid BOLD_OPEN_RE")
});

/// Classes de `<section>` que o renderizador emite para listas canônicas
/// (`note_render::render_list` e `validated_flow`).
const CANONICAL_LIST_SECTIONS: [&str; 3] = ["anki-list", "anki-steps", "anki-flow"];

/// Prefixos legados removidos antes de exigir `<b>` no item: os glifos de bullet da união
/// única ([`crate::text_norm::BULLET_GLYPHS`], Onda 2/B24) mais `-`, `*` e espaço.
/// Não são defeito por si sós: um item "- Texto" dentro de `<section>` canônica é
/// saída legítima do compilador quando o autor digita o prefixo.
fn is_legacy_item_prefix(c: char) -> bool {
    c == ' ' || crate::text_norm::BULLET_GLYPHS.contains(&c)
}

/// Tags `<section>` de abertura/fechamento; grupo 1 = `/` em tags de fechamento, grupo 2 = atributos.
static SECTION_TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<\s*(/?)\s*section\b([^>]*)>").expect("valid SECTION_TAG_RE")
});

#[must_use]
pub fn is_exam_only_note(notes: &str) -> bool {
    is_exam_only_note_with_profile(notes, DomainProfile::default_profile())
}

#[must_use]
pub fn is_exam_only_note_with_profile(notes: &str, profile: &DomainProfile) -> bool {
    profile.is_exam_only_note(notes)
}

#[must_use]
pub fn check_notes(notes: &str, answer: &str, row: Option<usize>) -> Vec<Diagnostic> {
    check_notes_ext(notes, answer, row, DEFAULT_FIELD, DEFAULT_MAX_LENGTH)
}

#[must_use]
pub fn check_notes_ext(
    notes: &str,
    answer: &str,
    row: Option<usize>,
    field: &str,
    max_length: usize,
) -> Vec<Diagnostic> {
    check_notes_with_profile(
        notes,
        answer,
        row,
        field,
        Some(max_length),
        DomainProfile::default_profile(),
    )
}

#[must_use]
pub fn check_notes_with_profile(
    notes: &str,
    answer: &str,
    row: Option<usize>,
    field: &str,
    max_length: Option<usize>,
    profile: &DomainProfile,
) -> Vec<Diagnostic> {
    if notes.trim().is_empty() {
        return Vec::new();
    }

    let mut diags = Vec::new();
    // Preserva quebras de linha (\n) para que (?im)^ capture cabeçalhos após <br>/<p>/<li>.
    let (block_text, _, _) = parse_html_text(notes);

    // 1. NOTE.METADATA (respeita InfoPolicy::forbid_metadata_headers)
    //
    // Varre linha a linha para poder remover o bullet inicial antes do rótulo
    // (Onda 2, B26): `• ID:` e `**ID:**` são o mesmo metadado que `ID:`.
    if profile.info_policy.forbid_metadata_headers {
        for line in block_text.lines() {
            let (rest, _) = crate::text_norm::strip_leading_marker(line);
            let Some(caps) = METADATA_HEADER_LABEL_RE.captures(rest) else {
                continue;
            };
            let label = caps.get(1).map_or("", |m| m.as_str()).trim();
            diags.push(build_diagnostic(
                "NOTE.METADATA",
                Severity::Critical,
                format!("Forbidden metadata header found: '{label}'"),
                row,
                field,
                Some("Remove metadata — Notes field is for pedagogical explanations only"),
            ));
        }
    }

    // 2. NOTE.DUP_ANSWER_WARN (respeita InfoPolicy::forbid_verbatim_answer_restatement)
    if profile.info_policy.forbid_verbatim_answer_restatement
        && !answer.trim().is_empty()
        && is_exact_answer_restatement(notes, answer)
    {
        diags.push(build_diagnostic(
            "NOTE.DUP_ANSWER_WARN",
            Severity::Warn,
            "Notes repeat the answer verbatim — deepen the explanation instead",
            row,
            field,
            Some("Explain *why* the answer is correct or add related nuance from the source"),
        ));
    }

    // 3. NOTE.EXAM_ONLY_WARN (respeita InfoPolicy::forbid_exam_only_mentions)
    if profile.info_policy.forbid_exam_only_mentions
        && is_exam_only_note_with_profile(notes, profile)
    {
        diags.push(build_diagnostic(
            "NOTE.EXAM_ONLY_WARN",
            Severity::Warn,
            "Notes contain only an exam mention without pedagogical explanation",
            row,
            field,
            Some("Menção isolada a prova não explica a unidade; acrescente explicação útil da fonte ou deixe Notas Adicionais vazio."),
        ));
    }

    // 4. NOTE.LENGTH_WARN
    let base_max = max_length.unwrap_or(profile.default_max_note_chars);
    let has_table = HAS_TABLE_RE.is_match(notes);
    let effective_max = if has_table {
        base_max.max(profile.table_max_note_chars)
    } else {
        base_max
    };
    let visible_length = visible_char_count(notes);
    if visible_length > effective_max {
        diags.push(build_diagnostic(
            "NOTE.LENGTH_WARN",
            Severity::Warn,
            format!(
                "Notes field is {visible_length} visible chars (recommended ≤ {effective_max}; advisory only)"
            ),
            row,
            field,
            None,
        ));
    }

    // 5. NOTE.STYLE_WARN
    if has_unbolded_list_item(notes) {
        let pref = &profile.info_policy.preferred_list_format;
        diags.push(build_diagnostic(
            "NOTE.STYLE_WARN",
            Severity::Warn,
            format!("Legacy list formatting found — preferred: '{pref}'"),
            row,
            field,
            None,
        ));
    }

    diags
}

fn has_unbolded_list_item(notes: &str) -> bool {
    let non_table = strip_table_blocks(notes);

    // Regra legada: bullets em texto puro (`•`, `-`, `*`) fora de `<li>`.
    for m in BULLET_LINE_RE.find_iter(&non_table) {
        let rest = non_table[m.end()..].trim_start();
        if !rest.is_empty() && !BOLD_OPEN_RE.is_match(rest) {
            return true;
        }
    }

    let canonical = canonical_list_ranges(&non_table);

    for cap in LI_BLOCK_RE.captures_iter(&non_table) {
        let Some(item) = cap.get(0) else {
            continue;
        };
        let inner = cap[1].trim_start();
        let text = inner.trim_start_matches(is_legacy_item_prefix);
        // Item emoldurado por `<b>`/`<strong>`/`<span class="rot">` é o formato
        // recomendado, com ou sem prefixo legado na frente.
        if BOLD_OPEN_RE.is_match(text) {
            continue;
        }
        // Glifo de bullet literal no prefixo do item é defeito visual mesmo em lista
        // canônica (`<li>• item</li>` duplicaria o marcador nativo), inclusive quando o
        // item é só o glifo ou combina prefixos (`<li>- • item</li>`). Prefixos `-`/`*`
        // não são defeito por si sós: o compilador os preserva quando o autor os digita.
        let prefix = &inner[..inner.len() - text.len()];
        let has_bullet_glyph = prefix.chars().any(crate::text_norm::is_glyph_bullet);
        if has_bullet_glyph {
            return true;
        }
        if text.is_empty() {
            continue;
        }
        // Qualquer outro item sem negrito só é punido fora das `<section>` canônicas
        // emitidas pelo renderizador.
        if !is_inside(&canonical, item.start()) {
            return true;
        }
    }

    false
}

/// Valor do atributo `class` de uma tag de abertura, se presente.
///
/// Percorre os atributos de verdade (nome, `=`, valor entre aspas ou sem aspas) em
/// vez de procurar `class=` por regex: um `class=anki-list` dentro do valor de outro
/// atributo (`data-x=" class=anki-list"`) não é o atributo `class` e não deve isentar
/// a lista. Nomes de atributo são comparados sem diferenciar maiúsculas.
fn class_attribute_value(attrs: &str) -> Option<&str> {
    let bytes = attrs.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        let name_start = i;
        while i < bytes.len() && !is_attribute_delimiter(bytes[i]) {
            i += 1;
        }
        let name = &attrs[name_start..i];

        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }

        let mut value = None;
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i];
                i += 1;
                let value_start = i;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                value = Some(&attrs[value_start..i]);
                if i < bytes.len() {
                    i += 1;
                }
            } else {
                let value_start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                    i += 1;
                }
                if i > value_start {
                    value = Some(&attrs[value_start..i]);
                }
            }
        }

        if name.eq_ignore_ascii_case("class") {
            return value;
        }
        if i == name_start {
            // Sem progresso (atributo vazio): avança para não repetir o laço.
            i += 1;
        }
    }

    None
}

/// `true` quando o byte termina o nome de um atributo.
fn is_attribute_delimiter(byte: u8) -> bool {
    byte.is_ascii_whitespace() || byte == b'=' || byte == b'>'
}

/// `true` quando os atributos de uma `<section>` de abertura declaram uma classe de lista canônica.
fn is_canonical_list_section(attrs: &str) -> bool {
    let Some(classes) = class_attribute_value(attrs) else {
        return false;
    };
    classes.split_whitespace().any(|token| {
        CANONICAL_LIST_SECTIONS
            .iter()
            .any(|class| token.eq_ignore_ascii_case(class))
    })
}

/// Intervalos `[início, fim)` de `<section>` canônicas de lista, resolvidos por pilha.
///
/// A pilha casa aberturas e fechamentos de `<section>` na ordem do documento, então
/// seções aninhadas (`anki-note-section` contendo `anki-list`) são resolvidas
/// corretamente, sem depender do "primeiro `</section>`" após a abertura.
///
/// Escolhas para entradas não balanceadas (determinísticas e conservadoras):
/// - `</section>` sem abertura correspondente é ignorado;
/// - abertura canônica sem fechamento é tratada como canônica até o fim do HTML
///   (o renderizador nunca emite isso; assumimos o formato canônico para não punir saída própria).
fn canonical_list_ranges(html: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut stack: Vec<(usize, bool)> = Vec::new();

    for caps in SECTION_TAG_RE.captures_iter(html) {
        let Some(tag) = caps.get(0) else {
            continue;
        };
        let closing = caps.get(1).is_some_and(|m| !m.as_str().is_empty());
        if closing {
            if let Some((start, canonical)) = stack.pop()
                && canonical
            {
                ranges.push((start, tag.end()));
            }
        } else {
            let attrs = caps.get(2).map_or("", |m| m.as_str());
            stack.push((tag.start(), is_canonical_list_section(attrs)));
        }
    }

    for (start, canonical) in stack {
        if canonical {
            ranges.push((start, html.len()));
        }
    }

    // Ordena e funde intervalos sobrepostos (possível em entrada malformada, ex.:
    // `<section class="anki-list">` aberta dentro de outra e nunca fechada): a busca
    // de contenção passa a ser um único binary search, sem custo quadrático em notas
    // grandes.
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// `true` quando `position` cai dentro de algum intervalo `[início, fim)`.
///
/// `ranges` vem de `canonical_list_ranges`, já ordenado e sem sobreposição, então
/// basta o último intervalo que começa até `position`.
fn is_inside(ranges: &[(usize, usize)], position: usize) -> bool {
    let index = ranges.partition_point(|&(start, _)| start <= position);
    index > 0 && position < ranges[index - 1].1
}

fn build_diagnostic(
    code: &'static str,
    severity: Severity,
    message: impl Into<String>,
    row: Option<usize>,
    field: &str,
    suggestion: Option<&str>,
) -> Diagnostic {
    let mut diag = Diagnostic::new(
        anki_domain::DiagnosticCode::from_static(code),
        severity,
        OWNER,
        message,
    )
    .with_field(field);
    if let Some(r) = row {
        diag = diag.with_row(r);
    }
    if let Some(s) = suggestion {
        diag = diag.with_suggestion(s);
    }
    diag
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_and_short_notes() {
        assert!(check_notes("", "", None).is_empty());
        assert!(check_notes("   ", "", None).is_empty());
        assert!(check_notes("Meia-vida longa.", "", None).is_empty());
        assert!(check_notes("Via renal.", "", None).is_empty());
    }

    #[test]
    fn test_forbidden_metadata_headers() {
        let d1 = check_notes("ID: B01-QA-001<br>Explicação detalhada.", "", Some(1));
        assert!(
            d1.iter()
                .any(|d| d.code == "NOTE.METADATA" && d.severity == Severity::Critical)
        );

        let d2 = check_notes("Âncora: p. 83<br>Texto explicativo.", "", None);
        assert!(d2.iter().any(|d| d.code == "NOTE.METADATA"));

        let d3 = check_notes("Cross-ref: B01-QA-002", "", None);
        assert!(d3.iter().any(|d| d.code == "NOTE.METADATA"));

        let d4 = check_notes("Relevância: Alta para provas", "", None);
        assert!(d4.iter().any(|d| d.code == "NOTE.METADATA"));
    }

    /// B26: o realce markdown e o bullet inicial não podem esconder o metadado.
    #[test]
    fn test_metadata_headers_survive_markdown_and_bullet_prefixes() {
        for notes in [
            "**ID:** B01-QA-001<br>Explicação detalhada.",
            "__ID:__ B01-QA-001<br>Explicação detalhada.",
            "• ID: B01-QA-001<br>Explicação detalhada.",
            "• **ID:** B01-QA-001<br>Explicação detalhada.",
            "▪ Relevância: alta<br>Texto explicativo.",
            "* Âncora: p. 83<br>Texto explicativo.",
        ] {
            let diags = check_notes(notes, "", Some(1));
            assert!(
                diags
                    .iter()
                    .any(|d| d.code == "NOTE.METADATA" && d.severity == Severity::Critical),
                "metadado disfarçado escapou da checagem: {notes}"
            );
        }

        // A mensagem continua a histórica: só o rótulo, sem os asteriscos nem o bullet.
        let diags = check_notes("• **ID:** B01-QA-001", "", Some(1));
        let metadata = diags
            .iter()
            .find(|d| d.code == "NOTE.METADATA")
            .expect("NOTE.METADATA esperado");
        assert_eq!(metadata.message, "Forbidden metadata header found: 'ID:'");

        // Prosa legítima com realce no meio da linha não vira metadado.
        for prose in [
            "A **ID** do paciente não entra no campo de notas.",
            "Ver **Relevância** clínica no rodapé da diretriz.",
            "Identificação de critérios maiores de Jones.",
        ] {
            let diags = check_notes(prose, "", None);
            assert!(
                !diags.iter().any(|d| d.code == "NOTE.METADATA"),
                "falso positivo de NOTE.METADATA em: {prose}"
            );
        }
    }

    #[test]
    fn test_duplicate_answer_warn() {
        let d1 = check_notes("<b>Riluzol.</b>", "riluzol", None);
        assert!(
            d1.iter()
                .any(|d| d.code == "NOTE.DUP_ANSWER_WARN" && d.severity == Severity::Warn)
        );

        let d2 = check_notes("<p>&ge;&nbsp;140 mmHg!</p>", "<b>&ge; 140 mmHg</b>", None);
        assert!(d2.iter().any(|d| d.code == "NOTE.DUP_ANSWER_WARN"));

        let d3 = check_notes("A resposta correta é: IECA.", "<i>IECA</i>", None);
        assert!(d3.iter().any(|d| d.code == "NOTE.DUP_ANSWER_WARN"));

        // Does not warn when pedagogical explanation is present
        let d4 = check_notes(
            "<p><b>Riluzol</b> reduz a liberação de glutamato e prolonga moderadamente a sobrevida.</p>",
            "Riluzol",
            None,
        );
        assert!(!d4.iter().any(|d| d.code == "NOTE.DUP_ANSWER_WARN"));
    }

    #[test]
    fn test_exam_only_notes() {
        let cases = [
            "Já caiu em prova.",
            "Caiu na prova de 2024.",
            "<p>• <b>Prova:</b> Muito cobrado em provas de residência.</p>",
            "Caiu na prova da USP de 2024.",
            "Prova do ENARE 2024.",
        ];
        for txt in cases {
            assert!(is_exam_only_note(txt), "Should be exam-only: {txt}");
            let diags = check_notes(txt, "", None);
            assert!(
                diags
                    .iter()
                    .any(|d| d.code == "NOTE.EXAM_ONLY_WARN" && d.severity == Severity::Warn),
                "Missing NOTE.EXAM_ONLY_WARN on: {txt}"
            );
        }

        let non_exam_cases = [
            "Caiu na prova de 2024. A meia-vida longa explica o platô tardio.",
            "Tema cobrado em prova: segue cinética de ordem zero em doses terapêuticas.",
            "Prova calórica ausente indica arreflexia vestibular.",
            "Caiu na prova de 2024 por distinguir o grupo alfa do beta",
            "Caiu na prova de 2024 de mecanismo ainda desconhecido",
            "Caiu na prova de 2024 para distinguir o grupo alfa do beta",
            "Muito cobrado em provas na diferenciação entre crise focal e síncope",
            "Já caiu em prova em pacientes com hepatopatia crônica para evitar toxicidade",
        ];
        for txt in non_exam_cases {
            assert!(!is_exam_only_note(txt), "Should NOT be exam-only: {txt}");
            let diags = check_notes(txt, "", None);
            assert!(
                !diags.iter().any(|d| d.code == "NOTE.EXAM_ONLY_WARN"),
                "False positive NOTE.EXAM_ONLY_WARN on: {txt}"
            );
        }
    }

    #[test]
    fn test_length_warn() {
        let base = "Explicação clínica. ".repeat(60); // ~1200 chars
        let d_ok = check_notes(&base, "", None);
        assert!(!d_ok.iter().any(|d| d.code == "NOTE.LENGTH_WARN"));

        let huge = "Explicação clínica. ".repeat(300);
        let d_huge = check_notes(&huge, "", None);
        assert!(
            d_huge
                .iter()
                .any(|d| d.code == "NOTE.LENGTH_WARN" && d.severity == Severity::Warn)
        );
        assert!(!d_huge.iter().any(|d| d.severity == Severity::Critical));

        // Table with ~2000 chars: > 1200, but <= 2500 for tables -> no warning
        let table_ok = format!(
            "<table><tr><td>{}</td></tr></table>",
            "Explicação clínica. ".repeat(100)
        );
        let d_table_ok = check_notes(&table_ok, "", None);
        assert!(!d_table_ok.iter().any(|d| d.code == "NOTE.LENGTH_WARN"));

        // Table with ~3000 chars: > 2500 -> warning emitted (WARN, not CRITICAL)
        let table_huge = format!(
            "<table><tr><td>{}</td></tr></table>",
            "Explicação clínica. ".repeat(150)
        );
        let d_table_huge = check_notes(&table_huge, "", None);
        assert!(
            d_table_huge
                .iter()
                .any(|d| d.code == "NOTE.LENGTH_WARN" && d.severity == Severity::Warn)
        );
        assert!(
            !d_table_huge
                .iter()
                .any(|d| d.severity == Severity::Critical)
        );
    }

    #[test]
    fn test_style_warn() {
        let valid_style = "• <b>Avaliação prática:</b> Olhos abertos não garantem consciência.<br>• <b>Fisiopatologia:</b> Descarga paroxística.";
        let d_valid = check_notes(valid_style, "", None);
        assert!(!d_valid.iter().any(|d| d.code == "NOTE.STYLE_WARN"));

        let invalid_style = "• Avaliação prática: Olhos abertos não garantem consciência.";
        let d_invalid = check_notes(invalid_style, "", None);
        assert!(
            d_invalid
                .iter()
                .any(|d| d.code == "NOTE.STYLE_WARN" && d.severity == Severity::Warn)
        );

        let table_style = "<table><tr><td>• Item sem negrito</td></tr></table>";
        let d_table = check_notes(table_style, "", None);
        assert!(!d_table.iter().any(|d| d.code == "NOTE.STYLE_WARN"));
    }

    fn has_style_warn(notes: &str) -> bool {
        check_notes(notes, "", None)
            .iter()
            .any(|d| d.code == "NOTE.STYLE_WARN")
    }

    #[test]
    fn test_style_warn_exempts_canonical_list_sections() {
        // Saída canônica do renderizador (note_render::render_list / validated_flow): itens em
        // texto puro são o formato esperado, não formatação legada.
        let exempt = [
            r#"<section class="anki-list"><p><b>T</b></p><ul><li>x</li></ul></section>"#,
            r#"<section class="anki-steps"><ol><li>Etapa</li></ol></section>"#,
            r#"<section class="anki-flow"><ul><li>Nó</li><li>Nó<ul><li>filho</li></ul></li></ul></section>"#,
            r#"<details class="anki-detail"><summary><b>Ver</b></summary><section class="anki-list"><ul><li>x</li></ul></section></details>"#,
            // marcador literal seguido de negrito continua aceito (paridade com o oráculo Python)
            r#"<section class="anki-list"><ul><li>• <b>X:</b> y</li></ul></section>"#,
            // section aninhada: só a interna é canônica
            r#"<section class="anki-note-section"><p><b>T</b></p><section class="anki-list"><ul><li>x</li></ul></section></section>"#,
            // section canônica não fechada: tratada como canônica até o fim do HTML
            r#"<section class="anki-list"><ul><li>y</li></ul>"#,
            // lista dentro de <table> continua isenta (comportamento preservado)
            "<table><tr><td><ul><li>Item na tabela sem negrito</li></ul></td></tr></table>",
        ];
        for case in exempt {
            assert!(
                !has_style_warn(case),
                "canonical/table list must NOT trigger NOTE.STYLE_WARN: {case}"
            );
        }
    }

    #[test]
    fn test_style_warn_still_flags_legacy_lists() {
        let flagged = [
            "• texto",
            "- texto",
            "* texto",
            "<ul><li>texto</li></ul>",
            "<ol><li>texto</li></ol>",
            // marcador literal dentro de `<li>` é defeito visual mesmo em section canônica
            r#"<section class="anki-list"><ul><li>• item</li></ul></section>"#,
            // section não-lista não isenta
            r#"<section class="anki-kv-section"><p><b>T</b></p><ul><li>x</li></ul></section>"#,
            // abertura canônica já fechada: itens seguintes voltam a ser punidos
            r#"<section class="anki-list"><ul><li>x</li></ul></section><ul><li>y</li></ul>"#,
        ];
        for case in flagged {
            assert!(
                has_style_warn(case),
                "legacy list formatting must still trigger NOTE.STYLE_WARN: {case}"
            );
        }
    }

    #[test]
    fn test_canonical_list_ranges_stack_nesting() {
        // aninhamento: a canônica interna não "vaza" para fora dela
        let html = r#"<section class="anki-list"><ul><li>a</li></ul></section><ul><li>b</li></ul>"#;
        let ranges = canonical_list_ranges(html);
        assert_eq!(ranges.len(), 1);
        assert!(is_inside(&ranges, html.find("<li>a").expect("li a")));
        assert!(!is_inside(&ranges, html.rfind("<li>b").expect("li b")));

        // abertura não balanceada: canônica até o fim do HTML
        let unclosed = r#"<section class="anki-list"><ul><li>a</li></ul>"#;
        let ranges_unclosed = canonical_list_ranges(unclosed);
        assert_eq!(ranges_unclosed, vec![(0, unclosed.len())]);

        // `</section>` órfão é ignorado (não abre nem fecha nada)
        let orphan = r#"<section class="anki-list"></section></section><ul><li>a</li></ul>"#;
        let ranges_orphan = canonical_list_ranges(orphan);
        let close_pos = orphan
            .find("</section>")
            .map(|p| p + "</section>".len())
            .expect("first close tag");
        assert_eq!(ranges_orphan, vec![(0, close_pos)]);
    }

    #[test]
    fn test_canonical_section_class_detection() {
        assert!(is_canonical_list_section(r#" class="anki-list""#));
        assert!(is_canonical_list_section(" class='anki-steps'"));
        assert!(is_canonical_list_section(" class=anki-flow"));
        assert!(is_canonical_list_section(r#" class="anki-note anki-list""#));
        assert!(is_canonical_list_section(r#" class="ANKI-LIST""#));
        assert!(!is_canonical_list_section(r#" class="anki-kv-section""#));
        assert!(!is_canonical_list_section(r#" class="anki-list-ish""#));
        assert!(!is_canonical_list_section(r#" data-class="anki-list""#));
        assert!(!is_canonical_list_section(""));
    }

    #[test]
    fn test_legacy_prefix_inside_canonical_item_is_not_a_defect() {
        // O compilador preserva prefixos digitados pelo autor (`- Item`, `* Item`)
        // dentro de <li>; isso não duplica marcador e não deve ser punido.
        for item in ["- Sintoma hipotético", "* p &lt; 0,05", "  - Recuo"] {
            let notes = format!("<section class=\"anki-list\"><ul><li>{item}</li></ul></section>");
            assert!(
                !has_style_warn(&notes),
                "prefixo legado em item canônico não é defeito: {notes}"
            );
        }

        // Glifo de bullet literal continua sendo defeito visual (marcador duplicado),
        // mesmo dentro de <section> canônica...
        let bullet_in_section = "<section class=\"anki-list\"><ul><li>• Item</li></ul></section>";
        assert!(
            has_style_warn(bullet_in_section),
            "bullet literal duplica o marcador nativo: {bullet_in_section}"
        );
        // ...mas é aceito quando emoldurado por <b>.
        let bullet_bold =
            "<section class=\"anki-list\"><ul><li>• <b>Item:</b> explicação</li></ul></section>";
        assert!(!has_style_warn(bullet_bold));
    }

    #[test]
    fn test_class_attribute_lookalike_inside_other_attribute_does_not_exempt() {
        // `class=anki-list` dentro do valor de outro atributo não é o atributo class.
        let disguised = "<section data-x=\" class=anki-list\"><ul><li>Item</li></ul></section>";
        assert!(
            !is_canonical_list_section(" data-x=\" class=anki-list\""),
            "valor de outro atributo não isenta"
        );
        assert!(
            has_style_warn(disguised),
            "lista não canônica deve continuar punida: {disguised}"
        );

        // Aspas simples, sem aspas e com espaços continuam reconhecidas.
        assert!(is_canonical_list_section(
            " class = \"anki-list\" data-y=\"1\""
        ));
        assert!(is_canonical_list_section(" id=\"s1\" class=anki-list"));
        assert!(is_canonical_list_section(" class=\"x anki-steps y\""));
    }

    #[test]
    fn test_canonical_ranges_merge_overlaps_and_stay_sorted() {
        // Seções sobrepostas (entrada malformada) são fundidas: um único intervalo
        // ordenado, consultável por binary search sem custo quadrático.
        let html = "<section class=\"anki-list\"><section class=\"anki-steps\">\
                    <ol><li>a</li></ol></section><ul><li>b</li></ul></section>";
        let ranges = canonical_list_ranges(html);
        assert_eq!(ranges.len(), 1, "ranges fundidos: {ranges:?}");
        assert!(is_inside(&ranges, html.find("<li>a").expect("li")));
        assert!(is_inside(&ranges, html.find("<li>b").expect("li")));
        assert!(!is_inside(&ranges, html.len()));

        // Intervalos disjuntos permanecem separados e ordenados.
        let two = "<section class=\"anki-list\"><ul><li>x</li></ul></section>\
                   <p>texto</p>\
                   <section class=\"anki-steps\"><ol><li>y</li></ol></section>";
        let ranges_two = canonical_list_ranges(two);
        assert_eq!(ranges_two.len(), 2);
        assert!(ranges_two.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn test_glyph_only_and_combined_prefix_items_are_punished() {
        // Item que é só o glifo e item com prefixos combinados também duplicam o
        // marcador nativo e devem ser punidos mesmo em <section> canônica.
        for item in ["•", "- • Item", "* ▪ Item", "  •   Item"] {
            let notes = format!("<section class=\"anki-list\"><ul><li>{item}</li></ul></section>");
            assert!(
                has_style_warn(&notes),
                "glifo literal no prefixo é defeito: {notes}"
            );
        }

        // Item vazio ou com espaço não é defeito.
        assert!(!has_style_warn(
            "<section class=\"anki-list\"><ul><li> </li></ul></section>"
        ));
    }

    #[test]
    fn test_golden_notes_cases() {
        let good_note = "A atualização da ILAE aboliu os termos obsoletos 'parcial simples' e 'parcial complexa'.<br>• <b>Avaliação prática:</b> A manutenção de olhos abertos não garante consciência.<br>• <b>Fisiopatologia:</b> O comprometimento da consciência ocorre quando a descarga paroxística ultrapassa a zona cortical focal.";
        let d_good = check_notes(good_note, "Consciência preservada vs. comprometida", None);
        assert!(d_good.is_empty() || !d_good.iter().any(|d| d.severity == Severity::Critical));

        let bad_metadata = "ID: B01-QA-001<br>Âncora: p. 12<br>Explicação: ...";
        let d_meta = check_notes(bad_metadata, "", None);
        assert!(
            d_meta
                .iter()
                .any(|d| d.code == "NOTE.METADATA" && d.severity == Severity::Critical)
        );

        let good_mention = "Cefaleia intensa em salvas com rinorreia ipsilateral. A resposta correta é cefaleia em salvas.";
        let d_mention = check_notes(good_mention, "cefaleia em salvas", None);
        assert!(!d_mention.iter().any(|d| d.code == "NOTE.DUP_ANSWER_WARN"));

        let bad_verbatim = "A resposta correta é cefaleia em salvas.";
        let d_verbatim = check_notes(bad_verbatim, "cefaleia em salvas", None);
        assert!(
            d_verbatim
                .iter()
                .any(|d| d.code == "NOTE.DUP_ANSWER_WARN" && d.severity == Severity::Warn)
        );
    }

    #[test]
    fn test_policy_flags_honored() {
        let custom_json = r#"{
            "id": "lenient",
            "name": "Lenient Profile",
            "description": "desc",
            "language": "pt",
            "default_max_note_chars": 1200,
            "table_max_note_chars": 2500,
            "exam_boilerplate": { "enabled": true, "known_labels": ["USP"], "phrase_patterns": ["(?:USP)"] },
            "info_policy": {
                "source_fidelity": "ok",
                "notes_purpose": "ok",
                "forbid_metadata_headers": false,
                "forbid_verbatim_answer_restatement": false,
                "forbid_exam_only_mentions": false,
                "preferred_list_format": "• <b>Tópico:</b> ..."
            }
        }"#;

        let profile = DomainProfile::from_json_str(custom_json).expect("valid profile");

        // Metadata headers ignored under lenient policy
        let d_meta =
            check_notes_with_profile("ID: CARD-1", "Ans", None, DEFAULT_FIELD, None, &profile);
        assert!(!d_meta.iter().any(|d| d.code == "NOTE.METADATA"));

        // Verbatim restatement ignored under lenient policy
        let d_dup = check_notes_with_profile("Ans", "Ans", None, DEFAULT_FIELD, None, &profile);
        assert!(!d_dup.iter().any(|d| d.code == "NOTE.DUP_ANSWER_WARN"));

        // Exam only ignored under lenient policy
        let d_exam = check_notes_with_profile("USP", "Ans", None, DEFAULT_FIELD, None, &profile);
        assert!(!d_exam.iter().any(|d| d.code == "NOTE.EXAM_ONLY_WARN"));
    }

    #[test]
    fn test_general_profile_ignores_exam_mentions() {
        let general = DomainProfile::general();
        assert!(!is_exam_only_note_with_profile(
            "Cobrado na prova da USP 2023.",
            general
        ));
        let diags = check_notes_with_profile(
            "Cobrado na prova da USP 2023.",
            "Ans",
            None,
            DEFAULT_FIELD,
            None,
            general,
        );
        assert!(!diags.iter().any(|d| d.code == "NOTE.EXAM_ONLY_WARN"));
    }
}
