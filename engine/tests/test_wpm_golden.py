"""The engine's lesson timing matches the hand-worked golden lesson that the
app's Rust test also checks (testdata/wpm_golden.json, decision D27)."""

from __future__ import annotations

import json

from qwertea_engine.db import KeyEvent
from qwertea_engine.derive import lesson_timing
from qwertea_engine.paths import REPO_ROOT


def test_engine_timing_matches_golden_lesson() -> None:
    g = json.loads((REPO_ROOT / "testdata" / "wpm_golden.json").read_text())
    events = [KeyEvent(lesson_id=1, **e) for e in g["events"]]
    t = lesson_timing(g["text"], events)
    x = g["expected"]
    assert abs(t.seconds - x["seconds"]) < 1e-9
    assert t.correct == x["correct"]
    assert t.keystrokes == x["keystrokes"]
    wpm = t.correct / 5 / (t.seconds / 60)
    raw = t.keystrokes / 5 / (t.seconds / 60)
    assert abs(wpm - x["wpm"]) < 1e-9
    assert abs(raw - x["raw_wpm"]) < 1e-9


def test_stop_on_error_lesson_counts_only_accepted_keys() -> None:
    # "ab" typed a, x (blocked), x (blocked), b: cursor recorded on each event.
    def ev(seq: int, t_ms: int, ch: str, idx: int) -> KeyEvent:
        return KeyEvent(1, seq, t_ms * 1_000_000, None, 1, True, ch, False, False, idx)

    events = [ev(0, 100, "a", 0), ev(1, 200, "x", 1), ev(2, 300, "x", 1), ev(3, 400, "b", 1)]
    t = lesson_timing("ab", events)
    assert t.correct == 2 and t.keystrokes == 4
    assert abs(t.seconds - 0.3) < 1e-9
