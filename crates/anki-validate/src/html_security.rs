//! Fail-closed HTML5 active-content boundary for Rust-first outputs.
//! Uses `ammonia` (`html5ever`) with a strict AnkiMBR + F2 semantic allowlist.

use ammonia::{Builder, UrlRelative};
use std::borrow::Cow;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

const ALLOWED_TAGS: &[&str] = &[
    "abbr",
    "b",
    "blockquote",
    "br",
    "caption",
    "cite",
    "code",
    "col",
    "colgroup",
    "dd",
    "del",
    "details",
    "div",
    "dl",
    "dt",
    "em",
    "figcaption",
    "figure",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "img",
    "ins",
    "kbd",
    "li",
    "mark",
    "ol",
    "p",
    "pre",
    "s",
    "section",
    "small",
    "span",
    "strong",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "u",
    "ul",
    "wbr",
];

const CLEAN_CONTENT_TAGS: &[&str] = &[
    "applet", "audio", "base", "button", "canvas", "embed", "form", "iframe", "input", "link",
    "math", "meta", "noscript", "object", "script", "select", "style", "svg", "template",
    "textarea", "title", "video", "xmp",
];

thread_local! {
    static ATTR_REJECTED: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn safe_style(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    ![
        "url",
        "expression",
        "import",
        "behavior",
        "binding",
        "javascript",
        "vbscript",
    ]
    .iter()
    .any(|s| lower.contains(s))
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || ":;#%.,()-".contains(c)
        })
}

pub(crate) fn safe_img_src(value: &str) -> bool {
    !value.is_empty()
        && !value.contains("..")
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-.".contains(&c))
}

static SANITIZER: LazyLock<Builder<'static>> = LazyLock::new(|| {
    let mut builder = Builder::default();

    builder.tags(ALLOWED_TAGS.iter().copied().collect());
    builder.clean_content_tags(CLEAN_CONTENT_TAGS.iter().copied().collect());
    builder.generic_attributes(HashSet::from(["title", "style"]));
    builder.tag_attributes(HashMap::from([
        ("img", HashSet::from(["src", "alt"])),
        ("th", HashSet::from(["colspan", "rowspan", "scope"])),
        ("td", HashSet::from(["colspan", "rowspan", "scope"])),
    ]));
    builder.allowed_classes(HashMap::from([
        (
            "figure",
            HashSet::from(["anki-figure", "anki-table-figure", "media"]),
        ),
        (
            "span",
            HashSet::from([
                "destaque",
                "cloze",
                "cloze-inactive",
                "ambr-badge",
                "ambr-pill",
                "ambr-tag",
                "ambr-muted",
                "ambr-accent",
                "alerta",
                "crit",
                "crit-i",
                "dose",
                "faixa",
                "fig-context",
                "fig-title",
                "hints",
                "lab",
                "nl",
                "nl-i",
                "nl-sub",
                "pearl",
                "rot",
                "sigla",
                "style-notas",
                "style-ref",
                "tbl",
                "termo",
                "uni",
            ]),
        ),
        (
            "details",
            HashSet::from([
                "hint-details",
                "anki-details",
                "ambr-details",
                "anki-detail",
            ]),
        ),
        ("summary", HashSet::from(["hint-summary", "ambr-summary"])),
        (
            "div",
            HashSet::from([
                "ambr-card",
                "ambr-section",
                "ambr-callout",
                "ambr-note",
                "ambr-table-wrap",
                "anki-note",
                "anki-callout",
                "alerta",
            ]),
        ),
        ("table", HashSet::from(["anki-table", "ambr-table", "tbl"])),
        (
            "p",
            HashSet::from(["ambr-note", "ambr-muted", "destaque", "alerta"]),
        ),
        (
            "blockquote",
            HashSet::from([
                "ambr-callout",
                "anki-callout",
                "anki-callout--trap",
                "anki-callout--pearl",
                "anki-callout--diff",
                "alerta",
            ]),
        ),
        (
            "section",
            HashSet::from([
                "anki-note-section",
                "anki-kv-section",
                "anki-list",
                "anki-steps",
                "anki-flow",
                "ambr-section",
            ]),
        ),
        ("footer", HashSet::from(["anki-sources"])),
        ("dl", HashSet::from(["anki-kv"])),
        ("ol", HashSet::from(["anki-steps"])),
        ("ul", HashSet::from(["anki-list", "anki-flow"])),
    ]));

    builder.url_schemes(HashSet::new());
    builder.url_relative(UrlRelative::PassThrough);
    builder.link_rel(None);
    builder.strip_comments(true);

    builder.attribute_filter(|tag, attr, val| {
        let allowed = match (tag, attr) {
            (_, "style") => safe_style(val),
            // `title`/`alt` recebem texto livre: `<` e `>` já decodificados pelo parser não
            // escapam de um valor de atributo entre aspas, e o serializador re-escapa
            // (`&lt;`, `&gt;`, `&amp;`, `&quot;`). Quebra real de atributo viraria atributo
            // separado e é pega pela comparação estrutural. Rejeitar `>` aqui transformava
            // `title="a &gt; b"` em falso-positivo.
            (_, "title") | ("img", "alt") => true,
            ("img", "src") => safe_img_src(val),
            ("th" | "td", "colspan" | "rowspan") => {
                !val.is_empty() && val.bytes().all(|c| c.is_ascii_digit())
            }
            ("th" | "td", "scope") => matches!(val, "row" | "col" | "rowgroup" | "colgroup"),
            _ => true,
        };
        if allowed {
            Some(Cow::Borrowed(val))
        } else {
            ATTR_REJECTED.with(|flag| flag.set(true));
            None
        }
    });

    builder
});

fn is_benign_html5_diff(orig: &str, clean: &str) -> bool {
    let mut o = orig.as_bytes();
    let mut c = clean.as_bytes();
    let mut in_tag = false;

    while !o.is_empty() && !c.is_empty() {
        if !in_tag {
            // HTML5 parser auto-inserts <tbody> in tables
            if c.starts_with(b"<tbody>") && !o.starts_with(b"<tbody>") {
                c = &c[b"<tbody>".len()..];
                continue;
            }
            if c.starts_with(b"</tbody>") && !o.starts_with(b"</tbody>") {
                c = &c[b"</tbody>".len()..];
                continue;
            }

            // Entity decodings in text nodes (shared decoder with html_text::try_decode_entity)
            if o[0] == b'&'
                && let Ok(o_str) = std::str::from_utf8(o)
                && let Some((decoded, consumed)) = crate::html_text::try_decode_entity(o_str)
                && !matches!(decoded, '<' | '>' | '&')
            {
                let mut buf = [0u8; 4];
                let encoded = decoded.encode_utf8(&mut buf).as_bytes();
                if c.starts_with(encoded) {
                    o = &o[consumed..];
                    c = &c[encoded.len()..];
                    continue;
                }
            }

            if o.starts_with("\u{00A0}".as_bytes()) && c.starts_with(b"&nbsp;") {
                o = &o["\u{00A0}".len()..];
                c = &c[6..];
                continue;
            }

            // Clinical inequalities / raw entity serialization in text
            if o[0] == b'>' && c.starts_with(b"&gt;") {
                o = &o[1..];
                c = &c[4..];
                continue;
            }
            if o[0] == b'&' && c.starts_with(b"&amp;") {
                o = &o[1..];
                c = &c[5..];
                continue;
            }
            if o[0] == b'<'
                && c.starts_with(b"&lt;")
                && !o
                    .get(1)
                    .is_some_and(|b| b.is_ascii_alphabetic() || matches!(b, b'/' | b'!' | b'?'))
            {
                o = &o[1..];
                c = &c[4..];
                continue;
            }
        } else {
            // Self-closing void tag normalization (<br/> -> <br>, <hr /> -> <hr>)
            if o.starts_with(b"/>") && c.starts_with(b">") {
                o = &o[2..];
                c = &c[1..];
                in_tag = false;
                continue;
            }
            let ws_len = o.iter().take_while(|&&b| b == b' ' || b == b'\t').count();
            if ws_len > 0 && o[ws_len..].starts_with(b"/>") && c.starts_with(b">") {
                o = &o[ws_len + 2..];
                c = &c[1..];
                in_tag = false;
                continue;
            }

            // Normalization of attribute quote styles: 'value' -> "value"
            if o.starts_with(b"'") && c.starts_with(b"\"") {
                o = &o[1..];
                c = &c[1..];
                continue;
            }
        }

        if o[0] == c[0] {
            if o[0] == b'<' {
                in_tag = true;
            } else if o[0] == b'>' {
                in_tag = false;
            }
            o = &o[1..];
            c = &c[1..];
            continue;
        }

        return false;
    }

    // Trailing benign insertions (e.g. </tbody> before EOF)
    while !in_tag && c.starts_with(b"</tbody>") {
        c = &c[b"</tbody>".len()..];
    }

    o.is_empty() && c.is_empty()
}

pub fn has_unsafe_html(value: &str) -> bool {
    if !value.contains('<') {
        return false;
    }
    // B14: limites determinísticos ANTES do parse. `html5ever` tem custo quadrático em
    // profundidade de aninhamento (20 mil `<div>` custavam ~4 s em release / ~72 s em debug)
    // e ainda assim a decisão era "seguro". Estouro ⇒ rejeição imediata, sem parse.
    //
    // Fail-closed sem perder o diagnóstico: este predicado responde apenas "rejeitar ou não".
    // Quem precisa do código específico (`HTML.FIELD_TOO_LARGE` / `HTML.NESTING_TOO_DEEP`)
    // chama [`html_field_limit_violation`] ANTES deste ponto — é o que `quality` e
    // `draft_compile` fazem.
    if html_field_limit_violation(value).is_some() {
        return true;
    }
    ATTR_REJECTED.with(|flag| flag.set(false));
    let cleaned = SANITIZER.clean(value).to_string();
    if ATTR_REJECTED.with(|flag| flag.get()) {
        return true;
    }
    if cleaned == value || is_benign_html5_diff(value, &cleaned) {
        return false;
    }
    // B13: decisão ESTRUTURAL. Diferença de serialização (case, ordem de atributos,
    // entidades, espaços em atributos, comentários descartados de propósito) é benigna;
    // qualquer token que o sanitizer removeu de fato não sobrevive à comparação.
    !is_benign_structural_diff(value, &cleaned)
}

/// Limite determinístico de bytes para um campo HTML.
///
/// O sanitizador (`html5ever`) tem custo superlinear em profundidade de aninhamento: medido
/// em `tests/html5_boundary.rs`, 20.000 `<div>` aninhados custavam 4,1 s (release) e 72,1 s
/// (debug) — e ainda assim eram ACEITOS. 16 KiB é ~44× o maior campo dos fixtures de
/// paridade do repositório (371 bytes) e corta qualquer payload de amplificação antes de
/// custar um parse.
pub const HTML_FIELD_MAX_BYTES: usize = 16 * 1024;

/// Profundidade máxima de aninhamento de elementos aceita em um campo HTML.
pub const HTML_FIELD_MAX_NESTING: usize = 32;

/// Diagnóstico próprio: campo acima de [`HTML_FIELD_MAX_BYTES`].
pub const HTML_FIELD_TOO_LARGE: &str = "HTML.FIELD_TOO_LARGE";

/// Diagnóstico próprio: aninhamento acima de [`HTML_FIELD_MAX_NESTING`].
pub const HTML_FIELD_TOO_DEEP: &str = "HTML.NESTING_TOO_DEEP";

/// Elementos HTML5 sem conteúdo: nunca abrem um nível de aninhamento.
const VOID_ELEMENTS: &[&str] = &["br", "col", "hr", "img", "wbr"];

/// Elementos cuja presença é decisão exclusiva do parser (o `<tbody>` implícito de tabelas).
/// Não carregam conteúdo ativo nem atributos relevantes, então são neutros na comparação.
const PARSER_IMPLIED_ELEMENTS: &[&str] = &["tbody"];

/// Aplica os limites determinísticos de campo (profundidade e bytes) sem parsear HTML.
///
/// Devolve o código de diagnóstico do primeiro limite violado, ou `None` quando o campo está
/// dentro dos limites. Pode ser chamado por qualquer portão que queira o diagnóstico
/// específico antes de invocar [`has_unsafe_html`].
///
/// A profundidade é testada ANTES dos bytes porque é ela — e não o tamanho — a causa do custo
/// superlinear do parser: o payload canônico de amplificação (20.000 `<div>`) também estoura
/// os 16 KiB, e reportá-lo como "campo grande" esconderia o motivo real. A varredura de
/// profundidade aborta no primeiro nível acima do limite (~130 bytes no payload de 20.000
/// `<div>`), então testá-la primeiro não custa um scan completo de campos grandes e rasos.
#[must_use]
pub fn html_field_limit_violation(value: &str) -> Option<&'static str> {
    if nesting_exceeds(value, HTML_FIELD_MAX_NESTING) {
        return Some(HTML_FIELD_TOO_DEEP);
    }
    if value.len() > HTML_FIELD_MAX_BYTES {
        return Some(HTML_FIELD_TOO_LARGE);
    }
    None
}

/// Mensagem legível para uma violação de limite, citando o limite real e o valor medido.
///
/// Existe para que o consumidor receba um diagnóstico honesto: antes, campo legítimo de
/// 17.007 bytes gerava "Unsafe HTML attributes or active content", o que é falso.
#[must_use]
pub fn html_field_limit_message(code: &str, value: &str) -> String {
    match code {
        HTML_FIELD_TOO_LARGE => format!(
            "HTML field has {} bytes, above the {HTML_FIELD_MAX_BYTES}-byte (16 KiB) field limit; \
             shorten the text or split it across cards",
            value.len()
        ),
        HTML_FIELD_TOO_DEEP => format!(
            "HTML field nests {} element levels deep, above the {HTML_FIELD_MAX_NESTING}-level \
             nesting limit; flatten the markup",
            html_field_depth(value)
        ),
        other => format!("HTML field violates the deterministic field limits ({other})"),
    }
}

/// Profundidade máxima de aninhamento observada no campo (varredura linear, sem alocação).
///
/// Só é chamada depois de um limite já decidido, para montar a mensagem — o custo é O(n) sobre
/// um campo que já foi rejeitado.
#[must_use]
pub fn html_field_depth(value: &str) -> usize {
    scan_nesting(value, usize::MAX)
}

/// Conta aninhamento aberto por varredura linear (sem alocação) e aborta ao passar de `max`.
fn nesting_exceeds(value: &str, max: usize) -> bool {
    scan_nesting(value, max) > max
}

/// Varredura linear de aninhamento: devolve a profundidade máxima, abortando ao passar de `max`.
fn scan_nesting(value: &str, max: usize) -> usize {
    let bytes = value.as_bytes();
    let mut depth = 0usize;
    let mut max_depth = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        if value[i..].starts_with("<!--") {
            i = match value[i + 4..].find("-->") {
                Some(offset) => i + 4 + offset + 3,
                None => bytes.len(),
            };
            continue;
        }
        let is_end = bytes.get(i + 1) == Some(&b'/');
        let name_start = if is_end { i + 2 } else { i + 1 };
        if !bytes
            .get(name_start)
            .is_some_and(|b| b.is_ascii_alphabetic())
        {
            i += 1;
            continue;
        }
        let Some(gt) = find_tag_gt(bytes, name_start) else {
            return max_depth;
        };
        let name = read_name(&value[name_start..gt]);
        if is_end {
            depth = depth.saturating_sub(1);
        } else if !VOID_ELEMENTS
            .iter()
            .any(|void| name.eq_ignore_ascii_case(void))
        {
            depth += 1;
            max_depth = max_depth.max(depth);
            if depth > max {
                return depth;
            }
        }
        i = gt + 1;
    }

    max_depth
}

/// Posição do `>` que fecha a tag, respeitando aspas simples e duplas em valores.
fn find_tag_gt(bytes: &[u8], start: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    let mut idx = start;

    while idx < bytes.len() {
        let byte = bytes[idx];
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None => match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'>' => return Some(idx),
                _ => {}
            },
        }
        idx += 1;
    }

    None
}

/// Nome do elemento/atributo: alfanuméricos ASCII, `-`, `_` e `:`.
fn read_name(slice: &str) -> &str {
    let end = slice
        .bytes()
        .position(|b| !(b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':')))
        .unwrap_or(slice.len());
    &slice[..end]
}

/// Token normalizado de um fragmento HTML (visão estrutural, não byte-a-byte).
#[derive(Debug, PartialEq, Eq)]
enum HtmlToken {
    /// Tag de abertura: nome minúsculo e conjunto de atributos normalizado e ordenado.
    Start {
        name: String,
        attrs: Vec<(String, String)>,
    },
    /// Tag de fechamento com nome minúsculo.
    End { name: String },
    /// Nó de texto com entidades decodificadas e espaços colapsados.
    Text(String),
    /// Construção que o sanitizador nunca emite (`<!doctype>`, `<?...>`, tag truncada).
    Foreign,
}

/// Compara entrada e saída do sanitizador na forma ESTRUTURAL.
///
/// Benigno = apenas diferença de serialização: case de tag/atributo, ordem de atributos,
/// entidades decodificadas, espaços em atributos (`class="x "` → `class="x"`), aspas,
/// auto-fechamento de elementos vazios, `<tbody>` implícito do parser e comentários que o
/// sanitizador remove de propósito. Qualquer tag, atributo ou texto que ele tenha de fato
/// removido aparece como token ausente e a comparação falha (fail-closed).
fn is_benign_structural_diff(original: &str, cleaned: &str) -> bool {
    normalized_tokens(original) == normalized_tokens(cleaned)
}

fn normalized_tokens(value: &str) -> Vec<HtmlToken> {
    tokenize(value)
        .into_iter()
        .filter(|token| !is_parser_implied(token))
        .collect()
}

fn is_parser_implied(token: &HtmlToken) -> bool {
    match token {
        HtmlToken::Start { name, attrs } => {
            attrs.is_empty() && PARSER_IMPLIED_ELEMENTS.contains(&name.as_str())
        }
        HtmlToken::End { name } => PARSER_IMPLIED_ELEMENTS.contains(&name.as_str()),
        HtmlToken::Text(_) | HtmlToken::Foreign => false,
    }
}

/// Tokeniza `value` aplicando as normalizações benignas descritas em
/// [`is_benign_structural_diff`]. O mesmo tokenizador é usado nos dois lados, então qualquer
/// divergência de leitura vira diferença de tokens e o campo é rejeitado (fail-closed).
fn tokenize(value: &str) -> Vec<HtmlToken> {
    let bytes = value.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0usize;

    while i < len {
        if bytes[i] != b'<' {
            let mut raw = String::new();
            while i < len && bytes[i] != b'<' {
                if bytes[i] == b'&'
                    && let Some((decoded, consumed)) =
                        crate::html_text::try_decode_entity(&value[i..])
                {
                    raw.push(decoded);
                    i += consumed;
                    continue;
                }
                let ch = value[i..]
                    .chars()
                    .next()
                    .expect("índice em fronteira de caractere");
                raw.push(ch);
                i += ch.len_utf8();
            }
            push_text(&mut tokens, &raw);
            continue;
        }

        // Comentários: o sanitizador os remove de propósito (`strip_comments(true)`), então
        // são benignos — desde que não escondam marcação. Um comentário com `<` dentro é
        // ambíguo para consumidores que não parseiam HTML: rejeita (fail-closed).
        if value[i..].starts_with("<!--") {
            let (body_start, body_end, next) = match value[i + 4..].find("-->") {
                Some(offset) => (i + 4, i + 4 + offset, i + 4 + offset + 3),
                None => (i + 4, len, len),
            };
            if value[body_start..body_end].contains('<') {
                tokens.push(HtmlToken::Foreign);
            }
            i = next;
            continue;
        }

        let is_end = bytes.get(i + 1) == Some(&b'/');
        let name_start = if is_end { i + 2 } else { i + 1 };
        let starts_with_alpha = bytes
            .get(name_start)
            .is_some_and(|b| b.is_ascii_alphabetic());
        if !starts_with_alpha {
            if !is_end && bytes.get(i + 1).is_some_and(|b| matches!(b, b'!' | b'?')) {
                tokens.push(HtmlToken::Foreign);
                match bytes[i..].iter().position(|&b| b == b'>') {
                    Some(offset) => i += offset + 1,
                    None => break,
                }
                continue;
            }
            push_text(&mut tokens, "<");
            i += 1;
            continue;
        }

        let Some(gt) = find_tag_gt(bytes, name_start) else {
            tokens.push(HtmlToken::Foreign);
            break;
        };
        let raw_name = read_name(&value[name_start..gt]);
        let name = raw_name.to_ascii_lowercase();
        if is_end {
            tokens.push(HtmlToken::End { name });
        } else {
            let attrs = parse_attributes(&value[name_start + raw_name.len()..gt]);
            tokens.push(HtmlToken::Start { name, attrs });
        }
        i = gt + 1;
    }

    tokens
}

/// Atributos normalizados: nomes minúsculos, valores com entidades decodificadas, `class`
/// com espaços colapsados (é o que o sanitizador faz com classes permitidas), duplicados
/// descartados (o parser mantém o primeiro) e ordem canônica por nome.
fn parse_attributes(inner: &str) -> Vec<(String, String)> {
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut rest = inner;

    loop {
        rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace());
        if rest.is_empty() {
            break;
        }
        if let Some(after_slash) = rest.strip_prefix('/') {
            rest = after_slash;
            continue;
        }
        let name_len = rest
            .find(|c: char| c.is_ascii_whitespace() || matches!(c, '=' | '>' | '/'))
            .unwrap_or(rest.len());
        if name_len == 0 {
            break;
        }
        let raw_name = &rest[..name_len];
        let after_name = rest[name_len..].trim_start_matches(|c: char| c.is_ascii_whitespace());
        rest = after_name;
        let mut value = String::new();
        if let Some(after_eq) = after_name.strip_prefix('=') {
            let after_eq = after_eq.trim_start_matches(|c: char| c.is_ascii_whitespace());
            match after_eq.chars().next() {
                Some(quote @ ('"' | '\'')) => {
                    let body = &after_eq[1..];
                    match body.find(quote) {
                        Some(end) => {
                            value = decode_entities(&body[..end]);
                            rest = &body[end + 1..];
                        }
                        None => {
                            value = decode_entities(body);
                            rest = "";
                        }
                    }
                }
                _ => {
                    let end = after_eq
                        .find(|c: char| c.is_ascii_whitespace())
                        .unwrap_or(after_eq.len());
                    value = decode_entities(&after_eq[..end]);
                    rest = &after_eq[end..];
                }
            }
        }
        let name = raw_name.to_ascii_lowercase();
        if attrs.iter().any(|(existing, _)| *existing == name) {
            continue;
        }
        let value = if name == "class" {
            value.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
        } else {
            value
        };
        attrs.push((name, value));
    }

    attrs.sort_by(|left, right| left.0.cmp(&right.0));
    attrs
}

fn decode_entities(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] == b'&'
            && let Some((decoded, consumed)) = crate::html_text::try_decode_entity(&raw[i..])
        {
            out.push(decoded);
            i += consumed;
            continue;
        }
        let ch = raw[i..]
            .chars()
            .next()
            .expect("índice em fronteira de caractere");
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

/// Espaços ASCII colapsados em um único espaço; trechos só com espaço são descartados.
fn push_text(tokens: &mut Vec<HtmlToken>, raw: &str) {
    let mut collapsed = String::with_capacity(raw.len());
    let mut in_space = false;
    for ch in raw.chars() {
        if ch.is_ascii_whitespace() {
            if !in_space {
                collapsed.push(' ');
                in_space = true;
            }
            continue;
        }
        collapsed.push(ch);
        in_space = false;
    }
    let trimmed = collapsed.trim_matches(' ');
    if trimmed.is_empty() {
        return;
    }
    if let Some(HtmlToken::Text(previous)) = tokens.last_mut() {
        previous.push(' ');
        previous.push_str(trimmed);
        return;
    }
    tokens.push(HtmlToken::Text(trimmed.to_owned()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_benign_semantic_tags_and_classes() {
        let safe_cases = [
            "<section class=\"anki-note-section\"><p><b>Título</b></p><p>Texto</p></section>",
            "<dl class=\"anki-kv\"><dt>Chave</dt><dd>Valor</dd></dl>",
            "<section class=\"anki-kv-section\"><p><b>Par</b></p><dl class=\"anki-kv\"><dt>K</dt><dd>V</dd></dl></section>",
            "<blockquote class=\"anki-callout anki-callout--trap\"><p><b>Cuidado:</b> Perigo</p></blockquote>",
            "<blockquote class=\"anki-callout anki-callout--pearl\"><p><b>Dica:</b> Peróla</p></blockquote>",
            "<blockquote class=\"anki-callout anki-callout--diff\"><p><b>Diferencial:</b> Comparação</p></blockquote>",
            "<section class=\"anki-list\"><ul><li>Item 1</li><li>Item 2</li></ul></section>",
            "<section class=\"anki-steps\"><ol><li>Passo 1</li><li>Passo 2</li></ol></section>",
            "<section class=\"anki-flow\"><ul><li>Etapa A</li></ul></section>",
            "<details class=\"anki-detail\"><summary><b>Expandir</b></summary><p>Detalhe</p></details>",
            "<details class=\"hint-details\"><summary class=\"hint-summary\">Dica</summary>Texto</details>",
            "<footer class=\"anki-sources\"><small><b>Fonte:</b> <cite>Harrison 21ed</cite></small></footer>",
            "<table class=\"anki-table\"><thead><tr><th>A</th></tr></thead><tbody><tr><td>1</td></tr></tbody></table>",
            "<figure class=\"anki-figure\"><img src=\"fig1.png\" alt=\"Foto\"><figcaption>Legenda</figcaption></figure>",
            "<figure class=\"anki-figure anki-table-figure\"><img src=\"tbl1.png\" alt=\"Tabela\"></figure>",
            "<span class=\"destaque\" style=\"color: red\">Destaque seguro</span>",
        ];

        for case in safe_cases {
            assert!(!has_unsafe_html(case), "should be accepted as safe: {case}");
        }
    }

    #[test]
    fn accepts_clinical_inequalities_and_table_normalization() {
        let benign_cases = [
            "<p>K+ > 5.5 mEq/L</p>",
            "<p>pH < 7.35</p>",
            "<p>Troponina <= 0.04</p>",
            "<p>Glicemia >= 126</p>",
            "<p>Item A & Item B</p>",
            "<p>Texto com &#124; pipe escapado</p>",
            "<p>Linha 1<br/>Linha 2</p>",
            "<p>Linha 1<br />Linha 2</p>",
            "<table><tr><th>Sem Tbody</th></tr><tr><td>Valor</td></tr></table>",
        ];

        for case in benign_cases {
            assert!(
                !has_unsafe_html(case),
                "benign normalization should not be marked unsafe: {case}"
            );
        }
    }

    #[test]
    fn accepts_decoded_clinical_entities() {
        let clinical_cases = [
            "<p>A &rarr; B &uarr; PA &darr; FC &harr; equilíbrio &rArr; conduta</p>",
            "<p>Temperatura 37,5 &plusmn; 0,5 &deg;C; dose 50 &mu;g</p>",
            "<p>Receptores &alpha;, &beta;, &gamma;, &delta; e &Delta;pH</p>",
            "<p>Faixa 10&ndash;20 &bull; p &ne; 0,05 &bull; IMC &asymp; 25 kg/m&sup2; &bull; 10 mm&sup3;</p>",
        ];

        for case in clinical_cases {
            assert!(
                !has_unsafe_html(case),
                "valid clinical entities must not trigger has_unsafe_html: {case}"
            );
        }
    }

    #[test]
    fn rejects_active_content_and_script_injection() {
        let unsafe_cases = [
            "<script>alert(1)</script>",
            "<script src=\"evil.js\"></script>",
            "<b onclick=\"alert(1)\">Texto</b>",
            "<b oNcLiCk='alert(1)'>Texto</b>",
            "<img src=\"x\" onerror=\"alert(1)\">",
            "<svg><circle onload=\"alert(1)\"/></svg>",
            "<iframe src=\"javascript:alert(1)\"></iframe>",
            "<a href=\"javascript:alert(1)\">Link</a>",
            "<object data=\"evil.swf\"></object>",
            "<embed src=\"evil.swf\">",
            "<form action=\"evil.com\"><input type=\"text\"></form>",
        ];

        for case in unsafe_cases {
            assert!(has_unsafe_html(case), "must be rejected as unsafe: {case}");
        }
    }

    #[test]
    fn rejects_dangerous_css_and_escapes() {
        let unsafe_styles = [
            "<span style=\"background:url(https://evil.com/x)\">Texto</span>",
            "<span style=\"background:u&#114;l(x)\">Texto</span>",
            "<span style=\"background:u\\72l(x)\">Texto</span>",
            "<span style=\"expression(alert(1))\">Texto</span>",
            "<span style=\"behavior:url(evil.htc)\">Texto</span>",
            "<span style=\"binding:url(evil.xml)\">Texto</span>",
            "<span style=\"/*comment*/color:red\">Texto</span>",
            "<span style=\"@import 'evil.css'\">Texto</span>",
        ];

        for case in unsafe_styles {
            assert!(
                has_unsafe_html(case),
                "dangerous CSS must be rejected: {case}"
            );
        }
    }

    #[test]
    fn rejects_media_traversal_and_remote_urls() {
        let unsafe_media = [
            "<img src=\"../secret.png\" alt=\"x\">",
            "<img src=\"sub/folder.png\" alt=\"x\">",
            "<img src=\"https://remote.com/pic.png\" alt=\"x\">",
            "<img src=\"data:image/png;base64,AAAA\" alt=\"x\">",
            "<img src=\"javascript:alert(1)\" alt=\"x\">",
        ];

        for case in unsafe_media {
            assert!(
                has_unsafe_html(case),
                "media traversal or remote URL must be rejected: {case}"
            );
        }
    }

    #[test]
    fn rejects_unknown_classes_and_attributes() {
        let unknown_cases = [
            "<div class=\"unregistered-class\">Texto</div>",
            "<span class=\"destaque evil-class\">Texto</span>",
            "<p id=\"custom-id\">Texto</p>",
            "<p data-bind=\"x\">Texto</p>",
        ];

        for case in unknown_cases {
            assert!(
                has_unsafe_html(case),
                "unknown classes and attributes must be rejected: {case}"
            );
        }
    }

    /// B13: HTML seguro em forma alternativa de serialização é ACEITO.
    #[test]
    fn accepts_alternative_safe_serializations() {
        let safe_cases = [
            // case de tag
            "<B>Negrito</B>",
            "<Strong>Forte</Strong>",
            // case de tag + de atributo
            "<IMG SRC=\"fig1.png\" ALT=\"Foto\">",
            "<IMG SRC='fig1.png' ALT='Foto'>",
            // comentário de texto (o sanitizador remove de propósito)
            "<p>Texto</p><!-- comentário benigno -->",
            "<!-- inicial --><p>Texto</p>",
            // espaço dentro do valor de atributo (o sanitizador colapsa classes)
            "<span class=\"destaque \">Destaque</span>",
            "<span class=\"  destaque   cloze  \">Destaque</span>",
            // entidades em atributos
            "<span title=\"a &gt; b\">Comparação</span>",
            "<span title=\"a &amp; b\">Comercial</span>",
            "<span title='a &quot;b&quot; c'>Aspas</span>",
            // auto-fechamento de elementos vazios
            "<p>Linha<BR>quebra</p>",
            "<p>Linha<BR/>quebra</p>",
            "<hr/>",
            // case em tabela + tbody implícito
            "<table><THEAD><TR><TH>Exame</TH></TR></THEAD><TBODY><TR><TD>1</TD></TR></TBODY></table>",
        ];

        for case in safe_cases {
            assert!(
                !has_unsafe_html(case),
                "benign serialization must be accepted: {case}"
            );
        }
    }

    /// B13 (contraprova): a troca para decisão estrutural NÃO afrouxa nenhum payload perigoso.
    #[test]
    fn rejects_active_content_after_structural_comparison() {
        let unsafe_cases = [
            "<script>alert(1)</script>",
            "<SCRIPT>alert(1)</SCRIPT>",
            "<script src=\"evil.js\"></script>",
            "<img src=\"x\" onerror=\"alert(1)\">",
            "<IMG SRC=\"x\" ONERROR=\"alert(1)\">",
            "<b onclick=\"alert(1)\">Texto</b>",
            "<b oNcLiCk='alert(1)'>Texto</b>",
            "<a href=\"javascript:alert(1)\">Link</a>",
            "<a HREF=\"JavaScript:alert(1)\">Link</a>",
            "<iframe src=\"javascript:alert(1)\"></iframe>",
            "<svg onload=\"alert(1)\"></svg>",
            "<svg/onload=alert(1)>",
            "<span style=\"background:url(javascript:alert(1))\">Texto</span>",
            "<span style=\"background:url(https://evil.com/x)\">Texto</span>",
            "<span style=\"expression(alert(1))\">Texto</span>",
            "<span STYLE=\"background:u&#114;l(x)\">Texto</span>",
            "<object data=\"evil.swf\"></object>",
            "<embed src=\"evil.swf\">",
            "<form action=\"https://evil.com\"><input type=\"text\"></form>",
            "<style>body{background:url(evil)}</style>",
            "<meta http-equiv=\"refresh\" content=\"0;url=javascript:alert(1)\">",
            "<template><script>alert(1)</script></template>",
            "<div class=\"unregistered-class\">Texto</div>",
            "<p id=\"custom-id\">Texto</p>",
            "<img src=\"../secret.png\" alt=\"x\">",
            "<img src=\"https://remote.com/pic.png\" alt=\"x\">",
            "<img src=\"data:image/png;base64,AAAA\" alt=\"x\">",
            // comentário que esconde marcação não é tratado como benigno
            "<!--<script>alert(1)</script>-->",
            "<!--\"><script>alert(1)</script>-->",
        ];

        for case in unsafe_cases {
            assert!(
                has_unsafe_html(case),
                "dangerous payload must stay rejected: {case}"
            );
        }
    }

    /// B14: limites determinísticos de bytes e de profundidade, com diagnóstico próprio.
    #[test]
    fn reports_deterministic_field_limits() {
        let within = format!("<p>{}</p>", "a".repeat(HTML_FIELD_MAX_BYTES - 8));
        assert!(within.len() <= HTML_FIELD_MAX_BYTES);
        assert_eq!(html_field_limit_violation(&within), None);

        let oversized = format!("<p>{}</p>", "a".repeat(HTML_FIELD_MAX_BYTES));
        assert!(oversized.len() > HTML_FIELD_MAX_BYTES);
        assert_eq!(
            html_field_limit_violation(&oversized),
            Some(HTML_FIELD_TOO_LARGE)
        );
        assert!(
            has_unsafe_html(&oversized),
            "oversized field must be unsafe"
        );
        let large_message = html_field_limit_message(HTML_FIELD_TOO_LARGE, &oversized);
        assert!(
            large_message.contains(&oversized.len().to_string())
                && large_message.contains(&HTML_FIELD_MAX_BYTES.to_string())
                && large_message.contains("16 KiB"),
            "message must cite the measured size and the real limit: {large_message}"
        );

        let at_limit = format!(
            "{}{}",
            "<div>".repeat(HTML_FIELD_MAX_NESTING),
            "</div>".repeat(HTML_FIELD_MAX_NESTING)
        );
        assert_eq!(html_field_limit_violation(&at_limit), None);
        assert!(!has_unsafe_html(&at_limit), "depth == limit must stay safe");

        let over_limit = format!(
            "{}{}",
            "<div>".repeat(HTML_FIELD_MAX_NESTING + 1),
            "</div>".repeat(HTML_FIELD_MAX_NESTING + 1)
        );
        assert_eq!(
            html_field_limit_violation(&over_limit),
            Some(HTML_FIELD_TOO_DEEP)
        );
        assert!(has_unsafe_html(&over_limit), "depth > limit must be unsafe");
        let deep_message = html_field_limit_message(HTML_FIELD_TOO_DEEP, &over_limit);
        assert!(
            deep_message.contains(&(HTML_FIELD_MAX_NESTING + 1).to_string())
                && deep_message.contains(&HTML_FIELD_MAX_NESTING.to_string()),
            "message must cite the measured depth and the real limit: {deep_message}"
        );

        // Profundidade não conta elementos vazios nem fechamentos soltos.
        let wide = "<br><hr><img src=\"a.png\">".repeat(4);
        assert_eq!(html_field_limit_violation(&wide), None);
    }

    /// B14: payload de aninhamento profundo é rejeitado sem parse caro.
    #[test]
    fn rejects_deep_nesting_payload_in_bounded_time() {
        use std::time::Instant;

        let payload = format!("{}{}", "<div>".repeat(20_000), "</div>".repeat(20_000));
        assert!(payload.len() > HTML_FIELD_MAX_BYTES);

        // O payload também estoura os 16 KiB: o motivo reportado é a profundidade, não o tamanho.
        assert_eq!(
            html_field_limit_violation(&payload),
            Some(HTML_FIELD_TOO_DEEP)
        );
        assert_eq!(html_field_depth(&payload), 20_000);

        let started = Instant::now();
        assert!(
            has_unsafe_html(&payload),
            "20k nested divs must be rejected"
        );
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "deep nesting must be rejected in bounded time, took {elapsed:?}"
        );
    }
}
