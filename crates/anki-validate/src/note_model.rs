use std::num::NonZeroU64;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::media_figure::MediaFigure;
use crate::table_render::TableSpec;
use crate::validated_flow::ValidatedFlow;

/// Modo de compilação do documento de notas adicionais.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompileMode {
    #[default]
    LegacyCompat,
    StrictV2,
}

/// Variante semântica de um bloco de destaque (`Callout`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CalloutKind {
    Trap,
    Pearl,
    Diff,
}

impl CalloutKind {
    #[must_use]
    pub const fn css_modifier(&self) -> &'static str {
        match self {
            Self::Trap => "anki-callout--trap",
            Self::Pearl => "anki-callout--pearl",
            Self::Diff => "anki-callout--diff",
        }
    }

    #[must_use]
    pub const fn default_label(&self) -> &'static str {
        match self {
            Self::Trap => "Armadilha",
            Self::Pearl => "Regra prática",
            Self::Diff => "Diferencial",
        }
    }
}

/// Par chave/valor exibido em blocos `NoteBlock::ChaveValor`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyValueItem {
    pub chave: String,
    pub valor: String,
}

/// Localização estruturada dentro de uma fonte bibliográfica ou de mídia.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "formato", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceLocation {
    Pdf { pagina: NonZeroU64 },
    Secao { referencia: String },
    Url { href: String },
    Timestamp { marca: String },
}

/// Fonte bibliográfica ou de referência associada ao documento de nota.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoteSource {
    pub titulo: String,
    #[serde(default)]
    pub localizacao: Option<SourceLocation>,
    #[serde(default)]
    pub trecho: Option<String>,
}

impl NoteSource {
    #[must_use]
    pub fn format_citation(&self) -> String {
        let titulo = self.titulo.trim();
        match &self.localizacao {
            Some(SourceLocation::Pdf { pagina }) => format!("{titulo}, p. PDF {pagina}."),
            Some(SourceLocation::Secao { referencia }) => {
                format!("{titulo}, {}.", referencia.trim())
            }
            Some(SourceLocation::Url { href }) => format!("{titulo} ({}).", href.trim()),
            Some(SourceLocation::Timestamp { marca }) => format!("{titulo} ({}).", marca.trim()),
            None => format!("{titulo}."),
        }
    }

    pub fn validate(&self, index: usize) -> Result<(), NoteModelError> {
        if self.titulo.trim().is_empty() {
            return Err(NoteModelError::EmptySourceTitle { index });
        }

        match &self.localizacao {
            Some(SourceLocation::Secao { referencia }) if referencia.trim().is_empty() => {
                Err(NoteModelError::EmptySourceLocationField {
                    index,
                    formato: "secao",
                    field: "referencia",
                })
            }
            Some(SourceLocation::Url { href }) if href.trim().is_empty() => {
                Err(NoteModelError::EmptySourceLocationField {
                    index,
                    formato: "url",
                    field: "href",
                })
            }
            Some(SourceLocation::Timestamp { marca }) if marca.trim().is_empty() => {
                Err(NoteModelError::EmptySourceLocationField {
                    index,
                    formato: "timestamp",
                    field: "marca",
                })
            }
            Some(_) | None => Ok(()),
        }
    }
}

/// Bloco tipado de conteúdo que compõe um `NoteDocument`.
/// Estados inválidos tornam-se irrepresentáveis em tempo de desserialização.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "tipo", rename_all = "snake_case", deny_unknown_fields)]
pub enum NoteBlock {
    Paragrafo {
        #[serde(default)]
        titulo: Option<String>,
        texto: String,
        #[serde(default)]
        colapsavel: Option<String>,
    },
    ChaveValor {
        #[serde(default)]
        titulo: Option<String>,
        pares: Vec<KeyValueItem>,
        #[serde(default)]
        colapsavel: Option<String>,
    },
    Callout {
        variante: CalloutKind,
        #[serde(default)]
        rotulo: Option<String>,
        texto: String,
    },
    Lista {
        #[serde(default)]
        titulo: Option<String>,
        #[serde(default)]
        ordenada: bool,
        itens: Vec<String>,
        #[serde(default)]
        colapsavel: Option<String>,
    },
    Tabela {
        spec: TableSpec,
        #[serde(default)]
        colapsavel: Option<String>,
    },
    Fluxograma {
        #[serde(default)]
        titulo: Option<String>,
        fluxo: ValidatedFlow,
        #[serde(default)]
        colapsavel: Option<String>,
    },
    Figura {
        figura: MediaFigure,
        #[serde(default)]
        colapsavel: Option<String>,
    },
    #[doc(hidden)]
    #[serde(skip_deserializing)]
    #[schemars(skip)]
    LegacyHtml { html: String },
}

/// Documento estruturado de nota composto por blocos tipados e fontes deduplicáveis.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoteDocument {
    #[serde(default)]
    pub blocos: Vec<NoteBlock>,
    #[serde(default)]
    pub fontes: Vec<NoteSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NoteModelError {
    #[error("bloco #{index} ({kind}): campo `texto` não pode estar vazio")]
    EmptyText { index: usize, kind: &'static str },

    #[error("bloco #{index} (chave_valor): `pares` não pode estar vazio")]
    EmptyKeyValueList { index: usize },

    #[error("bloco #{index} (chave_valor): par #{item_index} possui `{field}` vazio")]
    EmptyKeyValueField {
        index: usize,
        item_index: usize,
        field: &'static str,
    },

    #[error("bloco #{index} (lista): `itens` não pode estar vazio")]
    EmptyListItems { index: usize },

    #[error("bloco #{index} (lista): item #{item_index} não pode estar vazio")]
    EmptyListItem { index: usize, item_index: usize },

    #[error("bloco #{index} (tabela): `spec.headers` não pode estar vazio")]
    EmptyTableHeaders { index: usize },

    #[error("bloco #{index} (figura): `figura.filename` não pode estar vazio")]
    EmptyFigureFilename { index: usize },

    #[error(
        "bloco #{index} (figura): `figura.filename` não pode usar prefixo remoto ou data URI (`{filename}`)"
    )]
    ForbiddenFigureFilenamePrefix { index: usize, filename: String },

    #[error("bloco #{index} (figura): `figura.filename` inseguro: {reason} (`{filename}`)")]
    UnsafeFigureFilename {
        index: usize,
        filename: String,
        reason: &'static str,
    },

    #[error("bloco #{index} ({kind}): `colapsavel`, quando informado, não pode estar vazio")]
    EmptyCollapsibleTitle { index: usize, kind: &'static str },

    #[error("fonte #{index}: `titulo` não pode estar vazio")]
    EmptySourceTitle { index: usize },

    #[error("fonte #{index}: localização `{formato}` possui `{field}` vazio")]
    EmptySourceLocationField {
        index: usize,
        formato: &'static str,
        field: &'static str,
    },
}

impl NoteDocument {
    pub fn validate(&self) -> Result<(), NoteModelError> {
        for (index, block) in self.blocos.iter().enumerate() {
            validate_block(index, block)?;
        }
        for (index, source) in self.fontes.iter().enumerate() {
            source.validate(index)?;
        }
        Ok(())
    }
}

fn validate_block(index: usize, block: &NoteBlock) -> Result<(), NoteModelError> {
    match block {
        NoteBlock::Paragrafo {
            texto, colapsavel, ..
        } => {
            if texto.trim().is_empty() {
                return Err(NoteModelError::EmptyText {
                    index,
                    kind: "paragrafo",
                });
            }
            validate_collapsible(index, "paragrafo", colapsavel.as_deref())?;
        }
        NoteBlock::ChaveValor {
            pares, colapsavel, ..
        } => {
            if pares.is_empty() {
                return Err(NoteModelError::EmptyKeyValueList { index });
            }
            for (item_index, item) in pares.iter().enumerate() {
                if item.chave.trim().is_empty() {
                    return Err(NoteModelError::EmptyKeyValueField {
                        index,
                        item_index,
                        field: "chave",
                    });
                }
                if item.valor.trim().is_empty() {
                    return Err(NoteModelError::EmptyKeyValueField {
                        index,
                        item_index,
                        field: "valor",
                    });
                }
            }
            validate_collapsible(index, "chave_valor", colapsavel.as_deref())?;
        }
        NoteBlock::Callout { texto, .. } => {
            if texto.trim().is_empty() {
                return Err(NoteModelError::EmptyText {
                    index,
                    kind: "callout",
                });
            }
        }
        NoteBlock::Lista {
            itens, colapsavel, ..
        } => {
            if itens.is_empty() {
                return Err(NoteModelError::EmptyListItems { index });
            }
            for (item_index, item) in itens.iter().enumerate() {
                if item.trim().is_empty() {
                    return Err(NoteModelError::EmptyListItem { index, item_index });
                }
            }
            validate_collapsible(index, "lista", colapsavel.as_deref())?;
        }
        NoteBlock::Tabela { spec, colapsavel } => {
            if spec.headers.is_empty() {
                return Err(NoteModelError::EmptyTableHeaders { index });
            }
            validate_collapsible(index, "tabela", colapsavel.as_deref())?;
        }
        NoteBlock::Fluxograma { colapsavel, .. } => {
            validate_collapsible(index, "fluxograma", colapsavel.as_deref())?;
        }
        NoteBlock::Figura { figura, colapsavel } => {
            let filename = figura.filename.trim();
            if filename.is_empty() {
                return Err(NoteModelError::EmptyFigureFilename { index });
            }
            if has_forbidden_media_prefix(filename) {
                return Err(NoteModelError::ForbiddenFigureFilenamePrefix {
                    index,
                    filename: figura.filename.clone(),
                });
            }
            if let Some(reason) = is_unsafe_figure_filename(filename) {
                return Err(NoteModelError::UnsafeFigureFilename {
                    index,
                    filename: figura.filename.clone(),
                    reason,
                });
            }
            validate_collapsible(index, "figura", colapsavel.as_deref())?;
        }
        NoteBlock::LegacyHtml { html } => {
            if html.trim().is_empty() {
                return Err(NoteModelError::EmptyText {
                    index,
                    kind: "legacy_html",
                });
            }
        }
    }
    Ok(())
}

fn validate_collapsible(
    index: usize,
    kind: &'static str,
    colapsavel: Option<&str>,
) -> Result<(), NoteModelError> {
    if let Some(label) = colapsavel
        && label.trim().is_empty()
    {
        return Err(NoteModelError::EmptyCollapsibleTitle { index, kind });
    }
    Ok(())
}

fn has_forbidden_media_prefix(filename: &str) -> bool {
    let lower = filename.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("data:")
        || lower.starts_with("ftp://")
        || lower.starts_with("file://")
        || lower.starts_with("javascript:")
        || lower.starts_with("blob:")
        || lower.starts_with("about:")
        || lower.starts_with("vbscript:")
        || lower.contains("://")
}

fn is_unsafe_figure_filename(filename: &str) -> Option<&'static str> {
    if filename.contains("..") {
        return Some("caminho com travessia de diretório (`..`) não permitido");
    }
    if filename.contains('/') || filename.contains('\\') {
        return Some("separadores de diretório (`/` ou `\\`) não permitidos");
    }
    if filename.contains(':') {
        return Some("dois-pontos (`:`) não permitidos no nome do arquivo");
    }
    if filename.chars().any(char::is_control) {
        return Some("caracteres de controle não permitidos no nome do arquivo");
    }
    if filename.contains('<')
        || filename.contains('>')
        || filename.contains('"')
        || filename.contains('\'')
    {
        return Some("caracteres delimitadores de HTML não permitidos no nome do arquivo");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_figure(filename: &str) -> MediaFigure {
        serde_json::from_value(json!({ "filename": filename })).expect("valid MediaFigure")
    }

    #[test]
    fn source_format_citation_all_variants() {
        let pdf = NoteSource {
            titulo: "Harrison".into(),
            localizacao: Some(SourceLocation::Pdf {
                pagina: NonZeroU64::new(105).unwrap(),
            }),
            trecho: None,
        };
        assert_eq!(pdf.format_citation(), "Harrison, p. PDF 105.");

        let sec = NoteSource {
            titulo: "Diretriz SBC".into(),
            localizacao: Some(SourceLocation::Secao {
                referencia: "Cap. 3".into(),
            }),
            trecho: None,
        };
        assert_eq!(sec.format_citation(), "Diretriz SBC, Cap. 3.");

        let url = NoteSource {
            titulo: "UpToDate".into(),
            localizacao: Some(SourceLocation::Url {
                href: "https://uptodate.com/topic".into(),
            }),
            trecho: None,
        };
        assert_eq!(
            url.format_citation(),
            "UpToDate (https://uptodate.com/topic)."
        );

        let ts = NoteSource {
            titulo: "Vídeo Aula".into(),
            localizacao: Some(SourceLocation::Timestamp {
                marca: "12:34".into(),
            }),
            trecho: None,
        };
        assert_eq!(ts.format_citation(), "Vídeo Aula (12:34).");

        let none = NoteSource {
            titulo: "Consenso 2024".into(),
            localizacao: None,
            trecho: None,
        };
        assert_eq!(none.format_citation(), "Consenso 2024.");
    }

    #[test]
    fn rejects_remote_and_data_uri_figures() {
        for bad in [
            "https://x.com/a.png",
            "http://x.com/a.png",
            "data:image/png;base64,AA",
            "ftp://files.org/a.jpg",
            "javascript:alert(1)",
        ] {
            let doc = NoteDocument {
                blocos: vec![NoteBlock::Figura {
                    figura: sample_figure(bad),
                    colapsavel: None,
                }],
                fontes: vec![],
            };
            assert!(matches!(
                doc.validate(),
                Err(NoteModelError::ForbiddenFigureFilenamePrefix { .. })
            ));
        }
    }

    #[test]
    fn rejects_unsafe_figure_filenames() {
        for bad in [
            "../secret.png",
            "subdir/fig.png",
            "C:\\image.png",
            "fig:1.png",
            "bad<name>.png",
            "\"escaped\".png",
        ] {
            let doc = NoteDocument {
                blocos: vec![NoteBlock::Figura {
                    figura: sample_figure(bad),
                    colapsavel: None,
                }],
                fontes: vec![],
            };
            assert!(matches!(
                doc.validate(),
                Err(NoteModelError::UnsafeFigureFilename { .. })
            ));
        }
    }

    #[test]
    fn deny_unknown_fields_on_document_and_source() {
        assert!(serde_json::from_value::<NoteDocument>(json!({ "extra": 1 })).is_err());
        assert!(serde_json::from_value::<NoteSource>(json!({ "titulo": "A", "x": 1 })).is_err());
        assert!(
            serde_json::from_value::<KeyValueItem>(json!({ "chave": "k", "valor": "v", "foo": 1 }))
                .is_err()
        );
    }

    #[test]
    fn rejects_unknown_fields_on_blocks() {
        let bad_paragraph = json!({
            "tipo": "paragrafo",
            "texto": "teste",
            "pares": []
        });
        assert!(serde_json::from_value::<NoteBlock>(bad_paragraph).is_err());

        let bad_callout = json!({
            "tipo": "callout",
            "variante": "trap",
            "texto": "cuidado",
            "colapsavel": "recolher"
        });
        assert!(serde_json::from_value::<NoteBlock>(bad_callout).is_err());
    }

    #[test]
    fn rejects_empty_collapsible_titles() {
        let doc = NoteDocument {
            blocos: vec![NoteBlock::Paragrafo {
                titulo: None,
                texto: "ok".into(),
                colapsavel: Some("   ".into()),
            }],
            fontes: vec![],
        };
        assert!(matches!(
            doc.validate(),
            Err(NoteModelError::EmptyCollapsibleTitle {
                kind: "paragrafo",
                ..
            })
        ));
    }
}
