"""`ProseGenerator`: calibration plans and adaptive prose lessons.

Adaptive lessons combine three sources (section 8.5):
  1. word ranking: dictionary words scored by the summed weakness of their
     n-grams, picked greedily with a diversity penalty;
  2. Markov pseudo-words: drill strings for n-grams real words rarely carry;
  3. natural sentences from the corpus that contain the targets, chosen by
     target coverage per character.
"""

from __future__ import annotations

import math
import random
import string
from collections import Counter, defaultdict
from dataclasses import dataclass
from typing import Any

from ..corpus import Corpus
from ..items import Item, extract_items, item_counts, ngrams
from ..layouts import Layout
from .base import Difficulty, LessonSpec, Target
from .setcover import greedy_cover

VERSION = "1.0"
MAX_KEYSTROKES = 12_000  # section 7.1: 8,000-12,000 keystrokes in total
PSEUDO_RESERVE = 2_000
LETTERS = string.ascii_lowercase
DIGITS = string.digits
PUNCT = string.punctuation  # 32 symbols


# --------------------------------------------------------------------------- Markov


class Markov:
    """Order-2 character model over words, forwards and backwards."""

    def __init__(self, words: list[str], weights: list[int]) -> None:
        self.fwd: dict[str, Counter[str]] = defaultdict(Counter)
        self.bwd: dict[str, Counter[str]] = defaultdict(Counter)
        for w, c in zip(words, weights, strict=True):
            p = f"^^{w}$$"
            for i in range(2, len(p)):
                self.fwd[p[i - 2 : i]][p[i]] += c
            r = f"$${w[::-1]}^^"
            for i in range(2, len(r)):
                self.bwd[r[i - 2 : i]][r[i]] += c

    @staticmethod
    def _draw(table: Counter[str], rng: random.Random) -> str:
        total = sum(table.values())
        x = rng.random() * total
        for ch, c in table.items():
            x -= c
            if x <= 0:
                return ch
        return next(iter(table))

    def word_with(self, core: str, rng: random.Random, max_len: int = 8) -> str:
        """A pronounceable-ish pseudo-word that contains `core`."""
        w = core
        # Extend left.
        for _ in range(rng.randint(0, 2)):
            key = (w[:2][::-1]) if len(w) >= 2 else ("$" + w)
            t = self.bwd.get(key)
            if not t:
                break
            ch = self._draw(t, rng)
            if ch in "^$" or len(w) >= max_len:
                break
            w = ch + w
        # Extend right.
        while len(w) < max_len:
            t = self.fwd.get(w[-2:])
            if not t:
                break
            ch = self._draw(t, rng)
            if ch in "^$":
                break
            w += ch
        return w


# --------------------------------------------------------------------------- helpers


def _units(text: str) -> Counter[str]:
    """Coverage units of a text: k:<char>, b:<bigram>, t:<trigram> (letter n-grams only)."""
    u: Counter[str] = Counter()
    for ch in text:
        u["k:" + ch] += 1
    for g in ngrams(text, 2):
        if g.isalpha():
            u["b:" + g] += 1
    for g in ngrams(text, 3):
        if g.isalpha():
            u["t:" + g] += 1
    return u


def _chunk(tokens: list[str], size: int, sep: str = " ") -> list[str]:
    """Join tokens into lessons of about `size` characters."""
    out: list[str] = []
    cur: list[str] = []
    n = 0
    for t in tokens:
        if cur and n + len(t) + 1 > size:
            out.append(sep.join(cur))
            cur, n = [], 0
        cur.append(t)
        n += len(t) + 1
    if cur:
        out.append(sep.join(cur))
    return out


def _split_even(items: list[str], parts: int) -> list[list[str]]:
    k, m = divmod(len(items), parts)
    out, i = [], 0
    for p in range(parts):
        j = i + k + (1 if p < m else 0)
        out.append(items[i:j])
        i = j
    return out


@dataclass(frozen=True)
class CoverageTargets:
    letters: int = 40
    capitals: int = 10
    digits: int = 20
    punctuation: int = 20
    enter: int = 20
    top_bigrams: int = 300
    top_bigram_samples: int = 15
    rest_bigram_samples: int = 3
    top_trigrams: int = 150
    top_trigram_samples: int = 8
    numbers: int = 20


# --------------------------------------------------------------------------- generator


class ProseGenerator:
    mode = "prose"
    version = VERSION

    def __init__(
        self,
        corpus: Corpus,
        layout: Layout,
        *,
        seed: int = 0,
        sittings: int = 3,
        targets: CoverageTargets | None = None,
    ) -> None:
        self.corpus = corpus
        self.layout = layout
        self.rng = random.Random(seed)
        self.sittings = max(1, sittings)
        self.cov = targets or CoverageTargets()
        top = corpus.words[:12000]
        self.markov = Markov(top, [corpus.word_freq[w] for w in top])
        # Word weights for adaptive ranking; set by the caller (item value -> weakness).
        self.weights: dict[str, float] = {}
        self.recent_sentences: list[int] = []

    # ------------------------------------------------------------------ protocol

    def item_kinds(self) -> list[str]:
        return ["key", "bigram", "trigram", "word"]

    def extract_items(self, text: str) -> list[Item]:
        return extract_items(text, self.mode)

    # ------------------------------------------------------------------ calibration

    def coverage_need(self) -> Counter[str]:
        c = self.cov
        need: Counter[str] = Counter()
        for ch in LETTERS:
            need["k:" + ch] = c.letters
        for ch in LETTERS.upper():
            need["k:" + ch] = c.capitals
        for ch in DIGITS:
            need["k:" + ch] = c.digits
        for ch in PUNCT:
            need["k:" + ch] = c.punctuation
        need["k:\n"] = c.enter
        need["k: "] = c.letters
        top_b = self.corpus.top("bigram", c.top_bigrams)
        for g in top_b:
            need["b:" + g] = c.top_bigram_samples
        top_set = set(top_b)
        for a in LETTERS:
            for b in LETTERS:
                if a + b not in top_set:
                    need["b:" + a + b] = c.rest_bigram_samples
        for g in self.corpus.top("trigram", c.top_trigrams):
            need["t:" + g] = c.top_trigram_samples
        return need

    def _natural(self, n_chars: int, rng: random.Random, avoid: set[int]) -> str:
        out: list[str] = []
        total = 0
        tries = 0
        while total < n_chars and tries < 10_000:
            tries += 1
            i = rng.randrange(len(self.corpus.sentences))
            s = self.corpus.sentences[i]
            if i in avoid or len(s) > 160 or len(s) < 40:
                continue
            avoid.add(i)
            out.append(s)
            total += len(s) + 1
        return " ".join(out)

    def _easy_words(self, n_chars: int, rng: random.Random) -> str:
        pool = [w for w in self.corpus.words[:200] if 2 <= len(w) <= 5]
        words: list[str] = []
        while sum(len(w) + 1 for w in words) < n_chars:
            words.append(rng.choice(pool))
        return " ".join(words)

    def special_tokens(self, need: Counter[str], rng: random.Random) -> list[str]:
        """Lines of compact tokens for capitals, digits, punctuation, numbers and Enter.

        Each token packs up to three still-needed symbols between short words or
        numbers (e.g. `Cat#dog~12`), so 20 samples of every symbol fit in a few
        thousand keystrokes. Bracket pairs are emitted together.
        """
        words = [w for w in self.corpus.words[:1500] if 2 <= len(w) <= 4]
        pairs = {"(": ")", "[": "]", "{": "}", "<": ">"}
        closers = {v: k for k, v in pairs.items()}
        have: Counter[str] = Counter()
        tokens: list[str] = []
        numbers = 0

        def short() -> str:
            return rng.choice(words)

        def deficit(group: str) -> list[str]:
            return [ch for ch in group if have["k:" + ch] < need["k:" + ch]]

        for _ in range(4000):
            syms = deficit(PUNCT)
            digs = deficit(DIGITS)
            caps = deficit(LETTERS.upper())
            if not (syms or digs or caps) and numbers >= self.cov.numbers:
                break
            rng.shuffle(syms)
            rng.shuffle(caps)

            def filler(caps: list[str] = caps, digs: list[str] = digs) -> str:
                if caps:
                    c = caps.pop()
                    pool = [x for x in words if x[0] == c.lower()] or words
                    return c + rng.choice(pool)[1:]
                if digs and rng.random() < 0.5:
                    return rng.choice(digs) + "".join(
                        rng.choice(DIGITS) for _ in range(rng.randint(0, 2))
                    )
                return short()

            parts: list[str] = [filler()]
            for sym in syms[:4]:
                if sym in closers:
                    sym = closers[sym]
                if sym in pairs:
                    parts[-1] = f"{sym}{parts[-1]}{pairs[sym]}"
                else:
                    parts.append(sym + filler())
            if not syms:
                parts.append(filler())
                if not caps:
                    d = rng.choice(digs) if digs else rng.choice(DIGITS)
                    parts.append(d + "".join(rng.choice(DIGITS) for _ in range(rng.randint(1, 3))))
                    numbers += 1
            tok = "".join(parts) if syms else " ".join(parts)
            tokens.append(tok)
            have.update("k:" + ch for ch in tok)
        rng.shuffle(tokens)
        # Line breaks for Enter: one after every third token.
        lines = [" ".join(tokens[i : i + 3]) for i in range(0, len(tokens), 3)]
        return lines

    def calibration_plan(self) -> list[LessonSpec]:
        """The full multi-sitting calibration plan (section 7)."""
        rng = random.Random(self.rng.random())
        S = self.sittings
        need = self.coverage_need()
        used_sentences: set[int] = set()
        lesson_chars = 260

        def take(text: str) -> None:
            for u, c in _units(text).items():
                if need[u] > 0:
                    need[u] = max(0, need[u] - c)

        warmups = [self._easy_words(150, rng) for _ in range(S)]
        naturals = [[self._natural(260, rng, used_sentences)] for _ in range(S)]
        probes = [self._natural(150, rng, used_sentences) for _ in range(S)]
        for s in range(S):
            for t in naturals[s]:
                take(t)
            take(probes[s])
            take(probes[s])  # typed twice: speed and accuracy blocks
        retest_text = naturals[0][0]
        take(retest_text)

        # Special: capitals, digits, punctuation, numbers, Enter.
        special_lines = self.special_tokens(need, rng)
        special_text = "\n".join(special_lines)
        take(special_text)
        per = _split_even(special_lines, S)
        specials = [["\n".join(group) for group in _group_lines(p, lesson_chars)] for p in per]

        # Coverage: greedy set cover over real words (letters, top bigrams/trigrams).
        candidates = [w for w in self.corpus.words[:15000] if 2 <= len(w) <= 12]
        top_b = set(self.corpus.top("bigram", self.cov.top_bigrams))

        def coverable(u: str) -> bool:
            kind, v = u[:2], u[2:]
            return kind == "t:" or (kind == "b:" and v in top_b) or (kind == "k:" and v in LETTERS)

        cov_need = Counter({u: n for u, n in need.items() if n > 0 and coverable(u)})
        # Coverage gets what is left of the keystroke budget, reserving room for pseudo-words.
        committed = sum(len(t) for ts in naturals for t in ts) + 2 * sum(map(len, probes))
        committed += len(retest_text) + len(special_text) + PSEUDO_RESERVE
        budget = max(1500, MAX_KEYSTROKES - committed)
        chosen = greedy_cover(
            candidates, cov_need, lambda x: _units(" " + x + " "), budget_chars=budget
        )
        rng.shuffle(chosen)
        take(" ".join(chosen))
        coverage = [_chunk(part, lesson_chars) for part in _split_even(chosen, S)]

        # Pseudo-words for the remaining letter bigrams (walk the deficit graph).
        pseudo_words = self._pseudo_cover(need, rng)
        take(" ".join(pseudo_words))
        pseudo = [_chunk(part, lesson_chars) for part in _split_even(pseudo_words, S)]

        # Assemble sittings with counterbalanced phase order.
        middle = ["natural", "coverage", "pseudo", "special"]
        specs: list[LessonSpec] = []
        for s in range(S):
            k = s % len(middle)
            order = middle[k:] + middle[:k]
            probe_order = ["speed", "accuracy"] if s % 2 == 0 else ["accuracy", "speed"]
            final = s == S - 1
            phases = ["warmup", *order, "probes"] + (["retest"] if final else [])
            texts: dict[str, list[str]] = {
                "warmup": [warmups[s]],
                "natural": naturals[s],
                "coverage": coverage[s],
                "pseudo": pseudo[s],
                "special": specials[s],
            }
            for pno, phase in enumerate(phases, start=1):
                base_meta = {
                    "sitting": s + 1,
                    "sittings": S,
                    "phase_no": pno,
                    "phases": len(phases),
                }
                if phase == "probes":
                    for sub in probe_order:
                        specs.append(
                            LessonSpec(
                                phase=sub,
                                text=probes[s],
                                meta={**base_meta, **PHASE_INFO[sub], "phase_order": phases},
                            )
                        )
                    continue
                if phase == "retest":
                    specs.append(
                        LessonSpec(
                            phase="retest",
                            text=retest_text,
                            meta={**base_meta, **PHASE_INFO["retest"]},
                        )
                    )
                    continue
                for text in texts[phase]:
                    if text.strip():
                        specs.append(
                            LessonSpec(
                                phase=phase,
                                text=text,
                                is_discarded=phase == "warmup",
                                meta={**base_meta, **PHASE_INFO[phase], "phase_order": phases},
                            )
                        )
        return specs

    def _pseudo_cover(self, need: Counter[str], rng: random.Random) -> list[str]:
        remaining = Counter({u[2:]: n for u, n in need.items() if u.startswith("b:") and n > 0})
        out_edges: dict[str, list[str]] = defaultdict(list)
        for g in remaining:
            out_edges[g[0]].append(g[1])
        words: list[str] = []
        while remaining:
            start = max(sorted(remaining), key=lambda g: remaining[g])
            w = start
            remaining[start] -= 1
            while len(w) < 7:
                nxt = [b for b in out_edges[w[-1]] if remaining[w[-1] + b] > 0]
                if not nxt:
                    break
                b = rng.choice(sorted(nxt))
                remaining[w[-1] + b] -= 1
                w += b
            remaining = +remaining
            words.append(w)
        rng.shuffle(words)
        return words

    def coverage_report(self, specs: list[LessonSpec]) -> dict[str, Any]:
        text = " ".join(s.text for s in specs if not s.is_discarded)
        have = _units(text)
        need = self.coverage_need()
        short = {u: (have[u], n) for u, n in need.items() if have[u] < n}
        groups: dict[str, list[str]] = defaultdict(list)
        for u, (h, n) in sorted(short.items()):
            groups[_unit_group(u)].append(f"{u[2:]!r} {h}/{n}")
        return {
            "keystrokes": len(text),
            "lessons": len(specs),
            "sittings": self.sittings,
            "units_required": len(need),
            "units_short": len(short),
            "shortfall": dict(groups),
        }

    # ------------------------------------------------------------------ adaptive

    def _candidate_words(self, value: str, max_len: int) -> list[str]:
        """Words carrying `value`; a leading/trailing space means word start/end."""
        v = value.lower()
        core = v.strip()
        if not core:
            return []
        start, end = v.startswith(" "), v.endswith(" ")

        def ok(w: str) -> bool:
            if start and end:
                return w == core
            if start:
                return w.startswith(core)
            if end:
                return w.endswith(core)
            return core in w

        return [w for w in self.corpus.words[:10000] if len(w) <= max_len and ok(w)]

    def _word_score(self, w: str) -> float:
        padded = f" {w} "
        s = sum(self.weights.get(g, 0.0) for g in ngrams(padded, 2) + ngrams(padded, 3))
        s += sum(self.weights.get(ch, 0.0) for ch in w)
        return s / math.sqrt(len(w))

    def drill_words(
        self, target: Item, n: int, difficulty: Difficulty, used: Counter[str]
    ) -> list[str]:
        if target.kind == "word":
            return [target.value] * max(1, n // 3)
        cands = self._candidate_words(target.value, difficulty.max_word_len)
        picks: list[str] = []
        if cands:
            freq = self.corpus.word_freq
            scored = sorted(
                cands[:400],
                key=lambda w: -(self._word_score(w) + 0.15 * (freq.get(w, 1) ** 0.1)),
            )
            pool = scored[:40]
            for _ in range(n):
                best = max(
                    pool,
                    key=lambda w: self._word_score(w) * 0.5 ** used[w] + self.rng.random() * 0.05,
                )
                picks.append(best)
                used[best] += 1
        # Too few real words: top up with Markov pseudo-words built around the target.
        core = target.value.lower().strip()
        while (
            len(set(picks)) < min(3, n) and target.kind in ("bigram", "trigram") and len(core) >= 2
        ):
            pw = self.markov.word_with(core, self.rng, difficulty.max_word_len)
            picks.append(pw)
            if len(picks) > n + 3:
                break
        return picks[: max(n, 1)]

    def best_sentence(self, targets: list[Item], max_chars: int) -> str | None:
        idx = self.corpus.sentence_index()
        tri = [t.value.lower() for t in targets if t.kind == "trigram"]
        bi = [t.value.lower() for t in targets if t.kind == "bigram"]
        keys = [t.value for t in targets if t.kind == "key"]
        cand_ids: set[int] = set()
        for g in tri:
            cand_ids.update(idx.get(g, [])[:300])
        if not cand_ids:
            for g in bi:
                for k, ids in idx.items():
                    if g in k:
                        cand_ids.update(ids[:50])
                        if len(cand_ids) > 2000:
                            break
        if not cand_ids:
            cand_ids = set(self.rng.sample(range(len(self.corpus.sentences)), 300))
        scored: list[tuple[float, int]] = []
        for i in cand_ids:
            s = self.corpus.sentences[i]
            if len(s) > max_chars or len(s) < 30 or i in self.recent_sentences:
                continue
            low = s.lower()
            hits = sum(low.count(g) for g in tri) * 1.5 + sum(low.count(g) for g in bi)
            hits += sum(s.count(k) for k in keys) * 0.5
            scored.append((hits / len(s), i))
        if not scored:
            return None
        scored.sort(reverse=True)
        _, pick = self.rng.choice(scored[:5])  # some variety among the best matches
        self.recent_sentences.append(pick)
        del self.recent_sentences[:-500]
        return self.corpus.sentences[pick]

    def generate(
        self,
        targets: list[Target],
        budget: int,
        difficulty: Difficulty,
    ) -> LessonSpec:
        weak = [t for t in targets if t.reason == "weak"]
        review = [t for t in targets if t.reason == "review"]
        used: Counter[str] = Counter()
        drill_budget = int(budget * difficulty.drill_share) if (weak or review) else 0
        tokens: list[str] = []
        groups = weak + review
        if groups:
            per_target = max(2, (drill_budget // 6) // len(groups))
            for t in groups:
                tokens.extend(self.drill_words(t.item, per_target, difficulty, used))
        self.rng.shuffle(tokens)
        if difficulty.capitals:
            tokens = [w[:1].upper() + w[1:] if self.rng.random() < 0.15 else w for w in tokens]
        if difficulty.punctuation:
            tokens = [w + self.rng.choice(",.;") if self.rng.random() < 0.08 else w for w in tokens]
        drill = _fit(tokens, drill_budget)
        remaining = budget - len(drill) - (1 if drill else 0)
        sentence = ""
        if remaining >= 40:
            parts: list[str] = []
            left = remaining
            for _ in range(3):
                s = self.best_sentence([t.item for t in groups], left)
                if not s or s in parts:
                    break
                parts.append(s)
                left -= len(s) + 1
                if left < 40:
                    break
            sentence = " ".join(parts)
        text = " ".join(x for x in (sentence, drill) if x).strip()
        if not text:
            text = self._natural(budget, self.rng, set())
        phase = "drill" if weak else ("review" if review else "mixed")
        return LessonSpec(
            phase=phase,
            text=text,
            targets=[
                {"kind": t.item.kind, "item": t.item.value, "reason": t.reason} for t in targets
            ],
            meta={
                "title": PHASE_INFO[phase]["title"],
                "instruction": PHASE_INFO[phase]["instruction"],
            },
        )

    def probe_lesson(self) -> LessonSpec:
        """Fixed mini-recalibration text: identical every time (fixed-position probe)."""
        rng = random.Random(20240601)
        text = self._natural(220, rng, set())
        return LessonSpec(phase="probe", text=text, meta=dict(PHASE_INFO["probe"]))


def _fit(tokens: list[str], budget: int) -> str:
    out: list[str] = []
    n = 0
    for t in tokens:
        if n + len(t) + 1 > budget:
            break
        out.append(t)
        n += len(t) + 1
    return " ".join(out)


def _group_lines(lines: list[str], size: int) -> list[list[str]]:
    groups: list[list[str]] = []
    cur: list[str] = []
    n = 0
    for ln in lines:
        if cur and n + len(ln) + 1 > size:
            groups.append(cur)
            cur, n = [], 0
        cur.append(ln)
        n += len(ln) + 1
    if cur:
        groups.append(cur)
    return groups


def _unit_group(u: str) -> str:
    kind, v = u[:2], u[2:]
    if kind == "k:":
        if v in LETTERS:
            return "letters"
        if v in LETTERS.upper():
            return "capitals"
        if v in DIGITS:
            return "digits"
        return "punctuation/space/enter"
    return {"b:": "bigrams", "t:": "trigrams"}[kind]


PHASE_INFO: dict[str, dict[str, str]] = {
    "warmup": {"title": "Warm-up", "instruction": "Loosen up. This block is not scored."},
    "natural": {"title": "Natural text", "instruction": "Type at your normal, comfortable pace."},
    "coverage": {
        "title": "Word coverage",
        "instruction": "A mix of words chosen to sample every letter pattern.",
    },
    "pseudo": {
        "title": "Rare patterns",
        "instruction": "Made-up words with rare letter pairs. Take them steadily.",
    },
    "special": {
        "title": "Capitals, numbers & symbols",
        "instruction": "Shifted keys, digits, punctuation and line breaks.",
    },
    "speed": {
        "title": "Speed probe",
        "instruction": "Type this as FAST as you can. Errors are fine.",
    },
    "accuracy": {
        "title": "Accuracy probe",
        "instruction": "Type this as ACCURATELY as you can. Take your time.",
    },
    "retest": {
        "title": "Retest",
        "instruction": "A passage from your first sitting, to measure your progress.",
    },
    "drill": {
        "title": "Focused practice",
        "instruction": "Built from your weakest keys and letter patterns.",
    },
    "review": {
        "title": "Review",
        "instruction": "Patterns you improved, checked again before you forget them.",
    },
    "mixed": {"title": "Practice", "instruction": "Natural text at your level."},
    "probe": {
        "title": "Checkpoint",
        "instruction": "The same short passage every time, to track your progress.",
    },
}


__all__ = [
    "PHASE_INFO",
    "VERSION",
    "Difficulty",
    "LessonSpec",
    "Markov",
    "ProseGenerator",
    "Target",
    "item_counts",
]
