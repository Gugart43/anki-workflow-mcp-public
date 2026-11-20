"""Byte-for-byte Python/Rust draft compiler parity on synthetic examples."""

from __future__ import annotations

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

try:
    from tests.differential.conftest import OracleRunner
except ImportError:
    from conftest import OracleRunner


def _split_csv_rows(csv_text: str) -> list[list[str]]:
    return [line.split("|") for line in csv_text.splitlines()]


def _has_evolved_notes_html(drafts: list[dict]) -> list[str]:
    """Return expected Phase 2 semantic HTML snippets when drafts exercise upgraded notes rendering."""
    markers: list[str] = []
    for d in drafts:
        notas = d.get("notas") or d.get("notas_adicionais")
        if d.get("tabela_notas") or d.get("tabelas_notas"):
            markers.append('<table class="anki-table">')
        if isinstance(notas, str) and "|---|" in notas:
            markers.append('<table class="anki-table">')
        if isinstance(notas, dict):
            if any(
                notas.get(k)
                for k in (
                    "fisiopatologia",
                    "raciocinio_clinico",
                    "armadilhas_diferenciais",
                    "correlacao_visual",
                )
            ):
                markers.append('<dl class="anki-kv">')
            if notas.get("tabela"):
                markers.append('<table class="anki-table">')
            if notas.get("texto_livre"):
                markers.append("<p>")
            for blk in notas.get("blocos") or []:
                btype = blk.get("tipo")
                if btype == "paragrafo":
                    markers.append("<p>")
                elif btype == "lista":
                    markers.append('<section class="anki-list"><ul>')
                elif btype == "tabela":
                    markers.append('<table class="anki-table">')
                elif btype == "alerta":
                    markers.append('<blockquote class="anki-callout anki-callout--trap">')
                elif btype == "fluxograma":
                    markers.append('<section class="anki-steps">')
    return markers


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
def test_compile_draft_parity(drafts: list[dict], options: dict, run_oracle: OracleRunner) -> None:
    normalized = [CardDraft.model_validate(d) for d in drafts]
    qa, cl, stats = compile_drafts_to_csv(normalized, **options)
    rs = run_oracle("compile_drafts", drafts=drafts, **options)
    expected_markers = _has_evolved_notes_html(drafts)
    if not expected_markers:
        assert rs == {
            "qa_csv": qa,
            "cloze_csv": cl,
            "stats": stats,
        }
        return

    assert rs["stats"] == stats
    rs_qa = _split_csv_rows(rs["qa_csv"])
    py_qa = _split_csv_rows(qa)
    assert len(rs_qa) == len(py_qa)
    for r_idx, (r_row, p_row) in enumerate(zip(rs_qa, py_qa, strict=True)):
        assert len(r_row) == len(p_row) == 7
        for c_idx in (0, 1, 2, 4, 5, 6):
            assert r_row[c_idx] == p_row[c_idx]
        if r_idx == 0:
            assert r_row[3] == p_row[3]
        else:
            for marker in expected_markers:
                assert marker in r_row[3]

    rs_cl = _split_csv_rows(rs["cloze_csv"])
    py_cl = _split_csv_rows(cl)
    assert len(rs_cl) == len(py_cl)
    for r_idx, (r_row, p_row) in enumerate(zip(rs_cl, py_cl, strict=True)):
        assert len(r_row) == len(p_row) == 6
        for c_idx in (0, 2, 3, 4, 5):
            assert r_row[c_idx] == p_row[c_idx]
        if r_idx == 0:
            assert r_row[1] == p_row[1]
        else:
            for marker in expected_markers:
                assert marker in r_row[1]


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
def test_patch_parity(updates: dict, draft: dict, run_oracle: OracleRunner) -> None:
    card = CardDraft.model_validate(draft)
    expected = apply_patch_to_draft(card, CardPatch(updates=updates)).model_dump(mode="json")
    rs = run_oracle("patch_draft", draft=draft, updates=updates)
    if updates == {"tags": "Neurologia::Tema"}:
        # Wave 2 explicitly canonicalizes alias updates (`tags` -> `etiquetas`) instead of
        # silently retaining `etiquetas: []` like legacy Python `CardDraft(**model_dump())`.
        assert rs["card"]["etiquetas"] == "Neurologia::Tema"
        expected_copy = dict(expected)
        expected_copy["etiquetas"] = "Neurologia::Tema"
        assert rs == {"card": expected_copy}
    else:
        assert rs == {"card": expected}


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
def test_patch_rejects_invalid_nested_notes_like_python(
    notes: dict, run_oracle: OracleRunner
) -> None:
    draft = {"tipo": "QA", "enunciado_ou_sentenca": "Q", "resposta": "R"}
    with pytest.raises(ValueError):
        apply_patch_to_draft(
            CardDraft.model_validate(draft), CardPatch(updates={"notas_adicionais": notes})
        )
    result = run_oracle.run_raw_process(
        [{"op": "patch_draft", "draft": draft, "updates": {"notas_adicionais": notes}}],
        check=False,
    )
    assert result.returncode != 0


def test_quarantine_partition_parity(run_oracle: OracleRunner) -> None:
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
        run_oracle(
            "partition_quarantine",
            drafts=drafts,
            failed_indices=[1, 1, 8],
            diagnostics=diagnostics,
            reason="bad",
        )
        == expected
    )
