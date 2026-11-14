"""core/rows.py — single CSV row splitter shared by checks and pipeline.

Rules (revisão 03, A1/M3):
  - Split exactly on the raw pipe. Never pad, never truncate: a row whose column
    count differs from the note type is *structurally invalid* and reported as
    such, so its semantic checks can be SKIPPED_STRUCTURAL instead of being run
    on shifted values.
  - Never strip field content. Whitespace inside a field is part of the text the
    Anki importer receives; only the line terminator is removed.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class SplitRow:
    cols: list[str]  # raw columns, exactly as split on '|'
    expected: int
    structural_ok: bool  # True iff len(cols) == expected

    @property
    def count(self) -> int:
        return len(self.cols)


def split_row(line: str, expected: int) -> SplitRow:
    """Split *line* on '|' without normalising any column."""
    cols = line.rstrip("\r\n").split("|")
    return SplitRow(cols=cols, expected=expected, structural_ok=len(cols) == expected)


def iter_data_lines(csv_text: str) -> list[tuple[int, str]]:
    """Return (1-based line number, raw line) for every non-blank line after the header.

    Line numbers are file line numbers (header = 1), matching the diagnostics.
    """
    out: list[tuple[int, str]] = []
    for lineno, line in enumerate(csv_text.splitlines()[1:], start=2):
        if line.strip():
            out.append((lineno, line))
    return out
