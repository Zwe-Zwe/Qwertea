"""Scripted typist for headless timing checks (X11 only).

Focuses the `timing_check` window and types a fixed pangram through the XTEST
extension with randomised key-down and key-up gaps, then presses Esc.
Used to produce the Xvfb rows in `docs/timing-validation.md`:

    Xvfb :99 &
    DISPLAY=:99 uv run --with python-xlib python client/scripts/xtest_typist.py 3 &
    DISPLAY=:99 client/target/release/timing_check --present mailbox

XTEST events are generated inside the X server, so this measures the client
side only (X server -> winit -> stamp), not keyboard or kernel latency.
"""

from __future__ import annotations

import random
import sys
import time

from Xlib import XK, X, display
from Xlib.ext import xtest

TITLE = "Qwertea timing check"


def find(win):  # type: ignore[no-untyped-def]
    try:
        if win.get_wm_name() == TITLE:
            return win
    except Exception:
        pass
    for child in win.query_tree().children:
        found = find(child)
        if found:
            return found
    return None


def tap(d: display.Display, keycode: int, hold: float) -> None:
    xtest.fake_input(d, X.KeyPress, keycode)
    d.sync()
    time.sleep(hold)
    xtest.fake_input(d, X.KeyRelease, keycode)
    d.sync()


def main() -> None:
    d = display.Display()
    time.sleep(float(sys.argv[1]) if len(sys.argv) > 1 else 3.0)
    win = find(d.screen().root)
    if win is None:
        sys.exit(f"window {TITLE!r} not found")
    win.set_input_focus(X.RevertToParent, X.CurrentTime)
    d.sync()
    rng = random.Random(7)
    for ch in "the quick brown fox jumps over the lazy dog " * 6:
        keycode = d.keysym_to_keycode(XK.string_to_keysym("space" if ch == " " else ch))
        tap(d, keycode, rng.uniform(0.03, 0.09))
        time.sleep(rng.uniform(0.03, 0.15))
    tap(d, d.keysym_to_keycode(XK.string_to_keysym("Escape")), 0.05)


if __name__ == "__main__":
    main()
