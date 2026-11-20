"""The explicit legacy entrypoint warns only on stderr."""

from anki_mcp import server


def test_legacy_entrypoint_warns_without_polluting_stdout(monkeypatch, capsys):
    calls = []
    monkeypatch.setattr(server.mcp, "run", lambda: calls.append(True))
    server.main()
    captured = capsys.readouterr()
    assert captured.out == ""
    assert "deprecated" in captured.err.lower()
    assert "anki-workflow-mcp serve" in captured.err
    assert calls == [True]
