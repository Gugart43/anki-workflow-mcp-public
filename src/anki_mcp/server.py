"""anki-workflow-mcp — FastMCP server entry point.

All tools are registered here. Implementation lives in checks/ and tools/.
"""

from __future__ import annotations

import logging
import sys
import warnings

from fastmcp import FastMCP

from anki_mcp.core.mcp_meta import register_prompts_and_resources, tag_tools
from anki_mcp.core.safe import guard_tools
from anki_mcp.tools.compiler import register_compiler_tools
from anki_mcp.tools.corpus import register_corpus_tools
from anki_mcp.tools.hierarchy import register_hierarchy_tools
from anki_mcp.tools.mechanics import register_mechanics_tools
from anki_mcp.tools.media import register_media_tools
from anki_mcp.tools.pipeline import register_pipeline_tools
from anki_mcp.tools.state import register_state_tools
from anki_mcp.tools.tables import register_tables_tools
from anki_mcp.tools.validate import register_validate_tools


def _configure_stdio_transport() -> None:
    for stream_name in ("stdin", "stdout", "stderr"):
        stream = getattr(sys, stream_name, None)
        if stream is not None and hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="strict")
    logging.basicConfig(
        level=logging.WARNING,
        format="[%(levelname)s] %(name)s: %(message)s",
        stream=sys.stderr,
        force=True,
    )
    warnings.simplefilter("default")
    logging.captureWarnings(True)


_configure_stdio_transport()

mcp = FastMCP(
    name="anki-workflow",
    version="0.3.1",
    instructions=(
        "Deterministic validation & state layer for the Anki flashcard generation workflow. "
        "Tools are tagged by domain: validate, mechanics, state, pipeline, media, compiler, tables, triage, hierarchy. "
        "Prompts: phase_rules(phase) provides guidance per pipeline phase. "
        "Resources: schema://card-draft, schema://manifesto, schema://report, vocab://classes. "
        "The LLM writes; the MCP judges, counts, and records."
    ),
)

guard_tools(mcp)  # every tool: structured errors only (A3)
register_validate_tools(mcp)
register_compiler_tools(mcp)
register_mechanics_tools(mcp)
register_state_tools(mcp)
register_media_tools(mcp)
register_tables_tools(mcp)
register_pipeline_tools(mcp)
register_corpus_tools(mcp)
register_hierarchy_tools(mcp)
tag_tools(mcp)
register_prompts_and_resources(mcp)


def main() -> None:
    mcp.run()


if __name__ == "__main__":
    main()
