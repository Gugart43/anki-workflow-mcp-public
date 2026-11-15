use anki_domain::{CLOZE_HEADER_CANONICAL as CL, QA_HEADER_CANONICAL as QA, Severity};
use anki_validate::review_content;

fn qa_csv(rows: &[(&str, &str, &str, &str, &str, &str, &str)]) -> String {
    let mut out = String::from(QA);
    for (enunciado, resposta, campo, notas, imagens, refs, tags) in rows {
        out.push('\n');
        out.push_str(&format!(
            "{enunciado}|{resposta}|{campo}|{notas}|{imagens}|{refs}|{tags}"
        ));
    }
    out
}

fn cl_csv(rows: &[(&str, &str, &str, &str, &str, &str)]) -> String {
    let mut out = String::from(CL);
    for (sentenca, notas, campo, imagens, refs, tags) in rows {
        out.push('\n');
        out.push_str(&format!(
            "{sentenca}|{notas}|{campo}|{imagens}|{refs}|{tags}"
        ));
    }
    out
}

#[test]
fn formatted_binary_choices_are_checked_as_visible_text() {
    let csv = cl_csv(&[(
        "Synthetic marker is {{c1::Alpha}} [<b>A</b> / <b>B</b>].",
        "",
        "Testing",
        "",
        "Synthetic 2026",
        "test",
    )]);
    assert!(
        review_content(QA, &csv)
            .iter()
            .any(|d| d.code == "QUALITY.BINARY_CHOICE_CLOZE")
    );
}

#[test]
fn missing_reference_warns_for_qa_and_cloze() {
    let qa = qa_csv(&[(
        "Em paciente adulto com síndrome X sintética, qual é o marcador de primeira linha?",
        "Marcador Alfa",
        "Neurologia Sintética",
        "Nota explicativa sintética.",
        "",
        "",
        "neuro_sintetica",
    )]);
    let cl = cl_csv(&[(
        "Na síndrome X sintética em adultos, o achado inicial é {{c1::sinal alfa}} conforme protocolo.",
        "Nota explicativa sintética.",
        "Neurologia Sintética",
        "",
        "   <span>  </span> ",
        "neuro_sintetica",
    )]);

    let qa_diags = review_content(&qa, CL);
    assert!(qa_diags.iter().any(|d| {
        d.code == "QUALITY.MISSING_REFERENCE"
            && d.severity == Severity::Warn
            && d.owner_skill == "anki-post-curator"
            && d.row == Some(2)
            && d.field.as_deref() == Some("Referências")
    }));

    let cl_diags = review_content(QA, &cl);
    assert!(cl_diags.iter().any(|d| {
        d.code == "QUALITY.MISSING_REFERENCE"
            && d.severity == Severity::Warn
            && d.owner_skill == "anki-post-curator"
            && d.row == Some(2)
            && d.field.as_deref() == Some("Referências")
    }));
}

#[test]
fn generic_reference_warns_and_verifiable_reference_passes() {
    let generic_refs = [
        "fluxograma",
        "Fluxograma clínico",
        "TDC",
        "resumo",
        "aula",
        "Apostila do módulo",
        "UpToDate",
        "anotação",
        "anotações de aula",
        "livro",
        "Diretriz clínica",
    ];
    for g_ref in generic_refs {
        let qa = qa_csv(&[(
            "Em paciente adulto com síndrome X sintética, qual é o marcador de primeira linha?",
            "Marcador Alfa",
            "Neurologia Sintética",
            "Nota explicativa sintética.",
            "",
            g_ref,
            "neuro_sintetica",
        )]);
        let diags = review_content(&qa, CL);
        assert!(
            diags.iter().any(|d| {
                d.code == "QUALITY.GENERIC_REFERENCE"
                    && d.severity == Severity::Warn
                    && d.owner_skill == "anki-post-curator"
                    && d.row == Some(2)
                    && d.field.as_deref() == Some("Referências")
            }),
            "expected QUALITY.GENERIC_REFERENCE for '{g_ref}', got: {diags:?}"
        );
    }

    let valid_refs = [
        "Protocolo Alfa 2026",
        "Manual Sintético, Cap. 4, p. 112",
        "Diretriz Alfa, Seção 2.1",
        "Compêndio Sintético, 3ª ed.",
        "Estudo Alfa (https://exemplo.invalid/protocolo-alfa)",
        "Consenso Sintético, doi:10.1000/alfa",
    ];
    for v_ref in valid_refs {
        let qa = qa_csv(&[(
            "Em paciente adulto com síndrome X sintética, qual é o marcador de primeira linha?",
            "Marcador Alfa",
            "Neurologia Sintética",
            "Nota explicativa sintética.",
            "",
            v_ref,
            "neuro_sintetica",
        )]);
        let diags = review_content(&qa, CL);
        assert!(
            !diags.iter().any(|d| {
                d.code == "QUALITY.MISSING_REFERENCE" || d.code == "QUALITY.GENERIC_REFERENCE"
            }),
            "unexpected reference warning for '{v_ref}', got: {diags:?}"
        );
    }
}

#[test]
fn binary_choice_cloze_critical_without_false_positive_on_mask_or_units() {
    let qa_stem = qa_csv(&[(
        "Na síndrome X sintética, o tônus encontra-se [aumentado / diminuído] na fase aguda.",
        "Aumentado",
        "Neurologia Sintética",
        "Nota explicativa.",
        "",
        "Protocolo Alfa 2026",
        "neuro_sintetica",
    )]);
    assert!(review_content(&qa_stem, CL).iter().any(|d| {
        d.code == "QUALITY.BINARY_CHOICE_CLOZE"
            && d.severity == Severity::Critical
            && d.owner_skill == "anki-post-curator"
            && d.row == Some(2)
    }));

    let qa_ans = qa_csv(&[(
        "Na síndrome X sintética, qual o resultado esperado do teste alfa?",
        "[positivo / negativo]",
        "Neurologia Sintética",
        "Nota explicativa.",
        "",
        "Protocolo Alfa 2026",
        "neuro_sintetica",
    )]);
    assert!(
        review_content(&qa_ans, CL).iter().any(|d| {
            d.code == "QUALITY.BINARY_CHOICE_CLOZE" && d.severity == Severity::Critical
        })
    );

    let cl_inside = cl_csv(&[(
        "Na síndrome X sintética, o protocolo {{c1::[indica / contraindica]}} o agente alfa e o resultado é {{c2::[sim/não]}}.",
        "Nota explicativa.",
        "Neurologia Sintética",
        "",
        "Protocolo Alfa 2026",
        "neuro_sintetica",
    )]);
    assert!(review_content(QA, &cl_inside).iter().any(|d| {
        d.code == "QUALITY.BINARY_CHOICE_CLOZE"
            && d.severity == Severity::Critical
            && d.row == Some(2)
    }));

    let cl_clean = cl_csv(&[(
        "Na síndrome X sintética, a faixa de referência [...] fica em [mg/dL] com esquema [12/12h] para {{c1::agente alfa}}.",
        "Nota explicativa.",
        "Neurologia Sintética",
        "",
        "Protocolo Alfa 2026",
        "neuro_sintetica",
    )]);
    assert!(
        !review_content(QA, &cl_clean)
            .iter()
            .any(|d| d.code == "QUALITY.BINARY_CHOICE_CLOZE")
    );
}

#[test]
fn allinone_mc_leak_critical_for_commands_and_binary_vectors() {
    let qa_cmd = qa_csv(&[
        (
            "Sobre a síndrome X sintética, é incorreto afirmar que:",
            "Achado gama",
            "Neurologia Sintética",
            "Nota.",
            "",
            "Protocolo Alfa 2026",
            "neuro_sintetica",
        ),
        (
            "Assinale a alternativa correta sobre o Protocolo Alfa:",
            "Opção delta",
            "Neurologia Sintética",
            "Nota.",
            "",
            "Protocolo Alfa 2026",
            "neuro_sintetica",
        ),
        (
            "Em paciente adulto, marque a alternativa falsa. EXCETO:",
            "Opção beta",
            "Neurologia Sintética",
            "Nota.",
            "",
            "Protocolo Alfa 2026",
            "neuro_sintetica",
        ),
        (
            "Qual o gabarito da questão sintética?",
            "1 0 0 0 0",
            "Neurologia Sintética",
            "Nota.",
            "",
            "Protocolo Alfa 2026",
            "neuro_sintetica",
        ),
    ]);
    let qa_diags = review_content(&qa_cmd, CL);
    for row in 2..=5 {
        assert!(
            qa_diags.iter().any(|d| {
                d.code == "QUALITY.ALLINONE_MC_LEAK"
                    && d.severity == Severity::Critical
                    && d.owner_skill == "anki-post-curator"
                    && d.row == Some(row)
            }),
            "expected QUALITY.ALLINONE_MC_LEAK on QA row {row}, got: {qa_diags:?}"
        );
    }

    let cl_vec = cl_csv(&[(
        "Na síndrome X sintética, o padrão de resposta {{c1::0 1 0 0}} indica alternativa B.",
        "Nota.",
        "Neurologia Sintética",
        "",
        "Protocolo Alfa 2026",
        "neuro_sintetica",
    )]);
    assert!(review_content(QA, &cl_vec).iter().any(|d| {
        d.code == "QUALITY.ALLINONE_MC_LEAK" && d.severity == Severity::Critical && d.row == Some(2)
    }));

    let clean = qa_csv(&[(
        "Na síndrome X sintética, todos os marcadores se elevam, exceto o marcador alfa?",
        "Marcador alfa permanece normal",
        "Neurologia Sintética",
        "Nota.",
        "",
        "Protocolo Alfa 2026",
        "neuro_sintetica",
    )]);
    assert!(
        !review_content(&clean, CL)
            .iter()
            .any(|d| d.code == "QUALITY.ALLINONE_MC_LEAK")
    );
}

#[test]
fn dual_target_qa_warns_without_false_positive_on_single_target() {
    let dual_stems = [
        "Em paciente adulto com síndrome X sintética, qual é a droga de primeira linha e qual a dose inicial?",
        "Na síndrome X sintética, como diagnosticar e como tratar na fase aguda?",
        "No Protocolo Alfa, quais os achados precoces versus quais os achados tardios?",
        "Em adultos com síndrome X sintética, qual é o fármaco de escolha e o mecanismo de ação?",
    ];
    for stem in dual_stems {
        let qa = qa_csv(&[(
            stem,
            "Agente Alfa",
            "Neurologia Sintética",
            "Nota.",
            "",
            "Protocolo Alfa 2026",
            "neuro_sintetica",
        )]);
        let diags = review_content(&qa, CL);
        assert!(
            diags.iter().any(|d| {
                d.code == "QUALITY.DUAL_TARGET_QA"
                    && d.severity == Severity::Warn
                    && d.owner_skill == "anki-post-curator"
                    && d.row == Some(2)
                    && d.field.as_deref() == Some("Enunciado")
            }),
            "expected QUALITY.DUAL_TARGET_QA for '{stem}', got: {diags:?}"
        );
    }

    let single_stems = [
        "Em paciente adulto com síndrome X sintética, qual é o fármaco usado como primeira linha segundo o Protocolo Alfa?",
        "Na síndrome X sintética, qual a principal diferença clínica entre a forma alfa e a forma beta?",
    ];
    for stem in single_stems {
        let qa = qa_csv(&[(
            stem,
            "Agente Alfa",
            "Neurologia Sintética",
            "Nota.",
            "",
            "Protocolo Alfa 2026",
            "neuro_sintetica",
        )]);
        assert!(
            !review_content(&qa, CL)
                .iter()
                .any(|d| d.code == "QUALITY.DUAL_TARGET_QA"),
            "unexpected QUALITY.DUAL_TARGET_QA for '{stem}'"
        );
    }
}

#[test]
fn answer_explanation_mix_warns_only_when_notes_empty_and_answer_long_or_multisegment() {
    let long_ans = "Agente Alfa em dose padrão de ataque seguido de manutenção prolongada com monitorização clínica periódica conforme o Protocolo Alfa revisado para pacientes adultos com função preservada e sem contraindicações sistêmicas relevantes no cenário hospitalar.";
    assert!(long_ans.chars().count() > 220);

    let qa_long = qa_csv(&[(
        "Na síndrome X sintética, qual é a conduta terapêutica?",
        long_ans,
        "Neurologia Sintética",
        "",
        "",
        "Protocolo Alfa 2026",
        "neuro_sintetica",
    )]);
    assert!(review_content(&qa_long, CL).iter().any(|d| {
        d.code == "QUALITY.ANSWER_EXPLANATION_MIX"
            && d.severity == Severity::Warn
            && d.owner_skill == "anki-post-curator"
            && d.row == Some(2)
            && d.field.as_deref() == Some("Resposta")
    }));

    let qa_three_sentences = qa_csv(&[(
        "Na síndrome X sintética, qual é o mecanismo principal?",
        "Bloqueio do canal alfa. Isso reduz a excitabilidade neuronal. Consequentemente previne crises.",
        "Neurologia Sintética",
        "",
        "",
        "Protocolo Alfa 2026",
        "neuro_sintetica",
    )]);
    assert!(review_content(&qa_three_sentences, CL).iter().any(|d| {
        d.code == "QUALITY.ANSWER_EXPLANATION_MIX"
            && d.severity == Severity::Warn
            && d.field.as_deref() == Some("Resposta")
    }));

    // With Notas Adicionais populated, should NOT emit QUALITY.ANSWER_EXPLANATION_MIX
    let qa_with_notes = qa_csv(&[(
        "Na síndrome X sintética, qual é o mecanismo principal?",
        "Bloqueio do canal alfa. Isso reduz a excitabilidade neuronal. Consequentemente previne crises.",
        "Neurologia Sintética",
        "Detalhe adicional em Notas Adicionais.",
        "",
        "Protocolo Alfa 2026",
        "neuro_sintetica",
    )]);
    assert!(
        !review_content(&qa_with_notes, CL)
            .iter()
            .any(|d| d.code == "QUALITY.ANSWER_EXPLANATION_MIX")
    );

    // Decimal single sentence <= 220 chars with empty notes should NOT trigger
    let qa_decimal = qa_csv(&[(
        "Na síndrome X sintética, qual a dose de ataque?",
        "1.5 mg/kg em infusão única.",
        "Neurologia Sintética",
        "",
        "",
        "Protocolo Alfa 2026",
        "neuro_sintetica",
    )]);
    assert!(
        !review_content(&qa_decimal, CL)
            .iter()
            .any(|d| d.code == "QUALITY.ANSWER_EXPLANATION_MIX")
    );
}

#[test]
fn list_scaffold_warn_for_single_cloze_ul_with_three_items() {
    let cl_bad = cl_csv(&[(
        "Na síndrome X sintética, o {{c1::critério diagnóstico}} inclui:<ul><li>Achado alfa</li><li>Achado beta</li><li>Achado gama</li></ul>",
        "Nota.",
        "Neurologia Sintética",
        "",
        "Protocolo Alfa 2026",
        "neuro_sintetica",
    )]);
    assert!(review_content(QA, &cl_bad).iter().any(|d| {
        d.code == "QUALITY.LIST_SCAFFOLD_WARN"
            && d.severity == Severity::Warn
            && d.owner_skill == "anki-post-curator"
            && d.row == Some(2)
            && d.field.as_deref() == Some("Sentença")
    }));

    // 2 distinct cloze indices should NOT trigger
    let cl_two_indices = cl_csv(&[(
        "Na síndrome X sintética, critérios incluem:<ul><li>{{c1::Achado alfa}}</li><li>{{c2::Achado beta}}</li><li>Achado gama</li></ul>",
        "Nota.",
        "Neurologia Sintética",
        "",
        "Protocolo Alfa 2026",
        "neuro_sintetica",
    )]);
    assert!(
        !review_content(QA, &cl_two_indices)
            .iter()
            .any(|d| d.code == "QUALITY.LIST_SCAFFOLD_WARN")
    );

    // Only 2 <li> items should NOT trigger
    let cl_two_items = cl_csv(&[(
        "Na síndrome X sintética, o {{c1::critério diagnóstico}} inclui:<ul><li>Achado alfa</li><li>Achado beta</li></ul>",
        "Nota.",
        "Neurologia Sintética",
        "",
        "Protocolo Alfa 2026",
        "neuro_sintetica",
    )]);
    assert!(
        !review_content(QA, &cl_two_items)
            .iter()
            .any(|d| d.code == "QUALITY.LIST_SCAFFOLD_WARN")
    );
}

#[test]
fn missing_media_tag_critical_and_generic_img_alt_warn() {
    let qa = qa_csv(&[
        (
            "Na síndrome X sintética, qual o achado de imagem?",
            "Sinal alfa",
            "Neurologia Sintética",
            "Nota.",
            r#"<figure class="anki-figure"><img src="alfa_p1_1.png" alt="imagem"><figcaption>Achado sintético alfa demonstrado na sequência axial.</figcaption></figure>"#,
            "Protocolo Alfa 2026",
            "neuro_sintetica MÍDIA-AUSENTE",
        ),
        (
            "Na síndrome X sintética, qual o achado complementar?<img src=\"alfa_p2_1.png\">",
            "Sinal beta",
            "Neurologia Sintética",
            "Nota.",
            r#"<figure class="anki-figure"><img src="alfa_p3_1.png" alt=""><figcaption>Achado sintético beta demonstrado na sequência coronal.</figcaption></figure>"#,
            "Protocolo Alfa 2026",
            "neuro_sintetica",
        ),
    ]);
    let cl = cl_csv(&[(
        "Na síndrome X sintética, observa-se {{c1::sinal gama}} na avaliação.",
        "Nota.",
        "Neurologia Sintética",
        r#"<figure class="anki-figure"><img src="alfa_p4_1.png" alt="Sinal gama hiperintenso em corte sintético"><figcaption>Sinal gama hiperintenso em corte sintético demonstrativo.</figcaption></figure>"#,
        "Protocolo Alfa 2026",
        "neuro_sintetica midia-ausente",
    )]);

    let qa_diags = review_content(&qa, CL);
    assert!(qa_diags.iter().any(|d| {
        d.code == "QUALITY.MISSING_MEDIA_TAG"
            && d.severity == Severity::Critical
            && d.owner_skill == "anki-post-curator"
            && d.row == Some(2)
            && d.field.as_deref() == Some("Etiquetas")
    }));
    assert!(qa_diags.iter().any(|d| {
        d.code == "QUALITY.GENERIC_IMG_ALT"
            && d.severity == Severity::Warn
            && d.owner_skill == "anki-post-curator"
            && d.row == Some(2)
            && d.field.as_deref() == Some("Imagens Relacionadas")
    }));
    assert!(qa_diags.iter().any(|d| {
        d.code == "QUALITY.GENERIC_IMG_ALT"
            && d.severity == Severity::Warn
            && d.row == Some(3)
            && d.field.as_deref() == Some("Enunciado")
    }));
    assert!(qa_diags.iter().any(|d| {
        d.code == "QUALITY.GENERIC_IMG_ALT"
            && d.severity == Severity::Warn
            && d.row == Some(3)
            && d.field.as_deref() == Some("Imagens Relacionadas")
    }));

    let cl_diags = review_content(QA, &cl);
    assert!(cl_diags.iter().any(|d| {
        d.code == "QUALITY.MISSING_MEDIA_TAG"
            && d.severity == Severity::Critical
            && d.row == Some(2)
            && d.field.as_deref() == Some("Etiquetas")
    }));
    assert!(!cl_diags.iter().any(|d| d.code == "QUALITY.GENERIC_IMG_ALT"));
}

#[test]
fn near_duplicate_warn_triggers_within_same_study_field_only() {
    let qa_near = qa_csv(&[
        (
            "Em paciente adulto com síndrome X sintética aguda, qual o fármaco de primeira linha no Protocolo Alfa?",
            "Agente Alfa",
            "Neurologia Sintética",
            "Nota 1.",
            "",
            "Protocolo Alfa 2026",
            "neuro_sintetica",
        ),
        (
            "Em paciente adulto com síndrome X sintética aguda, qual o medicamento de primeira linha no Protocolo Alfa?",
            "Agente Alfa",
            "Neurologia Sintética",
            "Nota 2.",
            "",
            "Protocolo Alfa 2026",
            "neuro_sintetica",
        ),
    ]);
    let diags_same = review_content(&qa_near, CL);
    assert!(diags_same.iter().any(|d| {
        d.code == "QUALITY.NEAR_DUPLICATE_WARN"
            && d.severity == Severity::Warn
            && d.owner_skill == "anki-post-curator"
            && d.row == Some(3)
    }));
    assert!(
        !diags_same
            .iter()
            .any(|d| d.code == "QUALITY.DUPLICATE_RETRIEVAL")
    );

    // Different Campo de Estudo -> should NOT emit QUALITY.NEAR_DUPLICATE_WARN
    let qa_diff_field = qa_csv(&[
        (
            "Em paciente adulto com síndrome X sintética aguda, qual o fármaco de primeira linha no Protocolo Alfa?",
            "Agente Alfa",
            "Neurologia Sintética",
            "Nota 1.",
            "",
            "Protocolo Alfa 2026",
            "neuro_sintetica",
        ),
        (
            "Em paciente adulto com síndrome X sintética aguda, qual o medicamento de primeira linha no Protocolo Alfa?",
            "Agente Alfa",
            "Emergência Sintética",
            "Nota 2.",
            "",
            "Protocolo Alfa 2026",
            "neuro_sintetica",
        ),
    ]);
    assert!(
        !review_content(&qa_diff_field, CL)
            .iter()
            .any(|d| d.code == "QUALITY.NEAR_DUPLICATE_WARN")
    );

    // Exact duplicate -> QUALITY.DUPLICATE_RETRIEVAL, NOT QUALITY.NEAR_DUPLICATE_WARN
    let qa_exact = qa_csv(&[
        (
            "Em paciente adulto com síndrome X sintética aguda, qual o fármaco de primeira linha no Protocolo Alfa?",
            "Agente Alfa",
            "Neurologia Sintética",
            "Nota 1.",
            "",
            "Protocolo Alfa 2026",
            "neuro_sintetica",
        ),
        (
            "Em paciente adulto com síndrome X sintética aguda, qual o fármaco de primeira linha no Protocolo Alfa?",
            "Agente Alfa",
            "Neurologia Sintética",
            "Nota 2.",
            "",
            "Protocolo Alfa 2026",
            "neuro_sintetica",
        ),
    ]);
    let diags_exact = review_content(&qa_exact, CL);
    assert!(
        diags_exact
            .iter()
            .any(|d| d.code == "QUALITY.DUPLICATE_RETRIEVAL")
    );
    assert!(
        !diags_exact
            .iter()
            .any(|d| d.code == "QUALITY.NEAR_DUPLICATE_WARN")
    );

    // Multi-index Cloze note on the same row (c1 and c2) must NOT flag itself as near-duplicate
    let cl_multi = cl_csv(&[(
        "Na síndrome X sintética aguda segundo o Protocolo Alfa 2026, o achado inicial é {{c1::sinal alfa}} e a confirmação usa {{c2::exame gama}}.",
        "Nota.",
        "Neurologia Sintética",
        "",
        "Protocolo Alfa 2026",
        "neuro_sintetica",
    )]);
    assert!(
        !review_content(QA, &cl_multi)
            .iter()
            .any(|d| d.code == "QUALITY.NEAR_DUPLICATE_WARN")
    );
}
