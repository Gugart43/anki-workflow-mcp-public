"""checks/tags.py — anki-registry tag taxonomy implementation.

Checks:
  TAG.UNREGISTERED — tag not present in Manifesto taxonomy
  TAG.DEPTH        — tag depth exceeds 4 levels
  TAG.FORMAT       — tag contains spaces or invalid characters
"""

from __future__ import annotations

import re

from anki_mcp.models import Diagnostic

_VALID_TAG_CHARS = re.compile(r"^[A-Za-zÀ-ÿ0-9_:]+$")
_SPACE_IN_TAG = re.compile(r"\s")
_SEPARATOR = "::"


def _depth(tag: str) -> int:
    return tag.count(_SEPARATOR) + 1


def check_tags(
    tags_field: str,
    taxonomy_nodes: set[str] | None = None,
    *,
    row: int | None = None,
    field: str = "Etiquetas",
) -> list[Diagnostic]:
    """Check tags in the Etiquetas field.

    taxonomy_nodes: flat set of all authorized tag strings from the Manifesto.
    If None, taxonomy check is skipped (useful for structural-only validation).
    """
    diags: list[Diagnostic] = []
    if not tags_field.strip():
        return diags

    raw_tags = [t.strip() for t in tags_field.split() if t.strip()]

    for tag in raw_tags:
        # Technical id tags (id::<CARD_ID>) are exempt from taxonomy checks
        if tag.startswith("id::"):
            continue

        # TAG.FORMAT — spaces
        if _SPACE_IN_TAG.search(tag):
            diags.append(
                Diagnostic.from_code(
                    "TAG.FORMAT",
                    f"Tag '{tag}' contains spaces — use :: as separator, no spaces",
                    row=row,
                    field=field,
                )
            )
            continue

        # TAG.FORMAT — invalid chars (allow :: as separator)
        tag_for_check = tag.replace("::", "")
        if not re.match(r"^[A-Za-zÀ-ÿ0-9_]+$", tag_for_check) and tag_for_check:
            diags.append(
                Diagnostic.from_code(
                    "TAG.FORMAT",
                    f"Tag '{tag}' contains invalid characters",
                    row=row,
                    field=field,
                    suggestion="Use only letters, digits, underscores, and :: as separator",
                )
            )

        # TAG.DEPTH
        if _depth(tag) > 4:
            diags.append(
                Diagnostic.from_code(
                    "TAG.DEPTH",
                    f"Tag '{tag}' has depth {_depth(tag)} (maximum is 4 levels)",
                    row=row,
                    field=field,
                )
            )

        # TAG.UNREGISTERED
        if taxonomy_nodes is not None and tag not in taxonomy_nodes:
            diags.append(
                Diagnostic.from_code(
                    "TAG.UNREGISTERED",
                    f"Tag '{tag}' is not registered in the Manifesto taxonomy",
                    row=row,
                    field=field,
                    suggestion="Register the tag first, or check for typos",
                )
            )

    return diags
