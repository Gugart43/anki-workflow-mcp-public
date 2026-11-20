pub fn infer_campo_estudo(etiquetas: &[String]) -> String {
    const EXCLUDED_PREFIXES: [&str; 5] = ["id::", "FocoClinico::", "Fonte::", "foco::", "tag::"];

    for raw_tag in etiquetas {
        let tag = raw_tag.trim().trim_start_matches('#');
        if tag.is_empty() || !tag.contains("::") {
            continue;
        }
        if EXCLUDED_PREFIXES.iter().any(|prefix| {
            tag.get(..prefix.len())
                .map(|head| head.eq_ignore_ascii_case(prefix))
                .unwrap_or(false)
        }) {
            continue;
        }
        let parts: Vec<String> = tag
            .split("::")
            .filter_map(|p| {
                let trimmed = p.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.replace('_', " "))
                }
            })
            .collect();
        if parts.is_empty() {
            continue;
        }
        if parts[0].to_lowercase() == "medicina" {
            return parts[..parts.len().min(3)].join("::");
        }
        if parts.len() >= 2 {
            return format!("Medicina::{}::{}", parts[0], parts[1]);
        }
        if parts.len() == 1 {
            return format!("Medicina::{}", parts[0]);
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::infer_campo_estudo;

    #[test]
    fn test_empty_and_negative_cases() {
        assert_eq!(infer_campo_estudo(&[]), "");

        let negative = vec![
            String::new(),
            "   ".to_string(),
            "###".to_string(),
            "sem_hierarquia".to_string(),
            "::::".to_string(),
            "id::123::Cardio".to_string(),
            "FocoClinico::Diagnóstico".to_string(),
            "Fonte::Diretriz".to_string(),
            "foco::Tratamento".to_string(),
            "tag::Revisão".to_string(),
        ];
        assert_eq!(infer_campo_estudo(&negative), "");
    }

    #[test]
    fn test_medicina_prefix_and_case_preservation() {
        let tags = vec![
            "id::999::Ignorar".to_string(),
            "  ##Medicina::Cardio_Vascular::Insuficiência_Cardíaca::Extra  ".to_string(),
        ];
        assert_eq!(
            infer_campo_estudo(&tags),
            "Medicina::Cardio Vascular::Insuficiência Cardíaca"
        );

        let upper = vec!["MEDICINA::Neuro_logia".to_string()];
        assert_eq!(infer_campo_estudo(&upper), "MEDICINA::Neuro logia");
    }

    #[test]
    fn test_non_medicina_single_part_and_case_insensitive_exclusions() {
        let two_or_more = vec!["Clínica_Médica::Nefro_logia::Extra".to_string()];
        assert_eq!(
            infer_campo_estudo(&two_or_more),
            "Medicina::Clínica Médica::Nefro logia"
        );

        let single_part = vec!["::Endócrino_logia::".to_string()];
        assert_eq!(
            infer_campo_estudo(&single_part),
            "Medicina::Endócrino logia"
        );

        let case_insensitive = vec![
            "ID::123::Cardio".to_string(),
            "FOCO::Diagnóstico".to_string(),
            "TAG::Exame".to_string(),
            "FONTE::Diretriz".to_string(),
            "FOCOCLINICO::Conduta".to_string(),
            "Pneumo::Asma".to_string(),
        ];
        assert_eq!(
            infer_campo_estudo(&case_insensitive),
            "Medicina::Pneumo::Asma"
        );

        let all_excluded = vec![
            "id::123::Ignorar".to_string(),
            "ID::456::IgnorarTambem".to_string(),
        ];
        assert_eq!(infer_campo_estudo(&all_excluded), "");
    }
}
