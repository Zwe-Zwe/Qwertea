from __future__ import annotations

from pathlib import Path

import pytest
from hypothesis import given, settings
from hypothesis import strategies as st

from qwertea_engine import db
from qwertea_engine.derive import derive_and_store, derive_lesson, rebuild_all
from qwertea_engine.layouts import Layout
from qwertea_engine.typist import BACKSPACE, ESC, Typist

LAYOUT = Layout.load("qwerty-us")


def run(text: str, keys: str, gap: float = 150.0) -> list:  # type: ignore[type-arg]
    t = Typist(text, LAYOUT)
    t.type_string(keys, gap)
    return derive_lesson(text, t.events, LAYOUT)


def first_errors(ks: list) -> dict[int, str | None]:  # type: ignore[type-arg]
    return {k.target_idx: k.error_type for k in ks if k.is_first_attempt}


def test_clean_typing_has_no_errors_and_timings() -> None:
    ks = run("the cat", "the cat")
    assert len(ks) == 7
    assert all(k.error_type is None for k in ks)
    assert ks[0].flight_ms is None, "first keystroke has no flight"
    assert all(k.flight_ms == pytest.approx(150.0) for k in ks[1:])
    assert all(k.dwell_ms == pytest.approx(80.0) for k in ks)
    assert ks[1].prev1 == "t" and ks[2].prev2 == "t"
    assert (ks[0].finger, ks[0].hand, ks[0].row) == ("l_index", "left", 1)


def test_substitution() -> None:
    errs = first_errors(run("cat", "cpt"))
    assert errs == {0: None, 1: "substitution", 2: None}


def test_adjacent_slip() -> None:
    errs = first_errors(run("fun", "gun"))  # g is next to f
    assert errs[0] == "adjacent_slip"


def test_transposition_labels_both_positions() -> None:
    errs = first_errors(run("form", "from"))
    assert errs == {0: None, 1: "transposition", 2: "transposition", 3: None}


def test_omission() -> None:
    # "abcd" typed as "acd" then the user keeps going.
    errs = first_errors(run("abcde", "acde"))
    assert errs[1] == "omission"


def test_insertion() -> None:
    errs = first_errors(run("abc", "axbc"))
    assert errs[1] == "insertion"


def test_correction_tracking() -> None:
    text = "cats"
    t = Typist(text, LAYOUT)
    t.type_string("cp")  # wrong at 1
    t.press(BACKSPACE, 300)
    t.type_string("ats")
    ks = derive_lesson(text, t.events, LAYOUT)
    err = next(k for k in ks if k.error_type)
    assert err.target_idx == 1 and err.corrected == 1
    assert err.time_to_notice_ms == pytest.approx(300.0)
    assert err.time_to_correct_ms == pytest.approx(450.0)
    retry = [k for k in ks if k.target_idx == 1 and not k.is_first_attempt]
    assert len(retry) == 1 and retry[0].error_type is None
    # Flight of the retyped key is measured from the backspace.
    assert retry[0].flight_ms == pytest.approx(150.0)


def test_immediate_correction_is_not_an_insertion() -> None:
    text = "cats"
    t = Typist(text, LAYOUT)
    t.type_string("cp")  # p is not next to a
    t.press(BACKSPACE)
    t.type_string("ats")
    ks = derive_lesson(text, t.events, LAYOUT)
    assert first_errors(ks)[1] == "substitution"
    t = Typist(text, LAYOUT)
    t.type_string("cs")  # s is next to a
    t.press(BACKSPACE)
    t.type_string("ats")
    assert first_errors(derive_lesson(text, t.events, LAYOUT))[1] == "adjacent_slip"


def test_uncorrected_error() -> None:
    ks = run("cats", "cpts")
    err = next(k for k in ks if k.error_type)
    assert err.corrected == 0 and err.time_to_correct_ms is None
    assert err.post_error_slowdown_ms == pytest.approx(0.0)


def test_backspace_chain() -> None:
    text = "abcd"
    t = Typist(text, LAYOUT)
    t.type_string("axy")
    t.press(BACKSPACE)
    t.press(BACKSPACE)
    t.type_string("bcd")
    ks = derive_lesson(text, t.events, LAYOUT)
    assert first_errors(ks) == {0: None, 1: "substitution", 2: "substitution", 3: None}
    assert sum(1 for k in ks if k.corrected == 1) == 2


def test_pause_time_is_excluded() -> None:
    text = "abc"
    t = Typist(text, LAYOUT)
    t.press("a")
    t.press(ESC, 100)
    t.press("b", 50)  # pressed while paused: logged but not a keystroke
    t.wait(10_000)
    t.press(ESC, 100)
    t.press("b", 100)
    t.press("c")
    ks = derive_lesson(text, t.events, LAYOUT)
    assert [k.typed for k in ks] == ["a", "b", "c"]
    # Flight a -> b counts only active time: 100 ms to the pause + 100 ms after it.
    assert ks[1].flight_ms == pytest.approx(200.0)


def test_non_text_keys_are_ignored() -> None:
    text = "ab"
    t = Typist(text, LAYOUT)
    t.press("a")
    # A shift press and release (no produced char) between keystrokes.
    t._push(t.t_ns + 50_000_000, 42, True, None)
    t._push(t.t_ns + 60_000_000, 42, False, None)
    t.press("b")
    ks = derive_lesson(text, t.events, LAYOUT)
    assert len(ks) == 2
    assert ks[1].flight_ms == pytest.approx(150.0)


def test_kernel_stamps_preferred_when_requested() -> None:
    text = "ab"
    t = Typist(text, LAYOUT)
    t.type_string("ab")
    ev = [
        db.KeyEvent(**{**e.__dict__, "t_kernel_ns": e.t_ns - (2_000_000 if e.seq < 2 else 0)})
        for e in t.events
    ]
    ks = derive_lesson(text, ev, LAYOUT, prefer_kernel=True)
    assert ks[1].flight_ms == pytest.approx(152.0)


typed_keys = st.lists(
    st.one_of(st.sampled_from(list("abcd ")), st.just(BACKSPACE)), min_size=1, max_size=40
)


@settings(max_examples=60, deadline=None)
@given(text=st.text(alphabet="abcd ", min_size=1, max_size=20), keys=typed_keys)
def test_derivation_is_deterministic(text: str, keys: list[str]) -> None:
    t = Typist(text, LAYOUT)
    for k in keys:
        t.press(k)
    a = derive_lesson(text, t.events, LAYOUT)
    b = derive_lesson(text, list(t.events), LAYOUT)
    assert a == b
    # Exactly one first attempt per position reached.
    firsts = [k.target_idx for k in a if k.is_first_attempt]
    assert len(firsts) == len(set(firsts))


def _store_lesson(conn: object, text: str, keys: str) -> int:
    import sqlite3

    assert isinstance(conn, sqlite3.Connection)
    lid = db.insert_pending_lesson(conn, text=text, phase="drill")
    sid = conn.execute(
        "INSERT INTO sessions (started_at_utc, kind, layout, timestamp_source) "
        "VALUES (?, 'lesson', 'qwerty-us', 'winit')",
        (db.utc_now(),),
    ).lastrowid
    t = Typist(text, LAYOUT, lesson_id=lid)
    t.type_string(keys)
    conn.executemany(
        "INSERT INTO key_events VALUES (?,?,?,?,?,?,?,?,?,?)",
        [
            (
                e.lesson_id,
                e.seq,
                e.t_ns,
                e.t_kernel_ns,
                e.scancode,
                int(e.is_down),
                e.produced_char,
                int(e.is_backspace),
                int(e.is_pause_marker),
                e.target_idx,
            )
            for e in t.events
        ],
    )
    conn.execute(
        "UPDATE lessons SET status='done', session_id=?, finished_at_utc=? WHERE id=?",
        (sid, db.utc_now(), lid),
    )
    return lid


def test_store_is_idempotent_and_rebuild_matches(tmp_path: Path) -> None:
    conn = db.connect(tmp_path / "q.db")
    a = _store_lesson(conn, "the quick fox", "the quixk fox")
    b = _store_lesson(conn, "form a line", "from a line")
    derive_and_store(conn, a, LAYOUT)
    derive_and_store(conn, a, LAYOUT)
    derive_and_store(conn, b, LAYOUT)
    incremental = conn.execute("SELECT * FROM keystrokes ORDER BY lesson_id, seq").fetchall()
    rebuild_all(conn, LAYOUT)
    rebuilt = conn.execute("SELECT * FROM keystrokes ORDER BY lesson_id, seq").fetchall()
    assert [tuple(r) for r in incremental] == [tuple(r) for r in rebuilt]
    assert len(rebuilt) == 13 + 11
