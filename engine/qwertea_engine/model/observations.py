"""Turn derived keystrokes into per-item and per-lesson observations.

Cleaning rules (section 8.1): only `done`, non-discarded lessons of the mode;
only first attempts label errors; latencies above `think_pause_ms` (and the
first keystroke of each lesson, which has no flight) are dropped from latency
statistics but the keystroke still counts for errors.
"""

from __future__ import annotations

import math
import sqlite3
import statistics
from collections import defaultdict
from dataclasses import dataclass, field

from ..items import Item, context_items, word_spans


@dataclass
class ItemObs:
    n: int = 0
    errors: int = 0
    log_lat: list[float] = field(default_factory=list)
    last_seen: str | None = None
    # (lesson order, error 0/1) in time order, for trends and holdout comparison.
    history: list[tuple[int, int]] = field(default_factory=list)


@dataclass
class LessonObs:
    lesson_id: int
    phase: str
    finished_at: str
    chars: int
    first_attempts: int
    first_errors: int
    active_ms: float
    correct_chars: int
    targets_json: str | None
    error_types: dict[str, int]

    @property
    def accuracy(self) -> float:
        return 1.0 - self.first_errors / self.first_attempts if self.first_attempts else 1.0

    @property
    def wpm(self) -> float:
        return (
            (self.correct_chars / 5.0) / (self.active_ms / 60000.0) if self.active_ms > 0 else 0.0
        )


@dataclass
class Observations:
    items: dict[Item, ItemObs]
    lessons: list[LessonObs]
    flights: list[float]  # all valid flights (ms)
    slowdowns: list[float]
    session_pos: list[tuple[float, float]]  # (session_pos, log flight)


def collect(
    conn: sqlite3.Connection, mode: str = "prose", think_pause_ms: float = 2500.0
) -> Observations:
    lessons = conn.execute(
        """SELECT id, phase, text, finished_at_utc, targets_json FROM lessons
           WHERE status='done' AND is_discarded=0 AND mode=?
             AND EXISTS (SELECT 1 FROM keystrokes k WHERE k.lesson_id = lessons.id)
           ORDER BY finished_at_utc, id""",
        (mode,),
    ).fetchall()
    items: dict[Item, ItemObs] = defaultdict(ItemObs)
    lesson_obs: list[LessonObs] = []
    flights: list[float] = []
    slowdowns: list[float] = []
    session_pos: list[tuple[float, float]] = []
    for order, les in enumerate(lessons):
        text = les["text"].replace("\r\n", "\n").replace("\r", "\n")
        rows = conn.execute(
            """SELECT target_idx, typed, expected, is_first_attempt, error_type, flight_ms,
                      post_error_slowdown_ms, session_pos
               FROM keystrokes WHERE lesson_id=? ORDER BY seq""",
            (les["id"],),
        ).fetchall()
        first: dict[int, tuple[int, float | None]] = {}
        active = 0.0
        etypes: dict[str, int] = defaultdict(int)
        for r in rows:
            f = r["flight_ms"]
            if f is not None:
                active += min(f, think_pause_ms)
            if r["is_first_attempt"] and r["target_idx"] < len(text):
                err = int(r["error_type"] is not None)
                lat = math.log(f) if f is not None and 0 < f <= think_pause_ms else None
                first[r["target_idx"]] = (err, lat)
                if r["error_type"]:
                    etypes[r["error_type"]] += 1
                if lat is not None:
                    flights.append(f)
                    if r["session_pos"] is not None:
                        session_pos.append((r["session_pos"], lat))
                if r["post_error_slowdown_ms"] is not None:
                    slowdowns.append(r["post_error_slowdown_ms"])
        # Final state of each position decides "correct characters" for WPM.
        final: dict[int, str] = {}
        for r in rows:
            final[r["target_idx"]] = r["typed"]
        correct = sum(1 for i, ch in final.items() if i < len(text) and ch == text[i])
        for i, (err, lat) in first.items():
            for it in context_items(text, i):
                o = items[it]
                o.n += 1
                o.errors += err
                if lat is not None:
                    o.log_lat.append(lat)
                o.last_seen = les["finished_at_utc"]
                o.history.append((order, err))
        for start, end, w in word_spans(text):
            obs = [first[i] for i in range(start, end) if i in first]
            if len(obs) != end - start:
                continue
            it = Item("prose", "word", w)
            o = items[it]
            o.n += 1
            err = int(any(e for e, _ in obs))
            o.errors += err
            lats = [lt for _, lt in obs[1:] if lt is not None]
            if lats:
                o.log_lat.append(statistics.fmean(lats))
            o.last_seen = les["finished_at_utc"]
            o.history.append((order, err))
        lesson_obs.append(
            LessonObs(
                lesson_id=les["id"],
                phase=les["phase"],
                finished_at=les["finished_at_utc"],
                chars=len(text),
                first_attempts=len(first),
                first_errors=sum(e for e, _ in first.values()),
                active_ms=active,
                correct_chars=correct,
                targets_json=les["targets_json"],
                error_types=dict(etypes),
            )
        )
    return Observations(dict(items), lesson_obs, flights, slowdowns, session_pos)
