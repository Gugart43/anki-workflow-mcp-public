use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub enum CardTipo {
    QA,
    CLOZE,
}

impl<'de> Deserialize<'de> for CardTipo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        match raw.trim().to_uppercase().as_str() {
            "QA" => Ok(Self::QA),
            "CLOZE" | "CL" => Ok(Self::CLOZE),
            _ => Err(serde::de::Error::custom(format!(
                "Unsupported draft tipo: '{raw}' (expected 'QA' or 'CLOZE')"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum StringOrVec {
    One(String),
    Many(Vec<String>),
}

impl Default for StringOrVec {
    fn default() -> Self {
        Self::Many(Vec::new())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RawTableSpec {
    #[serde(default)]
    pub caption: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl RawTableSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.headers.is_empty() {
            return Err("TableSpec.headers must be a non-empty string array".to_string());
        }
        if self.rows.iter().any(|row| row.len() != self.headers.len()) {
            return Err("TableSpec.rows must match the headers column count".to_string());
        }
        Ok(())
    }
}

fn default_item_type() -> String {
    "figure".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RawMediaFigure {
    pub filename: String,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub context_link: String,
    #[serde(default = "default_item_type")]
    pub item_type: String,
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub caption_style: String,
}

impl RawMediaFigure {
    pub fn validate(&self) -> Result<(), String> {
        if self.item_type != "figure" && self.item_type != "table" {
            return Err("Field 'MediaFigure.item_type' must be figure or table".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum OneOrManyFigures {
    One(RawMediaFigure),
    Many(Vec<RawMediaFigure>),
}

impl OneOrManyFigures {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::One(fig) => fig.validate(),
            Self::Many(figs) => figs.iter().try_for_each(RawMediaFigure::validate),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RawNoteSource {
    pub fonte: String,
    pub trecho: String,
    pub pagina: std::num::NonZeroU64,
}

impl RawNoteSource {
    pub fn normalize(&mut self) -> Result<(), String> {
        self.fonte = self.fonte.trim().to_string();
        if self.fonte.is_empty() {
            return Err("NoteSource.fonte must not be empty".to_string());
        }
        self.trecho = self.trecho.trim().to_string();
        if self.trecho.is_empty() {
            return Err("NoteSource.trecho must not be empty".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RawFlowNode {
    pub id: String,
    pub texto: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RawFlowEdge {
    pub origem: String,
    pub destino: String,
    pub rotulo: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RawNoteFlow {
    pub inicio: String,
    pub nos: Vec<RawFlowNode>,
    #[serde(default)]
    pub arestas: Vec<RawFlowEdge>,
}

impl RawNoteFlow {
    pub fn validate(&self) -> Result<(), String> {
        if self.nos.is_empty() {
            return Err("NoteFlow.nos must not be empty".to_string());
        }
        let mut ids = HashSet::with_capacity(self.nos.len());
        for node in &self.nos {
            if node.id.is_empty() {
                return Err("FlowNode.id is required".to_string());
            }
            if node.texto.is_empty() {
                return Err("FlowNode.texto is required".to_string());
            }
            if !ids.insert(node.id.as_str()) {
                return Err("Duplicate flow node IDs".to_string());
            }
        }
        if !ids.contains(self.inicio.as_str()) {
            return Err("Flow requires unique node IDs and a valid inicio".to_string());
        }

        let mut edge_set = HashSet::with_capacity(self.arestas.len());
        for edge in &self.arestas {
            if edge.rotulo.is_empty() {
                return Err("FlowEdge.rotulo is required".to_string());
            }
            if !ids.contains(edge.origem.as_str()) || !ids.contains(edge.destino.as_str()) {
                return Err("Flow edge refers to a missing node".to_string());
            }
            if !edge_set.insert((
                edge.origem.as_str(),
                edge.destino.as_str(),
                edge.rotulo.as_str(),
            )) {
                return Err("Duplicate flow edges".to_string());
            }
        }

        let mut reached = HashSet::from([self.inicio.as_str()]);
        loop {
            let prior = reached.len();
            for edge in &self.arestas {
                if reached.contains(edge.origem.as_str()) {
                    reached.insert(edge.destino.as_str());
                }
            }
            if prior == reached.len() {
                break;
            }
        }
        if reached != ids {
            return Err("Every flow node must be reachable from inicio".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NoteBlockKind {
    Paragrafo,
    Lista,
    Alerta,
    Tabela,
    Fluxograma,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawNoteBlock {
    pub tipo: NoteBlockKind,
    #[serde(default)]
    pub titulo: String,
    #[serde(default)]
    pub texto: String,
    #[serde(default)]
    pub itens: Vec<String>,
    #[serde(default)]
    pub tabela: Option<RawTableSpec>,
    #[serde(default)]
    pub fluxograma: Option<RawNoteFlow>,
    pub fontes: Vec<RawNoteSource>,
}

impl RawNoteBlock {
    pub fn normalize(&mut self) -> Result<(), String> {
        if let Some(table) = &self.tabela {
            table.validate()?;
        }
        if let Some(flow) = &self.fluxograma {
            flow.validate()?;
        }
        if self.fontes.is_empty() {
            return Err("NoteBlock.fontes must contain at least one source".to_string());
        }
        for source in &mut self.fontes {
            source.normalize()?;
        }
        let has_table = self.tabela.is_some();
        let has_flow = self.fluxograma.is_some();
        let valid = match self.tipo {
            NoteBlockKind::Paragrafo | NoteBlockKind::Alerta => {
                !self.texto.trim().is_empty() && self.itens.is_empty() && !has_table && !has_flow
            }
            NoteBlockKind::Lista => {
                !self.itens.is_empty()
                    && self.itens.iter().all(|s| !s.trim().is_empty())
                    && self.texto.is_empty()
                    && !has_table
                    && !has_flow
            }
            NoteBlockKind::Tabela => {
                has_table && !has_flow && self.texto.is_empty() && self.itens.is_empty()
            }
            NoteBlockKind::Fluxograma => {
                has_flow && !has_table && self.texto.is_empty() && self.itens.is_empty()
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawExtraNotes {
    #[serde(default)]
    pub fisiopatologia: Option<String>,
    #[serde(default)]
    pub raciocinio_clinico: Option<String>,
    #[serde(default)]
    pub armadilhas_diferenciais: Option<String>,
    #[serde(default)]
    pub correlacao_visual: Option<String>,
    #[serde(default)]
    pub tabela: Option<RawTableSpec>,
    #[serde(default)]
    pub texto_livre: Option<String>,
    #[serde(default)]
    pub blocos: Vec<RawNoteBlock>,
}

impl RawExtraNotes {
    pub fn normalize(&mut self) -> Result<(), String> {
        if let Some(table) = &self.tabela {
            table.validate()?;
        }
        for block in &mut self.blocos {
            block.normalize()?;
        }
        Ok(())
    }
}

const LEGACY_EXTRA_NOTES_KEYS: &[&str] = &[
    "fisiopatologia",
    "raciocinio_clinico",
    "armadilhas_diferenciais",
    "correlacao_visual",
    "texto_livre",
    "tabela",
];

fn has_populated_legacy_key(map: &serde_json::Map<String, Value>) -> bool {
    LEGACY_EXTRA_NOTES_KEYS
        .iter()
        .any(|k| matches!(map.get(*k), Some(v) if !v.is_null()))
}

fn has_any_legacy_key(map: &serde_json::Map<String, Value>) -> bool {
    map.keys()
        .any(|k| LEGACY_EXTRA_NOTES_KEYS.contains(&k.as_str()))
}

fn has_canonical_only_block(map: &serde_json::Map<String, Value>) -> bool {
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

fn has_legacy_only_block(map: &serde_json::Map<String, Value>) -> bool {
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
            || (b.contains_key("itens") && b.contains_key("texto"))
            || (tipo == Some("tabela") && b.contains_key("tabela"))
    })
}

#[derive(Debug, Clone, PartialEq, Eq, JsonSchema)]
#[serde(untagged)]
pub enum RawExtraNotesInput {
    Text(String),
    Canonical(crate::note_model::NoteDocument),
    Legacy(RawExtraNotes),
}

impl Default for RawExtraNotesInput {
    fn default() -> Self {
        Self::Legacy(RawExtraNotes::default())
    }
}

impl Serialize for RawExtraNotesInput {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Text(text) => text.serialize(serializer),
            Self::Canonical(doc) => doc.serialize(serializer),
            Self::Legacy(notes) => notes.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for RawExtraNotesInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let val = Value::deserialize(deserializer)?;
        match val {
            Value::String(s) => Ok(Self::Text(s)),
            Value::Object(map) => {
                let has_top_fontes = map.contains_key("fontes");
                let has_canonical_blocks = has_canonical_only_block(&map);
                let has_legacy_blocks = has_legacy_only_block(&map);
                let blocos_non_empty =
                    matches!(map.get("blocos"), Some(Value::Array(arr)) if !arr.is_empty());
                if (has_populated_legacy_key(&map)
                    && (has_top_fontes
                        || has_canonical_blocks
                        || (blocos_non_empty && !has_legacy_blocks)))
                    || (has_legacy_blocks && has_top_fontes)
                {
                    return Err(serde::de::Error::custom(
                        "Field 'notas_adicionais' cannot mix canonical fields ('fontes'/v2 blocks) with legacy fields",
                    ));
                }

                if has_any_legacy_key(&map)
                    || has_legacy_blocks
                    || (!map.contains_key("blocos") && !has_top_fontes)
                {
                    let legacy: RawExtraNotes = serde_json::from_value(Value::Object(map))
                        .map_err(serde::de::Error::custom)?;
                    Ok(Self::Legacy(legacy))
                } else {
                    let doc: crate::note_model::NoteDocument =
                        serde_json::from_value(Value::Object(map))
                            .map_err(serde::de::Error::custom)?;
                    doc.validate().map_err(serde::de::Error::custom)?;
                    Ok(Self::Canonical(doc))
                }
            }
            Value::Null => Ok(Self::Legacy(RawExtraNotes::default())),
            other => Err(serde::de::Error::custom(format!(
                "Field 'notas_adicionais' must be an object, string, or null (received {})",
                json_type_name(&other)
            ))),
        }
    }
}

fn json_type_name(val: &Value) -> &'static str {
    match val {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    T: Default + Deserialize<'de>,
    D: Deserializer<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RawCardDraft {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(alias = "note_type")]
    pub tipo: CardTipo,
    #[serde(alias = "pergunta", alias = "sentenca", alias = "enunciado")]
    pub enunciado_ou_sentenca: String,
    #[serde(default)]
    pub resposta: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        alias = "campo_de_estudo",
        alias = "campo",
        alias = "area",
        alias = "study_field",
        alias = "field_of_study",
        alias = "especialidade"
    )]
    pub campo_estudo: String,
    #[serde(default, alias = "notas")]
    pub notas_adicionais: RawExtraNotesInput,
    #[serde(default)]
    pub tabela_notas: Option<RawTableSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tabelas_notas: Option<Vec<RawTableSpec>>,
    #[serde(default)]
    pub figura: Option<RawMediaFigure>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub figuras: Vec<RawMediaFigure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub figuras_notas: Option<OneOrManyFigures>,
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        alias = "fonte"
    )]
    pub referencias: StringOrVec,
    #[serde(default, deserialize_with = "deserialize_null_default", alias = "tags")]
    pub etiquetas: StringOrVec,
    #[serde(default, skip_serializing)]
    #[schemars(skip)]
    pub images: Option<Value>,
    #[serde(default, skip_serializing)]
    #[schemars(skip)]
    pub imagens_relacionadas: Option<Value>,
    #[serde(default, skip_serializing)]
    #[schemars(skip)]
    pub imagens: Option<Value>,
}

impl RawCardDraft {
    pub fn normalize(mut self) -> Result<Self, String> {
        match &mut self.notas_adicionais {
            RawExtraNotesInput::Legacy(notes) => {
                notes.normalize()?;
            }
            RawExtraNotesInput::Canonical(doc) => {
                doc.validate().map_err(|e| e.to_string())?;
            }
            RawExtraNotesInput::Text(_) => {}
        }
        if let Some(table) = &self.tabela_notas {
            table.validate()?;
        }
        if let Some(tables) = &self.tabelas_notas {
            for table in tables {
                table.validate()?;
            }
        }
        if let Some(fig) = &self.figura {
            fig.validate()?;
        }
        for fig in &self.figuras {
            fig.validate()?;
        }
        for raw_imgs in [
            self.images.take(),
            self.imagens_relacionadas.take(),
            self.imagens.take(),
        ]
        .into_iter()
        .flatten()
        {
            for fig_val in crate::image_alias::parse_images_html_to_figures(&raw_imgs) {
                let fig: RawMediaFigure =
                    serde_json::from_value(fig_val).map_err(|e| e.to_string())?;
                fig.validate()?;
                self.figuras.push(fig);
            }
        }
        if let Some(note_figs) = &self.figuras_notas {
            note_figs.validate()?;
        }
        Ok(self)
    }
}

pub fn normalize_draft(input: &Value) -> Result<Value, String> {
    if !input.is_object() {
        return Err("CardDraft input must be a JSON object".to_string());
    }
    let raw: RawCardDraft = serde_json::from_value(input.clone()).map_err(|e| e.to_string())?;
    let normalized = raw.normalize()?;
    serde_json::to_value(normalized).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_minimal_aliases_and_defaults() {
        let input = json!({
            "note_type": "cl",
            "pergunta": "Qual o achado?",
            "notas": "Resumo",
            "fonte": ["Diretriz 2024"],
            "tags": ["cardio"],
            "especialidade": "Cardiologia",
            "unknown_extra_key": 123
        });

        let out = normalize_draft(&input).expect("should normalize");
        let obj = out.as_object().expect("object");
        assert_eq!(obj.get("tipo"), Some(&json!("CLOZE")));
        assert_eq!(
            obj.get("enunciado_ou_sentenca"),
            Some(&json!("Qual o achado?"))
        );
        assert_eq!(obj.get("notas_adicionais"), Some(&json!("Resumo")));
        assert_eq!(obj.get("referencias"), Some(&json!(["Diretriz 2024"])));
        assert_eq!(obj.get("etiquetas"), Some(&json!(["cardio"])));
        assert_eq!(obj.get("campo_estudo"), Some(&json!("Cardiologia")));
        assert_eq!(obj.get("id"), Some(&Value::Null));
        assert_eq!(obj.get("resposta"), Some(&Value::Null));
        assert!(!obj.contains_key("unknown_extra_key"));
    }

    #[test]
    fn reject_missing_required_fields() {
        let missing_tipo = json!({ "enunciado_ou_sentenca": "Texto" });
        assert!(normalize_draft(&missing_tipo).is_err());

        let missing_enunciado = json!({ "tipo": "QA" });
        assert!(normalize_draft(&missing_enunciado).is_err());

        let non_object = json!(["not", "an", "object"]);
        assert!(normalize_draft(&non_object).is_err());
    }

    #[test]
    fn reject_invalid_scalar_types() {
        let bad_id = json!({
            "id": 42,
            "tipo": "QA",
            "enunciado_ou_sentenca": "Pergunta?"
        });
        assert!(normalize_draft(&bad_id).is_err());

        let bad_refs = json!({
            "tipo": "QA",
            "enunciado_ou_sentenca": "Pergunta?",
            "referencias": [1, 2]
        });
        assert!(normalize_draft(&bad_refs).is_err());
    }

    #[test]
    fn nested_defaults_applied() {
        let input = json!({
            "tipo": "qa",
            "enunciado": "Pergunta?",
            "tabela_notas": { "headers": ["A"], "rows": [["1"]] },
            "figura": { "filename": "a.png" }
        });
        let out = normalize_draft(&input).expect("should normalize");
        assert_eq!(out["tipo"], json!("QA"));
        assert_eq!(out["notas_adicionais"]["fisiopatologia"], Value::Null);
        assert_eq!(out["notas_adicionais"]["blocos"], json!([]));
        assert_eq!(out["tabela_notas"]["caption"], json!(""));
        assert_eq!(out["figura"]["caption"], json!(""));
        assert_eq!(out["figura"]["item_type"], json!("figure"));
    }

    #[test]
    fn reject_invalid_draft_tipo() {
        let bad_tipo = json!({
            "tipo": "CLOZ",
            "enunciado_ou_sentenca": "Texto"
        });
        let err = normalize_draft(&bad_tipo).unwrap_err();
        assert!(err.contains("Unsupported draft tipo"));

        let bad_tipo2 = json!({
            "note_type": "INVALID",
            "enunciado_ou_sentenca": "Texto"
        });
        let err2 = normalize_draft(&bad_tipo2).unwrap_err();
        assert!(err2.contains("Unsupported draft tipo"));
    }
}
