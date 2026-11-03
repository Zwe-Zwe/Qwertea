"""Prose corpus: sentences, word frequencies and n-gram frequencies."""

from __future__ import annotations

import gzip
import math
from collections import Counter
from dataclasses import dataclass, field
from functools import cache
from pathlib import Path

from .items import ngrams
from .paths import corpus_dir


@dataclass
class Corpus:
    sentences: list[str]
    word_freq: dict[str, int]
    words: list[str] = field(init=False)
    bigram_freq: Counter[str] = field(init=False)
    trigram_freq: Counter[str] = field(init=False)
    key_freq: Counter[str] = field(init=False)
    _index: dict[str, list[int]] | None = field(default=None, init=False, repr=False)

    def __post_init__(self) -> None:
        # Purely alphabetic words, most frequent first.
        self.words = [w for w in self.word_freq if w.isalpha()]
        self.bigram_freq = Counter()
        self.trigram_freq = Counter()
        self.key_freq = Counter()
        for w, c in self.word_freq.items():
            padded = f" {w} "
            for g in ngrams(padded, 2):
                self.bigram_freq[g] += c
            for g in ngrams(padded, 3):
                self.trigram_freq[g] += c
            for ch in w:
                self.key_freq[ch] += c

    @classmethod
    def load(cls, directory: Path | None = None) -> Corpus:
        return _load(directory or corpus_dir())

    def frequency(self, kind: str, value: str) -> float:
        """Relative real-text frequency in (0, 1], log-scaled so rare items still count."""
        table = {
            "bigram": self.bigram_freq,
            "trigram": self.trigram_freq,
            "key": self.key_freq,
        }.get(kind)
        if kind == "word":
            c = self.word_freq.get(value, 0)
            top = max(self.word_freq.values(), default=1)
        elif table is not None:
            c = table.get(value.lower(), 0)
            top = max(table.values(), default=1)
        else:
            return 0.0
        return math.log1p(c) / math.log1p(top)

    def top(self, kind: str, n: int, letters_only: bool = True) -> list[str]:
        table = self.bigram_freq if kind == "bigram" else self.trigram_freq
        out = []
        for g, _ in table.most_common():
            if letters_only and not g.isalpha():
                continue
            out.append(g)
            if len(out) == n:
                break
        return out

    def sentence_index(self) -> dict[str, list[int]]:
        """Trigram -> sentence ids (built lazily)."""
        if self._index is None:
            idx: dict[str, list[int]] = {}
            for i, s in enumerate(self.sentences):
                for g in set(ngrams(s, 3)):
                    idx.setdefault(g, []).append(i)
            self._index = idx
        return self._index


@cache
def _load(directory: Path) -> Corpus:
    with gzip.open(directory / "sentences.txt.gz", "rt", encoding="ascii") as f:
        sentences = [line.rstrip("\n") for line in f if line.strip()]
    freq: dict[str, int] = {}
    with (directory / "wordfreq.tsv").open(encoding="ascii") as f:
        for line in f:
            w, c = line.rstrip("\n").split("\t")
            freq[w] = int(c)
    return Corpus(sentences, freq)
