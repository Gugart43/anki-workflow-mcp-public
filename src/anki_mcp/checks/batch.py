"""checks/batch.py — batch size enforcement.

Checks:
  BATCH.OVER_LIMIT — total Anki cards (QA + Cloze expansion) exceeds 20

Exposes:
  count_batch(qa_csv, cloze_csv) -> BatchCount
"""

from __future__ import annotations

import re

from anki_mcp.checks.cloze import count_cloze_indices
from anki_mcp.models import BatchCount

MAX_BATCH = 20

_CLOZE_SENTENCA_COL = 0  # "Sentença" is the 1st field in Cloze CSV


def _count_cloze_cards_in_csv(cloze_csv: str) -> tuple[int, int]:
    """Return (note_count, total_expanded_card_count) for a Cloze CSV block."""
    lines = cloze_csv.splitlines()
    if not lines:
        return 0, 0
    # Skip header
    data = [line for line in lines[1:] if line.strip()]
    notes = 0
    indices = 0
    for line in data:
        cols = line.split("|")
        if not cols:
            continue
        sentenca = cols[0]
        idxs = count_cloze_indices(sentenca)
        notes += 1
        indices += len(idxs) if idxs else 1  # 0-index note still counts as 1 QA-equivalent
    return notes, indices


def _count_qa_cards_in_csv(qa_csv: str) -> int:
    lines = qa_csv.splitlines()
    data = [line for line in lines[1:] if line.strip()]
    return len(data)


def count_batch(
    qa_csv: str,
    cloze_csv: str,
    batch_id: str | None = None,
) -> BatchCount:
    """Count the Anki card budget for a batch.

    QA notes = 1 card each.
    Cloze notes with N distinct indices = N cards each.
    """
    qa_cards = _count_qa_cards_in_csv(qa_csv)
    cloze_notes, cloze_indices = _count_cloze_cards_in_csv(cloze_csv)
    total = qa_cards + cloze_indices
    within = total <= MAX_BATCH
    excess = max(0, total - MAX_BATCH)

    ext_suggestion: str | None = None
    if not within and batch_id:
        # Suggest an EXT batch name
        m = re.match(r"(B\d+)", batch_id)
        if m:
            ext_suggestion = f"{m.group(1)}-EXT"

    return BatchCount(
        qa_cards=qa_cards,
        cloze_notes=cloze_notes,
        cloze_indices=cloze_indices,
        total=total,
        within_limit=within,
        excess=excess,
        ext_batch_suggested=ext_suggestion,
    )
