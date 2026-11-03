"""Engine-side database access.

The engine is the only writer of `keystrokes`, `item_stats`,
`model_snapshots` and new `pending` lessons. It only reads `key_events`.
"""

from __future__ import annotations

import sqlite3
from collections.abc import Iterator
from contextlib import contextmanager
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path

from .paths import SCHEMA_DIR

SCHEMA_FILE = SCHEMA_DIR / "001_init.sql"
# Numbered migrations applied after 001, in order: (version, file).
MIGRATIONS = [(2, SCHEMA_DIR / "002_lesson_meta.sql")]
SCHEMA_VERSION = MIGRATIONS[-1][0]


def utc_now() -> str:
    """ISO-8601 UTC timestamp with milliseconds, same format as the client."""
    return datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z"


def connect(path: Path | str) -> sqlite3.Connection:
    """Open the database in WAL mode with foreign keys, applying the schema if missing."""
    if str(path) != ":memory:":
        Path(path).parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(str(path), timeout=5.0, isolation_level=None)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA foreign_keys=ON")
    ensure_schema(conn)
    return conn


def ensure_schema(conn: sqlite3.Connection) -> bool:
    """Apply `001_init.sql` when `schema_version` is missing, then any newer migrations.

    Returns True if anything was applied. Each step runs in its own transaction and
    re-checks the version inside it, so the client and engine can race safely.
    """
    applied = False
    row = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_version'"
    ).fetchone()
    if row is None:
        sql = SCHEMA_FILE.read_text(encoding="utf-8")
        conn.execute("BEGIN IMMEDIATE")
        exists = conn.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_version'"
        ).fetchone()
        if exists is None:
            for stmt in _statements(sql):
                conn.execute(stmt)
            applied = True
        conn.execute("COMMIT")
    for version, path in MIGRATIONS:
        conn.execute("BEGIN IMMEDIATE")
        current = conn.execute("SELECT MAX(version) FROM schema_version").fetchone()[0]
        if current < version:
            for stmt in _statements(path.read_text(encoding="utf-8")):
                conn.execute(stmt)
            applied = True
        conn.execute("COMMIT")
    return applied


def _statements(sql: str) -> list[str]:
    """Split a schema file into statements (no semicolons inside our SQL literals)."""
    lines = [ln.split("--", 1)[0] for ln in sql.splitlines()]
    return [s.strip() for s in "\n".join(lines).split(";") if s.strip()]


def schema_version(conn: sqlite3.Connection) -> int | None:
    row = conn.execute("SELECT MAX(version) FROM schema_version").fetchone()
    return None if row is None else row[0]


@contextmanager
def transaction(conn: sqlite3.Connection) -> Iterator[sqlite3.Connection]:
    conn.execute("BEGIN IMMEDIATE")
    try:
        yield conn
    except BaseException:
        conn.execute("ROLLBACK")
        raise
    conn.execute("COMMIT")


@dataclass(frozen=True)
class KeyEvent:
    """One raw `key_events` row, as written by the client."""

    lesson_id: int
    seq: int
    t_ns: int
    t_kernel_ns: int | None
    scancode: int
    is_down: bool
    produced_char: str | None
    is_backspace: bool
    is_pause_marker: bool
    target_idx: int


@dataclass(frozen=True)
class Lesson:
    id: int
    session_id: int | None
    order_idx: int
    mode: str
    phase: str
    text: str
    status: str
    is_discarded: bool


def insert_pending_lesson(
    conn: sqlite3.Connection,
    *,
    text: str,
    phase: str,
    order_idx: int | None = None,
    mode: str = "prose",
    targets_json: str | None = None,
    is_discarded: bool = False,
    generator: str | None = None,
    generator_version: str | None = None,
    model_snapshot_id: int | None = None,
) -> int:
    """Queue a lesson. `order_idx` defaults to one past the current maximum."""
    if order_idx is None:
        row = conn.execute("SELECT COALESCE(MAX(order_idx), 0) + 1 FROM lessons").fetchone()
        order_idx = int(row[0])
    cur = conn.execute(
        """INSERT INTO lessons (order_idx, mode, phase, text, targets_json, is_discarded,
                                status, created_at_utc, generator, generator_version,
                                model_snapshot_id)
           VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?)""",
        (
            order_idx,
            mode,
            phase,
            text,
            targets_json,
            int(is_discarded),
            utc_now(),
            generator,
            generator_version,
            model_snapshot_id,
        ),
    )
    assert cur.lastrowid is not None
    return cur.lastrowid


def get_lesson(conn: sqlite3.Connection, lesson_id: int) -> Lesson | None:
    r = conn.execute(
        """SELECT id, session_id, order_idx, mode, phase, text, status, is_discarded
           FROM lessons WHERE id = ?""",
        (lesson_id,),
    ).fetchone()
    if r is None:
        return None
    return Lesson(
        id=r["id"],
        session_id=r["session_id"],
        order_idx=r["order_idx"],
        mode=r["mode"],
        phase=r["phase"],
        text=r["text"],
        status=r["status"],
        is_discarded=bool(r["is_discarded"]),
    )


def load_key_events(conn: sqlite3.Connection, lesson_id: int) -> list[KeyEvent]:
    rows = conn.execute(
        """SELECT lesson_id, seq, t_ns, t_kernel_ns, scancode, is_down, produced_char,
                  is_backspace, is_pause_marker, target_idx
           FROM key_events WHERE lesson_id = ? ORDER BY seq""",
        (lesson_id,),
    ).fetchall()
    return [
        KeyEvent(
            lesson_id=r["lesson_id"],
            seq=r["seq"],
            t_ns=r["t_ns"],
            t_kernel_ns=r["t_kernel_ns"],
            scancode=r["scancode"],
            is_down=bool(r["is_down"]),
            produced_char=r["produced_char"],
            is_backspace=bool(r["is_backspace"]),
            is_pause_marker=bool(r["is_pause_marker"]),
            target_idx=r["target_idx"],
        )
        for r in rows
    ]


def count_pending(conn: sqlite3.Connection) -> int:
    return int(conn.execute("SELECT COUNT(*) FROM lessons WHERE status='pending'").fetchone()[0])
