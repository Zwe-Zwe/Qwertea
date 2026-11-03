"""Spaced review: a Leitner ladder for items that improved after drilling.

State is derived, never stored separately, so `rebuild` reproduces it:
  * an item is *drilled* when a done lesson listed it as a target with
    reason 'weak' (from `lessons.targets_json`);
  * it is *improved* when its posterior error rate is at or below the user's
    global rate and its latency z-score is below `LAT_OK`;
  * its box is the number of review lessons that included it since it was
    last drilled; the next review is due `INTERVALS[box]` after the latest of
    those (or after the last drill).
"""

from __future__ import annotations

import json
import sqlite3
from dataclasses import dataclass
from datetime import datetime, timedelta

from .items import Item
from .model.baseline import ItemScore

INTERVALS = [timedelta(days=d) for d in (1, 3, 7, 14, 30, 60)]
LAT_OK = 0.25


@dataclass(frozen=True)
class ReviewState:
    last_drilled: str | None
    next_review: str | None
    box: int


def _dt(s: str) -> datetime:
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def _fmt(d: datetime) -> str:
    return d.strftime("%Y-%m-%dT%H:%M:%S.") + f"{d.microsecond // 1000:03d}Z"


def target_history(conn: sqlite3.Connection, mode: str) -> dict[Item, list[tuple[str, str]]]:
    """item -> [(finished_at, reason)] over done lessons, oldest first."""
    hist: dict[Item, list[tuple[str, str]]] = {}
    rows = conn.execute(
        """SELECT finished_at_utc, targets_json FROM lessons
           WHERE status='done' AND mode=? AND targets_json IS NOT NULL
           ORDER BY finished_at_utc""",
        (mode,),
    ).fetchall()
    for r in rows:
        try:
            targets = json.loads(r["targets_json"])
        except (TypeError, ValueError):
            continue
        for t in targets if isinstance(targets, list) else []:
            if isinstance(t, dict) and "kind" in t and "item" in t:
                it = Item(mode, str(t["kind"]), str(t["item"]))
                hist.setdefault(it, []).append((r["finished_at_utc"], str(t.get("reason", "weak"))))
    return hist


def review_state(score: ItemScore, history: list[tuple[str, str]], err_rate: float) -> ReviewState:
    drills = [t for t, reason in history if reason == "weak"]
    if not drills:
        return ReviewState(None, None, 0)
    last_drill = drills[-1]
    reviews = [t for t, reason in history if reason == "review" and t > last_drill]
    improved = score.p_err <= err_rate and score.z_lat < LAT_OK
    if not improved:
        return ReviewState(last_drill, None, 0)
    box = min(len(reviews), len(INTERVALS) - 1)
    anchor = _dt(reviews[-1] if reviews else last_drill)
    return ReviewState(last_drill, _fmt(anchor + INTERVALS[box]), box)


def due(states: dict[Item, ReviewState], now: datetime) -> list[Item]:
    out = [
        (st.next_review, it)
        for it, st in states.items()
        if st.next_review and _dt(st.next_review) <= now
    ]
    out.sort()
    return [it for _, it in out]
