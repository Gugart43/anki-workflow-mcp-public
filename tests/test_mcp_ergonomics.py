"""tests/test_mcp_ergonomics.py — tests for FastMCP tags, prompts and resources (H1-3)."""

from __future__ import annotations

import asyncio
import json
from typing import Any

from anki_mcp.core.mcp_meta import TOOL_TAGS, get_phase_rules
from anki_mcp.server import mcp


def test_all_tools_have_tags():
    """Verify that all 49 registered tools have at least one semantic tag."""
    tools = asyncio.run(mcp.list_tools())
    assert len(tools) == 49, f"Expected 49 tools, got {len(tools)}"

    untagged: list[str] = []
    for tool in tools:
        tags = getattr(tool, "tags", None)
        if not tags:
            untagged.append(tool.name)

    assert untagged == [], f"Tools missing semantic tags: {untagged}"

    # Also assert against TOOL_TAGS catalog
    for tool in tools:
        expected = TOOL_TAGS.get(tool.name, set())
        assert tool.tags == expected, f"Tag mismatch for {tool.name}: {tool.tags} != {expected}"


def test_phase_rules_prompt_exists():
    """Verify phase_rules prompt is registered and returns guidelines for all 6 phases."""
    prompts = asyncio.run(mcp.list_prompts())
    prompt_names = [p.name for p in prompts]
    assert "phase_rules" in prompt_names

    for phase in ("drafted", "curated", "enriched", "illustrated", "validated", "exported"):
        rules = get_phase_rules(phase)
        assert f"Fase: {phase}" in rules
        assert "Checks Obrigatórios" in rules
        assert "Tools Recomendadas" in rules

    # Unknown phase handling
    unknown = get_phase_rules("nonexistent_phase")
    assert "Fase desconhecida" in unknown


def test_resources_exist():
    """Verify all 4 schemas and vocab resources are registered and valid JSON/text."""
    resources = asyncio.run(mcp.list_resources())
    uris = {str(r.uri) for r in resources}

    expected_uris = {
        "schema://card-draft",
        "schema://manifesto",
        "vocab://classes",
        "schema://report",
    }
    assert expected_uris.issubset(uris), f"Missing URIs: {expected_uris - uris}"

    # Test reading resources
    def _extract_content(res: Any) -> str:
        if hasattr(res, "contents") and res.contents:
            return res.contents[0].content  # type: ignore[no-any-return]
        return str(res)

    card_draft_obj = json.loads(
        _extract_content(asyncio.run(mcp.read_resource("schema://card-draft")))
    )
    assert "properties" in card_draft_obj

    manifesto_obj = json.loads(
        _extract_content(asyncio.run(mcp.read_resource("schema://manifesto")))
    )
    assert "properties" in manifesto_obj

    vocab_raw = _extract_content(asyncio.run(mcp.read_resource("vocab://classes")))
    assert "quantitativo" in vocab_raw and "callout" in vocab_raw

    report_obj = json.loads(_extract_content(asyncio.run(mcp.read_resource("schema://report"))))
    assert "properties" in report_obj
