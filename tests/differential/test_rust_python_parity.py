"""Differential parity tests comparing Python legacy oracle vs Rust v2 bootstrap."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
from pathlib import Path
from typing import Any

import pytest

from anki_mcp.checks.batch import count_batch
from anki_mcp.checks.cloze import check_cloze
from anki_mcp.checks.csv_structure import check_csv
from anki_mcp.checks.hints import check_hints
from anki_mcp.checks.notes import check_notes
from anki_mcp.checks.qa import check_qa
from anki_mcp.core.receipt import compute_receipt
from anki_mcp.core.rows import split_row
from anki_mcp.core.sanitize import sanitize_field

ROOT = Path(__file__).resolve().parents[2]
FIXTURE_PATH = ROOT / "fixtures" / "golden_parity.json"


def _find_rust_binary() -> Path | None:
    exe_name = "anki-workflow-mcp.exe" if os.name == "nt" else "anki-workflow-mcp"
    for profile in ("debug", "release"):
        candidate = ROOT / "target" / profile / exe_name
        if candidate.is_file():
            return candidate
    if shutil.which("cargo"):
        subprocess.run(
            ["cargo", "build", "-p", "anki-workflow-mcp"],
            cwd=ROOT,
            check=True,
        )
        candidate = ROOT / "target" / "debug" / exe_name
        if candidate.is_file():
            return candidate
    return None


def _run_rust_oracle(binary: Path, requests: list[dict[str, Any]]) -> list[dict[str, Any]]:
    proc = subprocess.run(
        [str(binary), "oracle"],
        input=json.dumps(requests, ensure_ascii=False),
        text=True,
        encoding="utf-8",
        capture_output=True,
        check=True,
    )
    return json.loads(proc.stdout)


def test_python_vs_rust_golden_parity() -> None:
    if os.environ.get("ANKI_SKIP_RUST_ORACLE") == "1":
        pytest.skip("ANKI_SKIP_RUST_ORACLE=1")

    binary = _find_rust_binary()
    if binary is None:
        pytest.skip("Rust binary/cargo not available in this environment")

    fixtures = json.loads(FIXTURE_PATH.read_text(encoding="utf-8"))

    requests: list[dict[str, Any]] = []
    expected_python: list[tuple[str, dict[str, Any]]] = []

    for case in fixtures["receipt_v1"]:
        requests.append(
            {
                "op": "receipt_v1",
                "qa_csv": case["qa_csv"],
                "cloze_csv": case["cloze_csv"],
            }
        )
        expected_python.append(
            (
                f"receipt_v1:{case['name']}",
                {"receipt": compute_receipt(case["qa_csv"], case["cloze_csv"])},
            )
        )

    for case in fixtures["split_row"]:
        requests.append(
            {
                "op": "split_row",
                "line": case["line"],
                "expected": case["expected"],
            }
        )
        sr = split_row(case["line"], case["expected"])
        expected_python.append(
            (
                f"split_row:{case['name']}",
                {
                    "cols": sr.cols,
                    "expected": sr.expected,
                    "structural_ok": sr.structural_ok,
                },
            )
        )

    for case in fixtures["count_batch_cards"]:
        requests.append(
            {
                "op": "count_batch_cards",
                "qa_csv": case["qa_csv"],
                "cloze_csv": case["cloze_csv"],
                "batch_id": case["batch_id"],
            }
        )
        bc = count_batch(case["qa_csv"], case["cloze_csv"], case["batch_id"] or None)
        expected_python.append(
            (
                f"count_batch_cards:{case['name']}",
                bc.model_dump(mode="json"),
            )
        )

    for case in fixtures["sanitize_field"]:
        requests.append(
            {
                "op": "sanitize_field",
                "text": case["text"],
                "is_images_field": case["is_images_field"],
                "convert_markdown": case["convert_markdown"],
            }
        )
        sanitized, fixes = sanitize_field(
            case["text"],
            is_images_field=case["is_images_field"],
            convert_markdown=case["convert_markdown"],
        )
        expected_python.append(
            (
                f"sanitize_field:{case['name']}",
                {"sanitized": sanitized, "fixes": fixes},
            )
        )

    for case in fixtures["check_csv"]:
        requests.append(
            {
                "op": "check_csv",
                "csv_text": case["csv_text"],
                "note_type": case["note_type"],
                "enforce_header": case["enforce_header"],
            }
        )
        diags = check_csv(
            case["csv_text"],
            case["note_type"],
            enforce_header=case["enforce_header"],
        )
        expected_python.append(
            (
                f"check_csv:{case['name']}",
                {"diagnostics": [d.model_dump(mode="json") for d in diags]},
            )
        )

    for case in fixtures["check_cloze"]:
        requests.append(
            {
                "op": "check_cloze",
                "sentence": case["sentence"],
                "row": case["row"],
            }
        )
        diags = check_cloze(case["sentence"], row=case["row"])
        expected_python.append(
            (
                f"check_cloze:{case['name']}",
                {"diagnostics": [d.model_dump(mode="json") for d in diags]},
            )
        )

    for case in fixtures["check_hints"]:
        requests.append(
            {
                "op": "check_hints",
                "sentence": case["sentence"],
                "row": case["row"],
            }
        )
        diags = check_hints(case["sentence"], row=case["row"])
        expected_python.append(
            (
                f"check_hints:{case['name']}",
                {"diagnostics": [d.model_dump(mode="json") for d in diags]},
            )
        )

    for case in fixtures["check_qa"]:
        requests.append(
            {
                "op": "check_qa",
                "enunciado": case["enunciado"],
                "resposta": case["resposta"],
                "row": case["row"],
            }
        )
        diags = check_qa(case["enunciado"], case["resposta"], row=case["row"])
        expected_python.append(
            (
                f"check_qa:{case['name']}",
                {"diagnostics": [d.model_dump(mode="json") for d in diags]},
            )
        )

    for case in fixtures["check_notes"]:
        requests.append(
            {
                "op": "check_notes",
                "notes": case["notes"],
                "answer": case["answer"],
                "row": case["row"],
            }
        )
        diags = check_notes(case["notes"], answer=case["answer"], row=case["row"])
        expected_python.append(
            (
                f"check_notes:{case['name']}",
                {"diagnostics": [d.model_dump(mode="json") for d in diags]},
            )
        )

    rust_results = _run_rust_oracle(binary, requests)
    assert len(rust_results) == len(expected_python)

    for (label, py_val), rs_val in zip(expected_python, rust_results, strict=True):
        assert rs_val == py_val, f"Parity mismatch on {label}: rust={rs_val} != python={py_val}"
