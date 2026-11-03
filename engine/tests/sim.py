"""Synthetic user with a known per-item error/latency profile (section 11).

Planted weaknesses:
  * error-prone bigrams (`WEAK_BIGRAMS`): extra error probability on the second key;
  * slow keys (`SLOW_KEYS`): longer flight times.
Everything else: 2 % errors and ~180 ms log-normal flights.
"""

from __future__ import annotations

import math
import random
import sqlite3

from qwertea_engine import db
from qwertea_engine.layouts import Layout
from qwertea_engine.typist import BACKSPACE, Typist, complete_lesson

WEAK_BIGRAMS = {"ou": 0.30, "ve": 0.30, "ck": 0.35}
SLOW_KEYS = {"p": 2.2, "b": 2.0}
BASE_ERR = 0.02
BASE_MS = 180.0


class SimUser:
    def __init__(self, layout: Layout, seed: int = 0) -> None:
        self.layout = layout
        self.rng = random.Random(seed)

    def p_error(self, prev: str, ch: str) -> float:
        return BASE_ERR + WEAK_BIGRAMS.get((prev + ch).lower(), 0.0)

    def gap(self, ch: str) -> float:
        f = SLOW_KEYS.get(ch.lower(), 1.0)
        return BASE_MS * f * math.exp(self.rng.gauss(0, 0.2))

    def wrong_char(self, ch: str) -> str:
        k = self.layout.lookup(ch)
        neigh = [c for c in "abcdefghijklmnopqrstuvwxyz" if k and self.layout.adjacent(c, ch)]
        return self.rng.choice(neigh or ["x" if ch != "x" else "z"])

    def type_text(self, text: str, lesson_id: int) -> Typist:
        t = Typist(text, self.layout, lesson_id=lesson_id)
        i = 0
        while not t.finished:
            ch = text[i]
            prev = text[i - 1] if i else " "
            if self.rng.random() < self.p_error(prev, ch) and ch.strip():
                t.press(self.wrong_char(ch), self.gap(ch))
                if t.finished:
                    break
                if self.rng.random() < 0.7:  # notices and fixes it
                    t.press(BACKSPACE, 250)
                    t.press(ch, self.gap(ch))
            else:
                t.press(ch, self.gap(ch))
            i = t.cursor
        return t


def run_pending(conn: sqlite3.Connection, user: SimUser, limit: int) -> int:
    """Type up to `limit` pending lessons in queue order, like the client."""
    done = 0
    for _ in range(limit):
        row = conn.execute(
            "SELECT id, text FROM lessons WHERE status='pending' ORDER BY order_idx, id LIMIT 1"
        ).fetchone()
        if row is None:
            break
        conn.execute("UPDATE lessons SET status='running' WHERE id=?", (row["id"],))
        t = user.type_text(row["text"], row["id"])
        complete_lesson(conn, row["id"], t.events)
        done += 1
    return done


def fresh_db(path: str) -> sqlite3.Connection:
    return db.connect(path)
