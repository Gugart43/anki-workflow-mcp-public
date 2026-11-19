//! Integration tests for Phase 1: Declarative domain profiles, HTML visible text parsing,
//! and structural notes metrics.

use anki_domain::Severity;
use anki_validate::notes::{
    DEFAULT_MAX_LENGTH, DomainProfile, DomainProfileError, check_cloze_target_restatement,
    check_notes, check_notes_ext, check_notes_with_profile, compute_notes_metrics,
    html_to_visible_text, is_exact_answer_restatement, is_exam_only_note,
    normalize_for_exact_match, parse_html_text, strip_table_blocks, visible_char_count,
};

// ============================================================================
// 1. HTML Text Parsing & Entity Decoding Tests
// ============================================================================

#[test]
fn test_html_entities_named_and_numeric() {
    // Standard named entities
    let named = "&amp; &lt; &gt; &quot; &apos; &nbsp; &ndash; &mdash; &bull;";
    assert_eq!(html_to_visible_text(named), "& < > \" ' – — •");

    // Math, Greek and currency entities
    let math =
        "&rarr; &larr; &le; &ge; &plusmn; &times; &divide; &deg; &mu; &micro; &hellip; &permil;";
    assert_eq!(html_to_visible_text(math), "→ ← ≤ ≥ ± × ÷ ° μ µ … ‰");

    // Decimal and hexadecimal entities
    let numeric = "Pipe: &#124; Semicolon: &#59; Hex-pipe: &#x7C; Hex-dash: &#x2013;";
    assert_eq!(
        html_to_visible_text(numeric),
        "Pipe: | Semicolon: ; Hex-pipe: | Hex-dash: –"
    );

    // Non-breaking space converted to regular space
    let nbsp_variants = "A&nbsp;B&#160;C&#xA0;D";
    assert_eq!(html_to_visible_text(nbsp_variants), "A B C D");

    // Unrecognized entity left literal
    let unknown = "&unknown_entity; &notanentity";
    assert_eq!(
        html_to_visible_text(unknown),
        "&unknown_entity; &notanentity"
    );
}

#[test]
fn test_script_and_style_stripping() {
    let html = r#"
        <style type="text/css">
            .card { color: red; font-size: 14px; }
            #main { display: none; }
        </style>
        <p>Visível no corpo</p>
        <SCRIPT language="javascript" async>
            var bad = "<p>Injetado</p>";
            for (var i = 0; i < 10; i++) { console.log(i); }
        </SCRIPT>
        <div>Texto final</div>
    "#;

    let (text, li_count, br_count) = parse_html_text(html);
    assert_eq!(li_count, 0);
    assert_eq!(br_count, 0);
    let vis = html_to_visible_text(html);
    assert_eq!(vis, "Visível no corpo Texto final");
    assert!(!text.contains(".card"));
    assert!(!text.contains("console.log"));
}

#[test]
fn test_unicode_and_portuguese_multibyte() {
    let pt_text = "<p>Ação terapêutica, farmacocinética e biotransformação hepática.</p>";
    let vis = html_to_visible_text(pt_text);
    assert_eq!(
        vis,
        "Ação terapêutica, farmacocinética e biotransformação hepática."
    );
    assert_eq!(visible_char_count(pt_text), 62);

    let normalized = normalize_for_exact_match("• <b>Hipertensão Arterial Sistêmica!</b>");
    assert_eq!(normalized, "hipertensão arterial sistêmica");
}

#[test]
fn test_list_and_break_counts() {
    let fragment =
        "<ul><li class='item'>Primeiro<br>detalhe</li><LI>Segundo<br/>linha<BR /></LI></ul>";
    let (_, li_count, br_count) = parse_html_text(fragment);
    assert_eq!(li_count, 2);
    assert_eq!(br_count, 3);
    assert_eq!(
        html_to_visible_text(fragment),
        "Primeiro detalhe Segundo linha"
    );
}

#[test]
fn test_f2_html5_block_tags_create_spacing() {
    let fragment = "<section><header><h1>Título</h1></header><article><p>Parágrafo</p><blockquote>Citação</blockquote><details><summary>Resumo</summary><dl><dt>Chave</dt><dd>Valor</dd></dl></details></article><footer>Fonte: Harrison</footer></section>";
    let (block_text, _, _) = parse_html_text(fragment);
    // Block tags ensure words do not concatenate
    assert!(block_text.contains("Título"));
    assert!(block_text.contains("Parágrafo"));
    assert!(block_text.contains("Citação"));
    assert!(block_text.contains("Resumo"));
    assert!(block_text.contains("Chave"));
    assert!(block_text.contains("Valor"));
    assert!(block_text.contains("Fonte: Harrison"));

    let vis = html_to_visible_text(fragment);
    assert_eq!(
        vis,
        "Título Parágrafo Citação Resumo Chave Valor Fonte: Harrison"
    );
}

#[test]
fn test_table_cell_spacing_and_strip_tables() {
    let raw = "<table><tr><th>Coluna A</th><th>Coluna B</th></tr><tr><td>Valor 1</td><td>Valor 2</td></tr></table>";
    let vis = html_to_visible_text(raw);
    assert_eq!(vis, "Coluna A Coluna B Valor 1 Valor 2");

    // Stripping tables removes content cleanly
    let with_prose = format!("<p>Antes</p>{raw}<p>Depois</p>");
    assert_eq!(
        html_to_visible_text(&strip_table_blocks(&with_prose)),
        "Antes Depois"
    );

    // Nested table stripping
    let nested =
        "<div>Fora<table><tr><td>T1<table><tr><td>T2</td></tr></table></td></tr></table>Fim</div>";
    assert_eq!(
        html_to_visible_text(&strip_table_blocks(nested)),
        "Fora Fim"
    );
}

// ============================================================================
// 2. Note Metrics & Cognitive Load Tests
// ============================================================================

#[test]
fn test_exact_answer_restatement_prefixes() {
    assert!(is_exact_answer_restatement(
        "A resposta correta é: IECA",
        "IECA"
    ));
    assert!(is_exact_answer_restatement(
        "A resposta correta é IECA",
        "IECA"
    ));
    assert!(is_exact_answer_restatement(
        "Resposta correta: IECA",
        "IECA"
    ));
    assert!(is_exact_answer_restatement("Resposta: IECA", "IECA"));
    assert!(is_exact_answer_restatement("A resposta é: IECA", "IECA"));
    assert!(is_exact_answer_restatement("<b>IECA.</b>", "ieca"));

    // Pedagogical explanations are NOT restatements
    assert!(!is_exact_answer_restatement(
        "A resposta correta é IECA porque reduz a pós-carga ventricular e remodelamento cardíaco.",
        "IECA"
    ));
    assert!(!is_exact_answer_restatement(
        "IECA. Indicado na ICFER e nefropatia diabética.",
        "IECA"
    ));
}

#[test]
fn test_cloze_target_restatement_diagnostic() {
    let diag =
        check_cloze_target_restatement("A resposta correta é: Metformina", "Metformina", 1, 42)
            .expect("should return diagnostic");
    assert_eq!(diag.code, "NOTE.DUP_ANSWER_WARN");
    assert_eq!(diag.severity, Severity::Warn);
    assert_eq!(diag.row, Some(42));
    assert_eq!(diag.field.as_deref(), Some("Notas Adicionais"));
    assert!(diag.message.contains("c1 target"));

    assert!(
        check_cloze_target_restatement(
            "Metformina reduz a gliconeogênese hepática.",
            "Metformina",
            1,
            42
        )
        .is_none()
    );
}

#[test]
fn test_compute_notes_metrics_nested_structures() {
    let complex_html = r#"
        <section class="anki-note-section">
            <p>Parágrafo de introdução clínica com contextualização fisiopatológica.</p>
            <table>
                <tr><td>Tabela externa
                    <table><tr><td>Célula aninhada</td></tr></table>
                </td></tr>
            </table>
            <ul class="anki-list">
                <li>Item principal 1
                    <ul>
                        <li>Subitem A</li>
                        <li>Subitem B</li>
                    </ul>
                </li>
                <li>Item principal 2</li>
            </ul>
            <figure>
                <figcaption>Legenda explicativa do fluxo arterial</figcaption>
            </figure>
            <footer class="anki-sources">
                <small><b>Fonte:</b> Diretriz SBC 2024</small>
            </footer>
        </section>
    "#;

    let metrics = compute_notes_metrics(complex_html);

    assert_eq!(metrics.table_count, 1);
    assert_eq!(metrics.list_item_count, 4); // 2 principais + 2 subitens
    assert!(metrics.citation_chars > 0);
    assert!(metrics.visual_caption_chars > 0);
    assert!(metrics.table_chars > 0);
    assert!(metrics.list_chars > 0);
    assert!(metrics.prose_chars > 0);
    assert!(metrics.max_paragraph_chars > 0);
    assert!(metrics.effective_cognitive_load < metrics.total_visible_chars);

    // Verify cognitive load weight formula: prose*1.0 + list*0.75 + caption*0.60 + table*0.55
    #[allow(clippy::cast_precision_loss)]
    let expected_load = ((metrics.prose_chars as f64)
        + (metrics.list_chars as f64) * 0.75
        + (metrics.visual_caption_chars as f64) * 0.60
        + (metrics.table_chars as f64) * 0.55)
        .round() as usize;
    assert_eq!(metrics.effective_cognitive_load, expected_load);
}

// ============================================================================
// 3. Declarative Domain Profiles & Serde Tests
// ============================================================================

#[test]
fn test_builtin_med_br_profile() {
    let profile = DomainProfile::med_br();
    assert_eq!(profile.id, "med_br");
    assert_eq!(profile.language, "pt-BR");
    assert_eq!(profile.default_max_note_chars, 1200);
    assert_eq!(profile.table_max_note_chars, 2500);
    assert!(profile.exam_boilerplate.enabled);
    assert_eq!(profile.exam_boilerplate.known_labels.len(), 41);
    assert!(profile.info_policy.forbid_metadata_headers);
    assert!(profile.info_policy.forbid_verbatim_answer_restatement);
    assert!(profile.info_policy.forbid_exam_only_mentions);
    assert!(profile.exam_boilerplate_regex().is_some());
}

#[test]
fn test_builtin_general_profile() {
    let profile = DomainProfile::general();
    assert_eq!(profile.id, "general");
    assert!(!profile.exam_boilerplate.enabled);
    assert!(profile.exam_boilerplate_regex().is_none());
    assert!(!profile.info_policy.forbid_exam_only_mentions);
}

#[test]
fn test_domain_profile_by_name() {
    assert_eq!(DomainProfile::by_name("med_br").unwrap().id, "med_br");
    assert_eq!(DomainProfile::by_name("med-br").unwrap().id, "med_br");
    assert_eq!(DomainProfile::by_name("default").unwrap().id, "med_br");
    assert_eq!(DomainProfile::by_name("general").unwrap().id, "general");
    assert_eq!(DomainProfile::by_name("neutral").unwrap().id, "general");
    assert!(matches!(
        DomainProfile::by_name("unknown_prof"),
        Err(DomainProfileError::UnknownProfile(_))
    ));
}

#[test]
fn test_direct_serde_profile_loading_compiles_regex() {
    let json = include_str!("../../../profiles/med_br.json");
    let deserialized: DomainProfile =
        serde_json::from_str(json).expect("direct serde_json::from_str must succeed");
    assert!(
        deserialized.exam_boilerplate_regex().is_some(),
        "direct serde deserialization must not silently disable exam regex"
    );
    assert!(deserialized.is_exam_only_note("Cobrado no ENARE 2024."));
}

#[test]
fn test_custom_profile_invalid_regex_fails_deserialization() {
    let bad_json = r#"{
        "id": "broken",
        "name": "Broken Regex Profile",
        "description": "test",
        "language": "en",
        "default_max_note_chars": 1000,
        "table_max_note_chars": 2000,
        "exam_boilerplate": {
            "enabled": true,
            "known_labels": ["LABEL"],
            "phrase_patterns": ["(?i)(unclosed_group"]
        },
        "info_policy": {
            "source_fidelity": "ok",
            "notes_purpose": "ok",
            "preferred_list_format": "•"
        }
    }"#;

    // from_json_str returns DomainProfileError::InvalidExamRegex
    let err = DomainProfile::from_json_str(bad_json).expect_err("should fail");
    assert!(matches!(err, DomainProfileError::InvalidExamRegex { .. }));

    // direct serde also fails with descriptive error
    let serde_err = serde_json::from_str::<DomainProfile>(bad_json);
    assert!(serde_err.is_err());
}

#[test]
fn test_custom_labels_with_regex_special_characters() {
    let custom_json = r#"{
        "id": "special_chars",
        "name": "Special Chars Profile",
        "description": "test",
        "language": "pt",
        "default_max_note_chars": 1200,
        "table_max_note_chars": 2500,
        "exam_boilerplate": {
            "enabled": true,
            "known_labels": ["C++", "A&B", "USP-SP [EXTRA]"],
            "phrase_patterns": [
                "(?:prova|concurso)?\\s*(?:^|\\s)(?:{LABELS})(?:$|\\s)(?:\\s*2024)?"
            ]
        },
        "info_policy": {
            "source_fidelity": "ok",
            "notes_purpose": "ok",
            "forbid_exam_only_mentions": true,
            "preferred_list_format": "•"
        }
    }"#;

    let profile = DomainProfile::from_json_str(custom_json)
        .expect("should handle special regex chars in labels");
    assert!(profile.exam_boilerplate_regex().is_some());
    assert!(profile.is_exam_only_note("USP-SP [EXTRA] 2024"));
    assert!(profile.is_exam_only_note("prova C++"));
}

#[test]
fn test_full_policy_flags_honored_in_validation() {
    let strict_json = r#"{
        "id": "strict",
        "name": "Strict",
        "description": "strict",
        "language": "pt",
        "default_max_note_chars": 500,
        "table_max_note_chars": 1000,
        "exam_boilerplate": {
            "enabled": true,
            "known_labels": ["USP"],
            "phrase_patterns": ["(?:USP)"]
        },
        "info_policy": {
            "source_fidelity": "ok",
            "notes_purpose": "ok",
            "forbid_metadata_headers": true,
            "forbid_verbatim_answer_restatement": true,
            "forbid_exam_only_mentions": true,
            "preferred_list_format": "• <b>Tópico:</b> Explicação."
        }
    }"#;

    let strict_profile = DomainProfile::from_json_str(strict_json).unwrap();
    let diags_strict_meta = check_notes_with_profile(
        "ID: 1<br>Explicação clínica.",
        "Alfa",
        Some(1),
        "Notas",
        None,
        &strict_profile,
    );
    assert!(diags_strict_meta.iter().any(|d| d.code == "NOTE.METADATA"));

    let diags_strict_exam =
        check_notes_with_profile("USP", "Alfa", Some(1), "Notas", None, &strict_profile);
    assert!(
        diags_strict_exam
            .iter()
            .any(|d| d.code == "NOTE.EXAM_ONLY_WARN")
    );

    let diags_strict_dup = check_notes_with_profile(
        "Resposta correta: Alfa",
        "Alfa",
        Some(1),
        "Notas",
        None,
        &strict_profile,
    );
    assert!(
        diags_strict_dup
            .iter()
            .any(|d| d.code == "NOTE.DUP_ANSWER_WARN")
    );

    let relaxed_json = r#"{
        "id": "relaxed",
        "name": "Relaxed",
        "description": "relaxed",
        "language": "pt",
        "default_max_note_chars": 500,
        "table_max_note_chars": 1000,
        "exam_boilerplate": {
            "enabled": true,
            "known_labels": ["USP"],
            "phrase_patterns": ["(?:USP)"]
        },
        "info_policy": {
            "source_fidelity": "ok",
            "notes_purpose": "ok",
            "forbid_metadata_headers": false,
            "forbid_verbatim_answer_restatement": false,
            "forbid_exam_only_mentions": false,
            "preferred_list_format": "• <b>Tópico:</b> Explicação."
        }
    }"#;

    let relaxed_profile = DomainProfile::from_json_str(relaxed_json).unwrap();
    let diags_relaxed_meta = check_notes_with_profile(
        "ID: 1<br>Explicação clínica.",
        "Alfa",
        Some(1),
        "Notas",
        None,
        &relaxed_profile,
    );
    assert!(!diags_relaxed_meta.iter().any(|d| d.code == "NOTE.METADATA"));
    let diags_relaxed_dup = check_notes_with_profile(
        "Resposta correta: Alfa",
        "Alfa",
        Some(1),
        "Notas",
        None,
        &relaxed_profile,
    );
    assert!(
        !diags_relaxed_dup
            .iter()
            .any(|d| d.code == "NOTE.DUP_ANSWER_WARN")
    );
    let diags_relaxed_exam =
        check_notes_with_profile("USP", "Alfa", Some(1), "Notas", None, &relaxed_profile);
    assert!(
        !diags_relaxed_exam
            .iter()
            .any(|d| d.code == "NOTE.EXAM_ONLY_WARN")
    );
}

// ============================================================================
// 4. Exam-Only False Positives & Diagnostic Ordering Tests
// ============================================================================

#[test]
fn test_exam_only_false_positives_comprehensive() {
    let non_exam_cases = [
        "Caiu na prova de 2024. A meia-vida longa explica o platô tardio.",
        "Tema cobrado em prova: segue cinética de ordem zero em doses terapêuticas.",
        "Prova calórica ausente indica arreflexia vestibular periférica.",
        "Caiu na prova de 2024 por distinguir o grupo alfa do beta.",
        "Caiu na prova de 2024 de mecanismo ainda desconhecido na literatura.",
        "Caiu na prova de 2024 para distinguir o grupo alfa do beta na prática.",
        "Muito cobrado em provas na diferenciação entre crise focal e síncope.",
        "Já caiu em prova em pacientes com hepatopatia crônica para evitar toxicidade.",
        "A prova de função pulmonar demonstrou padrão obstrutivo com resposta ao broncodilatador.",
        "Cobrado na prova da USP de 2024 quando associado a choque cardiogênico refratário.",
    ];

    for case in non_exam_cases {
        assert!(
            !is_exam_only_note(case),
            "Should not be flagged as exam-only: {case}"
        );
        let diags = check_notes(case, "", None);
        assert!(
            !diags.iter().any(|d| d.code == "NOTE.EXAM_ONLY_WARN"),
            "False positive NOTE.EXAM_ONLY_WARN on: {case}"
        );
    }
}

#[test]
fn test_exam_only_true_positives() {
    let true_cases = [
        "Já caiu em prova.",
        "Caiu na prova de 2024.",
        "<p>• <b>Prova:</b> Muito cobrado em provas de residência.</p>",
        "Caiu na prova da USP de 2024.",
        "Prova do ENARE 2024.",
        "USP 2023<br>UNIFESP 2024",
        "Cobrado na prova da USP 2023. Tema recorrente no ENARE!",
        "Alta incidência em provas de residência médica.",
        "Pegadinha clássica para as provas de residência.",
    ];

    for case in true_cases {
        assert!(
            is_exam_only_note(case),
            "Should be flagged as exam-only: {case}"
        );
        let diags = check_notes(case, "", None);
        assert!(
            diags.iter().any(|d| d.code == "NOTE.EXAM_ONLY_WARN"),
            "Expected NOTE.EXAM_ONLY_WARN on: {case}"
        );
    }
}

#[test]
fn test_diagnostic_ordering_preserved() {
    let text = "ID: 1<br>Resposta correta: Alfa<br>USP 2024<br>• Item sem negrito<br>Longa: "
        .to_string()
        + &"x".repeat(1500);
    let diags = check_notes_ext(
        &text,
        "Alfa",
        Some(1),
        "Notas Adicionais",
        DEFAULT_MAX_LENGTH,
    );

    let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
    let expected_order = [
        "NOTE.METADATA",
        "NOTE.DUP_ANSWER_WARN",
        "NOTE.EXAM_ONLY_WARN",
        "NOTE.LENGTH_WARN",
        "NOTE.STYLE_WARN",
    ];

    for window in codes.windows(2) {
        let idx0 = expected_order.iter().position(|&c| c == window[0]);
        let idx1 = expected_order.iter().position(|&c| c == window[1]);
        if let (Some(i0), Some(i1)) = (idx0, idx1) {
            assert!(i0 <= i1, "Diagnostics out of expected order: {codes:?}");
        }
    }
}
