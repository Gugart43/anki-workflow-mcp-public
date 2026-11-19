"""F3-07 pure draft helpers against the Python legacy oracle (no clinical data)."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

import pytest

from anki_mcp.core.compiler import _infer_campo_estudo, purge_id_leak
from anki_mcp.core.note_blocks import FlowEdge, FlowNode, NoteFlow
from anki_mcp.models.draft import CardDraft, MediaFigure

ROOT = Path(__file__).resolve().parents[2]

try:
    from tests.differential.conftest import resolve_rust_binary
except ImportError:
    from conftest import resolve_rust_binary

_binary = resolve_rust_binary


def _oracle(op: str, **kwargs: object) -> dict:
    result = subprocess.run(
        [str(_binary()), "oracle"],
        input=json.dumps([{"op": op, **kwargs}], ensure_ascii=False),
        text=True,
        encoding="utf-8",
        capture_output=True,
        check=True,
    )
    return json.loads(result.stdout)[0]


@pytest.mark.parametrize(
    "text",
    [
        "",
        "B01-QA-01 questão",
        "prefixo OF-B01-001 fim",
        "B03-CL-099 e B04-CLOZE-012",
        "Nada B01-QA-01 junto B01-QA-02",
        "xB01-QA-01y",
        "B01-QA-999   espaços",
    ],
)
def test_purge_id_leak(text: str) -> None:
    assert _oracle("purge_id_leak", text=text) == {"text": purge_id_leak(text)}


@pytest.mark.parametrize(
    "tags",
    [
        [],
        "",
        ["#Medicina::Neurologia::Epilepsia"],
        ["id::B01-QA-001", "Cardio::Tema"],
        "foco::abc FocoClinico::X tag::Y",
        ["Cardio::Arritmia::Sub", "Medicina::Outra"],
        ["único", "Medicina::Neuro_logia::Tópico_nome"],
        "#Neurologia::Cefaleia #Medicina::Outra",
    ],
)
def test_infer_campo(tags: list[str] | str) -> None:
    assert _oracle("infer_campo_estudo", etiquetas=tags) == {"text": _infer_campo_estudo(tags)}


@pytest.mark.parametrize(
    "sentence",
    [
        "",
        "Texto sem lacuna",
        "{{c3::alpha}} e {{c7::beta::h}}",
        "{{c2::a}} {{c2::b}} {{c5::c}}",
        "{{c10::\\frac{a}{b}::fração}}",
        "Pré {{c4::ação}} após {{c1::termo}}",
        "{{c01::A}} e {{c2::B}}",
    ],
)
def test_renumber_cloze(sentence: str) -> None:
    card = CardDraft(tipo="CLOZE", enunciado_ou_sentenca=sentence)
    card.renumber_cloze()
    assert _oracle("renumber_cloze", text=sentence) == {"text": card.enunciado_ou_sentenca}


@pytest.mark.parametrize(
    "figure",
    [
        {"filename": ""},
        {"filename": " f.png ", "caption": "Achado & explicação"},
        {
            "filename": "t.png",
            "title": "Título",
            "caption": "Padrão",
            "context_link": "Contexto",
            "item_type": "table",
        },
        {
            "filename": "a.png",
            "title": "Igual",
            "caption": "Igual <b>forte</b>",
            "style": "clinical",
        },
        {"filename": "x.png", "caption": "Teste", "style": "table", "caption_style": "color: red"},
        {"filename": "x.png", "caption": "<b>1</b> & <i>2</i>", "style": "custom"},
    ],
)
def test_figure_html(figure: dict) -> None:
    assert _oracle("render_media_figure", figure=figure) == {
        "text": MediaFigure(**figure).render_html()
    }


@pytest.mark.parametrize(
    ("nodes", "edges", "start"),
    [
        ([{"id": "A", "texto": "Início"}], [], "A"),
        (
            [{"id": "B", "texto": "Segundo"}, {"id": "A", "texto": "Começo"}],
            [{"origem": "A", "destino": "B", "rotulo": "Sim"}],
            "A",
        ),
        (
            [{"id": "A", "texto": "<alto>"}, {"id": "B", "texto": "baixo"}],
            [
                {"origem": "A", "destino": "B", "rotulo": "A & B"},
                {"origem": "B", "destino": "A", "rotulo": "voltar"},
            ],
            "A",
        ),
    ],
)
def test_note_flow_html(nodes: list[dict], edges: list[dict], start: str) -> None:
    flow = NoteFlow(
        inicio=start, nos=[FlowNode(**n) for n in nodes], arestas=[FlowEdge(**e) for e in edges]
    )
    assert _oracle("render_note_flow", flow={"inicio": start, "nos": nodes, "arestas": edges}) == {
        "text": flow.render_html()
    }
