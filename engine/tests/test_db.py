from __future__ import annotations

from pathlib import Path

from qwertea_engine import db


def test_schema_applied_once(tmp_path: Path) -> None:
    path = tmp_path / "q.db"
    conn = db.connect(path)
    assert db.schema_version(conn) == db.SCHEMA_VERSION
    assert db.ensure_schema(conn) is False
    conn.close()
    conn = db.connect(path)
    n = conn.execute("SELECT COUNT(*) FROM schema_version").fetchone()[0]
    assert n == 1


def test_wal_and_foreign_keys(tmp_path: Path) -> None:
    conn = db.connect(tmp_path / "q.db")
    assert conn.execute("PRAGMA journal_mode").fetchone()[0] == "wal"
    assert conn.execute("PRAGMA foreign_keys").fetchone()[0] == 1


def test_pending_lessons_get_increasing_order(tmp_path: Path) -> None:
    conn = db.connect(tmp_path / "q.db")
    a = db.insert_pending_lesson(conn, text="one", phase="drill")
    b = db.insert_pending_lesson(conn, text="two", phase="drill")
    la, lb = db.get_lesson(conn, a), db.get_lesson(conn, b)
    assert la is not None and lb is not None
    assert lb.order_idx == la.order_idx + 1
    assert la.status == "pending"
    assert db.count_pending(conn) == 2


def test_utc_format() -> None:
    s = db.utc_now()
    assert len(s) == len("2026-11-03T17:30:00.999Z")
    assert s.endswith("Z") and s[10] == "T"
