import json

import pytest

from anki_mcp.core.deck import load_manifesto, recover_journal
from tests.test_flow_e2e import CLOZE_CSV, NEW_IDS, QA_CSV
from tests.test_remote_integrity import call


@pytest.fixture
def batch(tmp_path):
    d = tmp_path / "deck"
    d.mkdir()
    call(
        "manifesto_init",
        deck_dir=str(d),
        campo_estudo="Medicina::Neurologia::ELA",
        deck_slug="ela",
        taxonomy_root="ELA",
    )
    receipt = call("validate_batch", batch_id="B01", qa_csv=QA_CSV, cloze_csv=CLOZE_CSV)[
        "content_hash"
    ]
    return d, {
        "deck_dir": str(d),
        "batch_id": "B01",
        "qa_csv": QA_CSV,
        "cloze_csv": CLOZE_CSV,
        "receipt_hash": receipt,
    }


def test_commit_changelog_retry_is_idempotent(batch):
    d, kw = batch
    changes = json.dumps(
        [{"date": "2026-11-14", "batch": "B01", "action": "TEST", "reason": "test"}]
    )
    assert call("manifesto_commit", **kw, changelog_entries=changes)["status"] == "COMMITTED"
    before = (d / "manifesto.json").read_bytes()
    assert call("manifesto_commit", **kw, changelog_entries=changes)["status"] == "ALREADY_EXPORTED"
    assert (d / "manifesto.json").read_bytes() == before


def test_duplicate_ids_rejected_even_after_prior_commit(batch):
    d, kw = batch
    call("manifesto_commit", **kw)
    ids = json.loads(NEW_IDS)
    res = call("manifesto_commit", **kw, new_ids=json.dumps(ids + ids))
    assert res.get("code") == "ID.DUPLICATE", res
    assert not load_manifesto(d).id_registry


@pytest.mark.parametrize("second", ["export_final", "manifesto_commit"])
def test_committed_receipt_conflict_blocks_second_write(batch, second):
    d, kw = batch
    call("manifesto_commit", **kw)
    before = (d / "manifesto.json").read_bytes()
    changed = CLOZE_CSV.replace("{{c1::ELA}}", "{{c1::ELA}} e {{c2::controle}}")
    gate = call("validate_batch", batch_id="B01", qa_csv=QA_CSV, cloze_csv=changed)
    kw.update(cloze_csv=changed, receipt_hash=gate["content_hash"])
    res = call(second, **kw)
    assert res.get("code") == "EXPORT.BATCH_CONFLICT", res
    assert (d / "manifesto.json").read_bytes() == before
    assert not (d / "exports/B01_QA.csv").exists()


def test_metadata_only_commit_preserves_expanded_count(batch):
    d, kw = batch
    call("manifesto_commit", **kw, new_ids=NEW_IDS)
    call("manifesto_commit", **kw, new_tags="ELA::Nova")
    assert load_manifesto(d).batches["B01"].cards == 2


def test_proven_export_rollback_leaves_clean_journal(batch, monkeypatch):
    import anki_mcp.tools.pipeline as pipeline

    d, kw = batch

    def fail(*args, **kwargs):
        raise OSError("simulated save failure")

    monkeypatch.setattr(pipeline, "save_manifesto", fail)
    assert call("export_final", **kw)["status"] == "ERROR"
    assert not (d / "exports/B01_QA.csv").exists()
    assert recover_journal(d)["status"] == "CLEAN"
