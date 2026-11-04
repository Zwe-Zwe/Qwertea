from __future__ import annotations

import json
import random
from pathlib import Path

from qwertea_engine import db
from qwertea_engine.generators.prose import apply_content
from qwertea_engine.layouts import Layout
from qwertea_engine.pipeline import Engine, ProfilePrefs, Settings

from .sim import SimUser, fresh_db, run_pending


def test_prefs_parsing() -> None:
    p = ProfilePrefs.from_settings(
        {"lesson_length": "short", "capitals": "false", "punctuation": "0", "numbers": "true"}
    )
    assert p.lesson_chars == (90, 160)
    assert not p.capitals and not p.punctuation and p.numbers
    custom = ProfilePrefs.from_settings({"lesson_length": "custom", "lesson_chars": "500"})
    assert custom.lesson_chars == (500, 500)
    assert ProfilePrefs.from_settings({}).lesson_chars is None


def test_apply_content_switches() -> None:
    rng = random.Random(1)
    t = "Well, I don't know! \"Yes,\" said Emma; 42 times."
    assert apply_content(t, False, True, False, rng) == 'well, i don\'t know! "yes," said emma; times.'
    assert apply_content(t, True, False, False, rng) == "Well I don't know Yes said Emma times"
    with_numbers = apply_content(" ".join(["word"] * 120), True, True, True, random.Random(3))
    assert any(w.isdigit() for w in with_numbers.split())


def test_engine_rebrews_queue_when_settings_change(tmp_path: Path) -> None:
    conn = fresh_db(str(tmp_path / "q.db"))
    eng = Engine(conn, Settings(sittings=1), seed=5)
    eng.run_once()
    user = SimUser(Layout.load("qwerty-us"), seed=2)
    run_pending(conn, user, 500)
    eng.run_once()
    conn.executemany(
        "INSERT OR REPLACE INTO profile_settings VALUES (?, ?)",
        [("lesson_length", "short"), ("capitals", "false"), ("punctuation", "false")],
    )
    res = eng.run_once()
    assert res.removed_stale >= 3 and res.queued >= 3
    rows = conn.execute(
        "SELECT text, meta_json FROM lessons WHERE status='pending' AND generator='prose/adaptive'"
    ).fetchall()
    assert len(rows) >= 3
    for r in rows:
        assert len(r["text"]) <= 175, len(r["text"])
        assert r["text"] == r["text"].lower()
        assert not any(c in r["text"] for c in ',.;:!?"')
        assert json.loads(r["meta_json"])["settings"] == eng.prefs().fingerprint()
    assert db.SCHEMA_VERSION == 3
