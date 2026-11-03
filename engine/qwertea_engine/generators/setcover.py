"""Greedy set cover for calibration text.

Repeatedly pick the candidate (word or short phrase) that adds the most
still-needed samples per character, with a diversity penalty so the same
word is not picked over and over. Lazy evaluation keeps it fast: a
candidate's gain can only shrink as needs are met, so a stale heap score is
an upper bound and only the top of the heap needs re-scoring.
"""

from __future__ import annotations

import heapq
from collections import Counter
from collections.abc import Callable, Iterable


def gain(units: Counter[str], need: Counter[str]) -> int:
    return sum(min(c, need[u]) for u, c in units.items() if need[u] > 0)


def greedy_cover(
    candidates: Iterable[str],
    need: Counter[str],
    units_of: Callable[[str], Counter[str]],
    *,
    budget_chars: int,
    diversity: float = 0.5,
    max_uses: int = 3,
) -> list[str]:
    """Choose candidates until `need` is met or `budget_chars` is spent.

    `need` is updated in place (it holds the remaining shortfall afterwards).
    Score = gain / (len + 1) * diversity ** uses.
    """
    units = {c: units_of(c) for c in dict.fromkeys(candidates)}
    uses: Counter[str] = Counter()
    heap = [(-gain(u, need) / (len(c) + 1), c) for c, u in units.items()]
    heap = [h for h in heap if h[0] < 0]
    heapq.heapify(heap)
    chosen: list[str] = []
    spent = 0
    while heap and spent < budget_chars:
        _, cand = heapq.heappop(heap)
        if uses[cand] >= max_uses:
            continue
        g = gain(units[cand], need)
        score = g / (len(cand) + 1) * diversity ** uses[cand]
        if score <= 0:
            continue
        if heap and score < -heap[0][0] - 1e-12:
            heapq.heappush(heap, (-score, cand))  # stale: re-queue with the fresh score
            continue
        chosen.append(cand)
        spent += len(cand) + 1
        uses[cand] += 1
        for u, c in units[cand].items():
            if need[u] > 0:
                need[u] = max(0, need[u] - c)
        new = gain(units[cand], need) / (len(cand) + 1) * diversity ** uses[cand]
        if new > 0:
            heapq.heappush(heap, (-new, cand))
    return chosen
