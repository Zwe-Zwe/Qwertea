"""Target selection: Thompson sampling over weakness, with holdouts.

benefit = max(sampled weakness, 0) * real_text_frequency * (1 + bonus * uncertainty)

The sampled weakness replaces the posterior-mean error rate by a draw from
the item's Beta posterior, so uncertain items are explored and confident
weak items are exploited. Holdout items are never drilled: they are the
control group that shows whether drilling helps (section 8.4).
"""

from __future__ import annotations

import random
from dataclasses import dataclass

from .corpus import Corpus
from .items import Item
from .model.baseline import ItemScore

# Which kinds are drilled, with a preference weight (n-grams are the richest signal).
KIND_WEIGHT = {"bigram": 1.0, "trigram": 0.9, "key": 0.8, "word": 0.5}
MIN_N = {"key": 8, "bigram": 4, "trigram": 3, "word": 2}


@dataclass(frozen=True)
class Selection:
    item: Item
    benefit: float
    sampled_weakness: float


def _overlaps(a: Item, b: Item) -> bool:
    """Too similar to drill together (one contains the other, or same final key)."""
    x, y = a.value.lower(), b.value.lower()
    if a.kind == "word" or b.kind == "word":
        return x == y
    return x in y or y in x


def thompson_select(
    scores: list[ItemScore],
    corpus: Corpus,
    rng: random.Random,
    *,
    k: int = 4,
    w_err: float,
    w_lat: float,
    err_rate: float,
    err_scale: float,
    bonus: float,
    holdouts: set[Item],
    exclude: set[Item] | None = None,
) -> list[Selection]:
    exclude = exclude or set()
    cands: list[Selection] = []
    for s in scores:
        it = s.item
        if it in holdouts or it in exclude or it.kind not in KIND_WEIGHT or s.n < MIN_N[it.kind]:
            continue
        if it.kind == "key" and not it.value.strip():
            continue  # space / newline are not drilled on their own
        freq = corpus.frequency(it.kind, it.value)
        if freq <= 0:
            continue
        # Thompson draw: replace the posterior mean by a sample.
        p = rng.betavariate(s.alpha, s.beta)
        w = w_err * (p - err_rate) / err_scale + w_lat * s.z_lat
        benefit = max(w, 0.0) * freq * (1.0 + bonus * s.uncertainty) * KIND_WEIGHT[it.kind]
        if benefit > 0:
            cands.append(Selection(it, benefit, w))
    cands.sort(key=lambda c: -c.benefit)
    chosen: list[Selection] = []
    for c in cands:
        if any(_overlaps(c.item, x.item) for x in chosen):
            continue
        chosen.append(c)
        if len(chosen) == k:
            break
    return chosen


def pick_holdouts(
    scores: list[ItemScore], fraction: float, rng: random.Random, pool_size: int = 60
) -> set[Item]:
    """Reserve ~`fraction` of the weak n-gram items as never-drilled controls."""
    weak = [
        s
        for s in scores
        if s.item.kind in ("bigram", "trigram") and s.n >= MIN_N[s.item.kind] and s.weakness > 0
    ]
    weak.sort(key=lambda s: -s.weakness)
    pool = weak[:pool_size]
    n = max(1, round(len(pool) * fraction)) if pool else 0
    return {s.item for s in rng.sample(pool, n)} if n else set()
