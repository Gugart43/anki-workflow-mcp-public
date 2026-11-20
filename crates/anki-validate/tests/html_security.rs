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
