"""Keyboard layouts loaded from `engine/data/layouts/<name>.toml`.

The finger/hand/row map lives in data, never in code.
"""

from __future__ import annotations

import math
import tomllib
from dataclasses import dataclass
from pathlib import Path

from .paths import LAYOUTS_DIR


@dataclass(frozen=True)
class KeyInfo:
    """Where a character is typed: the physical key plus whether Shift is needed."""

    char: str
    base: str
    shifted: bool
    row: int
    x: float
    finger: str
    hand: str
    code: int


class Layout:
    def __init__(self, name: str, keys: list[KeyInfo]) -> None:
        self.name = name
        self._by_char = {k.char: k for k in keys}
        self._by_code: dict[int, KeyInfo] = {}
        for k in keys:
            if not k.shifted:
                self._by_code.setdefault(k.code, k)

    @classmethod
    def load(cls, name: str, directory: Path = LAYOUTS_DIR) -> Layout:
        path = directory / f"{name}.toml"
        with path.open("rb") as f:
            data = tomllib.load(f)
        keys: list[KeyInfo] = []
        for k in data["keys"]:
            common = {
                "base": k["base"],
                "row": int(k["row"]),
                "x": float(k["x"]),
                "finger": k["finger"],
                "hand": k["hand"],
                "code": int(k["code"]),
            }
            keys.append(KeyInfo(char=k["base"], shifted=False, **common))
            if "shifted" in k:
                keys.append(KeyInfo(char=k["shifted"], shifted=True, **common))
        return cls(str(data.get("name", name)), keys)

    def __contains__(self, char: object) -> bool:
        return char in self._by_char

    def chars(self) -> list[str]:
        return list(self._by_char)

    def lookup(self, char: str) -> KeyInfo | None:
        return self._by_char.get(char)

    def by_code(self, code: int) -> KeyInfo | None:
        return self._by_code.get(code)

    def adjacent(self, a: str, b: str) -> bool:
        """True if the keys producing `a` and `b` are physical neighbours (not the same key)."""
        ka, kb = self.lookup(a), self.lookup(b)
        if ka is None or kb is None or ka.code == kb.code:
            return False
        if abs(ka.row - kb.row) > 1 or 4 in (ka.row, kb.row):
            return False
        return math.isclose(abs(ka.x - kb.x), 1.0) or (
            ka.row != kb.row and abs(ka.x - kb.x) <= 0.75
        )
