//! Detecção de duplicatas (incluindo QA vs Cloze), quase-duplicatas e pares de interferência em FSRS.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use anki_domain::{Diagnostic, DiagnosticCode, Severity};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::tags::normalize_ptbr;

pub const CODE_DUPLICATE_RETRIEVAL: DiagnosticCode =
    DiagnosticCode::from_static("QUALITY.DUPLICATE_RETRIEVAL");
pub const CODE_NEAR_DUPLICATE_WARN: DiagnosticCode =
    DiagnosticCode::from_static("QUALITY.NEAR_DUPLICATE_WARN");
pub const CODE_INTERFERENCE_PAIR_WARN: DiagnosticCode =
    DiagnosticCode::from_static("CARD.INTERFERENCE_PAIR_WARN");

pub const OWNER_SKILL: &str = "anki-card-generator";

// Nota: NÃO incluir "nao" ou "sem" como stopwords para preservar polaridade clínica.
const STOPWORDS_PT: &[&str] = &[
    "a", "o", "as", "os", "de", "da", "do", "das", "dos", "em", "na", "no", "nas", "nos", "um",
    "uma", "e", "ou", "para", "por", "com", "que", "qual", "quais", "se", "ao", "aos", "pelo",
    "pela", "pelos", "pelas", "num", "numa",
];

static CLOZE_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{c\d+::").expect("valid cloze syntax regex"));

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetrievalUnit {
    pub id: String,
    pub row: usize,
    pub is_cloze: bool,
    pub prompt: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SiblingCluster {
    pub unit_ids: Vec<String>,
    pub recommend_bury_siblings: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterferenceReport {
    pub diagnostics: Vec<Diagnostic>,
    pub sibling_clusters: Vec<SiblingCluster>,
}

/// Analisa um lote de unidades de recuperação (`RetrievalUnit`) para detectar:
/// 1. Duplicatas lexicais, incluindo QA/Cloze com alvo igual e contexto compartilhado.
/// 2. Pares de interferência semântica FSRS — prompts similares (>= 65%) mas alvos distintos (< 25%).
/// 3. Quase-duplicatas — similaridade combinada >= 60%.
///
/// Todas as violações são emitidas como advisories (`Severity::Warn`).
#[must_use]
pub fn analyze_batch_interference(units: &[RetrievalUnit]) -> InterferenceReport {
    let mut diagnostics = Vec::new();
    let mut sibling_clusters = Vec::new();

    let tokenized: Vec<(BTreeSet<String>, BTreeSet<String>, BTreeSet<String>)> = units
        .iter()
        .map(|u| {
            let p = if u.is_cloze {
                let visible = anki_parser::find_cloze_spans(&u.prompt);
                if visible.is_empty() {
                    token_set(&u.prompt)
                } else {
                    // `find_cloze_spans` reports UTF-8 byte offsets, not char indices.
                    let mut masked = String::with_capacity(u.prompt.len());
                    let mut cursor = 0;
                    for span in visible {
                        if span.start >= cursor {
                            masked.push_str(&u.prompt[cursor..span.start]);
                            masked.push(' ');
                            cursor = span.end;
                        }
                    }
                    masked.push_str(&u.prompt[cursor..]);
                    token_set(&masked)
                }
            } else {
                token_set(&u.prompt)
            };
            let t = token_set(&u.target);
            let mut combined = p.clone();
            combined.extend(t.iter().cloned());
            (p, t, combined)
        })
        .collect();

    for i in 0..units.len() {
        for j in (i + 1)..units.len() {
            let p_sim = jaccard(&tokenized[i].0, &tokenized[j].0);
            let t_sim = jaccard(&tokenized[i].1, &tokenized[j].1);
            let c_sim = jaccard(&tokenized[i].2, &tokenized[j].2);
            let shared_prompt = tokenized[i].0.intersection(&tokenized[j].0).count();
            let cross_type_match = units[i].is_cloze != units[j].is_cloze
                && t_sim == 1.0
                && p_sim >= 0.30
                && shared_prompt >= 3;
            let polarity_agrees = ["nao", "sem"].iter().all(|&word| {
                tokenized[i].0.contains(word) == tokenized[j].0.contains(word)
                    && tokenized[i].1.contains(word) == tokenized[j].1.contains(word)
            });

            if polarity_agrees
                && (c_sim >= 0.85 || (t_sim >= 0.90 && p_sim >= 0.70) || cross_type_match)
            {
                diagnostics.push(create_advisory(
                    CODE_DUPLICATE_RETRIEVAL,
                    format!(
                        "Possível duplicata entre '{}' e '{}' (similaridade lexical: contexto {:.0}%, alvo {:.0}%).",
                        units[i].id, units[j].id, p_sim * 100.0, t_sim * 100.0
                    ),
                    units[j].row,
                    Some("Consolide ou remova um dos cartões duplicados para evitar repetição no FSRS."),
                ));
            } else if p_sim >= 0.65 && t_sim < 0.25 {
                let p_sim_pct = p_sim * 100.0;
                diagnostics.push(create_advisory(
                    CODE_INTERFERENCE_PAIR_WARN,
                    format!(
                        "Alto risco de interferência FSRS entre '{}' e '{}' (prompts {:.0}% similares, alvos distintos): adicione discriminador contrastivo ou tabela comparativa nas Notas.",
                        units[i].id, units[j].id, p_sim_pct
                    ),
                    units[j].row,
                    Some("Adicione discriminador contrastivo no enunciado ou tabela comparativa nas Notas."),
                ));
                sibling_clusters.push(SiblingCluster {
                    unit_ids: vec![units[i].id.clone(), units[j].id.clone()],
                    // Anki only buries siblings generated from the same note.
                    recommend_bury_siblings: units[i].is_cloze
                        && units[j].is_cloze
                        && units[i].row == units[j].row,
                    reason: "Par de alta interferência semântica".to_string(),
                });
            } else if c_sim >= 0.60 || (t_sim >= 0.55 && shared_prompt >= 3) {
                let c_sim_pct = c_sim * 100.0;
                diagnostics.push(create_advisory(
                    CODE_NEAR_DUPLICATE_WARN,
                    format!(
                        "Quase-duplicata entre '{}' e '{}' (similaridade {:.0}%): avalie consolidar.",
                        units[i].id, units[j].id, c_sim_pct
                    ),
                    units[j].row,
                    Some("Avalie consolidar os conceitos em um único cartão estruturado."),
                ));
            }
        }
    }

    InterferenceReport {
        diagnostics,
        sibling_clusters,
    }
}

fn create_advisory(
    code: DiagnosticCode,
    message: String,
    row: usize,
    suggestion: Option<&str>,
) -> Diagnostic {
    let mut d = Diagnostic::new(code, Severity::Warn, OWNER_SKILL, message)
        .with_row(row)
        .with_field("Lote");
    if let Some(sug) = suggestion {
        d = d.with_suggestion(sug);
    }
    d
}

fn token_set(raw: &str) -> BTreeSet<String> {
    let stripped = strip_html_and_cloze(raw);
    let norm = normalize_for_tokens(&stripped);
    norm.split(|c: char| !c.is_alphanumeric())
        .filter(|t| {
            !t.is_empty()
                && !STOPWORDS_PT.contains(t)
                && (t.len() >= 2 || t.as_bytes()[0].is_ascii_digit())
        })
        .map(str::to_string)
        .collect()
}

fn strip_html_and_cloze(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    let s = out
        .replace("&nbsp;", " ")
        .replace("&amp;", " ")
        .replace("&lt;", " ")
        .replace("&gt;", " ")
        .replace("&#124;", " ");
    let s = CLOZE_TAG_RE.replace_all(&s, " ");
    s.replace("}}", " ").replace("::", " ")
}

fn normalize_for_tokens(s: &str) -> String {
    let base = normalize_ptbr(s);
    base.chars()
        .map(|c| match c {
            'ä' | 'à' | 'á' | 'â' | 'ã' | 'å' => 'a',
            'ë' | 'è' | 'é' | 'ê' => 'e',
            'ï' | 'ì' | 'í' | 'î' => 'i',
            'ö' | 'ò' | 'ó' | 'ô' | 'õ' => 'o',
            'ü' | 'ù' | 'ú' | 'û' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            _ => c,
        })
        .collect()
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 { 0.0 } else { inter / union }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wave2_integration_crohn_rcu_and_duplicate() {
        let units = vec![
            RetrievalUnit {
                id: "Q1".into(),
                row: 2,
                is_cloze: false,
                prompt: "Na suspeita de doença inflamatória intestinal, qual o achado endoscópico típico da doença de Crohn?".into(),
                target: "Úlceras aftoides salteadas em pedra de calçamento".into(),
            },
            RetrievalUnit {
                id: "Q2".into(),
                row: 3,
                is_cloze: false,
                prompt: "Na suspeita de doença inflamatória intestinal, qual o achado endoscópico típico da retocolite ulcerativa?".into(),
                target: "Mucosa friável contínua a partir do reto".into(),
            },
            RetrievalUnit {
                id: "C1".into(),
                row: 4,
                is_cloze: true,
                prompt: "Na suspeita de doença inflamatória intestinal, achado endoscópico típico da doença de Crohn: {{c1::úlceras aftoides salteadas em pedra de calçamento}}.".into(),
                target: "Úlceras aftoides salteadas em pedra de calçamento".into(),
            },
        ];
        let rep = analyze_batch_interference(&units);
        assert!(
            rep.diagnostics
                .iter()
                .any(|d| d.code == CODE_DUPLICATE_RETRIEVAL)
        );
        assert!(
            rep.diagnostics
                .iter()
                .any(|d| d.code == CODE_INTERFERENCE_PAIR_WARN)
        );
        assert!(
            rep.sibling_clusters
                .iter()
                .any(|pair| pair.unit_ids == ["Q1", "Q2"])
        );
        assert!(
            rep.sibling_clusters
                .iter()
                .all(|pair| !pair.recommend_bury_siblings)
        );

        // Verifica que são emitidos como advisories (Warn)
        for diag in &rep.diagnostics {
            assert_eq!(diag.severity, Severity::Warn);
            assert_eq!(diag.field.as_deref(), Some("Lote"));
        }
    }

    #[test]
    fn test_qa_cloze_cross_type_duplicate_exact_target() {
        let units = vec![
            RetrievalUnit {
                id: "QA-01".into(),
                row: 10,
                is_cloze: false,
                prompt: "Qual o tratamento farmacológico inicial de escolha para diabetes mellitus tipo 2?".into(),
                target: "Metformina 500 mg a 1000 mg ao dia".into(),
            },
            RetrievalUnit {
                id: "CLOZE-01".into(),
                row: 11,
                is_cloze: true,
                prompt: "No diabetes mellitus tipo 2, a primeira escolha medicamentosa é {{c1::metformina 500 mg a 1000 mg ao dia}}.".into(),
                target: "Metformina 500 mg a 1000 mg ao dia".into(),
            },
        ];
        let rep = analyze_batch_interference(&units);
        assert!(
            rep.diagnostics
                .iter()
                .any(|d| d.code == CODE_DUPLICATE_RETRIEVAL)
        );
    }

    #[test]
    fn test_near_duplicate_detection() {
        let units = vec![
            RetrievalUnit {
                id: "U1".into(),
                row: 20,
                is_cloze: false,
                prompt: "Quais os critérios clínicos diagnósticos de choque séptico refratário a volume?".into(),
                target: "Necessidade de vasopressor para PAM >= 65 mmHg e lactato sérico > 2 mmol/L".into(),
            },
            RetrievalUnit {
                id: "U2".into(),
                row: 21,
                is_cloze: false,
                prompt: "Como diagnosticar choque séptico refratário à reposição volêmica?".into(),
                target: "Vasopressor para PAM >= 65 mmHg e dosagem de lactato acima de 2 mmol/L".into(),
            },
        ];
        let rep = analyze_batch_interference(&units);
        assert!(
            rep.diagnostics
                .iter()
                .any(|d| d.code == CODE_NEAR_DUPLICATE_WARN || d.code == CODE_DUPLICATE_RETRIEVAL)
        );
    }

    #[test]
    fn test_clinical_polarity_stopwords_preserved() {
        // "com" vs "sem" e "nao": "com dor" e "sem dor" não devem ter seus alvos colapsados
        let units = [
            RetrievalUnit {
                id: "P1".into(),
                row: 30,
                is_cloze: false,
                prompt: "Paciente com apendicite aguda".into(),
                target: "Presença de dor em fossa ilíaca direita".into(),
            },
            RetrievalUnit {
                id: "P2".into(),
                row: 31,
                is_cloze: false,
                prompt: "Paciente pós-apendicectomia sem complicações".into(),
                target: "Sem dor e sem febre".into(),
            },
        ];
        let t1 = token_set(&units[0].target);
        let t2 = token_set(&units[1].target);
        assert!(t2.contains("sem"));
        assert!(!t1.contains("sem"));
    }

    #[test]
    fn test_ptbr_normalization_and_html_stripping() {
        let raw = "<p>O <b>achado</b> endoscópico típico de <i>Crohn</i> &nbsp; são {{c1::úlceras aftóides}}.</p>";
        let tokens = token_set(raw);
        assert!(tokens.contains("achado"));
        assert!(tokens.contains("endoscopico"));
        assert!(tokens.contains("tipico"));
        assert!(tokens.contains("crohn"));
        assert!(tokens.contains("ulceras"));
        assert!(tokens.contains("aftoides"));
        assert!(!tokens.contains("nbsp"));
        assert!(!tokens.contains("p"));
        assert!(!tokens.contains("b"));
    }

    #[test]
    fn test_empty_and_single_unit() {
        let rep_empty = analyze_batch_interference(&[]);
        assert!(rep_empty.diagnostics.is_empty());
        assert!(rep_empty.sibling_clusters.is_empty());

        let single = vec![RetrievalUnit {
            id: "S1".into(),
            row: 1,
            is_cloze: false,
            prompt: "Pergunta isolada sem par".into(),
            target: "Resposta isolada".into(),
        }];
        let rep_single = analyze_batch_interference(&single);
        assert!(rep_single.diagnostics.is_empty());
        assert!(rep_single.sibling_clusters.is_empty());
    }

    #[test]
    fn test_independent_unrelated_cards() {
        let units = vec![
            RetrievalUnit {
                id: "CARD-A".into(),
                row: 40,
                is_cloze: false,
                prompt: "Qual o agente etiológico mais comum da pneumonia adquirida na comunidade?".into(),
                target: "Streptococcus pneumoniae".into(),
            },
            RetrievalUnit {
                id: "CARD-B".into(),
                row: 41,
                is_cloze: false,
                prompt: "Qual a meta pressórica recomendada na diretriz para paciente com nefropatia diabética?".into(),
                target: "Pressão arterial menor que 130 por 80 mmHg".into(),
            },
        ];
        let rep = analyze_batch_interference(&units);
        assert!(rep.diagnostics.is_empty());
        assert!(rep.sibling_clusters.is_empty());
    }
}
