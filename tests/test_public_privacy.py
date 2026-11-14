"""Public distribution must not read ambient credential files."""

from pathlib import Path

from anki_mcp.checks import jev_triage


def test_secret_discovery_only_uses_explicit_process_environment(monkeypatch):
    monkeypatch.delenv("KNOWLINE_API_KEY", raising=False)
    monkeypatch.delenv("TYPESAFE_API_KEY", raising=False)
    monkeypatch.delenv("OPENROUTER_API_KEY", raising=False)
    reads = []

    def record_probe(self):
        reads.append(str(self))
        return False

    monkeypatch.setattr(Path, "is_file", record_probe)
    assert jev_triage.get_typesafe_api_key() is None
    assert reads == []


def test_optional_provider_has_no_private_endpoint_default(monkeypatch):
    import importlib

    with monkeypatch.context() as context:
        context.delenv("KNOWLINE_SYSTEMONE_URL", raising=False)
        importlib.reload(jev_triage)
        assert jev_triage.KNOWLINE_ENDPOINT == ""
    importlib.reload(jev_triage)


def test_explicit_environment_key_is_supported(monkeypatch):
    monkeypatch.setenv("TYPESAFE_API_KEY", "synthetic-test-key")
    assert jev_triage._discover_env_secret("TYPESAFE_API_KEY") == "synthetic-test-key"
