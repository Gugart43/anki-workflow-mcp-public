"""Registry contract tests for ``anki_mcp.core.codes``.

A diagnostic code that is emitted but not registered raises ``KeyError`` inside
``Diagnostic.from_code`` at runtime — that is a production failure, not a style
issue. These tests close that gap by scanning ``src/`` for code literals and
requiring every one of them to resolve through ``codes.get``.
"""

from __future__ import annotations

import ast
import re
from pathlib import Path

from anki_mcp.core import codes
from anki_mcp.models import Diagnostic, Severity

SRC_DIR = Path(__file__).resolve().parents[1] / "src"

#: ``NAMESPACE.NAME`` — the shape shared with Rust's ``DiagnosticCode::parse``
#: (crates/anki-domain/src/lib.rs), e.g. ``CARD.TOO_LONG_STEM``.
_CODE_LITERAL_RE = re.compile(
    r"(?<![A-Za-z0-9_.])([A-Z][A-Z0-9_]*(?:\.[A-Z0-9_]+)+)(?![A-Za-z0-9_])"
)


def _emitted_codes() -> dict[str, list[str]]:
    """Map every code-shaped string literal in ``src/`` to its definition sites."""
    found: dict[str, list[str]] = {}
    for path in sorted(SRC_DIR.rglob("*.py")):
        tree = ast.parse(path.read_text(encoding="utf-8"))
        for node in ast.walk(tree):
            if isinstance(node, ast.Constant) and isinstance(node.value, str):
                for match in _CODE_LITERAL_RE.finditer(node.value):
                    site = f"{path.relative_to(SRC_DIR)}:{node.lineno}"
                    found.setdefault(match.group(1), []).append(site)
    return found


def test_every_emitted_code_is_registered():
    """Every code literal in src/ must have an ErrorDef (no KeyError in production)."""
    missing = {}
    for code, sites in _emitted_codes().items():
        try:
            codes.get(code)
        except KeyError:
            missing[code] = sites[0]
    assert not missing, f"codes emitted but not registered in core/codes.py: {missing}"


def test_every_emitted_code_builds_a_diagnostic():
    """``Diagnostic.from_code`` is the real production path: it must never raise."""
    for code in _emitted_codes():
        diag = Diagnostic.from_code(code, "probe")
        assert diag.code == code
        assert isinstance(diag.severity, Severity)
        assert diag.owner_skill == codes.owner(code)


def test_registry_entries_are_well_formed():
    registered = codes.all_codes()
    assert len(registered) == len(set(registered))
    for code in registered:
        entry = codes.get(code)
        assert entry.severity in {"CRITICAL", "WARN", "LOG", "INFO"}, code
        assert entry.owner.startswith("anki-"), code
        assert entry.description.strip(), code
        assert _CODE_LITERAL_RE.fullmatch(code), code


def test_retired_interference_code_maps_to_the_rust_name():
    """QA.INTERFERENCE_WARN is retired: Rust emits CARD.INTERFERENCE_PAIR_WARN."""
    assert "CARD.INTERFERENCE_PAIR_WARN" in codes.all_codes()
    assert "QA.INTERFERENCE_WARN" not in codes.all_codes()
    assert codes.resolve("QA.INTERFERENCE_WARN") == "CARD.INTERFERENCE_PAIR_WARN"
    assert codes.get("QA.INTERFERENCE_WARN") is codes.get("CARD.INTERFERENCE_PAIR_WARN")
    assert codes.severity("QA.INTERFERENCE_WARN") == "WARN"
    assert codes.owner("QA.INTERFERENCE_WARN") == "anki-card-generator"


def test_retired_aliases_point_at_registered_codes():
    for retired, replacement in codes.RETIRED_ALIASES.items():
        assert retired not in codes.all_codes(), retired
        assert replacement in codes.all_codes(), replacement


def test_documented_production_codes_are_registered():
    """Spot-check the families called out by the incident (KeyError in production)."""
    for code in (
        "CARD.TOO_LONG_STEM",
        "CARD.TOO_LONG_ANSWER",
        "CARD.TOO_LONG_SENTENCE",
        "CARD.TOO_LONG_DELETION",
        "CARD.SPLIT_QA_MULTI_QUESTION",
        "CARD.SPLIT_CLOZE_INDICES",
        "PIPELINE.TERMINAL",
        "PIPELINE.INVALID_STATE",
        "PIPELINE.GATE_REQUIRED",
        "PIPELINE.GATE_INVALID",
        "PIPELINE.RECEIPT_MISMATCH",
        "MEDIA.CATALOG_ERROR",
        "MEDIA.COLLISION",
        "MEDIA.SOURCE_MISSING",
        "MEDIA.IO_ERROR",
        "EXPORT.NO_RECEIPT",
        "EXPORT.VALIDATION_FAIL",
        "EXPORT.BATCH_CONFLICT",
        "ARGUMENT.INVALID",
        "RUNTIME.ERROR",
        "INTERNAL.UNEXPECTED",
        "DECK.NOT_FOUND",
        "FILE.NOT_FOUND",
        "HIERARCHY.CORRUPT",
        "CORPUS.NO_SOURCE",
        "INFO.QA_HIDDEN_LIST",
        "INFO.CLOZE_HIDDEN_LIST",
        "INFO.PROMPT_UNAVAILABLE",
        "SANITIZE.INPUT_TOO_LARGE",
    ):
        assert code in codes.all_codes(), code
