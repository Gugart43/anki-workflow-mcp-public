"""Differential parity tests comparing Python legacy oracle vs Rust v2 bootstrap.

Cada caso do fixture `fixtures/golden_parity.json` é um teste parametrizado
(`op`, `case_name`), de modo que uma divergência de paridade localize o caso
exato em vez de derrubar um único teste monolítico.
"""

from __future__ import annotations

import json
import os
import re
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

from anki_mcp.checks.batch import count_batch
from anki_mcp.checks.cloze import check_cloze
from anki_mcp.checks.csv_structure import check_csv
from anki_mcp.checks.hints import check_hints
from anki_mcp.checks.notes import check_notes
from anki_mcp.checks.qa import check_qa
from anki_mcp.core import vocab
from anki_mcp.core.receipt import compute_receipt
from anki_mcp.core.rows import split_row
from anki_mcp.core.sanitize import sanitize_field

ROOT = Path(__file__).resolve().parents[2]
FIXTURE_PATH = ROOT / "fixtures" / "golden_parity.json"
RUST_SRC = ROOT / "crates" / "anki-validate" / "src"

try:
    from tests.differential.conftest import OracleRunner
except ImportError:
    from conftest import OracleRunner


def _load_fixtures() -> dict[str, Any]:
    return json.loads(FIXTURE_PATH.read_text(encoding="utf-8"))


FIXTURES = _load_fixtures()

Case = dict[str, Any]
Expectation = tuple[Case, Case]


def _receipt_v1(case: Case) -> Expectation:
    request = {
        "op": "receipt_v1",
        "qa_csv": case["qa_csv"],
        "cloze_csv": case["cloze_csv"],
    }
    expected = {"receipt": compute_receipt(case["qa_csv"], case["cloze_csv"])}
    return request, expected


def _split_row(case: Case) -> Expectation:
    request = {
        "op": "split_row",
        "line": case["line"],
        "expected": case["expected"],
    }
    sr = split_row(case["line"], case["expected"])
    expected = {
        "cols": sr.cols,
        "expected": sr.expected,
        "structural_ok": sr.structural_ok,
    }
    return request, expected


def _count_batch_cards(case: Case) -> Expectation:
    request = {
        "op": "count_batch_cards",
        "profile": "legacy_compat",  # Python remains the explicit compatibility oracle.
        "qa_csv": case["qa_csv"],
        "cloze_csv": case["cloze_csv"],
        "batch_id": case["batch_id"],
    }
    bc = count_batch(case["qa_csv"], case["cloze_csv"], case["batch_id"] or None)
    return request, bc.model_dump(mode="json")


def _sanitize_field(case: Case) -> Expectation:
    request = {
        "op": "sanitize_field",
        "text": case["text"],
        "is_images_field": case["is_images_field"],
        "convert_markdown": case["convert_markdown"],
    }
    sanitized, fixes = sanitize_field(
        case["text"],
        is_images_field=case["is_images_field"],
        convert_markdown=case["convert_markdown"],
    )
    return request, {"sanitized": sanitized, "fixes": fixes}


def _check_csv(case: Case) -> Expectation:
    request = {
        "op": "check_csv",
        "csv_text": case["csv_text"],
        "note_type": case["note_type"],
        "enforce_header": case["enforce_header"],
    }
    diags = check_csv(
        case["csv_text"],
        case["note_type"],
        enforce_header=case["enforce_header"],
    )
    return request, {"diagnostics": [d.model_dump(mode="json") for d in diags]}


def _check_cloze(case: Case) -> Expectation:
    request = {
        "op": "check_cloze",
        "sentence": case["sentence"],
        "row": case["row"],
    }
    diags = check_cloze(case["sentence"], row=case["row"])
    return request, {"diagnostics": [d.model_dump(mode="json") for d in diags]}


def _check_hints(case: Case) -> Expectation:
    request = {
        "op": "check_hints",
        "sentence": case["sentence"],
        "row": case["row"],
    }
    diags = check_hints(case["sentence"], row=case["row"])
    return request, {"diagnostics": [d.model_dump(mode="json") for d in diags]}


def _check_qa(case: Case) -> Expectation:
    request = {
        "op": "check_qa",
        "enunciado": case["enunciado"],
        "resposta": case["resposta"],
        "row": case["row"],
    }
    diags = check_qa(case["enunciado"], case["resposta"], row=case["row"])
    return request, {"diagnostics": [d.model_dump(mode="json") for d in diags]}


def _check_notes(case: Case) -> Expectation:
    request = {
        "op": "check_notes",
        "notes": case["notes"],
        "answer": case["answer"],
        "row": case["row"],
    }
    diags = check_notes(case["notes"], answer=case["answer"], row=case["row"])
    return request, {"diagnostics": [d.model_dump(mode="json") for d in diags]}


CASES_BY_OP: dict[str, Callable[[Case], Expectation]] = {
    "receipt_v1": _receipt_v1,
    "split_row": _split_row,
    "count_batch_cards": _count_batch_cards,
    "sanitize_field": _sanitize_field,
    "check_csv": _check_csv,
    "check_cloze": _check_cloze,
    "check_hints": _check_hints,
    "check_qa": _check_qa,
    "check_notes": _check_notes,
}

PARITY_CASES = [
    pytest.param(op, case["name"], id=f"{op}:{case['name']}")
    for op in CASES_BY_OP
    for case in FIXTURES[op]
]


def _assert_parity(label: str, py_val: Case, rs_val: Case) -> None:
    """Compara um caso, preservando as tolerâncias históricas de dois casos."""
    if label == "check_csv:entities_amp_tags_classes_mathjax_comments":
        rs_diags = [dict(d) for d in rs_val["diagnostics"]]
        py_diags = [dict(d) for d in py_val["diagnostics"]]
        assert len(rs_diags) == len(py_diags)
        for rs_d, py_d in zip(rs_diags, py_diags, strict=True):
            if rs_d.get("code") == "FMT.CLASS_UNKNOWN":
                rs_sugg = rs_d.pop("suggestion")
                py_sugg = py_d.pop("suggestion")
                assert rs_sugg.startswith("Use one of: ")
                assert py_sugg.startswith("Use one of: ")
                rs_classes = set(rs_sugg.removeprefix("Use one of: ").split(", "))
                py_classes = set(py_sugg.removeprefix("Use one of: ").split(", "))
                # B8: igualdade estrita — a comparação já foi `issubset`, tolerância que
                # mascarava classes faltando de um dos lados. Medido: hoje `py == rs`.
                assert py_classes == rs_classes, (
                    f"{label}: conjuntos de classes divergem; "
                    f"py-only={sorted(py_classes - rs_classes)} "
                    f"rs-only={sorted(rs_classes - py_classes)}"
                )
                assert {".anki-table", ".anki-callout", ".ambr-note"}.issubset(rs_classes)
            assert rs_d == py_d, f"Parity mismatch on {label}: rust={rs_d} != python={py_d}"
    elif label.startswith("check_notes:"):
        # Tolerância histórica de `check_notes:metadata_dup_and_style`, agora aplicada a
        # todo caso `check_notes`: o Rust compõe a mensagem com o `preferred_list_format`
        # do profile e o Python usa a string legada fixa. Prefixo idêntico, sufixo do
        # profile livre. Os demais campos seguem comparados por igualdade estrita.
        rs_diags = [dict(d) for d in rs_val["diagnostics"]]
        py_diags = [dict(d) for d in py_val["diagnostics"]]
        assert len(rs_diags) == len(py_diags)
        for rs_d, py_d in zip(rs_diags, py_diags, strict=True):
            if rs_d.get("code") == "NOTE.STYLE_WARN":
                rs_msg = rs_d.pop("message")
                py_msg = py_d.pop("message")
                prefix = "Legacy list formatting found — preferred: "
                assert rs_msg.startswith(prefix) and py_msg.startswith(prefix)
            assert rs_d == py_d, f"Parity mismatch on {label}: rust={rs_d} != python={py_d}"
    else:
        assert rs_val == py_val, f"Parity mismatch on {label}: rust={rs_val} != python={py_val}"


@pytest.mark.parametrize(("op", "case_name"), PARITY_CASES)
def test_python_vs_rust_golden_parity(run_oracle: OracleRunner, op: str, case_name: str) -> None:
    if os.environ.get("ANKI_SKIP_RUST_ORACLE") == "1":
        pytest.skip("ANKI_SKIP_RUST_ORACLE=1")

    case = next(c for c in FIXTURES[op] if c["name"] == case_name)
    request, expected_python = CASES_BY_OP[op](case)

    rust_results = run_oracle.raw([request])
    assert len(rust_results) == 1, (
        f"oracle must answer exactly one request for {op}:{case_name}, got {len(rust_results)}"
    )

    _assert_parity(f"{op}:{case_name}", expected_python, rust_results[0])


# ── Vocabulary parity: the Rust literals are the source of truth ─────────────
#
# `FMT.TAG_FORBIDDEN` is CRITICAL: a tag the Rust compiler accepts and Python
# rejects turns a canonical note into a rejected one. Comparing the *parsed*
# Rust literals by strict equality (instead of a hand-copied subset) fails when
# either side drifts, in either direction.


def _rust_str_const(path: Path, name: str) -> frozenset[str]:
    """Parse ``const NAME: &[&str] = &["a", "b", ...];`` from a Rust source file.

    Flat array of string literals — a non-greedy body capture plus ``"..."``
    extraction is enough, so no extra dependency is needed. A missing constant
    fails loudly instead of degrading into an empty set that compares equal.
    """
    source = path.read_text(encoding="utf-8")
    match = re.search(
        rf"(?:pub )?const {name}\s*:\s*&\[&str\]\s*=\s*&\[(.*?)\];",
        source,
        re.DOTALL,
    )
    assert match is not None, f"const {name} not found in {path}"
    tokens = re.findall(r'"([^"]*)"', match.group(1))
    assert tokens, f"const {name} in {path} has no string literals"
    return frozenset(tokens)


RUST_TAGS = _rust_str_const(RUST_SRC / "vocab.rs", "ALLOWED_TAGS")
RUST_IMAGES_ONLY_TAGS = _rust_str_const(RUST_SRC / "vocab.rs", "IMAGES_ONLY_TAGS")
RUST_PAIRED_TAGS = _rust_str_const(RUST_SRC / "vocab.rs", "PAIRED_TAGS")
RUST_ALLOWED_CLASSES = _rust_str_const(RUST_SRC / "vocab.rs", "ALLOWED_CLASSES")
RUST_SECURITY_TAGS = _rust_str_const(RUST_SRC / "html_security.rs", "ALLOWED_TAGS")


def _report(label: str, py_set: frozenset[str], rust_set: frozenset[str]) -> str:
    return (
        f"{label} diverges from the Rust vocabulary: "
        f"python-only={sorted(py_set - rust_set)} rust-only={sorted(rust_set - py_set)}"
    )


def test_tag_vocabulary_is_strictly_equal_to_vocab_rs():
    assert vocab.ALLOWED_TAGS == RUST_TAGS, _report("ALLOWED_TAGS", vocab.ALLOWED_TAGS, RUST_TAGS)
    assert vocab.IMAGES_ONLY_TAGS == RUST_IMAGES_ONLY_TAGS, _report(
        "IMAGES_ONLY_TAGS", vocab.IMAGES_ONLY_TAGS, RUST_IMAGES_ONLY_TAGS
    )
    assert vocab.PAIRED_TAGS == RUST_PAIRED_TAGS, _report(
        "PAIRED_TAGS", vocab.PAIRED_TAGS, RUST_PAIRED_TAGS
    )


def test_class_vocabulary_is_strictly_equal_to_vocab_rs():
    assert vocab.ALLOWED_CLASSES == RUST_ALLOWED_CLASSES, _report(
        "ALLOWED_CLASSES", vocab.ALLOWED_CLASSES, RUST_ALLOWED_CLASSES
    )


def test_html_security_allowlist_is_the_union_of_the_vocab_tags():
    """The sanitizer allowlist is `ALLOWED_TAGS` + `IMAGES_ONLY_TAGS`, never a third list.

    Two Rust lists exist on purpose (the sanitizer cannot know which field it is
    looking at), so the relation is pinned here: a future divergence between
    ``vocab.rs`` and ``html_security.rs`` fails loudly instead of being
    silently "resolved" by picking one side.
    """
    assert RUST_TAGS.isdisjoint(RUST_IMAGES_ONLY_TAGS)
    assert RUST_SECURITY_TAGS == RUST_TAGS | RUST_IMAGES_ONLY_TAGS
