"""Tests for checks/crossrefs.py"""

from anki_mcp.checks.crossrefs import check_crossrefs
from anki_mcp.models import Severity


def test_no_refs_returns_empty():
    assert check_crossrefs("No references here") == []


def test_detects_ref_format():
    text = "See B01-QA-003 for details"
    diags = check_crossrefs(text, active_ids=set(), current_batch_ids=set())
    assert len(diags) == 1
    assert diags[0].code == "XREF.BROKEN"
    assert diags[0].severity == Severity.CRITICAL


def test_ref_in_active_ids_passes():
    text = "See B01-QA-003 for details"
    diags = check_crossrefs(text, active_ids={"B01-QA-003"})
    assert diags == []


def test_ref_in_current_batch_passes():
    text = "See B02-CL-001a for details"
    diags = check_crossrefs(text, current_batch_ids={"B02-CL-001a"})
    assert diags == []


def test_mixed_resolved_and_broken():
    text = "B01-QA-001 refers to B99-CL-999"
    diags = check_crossrefs(text, active_ids={"B01-QA-001"})
    assert len(diags) == 1
    assert "B99-CL-999" in diags[0].message


def test_no_ledger_skips_resolution():
    text = "B01-QA-003 exists"
    diags = check_crossrefs(text)
    assert diags == []
