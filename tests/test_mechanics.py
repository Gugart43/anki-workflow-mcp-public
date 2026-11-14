"""Tests for tools/mechanics.py — csv_build_row and count_batch_cards."""

from anki_mcp.tools.mechanics import _escape_field


def test_escape_pipe():
    assert "|" not in _escape_field("A|B")
    assert "&#124;" in _escape_field("A|B")


def test_escape_newlines():
    assert "<br>" in _escape_field("line1\nline2")
    assert "<br>" in _escape_field("line1\r\nline2")


def test_escape_bare_amp():
    result = _escape_field("A & B")
    assert "&amp;" in result


def test_escape_existing_entity():
    result = _escape_field("A &amp; B")
    assert result.count("&amp;") == 1  # no double-escape


def test_csv_build_row_via_mcp():
    """Test csv_build_row through FastMCP registration."""
    import asyncio

    from fastmcp import FastMCP

    from anki_mcp.tools.mechanics import register_mechanics_tools

    mcp = FastMCP("test")
    register_mechanics_tools(mcp)

    async def _test():
        tool = await mcp.get_tool("csv_build_row")
        result = tool.fn(fields=["A", "B", "C", "D", "E", "F", "G"])
        assert "csv_line" in result
        assert result["field_count"] == 7
        return result

    asyncio.run(_test())


def test_count_batch_cards_via_mcp():
    """Test count_batch_cards through FastMCP."""
    import asyncio

    from fastmcp import FastMCP

    from anki_mcp.tools.mechanics import register_mechanics_tools

    mcp = FastMCP("test")
    register_mechanics_tools(mcp)

    qa_h = "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas"
    cloze_h = "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas"
    qa_csv = qa_h + "\nQ1|A1|C|N||R|T\nQ2|A2|C|N||R|T"
    cl_csv = cloze_h + "\n{{c1::A}} {{c2::B}}|N|C||R|T"

    async def _test():
        tool = await mcp.get_tool("count_batch_cards")
        result = tool.fn(qa_csv=qa_csv, cloze_csv=cl_csv)
        assert "total" in result
        assert result["total"] == 4  # 2 QA + 2 cloze cards
        return result

    asyncio.run(_test())
