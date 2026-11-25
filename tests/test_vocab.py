"""Tests for the semantic vocabulary layer (core/vocab.py + FMT.CLASS_UNKNOWN)."""

from __future__ import annotations

import re
from pathlib import Path

import pytest

from anki_mcp.checks import csv_structure
from anki_mcp.core import vocab
from anki_mcp.models import CLOZE_FIELDS, QA_FIELDS, Severity

HEADER = "|".join(QA_FIELDS)
CL_HEADER = "|".join(CLOZE_FIELDS)


def _row(notas: str, enunciado: str = "Pergunta?", resposta: str = "Resposta.") -> str:
    return f"{enunciado}|{resposta}|Medicina::Neurologia|{notas}||ref.pdf|Tag"


def _row_img(images: str, notas: str = "") -> str:
    return f"Pergunta?|Resposta.|Medicina::Neurologia|{notas}|{images}|ref.pdf|Tag"


def _codes(text: str, note_type: str = "QA") -> list[str]:
    return [d.code for d in csv_structure.check_csv(text, note_type)]


def _cls(text: str, note_type: str = "QA") -> list[str]:
    """Only the unknown-class tokens reported (FMT.CLASS_UNKNOWN)."""
    out = []
    for d in csv_structure.check_csv(text, note_type):
        if d.code == "FMT.CLASS_UNKNOWN":
            out.append(d.message.split("class '")[1].split("'")[0])
    return out


def _cls_in(notas: str = "", images: str = "") -> list[str]:
    return _cls(f"{HEADER}\n{_row_img(images, notas)}")


# ── Vocabulary integrity ─────────────────────────────────────────────────────


def test_vocabulary_is_flat_and_deduplicated():
    assert vocab.ALLOWED_CLASSES == frozenset().union(*vocab.CLASS_VOCABULARY.values())
    assert "nl" in vocab.ALLOWED_CLASSES
    assert "alerta" in vocab.ALLOWED_CLASSES


def test_unknown_classes_detects_tokens_not_in_vocabulary():
    assert vocab.unknown_classes("nl rot") == []
    assert vocab.unknown_classes("nl bogus") == ["bogus"]
    assert vocab.unknown_classes("") == []


def test_tag_allowed_respects_images_field():
    assert vocab.is_allowed_tag("b", is_images_field=False)
    assert vocab.is_allowed_tag("span", is_images_field=False)
    assert not vocab.is_allowed_tag("img", is_images_field=False)
    assert vocab.is_allowed_tag("img", is_images_field=True)


def test_void_tags_are_declared():
    assert {"br", "hr", "img"} <= vocab.VOID_TAGS


def test_cheatsheet_mentions_every_group_and_class():
    sheet = vocab.vocabulary_cheatsheet()
    for group in vocab.CLASS_VOCABULARY:
        assert group in sheet
    for cls in vocab.ALLOWED_CLASSES:
        assert f"`.{cls}`" in sheet


# ── Structural acceptance of the designed markup ─────────────────────────────


def test_designed_list_markup_is_accepted():
    notas = (
        '<div class="nl">'
        '<span class="nl-i">• <b class="rot">Genética:</b> expansão CAG ≥ 36 no gene <i>HTT</i>.</span>'
        '<span class="nl-i">• <b class="rot">Herança:</b> autossômica dominante.</span>'
        "</div>"
    )
    text = f"{HEADER}\n{_row(notas)}"
    assert _codes(text) == []


def test_callout_and_quantitative_markup_is_accepted():
    notas = (
        '<span class="alerta">Risco de suicídio com VMAT2.</span>'
        '<span class="pearl">Preferir atípicos se psicose.</span>'
        'Dose <span class="dose">12,5 mg</span> e limiar <span class="faixa">≥ 40</span>.'
        'Alvo <span class="lab">25(OH)D</span>.'
    )
    text = f"{HEADER}\n{_row(notas)}"
    assert _codes(text) == []


def test_criteria_table_and_emphasis_are_accepted():
    notas = (
        '<div class="crit">'
        '<span class="crit-i">Excluir mimetizadores.</span>'
        '<span class="crit-i">Confirmar com teste genético.</span>'
        "</div>"
        '<table class="tbl"><thead><tr><th class="rot">A</th><th>B</th></tr></thead>'
        "<tbody><tr><td>1</td><td>2</td></tr></tbody></table>"
        '<span class="termo">coreia</span> <span class="sigla">VMAT2</span> <span class="uni">mg/dia</span>'
    )
    text = f"{HEADER}\n{_row(notas)}"
    assert _codes(text) == []


def test_nested_div_span_and_lists_are_accepted():
    notas = "<div><span><ul><li>a</li><li>b</li></ul></span></div><hr><small>nota</small>"
    text = f"{HEADER}\n{_row(notas)}"
    assert _codes(text) == []


def test_sub_and_sup_and_mark_are_accepted():
    notas = 'Volume <span class="lab">30 mL/min/1,73m<sup>2</sup></span> e H<sub>2</sub>O <mark>crítico</mark>'
    text = f"{HEADER}\n{_row(notas)}"
    assert _codes(text) == []


# ── The new WARN fires correctly ─────────────────────────────────────────────


def test_unknown_class_warns_but_does_not_fail():
    notas = '<div class="totalmente-inventada">x</div>'
    text = f"{HEADER}\n{_row(notas)}"
    diags = csv_structure.check_csv(text, "QA")
    class_diags = [d for d in diags if d.code == "FMT.CLASS_UNKNOWN"]
    assert len(class_diags) == 1
    assert class_diags[0].severity == Severity.WARN
    assert not [d for d in diags if d.severity == Severity.CRITICAL]


def test_unknown_class_reported_once_per_token():
    notas = '<div class="mau">a</div><span class="mau">b</span>'
    text = f"{HEADER}\n{_row(notas)}"
    diags = [d for d in csv_structure.check_csv(text, "QA") if d.code == "FMT.CLASS_UNKNOWN"]
    assert len(diags) == 1


def test_multiple_unknown_classes_all_reported():
    notas = '<div class="mau1"><span class="mau2">x</span></div>'
    text = f"{HEADER}\n{_row(notas)}"
    diags = [d for d in csv_structure.check_csv(text, "QA") if d.code == "FMT.CLASS_UNKNOWN"]
    assert {d.message.split("class ")[1].split("'")[1] for d in diags} == {"mau1", "mau2"}


def test_unknown_class_suggestion_lists_the_vocabulary():
    notas = '<div class="mau">x</div>'
    text = f"{HEADER}\n{_row(notas)}"
    d = next(d for d in csv_structure.check_csv(text, "QA") if d.code == "FMT.CLASS_UNKNOWN")
    assert ".nl" in (d.suggestion or "")
    assert ".alerta" in (d.suggestion or "")


# ── Real markup from the previous batches still passes ───────────────────────


def test_legacy_bold_br_markup_still_passes():
    notas = "• <b>Clínica:</b> detalhe<br>• <b>Manejo:</b> outro detalhe"
    text = f"{HEADER}\n{_row(notas)}"
    assert _codes(text) == []


def test_cloze_accepts_designed_markup():
    sentenca = "Tríade de domínios {{c1::motor}}, {{c2::cognitivo}} e {{c3::psiquiátrico}}."
    notas = '<div class="nl"><span class="nl-i">• <b class="rot">Nota:</b> detalhe</span></div>'
    line = "|".join([sentenca, notas, "Medicina::Neurologia", "", "ref.pdf", "Tag"])
    text = f"{CL_HEADER}\n{line}"
    assert _codes(text, "CL") == []


def test_images_field_still_rejects_figure_outside_images_field():
    notas = '<figure><img src="a.png"></figure>'
    text = f"{HEADER}\n{_row(notas)}"
    assert "FMT.TAG_FORBIDDEN" in _codes(text)


def test_images_field_accepts_img_with_class_and_alt():
    line = "|".join(
        [
            "Pergunta?",
            "Resposta.",
            "Medicina::Neurologia",
            "nota",
            '<img class="media" src="huntington_img_p42_1.png" alt="RM com atrofia caudada">',
            "ref.pdf",
            "Tag",
        ]
    )
    text = f"{HEADER}\n{line}"
    assert _codes(text) == []


# ── Adversarial: parsing of the class attribute ──────────────────────────────


def test_unquoted_class_value_is_scanned():
    assert _cls_in("<div class=bogus>x</div>") == ["bogus"]
    assert _cls_in("<div class=nl>x</div>") == []


def test_whitespace_around_equals_is_scanned():
    assert _cls_in('<div class = "bogus">x</div>') == ["bogus"]
    assert _cls_in("<div class\t=\t'bogus'>x</div>") == ["bogus"]


def test_single_quoted_class_is_scanned():
    assert _cls_in("<div class='bogus'>x</div>") == ["bogus"]


def test_uppercase_attribute_name_is_scanned():
    assert _cls_in('<div CLASS="bogus">x</div>') == ["bogus"]


def test_class_token_is_case_sensitive():
    # CSS class names are case-sensitive: "NL" is not ".nl"
    assert _cls_in('<div class="NL">x</div>') == ["NL"]


def test_escaped_tag_is_text_not_markup():
    assert _cls_in('&lt;span class="bogus"&gt; é a sintaxe') == []


def test_prose_mentioning_class_attribute_is_not_markup():
    assert _cls_in('A sintaxe class="bogus" aplica-se ao campo.') == []


def test_prefixed_attributes_do_not_match():
    for value in (
        '<span data-class="bogus">x</span>',
        '<span myclass="bogus">x</span>',
        '<span subclass="bogus">x</span>',
    ):
        assert _cls_in(value) == [], value


def test_html_comment_is_flagged_once_without_class_noise():
    value = '<!-- class="bogus" -->texto'
    codes = _codes(f"{HEADER}\n{_row(value)}")
    assert codes.count("FMT.TAG_FORBIDDEN") == 1
    assert "FMT.CLASS_UNKNOWN" not in codes


def test_comment_content_yields_no_phantom_diagnostics():
    # everything inside a comment is dead text: exactly one diagnostic
    value = '<!-- <b>x</b> & <div class="bogus"> $ -->ok'
    codes = _codes(f"{HEADER}\n{_row(value)}")
    assert codes == ["FMT.TAG_FORBIDDEN"], codes


def test_empty_class_attribute_is_not_reported():
    assert _cls_in('<div class="">x</div>') == []


def test_duplicate_class_attributes_are_all_scanned():
    assert _cls_in('<div class="nl" class="bogus">x</div>') == ["bogus"]


def test_dotted_css_syntax_is_not_a_valid_token():
    assert _cls_in('<span class=".nl">x</span>') == [".nl"]


def test_cloze_syntax_inside_class_is_reported():
    assert _cls_in('<div class="{{c1::bogus}}">x</div>') == ["{{c1::bogus}}"]


def test_whitespace_variants_split_tokens():
    assert _cls_in('<span class="nl\u00a0bogus">x</span>') == ["bogus"]
    assert _cls_in('<span class="nl\tbogus">x</span>') == ["bogus"]
    assert _cls_in('<span class="  nl   bogus  ">x</span>') == ["bogus"]


# ── Adversarial: broken markup that used to pass silently ────────────────────


def test_unbalanced_quote_is_flagged_not_silently_swallowed():
    # `<span class="nl>x</span>` makes the browser eat the rest of the cell as
    # the attribute value — the text never renders. Must never pass silently.
    value = '<span class="nl>x</span>'
    codes = _codes(f"{HEADER}\n{_row(value)}")
    assert "FMT.ENTITY_MISSING" in codes
    assert "FMT.TAG_UNCLOSED" in codes


def test_unbalanced_single_quote_is_flagged():
    value = "<span class='nl>x</span>"
    codes = _codes(f"{HEADER}\n{_row(value)}")
    assert "FMT.ENTITY_MISSING" in codes


def test_gt_inside_quoted_attribute_is_legal():
    # a literal '>' inside an attribute value is valid HTML, not a raw entity
    value = '<img src="a.png" alt="PA > 140 mmHg">'
    codes = _codes(f"{HEADER}\n{_row_img(value)}")
    assert "FMT.ENTITY_MISSING" not in codes
    assert codes == []


def test_gt_inside_class_value_is_only_a_class_warning():
    value = '<span class="a>b">x</span>'
    codes = _codes(f"{HEADER}\n{_row(value)}")
    assert codes == ["FMT.CLASS_UNKNOWN"]


def test_malformed_quote_does_not_copy_tokens_across_tags():
    # the old regex matched from `class="` of tag 1 to the next quote in tag 2
    value = '<span class="foo>t</span><span class="nl">'
    assert _cls_in(value) == []


# ── Adversarial: field coverage ──────────────────────────────────────────────


def test_classes_are_checked_in_enunciado_and_resposta():
    line = _row("x", enunciado='<span class="bogus">Q?</span>')
    assert _cls(f"{HEADER}\n{line}") == ["bogus"]
    line = _row("x", resposta='<span class="bogus">R</span>')
    assert _cls(f"{HEADER}\n{line}") == ["bogus"]


def test_classes_are_checked_in_cloze_sentence():
    sentenca = 'Dose {{c1::12,5 mg}} <span class="bogus">x</span>'
    line = "|".join([sentenca, "", "Medicina::Neurologia", "", "ref.pdf", "Tag"])
    assert _cls(f"{CL_HEADER}\n{line}", "CL") == ["bogus"]


def test_class_is_allowed_on_img_figure_and_figcaption():
    markup = (
        '<figure class="media">'
        '<img class="media" src="a.png" alt="RM">'
        '<figcaption class="media">Figura 1</figcaption>'
        "</figure>"
    )
    assert _codes(f"{HEADER}\n{_row_img(markup)}") == []


def test_unknown_class_inside_images_field_is_reported():
    fig = '<figcaption class="bogus">x</figcaption>'
    img = '<img class="bogus" src="a.png">'
    assert _cls(f"{HEADER}\n{_row_img(fig)}") == ["bogus"]
    assert _cls(f"{HEADER}\n{_row_img(img)}") == ["bogus"]


# ── Adversarial: idempotency and vocabulary invariants ───────────────────────


def test_validator_is_idempotent():
    value = '<div class="nl"><span class="bogus">x</span></div>'
    text = f"{HEADER}\n{_row_img(value, value)}"
    first = [(d.code, d.row, d.field, d.message) for d in csv_structure.check_csv(text, "QA")]
    second = [(d.code, d.row, d.field, d.message) for d in csv_structure.check_csv(text, "QA")]
    assert first == second
    assert first  # the fixture must produce diagnostics, else this proves nothing


def test_class_vocabulary_groups_are_disjoint():
    owner: dict[str, str] = {}
    for group, members in vocab.CLASS_VOCABULARY.items():
        for member in members:
            assert member not in owner, f"{member} in {owner.get(member)} and {group}"
            owner[member] = group


def test_tag_sets_are_internally_consistent():
    known = vocab.ALLOWED_TAGS | vocab.IMAGES_ONLY_TAGS
    assert vocab.PAIRED_TAGS <= known
    assert vocab.VOID_TAGS <= known
    assert not (vocab.PAIRED_TAGS & vocab.VOID_TAGS)
    assert vocab.PAIRED_TAGS.isdisjoint(vocab.IMAGES_ONLY_TAGS - {"figure", "figcaption"})


def test_allowed_attrs_follow_the_spec():
    assert vocab.allowed_attrs_for("img") == frozenset({"class", "src", "alt", "width", "height"})
    assert vocab.allowed_attrs_for("td") == frozenset({"class", "colspan", "rowspan"})
    assert vocab.allowed_attrs_for("th") == frozenset({"class", "colspan", "rowspan"})
    assert vocab.allowed_attrs_for("span") == frozenset({"class"})


@pytest.mark.parametrize("tag", sorted(vocab.ALLOWED_TAGS | vocab.IMAGES_ONLY_TAGS))
def test_tag_whitelist_matches_the_vocab_helper(tag):
    """The checker's hardcoded sets must agree with vocab.is_allowed_tag."""
    markup = f"<{tag}>" if tag in vocab.VOID_TAGS else f"<{tag}>x</{tag}>"
    for is_images_field in (False, True):
        line = _row_img(markup) if is_images_field else _row_img("", markup)
        forbidden = "FMT.TAG_FORBIDDEN" in _codes(f"{HEADER}\n{line}")
        assert forbidden is (not vocab.is_allowed_tag(tag, is_images_field=is_images_field)), (
            tag,
            is_images_field,
        )


# ── B8: parity with crates/anki-validate/src/vocab.rs ───────────────────────

#: Rust is the source of truth: its literals are parsed here instead of being
#: copied, so a *subset* comparison can no longer hide drift in either direction.
RUST_VOCAB_RS = (
    Path(__file__).resolve().parents[1] / "crates" / "anki-validate" / "src" / "vocab.rs"
)


def _rust_str_const(name: str) -> frozenset[str]:
    """Parse ``const NAME: &[&str] = &["a", "b", ...];`` out of ``vocab.rs``.

    Simple literal parse, no new dependency: the constants are flat arrays of
    string literals. A renamed/removed constant fails loudly (``assert``) rather
    than degrading into an empty set that would pass every comparison.
    """
    source = RUST_VOCAB_RS.read_text(encoding="utf-8")
    match = re.search(
        rf"(?:pub )?const {name}\s*:\s*&\[&str\]\s*=\s*&\[(.*?)\];",
        source,
        re.DOTALL,
    )
    assert match is not None, f"const {name} not found in {RUST_VOCAB_RS}"
    tokens = re.findall(r'"([^"]*)"', match.group(1))
    assert tokens, f"const {name} in {RUST_VOCAB_RS} has no string literals"
    return frozenset(tokens)


RUST_ALLOWED_TAGS = _rust_str_const("ALLOWED_TAGS")
RUST_IMAGES_ONLY_TAGS = _rust_str_const("IMAGES_ONLY_TAGS")
RUST_PAIRED_TAGS = _rust_str_const("PAIRED_TAGS")
RUST_ALLOWED_CLASSES = _rust_str_const("ALLOWED_CLASSES")


def _diff_report(label: str, py_set: frozenset[str], rust_set: frozenset[str]) -> str:
    return (
        f"{label} diverges from vocab.rs: "
        f"python-only={sorted(py_set - rust_set)} rust-only={sorted(rust_set - py_set)}"
    )


def test_tag_vocabulary_is_exactly_the_rust_vocabulary():
    """Strict equality (not subset): a tag Rust adds must break this test."""
    assert vocab.ALLOWED_TAGS == RUST_ALLOWED_TAGS, _diff_report(
        "ALLOWED_TAGS", vocab.ALLOWED_TAGS, RUST_ALLOWED_TAGS
    )
    assert vocab.IMAGES_ONLY_TAGS == RUST_IMAGES_ONLY_TAGS, _diff_report(
        "IMAGES_ONLY_TAGS", vocab.IMAGES_ONLY_TAGS, RUST_IMAGES_ONLY_TAGS
    )
    assert vocab.PAIRED_TAGS == RUST_PAIRED_TAGS, _diff_report(
        "PAIRED_TAGS", vocab.PAIRED_TAGS, RUST_PAIRED_TAGS
    )


def test_tag_sets_cover_the_rust_void_and_images_split():
    """``VOID_TAGS`` is exactly everything Rust allows minus everything it pairs."""
    known = RUST_ALLOWED_TAGS | RUST_IMAGES_ONLY_TAGS
    assert vocab.VOID_TAGS == known - RUST_PAIRED_TAGS
    assert vocab.PAIRED_TAGS <= known


def test_class_vocabulary_is_exactly_the_rust_vocabulary():
    assert vocab.ALLOWED_CLASSES == RUST_ALLOWED_CLASSES, _diff_report(
        "ALLOWED_CLASSES", vocab.ALLOWED_CLASSES, RUST_ALLOWED_CLASSES
    )
    for cls in ("cloze", "cloze-inactive", "destaque", "hint-details", "hint-summary"):
        assert cls in vocab.ALLOWED_CLASSES, cls


def test_semantic_blocks_are_accepted_without_class_warnings():
    """Same CSV that the Rust validator accepts must not raise FMT.CLASS_UNKNOWN here."""
    notas = (
        '<div class="anki-note">'
        '<section class="anki-note-section">'
        '<span class="anki-detail">detalhe</span>'
        '<dl class="anki-kv"><dt class="anki-kv-section">Chave</dt><dd>valor</dd></dl>'
        "</section>"
        '<section class="anki-steps"><ol><li>passo</li></ol></section>'
        '<blockquote class="anki-callout anki-callout--pearl">pérola</blockquote>'
        '<details class="anki-details"><summary class="hint-summary">ver</summary>x</details>'
        '<table class="anki-table"><thead><tr><th>a</th></tr></thead>'
        "<tbody><tr><td>1</td></tr></tbody></table>"
        '<footer class="anki-sources">Fonte: ref.pdf</footer>'
        "</div>"
    )
    text = f"{HEADER}\n{_row(notas)}"
    assert _cls(text) == []
    # B9: the same block also emits zero tag diagnostics — the tag vocabulary is
    # a strict mirror of vocab.rs, so `<section>`/`<dl>`/`<details>` are canonical.
    assert _codes(text) == []
    assert vocab.unknown_classes("anki-note-section anki-list anki-steps anki-flow") == []


def test_canonical_v2_blocks_are_not_flagged_as_forbidden_tags():
    """Regression: `<section>`, `<details>/<summary>`, `<h2>`, `<dl>`, `<abbr>`.

    The Rust compiler (oracle) returns no diagnostic for this note; Python used
    to answer 18x ``FMT.TAG_FORBIDDEN`` (CRITICAL) because ``ALLOWED_TAGS`` was
    truncated to 28 tags.
    """
    notas = (
        '<section class="anki-note-section">'
        '<h2 class="anki-kv-section">Tratamento</h2>'
        '<dl class="anki-kv">'
        '<dt class="anki-kv-section"><abbr title="Ativador do plasminogênio tecidual">tPA</abbr></dt>'
        "<dd>Alteplase EV</dd>"
        "</dl>"
        '<details class="anki-details"><summary class="hint-summary">Ver detalhe</summary>'
        "<p>Janela de 4,5 h</p></details>"
        "<h3>Notas</h3>"
        "<pre>dose = peso * 0,9</pre>"
        "<p>2<sup>a</sup> linha<wbr>opcional</p>"
        "</section>"
    )
    text = f"{HEADER}\n{_row(notas)}"
    assert _codes(text) == [], _codes(text)
    for tag in ("section", "details", "summary", "h2", "dl", "dt", "dd", "abbr", "pre", "wbr"):
        assert vocab.is_allowed_tag(tag, is_images_field=False), tag
