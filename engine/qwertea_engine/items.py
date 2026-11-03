"""Items: the units the weakness model scores (prose mode).

* `key`: one character as typed, case-sensitive (shifted forms are distinct).
* `bigram` / `trigram`: consecutive characters, lower-cased so that "Th" and
  "th" pool their evidence (the shift cost lives in the `key` items).
* `word`: a lower-cased alphabetic word (apostrophes allowed).
"""

from __future__ import annotations

import re
from collections import Counter
from dataclasses import dataclass

KINDS = ("key", "bigram", "trigram", "word")
WORD_RE = re.compile(r"[A-Za-z]+(?:'[A-Za-z]+)*")


@dataclass(frozen=True, order=True)
class Item:
    mode: str  # 'prose'
    kind: str  # 'key' | 'bigram' | 'trigram' | 'word'
    value: str


def ngrams(text: str, n: int) -> list[str]:
    """Lower-cased character n-grams that contain no newline or tab."""
    low = text.lower()
    return [low[i : i + n] for i in range(len(low) - n + 1) if not set(low[i : i + n]) & {"\n", "\t"}]


def words(text: str) -> list[str]:
    return [w.lower() for w in WORD_RE.findall(text)]


def item_counts(text: str, mode: str = "prose") -> Counter[Item]:
    c: Counter[Item] = Counter()
    c.update(Item(mode, "key", ch) for ch in text)
    c.update(Item(mode, "bigram", g) for g in ngrams(text, 2))
    c.update(Item(mode, "trigram", g) for g in ngrams(text, 3))
    c.update(Item(mode, "word", w) for w in words(text))
    return c


def extract_items(text: str, mode: str = "prose") -> list[Item]:
    return sorted(item_counts(text, mode))


def context_items(text: str, i: int, mode: str = "prose") -> list[Item]:
    """Items whose *last* character is `text[i]` (key, bigram, trigram)."""
    out = [Item(mode, "key", text[i])]
    low = text.lower()
    for n, kind in ((2, "bigram"), (3, "trigram")):
        if i - n + 1 >= 0:
            g = low[i - n + 1 : i + 1]
            if not set(g) & {"\n", "\t"}:
                out.append(Item(mode, kind, g))
    return out


def word_spans(text: str) -> list[tuple[int, int, str]]:
    """(start, end, lower-cased word) for every word in `text`."""
    return [(m.start(), m.end(), m.group(0).lower()) for m in WORD_RE.finditer(text)]
