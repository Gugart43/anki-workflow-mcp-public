//! Loader e gerenciador de perfis declarativos de domínio (`DomainProfile`).
//!
//! Permite desacoplar regras específicas de provas médicas brasileiras (bancas, limites,
//! políticas editoriais) do motor principal de validação de notas.

use std::sync::LazyLock;

use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::html_text::parse_html_text;

pub const MED_BR_PROFILE_JSON: &str = include_str!("../../../profiles/med_br.json");
pub const GENERAL_PROFILE_JSON: &str = include_str!("../../../profiles/general.json");

const LABELS_PLACEHOLDER: &str = "{LABELS}";

static SEGMENT_SPLIT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\n.!?]+").expect("valid SEGMENT_SPLIT_RE"));

static MED_BR_PROFILE: LazyLock<DomainProfile> = LazyLock::new(|| {
    DomainProfile::from_json_str(MED_BR_PROFILE_JSON)
        .expect("built-in profiles/med_br.json must be valid")
});

static GENERAL_PROFILE: LazyLock<DomainProfile> = LazyLock::new(|| {
    DomainProfile::from_json_str(GENERAL_PROFILE_JSON)
        .expect("built-in profiles/general.json must be valid")
});

#[derive(Debug, Error)]
pub enum DomainProfileError {
    #[error("failed to parse domain profile JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("failed to compile exam boilerplate regex for profile '{profile_id}': {source}")]
    InvalidExamRegex {
        profile_id: String,
        #[source]
        source: regex::Error,
    },

    #[error("unknown built-in domain profile: '{0}'")]
    UnknownProfile(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExamBoilerplateConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub known_labels: Vec<String>,
    #[serde(default)]
    pub phrase_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InfoPolicy {
    pub source_fidelity: String,
    pub notes_purpose: String,
    #[serde(default = "default_true")]
    pub forbid_metadata_headers: bool,
    #[serde(default = "default_true")]
    pub forbid_verbatim_answer_restatement: bool,
    #[serde(default)]
    pub forbid_exam_only_mentions: bool,
    pub preferred_list_format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EditorialChecklistItem {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "DomainProfileRaw")]
pub struct DomainProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub language: String,
    pub default_max_note_chars: usize,
    pub table_max_note_chars: usize,
    pub exam_boilerplate: ExamBoilerplateConfig,
    pub info_policy: InfoPolicy,
    #[serde(default)]
    pub editorial_checklist: Vec<EditorialChecklistItem>,
    #[serde(skip)]
    #[schemars(skip)]
    exam_regex: Option<Regex>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DomainProfileRaw {
    id: String,
    name: String,
    description: String,
    language: String,
    default_max_note_chars: usize,
    table_max_note_chars: usize,
    exam_boilerplate: ExamBoilerplateConfig,
    info_policy: InfoPolicy,
    #[serde(default)]
    editorial_checklist: Vec<EditorialChecklistItem>,
}

impl TryFrom<DomainProfileRaw> for DomainProfile {
    type Error = DomainProfileError;

    fn try_from(raw: DomainProfileRaw) -> Result<Self, Self::Error> {
        let exam_regex = compile_exam_boilerplate_regex(&raw.id, &raw.exam_boilerplate)?;
        Ok(Self {
            id: raw.id,
            name: raw.name,
            description: raw.description,
            language: raw.language,
            default_max_note_chars: raw.default_max_note_chars,
            table_max_note_chars: raw.table_max_note_chars,
            exam_boilerplate: raw.exam_boilerplate,
            info_policy: raw.info_policy,
            editorial_checklist: raw.editorial_checklist,
            exam_regex,
        })
    }
}

fn default_true() -> bool {
    true
}

impl DomainProfile {
    pub fn from_json_str(json: &str) -> Result<Self, DomainProfileError> {
        let raw: DomainProfileRaw = serde_json::from_str(json)?;
        Self::try_from(raw)
    }

    #[must_use]
    pub fn default_profile() -> &'static Self {
        Self::med_br()
    }

    #[must_use]
    pub fn med_br() -> &'static Self {
        &MED_BR_PROFILE
    }

    #[must_use]
    pub fn general() -> &'static Self {
        &GENERAL_PROFILE
    }

    pub fn by_name(name: &str) -> Result<&'static Self, DomainProfileError> {
        match name.trim().to_ascii_lowercase().as_str() {
            "med_br" | "med-br" | "default" => Ok(Self::med_br()),
            "general" | "neutral" => Ok(Self::general()),
            other => Err(DomainProfileError::UnknownProfile(other.to_string())),
        }
    }

    #[must_use]
    pub fn exam_boilerplate_regex(&self) -> Option<&Regex> {
        self.exam_regex.as_ref()
    }

    #[must_use]
    pub fn is_exam_only_note(&self, notes: &str) -> bool {
        if !self.info_policy.forbid_exam_only_mentions || !self.exam_boilerplate.enabled {
            return false;
        }

        let Some(re) = self.exam_boilerplate_regex() else {
            return false;
        };

        // Usa parse_html_text().0 para preservar quebras de linha (\n) entre <br>/<p>/<li>.
        let (block_text, _, _) = parse_html_text(notes);
        let trimmed = block_text.trim();
        if trimmed.is_empty() {
            return false;
        }

        let mut has_segment = false;
        for raw_segment in SEGMENT_SPLIT_RE.split(trimmed) {
            let cleaned = clean_segment(raw_segment);
            if cleaned.is_empty() {
                continue;
            }
            has_segment = true;
            if !re.is_match(cleaned) {
                return false;
            }
        }

        has_segment
    }
}

fn clean_segment(segment: &str) -> &str {
    let trimmed = segment.trim();
    let without_leading = trimmed.trim_start_matches(|c: char| {
        c.is_whitespace() || matches!(c, '•' | '▪' | '▸' | '►' | '-' | '–' | '—' | '*')
    });
    let without_trailing = without_leading.trim_end_matches(|c: char| {
        c.is_whitespace() || matches!(c, ',' | ':' | ';' | '!' | '?' | '.')
    });
    let mut s = without_trailing.trim();
    loop {
        if let Some(inner) = s.strip_prefix('(').and_then(|rest| rest.strip_suffix(')')) {
            s = inner.trim();
        } else if let Some(inner) = s.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            s = inner.trim();
        } else {
            break;
        }
    }
    s
}

fn compile_exam_boilerplate_regex(
    profile_id: &str,
    config: &ExamBoilerplateConfig,
) -> Result<Option<Regex>, DomainProfileError> {
    if !config.enabled {
        return Ok(None);
    }

    let labels_alt = build_labels_alternation(&config.known_labels);
    let mut branches: Vec<String> = Vec::new();

    for pattern in &config.phrase_patterns {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.contains(LABELS_PLACEHOLDER) {
            if let Some(ref alt) = labels_alt {
                branches.push(trimmed.replace(LABELS_PLACEHOLDER, alt));
            }
        } else {
            branches.push(trimmed.to_string());
        }
    }

    if branches.is_empty() {
        if let Some(alt) = labels_alt {
            branches.push(format!(
                r"(?:[\(\[]?\b(?:{alt})\b[\)\]]?(?:\s*[-/–]?\s*(?:(?:19|20)\d{{2}}|'?\d{{2}}))?[\)\]]?)(?:\s*(?:[,;/&+–-]|\be\b)\s*[\(\[]?\b(?:{alt})\b[\)\]]?(?:\s*[-/–]?\s*(?:(?:19|20)\d{{2}}|'?\d{{2}}))?[\)\]]?)*"
            ));
        } else {
            return Ok(None);
        }
    }

    let full_pattern = format!(r"(?i)^(?:{})$", branches.join("|"));
    Regex::new(&full_pattern)
        .map(Some)
        .map_err(|source| DomainProfileError::InvalidExamRegex {
            profile_id: profile_id.to_string(),
            source,
        })
}

fn build_labels_alternation(labels: &[String]) -> Option<String> {
    let mut sorted: Vec<&str> = labels
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if sorted.is_empty() {
        return None;
    }

    sorted.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    sorted.dedup();

    let escaped: Vec<String> = sorted.into_iter().map(regex::escape).collect();
    Some(escaped.join("|"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn med_br_profile_loads_with_41_labels() {
        let profile = DomainProfile::med_br();
        assert_eq!(profile.id, "med_br");
        assert_eq!(profile.default_max_note_chars, 1200);
        assert_eq!(profile.table_max_note_chars, 2500);
        assert_eq!(profile.exam_boilerplate.known_labels.len(), 41);
        assert!(profile.exam_boilerplate_regex().is_some());
    }

    #[test]
    fn general_profile_disables_exam_boilerplate() {
        let profile = DomainProfile::general();
        assert!(profile.exam_boilerplate_regex().is_none());
        assert!(!profile.is_exam_only_note("Cobrado na prova da USP 2023."));
    }

    #[test]
    fn exam_only_respects_html_line_breaks() {
        let profile = DomainProfile::med_br();
        assert!(profile.is_exam_only_note("USP 2023<br>UNIFESP 2024"));
        assert!(
            !profile
                .is_exam_only_note("USP 2023<br>O bloqueio beta-1 reduz a frequência cardíaca.")
        );
    }

    #[test]
    fn direct_serde_loading_preserves_compiled_regex() {
        let profile: DomainProfile = serde_json::from_str(MED_BR_PROFILE_JSON)
            .expect("direct serde deserialization must succeed");
        assert!(
            profile.exam_boilerplate_regex().is_some(),
            "direct serde deserialization must compile exam regex"
        );
        assert!(profile.is_exam_only_note("Prova do ENARE 2024."));
    }

    #[test]
    fn invalid_custom_regex_fails_gracefully() {
        let invalid_json = r#"{
            "id": "bad_regex",
            "name": "Bad",
            "description": "desc",
            "language": "pt",
            "default_max_note_chars": 1200,
            "table_max_note_chars": 2500,
            "exam_boilerplate": {
                "enabled": true,
                "known_labels": ["TEST"],
                "phrase_patterns": ["(unclosed_paren"]
            },
            "info_policy": {
                "source_fidelity": "ok",
                "notes_purpose": "ok",
                "preferred_list_format": "ok"
            }
        }"#;

        let res = DomainProfile::from_json_str(invalid_json);
        assert!(matches!(
            res,
            Err(DomainProfileError::InvalidExamRegex { .. })
        ));

        // Direct serde also fails
        let serde_res: Result<DomainProfile, _> = serde_json::from_str(invalid_json);
        assert!(serde_res.is_err());
    }

    #[test]
    fn domain_profile_generates_json_schema() {
        let schema = schemars::schema_for!(DomainProfile);
        let schema_json =
            serde_json::to_string_pretty(&schema).expect("schema serialization must succeed");
        assert!(schema_json.contains("default_max_note_chars"));
        assert!(schema_json.contains("exam_boilerplate"));
        assert!(schema_json.contains("info_policy"));
        assert!(!schema_json.contains("exam_regex"));
    }
}
