"""test_safe_tools.py — contract tests for tool error safety (@safe_tool)."""

from __future__ import annotations

from anki_mcp.core.safe import safe_tool


def test_safe_tool_catches_file_not_found():
    @safe_tool
    def failing_fn():
        raise FileNotFoundError("manifesto.json not found in /fake/dir")

    res = failing_fn()
    assert res["status"] == "ERROR"
    assert res["code"] == "DECK.NOT_FOUND"
    assert "manifesto.json not found" in res["message"]
    assert "action" in res


def test_safe_tool_catches_value_error():
    @safe_tool
    def bad_arg_fn():
        raise ValueError("Invalid parameter value")

    res = bad_arg_fn()
    assert res["status"] == "ERROR"
    assert res["code"] == "ARGUMENT.INVALID"
    assert "Invalid parameter" in res["message"]


def test_safe_tool_passes_success():
    @safe_tool
    def good_fn(x, y):
        return {"status": "OK", "sum": x + y}

    res = good_fn(2, 3)
    assert res["status"] == "OK"
    assert res["sum"] == 5


def test_safe_tool_catches_runtime_error():
    @safe_tool
    def runtime_fn():
        raise RuntimeError("something broke")

    res = runtime_fn()
    assert res["status"] == "ERROR"
    assert res["code"] == "RUNTIME.ERROR"


def test_safe_tool_catches_generic_exception():
    @safe_tool
    def generic_fn():
        raise TypeError("type mismatch")

    res = generic_fn()
    assert res["status"] == "ERROR"
    assert res["code"] == "INTERNAL.UNEXPECTED"
    assert "TypeError" in res["message"]


def test_safe_tool_preserves_function_name():
    @safe_tool
    def my_tool():
        pass

    assert my_tool.__name__ == "my_tool"
