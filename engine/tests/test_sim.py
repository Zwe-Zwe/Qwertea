"""End-to-end: calibration with a synthetic user recovers the planted weaknesses,
and adaptive lessons then target them."""

from __future__ import annotations

import json

import pytest

from qwertea_engine.items import Item
from qwertea_engine.layouts import Layout
from qwertea_engine.pipeline import Engine, Settings

from .sim import SLOW_KEYS, WEAK_BIGRAMS, SimUser, fresh_db, run_pending


@pytest.fixture(scope="module")
def calibrated(tmp_path_factory: pytest.TempPathFactory) -> tuple[Engine, SimUser]:
    path = tmp_path_factory.mktemp("sim") / "q.db"
    conn = fresh_db(str(path))
    eng = Engine(conn, Settings(sittings=3), seed=7)
    first = eng.run_once()
    assert first.calibration_planned > 40
    user = SimUser(Layout.load("qwerty-us"), seed=3)
    typed = run_pending(conn, user, 500)
    assert typed == first.calibration_planned
    res = eng.run_once()
    assert res.refit and res.derived == typed
    return eng, user


def test_model_recovers_planted_weak_bigrams(calibrated: tuple[Engine, SimUser]) -> None:
    eng, _ = calibrated
    rows = eng.conn.execute(
        "SELECT item FROM item_stats WHERE item_kind='bigram' AND n >= 10 "
        "ORDER BY weakness DESC LIMIT 10"
    ).fetchall()
    top = [r["item"] for r in rows]
    for g in WEAK_BIGRAMS:
        assert g in top, (g, top)


def test_model_recovers_slow_keys(calibrated: tuple[Engine, SimUser]) -> None:
    eng, _ = calibrated
    rows = eng.conn.execute(
        "SELECT item FROM item_stats WHERE item_kind='key' AND n >= 20 "
        "ORDER BY lat_median_log DESC LIMIT 4"
    ).fetchall()
    top = [r["item"] for r in rows]
    for k in SLOW_KEYS:
        assert k in top, (k, top)


def test_level_and_snapshot(calibrated: tuple[Engine, SimUser]) -> None:
    eng, _ = calibrated
    snap = eng.conn.execute(
        "SELECT level_json, behavior_json FROM model_snapshots ORDER BY id DESC"
    ).fetchone()
    lvl = json.loads(snap["level_json"])
    beh = json.loads(snap["behavior_json"])
    # ~180 ms per key ~ 60 WPM nominal; errors and slow keys pull it down.
    assert 35 < lvl["wpm"] < 75
    assert 0.8 < lvl["accuracy"] < 1.0
    assert lvl["speed_probe_wpm"] is not None
    assert beh["error_types"]


def test_adaptive_lessons_target_weaknesses(calibrated: tuple[Engine, SimUser]) -> None:
    eng, _ = calibrated
    rows = eng.conn.execute(
        "SELECT text, targets_json, generator, meta_json FROM lessons "
        "WHERE status='pending' ORDER BY order_idx"
    ).fetchall()
    assert len(rows) >= 3
    targeted: set[str] = set()
    for r in rows:
        assert r["generator"] == "prose/adaptive"
        assert 100 <= len(r["text"]) <= 330, len(r["text"])
        for t in json.loads(r["targets_json"]):
            targeted.add(t["item"])
    planted = set(WEAK_BIGRAMS)
    hits = {g for g in planted if any(g in t or t in g for t in targeted)}
    assert len(hits) >= 2, (targeted, planted)
    text = " ".join(r["text"] for r in rows).lower()
    # Planted bigrams are far more frequent than in ordinary prose (~0.5-1.5 %).
    rate = sum(text.count(g) for g in planted) / len(text)
    assert rate > 0.04, rate


def test_holdouts_are_never_drilled(calibrated: tuple[Engine, SimUser]) -> None:
    eng, user = calibrated
    run_pending(eng.conn, user, 6)
    eng.run_once()
    holdouts = {
        (r["item_kind"], r["item"])
        for r in eng.conn.execute("SELECT item_kind, item FROM item_stats WHERE is_holdout=1")
    }
    assert holdouts
    for r in eng.conn.execute("SELECT targets_json FROM lessons WHERE generator='prose/adaptive'"):
        for t in json.loads(r["targets_json"]):
            assert (t["kind"], t["item"]) not in holdouts


def test_queue_is_refilled_and_stale_lessons_replaced(calibrated: tuple[Engine, SimUser]) -> None:
    eng, user = calibrated
    before = {r[0] for r in eng.conn.execute("SELECT id FROM lessons WHERE status='pending'")}
    run_pending(eng.conn, user, 1)
    res = eng.run_once()
    after = {r[0] for r in eng.conn.execute("SELECT id FROM lessons WHERE status='pending'")}
    assert len(after) >= 3
    assert res.removed_stale >= 1
    assert not (before & after) or res.removed_stale < len(before)
    # Idempotent: nothing new to do.
    again = eng.run_once()
    assert again.derived == 0 and again.queued == 0 and not again.refit


def test_probe_every_ten_adaptive_lessons(calibrated: tuple[Engine, SimUser]) -> None:
    eng, user = calibrated
    for _ in range(12):
        run_pending(eng.conn, user, 1)
        eng.run_once()
    n = eng.conn.execute("SELECT COUNT(*) FROM lessons WHERE generator='prose/probe'").fetchone()[0]
    assert n >= 1


def test_rebuild_equals_incremental(calibrated: tuple[Engine, SimUser]) -> None:
    eng, _ = calibrated
    snap = lambda: eng.conn.execute(  # noqa: E731
        "SELECT * FROM keystrokes ORDER BY lesson_id, seq"
    ).fetchall()
    before = [tuple(r) for r in snap()]
    eng.rebuild()
    after = [tuple(r) for r in snap()]
    assert before == after
    assert Item("prose", "bigram", "ou")  # sanity: Item is hashable/importable
