use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::media_figure::render_media_figure;
use crate::note_model::{
    CalloutKind, KeyValueItem, NoteBlock, NoteDocument, NoteModelError, NoteSource,
};
use crate::table_render::TableSpec;
use crate::validated_flow::render_validated_flow;

const KNOWN_ENTITIES: &[&str] = &[
    "amp;", "lt;", "gt;", "le;", "ge;", "#124;", "quot;", "#x27;", "#39;", "nbsp;",
];

const ALLOWED_INLINE_TAGS: &[&str] = &[
    "b", "/b", "i", "/i", "em", "/em", "strong", "/strong", "sub", "/sub", "sup", "/sup", "code",
    "/code", "u", "/u", "s", "/s", "small", "/small", "mark", "/mark",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedNotes {
    pub html: String,
    pub source_refs: Vec<String>,
    pub table_count: usize,
    pub figure_count: usize,
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error(transparent)]
    InvalidModel(#[from] NoteModelError),
    #[error("failed to render table: {0}")]
    TableRender(String),
}

/// Renderiza um [`NoteDocument`] validado em HTML5 semântico seguro, em passagem única,
/// linha única, sem `<br>` entre blocos e com `|` escapado como `&#124;`.
pub fn render_document(doc: &NoteDocument) -> Result<RenderedNotes, RenderError> {
    doc.validate()?;

    if doc.blocos.is_empty() && doc.fontes.is_empty() {
        return Ok(RenderedNotes {
            html: String::new(),
            source_refs: Vec::new(),
            table_count: 0,
            figure_count: 0,
        });
    }

    let mut html = String::new();
    let mut table_count = 0usize;
    let mut figure_count = 0usize;

    for bloco in &doc.blocos {
        let rendered_block = match bloco {
            NoteBlock::Paragrafo {
                titulo,
                texto,
                colapsavel,
            } => render_paragraph(titulo.as_deref(), texto, colapsavel.as_deref()),
            NoteBlock::ChaveValor {
                titulo,
                pares,
                colapsavel,
            } => render_key_value(titulo.as_deref(), pares, colapsavel.as_deref()),
            NoteBlock::Callout {
                variante,
                rotulo,
                texto,
            } => render_callout(*variante, rotulo.as_deref(), texto),
            NoteBlock::Lista {
                titulo,
                ordenada,
                itens,
                colapsavel,
            } => render_list(titulo.as_deref(), *ordenada, itens, colapsavel.as_deref()),
            NoteBlock::Tabela { spec, colapsavel } => {
                let table_html = render_table(spec, colapsavel.as_deref())?;
                table_count += 1;
                table_html
            }
            NoteBlock::Fluxograma {
                titulo,
                fluxo,
                colapsavel,
            } => {
                let raw = render_validated_flow(fluxo, titulo.as_deref());
                let sanitized = sanitize_rendered_fragment(&raw);
                wrap_collapsible(sanitized, colapsavel.as_deref())
            }
            NoteBlock::Figura { figura, colapsavel } => {
                let raw = render_media_figure(figura);
                let sanitized = sanitize_rendered_fragment(&raw);
                figure_count += 1;
                wrap_collapsible(sanitized, colapsavel.as_deref())
            }
            NoteBlock::LegacyHtml { html: raw_fragment } => {
                let sanitized = sanitize_rendered_fragment(raw_fragment);
                let found_tables = count_table_open_tags(&sanitized);
                if found_tables > 0 {
                    table_count += found_tables;
                    ensure_anki_table_class(&sanitized)
                } else {
                    sanitized
                }
            }
        };

        html.push_str(&rendered_block);
    }

    let source_refs = collect_unique_sources(&doc.fontes);
    if !source_refs.is_empty() {
        let refs_joined = source_refs
            .iter()
            .map(|r| escape_html_pipe(r))
            .collect::<Vec<_>>()
            .join("; ");
        html.push_str("<footer class=\"anki-sources\"><small><b>Fonte:</b> <cite>");
        html.push_str(&refs_joined);
        html.push_str("</cite></small></footer>");
    }

    Ok(RenderedNotes {
        html,
        source_refs,
        table_count,
        figure_count,
    })
}

/// Escapa caracteres especiais para CSV/HTML (`|` para `&#124;`, newlines para espaços,
/// bare `&` para `&amp;`), preservando entidades conhecidas e tags HTML inline seguras
/// sem permitir execução de conteúdo ativo.
#[must_use]
pub fn escape_html_pipe(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    // 1. Escapar bare `&`, preservando entidades conhecidas
    let mut amp_escaped = String::with_capacity(text.len());
    let mut last = 0;
    for (idx, _) in text.match_indices('&') {
        amp_escaped.push_str(&text[last..idx]);
        let rest = &text[idx + 1..];
        if KNOWN_ENTITIES.iter().any(|ent| rest.starts_with(ent)) {
            amp_escaped.push('&');
        } else {
            amp_escaped.push_str("&amp;");
        }
        last = idx + 1;
    }
    amp_escaped.push_str(&text[last..]);

    // 2. Normalizar newlines e pipes e escapar tags seguras vs inseguras
    let mut out = String::with_capacity(amp_escaped.len());
    let mut in_newline_run = false;
    let chars: Vec<char> = amp_escaped.chars().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        let ch = chars[i];
        match ch {
            '\r' | '\n' => {
                if !in_newline_run {
                    out.push(' ');
                    in_newline_run = true;
                }
                i += 1;
            }
            '|' => {
                in_newline_run = false;
                out.push_str("&#124;");
                i += 1;
            }
            '"' => {
                in_newline_run = false;
                out.push_str("&quot;");
                i += 1;
            }
            '<' => {
                in_newline_run = false;
                // Verificar se é uma tag inline permitida sem atributos ativos
                if let Some(close_idx) = chars[i + 1..].iter().position(|&c| c == '>') {
                    let tag_end = i + 1 + close_idx;
                    let inner_tag: String = chars[i + 1..tag_end].iter().collect();
                    let trimmed_tag = inner_tag.trim().to_ascii_lowercase();

                    if ALLOWED_INLINE_TAGS.contains(&trimmed_tag.as_str()) {
                        out.push('<');
                        out.push_str(&trimmed_tag);
                        out.push('>');
                        i = tag_end + 1;
                        continue;
                    }
                }
                out.push_str("&lt;");
                i += 1;
            }
            '>' => {
                in_newline_run = false;
                out.push_str("&gt;");
                i += 1;
            }
            _ => {
                in_newline_run = false;
                out.push(ch);
                i += 1;
            }
        }
    }

    out
}

fn non_empty_trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

fn render_paragraph(titulo: Option<&str>, texto: &str, colapsavel: Option<&str>) -> String {
    let summary = non_empty_trimmed(colapsavel);
    let escaped_text = escape_html_pipe(texto);

    let inner = if let Some(title) = non_empty_trimmed(titulo) {
        format!(
            "<section class=\"anki-note-section\"><p><b>{}</b></p><p>{}</p></section>",
            escape_html_pipe(title),
            escaped_text,
        )
    } else {
        format!("<p>{escaped_text}</p>")
    };

    wrap_collapsible(inner, summary)
}

fn render_key_value(
    titulo: Option<&str>,
    pares: &[KeyValueItem],
    colapsavel: Option<&str>,
) -> String {
    let summary = non_empty_trimmed(colapsavel);
    let mut dl = String::from("<dl class=\"anki-kv\">");

    for item in pares {
        dl.push_str("<dt>");
        dl.push_str(&escape_html_pipe(&item.chave));
        dl.push_str("</dt><dd>");
        dl.push_str(&escape_html_pipe(&item.valor));
        dl.push_str("</dd>");
    }
    dl.push_str("</dl>");

    let inner = if let Some(title) = non_empty_trimmed(titulo) {
        format!(
            "<section class=\"anki-kv-section\"><p><b>{}</b></p>{dl}</section>",
            escape_html_pipe(title),
        )
    } else {
        dl
    };

    wrap_collapsible(inner, summary)
}

fn render_callout(variante: CalloutKind, rotulo: Option<&str>, texto: &str) -> String {
    let raw_label = non_empty_trimmed(rotulo).unwrap_or_else(|| variante.default_label());
    let clean_label = raw_label.trim_end_matches(':').trim();

    format!(
        "<blockquote class=\"anki-callout {}\"><p><b>{}:</b> {}</p></blockquote>",
        variante.css_modifier(),
        escape_html_pipe(clean_label),
        escape_html_pipe(texto),
    )
}

fn render_list(
    titulo: Option<&str>,
    ordenada: bool,
    itens: &[String],
    colapsavel: Option<&str>,
) -> String {
    let summary = non_empty_trimmed(colapsavel);
    let (section_class, list_tag) = if ordenada {
        ("anki-steps", "ol")
    } else {
        ("anki-list", "ul")
    };

    let mut out = format!("<section class=\"{section_class}\">");
    if let Some(title) = non_empty_trimmed(titulo) {
        out.push_str("<p><b>");
        out.push_str(&escape_html_pipe(title));
        out.push_str("</b></p>");
    }

    out.push('<');
    out.push_str(list_tag);
    out.push('>');
    for item in itens {
        out.push_str("<li>");
        out.push_str(&escape_html_pipe(item));
        out.push_str("</li>");
    }
    out.push_str("</");
    out.push_str(list_tag);
    out.push_str("></section>");

    wrap_collapsible(out, summary)
}

fn render_table(spec: &TableSpec, colapsavel: Option<&str>) -> Result<String, RenderError> {
    let raw = spec.render_html().map_err(RenderError::TableRender)?;
    let sanitized = sanitize_rendered_fragment(&raw);
    let with_class = ensure_anki_table_class(&sanitized);
    Ok(wrap_collapsible(with_class, non_empty_trimmed(colapsavel)))
}

fn ensure_anki_table_class(html: &str) -> String {
    let mut result = String::with_capacity(html.len() + 32);
    let mut cursor = 0usize;

    while let Some(table_start) = find_table_open_tag_from(html, cursor) {
        let Some(rel_end) = html[table_start..].find('>') else {
            break;
        };
        let tag_end = table_start + rel_end;
        let open_tag = &html[table_start..=tag_end];

        if open_tag.contains("anki-table") {
            result.push_str(&html[cursor..=tag_end]);
        } else if let Some(class_attr_rel) = open_tag.find("class=\"") {
            let insert_at = table_start + class_attr_rel + "class=\"".len();
            result.push_str(&html[cursor..insert_at]);
            result.push_str("anki-table ");
            result.push_str(&html[insert_at..=tag_end]);
        } else {
            let insert_at = table_start + "<table".len();
            result.push_str(&html[cursor..insert_at]);
            result.push_str(" class=\"anki-table\"");
            result.push_str(&html[insert_at..=tag_end]);
        }
        cursor = tag_end + 1;
    }

    result.push_str(&html[cursor..]);
    result
}

fn count_table_open_tags(html: &str) -> usize {
    let mut count = 0usize;
    let mut offset = 0usize;
    while let Some(pos) = find_table_open_tag_from(html, offset) {
        count += 1;
        offset = pos + "<table".len();
    }
    count
}

fn find_table_open_tag_from(html: &str, start: usize) -> Option<usize> {
    let mut offset = start;
    while let Some(pos) = html[offset..].find("<table") {
        let abs = offset + pos;
        let next = html[abs + "<table".len()..].chars().next();
        if matches!(next, Some('>' | ' ' | '\t')) {
            return Some(abs);
        }
        offset = abs + "<table".len();
    }
    None
}

fn wrap_collapsible(inner_html: String, summary: Option<&str>) -> String {
    let Some(summary_text) = non_empty_trimmed(summary) else {
        return inner_html;
    };
    if inner_html.contains("<details") {
        return inner_html;
    }
    format!(
        "<details class=\"anki-detail\"><summary><b>{}</b></summary>{inner_html}</details>",
        escape_html_pipe(summary_text),
    )
}

fn sanitize_rendered_fragment(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_newline_run = false;

    for ch in html.chars() {
        match ch {
            '\r' | '\n' => {
                if !in_newline_run {
                    out.push(' ');
                    in_newline_run = true;
                }
            }
            '|' => {
                in_newline_run = false;
                out.push_str("&#124;");
            }
            _ => {
                in_newline_run = false;
                out.push(ch);
            }
        }
    }
    out
}

fn collect_unique_sources(fontes: &[NoteSource]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut refs = Vec::new();
    for src in fontes {
        let formatted = src.format_citation();
        let trimmed = formatted.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_lowercase()) {
            refs.push(trimmed.to_owned());
        }
    }
    refs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note_flow::{FlowEdge, FlowNode};
    use crate::note_model::SourceLocation;
    use crate::validated_flow::{RawFlow, ValidatedFlow};
    use serde_json::json;

    #[test]
    fn renders_semantic_blocks_without_br_and_escapes_pipes() {
        let flow = ValidatedFlow::try_from(RawFlow {
            inicio: "n1".into(),
            nos: vec![
                FlowNode {
                    id: "n1".into(),
                    texto: "Triagem".into(),
                },
                FlowNode {
                    id: "n2".into(),
                    texto: "Conduta".into(),
                },
            ],
            arestas: vec![FlowEdge {
                origem: "n1".into(),
                destino: "n2".into(),
                rotulo: "Positivo".into(),
            }],
        })
        .unwrap();

        let fig = serde_json::from_value(json!({
            "filename": "ecg_brd.png",
            "caption": "Padrão rSR' | V1"
        }))
        .unwrap();

        let doc = NoteDocument {
            blocos: vec![
                NoteBlock::Paragrafo {
                    titulo: Some("Mecanismo".into()),
                    texto: "A | B".into(),
                    colapsavel: Some("Ver mecanismo".into()),
                },
                NoteBlock::Callout {
                    variante: CalloutKind::Trap,
                    rotulo: None,
                    texto: "Não confundir X com Y.".into(),
                },
                NoteBlock::Tabela {
                    spec: TableSpec {
                        caption: "Diferencial".into(),
                        headers: vec!["A".into(), "B".into()],
                        rows: vec![vec!["1".into(), "2".into()]],
                    },
                    colapsavel: None,
                },
                NoteBlock::Fluxograma {
                    titulo: Some("Passos".into()),
                    fluxo: flow,
                    colapsavel: None,
                },
                NoteBlock::Figura {
                    figura: fig,
                    colapsavel: None,
                },
            ],
            fontes: vec![
                NoteSource {
                    titulo: "Diretriz SBC 2024".into(),
                    localizacao: Some(SourceLocation::Secao {
                        referencia: "cap. 3".into(),
                    }),
                    trecho: None,
                },
                NoteSource {
                    titulo: "Diretriz SBC 2024".into(),
                    localizacao: Some(SourceLocation::Secao {
                        referencia: "cap. 3".into(),
                    }),
                    trecho: None,
                },
            ],
        };

        let out = render_document(&doc).expect("render válido");
        assert_eq!(out.table_count, 1);
        assert_eq!(out.figure_count, 1);
        assert_eq!(out.source_refs, vec!["Diretriz SBC 2024, cap. 3."]);
        assert!(!out.html.contains("<br"));
        assert!(!out.html.contains('|'));
        assert!(!out.html.contains('\n'));
        assert!(out.html.contains("<details class=\"anki-detail\">"));
        assert!(out.html.contains("anki-callout--trap"));
        assert!(out.html.contains("class=\"anki-table"));
        assert!(out.html.ends_with("</cite></small></footer>"));
    }

    #[test]
    fn collapsible_does_not_swallow_title() {
        let doc = NoteDocument {
            blocos: vec![NoteBlock::Paragrafo {
                titulo: Some("Fisiopatologia".into()),
                texto: "Detalhes do mecanismo.".into(),
                colapsavel: Some("Expandir".into()),
            }],
            fontes: vec![],
        };
        let out = render_document(&doc).unwrap();
        assert!(out.html.contains("<summary><b>Expandir</b></summary>"));
        assert!(out.html.contains("<b>Fisiopatologia</b>"));
    }

    #[test]
    fn preserves_safe_inline_html_and_neutralizes_active_content() {
        let text =
            "<b>Negrito</b> &amp; <i>Itálico</i> & leigo <script>alert(1)</script> H<sub>2</sub>O";
        let escaped = escape_html_pipe(text);
        assert!(escaped.contains("<b>Negrito</b>"));
        assert!(escaped.contains("<i>Itálico</i>"));
        assert!(escaped.contains("H<sub>2</sub>O"));
        assert!(escaped.contains("&amp;"));
        assert!(escaped.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!escaped.contains("<script>"));
    }
}
