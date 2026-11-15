use serde_json::{Map, Value};

pub fn normalize_draft(input: &Value) -> Result<Value, String> {
    let obj = input
        .as_object()
        .ok_or_else(|| "CardDraft input must be a JSON object".to_string())?;

    let mut out = Map::new();

    // 1. id: str | None = None
    let id_val = match obj.get("id") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(s)) => Value::String(s.clone()),
        Some(_) => return Err("Field 'id' must be a string or null".to_string()),
    };
    out.insert("id".to_string(), id_val);

    // 2. tipo: Literal["QA", "CLOZE"] (alias: note_type)
    let tipo_str = if let Some(v) = obj.get("tipo") {
        let s = v
            .as_str()
            .ok_or_else(|| "Field 'tipo' must be a string".to_string())?;
        let upper = s.trim().to_uppercase();
        if upper == "CLOZE" || upper == "CL" {
            "CLOZE"
        } else {
            "QA"
        }
    } else if let Some(v) = obj.get("note_type") {
        let raw = match v {
            Value::Null => "",
            Value::String(s) => s.as_str(),
            _ => return Err("Field 'note_type' must be a string or null".to_string()),
        };
        let upper = raw.trim().to_uppercase();
        if upper == "CLOZE" || upper == "CL" {
            "CLOZE"
        } else {
            "QA"
        }
    } else {
        return Err("Missing required field 'tipo'".to_string());
    };
    out.insert("tipo".to_string(), Value::String(tipo_str.to_string()));

    // 3. enunciado_ou_sentenca: str (aliases: pergunta, sentenca, enunciado)
    let enunciado_val = obj
        .get("enunciado_ou_sentenca")
        .or_else(|| obj.get("pergunta"))
        .or_else(|| obj.get("sentenca"))
        .or_else(|| obj.get("enunciado"))
        .ok_or_else(|| "Missing required field 'enunciado_ou_sentenca'".to_string())?;
    let enunciado_str = enunciado_val
        .as_str()
        .ok_or_else(|| "Field 'enunciado_ou_sentenca' must be a string".to_string())?;
    out.insert(
        "enunciado_ou_sentenca".to_string(),
        Value::String(enunciado_str.to_string()),
    );

    // 4. resposta: str | None = None
    let resposta_val = match obj.get("resposta") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(s)) => Value::String(s.clone()),
        Some(_) => return Err("Field 'resposta' must be a string or null".to_string()),
    };
    out.insert("resposta".to_string(), resposta_val);

    // 5. campo_estudo: str = ""
    let campo_val = if let Some(v) = obj.get("campo_estudo") {
        v.as_str()
            .ok_or_else(|| "Field 'campo_estudo' must be a string".to_string())?
            .to_string()
    } else {
        let mut found = String::new();
        for alias in [
            "campo_de_estudo",
            "campo",
            "area",
            "study_field",
            "field_of_study",
            "especialidade",
        ] {
            if let Some(v) = obj.get(alias)
                && is_truthy_json(v)
            {
                found = v
                    .as_str()
                    .ok_or_else(|| format!("Field '{alias}' must be a string"))?
                    .to_string();
                break;
            }
        }
        found
    };
    out.insert("campo_estudo".to_string(), Value::String(campo_val));

    // 6. notas_adicionais: ExtraNotes | str = Field(default_factory=ExtraNotes) (alias: notas)
    let notas_raw = obj.get("notas_adicionais").or_else(|| obj.get("notas"));
    let notas_val = match notas_raw {
        None => default_extra_notes(),
        Some(Value::String(s)) => Value::String(s.clone()),
        Some(Value::Object(m)) => normalize_extra_notes(m)?,
        Some(_) => return Err("Field 'notas_adicionais' must be an object or string".to_string()),
    };
    out.insert("notas_adicionais".to_string(), notas_val);

    // 7. tabela_notas: TableSpec | dict[str, Any] | None = None
    let tabela_val = match obj.get("tabela_notas") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::Object(m)) => normalize_table_spec(m)?,
        Some(_) => return Err("Field 'tabela_notas' must be an object or null".to_string()),
    };
    out.insert("tabela_notas".to_string(), tabela_val);

    // 8. figura: MediaFigure | None = None
    let figura_val = match obj.get("figura") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::Object(m)) => normalize_media_figure(m)?,
        Some(_) => return Err("Field 'figura' must be an object or null".to_string()),
    };
    out.insert("figura".to_string(), figura_val);

    // 9. figuras: list[MediaFigure] = Field(default_factory=list)
    let mut figuras_vec: Vec<Value> = match obj.get("figuras") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(arr)) => {
            let mut out_arr = Vec::with_capacity(arr.len());
            for item in arr {
                let m = item
                    .as_object()
                    .ok_or_else(|| "Each item in 'figuras' must be an object".to_string())?;
                out_arr.push(normalize_media_figure(m)?);
            }
            out_arr
        }
        Some(_) => return Err("Field 'figuras' must be an array".to_string()),
    };

    for img_alias in ["images", "imagens_relacionadas", "imagens"] {
        if let Some(raw_imgs) = obj.get(img_alias) {
            let parsed = crate::image_alias::parse_images_html_to_figures(raw_imgs);
            for fig in parsed {
                if let Some(m) = fig.as_object() {
                    figuras_vec.push(normalize_media_figure(m)?);
                } else {
                    figuras_vec.push(fig);
                }
            }
        }
    }
    out.insert("figuras".to_string(), Value::Array(figuras_vec));

    // 10. referencias: list[str] | str = Field(default_factory=list) (alias: fonte)
    let refs_raw = obj.get("referencias").or_else(|| obj.get("fonte"));
    let refs_val = normalize_str_or_str_list(refs_raw, "referencias")?;
    out.insert("referencias".to_string(), refs_val);

    // 11. etiquetas: list[str] | str = Field(default_factory=list) (alias: tags)
    let tags_raw = obj.get("etiquetas").or_else(|| obj.get("tags"));
    let tags_val = normalize_str_or_str_list(tags_raw, "etiquetas")?;
    out.insert("etiquetas".to_string(), tags_val);

    Ok(Value::Object(out))
}

fn is_truthy_json(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(m) => !m.is_empty(),
    }
}

fn default_extra_notes() -> Value {
    let mut m = Map::new();
    for field in [
        "fisiopatologia",
        "raciocinio_clinico",
        "armadilhas_diferenciais",
        "correlacao_visual",
        "tabela",
        "texto_livre",
    ] {
        m.insert(field.to_string(), Value::Null);
    }
    m.insert("blocos".to_string(), Value::Array(Vec::new()));
    Value::Object(m)
}

fn normalize_extra_notes(input: &Map<String, Value>) -> Result<Value, String> {
    let mut m = default_extra_notes().as_object().cloned().expect("object");
    for field in [
        "fisiopatologia",
        "raciocinio_clinico",
        "armadilhas_diferenciais",
        "correlacao_visual",
        "texto_livre",
    ] {
        if let Some(value) = input.get(field) {
            if !value.is_null() && !value.is_string() {
                return Err(format!(
                    "Field 'notas_adicionais.{field}' must be a string or null"
                ));
            }
            m.insert(field.to_string(), value.clone());
        }
    }
    if let Some(value) = input.get("tabela") {
        m.insert(
            "tabela".to_string(),
            if let Some(table) = value.as_object() {
                normalize_table_spec(table)?
            } else if value.is_null() {
                Value::Null
            } else {
                return Err("Field 'notas_adicionais.tabela' must be an object or null".to_string());
            },
        );
    }
    if let Some(value) = input.get("blocos") {
        let blocks = value
            .as_array()
            .ok_or_else(|| "Field 'notas_adicionais.blocos' must be an array".to_string())?;
        let normalized = blocks
            .iter()
            .map(normalize_note_block)
            .collect::<Result<Vec<_>, _>>()?;
        m.insert("blocos".to_string(), Value::Array(normalized));
    }
    Ok(Value::Object(m))
}

fn normalize_note_block(value: &Value) -> Result<Value, String> {
    let input = value
        .as_object()
        .ok_or_else(|| "Each item in 'blocos' must be an object".to_string())?;
    let tipo = input
        .get("tipo")
        .and_then(Value::as_str)
        .ok_or_else(|| "NoteBlock.tipo is required".to_string())?;
    if !["paragrafo", "lista", "alerta", "tabela", "fluxograma"].contains(&tipo) {
        return Err(format!("Unsupported NoteBlock tipo: {tipo}"));
    }
    let mut block = Map::new();
    block.insert("tipo".to_string(), Value::String(tipo.to_string()));
    for key in ["titulo", "texto"] {
        let val = input
            .get(key)
            .cloned()
            .unwrap_or_else(|| Value::String(String::new()));
        if !val.is_string() {
            return Err(format!("NoteBlock.{key} must be a string"));
        }
        block.insert(key.to_string(), val);
    }
    let itens = input.get("itens").cloned().unwrap_or_else(json_array);
    if !itens
        .as_array()
        .is_some_and(|v| v.iter().all(Value::is_string))
    {
        return Err("NoteBlock.itens must be a string array".to_string());
    }
    block.insert("itens".to_string(), itens);
    for key in ["tabela", "fluxograma"] {
        let val = match input.get(key) {
            None | Some(Value::Null) => Value::Null,
            Some(Value::Object(obj)) if key == "tabela" => normalize_table_spec(obj)?,
            Some(Value::Object(obj)) => normalize_note_flow(obj)?,
            _ => return Err(format!("NoteBlock.{key} must be an object or null")),
        };
        block.insert(key.to_string(), val);
    }
    let sources = input
        .get("fontes")
        .and_then(Value::as_array)
        .ok_or_else(|| "NoteBlock.fontes must be an array".to_string())?;
    if sources.is_empty() {
        return Err("NoteBlock.fontes must contain at least one source".to_string());
    }
    block.insert(
        "fontes".to_string(),
        Value::Array(
            sources
                .iter()
                .map(normalize_note_source)
                .collect::<Result<_, _>>()?,
        ),
    );
    let text = block["texto"].as_str().unwrap_or("");
    let items = block["itens"].as_array().expect("normalized items");
    let table = !block["tabela"].is_null();
    let flow = !block["fluxograma"].is_null();
    let valid = match tipo {
        "paragrafo" | "alerta" => !text.trim().is_empty() && items.is_empty() && !table && !flow,
        "lista" => {
            !items.is_empty()
                && items
                    .iter()
                    .all(|x| x.as_str().is_some_and(|s| !s.trim().is_empty()))
                && text.is_empty()
                && !table
                && !flow
        }
        "tabela" => table && !flow && text.is_empty() && items.is_empty(),
        "fluxograma" => flow && !table && text.is_empty() && items.is_empty(),
        _ => false,
    };
    if !valid {
        return Err(
            "Block content must match tipo; empty or competing payloads are invalid".to_string(),
        );
    }
    Ok(Value::Object(block))
}

fn json_array() -> Value {
    Value::Array(Vec::new())
}

fn normalize_note_source(value: &Value) -> Result<Value, String> {
    let input = value.as_object().ok_or("NoteSource must be an object")?;
    let mut out = Map::new();
    for key in ["fonte", "trecho"] {
        let text = input
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("NoteSource.{key} is required"))?
            .trim();
        if text.is_empty() {
            return Err(format!("NoteSource.{key} must not be empty"));
        }
        out.insert(key.to_string(), Value::String(text.to_string()));
    }
    let page = input
        .get("pagina")
        .and_then(Value::as_u64)
        .filter(|&n| n >= 1)
        .ok_or("NoteSource.pagina must be >= 1")?;
    out.insert("pagina".to_string(), Value::from(page));
    Ok(Value::Object(out))
}

fn normalize_note_flow(input: &Map<String, Value>) -> Result<Value, String> {
    let start = input
        .get("inicio")
        .and_then(Value::as_str)
        .ok_or("NoteFlow.inicio is required")?;
    let nodes = input
        .get("nos")
        .and_then(Value::as_array)
        .ok_or("NoteFlow.nos is required")?;
    let edges = input.get("arestas").cloned().unwrap_or_else(json_array);
    if !edges.is_array() {
        return Err("NoteFlow.arestas must be an array".to_string());
    }
    if nodes.is_empty() {
        return Err("NoteFlow.nos must not be empty".to_string());
    }
    let mut ids = std::collections::HashSet::new();
    let normalized_nodes = nodes
        .iter()
        .map(|node| {
            let m = node.as_object().ok_or("FlowNode must be an object")?;
            let id = m
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or("FlowNode.id is required")?;
            let text = m
                .get("texto")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or("FlowNode.texto is required")?;
            if !ids.insert(id.to_string()) {
                return Err("Duplicate flow node IDs".to_string());
            }
            Ok(serde_json::json!({"id": id, "texto": text}))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if !ids.contains(start) {
        return Err("Flow requires unique node IDs and a valid inicio".to_string());
    }
    let mut edge_set = std::collections::HashSet::new();
    let normalized_edges = edges
        .as_array()
        .expect("checked array")
        .iter()
        .map(|edge| {
            let m = edge.as_object().ok_or("FlowEdge must be an object")?;
            let from = m
                .get("origem")
                .and_then(Value::as_str)
                .ok_or("FlowEdge.origem is required")?;
            let to = m
                .get("destino")
                .and_then(Value::as_str)
                .ok_or("FlowEdge.destino is required")?;
            let label = m
                .get("rotulo")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or("FlowEdge.rotulo is required")?;
            if !ids.contains(from) || !ids.contains(to) {
                return Err("Flow edge refers to a missing node".to_string());
            }
            if !edge_set.insert((from.to_string(), to.to_string(), label.to_string())) {
                return Err("Duplicate flow edges".to_string());
            }
            Ok(serde_json::json!({"origem": from, "destino": to, "rotulo": label}))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut reached = std::collections::HashSet::from([start.to_string()]);
    loop {
        let prior = reached.len();
        for edge in &normalized_edges {
            if reached.contains(edge["origem"].as_str().unwrap_or("")) {
                reached.insert(edge["destino"].as_str().unwrap_or("").to_string());
            }
        }
        if prior == reached.len() {
            break;
        }
    }
    if reached != ids {
        return Err("Every flow node must be reachable from inicio".to_string());
    }
    let mut out = Map::new();
    out.insert("inicio".to_string(), Value::String(start.to_string()));
    out.insert("nos".to_string(), Value::Array(normalized_nodes));
    out.insert("arestas".to_string(), Value::Array(normalized_edges));
    Ok(Value::Object(out))
}

fn normalize_table_spec(input: &Map<String, Value>) -> Result<Value, String> {
    let caption = match input.get("caption") {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        _ => return Err("TableSpec.caption must be a string".to_string()),
    };
    let headers = input
        .get("headers")
        .and_then(Value::as_array)
        .ok_or_else(|| "TableSpec.headers must be an array".to_string())?;
    if headers.is_empty() || !headers.iter().all(Value::is_string) {
        return Err("TableSpec.headers must be a non-empty string array".to_string());
    }
    let rows = input
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| "TableSpec.rows must be an array".to_string())?;
    if rows.iter().any(|row| {
        !row.as_array()
            .is_some_and(|cells| cells.len() == headers.len() && cells.iter().all(Value::is_string))
    }) {
        return Err("TableSpec.rows must match the headers column count".to_string());
    }
    Ok(serde_json::json!({"caption": caption, "headers": headers, "rows": rows}))
}

fn normalize_media_figure(input: &Map<String, Value>) -> Result<Value, String> {
    let mut m = Map::new();
    for field in [
        "filename",
        "caption",
        "title",
        "context_link",
        "item_type",
        "style",
        "caption_style",
    ] {
        let value = input.get(field);
        let string = match value {
            Some(Value::String(s)) => s.clone(),
            None if field == "filename" => {
                return Err("Field 'MediaFigure.filename' is required".to_string());
            }
            None if field == "item_type" => "figure".to_string(),
            None => String::new(),
            _ => return Err(format!("Field 'MediaFigure.{field}' must be a string")),
        };
        if field == "item_type" && string != "figure" && string != "table" {
            return Err("Field 'MediaFigure.item_type' must be figure or table".to_string());
        }
        m.insert(field.to_string(), Value::String(string));
    }
    Ok(Value::Object(m))
}

fn normalize_str_or_str_list(raw: Option<&Value>, field: &str) -> Result<Value, String> {
    match raw {
        None => Ok(Value::Array(Vec::new())),
        Some(Value::String(s)) => Ok(Value::String(s.clone())),
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                let s = item
                    .as_str()
                    .ok_or_else(|| format!("Each item in '{field}' must be a string"))?;
                out.push(Value::String(s.to_string()));
            }
            Ok(Value::Array(out))
        }
        Some(_) => Err(format!(
            "Field '{field}' must be a string or list of strings"
        )),
    }
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
}
