"""tests/test_tables.py — Comprehensive tests for table models, checks, and tools."""

from __future__ import annotations

import asyncio
import json

import pytest

from anki_mcp.checks.tables import (
    MAX_BODY_ROWS,
    MAX_CELL_CHARS,
    MAX_COLS,
    check_tables_field,
)
from anki_mcp.core.tables import TableSpec, escape_cell, markdown_tables_to_html
from anki_mcp.server import mcp

QA_H = (
    "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas"
)
CL_H = "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas"
QA_OK = (
    QA_H
    + "\nQual o tratamento da ELA?|Riluzol|Medicina::Neurologia::ELA|||Kiernan 2011|ELA::Tratamento"
)
CL_OK = (
    CL_H
    + "\nO riluzol reduz a progressão da {{c1::ELA}} em média.|Fonte: Kiernan 2011|Medicina::Neurologia::ELA|||ELA::Tratamento"
)


def _call_tool(name: str, **kwargs):
    return asyncio.run(mcp.get_tool(name)).fn(**kwargs)


# ── 1. TableSpec & Escaping Tests ────────────────────────────────────────────


def test_tablespec_render_structure():
    spec = TableSpec(
        caption="Comparação Clínica",
        headers=["Droga", "Indicação"],
        rows=[["Riluzol", "ELA"], ["Edaravone", "ELA inicial"]],
    )
    html = spec.render_html()
    assert "\n" not in html
    assert "style=" not in html
    assert "<caption>Comparação Clínica</caption>" in html
    assert "<thead><tr><th>Droga</th><th>Indicação</th></tr></thead>" in html
    assert (
        "<tbody><tr><td>Riluzol</td><td>ELA</td></tr>"
        "<tr><td>Edaravone</td><td>ELA inicial</td></tr></tbody>" in html
    )
    assert html.startswith("<table>") and html.endswith("</table>")


def test_tablespec_render_escaping_and_entities():
    # Direct escape_cell test
    assert escape_cell("a | b & <c>") == "a &#124; b &amp; &lt;c&gt;"
    assert escape_cell("<b>p &le; 0.05</b>") == "<b>p &le; 0.05</b>"

    spec = TableSpec(
        headers=["Condição", "Critério | Valores"],
        rows=[
            [
                "<b>Hipertensão</b>",
                "PA > 140 & < 160 | p &le; 0.05 e &amp; e &nbsp; e &#124;",
            ],
            [
                "<i>Água</i> e H<sub>2</sub>O",
                "Concentração <sup>131</sup>I & <script>alert(1)</script><br>Linha 2",
            ],
        ],
    )
    html = spec.render_html()
    # Pipes converted to &#124;
    assert "|" not in html
    assert "&#124;" in html
    # Allowed tags preserved
    assert "<b>Hipertensão</b>" in html
    assert "<i>Água</i>" in html
    assert "H<sub>2</sub>O" in html
    assert "<sup>131</sup>I" in html
    assert "<br>" in html
    # Inequalities and ampersands properly escaped
    assert "&gt; 140 &amp; &lt; 160" in html
    # Valid existing entities not double-escaped
    assert "p &le; 0.05" in html
    assert "&amp; e &nbsp;" in html
    # Dangerous tags stay escaped
    assert "&lt;script&gt;" in html
    assert "<script>" not in html


def test_tablespec_validation_ragged_raises_value_error():
    with pytest.raises(ValueError) as exc_info:
        TableSpec(headers=["A", "B"], rows=[["1", "2"], ["3"]])
    assert "Row 1 length 1 does not match headers length 2: ['3']" in str(exc_info.value)


def test_tablespec_validation_empty_headers_raises_value_error():
    with pytest.raises(ValueError) as exc_info:
        TableSpec(headers=[], rows=[])
    assert "headers must not be empty" in str(exc_info.value)


# ── 2. Markdown to HTML Conversion Tests ────────────────────────────────────


def test_markdown_tables_to_html_newline_separated():
    md = "Tabela:\n| Droga | Dose |\n| :--- | :---: |\n| Riluzol | 50 mg |\nFim."
    converted, count = markdown_tables_to_html(md)
    assert count == 1
    assert "Tabela:\n<table>" in converted
    assert "<thead><tr><th>Droga</th><th>Dose</th></tr></thead>" in converted
    assert "<tbody><tr><td>Riluzol</td><td>50 mg</td></tr></tbody></table>\nFim." in converted


def test_markdown_tables_to_html_br_separated():
    md = "Topo<br>| Exame | Referência |<br>| --- | --- |<br>| Hemoglobina | 12-16 |<br>Rodapé"
    converted, count = markdown_tables_to_html(md)
    assert count == 1
    assert "Topo<br><table>" in converted
    assert "<thead><tr><th>Exame</th><th>Referência</th></tr></thead>" in converted
    assert (
        "<tbody><tr><td>Hemoglobina</td><td>12-16</td></tr></tbody></table><br>Rodapé" in converted
    )


def test_markdown_tables_to_html_escaped_pipe():
    md = "&#124; Fármaco &#124; Classe &#124;<br>&#124; --- &#124; --- &#124;<br>&#124; Levodopa &#124; Dopaminérgico &#124;"
    converted, count = markdown_tables_to_html(md)
    assert count == 1
    assert "<table><thead><tr><th>Fármaco</th><th>Classe</th></tr></thead>" in converted
    assert "<tbody><tr><td>Levodopa</td><td>Dopaminérgico</td></tr></tbody></table>" in converted


def test_markdown_tables_to_html_ragged_normalization():
    # Header has 3 columns:
    # Row 1 has 2 columns -> padded with ""
    # Row 2 has 4 columns -> 4th joined into 3rd with space
    md = "| C1 | C2 | C3 |\n|---|---|---|\n| a | b |\n| x | y | z | extra |"
    converted, count = markdown_tables_to_html(md)
    assert count == 1
    assert "<tr><td>a</td><td>b</td><td></td></tr>" in converted
    assert "<tr><td>x</td><td>y</td><td>z extra</td></tr>" in converted


def test_markdown_tables_to_html_no_table():
    text = "Apenas texto clínico descritivo sem nenhuma tabela."
    converted, count = markdown_tables_to_html(text)
    assert count == 0
    assert converted == text


# ── 3. Diagnostic Codes Tests (One per code: positive and negative) ──────────


def test_diag_tbl_markdown_pipe():
    # Pos: standard markdown table
    pos1 = "| A | B |\n| --- | --- |\n| 1 | 2 |"
    d1 = check_tables_field(pos1)
    assert any(d.code == "TBL.MARKDOWN_PIPE" for d in d1)

    # Pos: 2 lines with >= 3 pipes
    pos2 = "| A | B | C |\n| 1 | 2 | 3 |"
    d2 = check_tables_field(pos2)
    assert any(d.code == "TBL.MARKDOWN_PIPE" for d in d2)

    # Neg: regular text or valid html table
    neg = "<table><thead><tr><th>A</th><th>B</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table>"
    d_neg = check_tables_field(neg)
    assert not any(d.code == "TBL.MARKDOWN_PIPE" for d in d_neg)


def test_diag_tbl_malformed():
    # Pos: unclosed table tag
    pos = "<table><thead><tr><th>A</th></tr></thead><tbody><tr><td>D</td></tr>"
    d_pos = check_tables_field(pos)
    assert any(d.code == "TBL.MALFORMED" for d in d_pos)

    # Neg: properly balanced table
    neg = "<table><thead><tr><th>A</th></tr></thead><tbody><tr><td>D</td></tr></tbody></table>"
    d_neg = check_tables_field(neg)
    assert not any(d.code == "TBL.MALFORMED" for d in d_neg)


def test_diag_tbl_nested():
    # Pos: table inside table
    pos = (
        "<table><thead><tr><th>A</th></tr></thead><tbody><tr><td>"
        "<table><thead><tr><th>Inner</th></tr></thead><tbody><tr><td>X</td></tr></tbody></table>"
        "</td></tr></tbody></table>"
    )
    d_pos = check_tables_field(pos)
    assert any(d.code == "TBL.NESTED" for d in d_pos)

    # Neg: single table
    neg = "<table><thead><tr><th>A</th></tr></thead><tbody><tr><td>X</td></tr></tbody></table>"
    d_neg = check_tables_field(neg)
    assert not any(d.code == "TBL.NESTED" for d in d_neg)


def test_diag_tbl_ragged():
    # Pos: inconsistent column count
    pos = (
        "<table><thead><tr><th>A</th><th>B</th></tr></thead>"
        "<tbody><tr><td>1</td></tr></tbody></table>"
    )
    d_pos = check_tables_field(pos)
    assert any(d.code == "TBL.RAGGED" for d in d_pos)

    # Neg: consistent column count (also with colspan)
    neg = (
        "<table><thead><tr><th>A</th><th>B</th></tr></thead>"
        "<tbody><tr><td colspan='2'>Total</td></tr></tbody></table>"
    )
    d_neg = check_tables_field(neg)
    assert not any(d.code == "TBL.RAGGED" for d in d_neg)


def test_diag_tbl_no_header():
    # Pos: no thead and first row has only td
    pos = "<table><tbody><tr><td>A</td><td>B</td></tr><tr><td>1</td><td>2</td></tr></tbody></table>"
    d_pos = check_tables_field(pos)
    assert any(d.code == "TBL.NO_HEADER" for d in d_pos)

    # Neg: has thead
    neg1 = "<table><thead><tr><th>A</th><th>B</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table>"
    assert not any(d.code == "TBL.NO_HEADER" for d in check_tables_field(neg1))

    # Neg: first row has th
    neg2 = (
        "<table><tbody><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></tbody></table>"
    )
    assert not any(d.code == "TBL.NO_HEADER" for d in check_tables_field(neg2))


def test_diag_tbl_too_wide():
    # Pos: > MAX_COLS columns
    headers = [f"Col{i}" for i in range(MAX_COLS + 1)]
    cells = [f"val{i}" for i in range(MAX_COLS + 1)]
    th_tags = "".join(f"<th>{h}</th>" for h in headers)
    td_tags = "".join(f"<td>{c}</td>" for c in cells)
    pos = f"<table><thead><tr>{th_tags}</tr></thead><tbody><tr>{td_tags}</tr></tbody></table>"
    d_pos = check_tables_field(pos)
    too_wide = [d for d in d_pos if d.code == "TBL.TOO_WIDE"]
    assert len(too_wide) == 1
    assert "dividir a tabela ou transpor" in (too_wide[0].suggestion or "").lower()

    # Neg: <= MAX_COLS columns
    headers_ok = [f"Col{i}" for i in range(MAX_COLS)]
    cells_ok = [f"val{i}" for i in range(MAX_COLS)]
    th_ok = "".join(f"<th>{h}</th>" for h in headers_ok)
    td_ok = "".join(f"<td>{c}</td>" for c in cells_ok)
    neg = f"<table><thead><tr>{th_ok}</tr></thead><tbody><tr>{td_ok}</tr></tbody></table>"
    assert not any(d.code == "TBL.TOO_WIDE" for d in check_tables_field(neg))


def test_diag_tbl_too_long():
    # Pos: > MAX_BODY_ROWS body rows
    body_rows = "".join(f"<tr><td>{i}</td></tr>" for i in range(1, MAX_BODY_ROWS + 2))
    pos = f"<table><thead><tr><th>Num</th></tr></thead><tbody>{body_rows}</tbody></table>"
    d_pos = check_tables_field(pos)
    assert any(d.code == "TBL.TOO_LONG" for d in d_pos)

    # Neg: MAX_BODY_ROWS body rows
    body_rows_ok = "".join(f"<tr><td>{i}</td></tr>" for i in range(1, MAX_BODY_ROWS + 1))
    neg = f"<table><thead><tr><th>Num</th></tr></thead><tbody>{body_rows_ok}</tbody></table>"
    assert not any(d.code == "TBL.TOO_LONG" for d in check_tables_field(neg))


def test_diag_tbl_cell_too_long():
    # Pos: cell with > 120 characters of visible text
    long_text = "A" * (MAX_CELL_CHARS + 1)
    pos = f"<table><thead><tr><th>H</th></tr></thead><tbody><tr><td>{long_text}</td></tr></tbody></table>"
    d_pos = check_tables_field(pos)
    assert any(d.code == "TBL.CELL_TOO_LONG" for d in d_pos)

    # Neg: cell <= 120 chars
    ok_text = "A" * MAX_CELL_CHARS
    neg = f"<table><thead><tr><th>H</th></tr></thead><tbody><tr><td>{ok_text}</td></tr></tbody></table>"
    assert not any(d.code == "TBL.CELL_TOO_LONG" for d in check_tables_field(neg))


def test_diag_tbl_inline_style():
    # Pos: style or width or bgcolor attribute
    pos1 = "<table style='color:red'><thead><tr><th>H</th></tr></thead><tbody><tr><td>D</td></tr></tbody></table>"
    pos2 = "<table><thead><tr><th>H</th></tr></thead><tbody><tr><td bgcolor='#fff'>D</td></tr></tbody></table>"
    assert any(d.code == "TBL.INLINE_STYLE" for d in check_tables_field(pos1))
    assert any(d.code == "TBL.INLINE_STYLE" for d in check_tables_field(pos2))

    # Neg: no inline presentation attributes
    neg = "<table><thead><tr><th>H</th></tr></thead><tbody><tr><td>D</td></tr></tbody></table>"
    assert not any(d.code == "TBL.INLINE_STYLE" for d in check_tables_field(neg))


def test_diag_tbl_in_card_face():
    table = "<table><thead><tr><th>H</th></tr></thead><tbody><tr><td>D</td></tr></tbody></table>"
    # Pos: face=True
    d_pos = check_tables_field(table, field="Enunciado", face=True)
    assert any(d.code == "TBL.IN_CARD_FACE" for d in d_pos)

    # Neg: face=False
    d_neg = check_tables_field(table, field="Notas Adicionais", face=False)
    assert not any(d.code == "TBL.IN_CARD_FACE" for d in d_neg)


def test_diag_empty_field():
    assert check_tables_field("") == []
    assert check_tables_field("   ") == []


# ── 4. MCP Tools Tests ───────────────────────────────────────────────────────


def test_tool_table_build_pass():
    spec = {
        "caption": "Estadiamento",
        "headers": ["Estágio", "Critério"],
        "rows": [["I", "Assintomático"], ["II", "Sintomas leves"]],
    }
    res = _call_tool("table_build", spec_json=json.dumps(spec))
    assert res["status"] == "PASS"
    assert "<table>" in res["html"]
    assert res["diagnostics"] == []


def test_tool_table_build_fail_on_critical():
    # spec with ragged rows (caught in validation)
    spec = {
        "headers": ["A", "B"],
        "rows": [["1"]],
    }
    res = _call_tool("table_build", spec_json=json.dumps(spec))
    assert res["status"] == "FAIL"
    assert any(d["code"] == "TBL.RAGGED" for d in res["diagnostics"])


def test_tool_table_from_markdown():
    text = "Exemplo:\n| H1 | H2 |\n| --- | --- |\n| A | B |"
    res = _call_tool("table_from_markdown", text=text)
    assert res["status"] == "PASS"
    assert res["tables_converted"] == 1
    assert "<table>" in res["text"]


def test_tool_validate_tables_field():
    bad_html = "<table><tr><td>1</td></tr></table>"
    res = _call_tool("validate_tables_field", html=bad_html, face=False)
    assert res["status"] == "PASS"  # TBL.NO_HEADER is WARN, so status is PASS
    assert any(d["code"] == "TBL.NO_HEADER" for d in res["diagnostics"])

    critical_html = "<table><tr><td>unclosed"
    res_crit = _call_tool("validate_tables_field", html=critical_html, face=False)
    assert res_crit["status"] == "FAIL"  # TBL.MALFORMED is CRITICAL
    assert any(d["code"] == "TBL.MALFORMED" for d in res_crit["diagnostics"])


# ── 5. Integration Tests: validate_batch ─────────────────────────────────────


def test_integration_validate_batch_detects_markdown_pipe():
    # QA row where Notas Adicionais contains markdown table with escaped pipes and <br>
    md_table_field = (
        "&#124; Medicamento &#124; Dose &#124;<br>"
        "&#124; --- &#124; --- &#124;<br>"
        "&#124; Riluzol &#124; 50 mg &#124;"
    )
    bad_qa = (
        QA_H
        + f"\nQual o tratamento da ELA?|Riluzol|Medicina::Neurologia::ELA|{md_table_field}||Kiernan 2011|ELA::Tratamento"
    )
    vb = _call_tool("validate_batch", batch_id="B01", qa_csv=bad_qa, cloze_csv=CL_OK)
    assert vb["result"] == "FAIL"
    assert any(e["code"] == "TBL.MARKDOWN_PIPE" for e in vb["errors"])


def test_integration_validate_batch_accepts_clean_html_table():
    good_table = (
        "<table><thead><tr><th>Fármaco</th><th>Dose</th></tr></thead>"
        "<tbody><tr><td>Riluzol</td><td>50 mg</td></tr></tbody></table>"
    )
    good_qa = (
        QA_H
        + f"\nQual o tratamento da ELA?|Riluzol|Medicina::Neurologia::ELA|{good_table}||Kiernan 2011|ELA::Tratamento"
    )
    vb = _call_tool("validate_batch", batch_id="B01", qa_csv=good_qa, cloze_csv=CL_OK)
    assert vb["result"] == "PASS"
    tbl_diags = [
        e for e in vb.get("errors", []) + vb.get("warnings", []) if e["code"].startswith("TBL.")
    ]
    assert tbl_diags == []


def test_integration_pipeline_run_all_checks_flags_table_in_card_face():
    from anki_mcp.models import NoteType
    from anki_mcp.tools.pipeline import _parse_csv, _run_all_checks

    table = "<table><thead><tr><th>H</th></tr></thead><tbody><tr><td>D</td></tr></tbody></table>"
    qa_table_face = (
        QA_H + f"\n{table}|Riluzol|Medicina::Neurologia::ELA|||Kiernan 2011|ELA::Tratamento"
    )
    qa_rows = _parse_csv(qa_table_face, NoteType.QA)
    cl_rows = _parse_csv(CL_OK, NoteType.CLOZE)
    _check_results, _errors, warnings = _run_all_checks(
        qa_rows, cl_rows, qa_table_face, CL_OK, deck_dir=None
    )
    assert any(w.code == "TBL.IN_CARD_FACE" for w in warnings)
