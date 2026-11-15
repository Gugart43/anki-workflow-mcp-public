"""Differential checks for the next five pure Rust modules against Python legacy."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
from functools import cache
from pathlib import Path

import pytest

from anki_mcp.checks.crossrefs import check_crossrefs
from anki_mcp.checks.images import check_images_field
from anki_mcp.checks.tables import check_tables_field
from anki_mcp.checks.tags import check_tags
from anki_mcp.core.tables import escape_cell, markdown_tables_to_html

ROOT = Path(__file__).resolve().parents[2]


@cache
def _rust_binary() -> Path:
    exe = "anki-workflow-mcp.exe" if os.name == "nt" else "anki-workflow-mcp"
    path = ROOT / "target" / "debug" / exe
    if os.environ.get("ANKI_SKIP_RUST_ORACLE") == "1":
        pytest.skip("ANKI_SKIP_RUST_ORACLE=1")
    if shutil.which("cargo") is None:
        pytest.skip("Rust cargo unavailable in this environment")
    # Build once even if a previous target exists: an old oracle would reject
    # new operations and produce misleading parity failures.
    subprocess.run(["cargo", "build", "-p", "anki-workflow-mcp"], cwd=ROOT, check=True)
    assert path.is_file(), "Rust oracle did not produce an executable"
    return path


def _oracle(op: str, **kwargs: object) -> dict:
    result = subprocess.run(
        [str(_rust_binary()), "oracle"],
        input=json.dumps([{"op": op, **kwargs}], ensure_ascii=False),
        text=True,
        encoding="utf-8",
        capture_output=True,
        check=True,
    )
    return json.loads(result.stdout)[0]


def _diagnostics(items: list) -> dict:
    return {"diagnostics": [item.model_dump(mode="json") for item in items]}


@pytest.mark.parametrize(
    ("text", "taxonomy"),
    [
        ("", None),
        ("id::B01-QA-001 sistema::estudo::alerta", None),
        ("sistema::estudo::alerta desconhecida", ["sistema::estudo::alerta"]),
        ("a::b::c::d::e xyz-ruim café", []),
        ("A::B á-foo", None),
    ],
)
def test_tags_parity(text: str, taxonomy: list[str] | None) -> None:
    expected = check_tags(
        text, set(taxonomy) if taxonomy is not None else None, row=3, field="Etiquetas"
    )
    assert _oracle(
        "check_tags", tags_field=text, taxonomy_nodes=taxonomy, row=3, field="Etiquetas"
    ) == _diagnostics(expected)


@pytest.mark.parametrize(
    ("text", "active", "current"),
    [
        ("Nothing here", None, None),
        ("[B01-QA-001] (B02-CL-003a)", None, None),
        ("B01-QA-001 B01-QA-001 B02-CL-003a", ["B01-QA-001"], []),
        ("(B02-CL-003a) and B01-QA-001", [], ["B02-CL-003a"]),
        ("xB01-QA-001y B99-CL-999Z", [], []),
    ],
)
def test_crossrefs_parity(text: str, active: list[str] | None, current: list[str] | None) -> None:
    expected = check_crossrefs(
        text,
        set(active) if active is not None else None,
        set(current) if current is not None else None,
        row=5,
        field="Referências",
    )
    assert _oracle(
        "check_crossrefs",
        text=text,
        active_ids=active,
        current_batch_ids=current,
        row=5,
        field="Referências",
    ) == _diagnostics(expected)


@pytest.mark.parametrize(
    ("html", "catalog"),
    [
        ("", None),
        ('<img src="tema_x.png" style="color:red">', None),
        (
            '<figure class="anki-figure"><img src="tema_x.png" alt="X"><figcaption>Achado visual descritivo e contextual.</figcaption></figure>',
            ["tema_x.png"],
        ),
        ('<figure><img src="Tema_X.png"><figcaption>Figura 1</figcaption></figure>', ["outro.png"]),
        (
            '<figure style="width:1px"><img src="tema_x.png" alt="X"><figcaption></figcaption></figure>'
            * 2,
            None,
        ),
        (
            '<figure><img src="tema_x.png" alt="X"><figcaption>Descrição longa e concreta.</figcaption></figure>'
            * 4,
            None,
        ),
    ],
)
def test_images_parity(html: str, catalog: list[str] | None) -> None:
    expected = check_images_field(html, set(catalog) if catalog is not None else None, row=9)
    assert _oracle(
        "check_images_field", html=html, catalog_filenames=catalog, row=9
    ) == _diagnostics(expected)


@pytest.mark.parametrize(
    ("html", "face"),
    [
        ("", False),
        ("<table><tr><th>Nome</th><th>Valor</th></tr><tr><td>A</td><td>B</td></tr></table>", False),
        ("<table style='x'><tr><td>1</td></tr><tr><td>2</td><td>3</td></tr></table>", True),
        ("<table><tr><td>esquerda</td></tr></table>", False),
        ("<table><tr><td>sem fechar</td></tr>", False),
        ("| A | B |\n|---|---|\n| 1 | 2 |", False),
        ("<table><tr><td><table><tr><td>x</td></tr></table></td></tr></table>", False),
    ],
)
def test_tables_parity(html: str, face: bool) -> None:
    expected = check_tables_field(html, field="Notas Adicionais", row=2, face=face)
    assert _oracle(
        "check_tables_field", html=html, field="Notas Adicionais", row=2, face=face
    ) == _diagnostics(expected)


@pytest.mark.parametrize(
    "text",
    [
        "",
        "A & B | C",
        "<b>sim</b> <img> &amp;",
        "<br/> &le; <sub>2</sub> &#124;",
        "Água < 10 &bogus; &",
    ],
)
def test_table_cell_parity(text: str) -> None:
    assert _oracle("escape_cell", text=text) == {"text": escape_cell(text)}


@pytest.mark.parametrize(
    "text",
    [
        "",
        "A|B\n---|---\n1|2\n",
        "Texto simples",
        "A&#124;B<br>---&#124;---<br>x&#124;y",
        "A|B\n:-|--:\n1|2|3\n",
    ],
)
def test_markdown_tables_parity(text: str) -> None:
    result, count = markdown_tables_to_html(text)
    assert _oracle("markdown_tables_to_html", text=text) == {"text": result, "count": count}
