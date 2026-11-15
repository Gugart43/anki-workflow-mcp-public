"""Byte-for-byte Python/Rust draft compiler parity on synthetic examples."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
from functools import cache
from pathlib import Path

import pytest

from anki_mcp.core.compiler import (
    apply_patch_to_draft,
    compile_drafts_to_csv,
    partition_quarantine,
)
from anki_mcp.models.draft import CardDraft, CardPatch

ROOT = Path(__file__).resolve().parents[2]
SOURCE = {"fonte": "Demo.pdf", "pagina": 2, "trecho": "Example."}


@cache
def _binary() -> Path:
    if os.environ.get("ANKI_SKIP_RUST_ORACLE") == "1" or shutil.which("cargo") is None:
        pytest.skip("Rust oracle unavailable")
    subprocess.run(["cargo", "build", "-p", "anki-workflow-mcp"], cwd=ROOT, check=True)
    binary = (
        ROOT
        / "target"
        / "debug"
        / ("anki-workflow-mcp.exe" if os.name == "nt" else "anki-workflow-mcp")
    )
    assert binary.is_file()
    return binary


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
    ("drafts", "options"),
    [
        ([], {}),
        ([{"tipo": "QA", "enunciado_ou_sentenca": "Q", "resposta": "R"}], {}),
        (
            [
                {
                    "tipo": "CLOZE",
                    "enunciado_ou_sentenca": "{{c4::uno}} + {{c8::dos}}",
                    "etiquetas": ["Medicina::Tema::Alvo"],
                }
            ],
            {},
        ),
        (
            [
                {
                    "tipo": "QA",
                    "id": "B01-QA-01",
                    "pergunta": "B01-QA-01 A | B?",
                    "resposta": "R > 4",
                    "tags": ["Cardio::Tema", "x y"],
                    "fonte": ["Ref 1", "Ref 2"],
                }
            ],
            {},
        ),
        (
            [
                {
                    "tipo": "CLOZE",
                    "sentenca": "{{c9::á}}",
                    "notas": "| A | B |\n|---|---|\n| x | y |",
                    "images": {"filename": "a.png", "caption": "Caption"},
                }
            ],
            {},
        ),
        (
            [
                {
                    "tipo": "QA",
                    "enunciado_ou_sentenca": "Sem default",
                    "resposta": "ok",
                    "etiquetas": "Geral",
                }
            ],
            {"default_campo_estudo": "Medicina::Geral"},
        ),
        (
            [{"tipo": "QA", "enunciado_ou_sentenca": "B01-QA-01 A", "resposta": "B02-CL-03 B"}],
            {"auto_purge_id_leaks": False},
        ),
        ([{"tipo": "CLOZE", "enunciado_ou_sentenca": "{{c5::x}}"}], {"auto_renumber_cloze": False}),
        (
            [
                {
                    "tipo": "QA",
                    "enunciado_ou_sentenca": "Tabela",
                    "resposta": "ok",
                    "tabela_notas": {"caption": "C", "headers": ["A", "B"], "rows": [["1", "2"]]},
                }
            ],
            {},
        ),
        (
            [
                {
                    "tipo": "CLOZE",
                    "enunciado_ou_sentenca": "{{c1::x}}",
                    "figura": {"filename": "a.png", "caption": "A"},
                    "figuras": [
                        {"filename": "a.png", "caption": "B"},
                        {"filename": "b.png", "caption": "C"},
                    ],
                }
            ],
            {},
        ),
        (
            [
                {
                    "tipo": "QA",
                    "enunciado_ou_sentenca": "Notes",
                    "resposta": "yes",
                    "notas_adicionais": {
                        "fisiopatologia": "Motivo",
                        "raciocinio_clinico": "Raciocínio",
                        "texto_livre": "Livre",
                    },
                }
            ],
            {},
        ),
        (
            [
                {
                    "tipo": "CLOZE",
                    "enunciado_ou_sentenca": "{{c2::x}}",
                    "notas_adicionais": {
                        "blocos": [
                            {"tipo": "paragrafo", "texto": "A & B", "fontes": [SOURCE]},
                            {"tipo": "lista", "itens": ["um", "dois"], "fontes": [SOURCE]},
                        ]
                    },
                    "referencias": ["Outra"],
                }
            ],
            {},
        ),
        (
            [
                {
                    "tipo": "QA",
                    "enunciado_ou_sentenca": "Fluxo",
                    "resposta": "ok",
                    "notas_adicionais": {
                        "blocos": [
                            {
                                "tipo": "fluxograma",
                                "titulo": "Mapa",
                                "fontes": [SOURCE],
                                "fluxograma": {
                                    "inicio": "a",
                                    "nos": [
                                        {"id": "a", "texto": "Início"},
                                        {"id": "b", "texto": "Fim"},
                                    ],
                                    "arestas": [{"origem": "a", "destino": "b", "rotulo": "Sim"}],
                                },
                            }
                        ]
                    },
                }
            ],
            {},
        ),
        (
            [
                {
                    "tipo": "QA",
                    "enunciado_ou_sentenca": "Q",
                    "resposta": "R",
                    "images": "<figure class='anki-figure'><img src='fig.png'><figcaption>Legenda</figcaption></figure>",
                }
            ],
            {},
        ),
        (
            [
                {
                    "tipo": "QA",
                    "enunciado_ou_sentenca": "Q",
                    "resposta": "R",
                    "notas_adicionais": {
                        "tabela": {"headers": ["A"], "rows": [["B"]]},
                        "blocos": [
                            {
                                "tipo": "tabela",
                                "tabela": {"headers": ["X"], "rows": [["Y"]]},
                                "fontes": [SOURCE],
                            }
                        ],
                    },
                }
            ],
            {},
        ),
        (
            [
                {
                    "tipo": "QA",
                    "enunciado_ou_sentenca": "Q",
                    "resposta": "R",
                    "notas_adicionais": {
                        "blocos": [
                            {
                                "tipo": "alerta",
                                "titulo": "Aviso",
                                "texto": "A | B",
                                "fontes": [SOURCE],
                            },
                            {
                                "tipo": "fluxograma",
                                "fontes": [SOURCE],
                                "fluxograma": {
                                    "inicio": "a",
                                    "nos": [{"id": "a", "texto": "A"}],
                                    "arestas": [],
                                },
                            },
                        ]
                    },
                }
            ],
            {},
        ),
        (
            [
                {
                    "tipo": "QA",
                    "enunciado_ou_sentenca": "Q",
                    "resposta": "R",
                    "notas_adicionais": {
                        "blocos": [
                            {
                                "tipo": "paragrafo",
                                "texto": "T",
                                "fontes": [
                                    {"fonte": " Demo.pdf ", "pagina": 2, "trecho": " Example. "}
                                ],
                            }
                        ]
                    },
                }
            ],
            {},
        ),
        (
            [
                {
                    "tipo": "QA",
                    "enunciado_ou_sentenca": "Q",
                    "resposta": "R",
                    "figura": {"filename": "x.png", "caption": "Legenda", "ignored": 42},
                }
            ],
            {},
        ),
        (
            [
                {
                    "tipo": "QA",
                    "enunciado_ou_sentenca": "Q",
                    "resposta": "R",
                    "figura": {"filename": "  "},
                }
            ],
            {},
        ),
    ],
)
def test_compile_draft_parity(drafts: list[dict], options: dict) -> None:
    normalized = [CardDraft.model_validate(d) for d in drafts]
    qa, cl, stats = compile_drafts_to_csv(normalized, **options)
    assert _oracle("compile_drafts", drafts=drafts, **options) == {
        "qa_csv": qa,
        "cloze_csv": cl,
        "stats": stats,
    }


@pytest.mark.parametrize(
    ("updates", "draft"),
    [
        ({"resposta": "Novo"}, {"tipo": "QA", "enunciado_ou_sentenca": "Q", "resposta": "R"}),
        (
            {"notas_adicionais": {"texto_livre": "A & B"}},
            {"tipo": "QA", "enunciado_ou_sentenca": "Q", "resposta": "R"},
        ),
        ({"tags": "Neurologia::Tema"}, {"note_type": "CL", "sentenca": "{{c1::x}}"}),
        (
            {
                "notas_adicionais": {
                    "blocos": [{"tipo": "lista", "itens": ["item"], "fontes": [SOURCE]}]
                }
            },
            {"tipo": "QA", "enunciado_ou_sentenca": "Q", "resposta": "R"},
        ),
        (
            {
                "notas_adicionais": {
                    "blocos": [
                        {
                            "tipo": "paragrafo",
                            "texto": "T",
                            "fontes": [
                                {"fonte": " Demo.pdf ", "pagina": 2, "trecho": " Example. "}
                            ],
                        }
                    ]
                }
            },
            {"tipo": "QA", "enunciado_ou_sentenca": "Q", "resposta": "R"},
        ),
        (
            {"figuras": [{"filename": "x.png"}]},
            {"tipo": "QA", "enunciado_ou_sentenca": "Q", "resposta": "R"},
        ),
        (
            {"tabela_notas": {"headers": ["A"], "rows": [["B"]], "ignored": "extra"}},
            {"tipo": "QA", "enunciado_ou_sentenca": "Q", "resposta": "R"},
        ),
        (
            {"tabela_notas": {"headers": ["A"], "rows": [["B"]]}},
            {"tipo": "QA", "enunciado_ou_sentenca": "Q", "resposta": "R"},
        ),
    ],
)
def test_patch_parity(updates: dict, draft: dict) -> None:
    card = CardDraft.model_validate(draft)
    expected = apply_patch_to_draft(card, CardPatch(updates=updates)).model_dump(mode="json")
    assert _oracle("patch_draft", draft=draft, updates=updates) == {"card": expected}


@pytest.mark.parametrize(
    "notes",
    [
        {"blocos": [{"tipo": "paragrafo", "texto": "T", "fontes": []}]},
        {"blocos": [{"tipo": "lista", "texto": "T", "fontes": [SOURCE]}]},
        {
            "blocos": [
                {
                    "tipo": "fluxograma",
                    "fontes": [SOURCE],
                    "fluxograma": {
                        "inicio": "a",
                        "nos": [{"id": "a", "texto": "A"}, {"id": "b", "texto": "B"}],
                        "arestas": [],
                    },
                }
            ]
        },
    ],
)
def test_patch_rejects_invalid_nested_notes_like_python(notes: dict) -> None:
    draft = {"tipo": "QA", "enunciado_ou_sentenca": "Q", "resposta": "R"}
    with pytest.raises(ValueError):
        apply_patch_to_draft(
            CardDraft.model_validate(draft), CardPatch(updates={"notas_adicionais": notes})
        )
    result = subprocess.run(
        [str(_binary()), "oracle"],
        input=json.dumps(
            [{"op": "patch_draft", "draft": draft, "updates": {"notas_adicionais": notes}}]
        ),
        text=True,
        encoding="utf-8",
        capture_output=True,
        check=False,
    )
    assert result.returncode != 0


def test_quarantine_partition_parity() -> None:
    drafts = [
        {"tipo": "QA", "enunciado_ou_sentenca": f"Q{i}", "resposta": f"R{i}"} for i in range(3)
    ]
    diagnostics = [{"row": 2, "code": "X"}, {"row": 3, "code": "Y"}]
    valid, quarantined = partition_quarantine(
        [CardDraft.model_validate(d) for d in drafts], [1, 1, 8], diagnostics, "bad"
    )
    expected = {
        "valid": [c.model_dump(mode="json") for c in valid],
        "quarantined": [c.model_dump(mode="json") for c in quarantined],
    }
    assert (
        _oracle(
            "partition_quarantine",
            drafts=drafts,
            failed_indices=[1, 1, 8],
            diagnostics=diagnostics,
            reason="bad",
        )
        == expected
    )
