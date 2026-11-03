"""Derivation: raw `key_events` -> `keystrokes` (deterministic and idempotent).

One keystroke row is produced for every text-producing key-down that the
lesson accepted: not a backspace, not a pause marker, not typed while paused.

Timing
  * Paused time is removed using the pause marker pairs (decision D5), so a
    pause never shows up as latency.
  * `flight_ms`: time since the previous keystroke or backspace key-down;
    `None` for the first keystroke of the lesson.
  * `dwell_ms`: from the key-down to the next key-up with the same scancode.
  * When the session used evdev, `t_kernel_ns` is preferred where present (D4).

Error labels (first attempt at each target position only)
  Precedence, first match wins (`expected = text[i]`, `next` = the typed
  character of the next keystroke, only when it was typed at position i+1;
  after an immediate backspace there is no `next`). The second position of a transposition
  is labelled `transposition` too.
    1. transposition  typed == text[i+1] and next == text[i]
    2. omission       typed == text[i+1] and next == text[i+2]
    3. insertion      next == text[i]          (an extra key, then back on track)
    4. adjacent_slip  typed key is a physical neighbour of the expected key
    5. substitution   anything else
"""

from __future__ import annotations

import itertools
import math
import sqlite3
import statistics
from dataclasses import astuple, dataclass
from datetime import datetime

from . import db
from .layouts import Layout

ERROR_TYPES = ("transposition", "omission", "insertion", "adjacent_slip", "substitution")


@dataclass(frozen=True)
class Keystroke:
    lesson_id: int
    seq: int
    target_idx: int
    expected: str | None
    typed: str
    is_first_attempt: int
    error_type: str | None
    dwell_ms: float | None
    flight_ms: float | None
    prev1: str | None
    prev2: str | None
    finger: str | None
    hand: str | None
    row: int | None
    corrected: int | None
    time_to_notice_ms: float | None
    time_to_correct_ms: float | None
    post_error_slowdown_ms: float | None
    session_pos: float | None


def _pause_adjuster(events: list[db.KeyEvent]) -> list[tuple[int, int]]:
    """Paused intervals [(start_ns, end_ns)] from marker pairs (an open pause runs to the end)."""
    out: list[tuple[int, int]] = []
    start: int | None = None
    for e in events:
        if not e.is_pause_marker:
            continue
        if e.is_down and start is None:
            start = e.t_ns
        elif not e.is_down and start is not None:
            out.append((start, e.t_ns))
            start = None
    if start is not None:
        out.append((start, max(e.t_ns for e in events)))
    return out


def _active_time(t: int, pauses: list[tuple[int, int]]) -> int:
    removed = 0
    for a, b in pauses:
        if t >= b:
            removed += b - a
        elif t > a:
            removed += t - a
    return t - removed


def classify_error(
    text: str, i: int, typed: str, next_typed: str | None, layout: Layout
) -> str | None:
    expected = text[i] if i < len(text) else None
    if expected is None or typed == expected:
        return None
    nxt1 = text[i + 1] if i + 1 < len(text) else None
    nxt2 = text[i + 2] if i + 2 < len(text) else None
    if typed == nxt1 and next_typed is not None and next_typed == expected:
        return "transposition"
    if typed == nxt1 and next_typed is not None and next_typed == nxt2:
        return "omission"
    if next_typed is not None and next_typed == expected:
        return "insertion"
    if layout.adjacent(typed, expected):
        return "adjacent_slip"
    return "substitution"


def derive_lesson(
    text: str,
    events: list[db.KeyEvent],
    layout: Layout,
    *,
    prefer_kernel: bool = False,
    session_offset_ns: int | None = None,
) -> list[Keystroke]:
    """Pure derivation for one lesson. `session_offset_ns`: lesson start minus session start."""
    if not events:
        return []

    def t_of(e: db.KeyEvent) -> int:
        if prefer_kernel and e.t_kernel_ns is not None:
            return e.t_kernel_ns
        return e.t_ns

    pauses = _pause_adjuster(events)
    act = [_active_time(t_of(e), pauses) for e in events]

    # Key-down -> matching key-up (same scancode, first one after it).
    up_time: dict[int, int] = {}
    open_downs: dict[int, list[int]] = {}
    for idx, e in enumerate(events):
        if e.is_pause_marker:
            continue
        if e.is_down:
            open_downs.setdefault(e.scancode, []).append(idx)
        elif open_downs.get(e.scancode):
            for d in open_downs.pop(e.scancode):
                up_time[d] = act[idx]

    # Accepted key-downs in order: keystrokes and backspaces.
    paused = False
    strokes: list[int] = []  # event indices of text key-downs
    backspaces: list[int] = []
    timeline: list[int] = []  # text + backspace key-downs, for flight
    for idx, e in enumerate(events):
        if e.is_pause_marker:
            paused = e.is_down
            continue
        if not e.is_down or paused:
            continue
        if e.is_backspace:
            backspaces.append(idx)
            timeline.append(idx)
        elif e.produced_char is not None:
            strokes.append(idx)
            timeline.append(idx)

    prev_down: dict[int, int] = {}
    for a, b in itertools.pairwise(timeline):
        prev_down[b] = a

    seen_positions: set[int] = set()
    rows: list[_Row] = []
    for k, idx in enumerate(strokes):
        e = events[idx]
        i = e.target_idx
        typed = e.produced_char or ""
        first = i not in seen_positions
        seen_positions.add(i)
        # Look-ahead labels only apply when the next keystroke moved on to i+1
        # (no correction in between): a retype after a backspace is not an insertion.
        nxt = None
        if k + 1 < len(strokes) and events[strokes[k + 1]].target_idx == i + 1:
            nxt = events[strokes[k + 1]].produced_char
        expected = text[i] if i < len(text) else None
        key = layout.lookup(expected) if expected is not None else None
        err = classify_error(text, i, typed, nxt, layout) if first else None
        if (
            first
            and err is not None
            and rows
            and rows[-1].error == "transposition"
            and rows[-1].target_idx == i - 1
            and typed == text[i - 1]
        ):
            err = "transposition"  # second half of the swap
        rows.append(
            _Row(
                idx=idx,
                seq=e.seq,
                target_idx=i,
                expected=expected,
                typed=typed,
                first=first,
                error=err,
                dwell=(up_time[idx] - act[idx]) / 1e6 if idx in up_time else None,
                flight=(act[idx] - act[prev_down[idx]]) / 1e6 if idx in prev_down else None,
                prev1=text[i - 1] if 0 < i <= len(text) else None,
                prev2=text[i - 2] if 1 < i <= len(text) else None,
                finger=key.finger if key else None,
                hand=key.hand if key else None,
                row=key.row if key else None,
            )
        )

    # Corrections: for an erroneous first attempt at position i, when did the user
    # first backspace afterwards, and when did they type the right character at i?
    flights = [r.flight for r in rows if r.flight is not None]
    median_flight = statistics.median(flights) if flights else None
    out: list[Keystroke] = []
    for k, r in enumerate(rows):
        corrected: int | None = None
        notice = fix = slowdown = None
        if r.error is not None:
            t0 = act[r.idx]
            later_bs = [b for b in backspaces if b > r.idx]
            fixes = [
                q for q in rows[k + 1 :] if q.target_idx == r.target_idx and q.typed == r.expected
            ]
            corrected = int(bool(fixes))
            if later_bs and fixes:
                notice = (act[later_bs[0]] - t0) / 1e6
            if fixes:
                fix = (act[fixes[0].idx] - t0) / 1e6
            nxt3 = [q.flight for q in rows[k + 1 : k + 4] if q.flight is not None]
            if nxt3 and median_flight is not None:
                slowdown = sum(nxt3) / len(nxt3) - median_flight
        pos = None
        if session_offset_ns is not None:
            # Fraction of an hour into the session (fatigue / within-session learning).
            pos = min(1.0, max(0.0, (session_offset_ns + act[r.idx]) / 3.6e12))
        out.append(
            Keystroke(
                lesson_id=events[0].lesson_id,
                seq=r.seq,
                target_idx=r.target_idx,
                expected=r.expected,
                typed=r.typed,
                is_first_attempt=int(r.first),
                error_type=r.error,
                dwell_ms=r.dwell,
                flight_ms=r.flight,
                prev1=r.prev1,
                prev2=r.prev2,
                finger=r.finger,
                hand=r.hand,
                row=r.row,
                corrected=corrected,
                time_to_notice_ms=notice,
                time_to_correct_ms=fix,
                post_error_slowdown_ms=slowdown,
                session_pos=pos,
            )
        )
    return out


@dataclass
class _Row:
    idx: int
    seq: int
    target_idx: int
    expected: str | None
    typed: str
    first: bool
    error: str | None
    dwell: float | None
    flight: float | None
    prev1: str | None
    prev2: str | None
    finger: str | None
    hand: str | None
    row: int | None


def _parse_utc(s: str | None) -> datetime | None:
    if not s:
        return None
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def derive_and_store(conn: sqlite3.Connection, lesson_id: int, layout: Layout) -> int:
    """(Re)derive one lesson's keystrokes. Idempotent: replaces existing rows."""
    row = conn.execute(
        """SELECT l.text, l.finished_at_utc, s.started_at_utc, s.timestamp_source
           FROM lessons l LEFT JOIN sessions s ON s.id = l.session_id WHERE l.id = ?""",
        (lesson_id,),
    ).fetchone()
    if row is None:
        return 0
    events = db.load_key_events(conn, lesson_id)
    offset = None
    fin, start = _parse_utc(row["finished_at_utc"]), _parse_utc(row["started_at_utc"])
    if fin and start and events:
        lesson_len_ns = max(e.t_ns for e in events)
        offset = int((fin - start).total_seconds() * 1e9) - lesson_len_ns
        offset = max(0, offset)
    ks = derive_lesson(
        row["text"].replace("\r\n", "\n").replace("\r", "\n"),
        events,
        layout,
        prefer_kernel=row["timestamp_source"] == "evdev",
        session_offset_ns=offset,
    )
    with db.transaction(conn):
        conn.execute("DELETE FROM keystrokes WHERE lesson_id = ?", (lesson_id,))
        conn.executemany(
            "INSERT INTO keystrokes VALUES (" + ",".join("?" * 19) + ")",
            [astuple(k) for k in ks],
        )
    return len(ks)


def underived_lessons(conn: sqlite3.Connection) -> list[int]:
    """Done lessons with raw events but no keystrokes yet."""
    rows = conn.execute(
        """SELECT l.id FROM lessons l
           WHERE l.status = 'done'
             AND EXISTS (SELECT 1 FROM key_events e WHERE e.lesson_id = l.id)
             AND NOT EXISTS (SELECT 1 FROM keystrokes k WHERE k.lesson_id = l.id)
           ORDER BY l.id"""
    ).fetchall()
    return [int(r[0]) for r in rows]


def rebuild_all(conn: sqlite3.Connection, layout: Layout) -> int:
    conn.execute("DELETE FROM keystrokes")
    ids = [
        int(r[0]) for r in conn.execute("SELECT id FROM lessons WHERE status='done' ORDER BY id")
    ]
    return sum(derive_and_store(conn, i, layout) for i in ids)


def log_ms(x: float) -> float:
    return math.log(max(x, 1.0))
