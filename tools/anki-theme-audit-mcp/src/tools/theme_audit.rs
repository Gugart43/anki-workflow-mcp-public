use rmcp::handler::server::wrapper::Parameters;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ThemeAuditCssParams {
    pub css: String,
    pub min_contrast: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ThemeAuditHtmlParams {
    pub html: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContrastCheck {
    pub theme: String,
    pub fg_token: String,
    pub bg_token: String,
    pub fg_color: String,
    pub bg_color: String,
    pub ratio: f64,
    pub threshold: f64,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThemeAuditReport {
    pub passed: bool,
    pub issues: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contrast_checks: Option<Vec<ContrastCheck>>,
}

/// Ponto de entrada assíncrono para a tool MCP `theme_audit_css`.
pub async fn audit_css(params: Parameters<ThemeAuditCssParams>) -> Result<String, String> {
    let report = audit_css_internal(&params.0.css, params.0.min_contrast);
    serde_json::to_string_pretty(&report).map_err(|e| e.to_string())
}

/// Ponto de entrada assíncrono para a tool MCP `theme_audit_card_html`.
pub async fn audit_html(params: Parameters<ThemeAuditHtmlParams>) -> Result<String, String> {
    let report = audit_html_internal(&params.0.html);
    serde_json::to_string_pretty(&report).map_err(|e| e.to_string())
}

/// Remove comentários CSS no formato `/* ... */`, substituindo-os por espaço para preservar espaçamento entre tokens.
pub fn strip_css_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let chars: Vec<char> = css.chars().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        if i + 1 < n && chars[i] == '/' && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < n && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            if i + 1 < n {
                i += 2; // consome '*/'
            }
            out.push(' ');
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }

    out
}

/// Remove comentários HTML no formato `<!-- ... -->`.
pub fn strip_html_comments(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let chars: Vec<char> = html.chars().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        if i + 3 < n
            && chars[i] == '<'
            && chars[i + 1] == '!'
            && chars[i + 2] == '-'
            && chars[i + 3] == '-'
        {
            i += 4;
            while i + 2 < n && !(chars[i] == '-' && chars[i + 1] == '-' && chars[i + 2] == '>') {
                i += 1;
            }
            if i + 2 < n {
                i += 3; // consome '-->'
            }
            out.push(' ');
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }

    out
}

#[derive(Debug, Clone)]
pub struct CssBlock {
    pub selector: String,
    pub body: String,
}

/// Analisador de blocos CSS considerando chaves aninhadas (media queries) e literais de string com escapes.
pub fn parse_css_blocks(css: &str) -> Vec<CssBlock> {
    let mut blocks = Vec::new();
    let mut current_selector = String::new();
    let mut current_body = String::new();
    let mut depth = 0usize;
    let mut in_string: Option<char> = None;
    let mut chars = css.chars().peekable();

    while let Some(ch) = chars.next() {
        if let Some(quote) = in_string {
            if ch == '\\' {
                if depth > 0 {
                    current_body.push(ch);
                    if let Some(escaped) = chars.next() {
                        current_body.push(escaped);
                    }
                } else {
                    current_selector.push(ch);
                    if let Some(escaped) = chars.next() {
                        current_selector.push(escaped);
                    }
                }
                continue;
            }
            if ch == quote {
                in_string = None;
            }
            if depth > 0 {
                current_body.push(ch);
            } else {
                current_selector.push(ch);
            }
            continue;
        }

        if ch == '"' || ch == '\'' {
            in_string = Some(ch);
            if depth > 0 {
                current_body.push(ch);
            } else {
                current_selector.push(ch);
            }
            continue;
        }

        match ch {
            '{' => {
                if depth == 0 {
                    depth = 1;
                } else {
                    depth += 1;
                    current_body.push(ch);
                }
            }
            '}' => {
                if depth == 1 {
                    depth = 0;
                    blocks.push(CssBlock {
                        selector: current_selector.trim().to_string(),
                        body: current_body.trim().to_string(),
                    });
                    current_selector.clear();
                    current_body.clear();
                } else if depth > 1 {
                    depth -= 1;
                    current_body.push(ch);
                }
            }
            _ => {
                if depth == 0 {
                    current_selector.push(ch);
                } else {
                    current_body.push(ch);
                }
            }
        }
    }

    blocks
}

/// Extrai custom properties CSS (`--token-name: value;`) do corpo de um bloco.
pub fn extract_tokens_from_body(body: &str) -> BTreeMap<String, String> {
    let mut tokens = BTreeMap::new();
    for line in body.lines() {
        let trimmed = line.trim();
        for part in trimmed.split(';') {
            let p = part.trim();
            if let Some((k, v)) = p.strip_prefix("--").and_then(|rest| rest.split_once(':')) {
                let key = format!("--{}", k.trim());
                let val = v.trim().to_string();
                if !val.is_empty() {
                    tokens.insert(key, val);
                }
            }
        }
    }
    tokens
}

/// Verifica se um corpo de regras CSS contém `!important` fora de literais de string.
pub fn body_contains_important(body: &str) -> bool {
    let mut in_string: Option<char> = None;
    let mut chars = body.chars().peekable();
    let mut decl = String::new();

    while let Some(ch) = chars.next() {
        if let Some(quote) = in_string {
            if ch == '\\' {
                let _ = chars.next();
                continue;
            }
            if ch == quote {
                in_string = None;
            }
            continue;
        }

        if ch == '"' || ch == '\'' {
            in_string = Some(ch);
            continue;
        }

        decl.push(ch);
    }

    decl.to_ascii_lowercase().contains("!important")
}

/// Converte um canal de cor sRGB de 0..255 para luminância linear conforme IEC 61966-2-1 / WCAG 2.1.
fn to_linear(val: u8) -> f64 {
    let c = (val as f64) / 255.0;
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Calcula a luminância relativa de uma cor sRGB conforme WCAG 2.1.
pub fn relative_luminance(r: u8, g: u8, b: u8) -> f64 {
    let r_lin = to_linear(r);
    let g_lin = to_linear(g);
    let b_lin = to_linear(b);
    0.2126 * r_lin + 0.7152 * g_lin + 0.0722 * b_lin
}

/// Calcula a razão de contraste entre duas cores no padrão WCAG 2.1 ((hi + 0.05) / (lo + 0.05)).
pub fn contrast_ratio(c1: (u8, u8, u8), c2: (u8, u8, u8)) -> f64 {
    let l1 = relative_luminance(c1.0, c1.1, c1.2);
    let l2 = relative_luminance(c2.0, c2.1, c2.2);
    let hi = l1.max(l2);
    let lo = l1.min(l2);
    let ratio = (hi + 0.05) / (lo + 0.05);
    (ratio * 100.0).round() / 100.0
}

/// Converte uma representação hexadecimal (#rgb, #rrggbb, #rrggbbaa) para tupla RGB (u8, u8, u8).
pub fn parse_hex_color(hex_str: &str) -> Option<(u8, u8, u8)> {
    let clean = hex_str.trim().trim_start_matches('#');
    // Byte slicing below requires ASCII; a multibyte hex string would panic.
    if !clean.is_ascii() {
        return None;
    }
    match clean.len() {
        3 => {
            let r_hex = &clean[0..1];
            let g_hex = &clean[1..2];
            let b_hex = &clean[2..3];
            let r = u8::from_str_radix(r_hex, 16).ok()? * 17;
            let g = u8::from_str_radix(g_hex, 16).ok()? * 17;
            let b = u8::from_str_radix(b_hex, 16).ok()? * 17;
            Some((r, g, b))
        }
        6 | 8 => {
            let r = u8::from_str_radix(&clean[0..2], 16).ok()?;
            let g = u8::from_str_radix(&clean[2..4], 16).ok()?;
            let b = u8::from_str_radix(&clean[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

/// Executa a auditoria completa da folha de estilo CSS.
pub fn audit_css_internal(css: &str, min_contrast: Option<f64>) -> ThemeAuditReport {
    let clean_css = strip_css_comments(css);
    let mut issues = Vec::new();

    // 1. Guarda AnkiDroid Force Dark no tema claro
    if !clean_css.contains("color-scheme: only light") {
        issues.push("WARN: Falta 'color-scheme: only light' no tema claro para bloquear Force Dark no AnkiDroid.".into());
    }

    // 2. Análise de blocos e regras CSS
    let top_blocks = parse_css_blocks(&clean_css);

    let mut light_tokens: BTreeMap<String, String> = BTreeMap::new();
    let mut dark_tokens: BTreeMap<String, String> = BTreeMap::new();
    let mut important_outside_print = false;

    // Coleta blocos candidatos genéricos caso seletores canônicos não sejam encontrados
    let mut generic_token_blocks = Vec::new();

    for block in &top_blocks {
        let sel_lower = block.selector.to_ascii_lowercase();

        if sel_lower.starts_with("@media") {
            let is_print = sel_lower.contains("print");
            let is_prefers_dark =
                sel_lower.contains("prefers-color-scheme") && sel_lower.contains("dark");

            let sub_blocks = parse_css_blocks(&block.body);

            // Verificação de !important dentro de media query não-print
            if !is_print && body_contains_important(&block.body) {
                important_outside_print = true;
            }

            if is_prefers_dark {
                // @media (prefers-color-scheme: dark) deve requerer [data-theme="auto"] e nunca sequestrar :root diretamente
                let has_auto_opt_in = sub_blocks
                    .iter()
                    .any(|b| b.selector.contains("[data-theme=\"auto\"]"));
                let targets_bare_root = sub_blocks.iter().any(|b| {
                    let s = b.selector.trim();
                    (s.contains(":root") || s.contains("body") || s.contains(".card"))
                        && !s.contains("[data-theme=\"auto\"]")
                });

                if !has_auto_opt_in || targets_bare_root {
                    issues.push("CRITICAL: @media (prefers-color-scheme: dark) sem escopo opt-in [data-theme=\"auto\"] sequestra o Tema Claro no Anki.".into());
                }
            }
        } else {
            // Regra comum fora de @media
            if body_contains_important(&block.body) {
                important_outside_print = true;
            }

            let tokens = extract_tokens_from_body(&block.body);
            if !tokens.is_empty() {
                generic_token_blocks.push((block.selector.clone(), tokens.clone()));

                let is_dark_sel = block.selector.contains(".nightMode")
                    || block.selector.contains(".night_mode")
                    || block.selector.contains("[data-theme=\"dark\"]");

                let is_light_sel = block.selector.contains(":root")
                    || block.selector.contains("[data-theme=\"light\"]")
                    || block.selector.contains(".card");

                if is_dark_sel {
                    dark_tokens.extend(tokens);
                } else if is_light_sel {
                    light_tokens.extend(tokens);
                }
            }
        }
    }

    if important_outside_print {
        issues.push("WARN: Declaração !important detectada fora de @media print.".into());
    }

    // Fallback: se os seletores convencionais não foram encontrados, utiliza o 1º e 2º blocos com tokens
    if light_tokens.is_empty() && dark_tokens.is_empty() && generic_token_blocks.len() >= 2 {
        light_tokens = generic_token_blocks[0].1.clone();
        dark_tokens = generic_token_blocks[1].1.clone();
    }

    // 3. Simetria de tokens Light vs Dark
    let light_keys: BTreeSet<&String> = light_tokens.keys().collect();
    let dark_keys: BTreeSet<&String> = dark_tokens.keys().collect();

    let missing_in_dark: Vec<_> = light_keys.difference(&dark_keys).cloned().collect();
    if !missing_in_dark.is_empty() {
        let mut missing_sorted: Vec<String> = missing_in_dark.into_iter().cloned().collect();
        missing_sorted.sort();
        issues.push(format!(
            "CRITICAL: Tokens ausentes no Tema Escuro: {}",
            missing_sorted.join(", ")
        ));
    }

    let missing_in_light: Vec<_> = dark_keys.difference(&light_keys).cloned().collect();
    if !missing_in_light.is_empty() {
        let mut missing_sorted: Vec<String> = missing_in_light.into_iter().cloned().collect();
        missing_sorted.sort();
        issues.push(format!(
            "WARN: Tokens presentes no Tema Escuro mas ausentes no Tema Claro: {}",
            missing_sorted.join(", ")
        ));
    }

    // 4. Verificação ativa de contraste WCAG 2.1 (usando min_contrast ou 4.5 como baseline AA)
    let threshold = min_contrast.unwrap_or(4.5);
    let mut contrast_checks = Vec::new();

    let mut check_pair = |theme: &str,
                          fg_token: &str,
                          bg_token: &str,
                          tokens: &BTreeMap<String, String>| {
        if let (Some(fg_val), Some(bg_val)) = (tokens.get(fg_token), tokens.get(bg_token))
            && let (Some(fg_color), Some(bg_color)) =
                (parse_hex_color(fg_val), parse_hex_color(bg_val))
        {
            let ratio = contrast_ratio(fg_color, bg_color);
            let passed = ratio >= threshold;
            contrast_checks.push(ContrastCheck {
                theme: theme.to_string(),
                fg_token: fg_token.to_string(),
                bg_token: bg_token.to_string(),
                fg_color: fg_val.clone(),
                bg_color: bg_val.clone(),
                ratio,
                threshold,
                passed,
            });
            if !passed {
                issues.push(format!(
                        "CRITICAL: Contraste insuficiente no {theme} entre {fg_token} ({fg_val}) e {bg_token} ({bg_val}): ratio {ratio:.2}:1 abaixo do mínimo requerido {threshold:.2}:1."
                    ));
            }
        }
    };

    for (theme_label, tokens) in [("Tema Claro", &light_tokens), ("Tema Escuro", &dark_tokens)] {
        // Pares correlacionados dinâmicos com terminação -fg e -bg
        let fg_keys: Vec<String> = tokens
            .keys()
            .filter(|k| k.ends_with("-fg"))
            .cloned()
            .collect();
        for fg_key in fg_keys {
            let base = &fg_key[..fg_key.len() - 3];
            let bg_key = format!("{base}-bg");
            if tokens.contains_key(&bg_key) {
                check_pair(theme_label, &fg_key, &bg_key, tokens);
            }
        }

        // Pares semânticos essenciais de texto e superfícies
        let core_pairs = [
            ("--ambr-ink", "--ambr-surface"),
            ("--ambr-ink-secondary", "--ambr-surface"),
            ("--ambr-ink-muted", "--ambr-surface"),
            ("--ambr-ink", "--ambr-bg-canvas"),
            ("--ambr-accent", "--ambr-surface"),
            ("--ambr-accent", "--ambr-accent-soft"),
        ];
        for (fg, bg) in core_pairs {
            check_pair(theme_label, fg, bg, tokens);
        }
    }

    ThemeAuditReport {
        passed: issues.is_empty(),
        issues,
        contrast_checks: if contrast_checks.is_empty() {
            None
        } else {
            Some(contrast_checks)
        },
    }
}

/// Executa a auditoria de HTML de cartões Anki contra marcações e estilos inline que quebram Dark/Light mode.
pub fn audit_html_internal(html: &str) -> ThemeAuditReport {
    let clean = strip_html_comments(html);
    let lower = clean.to_ascii_lowercase();
    let mut issues = Vec::new();

    // 1. Tags legadas descontinuadas: <font
    if let Some(pos) = lower.find("<font") {
        let after = &lower[pos + 5..];
        if after.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            issues.push(
                "CRITICAL: Marcação incompatível com Light/Dark Mode detectada: <font".into(),
            );
        }
    }

    // 2. Atributos legados de cor de fundo: bgcolor=
    if lower.contains("bgcolor=") || lower.contains("bgcolor =") {
        issues
            .push("CRITICAL: Marcação incompatível com Light/Dark Mode detectada: bgcolor=".into());
    }

    // 3. SVG inline cru sem estilização CSS flexível
    if let Some(pos) = lower.find("<svg") {
        let after = &lower[pos + 4..];
        if after.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            issues
                .push("CRITICAL: Marcação incompatível com Light/Dark Mode detectada: <svg".into());
        }
    }

    // 4. Estilos inline em atributos style="..." ou style='...'
    let mut search_from = 0;
    while let Some(style_idx) = lower[search_from..].find("style") {
        let actual_idx = search_from + style_idx;
        let rest = &lower[actual_idx + 5..];
        let rest_trimmed = rest.trim_start();
        if let Some(after_eq) = rest_trimmed.strip_prefix('=') {
            let after_eq = after_eq.trim_start();
            if let Some(quote @ ('"' | '\'')) = after_eq.chars().next() {
                let content_start = &after_eq[1..];
                if let Some(end_quote) = content_start.find(quote) {
                    let style_val = &content_start[..end_quote];
                    for decl in style_val.split(';') {
                        let decl_trim = decl.trim();
                        if let Some((prop, val)) = decl_trim.split_once(':') {
                            let p = prop.trim();
                            let v = val.trim();
                            if p == "color"
                                && !v.contains("var(--")
                                && v != "inherit"
                                && v != "currentcolor"
                            {
                                issues.push(format!("CRITICAL: Marcação incompatível com Light/Dark Mode detectada: style=\"color: {v}\""));
                            } else if p == "background-color"
                                && !v.contains("var(--")
                                && v != "inherit"
                                && v != "transparent"
                            {
                                issues.push(format!("CRITICAL: Marcação incompatível com Light/Dark Mode detectada: background-color: {v}"));
                            } else if p == "background"
                                && !v.contains("var(--")
                                && v != "none"
                                && v != "transparent"
                                && v != "inherit"
                            {
                                issues.push(format!("CRITICAL: Marcação incompatível com Light/Dark Mode detectada: background: {v}"));
                            }
                        }
                    }
                }
            }
        }
        search_from = actual_idx + 5;
    }

    ThemeAuditReport {
        passed: issues.is_empty(),
        issues,
        contrast_checks: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wcag_contrast_math_parity() {
        // V2 Dark Mode Badge corrigido (#0b1120 em #60a5fa) >= 7.0
        let dark_fg = parse_hex_color("#0b1120").expect("hex válido");
        let dark_bg = parse_hex_color("#60a5fa").expect("hex válido");
        let ratio_v2 = contrast_ratio(dark_fg, dark_bg);
        assert!(
            ratio_v2 >= 7.0,
            "Esperado ratio >= 7.0, obtido {}",
            ratio_v2
        );
        assert_eq!(ratio_v2, 7.41);

        // V1 Dark Mode Badge quebrado (#ffffff em #6bb4fa) < 4.5
        let v1_fg = parse_hex_color("#ffffff").expect("hex válido");
        let v1_bg = parse_hex_color("#6bb4fa").expect("hex válido");
        let ratio_v1 = contrast_ratio(v1_fg, v1_bg);
        assert!(ratio_v1 < 4.5, "Esperado ratio < 4.5, obtido {}", ratio_v1);
        assert_eq!(ratio_v1, 2.20);

        // Limites: preto e branco = 21.0
        let black = parse_hex_color("#000000").unwrap();
        let white = parse_hex_color("#ffffff").unwrap();
        assert_eq!(contrast_ratio(black, white), 21.0);

        // Cores idênticas = 1.0
        assert_eq!(contrast_ratio(black, black), 1.0);
    }

    #[test]
    fn test_comments_and_strings_not_confused_with_rules() {
        let css_with_comments = r#"
        /* 
           @media (prefers-color-scheme: dark) { :root { color: red; } }
           !important fora de print
           --token-fantasma: #123456;
        */
        :root, .card, [data-theme="light"] {
            color-scheme: only light;
            --ambr-ink: #0f172a;
            --ambr-surface: #ffffff;
        }

        .nightMode, [data-theme="dark"] {
            --ambr-ink: #e8eef5;
            --ambr-surface: #111827;
        }

        .tooltip::after {
            content: "Aviso: !important nao deve ser usado";
        }
        "#;

        let report = audit_css_internal(css_with_comments, Some(4.5));
        assert!(
            report.passed,
            "Comentários e strings não devem gerar falso positivo. Issues: {:?}",
            report.issues
        );
        assert!(report.issues.is_empty());
    }

    #[test]
    fn test_detects_unscoped_prefers_color_scheme() {
        let bad_css = r#"
        :root {
            color-scheme: only light;
            --ambr-ink: #000;
        }
        .nightMode {
            --ambr-ink: #fff;
        }
        @media (prefers-color-scheme: dark) {
            :root {
                --ambr-ink: #fff;
            }
        }
        "#;

        let report = audit_css_internal(bad_css, None);
        assert!(!report.passed);
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.contains("sequestra o Tema Claro no Anki"))
        );
    }

    #[test]
    fn test_detects_missing_color_scheme_only_light() {
        let bad_css = r#"
        :root {
            --ambr-ink: #000;
        }
        .nightMode {
            --ambr-ink: #fff;
        }
        "#;

        let report = audit_css_internal(bad_css, None);
        assert!(!report.passed);
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.contains("color-scheme: only light"))
        );
    }

    #[test]
    fn test_important_allowed_in_print_but_flagged_outside() {
        let css_print_ok = r#"
        :root {
            color-scheme: only light;
            --ambr-ink: #000;
        }
        .nightMode {
            --ambr-ink: #fff;
        }
        @media print {
            body {
                background: transparent !important;
            }
        }
        "#;

        let report_ok = audit_css_internal(css_print_ok, None);
        assert!(report_ok.issues.iter().all(|i| !i.contains("!important")));

        let css_important_bad = r#"
        :root {
            color-scheme: only light;
            --ambr-ink: #000;
        }
        .nightMode {
            --ambr-ink: #fff;
        }
        @media print {
            body { background: transparent !important; }
        }
        .card {
            color: red !important;
        }
        "#;

        let report_bad = audit_css_internal(css_important_bad, None);
        assert!(
            report_bad
                .issues
                .iter()
                .any(|i| i.contains("!important detectada fora de @media print"))
        );
    }

    #[test]
    fn test_detects_token_asymmetry() {
        let asymmetric_css = r#"
        :root {
            color-scheme: only light;
            --ambr-ink: #000;
            --ambr-surface: #fff;
            --token-so-no-claro: #123;
        }
        .nightMode {
            --ambr-ink: #fff;
            --ambr-surface: #000;
        }
        "#;

        let report = audit_css_internal(asymmetric_css, None);
        assert!(!report.passed);
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.contains("Tokens ausentes no Tema Escuro: --token-so-no-claro"))
        );
    }

    #[test]
    fn test_enforces_min_contrast_threshold() {
        let bad_contrast_css = r#"
        :root {
            color-scheme: only light;
            --anki-badge-fg: #ffffff;
            --anki-badge-bg: #6bb4fa;
        }
        .nightMode {
            --anki-badge-fg: #ffffff;
            --anki-badge-bg: #6bb4fa;
        }
        "#;

        // Com min_contrast 4.5, a razão 2.20 deve falhar
        let report_fail = audit_css_internal(bad_contrast_css, Some(4.5));
        assert!(!report_fail.passed);
        assert!(
            report_fail
                .issues
                .iter()
                .any(|i| i.contains("Contraste insuficiente"))
        );

        // Se o threshold requerido for absurdamente baixo (1.5), passaria
        let report_pass = audit_css_internal(bad_contrast_css, Some(1.5));
        assert!(
            report_pass
                .issues
                .iter()
                .all(|i| !i.contains("Contraste insuficiente"))
        );
    }

    #[test]
    fn test_audit_html_catches_incompatible_markup_and_ignores_comments() {
        let html_with_bad_markup = r##"
        <!-- Comentário inofensivo: <font color="red"> e bgcolor="#fff" -->
        <div class="card">
            <span style="font-weight: bold; color: #ff0000;">Texto com cor inline</span>
            <table bgcolor="#ffffff"><tr><td>Tabela legada</td></tr></table>
            <font size="3">Tag font descontinuada</font>
            <svg width="20" height="20"><circle cx="10" cy="10" r="5"/></svg>
        </div>
        "##;

        let report = audit_html_internal(html_with_bad_markup);
        assert!(!report.passed);
        assert!(report.issues.iter().any(|i| i.contains("<font")));
        assert!(report.issues.iter().any(|i| i.contains("bgcolor=")));
        assert!(report.issues.iter().any(|i| i.contains("<svg")));
        assert!(report.issues.iter().any(|i| i.contains("color: #ff0000")));

        let html_clean = r#"
        <!-- Comentário com palavras-chave: <font> bgcolor= -->
        <div class="card ambr-card">
            <div class="coluna enunciado">Pergunta</div>
            <div class="coluna resposta" style="margin-top: 10px; padding: 12px;">Resposta sem cor fixa</div>
        </div>
        "#;

        let report_clean = audit_html_internal(html_clean);
        assert!(
            report_clean.passed,
            "HTML limpo não deve ter falhas: {:?}",
            report_clean.issues
        );
    }
}
