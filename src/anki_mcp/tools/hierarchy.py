"""tools/hierarchy.py — deck hierarchy management and sync for 02_MEDICINA_AnkiMBR.

Tools:
  deck_hierarchy_get    — retrieve registered hierarchy (paths or visual tree)
  deck_hierarchy_search — search for subdecks matching a topic or keyword
  deck_hierarchy_add    — register a new subdeck and auto-create any missing parent levels
  deck_hierarchy_sync   — sync hierarchy from live Anki SQLite collection (safe under lock)
"""

from __future__ import annotations

import json
import logging
import os
import shutil
import sqlite3
import unicodedata
from datetime import datetime
from pathlib import Path
from typing import Any

from fastmcp import FastMCP

from anki_mcp.core.safe import safe_tool

logger = logging.getLogger("anki_mcp")


# ── Storage & Resolution ───────────────────────────────────────────────────────

DEFAULT_ROOT = "02_MEDICINA_AnkiMBR"


def get_default_anki_col() -> Path:
    """Locate the default user collection.anki2 on Windows or Linux/macOS."""
    appdata = os.environ.get("APPDATA")
    if appdata:
        p = Path(appdata) / "Anki2" / "Usuário 1" / "collection.anki2"
        if p.exists():
            return p
        # Try any profile under Anki2
        base_anki2 = Path(appdata) / "Anki2"
        if base_anki2.exists():
            for prof in base_anki2.iterdir():
                cand = prof / "collection.anki2"
                if cand.exists():
                    return cand
    # Fallback to home dir
    home_anki = Path.home() / ".local" / "share" / "Anki2"
    if home_anki.exists():
        for prof in home_anki.iterdir():
            cand = prof / "collection.anki2"
            if cand.exists():
                return cand
    return Path.home() / "AppData" / "Roaming" / "Anki2" / "Usuário 1" / "collection.anki2"


def get_hierarchy_file() -> Path:
    """Return the persistent JSON file path for deck hierarchy."""
    env_override = os.environ.get("ANKI_WORKFLOW_HIERARCHY_FILE")
    if env_override:
        return Path(env_override)
    user_file = Path.home() / ".anki-workflow" / "deck_hierarchy.json"
    user_file.parent.mkdir(parents=True, exist_ok=True)
    return user_file


def _normalize_name(name: str) -> str:
    """Normalize deck path using :: separator and strip whitespace."""
    # Replace unicode unit separator or slashes with ::
    clean = name.replace("\x1f", "::").replace("/", "::")
    parts = [p.strip() for p in clean.split("::") if p.strip()]
    return "::".join(parts)


def _strip_accents(s: str) -> str:
    """Normalize string for accent-insensitive search."""
    return "".join(
        c for c in unicodedata.normalize("NFD", s.lower()) if unicodedata.category(c) != "Mn"
    )


def load_hierarchy_data(root_deck: str = DEFAULT_ROOT) -> dict[str, Any]:
    """Load hierarchy JSON data or initialize if missing."""
    h_file = get_hierarchy_file()
    if h_file.exists():
        # A corrupt hierarchy file is an error to surface, never a silent re-sync (M1).
        try:
            with open(h_file, encoding="utf-8") as f:
                data = json.load(f)
        except (OSError, json.JSONDecodeError) as exc:
            raise RuntimeError(f"HIERARCHY.CORRUPT: cannot read {h_file}: {exc}") from exc
        if isinstance(data, dict) and "decks" in data:
            return data
        raise RuntimeError(f"HIERARCHY.CORRUPT: {h_file} has no 'decks' key")

    # If file doesn't exist, try auto-syncing from Anki collection
    col_path = get_default_anki_col()
    if col_path.exists():
        return sync_hierarchy_from_db(str(col_path), root_deck)

    # Empty fallback
    return {
        "root_deck": root_deck,
        "last_synced": datetime.now().isoformat(),
        "total_subdecks": 1,
        "decks": [root_deck],
        "deck_details": {root_deck: {"depth": 0, "created_at": datetime.now().isoformat()}},
    }


def save_hierarchy_data(data: dict[str, Any]) -> None:
    """Persist hierarchy data to disk."""
    h_file = get_hierarchy_file()
    h_file.parent.mkdir(parents=True, exist_ok=True)
    with open(h_file, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False, indent=2)


def sync_hierarchy_from_db(col_path: str, root_deck: str = DEFAULT_ROOT) -> dict[str, Any]:
    """Extract all decks from collection.anki2 safely even if Anki is open/locked."""
    src = Path(col_path)
    if not src.exists():
        raise FileNotFoundError(f"Anki database not found at {col_path}")

    # Use a temporary copy to avoid database lock if Anki is currently running
    temp_copy = (
        Path(os.environ.get("LOCALAPPDATA", str(Path.home())))
        / "Temp"
        / f"col_sync_{os.getpid()}.anki2"
    )
    try:
        shutil.copy2(src, temp_copy)
        db_to_use = temp_copy
    except OSError as exc:
        logger.warning("hierarchy: temp copy failed (%s); reading the live database", exc)
        db_to_use = src

    def unicase(a: str, b: str) -> int:
        a_l, b_l = a.lower(), b.lower()
        return (a_l > b_l) - (a_l < b_l)

    try:
        conn = sqlite3.connect(db_to_use)
        conn.create_collation("unicase", unicase)
        cursor = conn.cursor()
        cursor.execute("SELECT id, name FROM decks")
        rows = cursor.fetchall()
        conn.close()
    finally:
        if temp_copy.exists():
            try:
                temp_copy.unlink()
            except OSError as exc:
                logger.warning("hierarchy: could not remove temp copy %s: %s", temp_copy, exc)

    subdecks = []
    for did, raw_name in rows:
        clean_name = _normalize_name(raw_name)
        if clean_name == root_deck or clean_name.startswith(f"{root_deck}::"):
            subdecks.append({"id": did, "name": clean_name, "depth": clean_name.count("::")})

    subdecks.sort(key=lambda x: x["name"])

    # Merge with existing file if it had manual additions
    h_file = get_hierarchy_file()
    existing_decks = set()
    if h_file.exists():
        try:
            with open(h_file, encoding="utf-8") as f:
                prev = json.load(f)
        except (OSError, json.JSONDecodeError) as exc:
            raise RuntimeError(f"HIERARCHY.CORRUPT: cannot merge {h_file}: {exc}") from exc
        existing_decks = set(prev.get("decks", []))

    deck_names = sorted(set([d["name"] for d in subdecks] + list(existing_decks)))

    hierarchy_data = {
        "root_deck": root_deck,
        "last_synced": datetime.now().isoformat(),
        "total_subdecks": len(deck_names),
        "decks": deck_names,
        "deck_details": {
            d["name"]: {"id": d.get("id"), "depth": d["name"].count("::")} for d in subdecks
        },
    }
    save_hierarchy_data(hierarchy_data)
    return hierarchy_data


# ── Tool registration ─────────────────────────────────────────────────────────


def register_hierarchy_tools(mcp: FastMCP) -> None:

    @mcp.tool(
        description=(
            "Get the registered deck hierarchy for 02_MEDICINA_AnkiMBR. "
            "filter_prefix: optional sub-path filter (e.g. '02_MEDICINA_AnkiMBR::Ciclo Clínico::Neurologia'). "
            "max_depth: max nesting level to include (default 10). "
            "format_tree: if true, returns an ASCII/Unicode visual tree instead of list."
        )
    )
    @safe_tool
    def deck_hierarchy_get(
        filter_prefix: str = DEFAULT_ROOT,
        max_depth: int = 10,
        format_tree: bool = False,
    ) -> dict[str, Any]:
        data = load_hierarchy_data()
        prefix = _normalize_name(filter_prefix)
        all_decks = data.get("decks", [])

        filtered = [
            d
            for d in all_decks
            if (d == prefix or d.startswith(f"{prefix}::")) and d.count("::") <= max_depth
        ]

        if not format_tree:
            return {
                "root_deck": data.get("root_deck", DEFAULT_ROOT),
                "filter_prefix": prefix,
                "total_matched": len(filtered),
                "total_in_hierarchy": len(all_decks),
                "last_synced": data.get("last_synced"),
                "decks": filtered,
            }

        # Build visual tree representation
        tree_lines = [f"📂 {prefix}"]
        for d in filtered:
            if d == prefix:
                continue
            relative = d[len(prefix) + 2 :]
            depth = relative.count("::")
            indent = "  " * (depth + 1)
            deck_name = relative.split("::")[-1]
            tree_lines.append(f"{indent}└── 🏷️ {deck_name}")

        return {
            "root_deck": data.get("root_deck", DEFAULT_ROOT),
            "filter_prefix": prefix,
            "total_matched": len(filtered),
            "visual_tree": "\n".join(tree_lines),
        }

    @mcp.tool(
        description=(
            "Search the 02_MEDICINA_AnkiMBR hierarchy for subdecks matching a keyword or topic. "
            "query: search string (e.g. 'Catarata', 'Neurologia', 'Glaucoma', 'Esclerose'). "
            "root_deck: root prefix to restrict to (default '02_MEDICINA_AnkiMBR')."
        )
    )
    @safe_tool
    def deck_hierarchy_search(
        query: str,
        root_deck: str = DEFAULT_ROOT,
    ) -> dict[str, Any]:
        data = load_hierarchy_data(root_deck)
        all_decks = data.get("decks", [])
        norm_q = _strip_accents(query.strip())

        matches = []
        for d in all_decks:
            if norm_q in _strip_accents(d):
                parent = "::".join(d.split("::")[:-1]) if "::" in d else None
                leaf = d.split("::")[-1]
                matches.append(
                    {
                        "full_path": d,
                        "name": leaf,
                        "parent": parent,
                        "depth": d.count("::"),
                    }
                )

        return {
            "query": query,
            "total_matches": len(matches),
            "matches": matches,
        }

    @mcp.tool(
        description=(
            "Add a new subdeck to the 02_MEDICINA_AnkiMBR hierarchy and persist the update immediately. "
            "subdeck_path: the full or relative subdeck path to add (e.g. '02_MEDICINA_AnkiMBR::Ciclo Clínico::Oftalmologia::Catarata'). "
            "Automatically creates all missing parent levels in the hierarchy."
        )
    )
    @safe_tool
    def deck_hierarchy_add(
        subdeck_path: str,
        root_deck: str = DEFAULT_ROOT,
    ) -> dict[str, Any]:
        norm_path = _normalize_name(subdeck_path)
        if not norm_path.startswith(root_deck):
            norm_path = f"{root_deck}::{norm_path}"

        data = load_hierarchy_data(root_deck)
        decks_set = set(data.get("decks", []))

        # Ensure all ancestor levels exist
        parts = norm_path.split("::")
        ancestors_created = []
        for i in range(1, len(parts) + 1):
            ancestor = "::".join(parts[:i])
            if ancestor not in decks_set:
                decks_set.add(ancestor)
                ancestors_created.append(ancestor)

        already_existed = len(ancestors_created) == 0

        # Update data and persist
        sorted_decks = sorted(decks_set)
        data["decks"] = sorted_decks
        data["total_subdecks"] = len(sorted_decks)
        data["last_updated"] = datetime.now().isoformat()

        # Update details
        details = data.get("deck_details", {})
        for anc in ancestors_created:
            details[anc] = {
                "depth": anc.count("::"),
                "manually_added": True,
                "added_at": datetime.now().isoformat(),
            }
        data["deck_details"] = details

        save_hierarchy_data(data)

        return {
            "status": "already_exists" if already_existed else "added",
            "deck": norm_path,
            "parents_created": ancestors_created[:-1] if ancestors_created else [],
            "total_subdecks": len(sorted_decks),
            "last_updated": data["last_updated"],
        }

    @mcp.tool(
        description=(
            "Synchronize deck hierarchy from the live Anki SQLite database collection.anki2. "
            "Safely handles running Anki locks and merges with any manually added subdecks."
        )
    )
    @safe_tool
    def deck_hierarchy_sync(
        anki_collection_path: str = "",
        root_deck: str = DEFAULT_ROOT,
    ) -> dict[str, Any]:
        col = Path(anki_collection_path) if anki_collection_path else get_default_anki_col()
        if not col.exists():
            return {
                "error": f"Anki collection not found at {col}",
                "status": "failed",
            }

        result = sync_hierarchy_from_db(str(col), root_deck)
        return {
            "status": "synced",
            "root_deck": root_deck,
            "total_subdecks": result.get("total_subdecks", 0),
            "last_synced": result.get("last_synced"),
            "storage_path": str(get_hierarchy_file()),
        }
