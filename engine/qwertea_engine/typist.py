"""Reference implementation of the client's cursor rules, producing raw key events.

Used by tests and by the synthetic-user simulator. It mirrors
`client/src/session.rs` (`LessonRun`): every printable key advances the cursor,
backspace moves back, Esc toggles a pause marked by a marker pair, and the
lesson ends on the key-down that passes the final character.
"""

from __future__ import annotations

import sqlite3
from dataclasses import dataclass, field

from .db import KeyEvent
from .layouts import Layout

BACKSPACE = "\b"
ESC = "\x1b"
BACKSPACE_CODE = 14
ESC_CODE = 1


@dataclass
class Typist:
    text: str
    layout: Layout
    lesson_id: int = 1
    events: list[KeyEvent] = field(default_factory=list)
    cursor: int = 0
    t_ns: int = 0
    paused: bool = False

    @property
    def finished(self) -> bool:
        return self.cursor >= len(self.text)

    def _code(self, ch: str) -> int:
        k = self.layout.lookup(ch)
        return k.code if k else 0

    def _push(
        self, t: int, code: int, down: bool, ch: str | None, bs: bool = False, marker: bool = False
    ) -> None:
        self.events.append(
            KeyEvent(
                lesson_id=self.lesson_id,
                seq=len(self.events),
                t_ns=t,
                t_kernel_ns=None,
                scancode=code,
                is_down=down,
                produced_char=ch,
                is_backspace=bs,
                is_pause_marker=marker,
                target_idx=self.cursor,
            )
        )

    def press(self, key: str, gap_ms: float = 150.0, dwell_ms: float = 80.0) -> None:
        """Press and release `key` (a character, BACKSPACE or ESC) after `gap_ms`."""
        if self.finished:
            return
        self.t_ns += int(gap_ms * 1e6)
        down_t = self.t_ns
        if key == ESC:
            self._push(down_t, ESC_CODE, True, None)
            self._push(down_t, 0, not self.paused, None, marker=True)
            self.paused = not self.paused
            self._push(down_t + int(dwell_ms * 1e6), ESC_CODE, False, None)
            return
        code = BACKSPACE_CODE if key == BACKSPACE else self._code(key)
        if key == BACKSPACE:
            self._push(down_t, code, True, None, bs=True)
            if not self.paused and self.cursor > 0:
                self.cursor -= 1
        else:
            self._push(down_t, code, True, key)
            if not self.paused:
                self.cursor += 1
        self._push(down_t + int(dwell_ms * 1e6), code, False, None)

    def wait(self, ms: float) -> None:
        self.t_ns += int(ms * 1e6)

    def type_string(self, s: str, gap_ms: float = 150.0) -> None:
        for ch in s:
            self.press(ch, gap_ms)


def complete_lesson(
    conn: sqlite3.Connection,
    lesson_id: int,
    events: list[KeyEvent],
    *,
    kind: str = "lesson",
    finished_at: str | None = None,
) -> None:
    """Store a typed lesson exactly as the client does (one transaction). Tests/simulator only."""
    from . import db

    now = finished_at or db.utc_now()
    with db.transaction(conn):
        sid = conn.execute(
            "INSERT INTO sessions (started_at_utc, ended_at_utc, kind, layout, "
            "timestamp_source, client_version) "
            "VALUES (?, ?, ?, 'qwerty-us', 'winit', 'sim')",
            (now, now, kind),
        ).lastrowid
        conn.executemany(
            "INSERT INTO key_events VALUES (?,?,?,?,?,?,?,?,?,?)",
            [
                (
                    lesson_id,
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
                for e in events
            ],
        )
        conn.execute(
            "UPDATE lessons SET status='done', session_id=?, finished_at_utc=? WHERE id=?",
            (sid, now, lesson_id),
        )
