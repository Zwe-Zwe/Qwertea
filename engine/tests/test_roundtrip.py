"""M1 round trip: the engine queues a lesson, the Rust client code runs and
flushes it, and the engine reads back the raw events.

Uses the `roundtrip_fixture` binary (built by `make build-client` or
`cargo build --bin roundtrip_fixture`). Set `QWERTEA_FIXTURE_BIN` to override
its path. Skipped when the binary is missing.
"""

from __future__ import annotations

import itertools
import os
import subprocess
from pathlib import Path

import pytest

from qwertea_engine import db
from qwertea_engine.paths import REPO_ROOT

BIN = Path(
    os.environ.get(
        "QWERTEA_FIXTURE_BIN", REPO_ROOT / "client" / "target" / "debug" / "roundtrip_fixture"
    )
)

pytestmark = pytest.mark.skipif(not BIN.exists(), reason=f"{BIN} not built")


def replay(text: str, events: list[db.KeyEvent]) -> str:
    """Re-apply cursor semantics to raw events; must reproduce the target text."""
    typed: list[str] = []
    paused = False
    for e in events:
        if e.is_pause_marker:
            paused = e.is_down
            continue
        if not e.is_down or paused:
            continue
        if e.is_backspace:
            if typed:
                typed.pop()
        elif e.produced_char is not None:
            assert e.target_idx == len(typed)
            typed.append(e.produced_char)
    return "".join(typed)


def run_fixture(path: Path) -> int:
    out = subprocess.run([str(BIN), str(path)], check=True, capture_output=True, text=True)
    return int(out.stdout.strip())


def test_client_events_are_readable_by_engine(tmp_path: Path) -> None:
    path = tmp_path / "q.db"
    conn = db.connect(path)
    text = "The cat, 42.\nok"
    lesson_id = db.insert_pending_lesson(conn, text=text, phase="natural")
    conn.close()

    assert run_fixture(path) == lesson_id

    conn = db.connect(path)
    lesson = db.get_lesson(conn, lesson_id)
    assert lesson is not None
    assert lesson.status == "done"
    assert lesson.session_id is not None
    session = conn.execute(
        "SELECT kind, mode, timestamp_source, started_at_utc, ended_at_utc, client_version "
        "FROM sessions WHERE id = ?",
        (lesson.session_id,),
    ).fetchone()
    assert session["kind"] == "calibration"  # 'natural' is a calibration phase
    assert session["mode"] == "prose"
    assert session["timestamp_source"] == "winit"
    assert session["ended_at_utc"] >= session["started_at_utc"]
    assert session["client_version"]

    events = db.load_key_events(conn, lesson_id)
    assert [e.seq for e in events] == list(range(len(events)))
    assert all(a.t_ns <= b.t_ns for a, b in itertools.pairwise(events))
    # Every key has a down and an up event; pause markers come as a pair.
    markers = [e for e in events if e.is_pause_marker]
    assert [m.is_down for m in markers] == [True, False]
    keys = [e for e in events if not e.is_pause_marker]
    assert sum(e.is_down for e in keys) == sum(not e.is_down for e in keys)
    # One wrong key ('#') and one backspace were scripted.
    assert sum(e.is_backspace for e in events) == 1
    assert any(e.produced_char == "#" for e in events)
    # Non-ASCII-safe text column and newline survive the trip.
    assert any(e.produced_char == "\n" for e in events)
    # Kernel stamps are stored when present and NULL for markers.
    assert all(e.t_kernel_ns == e.t_ns - 1_500_000 for e in keys)
    assert all(m.t_kernel_ns is None for m in markers)
    assert replay(text, events) == text


def test_running_lesson_is_aborted_on_client_start(tmp_path: Path) -> None:
    path = tmp_path / "q.db"
    conn = db.connect(path)
    crashed = db.insert_pending_lesson(conn, text="abc", phase="drill")
    nxt = db.insert_pending_lesson(conn, text="xyz", phase="drill")
    conn.execute("UPDATE lessons SET status='running' WHERE id=?", (crashed,))
    conn.close()

    assert run_fixture(path) == nxt
    conn = db.connect(path)
    crashed_lesson = db.get_lesson(conn, crashed)
    assert crashed_lesson is not None and crashed_lesson.status == "aborted"
    assert db.load_key_events(conn, crashed) == []


def test_no_pending_lesson_exits_2(tmp_path: Path) -> None:
    path = tmp_path / "q.db"
    db.connect(path).close()
    res = subprocess.run([str(BIN), str(path)], capture_output=True, text=True)
    assert res.returncode == 2
