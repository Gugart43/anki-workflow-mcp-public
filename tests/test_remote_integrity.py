import asyncio

import pytest

from anki_mcp.server import mcp
from tests.test_flow_e2e import CLOZE_CSV, QA_CSV


def call(name, **kwargs):
    return asyncio.run(mcp.get_tool(name)).fn(**kwargs)


@pytest.mark.parametrize(
    "batch_id,topic_slug",
    [
        ("../escape", None),
        ("/absolute", None),
        ("B01", "../../escape"),
        ("B01", "x\\..\\evil"),
        ("B01", "."),
    ],
)
def test_export_rejects_path_components_before_any_write(tmp_path, batch_id, topic_slug):
    d = tmp_path / "deck"
    d.mkdir()
    call(
        "manifesto_init",
        deck_dir=str(d),
        campo_estudo="Medicina::Neurologia::ELA",
        deck_slug="ela",
        taxonomy_root="ELA",
    )
    result = call("validate_batch", batch_id="B01", qa_csv=QA_CSV, cloze_csv=CLOZE_CSV)
    before = {p.relative_to(tmp_path): p.read_bytes() for p in tmp_path.rglob("*") if p.is_file()}
    exported = call(
        "export_final",
        deck_dir=str(d),
        batch_id=batch_id,
        topic_slug=topic_slug,
        qa_csv=QA_CSV,
        cloze_csv=CLOZE_CSV,
        receipt_hash=result["content_hash"],
    )
    assert exported.get("code") == "ARGUMENT.INVALID", exported
    assert before == {
        p.relative_to(tmp_path): p.read_bytes() for p in tmp_path.rglob("*") if p.is_file()
    }


def test_export_records_expanded_anki_cards(tmp_path):
    from anki_mcp.core.deck import load_manifesto

    d = tmp_path / "deck"
    d.mkdir()
    call(
        "manifesto_init",
        deck_dir=str(d),
        campo_estudo="Medicina::Neurologia::ELA",
        deck_slug="ela",
        taxonomy_root="ELA",
    )
    cl = CLOZE_CSV.replace("{{c1::ELA}}", "{{c1::ELA}} e {{c2::controle}}")
    gate = call("validate_batch", batch_id="B01", qa_csv=QA_CSV, cloze_csv=cl)
    result = call(
        "export_final",
        deck_dir=str(d),
        batch_id="B01",
        qa_csv=QA_CSV,
        cloze_csv=cl,
        receipt_hash=gate["content_hash"],
    )
    assert result["status"] == "EXPORTED", result
    assert result["cloze_cards"] == 2
    assert result["cloze_notes"] == 1
    assert result["total_cards"] == 3
    assert load_manifesto(d).batches["B01"].cards == 3
    # Manifesto totals count registered notes, not expanded Anki cards.
    assert result["registered_notes"] == 0
    assert result["unregistered_notes"] == 2
    assert result["ledger_complete"] is False


@pytest.mark.parametrize("commit_first", [True, False])
def test_metadata_commit_and_file_export_coexist(tmp_path, commit_first):
    from anki_mcp.core.deck import load_manifesto

    d = tmp_path / "deck"
    d.mkdir()
    call(
        "manifesto_init",
        deck_dir=str(d),
        campo_estudo="Medicina::Neurologia::ELA",
        deck_slug="ela",
        taxonomy_root="ELA",
    )
    gate = call("validate_batch", batch_id="B01", qa_csv=QA_CSV, cloze_csv=CLOZE_CSV)
    kw = {
        "deck_dir": str(d),
        "batch_id": "B01",
        "qa_csv": QA_CSV,
        "cloze_csv": CLOZE_CSV,
        "receipt_hash": gate["content_hash"],
    }
    if commit_first:
        assert call("manifesto_commit", **kw, new_tags="ELA::Nova")["status"] == "COMMITTED"
        result = call("export_final", **kw)
    else:
        result = call("export_final", **kw)
        assert call("manifesto_commit", **kw, new_tags="ELA::Nova")["status"] == "COMMITTED"
    assert result["status"] == "EXPORTED", result
    assert (d / "exports/B01_QA.csv").read_text(encoding="utf-8") == QA_CSV
    assert "Nova" in load_manifesto(d).taxonomy.nodes
    assert load_manifesto(d).batches["B01"].state == "exported"
    # A second metadata payload for the same CSV must not be silently discarded.
    follow = call("manifesto_commit", **kw, new_tags="ELA::Outra")
    assert follow["status"] == "COMMITTED", follow
    assert "Outra" in load_manifesto(d).taxonomy.nodes
    assert call("manifesto_commit", **kw, new_tags="ELA::Outra")["status"] == "ALREADY_EXPORTED"


def test_failed_rollback_preserves_recovery_journal(tmp_path, monkeypatch):
    from pathlib import Path

    import anki_mcp.tools.pipeline as pipeline
    from anki_mcp.core.deck import recover_journal

    d = tmp_path / "deck"
    d.mkdir()
    call(
        "manifesto_init",
        deck_dir=str(d),
        campo_estudo="Medicina::Neurologia::ELA",
        deck_slug="ela",
        taxonomy_root="ELA",
    )
    gate = call("validate_batch", batch_id="B01", qa_csv=QA_CSV, cloze_csv=CLOZE_CSV)
    real_unlink = Path.unlink

    def fail_rollback(self, *args, **kwargs):
        if self.name == "B01_QA.csv":
            raise PermissionError("rollback blocked")
        return real_unlink(self, *args, **kwargs)

    def fail_save(*args, **kwargs):
        raise OSError("manifesto save failed")

    monkeypatch.setattr(pipeline, "save_manifesto", fail_save)
    monkeypatch.setattr(Path, "unlink", fail_rollback)
    result = call(
        "export_final",
        deck_dir=str(d),
        batch_id="B01",
        qa_csv=QA_CSV,
        cloze_csv=CLOZE_CSV,
        receipt_hash=gate["content_hash"],
    )
    assert result["status"] == "ERROR"
    assert (d / "exports/B01_QA.csv").exists()
    recovery = recover_journal(d)
    assert recovery["status"] == "STALE", recovery
    assert recovery["journal"]["stage"] == "rollback_failed"
    assert recovery["journal"]["batch_id"] == "B01"


def test_metadata_commit_then_export_accepts_same_registered_ids(tmp_path):
    from tests.test_flow_e2e import NEW_IDS

    d = tmp_path / "deck"
    d.mkdir()
    call(
        "manifesto_init",
        deck_dir=str(d),
        campo_estudo="Medicina::Neurologia::ELA",
        deck_slug="ela",
        taxonomy_root="ELA",
    )
    gate = call("validate_batch", batch_id="B01", qa_csv=QA_CSV, cloze_csv=CLOZE_CSV)
    kw = {
        "deck_dir": str(d),
        "batch_id": "B01",
        "qa_csv": QA_CSV,
        "cloze_csv": CLOZE_CSV,
        "receipt_hash": gate["content_hash"],
    }
    assert call("manifesto_commit", **kw, new_ids=NEW_IDS)["status"] == "COMMITTED"
    exported = call("export_final", **kw, new_ids_json=NEW_IDS)
    assert exported["status"] == "EXPORTED", exported
    assert exported["registered_notes"] == 1
