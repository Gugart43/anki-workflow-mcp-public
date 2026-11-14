"""checks/crossrefs.py — cross-reference resolution.

Checks:
  XREF.BROKEN — cross-reference ID does not resolve to an active card
                or a card in the current batch
"""

from __future__ import annotations

import re

from anki_mcp.models import Diagnostic

# Pattern: references like [B01-QA-001] or (B01-CL-003a) inside any field
_XREF_RE = re.compile(r"\b(B\d{2}-(?:QA|CL)-\d{3}[a-z]?)\b")


def check_crossrefs(
    text: str,
    active_ids: set[str] | None = None,
    current_batch_ids: set[str] | None = None,
    *,
    row: int | None = None,
    field: str | None = None,
) -> list[Diagnostic]:
    """Detect cross-references and verify they resolve.

    active_ids: IDs with status='active' in the Manifesto ledger.
    current_batch_ids: IDs being introduced in this batch (allowed even if not yet committed).
    If both are None, structural detection only (no resolution check).
    """
    diags: list[Diagnostic] = []
    refs_found = _XREF_RE.findall(text)

    if not refs_found:
        return diags

    if active_ids is None and current_batch_ids is None:
        return diags  # no ledger — skip resolution

    allowed = (active_ids or set()) | (current_batch_ids or set())
    for ref in refs_found:
        if ref not in allowed:
            diags.append(
                Diagnostic.from_code(
                    "XREF.BROKEN",
                    f"Cross-reference '{ref}' does not resolve to an active or current-batch ID",
                    row=row,
                    field=field,
                    suggestion="Check the ID is active in the Manifesto or being introduced in this batch",
                )
            )

    return diags
