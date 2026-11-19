//! Utilitários de extração, normalização e contagem de texto visível a partir de HTML.
//!
//! Preserva paridade com as regras de validação de `anki-validate`:
//! - descarte de blocos `<script>` e `<style>`;
//! - inserção de quebras de linha em tags de bloco (estendido para todas as tags HTML5 da F2);
//! - contagem de itens de lista (`<li>`) e quebras explícitas (`<br>` / `<br/>`);
//! - decodificação de entidades HTML nomeadas e numéricas (decimais e hexadecimais);
//! - normalização para comparação exata e remoção de blocos `<table>` (inclusive aninhados).

use std::sync::LazyLock;

use regex::Regex;

const MAX_ENTITY_SCAN_LEN: usize = 16;

const BLOCK_TAGS: &[&str] = &[
    "p",
    "div",
    "li",
    "br",
    "hr",
    "tr",
    "dt",
    "dd",
    "dl",
    "blockquote",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "section",
    "article",
    "header",
    "footer",
    "aside",
    "nav",
    "main",
    "figure",
    "figcaption",
    "details",
    "summary",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "ul",
    "ol",
];

static WHITESPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("valid WHITESPACE_RE"));

static LEADING_BULLET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[•▪▸►\-–—*\s]+").expect("valid LEADING_BULLET_RE"));

static TRAILING_PUNCT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[.!?…:;\s]+$").expect("valid TRAILING_PUNCT_RE"));

/// Extrai o texto visível com quebras de bloco de um fragmento HTML, além das contagens
/// de elementos `<li>` e `<br>`.
#[must_use]
pub fn parse_html_text(raw: &str) -> (String, usize, usize) {
    if raw.is_empty() {
        return (String::new(), 0, 0);
    }

    let bytes = raw.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut li_count = 0usize;
    let mut br_count = 0usize;
    let mut skip_tag: Option<&'static str> = None;
    let mut i = 0usize;

    while i < len {
        if let Some(target) = skip_tag {
            if bytes[i] == b'<' {
                if bytes.get(i + 1) == Some(&b'/')
                    && let Some(close_pos) = find_tag_end(bytes, i + 2)
                {
                    let closing_name = extract_tag_name(&raw[i + 2..close_pos]);
                    if closing_name.eq_ignore_ascii_case(target) {
                        skip_tag = None;
                        i = close_pos + 1;
                        continue;
                    }
                }
                i += 1;
            } else {
                match bytes[i..].iter().position(|&b| b == b'<') {
                    Some(offset) => i += offset,
                    None => break,
                }
            }
            continue;
        }

        match bytes[i] {
            b'<' => {
                if raw[i..].starts_with("<!--") {
                    if let Some(end_offset) = raw[i + 4..].find("-->") {
                        i = i + 4 + end_offset + 3;
                    } else {
                        break;
                    }
                    continue;
                }

                let is_tag_start = bytes
                    .get(i + 1)
                    .is_some_and(|&b| b.is_ascii_alphabetic() || matches!(b, b'/' | b'!' | b'?'));

                if !is_tag_start {
                    out.push('<');
                    i += 1;
                    continue;
                }

                let Some(close_pos) = find_tag_end(bytes, i + 1) else {
                    out.push('<');
                    i += 1;
                    continue;
                };

                let tag_inner = raw[i + 1..close_pos].trim();
                let is_closing = tag_inner.starts_with('/');
                let is_self_closing = !is_closing && tag_inner.ends_with('/');
                let name_slice = tag_inner.strip_prefix('/').unwrap_or(tag_inner);
                let tag_name = extract_tag_name(name_slice);

                if !is_closing && !is_self_closing {
                    if tag_name.eq_ignore_ascii_case("script") {
                        skip_tag = Some("script");
                        i = close_pos + 1;
                        continue;
                    }
                    if tag_name.eq_ignore_ascii_case("style") {
                        skip_tag = Some("style");
                        i = close_pos + 1;
                        continue;
                    }
                }

                if !is_closing {
                    if tag_name.eq_ignore_ascii_case("li") {
                        li_count += 1;
                    } else if tag_name.eq_ignore_ascii_case("br") {
                        br_count += 1;
                    } else if tag_name.eq_ignore_ascii_case("td")
                        || tag_name.eq_ignore_ascii_case("th")
                    {
                        out.push(' ');
                    }
                }

                if is_block_tag(tag_name) {
                    out.push('\n');
                }

                i = close_pos + 1;
            }
            b'&' => {
                if let Some((decoded, consumed)) = try_decode_entity(&raw[i..]) {
                    out.push(if decoded == '\u{00a0}' { ' ' } else { decoded });
                    i += consumed;
                } else {
                    out.push('&');
                    i += 1;
                }
            }
            _ => {
                let start = i;
                while i < len && bytes[i] != b'<' && bytes[i] != b'&' {
                    i += 1;
                }
                out.push_str(&raw[start..i]);
            }
        }
    }

    (out, li_count, br_count)
}

/// Converte HTML em texto visível linearizado, colapsando `\s+` em espaço único.
#[must_use]
pub fn html_to_visible_text(raw: &str) -> String {
    let (text, _, _) = parse_html_text(raw);
    WHITESPACE_RE.replace_all(&text, " ").trim().to_owned()
}

/// Conta o número de caracteres Unicode (`char`) no texto visível extraído de `raw`.
#[must_use]
pub fn visible_char_count(raw: &str) -> usize {
    html_to_visible_text(raw).chars().count()
}

/// Normaliza texto HTML para comparação exata (minúsculas, sem bullets iniciais nem pontuação final).
#[must_use]
pub fn normalize_for_exact_match(raw: &str) -> String {
    let lowered = html_to_visible_text(raw).to_lowercase();
    let without_leading = LEADING_BULLET_RE.replace(&lowered, "");
    let without_trailing = TRAILING_PUNCT_RE.replace(&without_leading, "");
    WHITESPACE_RE
        .replace_all(&without_trailing, " ")
        .trim()
        .to_owned()
}

/// Remove blocos `<table>...</table>` de um fragmento HTML, suportando aninhamento correto.
#[must_use]
pub fn strip_table_blocks(raw: &str) -> String {
    if !raw
        .as_bytes()
        .windows(6)
        .any(|w| w.eq_ignore_ascii_case(b"<table"))
    {
        return raw.to_owned();
    }

    let bytes = raw.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0usize;

    while i < len {
        if bytes[i] == b'<' {
            let rest = &raw[i..];
            if rest.len() >= 6
                && rest[1..].starts_with(['t', 'T'])
                && let Some(close_pos) = find_tag_end(bytes, i + 1)
            {
                let tag_name = extract_tag_name(&raw[i + 1..close_pos]);
                if tag_name.eq_ignore_ascii_case("table")
                    && !raw[i + 1..].trim_start().starts_with('/')
                    && let Some(table_end) = find_balanced_tag_end(raw, i, "table")
                {
                    if !out.is_empty()
                        && !out.ends_with(char::is_whitespace)
                        && !raw[table_end..].trim_start().is_empty()
                    {
                        out.push(' ');
                    }
                    i = table_end;
                    continue;
                }
            }
        }
        let ch = raw[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

fn find_tag_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    let mut idx = start;

    while idx < bytes.len() {
        let b = bytes[idx];
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'>' => return Some(idx),
                _ => {}
            },
        }
        idx += 1;
    }

    None
}

fn extract_tag_name(s: &str) -> &str {
    let trimmed = s.trim_start();
    let end = trimmed
        .bytes()
        .position(|b| !b.is_ascii_alphanumeric())
        .unwrap_or(trimmed.len());
    &trimmed[..end]
}

fn is_block_tag(tag_name: &str) -> bool {
    BLOCK_TAGS
        .iter()
        .any(|&block| block.eq_ignore_ascii_case(tag_name))
}

fn find_balanced_tag_end(raw: &str, start_pos: usize, target_tag: &str) -> Option<usize> {
    let bytes = raw.as_bytes();
    let len = bytes.len();
    let mut depth = 0usize;
    let mut i = start_pos;

    while i < len {
        if bytes[i] == b'<'
            && let Some(close_pos) = find_tag_end(bytes, i + 1)
        {
            let tag_inner = raw[i + 1..close_pos].trim();
            let is_closing = tag_inner.starts_with('/');
            let name_slice = tag_inner.strip_prefix('/').unwrap_or(tag_inner);
            let tag_name = extract_tag_name(name_slice);

            if tag_name.eq_ignore_ascii_case(target_tag) {
                if is_closing {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(close_pos + 1);
                    }
                } else if depth == 0 && tag_inner.ends_with('/') {
                    return Some(close_pos + 1);
                } else if !tag_inner.ends_with('/') {
                    depth += 1;
                }
            }
            i = close_pos + 1;
            continue;
        }
        i += 1;
    }

    None
}

fn try_decode_entity(s: &str) -> Option<(char, usize)> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'&') {
        return None;
    }

    let limit = bytes.len().min(MAX_ENTITY_SCAN_LEN);
    let mut semi_pos = None;

    for (idx, &b) in bytes.iter().enumerate().take(limit).skip(1) {
        if b == b';' {
            semi_pos = Some(idx);
            break;
        }
        if b == b'&' || b == b'<' || b.is_ascii_whitespace() {
            return None;
        }
    }

    let semi_pos = semi_pos?;
    let entity = &s[1..semi_pos];
    if entity.is_empty() {
        return None;
    }

    let decoded = if let Some(num) = entity.strip_prefix('#') {
        if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
            if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)?
        } else {
            if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            num.parse::<u32>().ok().and_then(char::from_u32)?
        }
    } else {
        match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "nbsp" => ' ',
            "ndash" => '\u{2013}',
            "mdash" => '\u{2014}',
            "bull" => '•',
            "rarr" => '\u{2192}',
            "larr" => '\u{2190}',
            "le" => '≤',
            "ge" => '≥',
            "plusmn" => '±',
            "times" => '×',
            "divide" => '÷',
            "deg" => '°',
            "mu" => '\u{03BC}',
            "micro" => '\u{00B5}',
            "hellip" => '…',
            "permil" => '‰',
            _ => return None,
        }
    };

    Some((decoded, semi_pos + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ignores_script_and_style_contents() {
        let raw = "<style>.card{color:red}</style><p>Visível</p><SCRIPT>var x='<li><br>';</SCRIPT><div>Final</div>";
        let (_, li_count, br_count) = parse_html_text(raw);
        assert_eq!(li_count, 0);
        assert_eq!(br_count, 0);
        assert_eq!(html_to_visible_text(raw), "Visível Final");
    }

    #[test]
    fn parse_counts_li_and_br_accurately() {
        let raw = "<ul><li class=\"a\">Um<br>linha</li><LI>Dois<br/>fim<BR /></LI></ul>";
        let (_, li_count, br_count) = parse_html_text(raw);
        assert_eq!(li_count, 2);
        assert_eq!(br_count, 3);
        assert_eq!(html_to_visible_text(raw), "Um linha Dois fim");
    }

    #[test]
    fn parse_decodes_named_and_numeric_entities() {
        let raw = "&amp;&lt;&gt;&quot;&apos;&nbsp;&ndash;&mdash;&rarr;&larr;&le;&ge;&plusmn;&times;&divide;&deg;&mu;&#124;&#x7C;";
        assert_eq!(parse_html_text(raw).0, "&<>\"' –—→←≤≥±×÷°μ||");
    }

    #[test]
    fn normalize_for_exact_match_strips_bullets_punctuation_and_case() {
        assert_eq!(
            normalize_for_exact_match("<li> • — *  Hipertensão   Arterial?!…  </li>"),
            "hipertensão arterial"
        );
    }

    #[test]
    fn strip_table_blocks_removes_multiline_tables() {
        let raw = "<p>Antes</p><TABLE>\n<tr><td>X</td></tr>\n</TABLE><p>Depois</p>";
        assert_eq!(
            html_to_visible_text(&strip_table_blocks(raw)),
            "Antes Depois"
        );
    }

    #[test]
    fn strip_table_blocks_handles_nested_tables() {
        let raw = "<div>Fora<table><tr><td>Interno<table><tr><td>Sub</td></tr></table>Mais</td></tr></table>Depois</div>";
        assert_eq!(
            html_to_visible_text(&strip_table_blocks(raw)),
            "Fora Depois"
        );
    }

    #[test]
    fn parse_handles_f2_block_tags() {
        let raw = "<section><header><h1>Título</h1></header><details><summary>Resumo</summary><p>Conteúdo</p></details><dl><dt>Termo</dt><dd>Definição</dd></dl><footer>Fim</footer></section>";
        let vis = html_to_visible_text(raw);
        assert_eq!(vis, "Título Resumo Conteúdo Termo Definição Fim");
    }
}
