use std::collections::HashSet;
use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::media_figure::MediaFigure;
use crate::note_model::{
    CalloutKind, CompileMode, KeyValueItem, NoteBlock, NoteDocument, NoteModelError, NoteSource,
    SourceLocation,
};
use crate::note_render::{RenderError, RenderedNotes, render_document};
use crate::table_render::{TableSpec, markdown_tables_to_html};
use crate::validated_flow::{FlowError, RawFlow, ValidatedFlow};

const LEGACY_EXTRA_NOTES_KEYS: &[&str] = &[
    "fisiopatologia",
    "raciocinio_clinico",
    "armadilhas_diferenciais",
    "correlacao_visual",
    "tabela",
    "texto_livre",
];

#[derive(Debug, Clone, Default)]
pub struct DraftNotesInput<'a> {
    pub notas_adicionais: Option<&'a Value>,
    pub tabela_notas: Option<&'a Value>,
    pub tabelas_notas: Option<&'a Value>,
    pub figuras_notas: Option<&'a Value>,
}

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("campo legado `{0}` não é permitido em CompileMode::StrictV2")]
    LegacyFieldNotAllowedInStrictV2(&'static str),

    #[error("campo JSON inválido `{field}`: {detail}")]
    InvalidJsonField { field: &'static str, detail: String },

    #[error("tipo de bloco legado não suportado: `{0}`")]
    UnsupportedLegacyBlockType(String),

    #[error("cargas conflitantes detectadas: {0}")]
    CompetingPayloads(String),

    #[error("fluxograma inválido: {0}")]
    InvalidFlow(#[from] FlowError),

    #[error("modelo de notas inválido: {0}")]
    InvalidModel(#[from] NoteModelError),
}

#[derive(Debug, Error)]
pub enum CompileNotesError {
    #[error(transparent)]
    Adapter(#[from] AdapterError),

    #[error(transparent)]
    Render(#[from] RenderError),
}

#[derive(Debug, Clone)]
pub struct CompiledNotes {
    pub doc: NoteDocument,
    pub rendered: RenderedNotes,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyExtraNotes {
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
    blocos: Option<Vec<LegacyNoteBlock>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyNoteBlock {
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
    fluxograma: Option<RawFlow>,
    #[serde(default)]
    fontes: Vec<LegacyNoteSource>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyNoteSource {
    fonte: String,
    #[serde(default)]
    pagina: u64,
    #[serde(default)]
    referencia: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    trecho: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SourceDedupKey {
    titulo: String,
    localizacao: Option<SourceLocationDedupKey>,
    trecho: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SourceLocationDedupKey {
    Pdf(u64),
    Secao(String),
    Url(String),
    Timestamp(String),
}

pub fn normalize_to_document(
    input: &DraftNotesInput<'_>,
    mode: CompileMode,
) -> Result<NoteDocument, AdapterError> {
    match mode {
        CompileMode::StrictV2 => normalize_strict_v2(input),
        CompileMode::LegacyCompat => normalize_legacy_compat(input),
    }
}

pub fn compile_extra_notes(
    input: &DraftNotesInput<'_>,
    mode: CompileMode,
) -> Result<CompiledNotes, CompileNotesError> {
    let doc = normalize_to_document(input, mode)?;
    let rendered = render_document(&doc)?;
    Ok(CompiledNotes { doc, rendered })
}

fn normalize_strict_v2(input: &DraftNotesInput<'_>) -> Result<NoteDocument, AdapterError> {
    if is_populated(input.tabela_notas) {
        return Err(AdapterError::LegacyFieldNotAllowedInStrictV2(
            "tabela_notas",
        ));
    }
    if is_populated(input.tabelas_notas) {
        return Err(AdapterError::LegacyFieldNotAllowedInStrictV2(
            "tabelas_notas",
        ));
    }
    if is_populated(input.figuras_notas) {
        return Err(AdapterError::LegacyFieldNotAllowedInStrictV2(
            "figuras_notas",
        ));
    }

    let Some(raw_notes) = input.notas_adicionais else {
        return Ok(NoteDocument::default());
    };
    if raw_notes.is_null() {
        return Ok(NoteDocument::default());
    }

    let obj = raw_notes
        .as_object()
        .ok_or_else(|| AdapterError::InvalidJsonField {
            field: "notas_adicionais",
            detail: "em StrictV2, notas_adicionais deve ser um objeto NoteDocument v2".into(),
        })?;

    if let Some(legacy_key) = find_legacy_key(obj) {
        return Err(AdapterError::LegacyFieldNotAllowedInStrictV2(legacy_key));
    }

    let doc: NoteDocument = serde_json::from_value(raw_notes.clone()).map_err(|err| {
        AdapterError::InvalidJsonField {
            field: "notas_adicionais",
            detail: err.to_string(),
        }
    })?;
    doc.validate()?;
    Ok(doc)
}

fn normalize_legacy_compat(input: &DraftNotesInput<'_>) -> Result<NoteDocument, AdapterError> {
    let mut blocos: Vec<NoteBlock> = Vec::new();
    let mut fontes: Vec<NoteSource> = Vec::new();
    let mut seen_sources: HashSet<SourceDedupKey> = HashSet::new();
    let mut seen_figures: HashSet<String> = HashSet::new();

    if let Some(raw_notes) = input.notas_adicionais {
        match raw_notes {
            Value::Null => {}
            Value::String(text) => {
                append_legacy_string_or_markdown(text, &mut blocos);
            }
            Value::Object(map) => {
                let has_top_fontes = map.contains_key("fontes");
                let has_populated_legacy = find_populated_legacy_key(map).is_some();
                let has_canonical_blocks = has_canonical_only_block(map);
                let has_legacy_blocks = has_legacy_only_block(map);
                let blocos_non_empty =
                    matches!(map.get("blocos"), Some(Value::Array(arr)) if !arr.is_empty());

                if (has_populated_legacy
                    && (has_top_fontes
                        || has_canonical_blocks
                        || (blocos_non_empty && !has_legacy_blocks)))
                    || (has_legacy_blocks && has_top_fontes)
                {
                    return Err(AdapterError::CompetingPayloads(
                        "notas_adicionais contém simultaneamente campos canônicos ('blocos'/'fontes') e chaves legadas".into(),
                    ));
                }

                let is_canonical_v2 = !has_populated_legacy
                    && !has_legacy_blocks
                    && (has_top_fontes
                        || (map.contains_key("blocos") && find_legacy_key(map).is_none()));

                if is_canonical_v2 {
                    // Documento canônico detectado: validação estrita sem fallback silencioso para legado.
                    let v2_doc: NoteDocument =
                        serde_json::from_value(raw_notes.clone()).map_err(|err| {
                            AdapterError::InvalidJsonField {
                                field: "notas_adicionais",
                                detail: err.to_string(),
                            }
                        })?;
                    v2_doc.validate()?;

                    for block in v2_doc.blocos {
                        if let NoteBlock::Figura { ref figura, .. } = block {
                            register_figure_filename(&mut seen_figures, &figura.filename);
                        }
                        blocos.push(block);
                    }
                    for source in v2_doc.fontes {
                        push_unique_source(&mut fontes, &mut seen_sources, source);
                    }
                } else {
                    let legacy: LegacyExtraNotes = serde_json::from_value(raw_notes.clone())
                        .map_err(|err| AdapterError::InvalidJsonField {
                            field: "notas_adicionais",
                            detail: err.to_string(),
                        })?;
                    append_legacy_extra_notes(legacy, &mut blocos, &mut fontes, &mut seen_sources)?;
                }
            }
            other => {
                return Err(AdapterError::InvalidJsonField {
                    field: "notas_adicionais",
                    detail: format!(
                        "esperado null, string ou objeto JSON; recebido {}",
                        json_type_name(other)
                    ),
                });
            }
        }
    }

    if is_populated(input.tabela_notas)
        && let Some(raw_table) = input.tabela_notas
    {
        let spec: TableSpec = serde_json::from_value(raw_table.clone()).map_err(|err| {
            AdapterError::InvalidJsonField {
                field: "tabela_notas",
                detail: err.to_string(),
            }
        })?;
        blocos.push(NoteBlock::Tabela {
            spec,
            colapsavel: None,
        });
    }

    if is_populated(input.tabelas_notas)
        && let Some(raw_tables) = input.tabelas_notas
    {
        let tabelas: Vec<TableSpec> =
            serde_json::from_value(raw_tables.clone()).map_err(|err| {
                AdapterError::InvalidJsonField {
                    field: "tabelas_notas",
                    detail: err.to_string(),
                }
            })?;
        for spec in tabelas {
            blocos.push(NoteBlock::Tabela {
                spec,
                colapsavel: None,
            });
        }
    }

    if is_populated(input.figuras_notas)
        && let Some(raw_figures) = input.figuras_notas
    {
        for figura in parse_legacy_figures(raw_figures)? {
            if register_figure_filename(&mut seen_figures, &figura.filename) {
                blocos.push(NoteBlock::Figura {
                    figura,
                    colapsavel: None,
                });
            }
        }
    }

    let doc = NoteDocument { blocos, fontes };
    doc.validate()?;
    Ok(doc)
}

fn append_legacy_string_or_markdown(text: &str, blocos: &mut Vec<NoteBlock>) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }

    // Se contém tabela markdown ou tabela HTML, converte mantendo o conteúdo íntegro
    if trimmed.contains('|') || trimmed.contains("&#124;") {
        let (html_table_converted, table_count) = markdown_tables_to_html(trimmed);
        if table_count > 0 {
            blocos.push(NoteBlock::LegacyHtml {
                html: html_table_converted,
            });
            return;
        }
    }

    if trimmed.contains("<table") {
        blocos.push(NoteBlock::LegacyHtml {
            html: trimmed.to_string(),
        });
        return;
    }

    blocos.push(NoteBlock::Paragrafo {
        titulo: None,
        texto: trimmed.to_string(),
        colapsavel: None,
    });
}

fn append_legacy_extra_notes(
    legacy: LegacyExtraNotes,
    blocos: &mut Vec<NoteBlock>,
    fontes: &mut Vec<NoteSource>,
    seen_sources: &mut HashSet<SourceDedupKey>,
) -> Result<(), AdapterError> {
    let mut medical_pares: Vec<KeyValueItem> = Vec::new();
    if let Some(val) = legacy.fisiopatologia.as_deref().and_then(clean_non_empty) {
        medical_pares.push(KeyValueItem {
            chave: "Fisiopatologia".into(),
            valor: val,
        });
    }
    if let Some(val) = legacy
        .raciocinio_clinico
        .as_deref()
        .and_then(clean_non_empty)
    {
        medical_pares.push(KeyValueItem {
            chave: "Raciocínio clínico".into(),
            valor: val,
        });
    }
    if let Some(val) = legacy
        .correlacao_visual
        .as_deref()
        .and_then(clean_non_empty)
    {
        medical_pares.push(KeyValueItem {
            chave: "Correlação visual".into(),
            valor: val,
        });
    }
    if !medical_pares.is_empty() {
        blocos.push(NoteBlock::ChaveValor {
            titulo: None,
            pares: medical_pares,
            colapsavel: None,
        });
    }

    if let Some(trap_text) = legacy
        .armadilhas_diferenciais
        .as_deref()
        .and_then(clean_non_empty)
    {
        blocos.push(NoteBlock::Callout {
            variante: CalloutKind::Trap,
            rotulo: Some("Armadilhas e diferenciais".into()),
            texto: trap_text,
        });
    }

    if let Some(texto) = legacy.texto_livre.as_deref().and_then(clean_non_empty) {
        append_legacy_string_or_markdown(&texto, blocos);
    }

    if let Some(spec) = legacy.tabela {
        blocos.push(NoteBlock::Tabela {
            spec,
            colapsavel: None,
        });
    }

    if let Some(legacy_blocks) = legacy.blocos {
        for block in legacy_blocks {
            let titulo = block.titulo.as_deref().and_then(clean_non_empty);
            let block_type = block.tipo.trim().to_ascii_lowercase();
            let has_text = block.texto.as_deref().is_some_and(|s| !s.trim().is_empty());
            let has_items = block.itens.as_ref().is_some_and(|v| !v.is_empty());

            let converted = match block_type.as_str() {
                "paragrafo" => {
                    if has_items || block.tabela.is_some() || block.fluxograma.is_some() {
                        return Err(AdapterError::CompetingPayloads(
                            "bloco legado `paragrafo` não pode conter `itens`, `tabela` ou `fluxograma`".into(),
                        ));
                    }
                    let texto = block
                        .texto
                        .as_deref()
                        .and_then(clean_non_empty)
                        .ok_or_else(|| AdapterError::InvalidJsonField {
                            field: "notas_adicionais.blocos",
                            detail: "bloco legado `paragrafo` requer `texto` não vazio".into(),
                        })?;
                    NoteBlock::Paragrafo {
                        titulo,
                        texto,
                        colapsavel: None,
                    }
                }
                "alerta" => {
                    if has_items || block.tabela.is_some() || block.fluxograma.is_some() {
                        return Err(AdapterError::CompetingPayloads(
                            "bloco legado `alerta` não pode conter `itens`, `tabela` ou `fluxograma`".into(),
                        ));
                    }
                    let texto = block
                        .texto
                        .as_deref()
                        .and_then(clean_non_empty)
                        .ok_or_else(|| AdapterError::InvalidJsonField {
                            field: "notas_adicionais.blocos",
                            detail: "bloco legado `alerta` requer `texto` não vazio".into(),
                        })?;
                    NoteBlock::Callout {
                        variante: CalloutKind::Trap,
                        rotulo: titulo,
                        texto,
                    }
                }
                "lista" => {
                    if has_text || block.tabela.is_some() || block.fluxograma.is_some() {
                        return Err(AdapterError::CompetingPayloads(
                            "bloco legado `lista` não pode conter `texto`, `tabela` ou `fluxograma`".into(),
                        ));
                    }
                    let itens: Vec<String> = block
                        .itens
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|item| clean_non_empty(&item))
                        .collect();
                    if itens.is_empty() {
                        return Err(AdapterError::InvalidJsonField {
                            field: "notas_adicionais.blocos",
                            detail: "bloco legado `lista` requer `itens` não vazio".into(),
                        });
                    }
                    NoteBlock::Lista {
                        titulo,
                        ordenada: false,
                        itens,
                        colapsavel: None,
                    }
                }
                "tabela" => {
                    if has_text || has_items || block.fluxograma.is_some() {
                        return Err(AdapterError::CompetingPayloads(
                            "bloco legado `tabela` não pode conter `texto`, `itens` ou `fluxograma`".into(),
                        ));
                    }
                    let mut spec = block.tabela.ok_or_else(|| AdapterError::InvalidJsonField {
                        field: "notas_adicionais.blocos",
                        detail: "bloco legado `tabela` requer campo `tabela`".into(),
                    })?;
                    if spec.caption.trim().is_empty()
                        && let Some(t) = titulo
                    {
                        spec.caption = t;
                    }
                    NoteBlock::Tabela {
                        spec,
                        colapsavel: None,
                    }
                }
                "fluxograma" => {
                    if has_text || has_items || block.tabela.is_some() {
                        return Err(AdapterError::CompetingPayloads(
                            "bloco legado `fluxograma` não pode conter `texto`, `itens` ou `tabela`".into(),
                        ));
                    }
                    let raw_flow =
                        block
                            .fluxograma
                            .ok_or_else(|| AdapterError::InvalidJsonField {
                                field: "notas_adicionais.blocos",
                                detail: "bloco legado `fluxograma` requer campo `fluxograma`"
                                    .into(),
                            })?;
                    let fluxo = ValidatedFlow::try_from(raw_flow)?;
                    NoteBlock::Fluxograma {
                        titulo,
                        fluxo,
                        colapsavel: None,
                    }
                }
                _ => return Err(AdapterError::UnsupportedLegacyBlockType(block.tipo)),
            };

            blocos.push(converted);

            for legacy_source in block.fontes {
                if let Some(converted_source) = convert_legacy_source(legacy_source) {
                    push_unique_source(fontes, seen_sources, converted_source);
                }
            }
        }
    }

    Ok(())
}

fn convert_legacy_source(src: LegacyNoteSource) -> Option<NoteSource> {
    let titulo = clean_non_empty(&src.fonte)?;
    let localizacao = if let Some(p) = NonZeroU64::new(src.pagina) {
        Some(SourceLocation::Pdf { pagina: p })
    } else if let Some(sec) = src.referencia.as_deref().and_then(clean_non_empty) {
        Some(SourceLocation::Secao { referencia: sec })
    } else if let Some(url) = src.url.as_deref().and_then(clean_non_empty) {
        Some(SourceLocation::Url { href: url })
    } else {
        src.timestamp
            .as_deref()
            .and_then(clean_non_empty)
            .map(|ts| SourceLocation::Timestamp { marca: ts })
    };

    let trecho = src.trecho.as_deref().and_then(clean_non_empty);
    Some(NoteSource {
        titulo,
        localizacao,
        trecho,
    })
}

fn push_unique_source(
    fontes: &mut Vec<NoteSource>,
    seen: &mut HashSet<SourceDedupKey>,
    source: NoteSource,
) {
    let localizacao = match &source.localizacao {
        Some(SourceLocation::Pdf { pagina }) => Some(SourceLocationDedupKey::Pdf(pagina.get())),
        Some(SourceLocation::Secao { referencia }) => Some(SourceLocationDedupKey::Secao(
            referencia.trim().to_ascii_lowercase(),
        )),
        Some(SourceLocation::Url { href }) => Some(SourceLocationDedupKey::Url(
            href.trim().to_ascii_lowercase(),
        )),
        Some(SourceLocation::Timestamp { marca }) => Some(SourceLocationDedupKey::Timestamp(
            marca.trim().to_ascii_lowercase(),
        )),
        None => None,
    };

    let key = SourceDedupKey {
        titulo: source.titulo.trim().to_ascii_lowercase(),
        localizacao,
        trecho: source
            .trecho
            .as_deref()
            .map(|t| t.trim().to_ascii_lowercase()),
    };

    if seen.insert(key) {
        fontes.push(source);
    }
}

fn parse_legacy_figures(raw: &Value) -> Result<Vec<MediaFigure>, AdapterError> {
    match raw {
        Value::Object(_) => {
            let figure: MediaFigure = serde_json::from_value(raw.clone()).map_err(|err| {
                AdapterError::InvalidJsonField {
                    field: "figuras_notas",
                    detail: err.to_string(),
                }
            })?;
            Ok(vec![figure])
        }
        Value::Array(_) => {
            serde_json::from_value(raw.clone()).map_err(|err| AdapterError::InvalidJsonField {
                field: "figuras_notas",
                detail: err.to_string(),
            })
        }
        other => Err(AdapterError::InvalidJsonField {
            field: "figuras_notas",
            detail: format!(
                "esperado objeto MediaFigure ou array; recebido {}",
                json_type_name(other)
            ),
        }),
    }
}

fn register_figure_filename(seen_figures: &mut HashSet<String>, filename: &str) -> bool {
    let normalized = filename.trim().to_ascii_lowercase();
    !normalized.is_empty() && seen_figures.insert(normalized)
}

fn find_legacy_key(map: &Map<String, Value>) -> Option<&'static str> {
    LEGACY_EXTRA_NOTES_KEYS
        .iter()
        .copied()
        .find(|key| map.contains_key(*key))
}

fn find_populated_legacy_key(map: &Map<String, Value>) -> Option<&'static str> {
    LEGACY_EXTRA_NOTES_KEYS
        .iter()
        .copied()
        .find(|key| matches!(map.get(*key), Some(v) if !v.is_null()))
}

fn has_canonical_only_block(map: &Map<String, Value>) -> bool {
    let Some(Value::Array(blocos)) = map.get("blocos") else {
        return false;
    };
    blocos.iter().any(|b_val| {
        let Some(b) = b_val.as_object() else {
            return false;
        };
        let tipo = b.get("tipo").and_then(Value::as_str);
        matches!(tipo, Some("callout" | "chave_valor" | "figura"))
            || b.contains_key("colapsavel")
            || b.contains_key("ordenada")
            || b.contains_key("spec")
            || b.contains_key("fluxo")
            || b.contains_key("variante")
            || b.contains_key("rotulo")
            || b.contains_key("pares")
    })
}

fn has_legacy_only_block(map: &Map<String, Value>) -> bool {
    let Some(Value::Array(blocos)) = map.get("blocos") else {
        return false;
    };
    blocos.iter().any(|b_val| {
        let Some(b) = b_val.as_object() else {
            return false;
        };
        let tipo = b.get("tipo").and_then(Value::as_str);
        tipo == Some("alerta")
            || b.contains_key("fontes")
            || b.contains_key("fluxograma")
            || (tipo == Some("tabela") && b.contains_key("tabela"))
    })
}

fn is_populated(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(Value::Array(arr)) => !arr.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        Some(_) => true,
    }
}

fn clean_non_empty(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_compat_unifies_all_five_paths_and_deduplicates() {
        let notas = json!({
            "fisiopatologia": "Aumento da RVP.",
            "armadilhas_diferenciais": "Não reduzir PA > 25% na 1ª hora.",
            "blocos": [
                {
                    "tipo": "fluxograma",
                    "titulo": "Triagem",
                    "fluxograma": {
                        "inicio": "n1",
                        "nos": [
                            { "id": "n1", "texto": "Suspeita" },
                            { "id": "n2", "texto": "Confirmar" }
                        ],
                        "arestas": [
                            { "origem": "n1", "destino": "n2", "rotulo": "Positivo" }
                        ]
                    },
                    "fontes": [
                        { "fonte": "Harrison", "pagina": 1820, "trecho": "Crise" },
                        { "fonte": " harrison ", "pagina": 1820, "trecho": "crise" }
                    ]
                }
            ]
        });
        let tab =
            json!({ "caption": "Drogas", "headers": ["Droga"], "rows": [["Nitroprussiato"]] });
        let figs = json!([
            { "filename": "ecg.png", "caption": "ECG 1" },
            { "filename": "ECG.PNG", "caption": "Duplicada" }
        ]);

        let compiled = compile_extra_notes(
            &DraftNotesInput {
                notas_adicionais: Some(&notas),
                tabela_notas: Some(&tab),
                tabelas_notas: None,
                figuras_notas: Some(&figs),
            },
            CompileMode::LegacyCompat,
        )
        .expect("compilação LegacyCompat deve passar");

        assert_eq!(compiled.doc.blocos.len(), 5);
        assert_eq!(compiled.doc.fontes.len(), 1);
        assert_eq!(compiled.rendered.table_count, 1);
        assert_eq!(compiled.rendered.figure_count, 1);
        assert!(!compiled.rendered.html.contains("<br"));
    }

    #[test]
    fn strict_v2_rejects_legacy_fields() {
        let tab = json!({ "caption": "", "headers": ["A"], "rows": [["1"]] });
        let err = normalize_to_document(
            &DraftNotesInput {
                tabela_notas: Some(&tab),
                ..Default::default()
            },
            CompileMode::StrictV2,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            AdapterError::LegacyFieldNotAllowedInStrictV2("tabela_notas")
        ));
    }

    #[test]
    fn legacy_compat_rejects_invalid_canonical_doc_without_fallback() {
        let bad_canonical = json!({
            "blocos": [
                {
                    "tipo": "paragrafo",
                    "texto": "  "
                }
            ]
        });
        let err = normalize_to_document(
            &DraftNotesInput {
                notas_adicionais: Some(&bad_canonical),
                ..Default::default()
            },
            CompileMode::LegacyCompat,
        );
        assert!(
            err.is_err(),
            "deve rejeitar documento canônico inválido em vez de ignorar"
        );
    }

    #[test]
    fn rejects_competing_payloads_canonical_and_legacy() {
        let competing_fontes = json!({
            "fisiopatologia": "Mecanismo X",
            "fontes": [{ "titulo": "Harrison" }]
        });
        let err = normalize_to_document(
            &DraftNotesInput {
                notas_adicionais: Some(&competing_fontes),
                ..Default::default()
            },
            CompileMode::LegacyCompat,
        );
        assert!(matches!(err, Err(AdapterError::CompetingPayloads(_))));

        let competing_blocks = json!({
            "fisiopatologia": "Mecanismo X",
            "blocos": [{ "tipo": "callout", "variante": "pearl", "texto": "Y" }]
        });
        let err = normalize_to_document(
            &DraftNotesInput {
                notas_adicionais: Some(&competing_blocks),
                ..Default::default()
            },
            CompileMode::LegacyCompat,
        );
        assert!(matches!(err, Err(AdapterError::CompetingPayloads(_))));
    }

    #[test]
    fn legacy_compat_rejects_unknown_fields_in_notas_adicionais() {
        let typo = json!({
            "blocks": [{ "tipo": "paragrafo", "texto": "Conteúdo perdido" }]
        });
        let err = normalize_to_document(
            &DraftNotesInput {
                notas_adicionais: Some(&typo),
                ..Default::default()
            },
            CompileMode::LegacyCompat,
        );
        assert!(matches!(err, Err(AdapterError::InvalidJsonField { .. })));
    }

    #[test]
    fn retains_all_source_locations_in_dedup() {
        let input = json!({
            "blocos": [
                {
                    "tipo": "paragrafo",
                    "texto": "Passo 1",
                    "fontes": [
                        { "fonte": "Diretriz SBC", "pagina": 0, "referencia": "Cap. 1", "trecho": "Ref A" },
                        { "fonte": "Diretriz SBC", "pagina": 0, "referencia": "Cap. 2", "trecho": "Ref B" },
                        { "fonte": "Diretriz SBC", "pagina": 0, "referencia": "cap. 1", "trecho": "ref a" }
                    ]
                }
            ]
        });

        let doc = normalize_to_document(
            &DraftNotesInput {
                notas_adicionais: Some(&input),
                ..Default::default()
            },
            CompileMode::LegacyCompat,
        )
        .unwrap();

        assert_eq!(doc.fontes.len(), 2);
    }
}
