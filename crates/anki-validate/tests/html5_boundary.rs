//! Comprehensive integration tests for the HTML5 active-content boundary (ammonia + F2 allowlist).

use anki_domain::{CLOZE_HEADER_CANONICAL, NoteType, QA_HEADER_CANONICAL};
use anki_validate::csv_structure::check_csv;
use anki_validate::media_figure::{MediaFigure, render_media_figure};
use anki_validate::sanitize::sanitize_field;
use anki_validate::table_render::TableSpec;
use anki_validate::vocab::{ALLOWED_CLASSES, ALLOWED_TAGS, is_always_allowed_tag, unknown_classes};
use anki_validate::{QualityInput, QualityMode, compile_drafts, prepare_quality};
use schemars::schema_for;
use serde_json::json;

#[test]
fn f2_semantic_tags_accepted_in_compiler_and_gate() {
    let semantic_notes = r#"<section class="anki-note-section"><p><b>Mecanismo:</b> Fisiopatologia</p><dl class="anki-kv"><dt>Chave</dt><dd>Valor</dd></dl><details class="anki-detail"><summary><b>Mais detalhes</b></summary><p>Conteúdo colapsável</p></details><footer class="anki-sources"><small><b>Fonte:</b> <cite>Diretriz SBC 2026</cite></small></footer></section>"#;

    let draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Qual o mecanismo de ação?",
        "resposta": "Bloqueio de canal de sódio.",
        "notas_adicionais": semantic_notes
    });

    let compiled = compile_drafts(&[draft], true, true, "")
        .expect("semantic F2 tags must compile successfully");
    let qa_csv = compiled["qa_csv"].as_str().expect("valid csv string");
    assert!(qa_csv.contains("anki-note-section"));
    assert!(qa_csv.contains("anki-kv"));
    assert!(qa_csv.contains("anki-detail"));
    assert!(qa_csv.contains("anki-sources"));

    let input = QualityInput {
        qa_csv: qa_csv.to_owned(),
        cloze_csv: CLOZE_HEADER_CANONICAL.to_owned(),
        mode: QualityMode::Strict,
        sources: vec![],
        obligations: vec![],
        bindings: vec![],
        reviews: vec![],
    };
    let report = prepare_quality(&input);
    let unsafe_diags: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "QUALITY.UNSAFE_HTML")
        .collect();
    assert!(
        unsafe_diags.is_empty(),
        "semantic HTML must not trigger QUALITY.UNSAFE_HTML: {unsafe_diags:?}"
    );
}

#[test]
fn f2_callout_variants_and_flow_classes_accepted() {
    let callout_notes = r#"<blockquote class="anki-callout anki-callout--trap"><p><b>Atenção:</b> Armadilha clássica</p></blockquote><blockquote class="anki-callout anki-callout--pearl"><p><b>Dica:</b> Pérola clínica</p></blockquote><blockquote class="anki-callout anki-callout--diff"><p><b>Diferencial:</b> Diagnóstico alternativo</p></blockquote><section class="anki-list"><ul><li>Sintoma 1</li><li>Sintoma 2</li></ul></section><section class="anki-steps"><ol><li>Primeiro passo</li><li>Segundo passo</li></ol></section><section class="anki-flow"><ul><li>Nó de decisão</li></ul></section>"#;

    let draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Diagnóstico diferencial?",
        "resposta": "Avaliar callouts e etapas.",
        "notas_adicionais": callout_notes
    });

    let compiled = compile_drafts(&[draft], true, true, "")
        .expect("callouts and flow classes must compile successfully");
    let qa_csv = compiled["qa_csv"].as_str().expect("valid csv");
    assert!(qa_csv.contains("anki-callout--trap"));
    assert!(qa_csv.contains("anki-callout--pearl"));
    assert!(qa_csv.contains("anki-callout--diff"));
    assert!(qa_csv.contains("anki-list"));
    assert!(qa_csv.contains("anki-steps"));
    assert!(qa_csv.contains("anki-flow"));
}

#[test]
fn clinical_inequalities_and_entities_preserved_and_safe() {
    let clinical_payloads = [
        "K+ > 5.5 mEq/L indica hipercalemia",
        "pH < 7.35 define acidemia",
        "Troponina I <= 0.04 ng/mL normal",
        "Glicemia de jejum >= 126 mg/dL",
        "Associação entre Fator A & Fator B",
        "Escape de pipe com &#124; na nota",
    ];

    for payload in clinical_payloads {
        let draft = json!({
            "tipo": "QA",
            "enunciado_ou_sentenca": format!("Pergunta sobre {payload}?"),
            "resposta": payload,
            "notas_adicionais": format!("<p>{payload}</p>")
        });

        let compiled = compile_drafts(&[draft], true, true, "")
            .unwrap_or_else(|e| panic!("clinical inequality '{payload}' must be accepted: {e}"));
        let qa_csv = compiled["qa_csv"].as_str().expect("valid csv");

        let input = QualityInput {
            qa_csv: qa_csv.to_owned(),
            cloze_csv: CLOZE_HEADER_CANONICAL.to_owned(),
            mode: QualityMode::Strict,
            sources: vec![],
            obligations: vec![],
            bindings: vec![],
            reviews: vec![],
        };
        let report = prepare_quality(&input);
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|d| d.code == "QUALITY.UNSAFE_HTML"),
            "clinical content '{payload}' must not trigger UNSAFE_HTML"
        );
    }
}

#[test]
fn table_normalization_handles_tbody_insertion() {
    let raw_table = "<table><tr><th>Exame</th><th>Referência</th></tr><tr><td>Potássio</td><td>3.5-5.0</td></tr></table>";
    let draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Valores de referência?",
        "resposta": "Ver tabela abaixo.",
        "notas_adicionais": raw_table
    });

    let compiled = compile_drafts(&[draft], true, true, "")
        .expect("table without tbody must be accepted through benign diff normalization");
    let qa_csv = compiled["qa_csv"].as_str().expect("valid csv");
    assert!(qa_csv.contains("Exame"));
}

#[test]
fn active_content_attacks_fail_closed() {
    let attack_payloads = [
        ("<script>alert(1)</script>", "script tag"),
        (
            "<script src=\"https://evil.com/x.js\"></script>",
            "external script",
        ),
        ("<svg onload=\"alert(1)\"></svg>", "svg onload"),
        (
            "<iframe src=\"javascript:alert(1)\"></iframe>",
            "iframe javascript",
        ),
        ("<img src=\"x\" onerror=\"alert(1)\">", "img onerror"),
        ("<b onclick=\"alert(1)\">Clique</b>", "b onclick"),
        ("<b ONCLICK='alert(1)'>Clique</b>", "mixed-case onclick"),
        (
            "<a href=\"javascript:alert(1)\">Link</a>",
            "a href javascript",
        ),
        ("<object data=\"evil.swf\"></object>", "object tag"),
        ("<embed src=\"evil.swf\">", "embed tag"),
        (
            "<form action=\"https://evil.com\"><input type=\"text\"></form>",
            "form tag",
        ),
        (
            "<span style=\"background:url(https://evil.com/x)\">CSS url</span>",
            "css url",
        ),
        (
            "<span style=\"background:u&#114;l(x)\">CSS entity url</span>",
            "css entity url",
        ),
        (
            "<span style=\"background:u\\72l(x)\">CSS escape url</span>",
            "css escape url",
        ),
        (
            "<span style=\"expression(alert(1))\">CSS expression</span>",
            "css expression",
        ),
        (
            "<span style=\"behavior:url(evil.htc)\">CSS behavior</span>",
            "css behavior",
        ),
        (
            "<span style=\"/*comment*/color:red\">CSS comment</span>",
            "css comment",
        ),
        ("<img src=\"../traversal.png\">", "path traversal src"),
        ("<img src=\"https://evil.com/pic.png\">", "remote image src"),
        ("<img src=\"data:image/png;base64,AAAA\">", "data uri src"),
        (
            "<div class=\"unauthorized-random-class\">Div</div>",
            "unknown class",
        ),
        ("<p id=\"target\">P com id</p>", "disallowed attribute id"),
        // B13: a decisão estrutural não pode afrouxar nenhum destes.
        ("<svg/onload=alert(1)>", "svg slash onload"),
        ("<SVG ONLOAD=\"alert(1)\"></SVG>", "uppercase svg onload"),
        (
            "<span style=\"background:url(javascript:alert(1))\">CSS url javascript</span>",
            "css url javascript",
        ),
        (
            "<span STYLE=\"background:url(javascript:alert(1))\">CSS url upper</span>",
            "css url javascript uppercase attribute",
        ),
        (
            "<meta http-equiv=\"refresh\" content=\"0;url=javascript:alert(1)\">",
            "meta refresh",
        ),
        (
            "<template><script>alert(1)</script></template>",
            "template with script",
        ),
        (
            "<!--<script>alert(1)</script>-->",
            "script hidden in comment",
        ),
        ("<b onclick=alert(1)>Sem aspas</b>", "unquoted onclick"),
        (
            "<span class=\"destaque\"><script>alert(1)</script></span>",
            "script inside allowed class",
        ),
    ];

    for (payload, description) in attack_payloads {
        let draft = json!({
            "tipo": "QA",
            "enunciado_ou_sentenca": "Pergunta de teste?",
            "resposta": payload
        });
        assert!(
            compile_drafts(&[draft], true, true, "").is_err(),
            "attack must be rejected at compiler boundary: {description} ({payload})"
        );

        let input = QualityInput {
            qa_csv: format!("{QA_HEADER_CANONICAL}\nPergunta?|{payload}|Notas|||Ref|tag"),
            cloze_csv: CLOZE_HEADER_CANONICAL.to_owned(),
            mode: QualityMode::Strict,
            sources: vec![],
            obligations: vec![],
            bindings: vec![],
            reviews: vec![],
        };
        let report = prepare_quality(&input);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "QUALITY.UNSAFE_HTML"),
            "attack must trigger QUALITY.UNSAFE_HTML at quality gate: {description}"
        );
        assert_eq!(report.verdict, "FAIL");
    }
}

#[test]
fn media_figure_direct_renderer_safety() {
    let dangerous_fig = MediaFigure {
        filename: "safe.png\" onerror=\"alert(1)".to_string(),
        caption: "Shows \"A\" & 'B' <tag>".to_string(),
        title: "Fig 1".to_string(),
        context_link: "".to_string(),
        item_type: "figure".to_string(),
        style: "width: 100px; \" onmouseover=\"alert(2)".to_string(),
        caption_style: "color: blue; \" onfocus=\"alert(3)".to_string(),
    };

    let rendered = render_media_figure(&dangerous_fig);
    // Attributes must be properly escaped, preventing tag breakouts
    assert!(!rendered.contains("onerror=\"alert(1)\""));
    assert!(!rendered.contains("onmouseover=\"alert(2)\""));
    assert!(!rendered.contains("onfocus=\"alert(3)\""));
    assert!(rendered.contains("&quot;"));
}

#[test]
fn canonical_schemas_derive_cleanly() {
    let table_schema = schema_for!(TableSpec);
    let table_json = serde_json::to_string(&table_schema).expect("schema serialize");
    assert!(table_json.contains("caption"));
    assert!(table_json.contains("headers"));
    assert!(table_json.contains("rows"));

    let figure_schema = schema_for!(MediaFigure);
    let figure_json = serde_json::to_string(&figure_schema).expect("schema serialize");
    assert!(figure_json.contains("filename"));
    assert!(figure_json.contains("caption"));
    assert!(figure_json.contains("item_type"));
    assert!(figure_json.contains("style"));
}

#[test]
fn vocab_and_csv_structure_accept_all_f2_elements() {
    for tag in [
        "section", "footer", "small", "cite", "dl", "dt", "dd", "details", "summary",
    ] {
        assert!(
            is_always_allowed_tag(tag),
            "tag '{tag}' must be in is_always_allowed_tag"
        );
        assert!(
            ALLOWED_TAGS.contains(&tag),
            "tag '{tag}' must be in ALLOWED_TAGS"
        );
    }

    for class in [
        "anki-note-section",
        "anki-kv",
        "anki-kv-section",
        "anki-callout",
        "anki-callout--trap",
        "anki-callout--pearl",
        "anki-callout--diff",
        "anki-list",
        "anki-steps",
        "anki-flow",
        "anki-detail",
        "anki-sources",
        "anki-table",
        "anki-figure",
        "anki-table-figure",
    ] {
        assert!(
            ALLOWED_CLASSES.contains(&class),
            "class '{class}' must be in ALLOWED_CLASSES"
        );
        assert!(
            unknown_classes(class).is_empty(),
            "class '{class}' must be recognized by unknown_classes"
        );
    }

    let csv_content = format!(
        "{QA_HEADER_CANONICAL}\nPergunta?|Resposta|<section class=\"anki-note-section\"><dl class=\"anki-kv\"><dt>A</dt><dd>B</dd></dl></section>|||Fonte|tag"
    );
    let diags = check_csv(&csv_content, NoteType::Qa, true);
    let tag_or_class_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == "FMT.TAG_FORBIDDEN" || d.code == "FMT.CLASS_UNKNOWN")
        .collect();
    assert!(
        tag_or_class_diags.is_empty(),
        "valid F2 semantic tags and classes must not trigger CSV structure warnings: {tag_or_class_diags:?}"
    );
}

#[test]
fn sanitize_field_preserves_f2_semantic_structure() {
    let input_html = "<section class=\"anki-note-section\"><p><b>Nota:</b> Conteúdo com <cite>Citação</cite></p><details class=\"anki-detail\"><summary>Resumo</summary><p>Detalhes</p></details><footer class=\"anki-sources\"><small>Fonte</small></footer></section>";
    let (sanitized, _) = sanitize_field(input_html, false, false);
    assert!(sanitized.contains("<section class=\"anki-note-section\">"));
    assert!(sanitized.contains("<cite>Citação</cite>"));
    assert!(sanitized.contains("<details class=\"anki-detail\">"));
    assert!(sanitized.contains("<footer class=\"anki-sources\">"));
    assert!(sanitized.contains("<small>Fonte</small>"));
}

/// B13: as formas alternativas de HTML SEGURO que o portão rejeitava por comparar bytes.
#[test]
fn alternative_safe_serializations_pass_gate_and_compiler() {
    let safe_cases = [
        // case de tag
        "<B>Negrito</B>",
        // case de tag e de atributos
        "<IMG SRC=\"fig1.png\" ALT=\"Foto\">",
        // comentário (o sanitizador remove de propósito)
        "<p>Texto</p><!-- comentário benigno -->",
        // espaço à direita em valor de classe
        "<span class=\"destaque \">Destaque</span>",
        // entidade em atributo (`&gt;` decodificado para `>`)
        "<span title=\"a &gt; b\">Comparação</span>",
        // conjunto combinado
        "<span class=\"destaque \" title=\"a &gt; b\" STYLE=\"color: red\">Combinado</span>",
    ];

    for case in safe_cases {
        let draft = json!({
            "tipo": "QA",
            "enunciado_ou_sentenca": "Pergunta de teste?",
            "resposta": "Resposta",
            "notas_adicionais": case
        });
        assert!(
            compile_drafts(&[draft], true, true, "").is_ok(),
            "compiler must accept benign serialization: {case}"
        );

        let input = QualityInput {
            qa_csv: format!("{QA_HEADER_CANONICAL}\nPergunta?|Resposta|{case}|||Ref|tag"),
            cloze_csv: CLOZE_HEADER_CANONICAL.to_owned(),
            mode: QualityMode::Strict,
            sources: vec![],
            obligations: vec![],
            bindings: vec![],
            reviews: vec![],
        };
        let report = prepare_quality(&input);
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|d| d.code == "QUALITY.UNSAFE_HTML"),
            "quality gate must accept benign serialization: {case}"
        );
    }
}

/// B14: profundidade de aninhamento acima do limite é rejeitada (32 ainda passa).
#[test]
fn nesting_depth_limit_is_enforced_at_gate() {
    for (depth, rejected) in [(32usize, false), (33usize, true), (512usize, true)] {
        let payload =
            format!("{}{}", "<div>".repeat(depth), "texto".to_owned()) + &"</div>".repeat(depth);
        let input = QualityInput {
            qa_csv: format!("{QA_HEADER_CANONICAL}\nPergunta?|Resposta|{payload}|||Ref|tag"),
            cloze_csv: CLOZE_HEADER_CANONICAL.to_owned(),
            mode: QualityMode::Strict,
            sources: vec![],
            obligations: vec![],
            bindings: vec![],
            reviews: vec![],
        };
        let report = prepare_quality(&input);
        let deep = report
            .diagnostics
            .iter()
            .find(|d| d.code == "HTML.NESTING_TOO_DEEP");
        let unsafe_flagged = report
            .diagnostics
            .iter()
            .any(|d| d.code == "QUALITY.UNSAFE_HTML");
        assert_eq!(
            deep.is_some(),
            rejected,
            "depth={depth} expected rejection={rejected}; diagnostics={:?}",
            report.diagnostics
        );
        // Profundidade nunca é reportada como conteúdo ativo: são causas diferentes.
        assert!(
            !unsafe_flagged,
            "depth={depth} must not be reported as active content: {:?}",
            report.diagnostics
        );
        if let Some(diag) = deep {
            // A mensagem cita o limite real (32) e a profundidade medida.
            assert!(
                diag.message.contains("32") && diag.message.contains(&depth.to_string()),
                "message must cite the real limit and the measured depth: {}",
                diag.message
            );
            assert_eq!(
                report.verdict, "FAIL",
                "depth={depth}: limit violation must stay fail-closed"
            );
        }
    }
}

/// B14: 20.000 `<div>` (220 KB) era ACEITO após 4,1 s (release) / 72,1 s (debug).
#[test]
fn deep_nesting_payloads_are_rejected_in_bounded_time() {
    use std::time::{Duration, Instant};

    let payload = format!("{}{}", "<div>".repeat(20_000), "</div>".repeat(20_000));
    assert!(payload.len() > 200_000);

    // Compilador: a decisão é O(n) e corta antes do parse (medido ~0,6 ms em debug).
    let draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Pergunta de teste?",
        "resposta": payload
    });
    let started = Instant::now();
    assert!(
        compile_drafts(&[draft], true, true, "").is_err(),
        "20k nested divs must be rejected by the compiler"
    );
    let compiler_elapsed = started.elapsed();
    assert!(
        compiler_elapsed < Duration::from_secs(2),
        "compiler took {compiler_elapsed:?} for a 20k-nesting payload"
    );

    // Portão de qualidade: rejeita. O custo residual (~2 s em debug, medido) é de checagens
    // não-HTML sobre uma linha de CSV de 220 KB; o parse foi cortado pelo limite. Folga
    // generosa para não flocar — voltar a parsear custava ~72 s.
    let input = QualityInput {
        qa_csv: format!("{QA_HEADER_CANONICAL}\nPergunta?|Resposta|{payload}|||Ref|tag"),
        cloze_csv: CLOZE_HEADER_CANONICAL.to_owned(),
        mode: QualityMode::Strict,
        sources: vec![],
        obligations: vec![],
        bindings: vec![],
        reviews: vec![],
    };
    let started = Instant::now();
    let report = prepare_quality(&input);
    let gate_elapsed = started.elapsed();
    // Rejeitado pelo código ESPECÍFICO: o payload também estoura os 16 KiB, mas o motivo
    // reportado é a profundidade (é ela que custa caro no parser).
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "HTML.NESTING_TOO_DEEP"),
        "20k nested divs must be rejected as too-deep by the quality gate: {:?}",
        report.diagnostics
    );
    assert!(
        gate_elapsed < Duration::from_secs(15),
        "gate took {gate_elapsed:?} for a 20k-nesting payload"
    );
}
