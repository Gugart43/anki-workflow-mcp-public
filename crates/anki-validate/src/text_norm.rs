//! Primitivas compartilhadas de normalização de texto (Onda 2, B24).
//!
//! Antes deste módulo o crate carregava cópias divergentes das mesmas duas regras:
//!
//! - **conjunto de glifos de bullet** — `qa::BULLET_INLINE_RE`, `qa::LEADING_MARKER_RE`,
//!   `notes::BULLET_LINE_RE`, `notes::BULLET_GLYPHS`, `notes::LEGACY_ITEM_PREFIXES` e
//!   `html_text::LEADING_BULLET_RE` (além de `domain_profile::clean_segment`, que continua
//!   com a sua própria lista por estar fora do escopo desta frente);
//! - **dobra de acentos/caixa** — `tags::normalize_ptbr`, `interference::normalize_for_tokens`
//!   e `quality_content::fold_accents_lower` (`hints::fold_latin_char` segue à parte: o seu
//!   contrato mapeia *qualquer* pontuação para espaço, o que [`fold`] deliberadamente não faz).
//!
//! Este módulo passa a ser a única fonte da verdade para as duas regras. Todos os
//! consumidores citados delegam aqui.
//!
//! Nada aqui depende de `unicode-normalization` (que **não** está em `Cargo.lock`): a dobra de
//! diacríticos usa a tabela explícita [`DIACRITIC_FOLD`] e descarta as marcas combinantes da
//! faixa `U+0300..=U+036F`, o que dá o mesmo resultado de uma NFD para o latim ocidental.

/// Glifos reconhecidos como marcador de bullet no início de uma linha ou de um item.
///
/// União de todas as tabelas antes espalhadas pelo crate (ver a documentação do módulo)
/// com os glifos tipográficos equivalentes que o brief da Onda 2 lista. Ordem estável:
/// glifos não-ASCII primeiro (ordem de ponto de código), depois a pontuação ASCII.
pub const BULLET_GLYPHS: &[char] = &[
    '•', // U+2022 BULLET — qa.rs, notes.rs, html_text.rs, domain_profile.rs
    '‣', // U+2023 TRIANGULAR BULLET
    '◦', // U+25E6 WHITE BULLET
    '·', // U+00B7 MIDDLE DOT
    '▪', // U+25AA BLACK SMALL SQUARE — notes.rs, html_text.rs, domain_profile.rs
    '▫', // U+25AB WHITE SMALL SQUARE
    '▸', // U+25B8 BLACK RIGHT-POINTING SMALL TRIANGLE — notes.rs, html_text.rs
    '►', // U+25BA BLACK RIGHT-POINTING POINTER — notes.rs, html_text.rs
    '●', // U+25CF BLACK CIRCLE
    '○', // U+25CB WHITE CIRCLE
    '-', // U+002D HYPHEN-MINUS — qa.rs, notes.rs, html_text.rs, domain_profile.rs
    '*', // U+002A ASTERISK — qa.rs, notes.rs, html_text.rs, domain_profile.rs
    '–', // U+2013 EN DASH — qa.rs, html_text.rs, domain_profile.rs
    '—', // U+2014 EM DASH — qa.rs, html_text.rs, domain_profile.rs
];

/// Tabela de dobra: diacrítico latino (minúsculo) → base ASCII.
///
/// Só precisa de minúsculas: [`fold`] aplica `char::to_lowercase` antes de consultar.
/// A tabela está **ordenada por ponto de código** e sem chaves duplicadas — [`fold_char`]
/// depende disso para a busca binária (há teste unitário garantindo a invariante).
///
/// É a união das tabelas de `tags::normalize_ptbr` (português), de
/// `interference::normalize_for_tokens` (`ä å ë ï ö ü ñ`) e de
/// `quality_content::fold_accents_lower`, com o repertório latino-oeste de
/// `hints::fold_latin_char` para não perder cobertura.
pub const DIACRITIC_FOLD: &[(char, char)] = &[
    ('à', 'a'),
    ('á', 'a'),
    ('â', 'a'),
    ('ã', 'a'),
    ('ä', 'a'),
    ('å', 'a'),
    ('ç', 'c'),
    ('è', 'e'),
    ('é', 'e'),
    ('ê', 'e'),
    ('ë', 'e'),
    ('ì', 'i'),
    ('í', 'i'),
    ('î', 'i'),
    ('ï', 'i'),
    ('ñ', 'n'),
    ('ò', 'o'),
    ('ó', 'o'),
    ('ô', 'o'),
    ('õ', 'o'),
    ('ö', 'o'),
    ('ø', 'o'),
    ('ù', 'u'),
    ('ú', 'u'),
    ('û', 'u'),
    ('ü', 'u'),
    ('ý', 'y'),
    ('ÿ', 'y'),
    ('ā', 'a'),
    ('ă', 'a'),
    ('ą', 'a'),
    ('ć', 'c'),
    ('ĉ', 'c'),
    ('ċ', 'c'),
    ('č', 'c'),
    ('ď', 'd'),
    ('đ', 'd'),
    ('ē', 'e'),
    ('ĕ', 'e'),
    ('ė', 'e'),
    ('ę', 'e'),
    ('ě', 'e'),
    ('ĝ', 'g'),
    ('ğ', 'g'),
    ('ġ', 'g'),
    ('ģ', 'g'),
    ('ĥ', 'h'),
    ('ħ', 'h'),
    ('ĩ', 'i'),
    ('ī', 'i'),
    ('ĭ', 'i'),
    ('į', 'i'),
    ('ı', 'i'),
    ('ĵ', 'j'),
    ('ķ', 'k'),
    ('ĸ', 'k'),
    ('ĺ', 'l'),
    ('ļ', 'l'),
    ('ľ', 'l'),
    ('ŀ', 'l'),
    ('ł', 'l'),
    ('ń', 'n'),
    ('ņ', 'n'),
    ('ň', 'n'),
    ('ŉ', 'n'),
    ('ŋ', 'n'),
    ('ō', 'o'),
    ('ŏ', 'o'),
    ('ő', 'o'),
    ('ŕ', 'r'),
    ('ŗ', 'r'),
    ('ř', 'r'),
    ('ś', 's'),
    ('ŝ', 's'),
    ('ş', 's'),
    ('š', 's'),
    ('ţ', 't'),
    ('ť', 't'),
    ('ŧ', 't'),
    ('ũ', 'u'),
    ('ū', 'u'),
    ('ŭ', 'u'),
    ('ů', 'u'),
    ('ű', 'u'),
    ('ų', 'u'),
    ('ŵ', 'w'),
    ('ŷ', 'y'),
    ('ź', 'z'),
    ('ż', 'z'),
    ('ž', 'z'),
];

/// Base ASCII de um diacrítico minúsculo, se mapeado em [`DIACRITIC_FOLD`].
fn fold_char(c: char) -> Option<char> {
    DIACRITIC_FOLD
        .binary_search_by_key(&c, |&(accented, _)| accented)
        .ok()
        .map(|idx| DIACRITIC_FOLD[idx].1)
}

/// `true` para as marcas combinantes (Unicode `Mn`) da faixa `U+0300..=U+036F`.
///
/// Texto vindo de NFD carrega `a` + `U+0301` em vez de `á`; descartar as marcas dá a
/// mesma dobra que a tabela, sem depender de `unicode-normalization`.
fn is_combining_mark(c: char) -> bool {
    matches!(c, '\u{0300}'..='\u{036F}')
}

/// Dobra texto para comparação insensível a caixa e a acentos.
///
/// Minúsculas + remoção de diacríticos: `"Hipertensão Arterial Sistêmica"` →
/// `"hipertensao arterial sistemica"`. Marcas combinantes são descartadas, então texto
/// já decomposto (NFD) e texto pré-composto convergem.
///
/// A função **não** mexe em pontuação, espaços ou glifos: quem precisa disso deve combinar
/// com [`strip_leading_marker`] ou com as rotinas de tokenização de cada chamador.
/// Isso é o que a distingue de `hints::normalize_hint_text`, que mapeia qualquer
/// caractere fora de `[a-z0-9 ]` para espaço.
#[must_use]
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        for lower in ch.to_lowercase() {
            if is_combining_mark(lower) {
                continue;
            }
            out.push(fold_char(lower).unwrap_or(lower));
        }
    }
    out
}

/// `true` quando `c` é um bullet **tipográfico** (não-ASCII) da união [`BULLET_GLYPHS`].
///
/// Separa `• ▪ ▸ ► ‣ ◦ · ▫ ● ○` de `-`, `*`, `–` e `—`: em texto clínico o hífen e o
/// asterisco são pontuação comum (`-item`, `**negrito**`, `p < 0,05`) e só valem como
/// marcador quando o chamador decide — por exemplo exigindo espaço à direita, como faz
/// `notes::BULLET_LINE_RE`.
#[must_use]
pub fn is_glyph_bullet(c: char) -> bool {
    !c.is_ascii() && BULLET_GLYPHS.contains(&c)
}

/// Remove um marcador de bullet inicial (ver [`BULLET_GLYPHS`]) e o espaço que o segue.
///
/// Devolve o restante e o glifo removido, ou `(s, None)` quando não há marcador — nesse
/// caso a string é devolvida **inalterada** (nem mesmo com espaços à esquerda aparados),
/// para que o chamador saiba que não houve remoção.
///
/// Espaço à esquerda é tolerado antes do glifo. A remoção é de **um** marcador; para varrer
/// uma sequência (`"• - item"`) o chamador itera — é o que `html_text` faz.
///
/// ```
/// # use anki_validate::text_norm::strip_leading_marker;
/// assert_eq!(strip_leading_marker("• Item"), ("Item", Some('•')));
/// assert_eq!(strip_leading_marker("  ▪  Item"), ("Item", Some('▪')));
/// assert_eq!(strip_leading_marker("Item"), ("Item", None));
/// ```
#[must_use]
pub fn strip_leading_marker(s: &str) -> (&str, Option<char>) {
    let trimmed = s.trim_start();
    match trimmed.chars().next() {
        Some(glyph) if BULLET_GLYPHS.contains(&glyph) => {
            (trimmed[glyph.len_utf8()..].trim_start(), Some(glyph))
        }
        _ => (s, None),
    }
}

/// `true` quando a linha começa com um marcador de bullet (ver [`BULLET_GLYPHS`]).
///
/// Espaço à esquerda é tolerado. Marcadores numerados (`1)`, `(1)`, `1.`) **não** são
/// bullets: quem precisa deles usa a regex de numeração do chamador.
#[must_use]
pub fn is_bullet_line(s: &str) -> bool {
    strip_leading_marker(s).1.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diacritic_table_is_sorted_and_unique() {
        for pair in DIACRITIC_FOLD.windows(2) {
            let (left, right) = (&pair[0], &pair[1]);
            assert!(
                left.0 < right.0,
                "DIACRITIC_FOLD fora de ordem entre {:?} e {:?}",
                left.0,
                right.0
            );
        }
        for &(accented, base) in DIACRITIC_FOLD {
            assert!(
                accented.is_lowercase(),
                "chave {accented:?} deveria ser minúscula"
            );
            assert!(
                base.is_ascii_lowercase(),
                "valor de {accented:?} deveria ser ASCII minúsculo, veio {base:?}"
            );
        }
    }

    #[test]
    fn bullet_glyph_union_covers_every_legacy_table() {
        for glyph in [
            '•', '▪', '▸', '►', '-', '*', '–', '—', '‣', '◦', '·', '▫', '●', '○',
        ] {
            assert!(
                BULLET_GLYPHS.contains(&glyph),
                "glifo {glyph:?} deveria estar na união"
            );
        }
        for &glyph in BULLET_GLYPHS {
            assert_eq!(strip_leading_marker(&glyph.to_string()).1, Some(glyph));
        }
    }

    #[test]
    fn fold_lowercases_and_strips_portuguese_accents() {
        assert_eq!(
            fold("Hipertensão Arterial Sistêmica"),
            "hipertensao arterial sistemica"
        );
        assert_eq!(fold("Âncora"), "ancora");
        assert_eq!(fold("AVC isquêmico"), "avc isquemico");
        assert_eq!(fold("Coração"), "coracao");
        assert_eq!(fold("úlcera"), "ulcera");
    }

    #[test]
    fn fold_covers_the_union_of_the_former_tables() {
        // `tags::normalize_ptbr`: á à ã â é ê í ó ô õ ú ü ç
        assert_eq!(fold("áàãâéêíóôõúüç"), "aaaaeeiooouuc");
        // `interference::normalize_for_tokens`: ä å ë ï ö ñ (sobre normalize_ptbr)
        assert_eq!(fold("äåëïöñ"), "aaeion");
        // `quality_content::fold_accents_lower` também aceitava maiúsculas
        assert_eq!(fold("ÁÀÂÃÄÉÈÊËÍÌÎÏÓÒÔÕÖÚÙÛÜÇ"), "aaaaaeeeeiiiiooooouuuuc");
        // repertório latino-oeste de `hints::fold_latin_char`
        for (accented, base) in [
            ('ā', 'a'),
            ('ć', 'c'),
            ('đ', 'd'),
            ('ě', 'e'),
            ('ģ', 'g'),
            ('ħ', 'h'),
            ('į', 'i'),
            ('ĵ', 'j'),
            ('ĸ', 'k'),
            ('ł', 'l'),
            ('ň', 'n'),
            ('ő', 'o'),
            ('ř', 'r'),
            ('ş', 's'),
            ('ŧ', 't'),
            ('ų', 'u'),
            ('ŵ', 'w'),
            ('ŷ', 'y'),
            ('ž', 'z'),
        ] {
            assert_eq!(fold(&accented.to_string()), base.to_string());
        }
    }

    #[test]
    fn fold_drops_combining_marks_so_nfd_and_nfc_converge() {
        let nfc = "Hipertensão";
        let nfd = "Hipertensa\u{0303}o";
        assert_eq!(fold(nfc), fold(nfd));
        assert_eq!(fold(nfd), "hipertensao");
    }

    #[test]
    fn fold_keeps_punctuation_and_digits() {
        assert_eq!(fold("PA ≥ 140/90 mmHg: 4,5 h."), "pa ≥ 140/90 mmhg: 4,5 h.");
        assert_eq!(fold(""), "");
    }

    #[test]
    fn strip_leading_marker_reports_the_glyph_and_strips_one_marker() {
        assert_eq!(strip_leading_marker("• Item"), ("Item", Some('•')));
        assert_eq!(strip_leading_marker("- Item"), ("Item", Some('-')));
        assert_eq!(strip_leading_marker("* Item"), ("Item", Some('*')));
        assert_eq!(strip_leading_marker("•• Item"), ("• Item", Some('•')));
    }

    #[test]
    fn strip_leading_marker_leaves_plain_text_untouched() {
        assert_eq!(strip_leading_marker("Item"), ("Item", None));
        assert_eq!(strip_leading_marker("  Item"), ("  Item", None));
        assert_eq!(strip_leading_marker(""), ("", None));
        // marcação markdown não é bullet: a decisão de tratá-la é do chamador
        assert_eq!(strip_leading_marker("**ID:** 1"), ("*ID:** 1", Some('*')));
    }

    #[test]
    fn is_bullet_line_accepts_leading_space_and_all_union_glyphs() {
        for glyph in BULLET_GLYPHS {
            assert!(is_bullet_line(&format!("{glyph} Item")));
            assert!(is_bullet_line(&format!("  {glyph} Item")));
        }
        assert!(!is_bullet_line("Item"));
        assert!(!is_bullet_line("1) Item"));
        assert!(!is_bullet_line("(1) Item"));
        assert!(!is_bullet_line(""));
    }
}
