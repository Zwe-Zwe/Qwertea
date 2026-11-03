from __future__ import annotations

import string

from qwertea_engine.layouts import Layout


def test_qwerty_covers_printable_ascii() -> None:
    lay = Layout.load("qwerty-us")
    for ch in string.ascii_letters + string.digits + string.punctuation + " \n\t":
        assert ch in lay, repr(ch)


def test_finger_hand_row() -> None:
    lay = Layout.load("qwerty-us")
    f = lay.lookup("f")
    assert f is not None
    assert (f.finger, f.hand, f.row, f.shifted) == ("l_index", "left", 2, False)
    big_p = lay.lookup("P")
    assert big_p is not None
    assert (big_p.base, big_p.shifted, big_p.finger, big_p.row) == ("p", True, "r_pinky", 1)
    excl = lay.lookup("!")
    assert excl is not None and excl.base == "1" and excl.shifted
    space = lay.lookup(" ")
    assert space is not None and space.finger == "thumb" and space.row == 4


def test_evdev_codes() -> None:
    lay = Layout.load("qwerty-us")
    a, q = lay.by_code(30), lay.by_code(16)
    assert a is not None and a.char == "a"
    assert q is not None and q.char == "q"


def test_adjacency() -> None:
    lay = Layout.load("qwerty-us")
    assert lay.adjacent("f", "g")
    assert lay.adjacent("f", "r")  # row above, offset 0.25
    assert lay.adjacent("e", "s")
    assert lay.adjacent("q", "1")
    assert lay.adjacent("F", "d"), "shifted forms use the same physical key"
    assert not lay.adjacent("f", "j")
    assert not lay.adjacent("q", "s")
    assert not lay.adjacent("a", "A"), "same key"
    assert not lay.adjacent("q", "z"), "two rows apart"
    assert not lay.adjacent("v", " "), "space bar is excluded"
