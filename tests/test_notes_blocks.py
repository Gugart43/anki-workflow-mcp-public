"""Structured supplementary notes survive schema, compiler and CSV validation."""

import pytest
from pydantic import ValidationError

from anki_mcp.checks.csv_structure import check_csv
from anki_mcp.core.compiler import compile_drafts_to_csv
from anki_mcp.models.draft import CardDraft, ExtraNotes

SOURCE = {"fonte": "Anexo.pdf", "pagina": 12, "trecho": "Texto literal de apoio."}


def test_multiple_notes_blocks_are_not_silently_discarded():
    notes = ExtraNotes.model_validate(
        {
            "blocos": [
                {
                    "tipo": "paragrafo",
                    "titulo": "Entenda",
                    "texto": "Contexto < 5 e A | B.",
                    "fontes": [SOURCE],
                },
                {"tipo": "lista", "itens": ["Primeiro ponto", "Segundo ponto"], "fontes": [SOURCE]},
                {"tipo": "alerta", "texto": "A ressalva importa.", "fontes": [SOURCE]},
                {
                    "tipo": "tabela",
                    "tabela": {"headers": ["A", "B"], "rows": [["1", "2"]]},
                    "fontes": [SOURCE],
                },
            ]
        }
    )
    rendered = notes.render_html()
    assert "<b>Entenda</b>" in rendered
    assert "Contexto &lt; 5 e A &#124; B." in rendered
    assert "<ul><li>Primeiro ponto</li>" in rendered
    assert "<blockquote>" in rendered
    assert "<table>" in rendered
    assert "Texto literal de apoio." not in rendered  # audit evidence isn't study prose
    draft = CardDraft(
        tipo="CLOZE",
        enunciado_ou_sentenca="O conceito avaliado é {{c1::unitário}}.",
        notas_adicionais=notes,
    )
    _, csv_text, _ = compile_drafts_to_csv([draft])
    assert "Anexo.pdf, p. PDF 12." in csv_text
    assert not [d for d in check_csv(csv_text, "CL") if d.severity.value == "CRITICAL"]


def test_flowchart_preserves_branches_without_javascript_or_external_media():
    notes = ExtraNotes.model_validate(
        {
            "blocos": [
                {
                    "tipo": "fluxograma",
                    "titulo": "Conduta",
                    "fontes": [SOURCE],
                    "fluxograma": {
                        "inicio": "avaliar",
                        "nos": [
                            {"id": "avaliar", "texto": "Reavaliar resposta"},
                            {"id": "trocar", "texto": "Trocar esquema"},
                            {"id": "manter", "texto": "Manter esquema"},
                        ],
                        "arestas": [
                            {"origem": "avaliar", "destino": "trocar", "rotulo": "Intolerância"},
                            {"origem": "avaliar", "destino": "manter", "rotulo": "Boa resposta"},
                        ],
                    },
                }
            ]
        }
    )
    rendered = notes.render_html()
    assert "<ol>" in rendered and "<ul>" in rendered
    assert "Intolerância → 2. Trocar esquema" in rendered
    assert "Boa resposta → 3. Manter esquema" in rendered
    assert "<script" not in rendered and "<svg" not in rendered and "<img" not in rendered


@pytest.mark.parametrize("change", ["dangling", "unreachable", "duplicate", "bad_start"])
def test_flow_rejects_lost_or_unreachable_decisions(change):
    from anki_mcp.core.note_blocks import NoteFlow

    graph = {
        "inicio": "a",
        "nos": [{"id": "a", "texto": "A"}, {"id": "b", "texto": "B"}],
        "arestas": [{"origem": "a", "destino": "b", "rotulo": "Se indicado"}],
    }
    if change == "dangling":
        graph["arestas"][0]["destino"] = "missing"
    elif change == "unreachable":
        graph["arestas"] = []
    elif change == "duplicate":
        graph["nos"][1]["id"] = "a"
    else:
        graph["inicio"] = "missing"
    with pytest.raises(ValidationError):
        NoteFlow.model_validate(graph)


@pytest.mark.parametrize(
    "block",
    [
        {"tipo": "paragrafo", "texto": ""},
        {"tipo": "lista", "itens": []},
        {"tipo": "tabela"},
        {"tipo": "paragrafo", "texto": "Texto", "unknown": "Not silently ignored"},
    ],
)
def test_invalid_blocks_fail_explicitly(block):
    from anki_mcp.core.note_blocks import NoteBlock

    with pytest.raises(ValidationError):
        NoteBlock.model_validate({**block, "fontes": [SOURCE]})


def test_block_evidence_is_required_and_does_not_claim_automatic_verification():
    from anki_mcp.core.note_blocks import NoteBlock

    with pytest.raises(ValidationError):
        NoteBlock.model_validate({"tipo": "paragrafo", "texto": "Sem fonte"})
    note = ExtraNotes(
        texto_livre="Legado preservado.",
        blocos=[{"tipo": "paragrafo", "texto": "Contexto", "fontes": [SOURCE, SOURCE]}],
    )
    assert note.source_references() == ["Anexo.pdf, p. PDF 12."]
    assert "Legado preservado." in note.render_html()


def test_compiler_counts_tables_from_multiple_blocks():
    notes = ExtraNotes(
        blocos=[
            {
                "tipo": "tabela",
                "tabela": {"headers": ["A", "B"], "rows": [["1", "2"]]},
                "fontes": [SOURCE],
            }
        ]
    )
    card = CardDraft(
        tipo="CLOZE", enunciado_ou_sentenca="Uma {{c1::resposta}}.", notas_adicionais=notes
    )
    _, _, stats = compile_drafts_to_csv([card])
    assert stats["tables_rendered"] == 1


def test_block_reference_is_not_lost_when_it_is_a_suffix_of_another_reference():
    source = {**SOURCE, "fonte": "Anexo.pdf"}
    card = CardDraft(
        tipo="CLOZE",
        enunciado_ou_sentenca="Uma {{c1::resposta}}.",
        referencias=["Outro Anexo.pdf, p. PDF 12."],
        notas_adicionais=ExtraNotes(
            blocos=[{"tipo": "paragrafo", "texto": "Contexto", "fontes": [source]}]
        ),
    )
    _, csv_text, _ = compile_drafts_to_csv([card])
    refs = csv_text.splitlines()[1].split("|")[4]
    assert refs == "Outro Anexo.pdf, p. PDF 12. &#124; Anexo.pdf, p. PDF 12."


@pytest.mark.parametrize("field", ["fonte", "trecho"])
def test_evidence_cannot_be_only_whitespace(field):
    from anki_mcp.core.note_blocks import NoteSource

    with pytest.raises(ValidationError):
        NoteSource.model_validate({**SOURCE, field: "   "})
