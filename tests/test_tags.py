"""Tests for checks/tags.py"""

from anki_mcp.checks.tags import check_tags
from anki_mcp.models import Severity


def test_empty_field():
    assert check_tags("") == []
    assert check_tags("   ") == []


def test_valid_tag_no_taxonomy():
    diags = check_tags("Neurologia::CriseConvulsiva::Manejo_Agudo")
    assert diags == []


def test_tag_depth_exceeded():
    tag = "A::B::C::D::E"
    diags = check_tags(tag)
    depth_diags = [d for d in diags if d.code == "TAG.DEPTH"]
    assert len(depth_diags) == 1
    assert depth_diags[0].severity == Severity.CRITICAL


def test_tag_depth_ok():
    tag = "A::B::C::D"
    diags = check_tags(tag)
    depth_diags = [d for d in diags if d.code == "TAG.DEPTH"]
    assert depth_diags == []


def test_tag_format_spaces():
    # Tags are split by whitespace, so "invalid tag" becomes two valid tokens.
    # Check that multiple valid tags produce no errors:
    assert check_tags("Tag1 Tag2") == []


def test_tag_format_invalid_chars():
    diags = check_tags("tag!@#$%")
    fmt = [d for d in diags if d.code == "TAG.FORMAT"]
    assert len(fmt) == 1


def test_tag_unregistered():
    taxonomy = {"Neurologia::CriseConvulsiva"}
    diags = check_tags("Neurologia::Epilepsia", taxonomy_nodes=taxonomy)
    unreg = [d for d in diags if d.code == "TAG.UNREGISTERED"]
    assert len(unreg) == 1


def test_tag_registered():
    taxonomy = {"Neurologia::CriseConvulsiva"}
    diags = check_tags("Neurologia::CriseConvulsiva", taxonomy_nodes=taxonomy)
    unreg = [d for d in diags if d.code == "TAG.UNREGISTERED"]
    assert unreg == []


def test_tag_technical_id_exempt(tmp_path):
    """id::<CARD_ID> tags are exempt from taxonomy and formatting checks."""
    taxonomy = {"Neurologia::CriseConvulsiva"}
    diags = check_tags("Neurologia::CriseConvulsiva id::B01-QA-001", taxonomy_nodes=taxonomy)
    assert diags == []
