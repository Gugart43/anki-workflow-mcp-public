"""Regressions for literal paragraph tags in the epilepsy export."""

from anki_mcp.checks.csv_structure import check_csv
from anki_mcp.checks.notes import check_notes
from anki_mcp.core.compiler import compile_drafts_to_csv
from anki_mcp.models import Severity
from anki_mcp.models.draft import CardDraft


def test_compile_paragraph_and_clinical_quote_as_renderable_html():
    notes = "<p><b>Contexto:</b> Explicação com valor &lt; 5.</p><blockquote>Armadilha clínica.</blockquote>"
    draft = CardDraft.model_validate(
        {
            "id": "B01-CL-001",
            "tipo": "CLOZE",
            "enunciado_ou_sentenca": "Na avaliação clínica, o achado caracteriza {{c1::epilepsia}}.",
            "notas_adicionais": notes,
            "campo_estudo": "Medicina::Neurologia",
            "referencias": ["Fonte, p. 3"],
            "etiquetas": ["Neurologia"],
        }
    )
    _, csv_text, _ = compile_drafts_to_csv([draft])
    assert "<p><b>Contexto:</b>" in csv_text
    assert "<blockquote>Armadilha clínica.</blockquote>" in csv_text
    assert "&lt;p&gt;" not in csv_text
    assert "&lt;blockquote&gt;" not in csv_text
    assert not [d for d in check_csv(csv_text, "CL") if d.severity.value == "CRITICAL"]


def test_explanatory_notes_are_not_an_atomicity_export_blocker():
    notes = "<p>" + "Explicação fundamentada na fonte com contexto e diferenciais. " * 100 + "</p>"
    diags = check_notes(notes)
    assert not [d for d in diags if d.severity == Severity.CRITICAL]
    assert any(d.code == "NOTE.LENGTH_WARN" for d in diags)


def test_explanatory_notes_may_mention_the_answer_without_duplication_warning():
    answer = "zona epileptogênica"
    notes = "<p>A zona epileptogênica integra a avaliação pré-cirúrgica.</p><p>O contexto adicional explica a relação com as outras zonas funcionais.</p>"
    assert "NOTE.DUP_ANSWER_WARN" not in {d.code for d in check_notes(notes, answer)}
    assert "NOTE.DUP_ANSWER_WARN" in {
        d.code for d in check_notes("<b>zona epileptogênica</b>", answer)
    }


def test_rich_html_keeps_forbidden_tags_and_unbalanced_blocks_detectable():
    from anki_mcp.core.sanitize import sanitize_field
    from anki_mcp.core.vocab import is_allowed_tag

    assert not is_allowed_tag("script", is_images_field=False)
    assert not is_allowed_tag("img", is_images_field=False)
    clean, _ = sanitize_field("<script>alert(1)</script><p>Texto</p>")
    assert "<script>" not in clean
    assert "<p>Texto</p>" in clean
    csv_text = "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas\nNa avaliação clínica, o achado é {{c1::epilepsia}}.|<p>Sem fechamento|Medicina::Neurologia||Fonte|Neurologia"
    assert any(d.code == "FMT.TAG_UNCLOSED" for d in check_csv(csv_text, "CL"))
