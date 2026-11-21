//! Suíte de Testes de Integração — Servidor MCP de Auditoria de Tema AnkiMBR
//! (`tools/anki-theme-audit-mcp/tests/integration_test.rs`)
//!
//! Substitui o teste vácuo pré-existente (`bad_css.contains(...)`) por testes
//! abrangentes e determinísticos das funções públicas do motor de auditoria:
//! 1. CSS Inválido: `@media (prefers-color-scheme: dark)` sem opt-in `[data-theme="auto"]`,
//!    ausência de `color-scheme: only light`, e `!important` fora de `@media print`.
//! 2. CSS v2 Válido: folha canônica `ankimbr-presentation-v2.css` e CSS sintético moderno.
//! 3. Tokens Ausentes: detecção precisa de assimetria onde tokens em `:root` faltam no `.nightMode`.
//! 4. Contraste WCAG 2.1: conformidade matemática da luminância relativa, ratio de contraste,
//!    verificação ativa de thresholds (AA 4.5:1 e AAA 7.0:1) e detecção de pares deficientes.
//! 5. HTML Incompatível: tags obsoletas (`<font`), atributos legados (`bgcolor=`),
//!    vetores inline crus (`<svg`) e estilos inline destrutivos (`color`, `background-color`).
//! 6. Casos Legítimos de Desigualdade e Comentários: descarte de falsos positivos em
//!    comentários CSS (`/* ... */`) e HTML (`<!-- ... -->`), tolerância a tokens adicionais
//!    exclusivos no Dark Mode, `!important` legítimo em `@media print`, e termos clínicos
//!    legítimos contendo substrings ("fonofobia", "fonte bibliográfica", texto "SVG").

#[path = "../src/tools/mod.rs"]
mod tools;

use rmcp::handler::server::wrapper::Parameters;
use tools::theme_audit::{
    ThemeAuditCssParams, ThemeAuditHtmlParams, ThemeAuditReport, audit_css, audit_css_internal,
    audit_html, audit_html_internal, contrast_ratio, parse_hex_color, relative_luminance,
    strip_css_comments, strip_html_comments,
};

/// Trait de extensão para prover `.inner()` caso a versão de `rmcp` do workspace não o exponha nativamente.
#[allow(dead_code)]
pub trait ParametersExt<T> {
    fn inner(&self) -> &T;
}

impl<T> ParametersExt<T> for Parameters<T> {
    fn inner(&self) -> &T {
        &self.0
    }
}

/// Construtor auxiliar para instanciar parâmetros de auditoria CSS com tipagem de extração MCP.
fn make_css_params(
    css: impl Into<String>,
    min_contrast: Option<f64>,
) -> Parameters<ThemeAuditCssParams> {
    Parameters(ThemeAuditCssParams {
        css: css.into(),
        min_contrast,
    })
}

/// Construtor auxiliar para instanciar parâmetros de auditoria HTML com tipagem de extração MCP.
fn make_html_params(html: impl Into<String>) -> Parameters<ThemeAuditHtmlParams> {
    Parameters(ThemeAuditHtmlParams { html: html.into() })
}

// =============================================================================
// GRUPO 1: TESTES DE CSS INVÁLIDO
// =============================================================================

#[test]
fn test_audit_css_flags_unscoped_prefers_color_scheme() {
    // Violação Crítica: @media (prefers-color-scheme: dark) sequestrando o :root diretamente
    // sem exigir o atributo de opt-in explícito do usuário [data-theme="auto"]
    let bad_css = r#"
        :root {
            color-scheme: only light;
            --ambr-ink: #0f172a;
            --ambr-surface: #ffffff;
        }
        .nightMode {
            --ambr-ink: #f8fafc;
            --ambr-surface: #0f172a;
        }
        @media (prefers-color-scheme: dark) {
            :root {
                --ambr-ink: #f8fafc;
                --ambr-surface: #0f172a;
            }
        }
    "#;

    let report = audit_css_internal(bad_css, None);
    assert!(
        !report.passed,
        "CSS com prefers-color-scheme solto deve ser reprovado."
    );
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.contains("sequestra o Tema Claro no Anki")),
        "Esperada mensagem crítica de sequestro de tema claro, obtido: {:?}",
        report.issues
    );
}

#[test]
fn test_audit_css_warns_missing_color_scheme_only_light() {
    // Alerta: Ausência de 'color-scheme: only light' permite que o Force Dark do AnkiDroid
    // inverta cores arbitrariamente em cartões com tema claro ativo
    let css_without_guard = r#"
        :root {
            --ambr-ink: #0f172a;
            --ambr-surface: #ffffff;
        }
        .nightMode {
            --ambr-ink: #f8fafc;
            --ambr-surface: #0f172a;
        }
    "#;

    let report = audit_css_internal(css_without_guard, None);
    assert!(
        !report.passed,
        "CSS sem guarda AnkiDroid deve conter aviso impeditivo."
    );
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.contains("color-scheme: only light") && i.contains("AnkiDroid")),
        "Esperado WARN de falta de 'color-scheme: only light', obtido: {:?}",
        report.issues
    );
}

#[test]
fn test_audit_css_warns_important_outside_media_print() {
    // Alerta: Declaração !important fora de bloco @media print quebra cascata e especificidade
    let css_with_important = r#"
        :root {
            color-scheme: only light;
            --ambr-ink: #0f172a !important;
            --ambr-surface: #ffffff;
        }
        .nightMode {
            --ambr-ink: #f8fafc;
            --ambr-surface: #0f172a;
        }
    "#;

    let report = audit_css_internal(css_with_important, None);
    assert!(
        !report.passed,
        "CSS com !important fora de print deve emitir advertência."
    );
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.contains("!important detectada fora de @media print")),
        "Esperado WARN de !important fora de print, obtido: {:?}",
        report.issues
    );
}

#[test]
fn test_audit_css_accumulates_multiple_violations() {
    let multiple_bad_css = r#"
        :root {
            --ambr-ink: #0f172a !important;
        }
        @media (prefers-color-scheme: dark) {
            body {
                --ambr-ink: #ffffff;
            }
        }
    "#;

    let report = audit_css_internal(multiple_bad_css, None);
    assert!(!report.passed);
    assert!(
        report.issues.len() >= 3,
        "Deve capturar sequestro prefers-color-scheme, falta de only-light e !important. Issues: {:?}",
        report.issues
    );
}

// =============================================================================
// GRUPO 2: TESTES DE CSS V2 VÁLIDO
// =============================================================================

#[test]
fn test_audit_css_v2_canonical_reference_passes() {
    // Validação integral da folha de estilo de referência ankimbr-presentation-v2.css
    let canonical_v2 = include_str!("../../../docs/reference-theme/ankimbr-presentation-v2.css");

    let report = audit_css_internal(canonical_v2, Some(4.5));
    assert!(
        report.passed,
        "A folha canônica ankimbr-presentation-v2.css deve passar com 100% de sucesso. Issues: {:?}",
        report.issues
    );
    assert!(
        report.issues.is_empty(),
        "Folha canônica não deve ter nenhuma issue."
    );

    // Garante que as checagens de contraste ativas foram executadas e registradas
    assert!(
        report.contrast_checks.is_some(),
        "Relatório da folha v2 deve conter auditoria detalhada de contraste."
    );
    let checks = report.contrast_checks.unwrap();
    assert!(
        !checks.is_empty(),
        "Devem existir pares de contraste verificados."
    );
    assert!(
        checks.iter().all(|c| c.passed),
        "Todos os pares auditados no CSS canônico devem cumprir o threshold WCAG."
    );
}

#[test]
fn test_audit_css_synthetic_valid_v2_passes() {
    // Folha sintética minimalista perfeitamente formatada conforme contrato v2
    let valid_css = r#"
        :root, .card, [data-theme="light"] {
            color-scheme: only light;
            --ambr-ink: #0f172a;
            --ambr-surface: #ffffff;
            --ambr-rule: #e2e8f0;
        }

        .nightMode, [data-theme="dark"] {
            --ambr-ink: #f8fafc;
            --ambr-surface: #0f172a;
            --ambr-rule: #334155;
        }

        @media (prefers-color-scheme: dark) {
            :root[data-theme="auto"] {
                --ambr-ink: #f8fafc;
                --ambr-surface: #0f172a;
                --ambr-rule: #334155;
            }
        }

        @media print {
            .card {
                background: #ffffff !important;
                color: #000000 !important;
                filter: none !important;
            }
        }
    "#;

    let report = audit_css_internal(valid_css, Some(4.5));
    assert!(
        report.passed,
        "CSS sintético válido deve passar sem falhas: {:?}",
        report.issues
    );
    assert!(report.issues.is_empty());
}

// =============================================================================
// GRUPO 3: TESTES DE TOKENS AUSENTES (ASSIMETRIA)
// =============================================================================

#[test]
fn test_audit_css_flags_missing_dark_tokens() {
    // Simula tokens declarados no Tema Claro (:root), porém esquecidos no Tema Escuro (.nightMode)
    let asymmetric_css = r#"
        :root {
            color-scheme: only light;
            --ambr-ink: #0f172a;
            --ambr-surface: #ffffff;
            --ambr-accent: #2563eb;
            --ambr-danger: #dc2626;
        }

        .nightMode {
            --ambr-ink: #f8fafc;
            --ambr-surface: #0f172a;
        }
    "#;

    let report = audit_css_internal(asymmetric_css, None);
    assert!(
        !report.passed,
        "Assimetria de tokens no tema escuro deve reprovar o CSS."
    );

    let missing_issue = report
        .issues
        .iter()
        .find(|i| i.contains("Tokens ausentes no Tema Escuro"))
        .expect("Deve conter aviso crítico de tokens ausentes no Tema Escuro");

    assert!(
        missing_issue.contains("--ambr-accent") && missing_issue.contains("--ambr-danger"),
        "Mensagem deve identificar nominalmente os tokens ausentes: {missing_issue}"
    );
}

// =============================================================================
// GRUPO 4: TESTES DE CONTRASTE WCAG 2.1
// =============================================================================

#[test]
fn test_wcag_contrast_mathematics_and_threshold_enforcement() {
    // 1. Parsing hexadecimal curto e longo
    assert_eq!(parse_hex_color("#000"), Some((0, 0, 0)));
    assert_eq!(parse_hex_color("#fff"), Some((255, 255, 255)));
    assert_eq!(parse_hex_color("#0f172a"), Some((15, 23, 42)));
    assert_eq!(parse_hex_color("#60a5fa"), Some((96, 165, 250)));
    assert_eq!(parse_hex_color("inválido"), None);

    // 2. Luminância relativa em extremos
    assert_eq!(relative_luminance(0, 0, 0), 0.0);
    assert_eq!(relative_luminance(255, 255, 255), 1.0);

    // 3. Extremos de razão de contraste
    assert_eq!(contrast_ratio((0, 0, 0), (255, 255, 255)), 21.0);
    assert_eq!(contrast_ratio((128, 128, 128), (128, 128, 128)), 1.0);

    // 4. Paridade matemática documentada na Wave 2 (Badge Dark Mode)
    // Par v2 corrigido (#0b1120 em #60a5fa): atinge WCAG AAA (7.41:1 >= 7.0)
    let dark_badge_fg = parse_hex_color("#0b1120").unwrap();
    let dark_badge_bg = parse_hex_color("#60a5fa").unwrap();
    let ratio_v2 = contrast_ratio(dark_badge_fg, dark_badge_bg);
    assert!(
        ratio_v2 >= 7.0,
        "Badge v2 deve atingir AAA, ratio: {ratio_v2}"
    );
    assert_eq!(ratio_v2, 7.41);

    // Par v1 quebrado (#ffffff em #6bb4fa): reprovado em AA (2.20:1 < 4.5)
    let broken_fg = parse_hex_color("#ffffff").unwrap();
    let broken_bg = parse_hex_color("#6bb4fa").unwrap();
    let ratio_v1 = contrast_ratio(broken_fg, broken_bg);
    assert!(
        ratio_v1 < 4.5,
        "Badge v1 deve falhar em AA, ratio: {ratio_v1}"
    );
    assert_eq!(ratio_v1, 2.20);
}

#[test]
fn test_audit_css_detects_insufficient_contrast_against_threshold() {
    // Folha com token de texto com baixo contraste proposital no Tema Escuro (#777777 em #111111)
    let low_contrast_css = r#"
        :root {
            color-scheme: only light;
            --ambr-ink: #000000;
            --ambr-surface: #ffffff;
        }
        .nightMode {
            --ambr-ink: #777777;
            --ambr-surface: #111111;
        }
    "#;

    let report_aa = audit_css_internal(low_contrast_css, Some(4.5));
    assert!(
        !report_aa.passed,
        "Contraste 3.28:1 deve falhar para baseline AA (4.5:1)"
    );
    assert!(
        report_aa
            .issues
            .iter()
            .any(|i| i.contains("Contraste insuficiente no Tema Escuro")),
        "Esperada issue de contraste insuficiente: {:?}",
        report_aa.issues
    );

    // Verificação estrita AAA (threshold 7.0:1)
    let report_aaa = audit_css_internal(low_contrast_css, Some(7.0));
    assert!(!report_aaa.passed);
    assert!(
        report_aaa.issues.iter().any(|i| i.contains("7.00:1")),
        "Esperada menção ao threshold estrito 7.00:1"
    );
}

// =============================================================================
// GRUPO 5: TESTES DE HTML INCOMPATÍVEL
// =============================================================================

#[test]
fn test_audit_html_detects_prohibited_elements_and_attributes() {
    // 1. Atributo bgcolor legado
    let html_bgcolor = r##"<table bgcolor="#ffffff"><tr><td>Dado clínico</td></tr></table>"##;
    let r1 = audit_html_internal(html_bgcolor);
    assert!(!r1.passed);
    assert!(r1.issues.iter().any(|i| i.contains("bgcolor=")));

    // 2. Tag <font obsoleta
    let html_font = r##"<p><font color="#ff0000">Alerta farmacológico</font></p>"##;
    let r2 = audit_html_internal(html_font);
    assert!(!r2.passed);
    assert!(r2.issues.iter().any(|i| i.contains("<font")));

    // 3. SVG inline desprotegido
    let html_svg = r#"<svg width="40" height="40"><path d="M0 0 L10 10"/></svg>"#;
    let r3 = audit_html_internal(html_svg);
    assert!(!r3.passed);
    assert!(r3.issues.iter().any(|i| i.contains("<svg")));

    // 4. Estilos inline fixos em aspas duplas (color)
    let html_color_double = r#"<span style="color: #111827;">Texto fixo escuro</span>"#;
    let r4 = audit_html_internal(html_color_double);
    assert!(!r4.passed);
    assert!(
        r4.issues
            .iter()
            .any(|i| i.contains("style=\"color: #111827\""))
    );

    // 5. Estilos inline fixos em aspas simples (color)
    let html_color_single = r#"<span style='color: #ffffff;'>Texto fixo claro</span>"#;
    let r5 = audit_html_internal(html_color_single);
    assert!(!r5.passed);
    assert!(
        r5.issues
            .iter()
            .any(|i| i.contains("style=\"color: #ffffff\""))
    );

    // 6. Estilos inline de background-color fixo
    let html_bg = r#"<div style="background-color: #f1f5f9; padding: 8px;">Bloco</div>"#;
    let r6 = audit_html_internal(html_bg);
    assert!(!r6.passed);
    assert!(
        r6.issues
            .iter()
            .any(|i| i.contains("background-color: #f1f5f9"))
    );
}

#[test]
fn test_audit_html_valid_compliant_card_markup_passes() {
    // HTML semântico com classes AnkiMBR e referências corretas a variáveis ou neutras
    let clean_card_html = r#"
        <div class="card ankimbr-qa">
            <header class="header">
                <span class="badge">Neurologia</span>
                <h1 class="title">Cefaleia em Salvas</h1>
            </header>
            <div class="content">
                <p>Quadro unilateral, periorbitário, associado a sintomas autonômicos parassimpáticos.</p>
                <figure class="anki-figure--preserve">
                    <img src="paste-12345.jpg" alt="Distribuição trigeminal"/>
                    <figcaption class="anki-figcaption">Figura 1: Território oftálmico (V1) acometido.</figcaption>
                </figure>
            </div>
            <footer class="footer" style="margin-top: 1rem; padding: 0.5rem;">
                <p class="reference">Referência: Diretriz SBC / ABN.</p>
            </footer>
        </div>
    "#;

    let report = audit_html_internal(clean_card_html);
    assert!(
        report.passed,
        "HTML em conformidade com o tema v2 deve passar sem avisos: {:?}",
        report.issues
    );
    assert!(report.issues.is_empty());
}

// =============================================================================
// GRUPO 6: CASOS LEGÍTIMOS DE DESIGUALDADE, COMENTÁRIOS E VOCABULÁRIO
// =============================================================================

#[test]
fn test_css_comments_do_not_trigger_false_positives() {
    // Comentários que mencionam regras proibidas ou tokens inexistentes não devem interferir
    let css_commented = r#"
        /* 
           Comentário de documentação técnica:
           @media (prefers-color-scheme: dark) { :root { color: red !important; } }
           --token-fantasma: #123456;
           bgcolor: #ffffff;
        */
        :root {
            color-scheme: only light;
            --ambr-ink: #0f172a;
            --ambr-surface: #ffffff;
        }

        .nightMode {
            --ambr-ink: #f8fafc;
            --ambr-surface: #0f172a;
        }

        .tooltip::after {
            content: "Nota: !important nao deve ser usado em producao";
        }
    "#;

    // Remove comentários antes da verificação direta
    let stripped = strip_css_comments(css_commented);
    assert!(!stripped.contains("--token-fantasma"));

    let report = audit_css_internal(css_commented, Some(4.5));
    assert!(
        report.passed,
        "Comentários não devem ser interpretados como regras CSS ativas. Issues: {:?}",
        report.issues
    );
    assert!(report.issues.is_empty());
}

#[test]
fn test_html_comments_do_not_trigger_false_positives() {
    // Comentários HTML que contêm tags e atributos obsoletos comentados não devem reprovar o cartão
    let html_commented = r##"
        <!-- Trecho legado removido: <font color="red">aviso</font> <svg></svg> bgcolor="#fff" -->
        <div class="card">
            <p>Texto principal limpo e sem formatação inline estática.</p>
        </div>
    "##;

    let stripped = strip_html_comments(html_commented);
    assert!(!stripped.contains("<font"));
    assert!(!stripped.contains("bgcolor"));

    let report = audit_html_internal(html_commented);
    assert!(
        report.passed,
        "Comentários HTML não devem disparar violações. Issues: {:?}",
        report.issues
    );
    assert!(report.issues.is_empty());
}

#[test]
fn test_dark_mode_additional_tokens_is_legitimate_asymmetry() {
    // Caso legítimo: Tema escuro declara tokens suplementares específicos (ex: elevações de sombra,
    // brilho ou bordas reforçadas) que não existem no tema claro.
    // Isso NÃO é um erro crítico de tokens ausentes no Dark Mode.
    let dark_extra_tokens_css = r#"
        :root {
            color-scheme: only light;
            --ambr-ink: #0f172a;
            --ambr-surface: #ffffff;
        }

        .nightMode {
            --ambr-ink: #f8fafc;
            --ambr-surface: #0f172a;
            --ambr-dark-elevation: rgba(0, 0, 0, 0.6);
            --ambr-dark-glow: 0 0 12px rgba(96, 165, 250, 0.25);
        }
    "#;

    let report = audit_css_internal(dark_extra_tokens_css, None);

    // Não deve conter a violação crítica de token ausente no dark mode
    assert!(
        report
            .issues
            .iter()
            .all(|i| !i.contains("CRITICAL: Tokens ausentes no Tema Escuro")),
        "Tokens suplementares no Dark Mode não são tokens faltantes: {:?}",
        report.issues
    );

    // O relatório pode opcionalmente emitir WARN informativo sobre tokens extras no Dark Mode
    let has_dark_extra_warn = report
        .issues
        .iter()
        .any(|i| i.contains("Tokens presentes no Tema Escuro mas ausentes no Tema Claro"));
    assert!(
        has_dark_extra_warn,
        "Deve emitir advertência informativa controlada."
    );
}

#[test]
fn test_important_in_media_print_is_legitimate() {
    // A especificação AnkiMBR permite explicitamente declarações !important
    // dentro de blocos @media print para forçar neutralização de fundos e impressões limpas
    let css_print_important = r#"
        :root {
            color-scheme: only light;
            --ambr-ink: #0f172a;
            --ambr-surface: #ffffff;
        }

        .nightMode {
            --ambr-ink: #f8fafc;
            --ambr-surface: #0f172a;
        }

        @media print {
            .card {
                background: #ffffff !important;
                color: #000000 !important;
                box-shadow: none !important;
            }
            .anki-figure img {
                filter: none !important;
            }
        }
    "#;

    let report = audit_css_internal(css_print_important, None);
    assert!(
        report
            .issues
            .iter()
            .all(|i| !i.contains("!important detectada fora de @media print")),
        "!important restrito a @media print é permitido e não deve gerar WARN: {:?}",
        report.issues
    );
}

#[test]
fn test_clinical_vocabulary_substrings_do_not_trigger_false_positives() {
    // Termos médicos legítimos que contêm substrings "font" ou "svg" em texto plano
    // não devem ser confundidos com tags HTML (<font ou <svg)
    let clinical_html = r#"
        <div class="field">
            <p>Fonte de transmissão principal: gotículas respiratórias.</p>
            <p>Paciente relata queixa de fonofobia e fonofobia associadas a fotoalergia.</p>
            <p>O gráfico SVG gerado no laboratório clínico foi exportado como PNG rasterizado.</p>
            <p>Conduta: hidratação com soro fisiológico e analgesia parenteral.</p>
        </div>
    "#;

    let report = audit_html_internal(clinical_html);
    assert!(
        report.passed,
        "Vocabulário clínico com substrings parciais não deve falhar: {:?}",
        report.issues
    );
    assert!(report.issues.is_empty());
}

// =============================================================================
// GRUPO 7: TESTES ASSÍNCRONOS DOS PONTOS DE ENTRADA MCP (audit_css e audit_html)
// =============================================================================

#[tokio::test]
async fn test_async_mcp_entrypoints_audit_css_and_audit_html() {
    // 1. Tool MCP theme_audit_css assíncrona com entrada JSON/Parameters
    let valid_css = r#"
        :root {
            color-scheme: only light;
            --ambr-ink: #0f172a;
            --ambr-surface: #ffffff;
        }
        .nightMode {
            --ambr-ink: #f8fafc;
            --ambr-surface: #0f172a;
        }
    "#;

    let css_params = make_css_params(valid_css, Some(4.5));
    let css_res = audit_css(css_params)
        .await
        .expect("audit_css deve retornar Ok(JSON)");
    let css_report: ThemeAuditReport =
        serde_json::from_str(&css_res).expect("JSON de relatório válido");
    assert!(
        css_report.passed,
        "CSS válido deve ser aprovado pelo endpoint assíncrono"
    );
    assert!(css_report.issues.is_empty());

    // 2. Tool MCP theme_audit_card_html assíncrona com entrada inválida
    let bad_html = r#"<div class="card"><font color="blue">Texto azul</font></div>"#;
    let html_params = make_html_params(bad_html);
    let html_res = audit_html(html_params)
        .await
        .expect("audit_html deve retornar Ok(JSON)");
    let html_report: ThemeAuditReport =
        serde_json::from_str(&html_res).expect("JSON de relatório válido");
    assert!(
        !html_report.passed,
        "HTML com <font> deve ser reprovado pelo endpoint assíncrono"
    );
    assert!(html_report.issues.iter().any(|i| i.contains("<font")));
}
