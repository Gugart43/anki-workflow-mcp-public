use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};

pub fn patch_draft(draft: &Value, updates: &Map<String, Value>) -> Result<Value, String> {
    let mut card_val = crate::draft_normalize::normalize_draft(draft)?;
    let card_obj = card_val
        .as_object_mut()
        .ok_or_else(|| "Normalized draft must be a JSON object".to_string())?;

    for (k, v) in updates {
        card_obj.insert(k.clone(), v.clone());
    }

    crate::draft_normalize::normalize_draft(&card_val)
}

pub fn partition_quarantine(
    drafts: &[Value],
    failed_indices: &[usize],
    diagnostics: &[Value],
    reason: &str,
) -> Result<(Vec<Value>, Vec<Value>), String> {
    for (idx, draft) in drafts.iter().enumerate() {
        if !draft.is_object() {
            return Err(format!("Draft at index {idx} must be a JSON object"));
        }
    }

    let failed_set: HashSet<usize> = failed_indices.iter().copied().collect();
    let mut diags_map: HashMap<usize, Vec<Value>> = HashMap::new();

    for d in diagnostics {
        if let Some(row_val) = d.get("row")
            && let Some(row_i64) = row_val.as_i64()
            && row_i64 >= 1
        {
            let target_idx = (row_i64 - 1) as usize;
            diags_map.entry(target_idx).or_default().push(d.clone());
        }
    }

    let mut valid = Vec::new();
    let mut quarantined = Vec::new();

    for (idx, card) in drafts.iter().enumerate() {
        let normalized = crate::draft_normalize::normalize_draft(card)?;
        if failed_set.contains(&idx) {
            let card_diags = diags_map.remove(&idx).unwrap_or_default();
            let record = json!({
                "card_index": idx,
                "card": normalized,
                "diagnostics": card_diags,
                "reason": reason,
                "timestamp": "",
            });
            quarantined.push(record);
        } else {
            valid.push(normalized);
        }
    }

    Ok((valid, quarantined))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_partition_quarantine_empty_inputs() {
        let (valid, quarantined) =
            partition_quarantine(&[], &[], &[], "No failures").expect("empty should succeed");
        assert!(valid.is_empty());
        assert!(quarantined.is_empty());
    }

    #[test]
    fn test_partition_quarantine_row_zero_and_out_of_range() {
        let drafts = vec![
            json!({"tipo": "QA", "enunciado_ou_sentenca": "Q1", "resposta": "R1"}),
            json!({"tipo": "QA", "enunciado_ou_sentenca": "Q2", "resposta": "R2"}),
        ];
        let failed = vec![1, 99];
        let diagnostics = vec![
            json!({"row": 0, "msg": "underflow guard"}),
            json!({"row": -3, "msg": "negative row"}),
            json!({"row": 2, "msg": "first error"}),
            json!({"row": 2, "msg": "second error"}),
        ];

        let (valid, quarantined) =
            partition_quarantine(&drafts, &failed, &diagnostics, "Budget exceeded")
                .expect("should partition");

        assert_eq!(valid.len(), 1);
        assert_eq!(valid[0]["enunciado_ou_sentenca"], "Q1");

        assert_eq!(quarantined.len(), 1);
        assert_eq!(quarantined[0]["card_index"], 1);
        assert_eq!(quarantined[0]["card"]["enunciado_ou_sentenca"], "Q2");
        assert_eq!(quarantined[0]["reason"], "Budget exceeded");
        assert_eq!(quarantined[0]["timestamp"], "");

        let diags = quarantined[0]["diagnostics"].as_array().unwrap();
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0]["msg"], "first error");
        assert_eq!(diags[1]["msg"], "second error");
    }

    #[test]
    fn test_partition_quarantine_invalid_draft_returns_err() {
        let drafts = vec![json!("not_an_object")];
        let err = partition_quarantine(&drafts, &[0], &[], "reason").unwrap_err();
        assert!(err.contains("Draft at index 0"));
    }

    #[test]
    fn test_patch_draft_invalid_draft_returns_err() {
        let invalid = json!(12345);
        let updates = Map::new();
        let res = patch_draft(&invalid, &updates);
        assert!(res.is_err());
    }
}
