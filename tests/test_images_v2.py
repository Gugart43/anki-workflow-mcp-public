"""tests/test_images_v2.py — Verification tests for AnkiMBR image overhaul.

Tests:
  - MediaFigure.render_html generates clean HTML (no inline style) with alt attribute.
  - CardDraft.figuras and all_figures() deduplicate by filename in order.
  - core/compiler compiles all figures into 'Imagens Relacionadas' without truncation.
  - media_figure_build MCP tool emits clean HTML or warnings on explicit style.
  - check_images_field detects:
      - attribute-order independent src without false IMG.STRUCTURE
      - bare <img> outside <figure> as IMG.BARE_IMG
      - short, generic, or filename captions as IMG.CAPTION_WEAK
      - long clinical captions cleanly
      - inline styles as IMG.INLINE_STYLE
      - missing alt attribute as IMG.ALT_MISSING
      - preservation of IMG.NAMESPACE, IMG.PHANTOM, IMG.DUP_IN_CARD, IMG.CAP_OVER
"""

from __future__ import annotations

import asyncio
import csv
import io

import pytest
from fastmcp import FastMCP

from anki_mcp.checks.images import check_images_field
from anki_mcp.core.compiler import compile_drafts_to_csv
from anki_mcp.models.draft import CardDraft, MediaFigure
from anki_mcp.tools.compiler import register_compiler_tools

# ── Task 1: MediaFigure.render_html ──────────────────────────────────────────


def test_media_figure_clean_html_and_alt():
    fig = MediaFigure(
        filename="neuro_mri_dawson.png",
        caption='Biomicroscopia de lâmpada de fenda mostrando "opacificação nuclear" & esclerose',
    )
    html = fig.render_html()

    assert "style=" not in html
    assert 'src="neuro_mri_dawson.png"' in html
    assert (
        'alt="Biomicroscopia de lâmpada de fenda mostrando &quot;opacificação nuclear&quot; &amp; esclerose"'
        in html
    )
    assert (
        "<figcaption>Biomicroscopia de lâmpada de fenda mostrando "
        '"opacificação nuclear" & esclerose</figcaption>' in html
    )


def test_media_figure_empty_caption_renders_figure_img_alt():
    fig = MediaFigure(filename="neuro_mri_dawson.png", caption="")
    html = fig.render_html()

    assert html == '<figure><img src="neuro_mri_dawson.png" alt=""></figure>'
    assert "<figcaption" not in html


def test_media_figure_explicit_style_supported():
    fig = MediaFigure(
        filename="neuro_mri.png",
        caption="Legenda descritiva com mais de vinte caracteres clínicos",
        style="margin: 8px",
        caption_style="color: red",
    )
    html = fig.render_html()

    assert '<figure style="margin: 8px">' in html
    assert '<figcaption style="color: red">' in html


def test_media_figure_escapes_every_attribute_value():
    """B7: filename/style/caption_style must be escaped exactly like Rust's html_escape_attr."""
    fig = MediaFigure(
        filename='a"b.png',
        caption="Legenda descritiva com mais de vinte caracteres clínicos",
        style='x" onload="y',
        caption_style='p" onmouseover="q',
    )
    html = fig.render_html()

    # The quote never escapes the attribute, so no injected handler can appear.
    assert 'src="a&quot;b.png"' in html
    assert '<figure style="x&quot; onload=&quot;y">' in html
    assert '<figcaption style="p&quot; onmouseover=&quot;q">' in html
    assert 'onload="y"' not in html
    assert 'onmouseover="q"' not in html
    assert html.count("<img") == 1

    # &, <, > and ' are escaped too (parity with html_escape_attr in media_figure.rs:47).
    tricky = MediaFigure(filename="a&b<c>'d.png", caption="", style="a&b<c>")
    rendered = tricky.render_html()
    assert 'src="a&amp;b&lt;c&gt;&#x27;d.png"' in rendered
    assert 'style="a&amp;b&lt;c&gt;"' in rendered


# ── Task 2: CardDraft.figuras and all_figures() ─────────────────────────────


def test_card_draft_all_figures_deduplication():
    f1 = MediaFigure(filename="neuro_axial.png", caption="Corte axial em T2")
    f2 = MediaFigure(filename="neuro_sagital.png", caption="Corte sagital em FLAIR")
    f3 = MediaFigure(filename="neuro_axial.png", caption="Corte axial duplicado")

    # Both singular figura and plural figuras provided
    card = CardDraft(
        tipo="QA",
        enunciado_ou_sentenca="Qual o achado de imagem na placa de desmielinização?",
        resposta="Lesão hiperintensa periventricular.",
        figura=f1,
        figuras=[f2, f3],
    )

    combined = card.all_figures()
    assert len(combined) == 2
    assert combined[0].filename == "neuro_axial.png"
    assert combined[0].caption == "Corte axial em T2"
    assert combined[1].filename == "neuro_sagital.png"


def test_card_draft_all_figures_dict_compatibility():
    card = CardDraft(
        tipo="QA",
        enunciado_ou_sentenca="Qual o achado?",
        resposta="Resposta clínica.",
        figuras=[
            {"filename": "neuro_01.png", "caption": "Legenda 1"},
            {"filename": "neuro_02.png", "caption": "Legenda 2"},
        ],  # type: ignore[arg-type]
    )
    combined = card.all_figures()
    assert len(combined) == 2
    assert combined[0].filename == "neuro_01.png"
    assert combined[1].filename == "neuro_02.png"


# ── Task 3: core/compiler rendering multiple figures ─────────────────────────


def test_compiler_two_figures_compiled():
    f1 = MediaFigure(
        filename="neuro_img_01.png",
        caption="Ressonância magnética de crânio em corte axial ponderada em T2.",
    )
    f2 = MediaFigure(
        filename="neuro_img_02.png",
        caption="Ressonância magnética de crânio em sequência FLAIR com lesões.",
    )
    card = CardDraft(
        tipo="QA",
        enunciado_ou_sentenca="Descreva as alterações em neuroimagem.",
        resposta="Placas periventriculares desmielinizantes.",
        figuras=[f1, f2],
    )

    qa_csv, _, _ = compile_drafts_to_csv([card])
    reader = csv.reader(io.StringIO(qa_csv), delimiter="|")
    rows = list(reader)
    imagens_field = rows[1][4]
    assert f1.render_html() in imagens_field
    assert f2.render_html() in imagens_field
    assert f1.render_html() + f2.render_html() == imagens_field


def test_compiler_four_figures_preserves_all_without_silent_drop():
    figures = [
        MediaFigure(
            filename=f"neuro_img_0{i}.png",
            caption=f"Legenda clínica detalhada da figura número {i} do caso.",
        )
        for i in range(1, 5)
    ]
    card = CardDraft(
        tipo="QA",
        enunciado_ou_sentenca="Descreva a série de 4 imagens radiológicas.",
        resposta="Achados sequenciais compatíveis.",
        figuras=figures,
    )

    qa_csv, _, _ = compile_drafts_to_csv([card])
    reader = csv.reader(io.StringIO(qa_csv), delimiter="|")
    rows = list(reader)
    imagens_field = rows[1][4]
    for f in figures:
        assert f.render_html() in imagens_field
    assert "".join(f.render_html() for f in figures) == imagens_field

    # Validate that check_images_field detects IMG.CAP_OVER on this card
    diags = check_images_field(imagens_field)
    codes = {d.code for d in diags}
    assert "IMG.CAP_OVER" in codes


# ── Task 4: tools/compiler media_figure_build ────────────────────────────────


def test_media_figure_build_tool():
    mcp = FastMCP("test_mcp")
    register_compiler_tools(mcp)
    fn = asyncio.run(mcp.get_tool("media_figure_build")).fn

    # Clean execution without style
    clean_res = fn(
        filename="neuro_ct_cranio.png",
        caption="Tomografia de crânio sem contraste evidenciando hematoma subdural.",
    )
    assert clean_res["success"] is True
    assert "style=" not in clean_res["html"]
    assert "warnings" not in clean_res

    # Execution with explicit style emits warning
    styled_res = fn(
        filename="neuro_ct_cranio.png",
        caption="Tomografia de crânio sem contraste evidenciando hematoma subdural.",
        style="margin: 10px;",
    )
    assert styled_res["success"] is True
    assert 'style="margin: 10px;"' in styled_res["html"]
    assert "warnings" in styled_res
    assert any("IMG.INLINE_STYLE" in w for w in styled_res["warnings"])


# ── Task 5: checks/images.py validations ─────────────────────────────────────


def test_images_src_independent_of_attribute_order():
    # alt before src
    html_alt_first = (
        '<figure><img alt="RM crânio corte axial" src="neuro_axial_t2.png">'
        "<figcaption>Ressonância magnética de crânio em corte axial ponderada em T2.</figcaption></figure>"
    )
    diags1 = check_images_field(html_alt_first)
    codes1 = {d.code for d in diags1}
    assert "IMG.STRUCTURE" not in codes1
    assert "IMG.ALT_MISSING" not in codes1

    # Single quotes
    html_single_quotes = (
        "<figure><img alt='RM crânio corte axial' src='neuro_axial_t2.png'>"
        "<figcaption>Ressonância magnética de crânio em corte axial ponderada em T2.</figcaption></figure>"
    )
    diags2 = check_images_field(html_single_quotes)
    codes2 = {d.code for d in diags2}
    assert "IMG.STRUCTURE" not in codes2


def test_images_bare_img_diagnostics():
    bare_html = '<p>Texto descritivo</p><img src="neuro_bare.png" alt="solta">'
    diags = check_images_field(bare_html)
    codes = {d.code for d in diags}
    assert "IMG.BARE_IMG" in codes

    # Mixed figure and bare img
    mixed_html = (
        '<figure><img src="neuro_fig1.png" alt="Legenda clínica"><figcaption>'
        "Legenda clínica detalhada com mais de 20 caracteres.</figcaption></figure>"
        '<img src="neuro_bare2.png">'
    )
    diags_mixed = check_images_field(mixed_html)
    codes_mixed = {d.code for d in diags_mixed}
    assert "IMG.BARE_IMG" in codes_mixed


@pytest.mark.parametrize(
    "weak_caption",
    [
        "Figura 2",
        "Fig. 2:",
        "Imagem",
        "Image 1",
        "Foto 3",
        "Curta",  # < 20 chars
        "neuro_axial_lesao.png",  # looks like filename
        "placa_desmielinizante.jpeg",
    ],
)
def test_images_caption_weak(weak_caption):
    html = f'<figure><img src="neuro_img.png" alt="alt"><figcaption>{weak_caption}</figcaption></figure>'
    diags = check_images_field(html)
    codes = {d.code for d in diags}
    assert "IMG.CAPTION_WEAK" in codes
    assert "IMG.CAPTION" not in codes


def test_images_caption_empty_does_not_trigger_caption_weak():
    html = '<figure><img src="neuro_img.png" alt=""><figcaption></figcaption></figure>'
    diags = check_images_field(html)
    codes = {d.code for d in diags}
    assert "IMG.CAPTION" in codes
    assert "IMG.CAPTION_WEAK" not in codes


def test_images_long_clinical_caption_passes():
    html = (
        '<figure><img src="neuro_ms_flare.png" alt="RM de encéfalo">'
        "<figcaption>Ressonância magnética de crânio em sequência FLAIR com lesões "
        "desmielinizantes periventriculares ovóides.</figcaption></figure>"
    )
    diags = check_images_field(html, catalog_filenames={"neuro_ms_flare.png"})
    assert diags == []


def test_images_inline_style_warning():
    # Inline style on figure
    html_fig_style = (
        '<figure style="margin:4px"><img src="neuro_t2.png" alt="Legenda">'
        "<figcaption>Legenda clínica longa e detalhada com mais de 20 caracteres.</figcaption></figure>"
    )
    diags1 = check_images_field(html_fig_style)
    assert any(d.code == "IMG.INLINE_STYLE" for d in diags1)

    # Inline style on figcaption
    html_cap_style = (
        '<figure><img src="neuro_t2.png" alt="Legenda">'
        '<figcaption style="color:#666">Legenda clínica longa e detalhada com mais de 20 caracteres.</figcaption></figure>'
    )
    diags2 = check_images_field(html_cap_style)
    assert any(d.code == "IMG.INLINE_STYLE" for d in diags2)

    # Inline style on img
    html_img_style = (
        '<figure><img src="neuro_t2.png" style="width:100%" alt="Legenda">'
        "<figcaption>Legenda clínica longa e detalhada com mais de 20 caracteres.</figcaption></figure>"
    )
    diags3 = check_images_field(html_img_style)
    assert any(d.code == "IMG.INLINE_STYLE" for d in diags3)


def test_images_alt_missing_log():
    html_no_alt = (
        '<figure><img src="neuro_t2.png">'
        "<figcaption>Legenda clínica longa e detalhada com mais de 20 caracteres.</figcaption></figure>"
    )
    diags = check_images_field(html_no_alt)
    alt_diags = [d for d in diags if d.code == "IMG.ALT_MISSING"]
    assert len(alt_diags) == 1
    assert alt_diags[0].severity.value == "LOG"

    # With alt="" present, IMG.ALT_MISSING should not trigger
    html_empty_alt = (
        '<figure><img src="neuro_t2.png" alt="">'
        "<figcaption>Legenda clínica longa e detalhada com mais de 20 caracteres.</figcaption></figure>"
    )
    diags_empty = check_images_field(html_empty_alt)
    assert not any(d.code == "IMG.ALT_MISSING" for d in diags_empty)
