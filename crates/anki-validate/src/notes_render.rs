use std::collections::HashSet;

use serde::Deserialize;

use crate::note_flow::{NoteFlow, render_note_flow};
use crate::sanitize::sanitize_field;
use crate::table_render::{TableSpec, markdown_tables_to_html};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteSource {
    fonte: String,
    pagina: u64,
    trecho: String,
}

impl NoteSource {
    fn validate(&self) -> Result<(), String> {
        if self.fonte.trim().is_empty() {
            return Err("NoteSource.fonte must not be empty".to_string());
        }
        if self.pagina < 1 {
            return Err("NoteSource.pagina must be >= 1".to_string());
        }
        if self.trecho.trim().is_empty() {
            return Err("NoteSource.trecho must not be empty".to_string());
        }
        Ok(())
    }

    fn reference(&self) -> String {
        format!("{}, p. PDF {}.", self.fonte.trim(), self.pagina)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteBlock {
    tipo: String,
    #[serde(default)]
    titulo: Option<String>,
    #[serde(default)]
    texto: Option<String>,
    #[serde(default)]
    itens: Option<Vec<String>>,
    #[serde(default)]
    tabela: Option<TableSpec>,
    #[serde(default)]
    fluxograma: Option<NoteFlow>,
    fontes: Vec<NoteSource>,
}

impl NoteBlock {
    fn validate(&self) -> Result<(), String> {
        if self.fontes.is_empty() {
            return Err("NoteBlock.fontes must contain at least one source".to_string());
        }
        for source in &self.fontes {
            source.validate()?;
        }

        let texto = self.texto.as_deref().unwrap_or("");
        let itens = self.itens.as_deref().unwrap_or(&[]);

        let valid = match self.tipo.as_str() {
            "paragrafo" | "alerta" => {
                !texto.trim().is_empty()
                    && itens.is_empty()
                    && self.tabela.is_none()
                    && self.fluxograma.is_none()
            }
            "lista" => {
                !itens.is_empty()
                    && itens.iter().all(|x| !x.trim().is_empty())
                    && texto.is_empty()
                    && self.tabela.is_none()
                    && self.fluxograma.is_none()
            }
            "fluxograma" => {
                self.fluxograma.is_some()
                    && self.tabela.is_none()
                    && texto.is_empty()
                    && itens.is_empty()
            }
            "tabela" => {
                self.tabela.is_some()
                    && self.fluxograma.is_none()
                    && texto.is_empty()
                    && itens.is_empty()
            }
            other => {
                return Err(format!("Unsupported NoteBlock tipo: {other}"));
            }
        };

        if !valid {
            return Err(
                "Block content must match tipo; empty or competing payloads are invalid"
                    .to_string(),
            );
        }

        Ok(())
    }

    fn render_html(&self) -> Result<String, String> {
        let titulo = self.titulo.as_deref().unwrap_or("");
        let title = if titulo.is_empty() {
            String::new()
        } else {
            format!("<p><b>{}</b></p>", text(titulo))
        };

        Ok(match self.tipo.as_str() {
            "tabela" => {
                let spec = self.tabela.as_ref().expect("validated tabela");
                format!("{title}{}", spec.render_html()?)
            }
            "fluxograma" => {
                let flow = self.fluxograma.as_ref().expect("validated fluxograma");
                format!("{title}{}", render_note_flow(flow))
            }
            "lista" => {
                let itens = self.itens.as_deref().unwrap_or(&[]);
                let list_items: String = itens
                    .iter()
                    .map(|item| format!("<li>{}</li>", text(item)))
                    .collect();
                format!("{title}<ul>{list_items}</ul>")
            }
            "alerta" => {
                let body = self.texto.as_deref().unwrap_or("");
                format!("{title}<blockquote>{}</blockquote>", text(body))
            }
            _ => {
                let body = self.texto.as_deref().unwrap_or("");
                format!("{title}<p>{}</p>", text(body))
            }
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ExtraNotes {
    #[serde(default)]
    fisiopatologia: Option<String>,
    #[serde(default)]
    raciocinio_clinico: Option<String>,
    #[serde(default)]
    armadilhas_diferenciais: Option<String>,
    #[serde(default)]
    correlacao_visual: Option<String>,
    #[serde(default)]
    tabela: Option<TableSpec>,
    #[serde(default)]
    texto_livre: Option<String>,
    #[serde(default)]
    blocos: Option<Vec<NoteBlock>>,
}

fn text(value: &str) -> String {
    sanitize_field(value, false, false).0
}

pub fn render_extra_notes(
    notes: &serde_json::Value,
) -> Result<(String, Vec<String>, usize), String> {
    if !notes.is_object() {
        return Err("ExtraNotes must be a JSON object".to_string());
    }

    let parsed: ExtraNotes = serde_json::from_value(notes.clone())
        .map_err(|err| format!("Invalid ExtraNotes payload: {err}"))?;

    let blocos = parsed.blocos.as_deref().unwrap_or(&[]);
    for block in blocos {
        block.validate()?;
    }

    let mut seen_refs = HashSet::new();
    let mut source_refs = Vec::new();
    for block in blocos {
        for source in &block.fontes {
            let reference = source.reference();
            if seen_refs.insert(reference.clone()) {
                source_refs.push(reference);
            }
        }
    }

    let mut parts = Vec::new();
    let mut table_count = 0usize;

    if let Some(val) = parsed.fisiopatologia.as_deref()
        && !val.trim().is_empty()
    {
        parts.push(format!(
            "• <b>Fisiopatologia &amp; Mecanismo:</b> {}",
            val.trim()
        ));
    }
    if let Some(val) = parsed.raciocinio_clinico.as_deref()
        && !val.trim().is_empty()
    {
        parts.push(format!(
            "• <b>Raciocínio Clínico &amp; Prática:</b> {}",
            val.trim()
        ));
    }
    if let Some(val) = parsed.armadilhas_diferenciais.as_deref()
        && !val.trim().is_empty()
    {
        parts.push(format!(
            "• <b>Armadilhas &amp; Diferenciais:</b> {}",
            val.trim()
        ));
    }
    if let Some(val) = parsed.correlacao_visual.as_deref()
        && !val.trim().is_empty()
    {
        parts.push(format!(
            "• <b>Correlação de Imagem / Quadro:</b> {}",
            val.trim()
        ));
    }
    if let Some(spec) = parsed.tabela.as_ref() {
        parts.push(spec.render_html()?);
        table_count += 1;
    }
    if let Some(val) = parsed.texto_livre.as_deref()
        && !val.trim().is_empty()
    {
        let (converted_free, _) = markdown_tables_to_html(val.trim());
        parts.push(converted_free);
    }

    for block in blocos {
        if block.tipo == "tabela" {
            table_count += 1;
        }
        parts.push(block.render_html()?);
    }

    Ok((parts.join("<br>"), source_refs, table_count))
}

#[cfg(test)]
mod tests {
    use super::render_extra_notes;
    use serde_json::json;

    #[test]
    fn empty_object_renders_empty_result() {
        let input = json!({});
        let (html, refs, tables) = render_extra_notes(&input).expect("empty object should succeed");
        assert_eq!(html, "");
        assert!(refs.is_empty());
        assert_eq!(tables, 0);
    }

    #[test]
    fn renders_fields_blocks_and_unique_sources_without_leaking_evidence() {
        let input = json!({
            "fisiopatologia": "  Mecanismo principal  ",
            "raciocinio_clinico": null,
            "blocos": [
                {
                    "tipo": "paragrafo",
                    "titulo": "Resumo",
                    "texto": "Texto clinico",
                    "fontes": [
                        {"fonte": "Diretriz A", "pagina": 12, "trecho": "Evidencia 1"},
                        {"fonte": "Diretriz B", "pagina": 3, "trecho": "Evidencia 2"}
                    ]
                },
                {
                    "tipo": "alerta",
                    "texto": "Atencao maxima",
                    "fontes": [
                        {"fonte": "Diretriz A", "pagina": 12, "trecho": "Evidencia repetida"}
                    ]
                }
            ]
        });

        let (html, refs, tables) = render_extra_notes(&input).expect("valid notes should render");
        assert!(html.contains("• <b>Fisiopatologia &amp; Mecanismo:</b> Mecanismo principal"));
        assert!(html.contains("<p><b>Resumo</b></p><p>Texto clinico</p>"));
        assert!(html.contains("<blockquote>Atencao maxima</blockquote>"));
        assert!(!html.contains("Evidencia"));
        assert_eq!(
            refs,
            vec![
                "Diretriz A, p. PDF 12.".to_string(),
                "Diretriz B, p. PDF 3.".to_string()
            ]
        );
        assert_eq!(tables, 0);
    }

    #[test]
    fn rejects_non_object_and_malformed_block() {
        let non_object = json!("not an object");
        assert!(render_extra_notes(&non_object).is_err());

        let bad_block = json!({
            "blocos": [
                {
                    "tipo": "lista",
                    "texto": "Competing text",
                    "itens": ["Item 1"],
                    "fontes": [
                        {"fonte": "Livro", "pagina": 1, "trecho": "Trecho"}
                    ]
                }
            ]
        });
        assert!(render_extra_notes(&bad_block).is_err());
    }
}
