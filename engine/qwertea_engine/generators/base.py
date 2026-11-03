"""The `LessonGenerator` protocol every mode implements."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Protocol

from ..items import Item


@dataclass(frozen=True)
class Difficulty:
    """Knobs the level model turns to keep accuracy inside the target band."""

    length: int = 220  # characters
    max_word_len: int = 9
    drill_share: float = 0.6  # share of the lesson spent on targeted words
    capitals: bool = False
    punctuation: bool = False


@dataclass
class Target:
    item: Item
    reason: str  # 'weak' | 'review' | 'coverage'
    score: float = 0.0


@dataclass
class LessonSpec:
    """A lesson to queue: what the engine writes as one `pending` row."""

    phase: str
    text: str
    targets: list[dict[str, Any]] = field(default_factory=list)
    meta: dict[str, Any] = field(default_factory=dict)
    is_discarded: bool = False


class LessonGenerator(Protocol):
    mode: str
    version: str

    def item_kinds(self) -> list[str]: ...

    def extract_items(self, text: str) -> list[Item]: ...

    def calibration_plan(self) -> list[LessonSpec]: ...

    def generate(self, targets: list[Target], budget: int, difficulty: Difficulty) -> LessonSpec: ...
