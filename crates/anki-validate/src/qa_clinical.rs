//! Validação de precisão clínica para flashcards QA médicos em PT-BR.
//!
//! Regras Wave 2:
//! - `QA.CLINICAL_ANCHOR_MISSING_WARN`: Enunciado terapêutico/posológico sem âncora clínica mínima.
//! - `QA.DRUG_PAIR_INCOMPLETE_WARN`: Pergunta de dose/esquema com par fármaco-dose incompleto na resposta.
//! - `QA.REVERSE_DIRECTION_WARN`: Direcionalidade clínica invertida (fármaco -> doença em vez de quadro -> conduta).
//! - `QA.BARE_CLINICAL_NUMBER_WARN`: Número clínico solto na resposta sem unidade associada.

use std::sync::LazyLock;

use anki_domain::{Diagnostic, DiagnosticCode, Severity};
use regex::Regex;

pub use crate::tags::normalize_ptbr;

const OWNER_SKILL: &str = "anki-qa-designer";

pub const CODE_CLINICAL_ANCHOR_MISSING_WARN: DiagnosticCode =
    DiagnosticCode::from_static("QA.CLINICAL_ANCHOR_MISSING_WARN");
pub const CODE_DRUG_PAIR_INCOMPLETE_WARN: DiagnosticCode =
    DiagnosticCode::from_static("QA.DRUG_PAIR_INCOMPLETE_WARN");
pub const CODE_REVERSE_DIRECTION_WARN: DiagnosticCode =
    DiagnosticCode::from_static("QA.REVERSE_DIRECTION_WARN");
pub const CODE_BARE_CLINICAL_NUMBER_WARN: DiagnosticCode =
    DiagnosticCode::from_static("QA.BARE_CLINICAL_NUMBER_WARN");

const CONTEXT_REQUIRED_TRIGGERS: &[&str] = &[
    "qual a dose",
    "qual o antibiotico de escolha",
    "qual o alvo pressorico",
    "qual a conduta inicial",
    "qual o tratamento de primeira linha",
];

const CLINICAL_ANCHORS: &[&str] = &[
    "adulto",
    "adulta",
    "gestante",
    "lactente",
    "crianca",
    "pediatria",
    "pediatrico",
    "pediatrica",
    "idoso",
    "idosa",
    "neonato",
    "rn",
    "recem-nascido",
    "recem nascido",
    "ambulatorial",
    "internado",
    "internada",
    "uti",
    "enfermaria",
    "grave",
    "estavel",
    "instavel",
    "sbc",
    "sbpt",
    "aha",
    "esc",
    "idsa",
    "ms",
    "ministerio da saude",
    "sbp",
    "febrasgo",
    "comorbidade",
    "sem comorbidade",
    "refratario",
    "refrataria",
    "agudo",
    "aguda",
    "cronico",
    "cronica",
];

const DOSE_TRIGGERS: &[&str] = &["dose", "posologia", "esquema", "como administrar"];

const ROUTES_OR_FREQ: &[&str] = &[
    "vo",
    "ev",
    "iv",
    "im",
    "sc",
    "sl",
    "inalat",
    "bolus",
    "12/12",
    "8/8",
    "6/6",
    "24/24",
    "1x/dia",
    "2x/dia",
    "min",
    "dia",
    "dose unica",
    "oral",
    "intravenosa",
    "intramuscular",
    "subcutanea",
];

const CLINICAL_UNITS: &[&str] = &[
    "mg", "g", "mcg", "µg", "ug", "ml", "l", "meq", "mmol", "mmhg", "bpm", "irpm", "cm", "mm", "%",
    "ui", "j", "s", "min", "h", "hora", "horas", "minuto", "minutos", "segundo", "segundos", "dia",
    "dias", "semana", "semanas", "mes", "meses", "ano", "anos", "°c", "kg", "kcal",
];

static DOSE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[0-9]+(?:[.,][0-9]+)?\s*(?:mg|mcg|µg|ug|g|ui|meq|mmol|ml)\b")
        .expect("valid dose pattern")
});
static HTML_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"</?[a-zA-Z][^>]*>").expect("valid HTML tag pattern"));

/// Verifica sinais formais em pares de pergunta e resposta médica (QA).
#[must_use]
pub fn check_qa_clinical(enunciado: &str, resposta: &str, row: Option<usize>) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let stem = normalize_ptbr(&HTML_TAG.replace_all(enunciado, " "));
    let visible_answer = HTML_TAG.replace_all(resposta, " ");
    let ans = normalize_ptbr(&visible_answer);

    // 1. Âncora clínica mínima
    if CONTEXT_REQUIRED_TRIGGERS.iter().any(|t| stem.contains(t))
        && !CLINICAL_ANCHORS.iter().any(|a| matches_anchor(&stem, a))
    {
        diags.push(diag(
            Severity::Warn,
            CODE_CLINICAL_ANCHOR_MISSING_WARN,
            "Enunciado terapêutico/posológico sem âncora clínica (população, gravidade/cenário ou diretriz).",
            row,
            "Enunciado",
        ));
    }

    // 2. Par farmacológico completo
    if DOSE_TRIGGERS.iter().any(|t| matches_anchor(&stem, t)) {
        let has_dose = DOSE_PATTERN.is_match(&ans);
        let has_route_or_freq = ROUTES_OR_FREQ.iter().any(|r| matches_anchor(&ans, r))
            || ans.split_whitespace().any(|w| {
                matches!(
                    w.trim_end_matches(['h', '.', ',', ';']),
                    "12/12" | "8/8" | "6/6" | "24/24"
                )
            });
        let has_drug_word = ans.split(|c: char| !c.is_alphabetic()).any(|w| {
            matches!(w, "aas" | "kcl" | "nacl")
                || (w.len() >= 5
                    && !matches!(
                        w,
                        "intravenosa"
                            | "intramuscular"
                            | "subcutanea"
                            | "bolus"
                            | "unica"
                            | "horas"
                            | "semanas"
                            | "administrar"
                    ))
        });
        if !(has_dose && has_route_or_freq && has_drug_word) {
            diags.push(diag(
                Severity::Warn,
                CODE_DRUG_PAIR_INCOMPLETE_WARN,
                "Enunciado solicita dose/esquema, mas a resposta omite fármaco, dose com unidade ou via/frequência.",
                row,
                "Resposta",
            ));
        }
    }

    // 3. Direcionalidade clínica invertida
    let asks_disease_from_drug = (stem.contains("para qual doenca")
        || stem.contains("em qual doenca")
        || stem.contains("qual doenca e tratada"))
        && !(stem.contains("efeito adverso")
            || stem.contains("contraindicacao")
            || stem.contains("mecanismo"));
    if asks_disease_from_drug {
        diags.push(diag(
            Severity::Warn,
            CODE_REVERSE_DIRECTION_WARN,
            "Direcionalidade clínica invertida (tratamento -> doença): formule no sentido quadro clínico -> conduta.",
            row,
            "Enunciado",
        ));
    }

    // 4. Números clínicos soltos sem unidade na resposta
    if let Some(bare) = find_bare_clinical_number(&visible_answer) {
        diags.push(diag(
            Severity::Warn,
            CODE_BARE_CLINICAL_NUMBER_WARN,
            format!("Valor numérico '{bare}' sem unidade clínica associada na resposta."),
            row,
            "Resposta",
        ));
    }

    diags
}

fn matches_anchor(stem: &str, anchor: &str) -> bool {
    stem.match_indices(anchor).any(|(start, _)| {
        let end = start + anchor.len();
        !stem[..start].ends_with(char::is_alphanumeric)
            && !stem[end..].starts_with(char::is_alphanumeric)
    })
}

fn is_number(token: &str) -> bool {
    token.chars().any(|c| c.is_ascii_digit())
        && token
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ',' | '.'))
}

fn starts_with_unit(token: &str) -> bool {
    let normalized = normalize_ptbr(token);
    CLINICAL_UNITS.iter().any(|unit| {
        normalized
            .strip_prefix(unit)
            .is_some_and(|rest| !rest.starts_with(char::is_alphanumeric))
    })
}

fn find_bare_clinical_number(text: &str) -> Option<String> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for (idx, tok) in tokens.iter().enumerate() {
        let clean = tok.trim_matches(|c: char| {
            matches!(
                c,
                '(' | ')' | ',' | '.' | ';' | ':' | '?' | '!' | '[' | ']' | '{' | '}'
            )
        });
        if !is_number(clean) {
            continue;
        }
        // Ignora anos 1990-2035
        if let Ok(n) = clean.parse::<u32>()
            && (1990..=2035).contains(&n)
        {
            continue;
        }
        let next = tokens
            .get(idx + 1)
            .map(|s| normalize_ptbr(s))
            .unwrap_or_default();
        let prev = idx
            .checked_sub(1)
            .and_then(|p| tokens.get(p))
            .map(|s| normalize_ptbr(s))
            .unwrap_or_default();
        if prev.contains("grau")
            || prev.contains("tipo")
            || prev.contains("estagio")
            || prev.contains("class")
            || prev.contains("escore")
            || prev.contains("score")
            || prev.contains("killip")
            || prev.contains("curb")
            || prev.contains("timi")
        {
            continue;
        }
        let has_unit = starts_with_unit(&next)
            || (matches!(next.as_str(), "a" | "ate" | "-" | "–" | "—")
                && tokens.get(idx + 2).is_some_and(|t| is_number(t))
                && tokens.get(idx + 3).is_some_and(|t| starts_with_unit(t)));
        if !has_unit {
            return Some(clean.to_string());
        }
    }
    None
}

fn diag(
    severity: Severity,
    code: DiagnosticCode,
    message: impl Into<String>,
    row: Option<usize>,
    field: &str,
) -> Diagnostic {
    let mut d = Diagnostic::new(code, severity, OWNER_SKILL, message).with_field(field);
    if let Some(r) = row {
        d = d.with_row(r);
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anchors_missing_and_present() {
        // Sem âncora: gera CODE_CLINICAL_ANCHOR_MISSING_WARN
        let d = check_qa_clinical(
            "Qual a dose de amoxicilina?",
            "Amoxicilina 500 mg VO 8/8h",
            Some(1),
        );
        assert!(
            d.iter()
                .any(|diag| diag.code == CODE_CLINICAL_ANCHOR_MISSING_WARN)
        );
        assert_eq!(d[0].severity, Severity::Warn);
        assert_eq!(d[0].field.as_deref(), Some("Enunciado"));
        assert_eq!(d[0].row, Some(1));

        let d_atb = check_qa_clinical("Qual o antibiótico de escolha?", "Ceftriaxona 1 g EV", None);
        assert!(
            d_atb
                .iter()
                .any(|diag| diag.code == CODE_CLINICAL_ANCHOR_MISSING_WARN)
        );

        let d_alvo = check_qa_clinical("Qual o alvo pressórico?", "PA < 130x80 mmHg", None);
        assert!(
            d_alvo
                .iter()
                .any(|diag| diag.code == CODE_CLINICAL_ANCHOR_MISSING_WARN)
        );

        let d_conduta = check_qa_clinical("Qual a conduta inicial?", "Oxigenioterapia", None);
        assert!(
            d_conduta
                .iter()
                .any(|diag| diag.code == CODE_CLINICAL_ANCHOR_MISSING_WARN)
        );

        let d_primeira_linha = check_qa_clinical(
            "Qual o tratamento de primeira linha?",
            "Metformina 500 mg VO",
            None,
        );
        assert!(
            d_primeira_linha
                .iter()
                .any(|diag| diag.code == CODE_CLINICAL_ANCHOR_MISSING_WARN)
        );

        // Com âncoras clínicas variadas: NÃO gera aviso
        let anchors_cases = [
            "Em adulto com faringite bacteriana, qual a dose de amoxicilina?",
            "Na gestante com cistite aguda, qual o antibiótico de escolha?",
            "No lactente febril, qual a conduta inicial?",
            "Na criança com otite, qual a dose de amoxicilina?",
            "No idoso hipertenso, segundo a SBC, qual o alvo pressórico?",
            "No RN pré-termo com desconforto respiratório, qual a conduta inicial?",
            "No paciente internado em UTI, qual o antibiótico de escolha?",
            "Na crise asmática grave, qual a conduta inicial?",
            "Segundo o Ministério da Saúde, qual o tratamento de primeira linha?",
            "Na dor crônica refratária, qual o tratamento de primeira linha?",
        ];
        for case in anchors_cases {
            let diags = check_qa_clinical(case, "Amoxicilina 500 mg VO 8/8h", None);
            assert!(
                !diags
                    .iter()
                    .any(|diag| diag.code == CODE_CLINICAL_ANCHOR_MISSING_WARN),
                "Falso positivo de âncora para: {case}"
            );
        }

        // Teste de fronteira de palavra: "perna" não deve validar como "rn"
        let d_perna = check_qa_clinical(
            "Na fratura de perna, qual a conduta inicial?",
            "Imobilização",
            None,
        );
        assert!(
            d_perna
                .iter()
                .any(|diag| diag.code == CODE_CLINICAL_ANCHOR_MISSING_WARN)
        );
    }

    #[test]
    fn test_drug_pair_incomplete_and_complete() {
        // Enunciado com "dose": resposta incompleta
        let d_no_route = check_qa_clinical(
            "Em adulto com PAC, qual a dose de azitromicina?",
            "Azitromicina 500 mg",
            Some(5),
        );
        assert!(
            d_no_route
                .iter()
                .any(|diag| diag.code == CODE_DRUG_PAIR_INCOMPLETE_WARN)
        );
        assert_eq!(d_no_route[0].field.as_deref(), Some("Resposta"));
        assert_eq!(d_no_route[0].severity, Severity::Warn);

        let d_no_unit = check_qa_clinical(
            "Em adulto, qual a posologia de amoxicilina?",
            "Amoxicilina 500 VO 8/8h",
            None,
        );
        assert!(
            d_no_unit
                .iter()
                .any(|diag| diag.code == CODE_DRUG_PAIR_INCOMPLETE_WARN)
        );

        let d_no_drug = check_qa_clinical(
            "Qual o esquema de ataque do antibiótico no idoso?",
            "1 g EV a cada 12/12h",
            None,
        );
        assert!(
            d_no_drug
                .iter()
                .any(|diag| diag.code == CODE_DRUG_PAIR_INCOMPLETE_WARN)
        );

        // Resposta completa: fármaco + dose + unidade + via/frequência
        let d_complete = check_qa_clinical(
            "Em adulto com PAC, qual a dose de azitromicina?",
            "Azitromicina 500 mg VO 1x/dia por 5 dias",
            None,
        );
        assert!(
            !d_complete
                .iter()
                .any(|diag| diag.code == CODE_DRUG_PAIR_INCOMPLETE_WARN)
        );

        // Enunciado não solicita dose/esquema: não deve exigir par farmacológico
        let d_non_dose = check_qa_clinical(
            "Em adulto, qual o principal agente etiológico da erisipela?",
            "Streptococcus pyogenes",
            None,
        );
        assert!(
            !d_non_dose
                .iter()
                .any(|diag| diag.code == CODE_DRUG_PAIR_INCOMPLETE_WARN)
        );
    }

    #[test]
    fn test_direction_reverse_and_exemptions() {
        // Direcionalidade invertida (fármaco -> doença)
        let d_rev1 = check_qa_clinical(
            "Para qual doença serve ceftriaxona 2 g EV?",
            "Meningite bacteriana.",
            Some(10),
        );
        assert!(
            d_rev1
                .iter()
                .any(|diag| diag.code == CODE_REVERSE_DIRECTION_WARN)
        );
        assert_eq!(d_rev1[0].field.as_deref(), Some("Enunciado"));
        assert_eq!(d_rev1[0].severity, Severity::Warn);

        let d_rev2 = check_qa_clinical(
            "Em qual doença prescrevemos carvedilol 25 mg?",
            "Insuficiência cardíaca.",
            None,
        );
        assert!(
            d_rev2
                .iter()
                .any(|diag| diag.code == CODE_REVERSE_DIRECTION_WARN)
        );

        let d_rev3 = check_qa_clinical(
            "Qual doença é tratada com azitromicina 500 mg?",
            "Coqueluche.",
            None,
        );
        assert!(
            d_rev3
                .iter()
                .any(|diag| diag.code == CODE_REVERSE_DIRECTION_WARN)
        );

        // Isenções legítimas: efeito adverso, contraindicação e mecanismo de ação
        let d_ea = check_qa_clinical(
            "Para qual doença o uso de metformina é suspenso por risco de efeito adverso?",
            "Insuficiência renal grave.",
            None,
        );
        assert!(
            !d_ea
                .iter()
                .any(|diag| diag.code == CODE_REVERSE_DIRECTION_WARN)
        );

        let d_ci = check_qa_clinical(
            "Em qual doença betabloqueadores possuem contraindicação formal?",
            "Bloqueio atrioventricular de 2º ou 3º grau.",
            None,
        );
        assert!(
            !d_ci
                .iter()
                .any(|diag| diag.code == CODE_REVERSE_DIRECTION_WARN)
        );

        let d_mec = check_qa_clinical(
            "Em qual doença o mecanismo de ação dos inibidores de SGLT2 reduz desfechos renais?",
            "Nefropatia diabética.",
            None,
        );
        assert!(
            !d_mec
                .iter()
                .any(|diag| diag.code == CODE_REVERSE_DIRECTION_WARN)
        );

        // Pergunta correta no sentido clínico -> conduta
        let d_correto = check_qa_clinical(
            "No choque séptico em adulto refratário a volume em UTI, qual o vasopressor de primeira escolha?",
            "Noradrenalina 0,1 a 2 µg/kg/min EV contínua.",
            None,
        );
        assert!(
            !d_correto
                .iter()
                .any(|diag| diag.code == CODE_REVERSE_DIRECTION_WARN)
        );
    }

    #[test]
    fn test_bare_clinical_numbers() {
        // Números sem unidade clínica associada
        let d_bare1 = check_qa_clinical(
            "Em adulto hipertenso, qual a meta?",
            "Manter PAM acima de 65",
            Some(3),
        );
        assert!(
            d_bare1
                .iter()
                .any(|diag| diag.code == CODE_BARE_CLINICAL_NUMBER_WARN)
        );
        let bare_diag = d_bare1
            .iter()
            .find(|d| d.code == CODE_BARE_CLINICAL_NUMBER_WARN)
            .unwrap();
        assert!(bare_diag.message.contains("'65'"));
        assert_eq!(bare_diag.field.as_deref(), Some("Resposta"));

        let d_bare2 = check_qa_clinical("Qual a dose de amoxicilina?", "Amoxicilina 500", None);
        assert!(
            d_bare2
                .iter()
                .any(|diag| diag.code == CODE_BARE_CLINICAL_NUMBER_WARN)
        );

        // Números com unidade clínica associada (não deve avisar)
        let ok_cases = [
            "Amoxicilina 500 mg VO 8/8h",
            "Ceftriaxona 2 g EV",
            "Noradrenalina 0,2 mcg/kg/min EV",
            "Glicemia alvo 140 a 180 mg/dL",
            "Saturação de O2 > 92 %",
            "Temperatura axilar 38 °C",
            "Pressão arterial alvo < 130 mmHg",
            "Tratamento por 14 dias",
            "Idade de 65 anos",
        ];
        for ans in ok_cases {
            let d = check_qa_clinical("Pergunta médica?", ans, None);
            assert!(
                !d.iter()
                    .any(|diag| diag.code == CODE_BARE_CLINICAL_NUMBER_WARN),
                "Falso positivo de número sem unidade para: {ans}"
            );
        }

        // Anos entre 1990 e 2035 ignorados
        let d_year = check_qa_clinical("Qual a diretriz?", "Recomendação SBC 2024", None);
        assert!(
            !d_year
                .iter()
                .any(|diag| diag.code == CODE_BARE_CLINICAL_NUMBER_WARN)
        );

        // Classificações e escores ignorados (tipo, grau, estágio, curb, timi, classe)
        let d_scores = check_qa_clinical(
            "Qual a classificação?",
            "Insuficiência cardíaca classe 3 ou Killip estágio 2 com score 1",
            None,
        );
        assert!(
            !d_scores
                .iter()
                .any(|diag| diag.code == CODE_BARE_CLINICAL_NUMBER_WARN)
        );
    }

    #[test]
    fn test_wave2_integration_suite_t02_exact() {
        // Caso 1: Enunciado sem âncora + dose incompleta + número solto 500
        let d1 = check_qa_clinical("Qual a dose de amoxicilina?", "Amoxicilina 500", Some(2));
        assert!(
            d1.iter()
                .any(|d| d.code == CODE_CLINICAL_ANCHOR_MISSING_WARN)
        );
        assert!(d1.iter().any(|d| d.code == CODE_DRUG_PAIR_INCOMPLETE_WARN));
        assert!(d1.iter().any(|d| d.code == CODE_BARE_CLINICAL_NUMBER_WARN));
        for diag in &d1 {
            assert_eq!(diag.severity, Severity::Warn);
            assert_eq!(diag.row, Some(2));
        }

        // Caso 2: Direcionalidade clínica invertida
        let d2 = check_qa_clinical(
            "Para qual doença serve ceftriaxona 2 g EV?",
            "Meningite.",
            Some(3),
        );
        assert!(d2.iter().any(|d| d.code == CODE_REVERSE_DIRECTION_WARN));
        let rev_diag = d2
            .iter()
            .find(|d| d.code == CODE_REVERSE_DIRECTION_WARN)
            .unwrap();
        assert_eq!(rev_diag.severity, Severity::Warn);
        assert_eq!(rev_diag.row, Some(3));
        assert_eq!(rev_diag.field.as_deref(), Some("Enunciado"));
    }
}
