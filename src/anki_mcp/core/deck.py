"""core/deck.py — atomic deck I/O with file locking.

Provides:
  DeckHandle   — resolved deck directory + config + file paths
  load_manifesto(deck_dir) → Manifesto
  save_manifesto(deck_dir, manifesto) → None  (atomic temp+replace)
  load_deck_config(deck_dir) → dict
"""

from __future__ import annotations

import json
import os
import tempfile
import threading
import time
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from typing import Any

from filelock import FileLock, Timeout

from anki_mcp.models import Manifesto

_MANIFESTO_JSON = "manifesto.json"
_MANIFESTO_MD = "manifesto.md"
_DECK_CONFIG = ".anki-workflow.json"
_LOCK_SUFFIX = ".anki-workflow.lock"
_LOCK_TIMEOUT_S = 5
JOURNAL_NAME = ".export-journal.json"


def journal_path(deck_dir: str | Path) -> Path:
    """Return path to the write-ahead journal file."""
    return Path(deck_dir) / JOURNAL_NAME


def write_journal(deck_dir: str | Path, stage: str, **extra: Any) -> None:
    """Write write-ahead journal entry atomically."""
    data = {
        "stage": stage,
        "pid": os.getpid(),
        "timestamp": time.time(),
        **extra,
    }
    atomic_write(journal_path(deck_dir), json.dumps(data, indent=2))


def read_journal(deck_dir: str | Path) -> dict[str, Any] | None:
    """Read journal entry if present on disk, or None."""
    p = journal_path(deck_dir)
    if not p.is_file():
        return None
    try:
        return json.loads(p.read_text(encoding="utf-8"))  # type: ignore[no-any-return]
    except Exception:
        return {"stage": "corrupt", "path": str(p)}


def clear_journal(deck_dir: str | Path) -> None:
    """Remove journal file if present."""
    p = journal_path(deck_dir)
    try:
        p.unlink(missing_ok=True)
    except OSError:
        pass


def recover_journal(deck_dir: str | Path) -> dict[str, Any]:
    """Inspect journal and return CLEAN or STALE recovery recommendation."""
    j = read_journal(deck_dir)
    if not j:
        return {"status": "CLEAN"}
    return {
        "status": "STALE",
        "journal": j,
        "action": "Inspect manifesto.json vs CSVs on disk (deck_audit) and re-run export if needed.",
    }


@contextmanager
def transaction(
    deck_dir: str | Path,
    *,
    require_manifesto: bool = True,
) -> Iterator[Manifesto]:
    """Single transactional writer for the manifesto.

    Holds the deck lock for the whole read-modify-write cycle and writes a
    write-ahead journal so a crash mid-transaction can be detected/recovered.
    Yields the loaded Manifesto; on clean exit, clears the journal.
    On exception, preserves the journal for recovery and re-raises.
    """
    deck = Path(deck_dir)
    with deck_lock(deck):
        m = load_manifesto(deck) if require_manifesto else None
        write_journal(deck, "open")
        try:
            yield m  # type: ignore[misc]
        except BaseException:
            # Only an explicit caller-confirmed rollback permits removing evidence.
            # "open" alone proves nothing: some transaction writers do not update stages.
            if (read_journal(deck) or {}).get("stage") == "rolled_back":
                clear_journal(deck)
            raise
        else:
            clear_journal(deck)


def _lock_path(deck_dir: Path) -> Path:
    return deck_dir / _LOCK_SUFFIX


# Re-entrant deck lock. A transaction (e.g. export_final) holds the lock for its whole
# duration; nested save_manifesto() calls inside it must not try to re-acquire a
# second FileLock on the same file (that would deadlock until timeout).
_held = threading.local()


def _held_set() -> set[str]:
    if not hasattr(_held, "paths"):
        _held.paths = set()
    return _held.paths  # type: ignore[no-any-return]


@contextmanager
def deck_lock(deck_dir: str | Path) -> Iterator[None]:
    """Hold the deck lock for the duration of a block (re-entrant within a thread)."""
    key = str(Path(deck_dir).resolve())
    if key in _held_set():
        yield
        return
    lock_path = str(_lock_path(Path(deck_dir)))
    try:
        lock = FileLock(lock_path, timeout=_LOCK_TIMEOUT_S)
        with lock:
            _held_set().add(key)
            try:
                yield
            finally:
                _held_set().discard(key)
    except Timeout as exc:
        raise RuntimeError(
            f"GOV.LOCK: could not acquire deck lock at {lock_path} "
            f"within {_LOCK_TIMEOUT_S}s. Another process may be writing."
        ) from exc


def save_manifesto(deck_dir: str | Path, manifesto: Manifesto) -> None:
    """Write manifesto.json atomically (temp file + os.replace), under the deck lock."""
    deck_dir = Path(deck_dir)
    with deck_lock(deck_dir):
        json_text = manifesto.model_dump_json(indent=2, exclude_none=False)
        _atomic_write(deck_dir / _MANIFESTO_JSON, json_text)


def resolve_deck(deck_dir: str) -> Path:
    p = Path(deck_dir).expanduser().resolve()
    if not p.is_dir():
        raise FileNotFoundError(f"Deck directory not found: {p}")
    return p


def load_deck_config(deck_dir: str | Path) -> dict[str, Any]:
    p = Path(deck_dir) / _DECK_CONFIG
    if p.exists():
        return json.loads(p.read_text(encoding="utf-8"))
    return {}


def load_manifesto(deck_dir: str | Path) -> Manifesto:
    p = Path(deck_dir) / _MANIFESTO_JSON
    if not p.exists():
        raise FileNotFoundError(
            f"manifesto.json not found in {deck_dir}. Run manifesto_init first."
        )
    data = json.loads(p.read_text(encoding="utf-8"))
    return Manifesto.model_validate(data)


def atomic_write(path: Path | str, content: str) -> None:
    """Write *content* to *path* atomically via temp file with fsync."""
    path = Path(path)
    dir_ = path.parent
    dir_.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=dir_, prefix=".anki_tmp_", suffix=path.suffix or ".tmp")
    try:
        os.write(fd, content.encode("utf-8"))
        os.fsync(fd)
        os.close(fd)
        # Windows-resilient atomic replace with exponential backoff against Defender/search indexers
        max_attempts = 5
        delay = 0.02
        for attempt in range(max_attempts):
            try:
                os.replace(tmp, path)
                break
            except OSError:
                if attempt == max_attempts - 1:
                    raise
                time.sleep(delay)
                delay *= 2
    except Exception:
        try:
            os.close(fd)
        except OSError:
            pass
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise


_atomic_write = atomic_write
