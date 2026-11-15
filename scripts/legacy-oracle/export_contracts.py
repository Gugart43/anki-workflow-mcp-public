"""Export canonical MCP contracts (tools, resources, prompts) from the legacy Python server."""

from __future__ import annotations

import asyncio
import json
from pathlib import Path
from typing import Any

from anki_mcp.server import mcp

ROOT = Path(__file__).resolve().parents[2]
OUT_DIR = ROOT / "contracts" / "legacy"


def _dump_json(path: Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, indent=2, ensure_ascii=False, sort_keys=True) + "\n",
        encoding="utf-8",
    )


async def export_all() -> None:
    tools = await mcp.list_tools()
    resources = await mcp.list_resources()
    prompts = await mcp.list_prompts()

    tools_data = sorted(
        [
            {
                "name": t.name,
                "description": t.description,
                "parameters": t.parameters,
                "output_schema": getattr(t, "output_schema", None),
                "tags": sorted(t.tags) if getattr(t, "tags", None) else [],
                "annotations": (
                    t.annotations.model_dump(mode="json", exclude_none=True)
                    if getattr(t, "annotations", None)
                    else None
                ),
            }
            for t in tools
        ],
        key=lambda x: x["name"],
    )

    resources_data = sorted(
        [
            {
                "uri": str(r.uri),
                "name": r.name,
                "description": r.description,
                "mime_type": getattr(r, "mime_type", None),
            }
            for r in resources
        ],
        key=lambda x: x["uri"],
    )

    prompts_data = sorted(
        [
            {
                "name": p.name,
                "description": p.description,
                "arguments": [
                    {
                        "name": a.name,
                        "description": a.description,
                        "required": a.required,
                    }
                    for a in (getattr(p, "arguments", None) or [])
                ],
            }
            for p in prompts
        ],
        key=lambda x: x["name"],
    )

    _dump_json(OUT_DIR / "tools.json", {"count": len(tools_data), "tools": tools_data})
    _dump_json(OUT_DIR / "resources.json", {"count": len(resources_data), "resources": resources_data})
    _dump_json(OUT_DIR / "prompts.json", {"count": len(prompts_data), "prompts": prompts_data})

    print(
        f"Exported {len(tools_data)} tools, {len(resources_data)} resources, {len(prompts_data)} prompts to {OUT_DIR}"
    )


if __name__ == "__main__":
    asyncio.run(export_all())
