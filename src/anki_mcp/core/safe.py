"""core/safe.py — safety decorator for FastMCP tools.

Prevents unhandled exceptions from leaking raw tracebacks to the MCP client,
normalizing errors into structured diagnostics.
"""

from __future__ import annotations

import functools
import logging
import re
from collections.abc import Callable
from typing import Any

logger = logging.getLogger("anki_mcp")

_CODED_MSG = re.compile(r"^([A-Z][A-Z_]*(?:\.[A-Z_]+)+):")


def error_result(code: str, message: str, action: str, **extra: Any) -> dict[str, Any]:
    """Canonical error payload {status, code, message, action}; extra keys never replace them."""
    return {**extra, "status": "ERROR", "code": code, "message": message, "action": action}


def safe_tool(fn: Callable[..., Any]) -> Callable[..., Any]:
    """Convert any exception raised by the decorated tool into the canonical error payload."""

    @functools.wraps(fn)
    def wrapper(*args: Any, **kwargs: Any) -> Any:
        try:
            return fn(*args, **kwargs)
        except FileNotFoundError as exc:
            return error_result(
                "DECK.NOT_FOUND",
                str(exc),
                "Ensure the directory exists and contains manifesto.json (or run manifesto_init).",
            )
        except ValueError as exc:
            # json.JSONDecodeError is a ValueError subclass: malformed JSON args land here.
            return error_result(
                "ARGUMENT.INVALID",
                str(exc),
                "Check parameters passed to this tool (JSON arguments must be valid).",
            )
        except RuntimeError as exc:
            msg = str(exc)
            # Deliberate failures are raised as "CODE.NAME: message" (e.g. GOV.LOCK, HIERARCHY.CORRUPT).
            m = _CODED_MSG.match(msg)
            if m and m.group(1) == "GOV.LOCK":
                return error_result(
                    "GOV.LOCK",
                    msg,
                    "Another process holds the deck lock; retry in a few seconds.",
                )
            if m:
                return error_result(
                    m.group(1),
                    msg,
                    "Inspect the file named in the message and restore or remove it.",
                )
            return error_result("RUNTIME.ERROR", msg, "Operation could not complete.")
        except Exception as exc:
            logger.exception("Unexpected error in tool %s", fn.__name__)
            return error_result(
                "INTERNAL.UNEXPECTED",
                f"{type(exc).__name__}: {exc!s}",
                "Report this unexpected error or verify input formats.",
            )

    return wrapper


def guard_tools(mcp: Any) -> None:
    """Route every tool registered on *mcp* through :func:`safe_tool`.

    Protocol-boundary guarantee (revision 03, A3): no tool can leak a raw exception, even one
    registered without an explicit ``@safe_tool``. Idempotent with explicit decorators.
    """
    original = mcp.tool

    def guarded(*args: Any, **kwargs: Any) -> Any:
        if len(args) == 1 and not kwargs and callable(args[0]):
            return original(safe_tool(args[0]))
        decorator = original(*args, **kwargs)

        def apply(fn: Callable[..., Any]) -> Any:
            return decorator(safe_tool(fn))

        return apply

    mcp.tool = guarded
