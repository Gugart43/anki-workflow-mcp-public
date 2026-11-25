//! Security boundaries for the Rust-first compiler and quality gate.
use anki_domain::{CLOZE_HEADER_CANONICAL, QA_HEADER_CANONICAL};
use anki_validate::{QualityInput, QualityMode, compile_drafts, prepare_quality};
use serde_json::json;

#[test]
fn compiler_rejects_active_html_attributes_in_any_card_field() {
    for field in [
        "enunciado_ou_sentenca",
        "resposta",
        "notas_adicionais",
        "campo_estudo",
        "referencias",
    ] {
        let mut draft =
            json!({"tipo":"QA", "enunciado_ou_sentenca":"Synthetic prompt?", "resposta":"Alpha"});
        draft[field] = json!("<b onclick=alert(1)>Synthetic content</b>");
        assert!(
            compile_drafts(&[draft], true, true, "").is_err(),
            "unsafe field accepted: {field}"
        );
    }
}

#[test]
fn compiler_rejects_injected_media_and_obfuscated_active_content() {
    for payload in [
        r#"<b title=">" onclick="alert(1)">Alpha</b>"#,
        r#"<b oNcLiCk = 'alert(1)'>Alpha</b>"#,
        r#"<span style="background:url(https://example.invalid/x)">Alpha</span>"#,
        r#"<span style="background:u&#114;l(x)">Alpha</span>"#,
    ] {
        let draft =
            json!({"tipo":"QA", "enunciado_ou_sentenca":"Synthetic prompt?", "resposta": payload});
        assert!(
            compile_drafts(&[draft], true, true, "").is_err(),
            "unsafe payload accepted: {payload}"
        );
    }
    for field in ["figura", "figuras_notas"] {
        let mut draft =
            json!({"tipo":"QA", "enunciado_ou_sentenca":"Synthetic prompt?", "resposta":"Alpha"});
        draft[field] = json!({"filename":"safe.png\" onerror=\"alert(1)", "caption":"Synthetic shape with a labeled boundary."});
        assert!(
            compile_drafts(&[draft], true, true, "").is_err(),
            "unsafe media accepted: {field}"
        );
    }
}

#[test]
fn compiler_preserves_safe_rich_notes_and_local_media() {
    let draft = json!({"tipo":"QA", "enunciado_ou_sentenca":"Synthetic prompt?", "resposta":"Alpha", "notas_adicionais":"<span class=\"destaque\" style=\"color: red\">Useful explanation.</span>", "figuras_notas":[{"filename":"demo_shape.png", "caption":"Synthetic shape with a labeled boundary."}]});
    let compiled = compile_drafts(&[draft], true, true, "").unwrap();
    let csv = compiled["qa_csv"].as_str().unwrap();
    assert!(csv.contains("Useful explanation."));
    assert!(csv.contains("<figure>"));
    assert!(csv.contains("demo_shape.png"));
}

#[test]
fn quality_gate_rejects_active_html_even_if_tag_name_is_allowed() {
    let input = QualityInput {
        qa_csv: format!(
            "{QA_HEADER_CANONICAL}\nSynthetic prompt?|<b onclick=alert(1)>Alpha</b>|Test|||Synthetic 2026|test"
        ),
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
            .any(|d| d.code == "QUALITY.UNSAFE_HTML")
    );
    assert_eq!(report.verdict, "FAIL");
}

/// B13: HTML seguro em forma alternativa deixa de ser rejeitado pelo compilador.
#[test]
fn compiler_accepts_benign_html_serialization_forms() {
    let safe_payloads = [
        "<B>Alpha</B>",
        "<IMG SRC=\"demo_shape.png\" ALT=\"Alternative illustration\">",
        "<p>Alpha</p><!-- benign comment -->",
        "<span class=\"destaque \">Alpha</span>",
        "<span title=\"a &gt; b\">Alpha</span>",
    ];

    for payload in safe_payloads {
        let draft = json!({
            "tipo": "QA",
            "enunciado_ou_sentenca": "Synthetic prompt?",
            "resposta": "Alpha",
            "notas_adicionais": payload
        });
        assert!(
            compile_drafts(&[draft], true, true, "").is_ok(),
            "benign serialization must compile: {payload}"
        );
    }
}

/// B14: aninhamento profundo é rejeitado antes do parse (DoS de 20.000 `<div>`).
#[test]
fn compiler_rejects_deep_nesting_without_expensive_parse() {
    let payload = format!("{}{}", "<div>".repeat(20_000), "</div>".repeat(20_000));
    let draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Synthetic prompt?",
        "resposta": payload
    });
    let started = std::time::Instant::now();
    assert!(
        compile_drafts(&[draft], true, true, "").is_err(),
        "20k nested divs must be rejected"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "deep nesting took {:?}",
        started.elapsed()
    );
}

/// Payload de 17.007 bytes, HTML legítimo e raso (só `<p>` e texto).
fn legit_field_of_size(bytes: usize) -> String {
    let filler = "a".repeat(bytes - "<p></p>".len());
    let payload = format!("<p>{filler}</p>");
    assert_eq!(payload.len(), bytes);
    payload
}

/// B14 / auditoria: campo apenas GRANDE não pode ser reportado como conteúdo ativo.
///
/// Medido antes do conserto: 17.007 bytes ⇒ `COMPILE.INVALID_DRAFT` com a mensagem
/// "Unsafe HTML attributes or active content in draft", o que era falso.
#[test]
fn oversized_legit_field_reports_field_too_large_instead_of_unsafe_html() {
    let payload = legit_field_of_size(17_007);
    assert!(payload.len() > 16 * 1024);

    // Compilador: rejeita (fail-closed) com o código e as medidas na mensagem.
    let draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Synthetic prompt?",
        "resposta": "Alpha",
        "notas_adicionais": payload
    });
    let err = compile_drafts(&[draft], true, true, "").expect_err("oversized field must fail");
    assert!(
        err.contains("HTML.FIELD_TOO_LARGE"),
        "compiler error must carry the specific code: {err}"
    );
    assert!(
        err.contains("17007") && err.contains("16384"),
        "compiler error must cite the measured size and the real limit: {err}"
    );
    assert!(
        !err.contains("Unsafe HTML attributes"),
        "oversized legit field must not be reported as active content: {err}"
    );

    // Portão de qualidade: mesmo diagnóstico específico, e nenhum QUALITY.UNSAFE_HTML.
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
    let too_large = report
        .diagnostics
        .iter()
        .find(|d| d.code == "HTML.FIELD_TOO_LARGE")
        .unwrap_or_else(|| panic!("expected HTML.FIELD_TOO_LARGE: {:?}", report.diagnostics));
    assert!(
        too_large.message.contains("17007") && too_large.message.contains("16384"),
        "gate message must cite the measured size and the real limit: {}",
        too_large.message
    );
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.code == "QUALITY.UNSAFE_HTML"),
        "oversized legit field must not be reported as active content: {:?}",
        report.diagnostics
    );
    assert_eq!(
        report.verdict, "FAIL",
        "limit violation must stay fail-closed"
    );
}

/// Controle: o caminho XSS continua com o diagnóstico de conteúdo ativo (nada regrediu).
#[test]
fn active_content_keeps_the_xss_diagnostic_after_the_limit_path() {
    let draft = json!({
        "tipo": "QA",
        "enunciado_ou_sentenca": "Synthetic prompt?",
        "resposta": "Alpha",
        "notas_adicionais": "<script>alert(1)</script>"
    });
    let err = compile_drafts(&[draft], true, true, "").expect_err("active content must fail");
    assert!(
        err.contains("Unsafe HTML attributes or active content"),
        "XSS path must keep its diagnosis: {err}"
    );
    assert!(
        !err.contains("HTML.FIELD_TOO_LARGE") && !err.contains("HTML.NESTING_TOO_DEEP"),
        "XSS must not be reported as a limit violation: {err}"
    );

    let input = QualityInput {
        qa_csv: format!(
            "{QA_HEADER_CANONICAL}\nPergunta?|Resposta|<script>alert(1)</script>|||Ref|tag"
        ),
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
        "XSS must keep QUALITY.UNSAFE_HTML: {:?}",
        report.diagnostics
    );
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.code == "HTML.FIELD_TOO_LARGE" || d.code == "HTML.NESTING_TOO_DEEP"),
        "XSS must not be reported as a limit violation: {:?}",
        report.diagnostics
    );
    assert_eq!(report.verdict, "FAIL");
}
