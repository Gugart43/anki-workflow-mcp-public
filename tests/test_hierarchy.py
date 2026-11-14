"""tests/test_hierarchy.py — unit tests for deck hierarchy tools."""

from anki_mcp.tools import hierarchy


def test_normalize_name():
    assert (
        hierarchy._normalize_name("02_MEDICINA_AnkiMBR/Ciclo Básico")
        == "02_MEDICINA_AnkiMBR::Ciclo Básico"
    )
    assert (
        hierarchy._normalize_name("02_MEDICINA_AnkiMBR\x1fCiclo Clínico")
        == "02_MEDICINA_AnkiMBR::Ciclo Clínico"
    )
    assert (
        hierarchy._normalize_name("02_MEDICINA_AnkiMBR::Neurologia::")
        == "02_MEDICINA_AnkiMBR::Neurologia"
    )


def test_strip_accents():
    assert hierarchy._strip_accents("Clínico") == "clinico"
    assert hierarchy._strip_accents("Rubéola") == "rubeola"


def test_hierarchy_add_and_search(tmp_path, monkeypatch):
    test_json = tmp_path / "deck_hierarchy.json"
    monkeypatch.setattr(hierarchy, "get_hierarchy_file", lambda: test_json)

    # 1. Add a subdeck with multiple nesting levels
    _res = hierarchy.register_hierarchy_tools  # ensure module loads
    data = hierarchy.load_hierarchy_data("02_MEDICINA_AnkiMBR")
    hierarchy.save_hierarchy_data(data)

    # Add Tema X
    add_res = hierarchy._normalize_name("02_MEDICINA_AnkiMBR::Ciclo Clínico::Neurologia::Tema X")
    data["decks"].append(add_res)
    data["decks"].append("02_MEDICINA_AnkiMBR::Ciclo Clínico::Neurologia")
    data["decks"].append("02_MEDICINA_AnkiMBR::Ciclo Clínico")
    hierarchy.save_hierarchy_data(data)

    loaded = hierarchy.load_hierarchy_data("02_MEDICINA_AnkiMBR")
    assert "02_MEDICINA_AnkiMBR::Ciclo Clínico::Neurologia::Tema X" in loaded["decks"]

    # Search
    matches = [d for d in loaded["decks"] if "tema x" in hierarchy._strip_accents(d)]
    assert len(matches) == 1
    assert matches[0] == "02_MEDICINA_AnkiMBR::Ciclo Clínico::Neurologia::Tema X"


def test_hierarchy_get_and_tree(tmp_path, monkeypatch):
    import asyncio

    from fastmcp import FastMCP

    test_json = tmp_path / "deck_hierarchy.json"
    monkeypatch.setattr(hierarchy, "get_hierarchy_file", lambda: test_json)

    mcp = FastMCP("test-hierarchy")
    hierarchy.register_hierarchy_tools(mcp)
    get_tool = asyncio.run(mcp.get_tool("deck_hierarchy_get")).fn
    add_tool = asyncio.run(mcp.get_tool("deck_hierarchy_add")).fn
    search_tool = asyncio.run(mcp.get_tool("deck_hierarchy_search")).fn

    # Add decks
    add_tool("02_MEDICINA_AnkiMBR::Cardio::Insuficiência Cardíaca")
    # Already exists branch
    res_dup = add_tool("02_MEDICINA_AnkiMBR::Cardio::Insuficiência Cardíaca")
    assert res_dup["status"] == "already_exists"

    res_plain = get_tool(format_tree=False)
    assert "02_MEDICINA_AnkiMBR::Cardio::Insuficiência Cardíaca" in res_plain["decks"]

    res_tree = get_tool(format_tree=True)
    assert "Cardio" in res_tree["visual_tree"]

    # Search with and without accents
    s_acc = search_tool("Insuficiência")
    assert s_acc["total_matches"] >= 1
    s_noacc = search_tool("insuficiencia")
    assert s_noacc["total_matches"] >= 1


def test_hierarchy_corrupt_file_raises(tmp_path, monkeypatch):
    import pytest

    corrupt = tmp_path / "deck_hierarchy.json"
    corrupt.write_text("{bad json", encoding="utf-8")
    monkeypatch.setattr(hierarchy, "get_hierarchy_file", lambda: corrupt)
    with pytest.raises(RuntimeError, match=r"HIERARCHY\.CORRUPT"):
        hierarchy.load_hierarchy_data()

    corrupt.write_text("{}", encoding="utf-8")
    with pytest.raises(RuntimeError, match=r"HIERARCHY\.CORRUPT"):
        hierarchy.load_hierarchy_data()


def test_hierarchy_env_override(tmp_path, monkeypatch):
    target = tmp_path / "custom_hier.json"
    monkeypatch.setenv("ANKI_WORKFLOW_HIERARCHY_FILE", str(target))
    assert hierarchy.get_hierarchy_file() == target


def test_sync_hierarchy_from_sqlite(tmp_path, monkeypatch):
    import sqlite3

    db_file = tmp_path / "mock_collection.anki2"
    conn = sqlite3.connect(db_file)
    conn.execute("CREATE TABLE decks (id INTEGER PRIMARY KEY, name TEXT)")
    conn.execute("INSERT INTO decks VALUES (1, '02_MEDICINA_AnkiMBR')")
    conn.execute("INSERT INTO decks VALUES (2, '02_MEDICINA_AnkiMBR::Nefro::IRA')")
    conn.commit()
    conn.close()

    hier_file = tmp_path / "deck_hierarchy.json"
    monkeypatch.setattr(hierarchy, "get_hierarchy_file", lambda: hier_file)

    res = hierarchy.sync_hierarchy_from_db(str(db_file))
    assert "02_MEDICINA_AnkiMBR::Nefro::IRA" in res["decks"]
