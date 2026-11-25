#!/usr/bin/env python3
"""Fail when a document cites a diagnostic code that does not exist.

Motivation
----------
Documentation drifts silently. A renamed code leaves ``docs/`` claiming a
diagnostic the binary can never emit, and nothing in CI notices. This script
closes that gap for the *diagnostic code* namespace.

The real catalog is the union of two independent sources:

1. The Python registry ``src/anki_mcp/core/codes.py`` (``all_codes()`` plus the
   ``RETIRED_ALIASES`` keys, which still resolve through ``codes.resolve``).
2. Every diagnostic-code-shaped string literal in the Rust workspace — the Rust
   authority. Harvested with a plain source grep over ``crates/*/src``,
   ``apps/*/src`` and ``tools/*/src``, exactly as the Rust side declares codes
   inline. ``crates/`` alone is not enough: the ``EXPORT.*`` family lives in
   ``apps/anki-workflow-mcp/src/export.rs``.

A token in the documents is accepted only if it exists in that union.

Scope and documented exclusions
-------------------------------
* Scanned: ``docs/**/*.md``, ``README.md`` and ``CONTRIBUTING.md``.
* Fenced code blocks (```` ``` ```` … ```` ``` ````) are skipped: they carry
  sample tool output and JSON payloads, where a dotted uppercase token is often
  illustrative rather than a reference to a registered code.
* ``docs/migration/**`` is skipped. Those files are the historical record of the
  pre-Rust Python pipeline: they enumerate the *legacy* diagnostic names
  (``TAG.EMPTY``, ``STUDY.EMPTY``, ``VOCAB.UNEXPANDED_ACRONYM_WARN``, …) and the
  retired module line numbers (``L.103``) of a module that no longer exists.
  Rewriting that record to match today's catalog would falsify history instead
  of fixing drift.

Usage
-----
    python scripts/check_docs_codes.py [--root PATH] [-v]

Exit status is 0 when every cited code resolves, 1 otherwise.
"""

from __future__ import annotations

import argparse
import importlib.util
import re
import sys
from collections.abc import Iterable, Iterator
from pathlib import Path

#: ``NAMESPACE.NAME`` — the shape shared with Rust's ``DiagnosticCode::parse``
#: (crates/anki-domain/src/lib.rs) and with the Python registry.
CODE_RE = re.compile(r"(?<![A-Za-z0-9_.])([A-Z][A-Z0-9_]*\.[A-Z0-9_.]+)(?![A-Za-z0-9_])")

#: Rust harvest: a quoted string literal spelling a code.
RUST_LITERAL_RE = re.compile(r'"([A-Z][A-Z0-9_]*\.[A-Z0-9_.]+)"')

#: Documentation entry points, relative to the repository root.
DOC_GLOBS = ("docs/**/*.md",)
DOC_FILES = ("README.md", "CONTRIBUTING.md")

#: Directories whose markdown documents a superseded implementation verbatim.
#: See the module docstring for the rationale.
HISTORICAL_DOC_DIRS = ("docs/migration",)

FENCE_RE = re.compile(r"^\s*(```+|~~~+)")


def iter_doc_files(root: Path) -> Iterator[Path]:
    seen: set[Path] = set()
    for pattern in DOC_GLOBS:
        for path in sorted(root.glob(pattern)):
            if path.is_file() and path not in seen:
                seen.add(path)
                yield path
    for name in DOC_FILES:
        path = root / name
        if path.is_file() and path not in seen:
            seen.add(path)
            yield path


def is_historical(root: Path, path: Path) -> bool:
    try:
        rel = path.resolve().relative_to(root.resolve())
    except ValueError:
        return False
    rel_posix = rel.as_posix()
    return any(
        rel_posix == prefix or rel_posix.startswith(prefix + "/") for prefix in HISTORICAL_DOC_DIRS
    )


def strip_fenced_blocks(text: str) -> list[str]:
    """Return ``text``'s lines with fenced code blocks blanked out (line count kept)."""
    out: list[str] = []
    fence: str | None = None
    for line in text.splitlines():
        match = FENCE_RE.match(line)
        if match:
            marker = match.group(1)
            if fence is None:
                fence = marker[0] * 3
                out.append("")
                continue
            if marker.startswith(fence):
                fence = None
                out.append("")
                continue
        out.append("" if fence is not None else line)
    return out


def cited_codes(root: Path) -> dict[str, list[str]]:
    """Map each cited code to ``path:line`` references, skipping exclusions."""
    found: dict[str, list[str]] = {}
    for path in iter_doc_files(root):
        if is_historical(root, path):
            continue
        try:
            raw = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:  # pragma: no cover - defensive
            continue
        lines = strip_fenced_blocks(raw)
        rel = path.relative_to(root).as_posix()
        for lineno, line in enumerate(lines, start=1):
            for match in CODE_RE.finditer(line):
                found.setdefault(match.group(1), []).append(f"{rel}:{lineno}")
    return found


def load_python_catalog(root: Path) -> set[str]:
    """Load ``src/anki_mcp/core/codes.py`` standalone and read its registry."""
    codes_path = root / "src" / "anki_mcp" / "core" / "codes.py"
    if not codes_path.is_file():
        raise FileNotFoundError(f"diagnostic code registry not found: {codes_path}")
    spec = importlib.util.spec_from_file_location("_anki_mcp_codes_catalog", codes_path)
    if spec is None or spec.loader is None:  # pragma: no cover - defensive
        raise ImportError(f"cannot load {codes_path}")
    module = importlib.util.module_from_spec(spec)
    # ``dataclasses`` resolves ``cls.__module__`` through ``sys.modules`` while
    # decorating ``ErrorDef``, so the module must be registered before executing.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    registered = set(module.all_codes())
    registered |= set(getattr(module, "RETIRED_ALIASES", {}))
    return registered


def rust_sources(root: Path) -> Iterator[Path]:
    """Yield Rust sources from every workspace crate under ``crates/``, ``apps/``, ``tools/``.

    The brief for this checker said ``crates/*/src``, but the ``EXPORT.*`` family
    is emitted from ``apps/anki-workflow-mcp/src/export.rs`` (the MCP tool
    ``export_reviewed_files``). Harvesting only ``crates/`` made real, emitted
    codes look unknown, so the harvest covers the whole Rust workspace. This is a
    superset of the requested glob, never a subset.
    """
    for group in ("crates", "apps", "tools"):
        base = root / group
        if not base.is_dir():
            continue
        for crate in sorted(base.iterdir()):
            src = crate / "src"
            if src.is_dir():
                yield from sorted(src.rglob("*.rs"))


def load_rust_catalog(root: Path) -> set[str]:
    """Harvest diagnostic-code string literals from ``crates/*/src``."""
    codes: set[str] = set()
    for path in rust_sources(root):
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:  # pragma: no cover - defensive
            continue
        codes.update(RUST_LITERAL_RE.findall(text))
    return codes


def build_catalog(root: Path) -> tuple[set[str], set[str], set[str]]:
    python_codes = load_python_catalog(root)
    rust_codes = load_rust_catalog(root)
    return python_codes, rust_codes, python_codes | rust_codes


def check(root: Path) -> tuple[dict[str, list[str]], int, int]:
    python_codes, rust_codes, catalog = build_catalog(root)
    cited = cited_codes(root)
    unknown = {code: refs for code, refs in cited.items() if code not in catalog}
    return unknown, len(catalog), len(python_codes) + len(rust_codes)


def format_report(unknown: dict[str, list[str]], total: int, raw_total: int) -> str:
    lines: list[str] = []
    for code in sorted(unknown):
        refs = unknown[code]
        shown = ", ".join(refs[:5])
        extra = "" if len(refs) <= 5 else f" (+{len(refs) - 5} more)"
        lines.append(f"UNKNOWN CODE {code} -> {shown}{extra}")
    lines.append(
        f"catalog={total} codes (unique union; {raw_total} raw entries); unknown={len(unknown)}"
    )
    return "\n".join(lines)


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="repository root (default: parent of this script's directory)",
    )
    parser.add_argument(
        "-v", "--verbose", action="store_true", help="print the resolved catalog size"
    )
    args = parser.parse_args(list(argv) if argv is not None else None)

    root: Path = args.root.resolve()
    unknown, total, raw_total = check(root)

    if unknown:
        print(format_report(unknown, total, raw_total), file=sys.stderr)
        print(
            f"\nFAIL: {len(unknown)} diagnostic code(s) cited in documentation are not "
            "registered in src/anki_mcp/core/codes.py nor in crates/*/src.",
            file=sys.stderr,
        )
        return 1

    if args.verbose:
        print(format_report({}, total, raw_total))
    else:
        print(f"OK: every documented diagnostic code resolves ({total} codes in catalog).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
