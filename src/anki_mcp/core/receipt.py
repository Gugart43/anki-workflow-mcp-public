"""core/receipt.py — universal validation receipt generation and verification.

A receipt is a SHA-256 hash of the concatenated QA and Cloze CSVs.
It is issued by validate_batch / pipeline_gate_check upon PASS, and
MUST be provided to export_final to prove the content hasn't been altered.
"""

from __future__ import annotations

import hashlib
import re

#: Shape of a receipt token (``sha256:`` + 64 lowercase hex). Anything else is not a receipt.
RECEIPT_RE = re.compile(r"^sha256:[0-9a-f]{64}$")


def canonical_csv_text(text: str) -> str:
    """Normalize line endings to canonical LF (\n) to prevent CRLF vs LF hash mismatches."""
    return "\n".join(line.rstrip("\r") for line in text.replace("\r\n", "\n").split("\n"))


def compute_receipt(qa_csv: str, cloze_csv: str) -> str:
    """Compute the universal receipt hash for a batch's CSV content (single formula)."""
    norm_qa = canonical_csv_text(qa_csv)
    norm_cloze = canonical_csv_text(cloze_csv)
    combined = (norm_qa + "\n---\n" + norm_cloze).encode("utf-8")
    return "sha256:" + hashlib.sha256(combined).hexdigest()


def verify_receipt(receipt_hash: str, qa_csv: str, cloze_csv: str) -> bool:
    """True if the receipt_hash perfectly matches the provided CSV content."""
    return receipt_hash == compute_receipt(qa_csv, cloze_csv)
