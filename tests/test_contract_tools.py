"""Contract tests — every MCP tool registered on the real server (revision 03, §2.1 / A3).

For each tool, the test calls the registered function with the *minimal* input its signature
requires and asserts:
  - the result is a dict (never an exception escaping to the protocol);
  - an error result has the canonical shape {status:'ERROR', code, message, action};
  - no internal failure leaked as INTERNAL.UNEXPECTED (that would mean an AttributeError,
    KeyError, TypeError, … inside the tool — the class of bug B1 was).

Disk side effects are isolated: writes go to tmp_path and the hierarchy/Anki paths are
redirected, so the repository and the user's Anki profile are never touched.
"""

from __future__ import annotations

import asyncio
import inspect
from pathlib import Path
from typing import Any

import pytest

from anki_mcp.server import mcp
from anki_mcp.tools import hierarchy as hierarchy_mod


def _all_tool_names() -> list[str]:
    return sorted(t.name for t in asyncio.run(mcp.list_tools()))


def _fn(name: str):
    return asyncio.run(mcp.get_tool(name)).fn


def _placeholder(name: str, annotation: Any, tmp: Path) -> Any:
    text = str(annotation)
    if "deck_dir" in name:
        return str(tmp / "deck")  # does not exist → every deck tool must answer DECK.NOT_FOUND
    if "path" in name or "catalog" in name or name in {"catalog_paths"}:
        return str(tmp / "missing.csv")
    if "list" in text:
        return []
    if "dict" in text:
        return {}
    if "bool" in text:
        return False
    if "int" in text:
        return 0
    return ""


def _minimal_args(name: str, tmp: Path) -> dict[str, Any]:
    sig = inspect.signature(_fn(name))
    args: dict[str, Any] = {}
    for pname, param in sig.parameters.items():
        if param.default is not inspect.Parameter.empty:
            continue  # optional → use the default
        args[pname] = _placeholder(pname, param.annotation, tmp)
    return args


@pytest.fixture(autouse=True)
def _isolated_disk(tmp_path, monkeypatch):
    """Never let a contract call write to the repo or to a real Anki profile."""
    monkeypatch.setattr(hierarchy_mod, "get_hierarchy_file", lambda: tmp_path / "hierarchy.json")
    monkeypatch.setattr(hierarchy_mod, "get_default_anki_col", lambda: tmp_path / "no-col.anki2")
    monkeypatch.chdir(tmp_path)


def test_server_registers_tools():
    names = _all_tool_names()
    assert len(names) >= 40, names  # 44 at revision 03; a silent drop must fail loudly


@pytest.mark.parametrize("tool_name", _all_tool_names())
def test_tool_contract_minimal_input(tool_name, tmp_path):
    args = _minimal_args(tool_name, tmp_path)
    result = _fn(tool_name)(**args)

    assert isinstance(result, dict), f"{tool_name} returned {type(result).__name__}"
    assert result.get("code") != "INTERNAL.UNEXPECTED", (
        f"{tool_name} leaked an internal error: {result}"
    )
    if result.get("status") == "ERROR":
        missing = {"code", "message", "action"} - result.keys()
        assert not missing, f"{tool_name} error payload lacks {missing}: {result}"
