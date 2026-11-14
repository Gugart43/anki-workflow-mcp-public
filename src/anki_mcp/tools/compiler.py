"""tools/compiler.py — FastMCP tools for structured JSON draft compilation, surgical card repair, and quarantine.

Exposes:
 - cards_compile_drafts: Compiles typed JSON drafts into verified Anki CSV blocks
 - card_patch_apply: Granular patch for single card repair
 - batch_patch_draft: Surgical patch on a staging JSONL file
 - card_quarantine: Partitions invalid cards out of the active batch
 - media_figure_build: Constructs compliant AnkiMBR figure tags with clinical captions
"""

from __future__ import annotations

import json
import threading
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Literal

from fastmcp import FastMCP
from filelock import FileLock, Timeout
from pydantic import ValidationError

from anki_mcp.core.compiler import (
    apply_patch_to_draft,
    compile_drafts_to_csv,
    partition_quarantine,
)
from anki_mcp.core.deck import atomic_write
from anki_mcp.core.safe import error_result, safe_tool
from anki_mcp.models.draft import (
    _SEMANTIC_STYLE_CLASSES,
    CardDraft,
    CardPatch,
    MediaFigure,
)

_STAGING_LOCK_TIMEOUT_S = 5.0
_held_staging_locks = threading.local()


def _held_staging_set() -> set[str]:
    if not hasattr(_held_staging_locks, "paths"):
        _held_staging_locks.paths = set()
    return _held_staging_locks.paths  # type: ignore[no-any-return]


def staging_lock_path(staging_path: str | Path) -> Path:
    """Return canonical lock file path for a resolved staging file."""
    p = Path(staging_path).resolve()
    return p.with_name(f"{p.name}.lock")


@contextmanager
def staging_lock(
    staging_path: str | Path,
    timeout: float = _STAGING_LOCK_TIMEOUT_S,
) -> Iterator[Path]:
    """Hold a filelock for the resolved staging path across threads and processes."""
    p = Path(staging_path).resolve()
    key = str(p)
    if key in _held_staging_set():
        yield p
        return
    lpath = staging_lock_path(p)
    lock = FileLock(str(lpath), timeout=timeout)
    with lock:
        _held_staging_set().add(key)
        try:
            yield p
        finally:
            _held_staging_set().discard(key)


def register_compiler_tools(mcp: FastMCP) -> None:

    @mcp.tool(
        description=(
            "Compile a list of structured CardDraft JSON objects into AnkiMBR-compliant QA and Cloze CSV blocks. "
            "Applies deterministic HTML escaping, sequential cloze renumbering (c1..cN), ID leak purging, "
            "markdown table conversion, and pipe-delimiter escaping. Accepts a JSON string of a list or {'cards': [...]}. "
            "Returns qa_csv, cloze_csv, and compilation stats."
        )
    )
    @safe_tool
    def cards_compile_drafts(
        drafts_json: Any,
        auto_renumber: bool = True,
        auto_purge_ids: bool = True,
        default_campo_estudo: str = "",
    ) -> dict[str, Any]:
        try:
            if isinstance(drafts_json, (list, dict)):
                payload = drafts_json
            elif isinstance(drafts_json, str):
                payload = json.loads(drafts_json)
            else:
                return {
                    "success": False,
                    "error": "drafts_json deve ser uma lista de cartões, um dicionário {'cards': [...]}, ou uma string JSON.",
                }

            if isinstance(payload, dict) and "cards" in payload:
                card_list = payload["cards"]
            elif isinstance(payload, list):
                card_list = payload
            else:
                return {
                    "success": False,
                    "error": "drafts_json deve ser uma lista de cartões ou um dicionário {'cards': [...]}.",
                }

            drafts = [CardDraft(**item) if isinstance(item, dict) else item for item in card_list]
        except (json.JSONDecodeError, ValidationError) as e:
            return {
                "success": False,
                "error": f"Erro de validação do esquema de rascunho: {e}",
            }

        qa_csv, cloze_csv, stats = compile_drafts_to_csv(
            drafts,
            auto_renumber_cloze=auto_renumber,
            auto_purge_id_leaks=auto_purge_ids,
            default_campo_estudo=default_campo_estudo,
        )

        return {
            "success": True,
            "qa_csv": qa_csv,
            "cloze_csv": cloze_csv,
            "stats": stats,
        }

    @mcp.tool(
        description=(
            "Apply a surgical patch or field updates to a single CardDraft JSON object. "
            "Allows repairing a failed card in isolation without regenerating the entire batch. "
            "card_json: JSON string of CardDraft. patch_json: JSON string with 'updates' dictionary."
        )
    )
    @safe_tool
    def card_patch_apply(card_json: str, patch_json: str) -> dict[str, Any]:
        try:
            card_obj = json.loads(card_json)
            patch_obj = json.loads(patch_json)

            draft = CardDraft(**card_obj)
            patch = (
                CardPatch(**patch_obj) if "updates" in patch_obj else CardPatch(updates=patch_obj)
            )
        except (json.JSONDecodeError, ValidationError) as e:
            return {"success": False, "error": f"Erro de parsing ou validação: {e}"}

        patched_draft = apply_patch_to_draft(draft, patch)
        return {
            "success": True,
            "patched_card": patched_draft.model_dump(),
        }

    @mcp.tool(
        description=(
            "Apply a surgical patch directly to a CardDraft within a persistent batch staging file. "
            "staging_path: path to drafts.jsonl or .staging_{batch_id}/drafts.jsonl. "
            "card_index: 0-based index of card to update. patch_json: JSON string with 'updates'."
        )
    )
    @safe_tool
    def batch_patch_draft(
        staging_path: str,
        card_index: int,
        patch_json: str,
        timeout_s: float | None = None,
    ) -> dict[str, Any]:
        p = Path(staging_path).resolve()
        if not p.is_file():
            return {"success": False, "error": f"Staging file not found: {staging_path}"}
        effective_timeout = _STAGING_LOCK_TIMEOUT_S if timeout_s is None else float(timeout_s)
        try:
            with staging_lock(p, timeout=effective_timeout):
                if not p.is_file():
                    return {"success": False, "error": f"Staging file not found: {staging_path}"}
                lines = [
                    line.strip()
                    for line in p.read_text(encoding="utf-8").splitlines()
                    if line.strip()
                ]
                if card_index < 0 or card_index >= len(lines):
                    return {
                        "success": False,
                        "error": f"Index {card_index} out of bounds (total: {len(lines)})",
                    }
                card_data = json.loads(lines[card_index])
                patch_obj = json.loads(patch_json)
                draft = CardDraft(**card_data)
                patch = (
                    CardPatch(**patch_obj)
                    if "updates" in patch_obj
                    else CardPatch(updates=patch_obj)
                )
                patched = apply_patch_to_draft(draft, patch)
                lines[card_index] = json.dumps(patched.model_dump(), ensure_ascii=False)
                atomic_write(p, "\n".join(lines) + "\n")
                return {
                    "success": True,
                    "card_index": card_index,
                    "patched_card": patched.model_dump(),
                }
        except Timeout:
            lock_file = staging_lock_path(p)
            return error_result(
                "GOV.LOCK",
                f"GOV.LOCK: could not acquire staging lock at {lock_file} within {effective_timeout}s. Another process may be writing.",
                "Another process holds the staging lock; retry in a few seconds.",
                success=False,
                error=f"Staging lock timeout after {effective_timeout}s: {lock_file}",
            )
        except Exception as exc:
            return {"success": False, "error": f"Error updating draft staging: {exc}"}

    @mcp.tool(
        description=(
            "Partition invalid cards out of the active batch into a quarantine record. "
            "Allows the remaining valid cards to advance through G3 export under PARTIAL_PASS. "
            "drafts_json: list of CardDraft objects. failed_indices: 0-based indices of cards to isolate."
        )
    )
    @safe_tool
    def card_quarantine(
        drafts_json: Any,
        failed_indices: list[int],
        reason: str = "Falha de validação persistente pós-reparo",
    ) -> dict[str, Any]:
        try:
            if isinstance(drafts_json, (list, dict)):
                payload = drafts_json
            elif isinstance(drafts_json, str):
                payload = json.loads(drafts_json)
            else:
                return {
                    "success": False,
                    "error": "drafts_json deve ser uma lista de cartões ou string JSON.",
                }
            card_list = payload.get("cards", payload) if isinstance(payload, dict) else payload
            drafts = [CardDraft(**item) if isinstance(item, dict) else item for item in card_list]
        except (json.JSONDecodeError, ValidationError) as e:
            return {"success": False, "error": f"Erro de decodificação: {e}"}

        valid, quarantined = partition_quarantine(
            drafts,
            failed_indices=failed_indices,
            reason=reason,
        )

        return {
            "success": True,
            "valid_count": len(valid),
            "quarantined_count": len(quarantined),
            "valid_cards": [c.model_dump() for c in valid],
            "quarantined_records": [q.model_dump() for q in quarantined],
        }

    @mcp.tool(
        description=(
            "Construct a standardized AnkiMBR <figure> tag with clinical caption. "
            "Ensures semantic container formatting (class='anki-figure') without inline styles. "
            "Supports optional title and context_link to connect the image/table to the card's clinical concept."
        )
    )
    @safe_tool
    def media_figure_build(
        filename: str,
        caption: str = "",
        style: str = "",
        title: str = "",
        context_link: str = "",
        item_type: Literal["figure", "table"] = "figure",
    ) -> dict[str, Any]:
        fig = MediaFigure(
            filename=filename,
            caption=caption,
            title=title,
            context_link=context_link,
            item_type=item_type,
            style=style,
        )
        warnings: list[str] = []
        style_clean = style.strip().lower() if style else ""
        if style_clean and style_clean not in _SEMANTIC_STYLE_CLASSES:
            warnings.append(
                f"IMG.INLINE_STYLE: explicit CSS style '{style}' applied; "
                "prefer style='clinical' or style='table' to use AnkiMBR semantic classes"
            )
        html = fig.render_html()
        result: dict[str, Any] = {
            "success": True,
            "html": html,
            "filename": fig.filename,
            "caption": fig.caption,
            "item_type": fig.item_type,
        }
        if warnings:
            result["warnings"] = warnings
        return result
