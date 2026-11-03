"""Empirical-Bayes error posteriors with hierarchical backoff.

Every item gets a Beta posterior for P(error). Its prior mean comes from the
level below it, so thin data shrinks toward better-estimated parents:

    word    -> 1 - prod(1 - p(char))   over its characters
    trigram -> its last bigram
    bigram  -> its last key (lower-case)
    key     -> the global error rate

Prior strength (pseudo-count `m`) is fitted per level by the method of
moments: the between-item variance of observed rates around their prior
means, after removing binomial noise. Little between-item variance means a
strong prior (more shrinkage); a lot means a weak prior.
"""

from __future__ import annotations

import math
import random
from dataclasses import dataclass

from ..items import Item
from .observations import ItemObs

M_MIN, M_MAX = 2.0, 400.0


@dataclass(frozen=True)
class Posterior:
    alpha: float
    beta: float

    @property
    def mean(self) -> float:
        return self.alpha / (self.alpha + self.beta)

    @property
    def sd(self) -> float:
        a, b = self.alpha, self.beta
        return math.sqrt(a * b / ((a + b) ** 2 * (a + b + 1)))

    def sample(self, rng: random.Random) -> float:
        return rng.betavariate(self.alpha, self.beta)


def fit_strength(obs: list[tuple[int, int, float]], min_n: int = 3) -> float:
    """Method-of-moments prior strength from (n, errors, prior_mean) triples."""
    rows = [(n, e, mu) for n, e, mu in obs if n >= min_n and 0 < mu < 1]
    if len(rows) < 5:
        return 30.0
    resid = [(e / n - mu) ** 2 for n, e, mu in rows]
    noise = [mu * (1 - mu) / n for n, _, mu in rows]
    between = sum(resid) / len(rows) - sum(noise) / len(rows)
    mean_var = sum(mu * (1 - mu) for _, _, mu in rows) / len(rows)
    if between <= 1e-9:
        return M_MAX
    m = mean_var / between - 1.0
    return max(M_MIN, min(M_MAX, m))


@dataclass
class BayesModel:
    global_rate: float
    strengths: dict[str, float]
    posteriors: dict[Item, Posterior]

    def posterior(self, item: Item) -> Posterior:
        if item in self.posteriors:
            return self.posteriors[item]
        mu = self.prior_mean(item)
        m = self.strengths.get(item.kind, 30.0)
        return Posterior(m * mu, m * (1 - mu))

    def sample_posterior(self, item: Item, rng: random.Random) -> float:
        """Thompson-sampling draw of P(error) for `item`."""
        return self.posterior(item).sample(rng)

    def prior_mean(self, item: Item) -> float:
        g = self.global_rate
        if item.kind == "key":
            return g
        if item.kind == "bigram":
            return self._mean(Item(item.mode, "key", item.value[-1]), g)
        if item.kind == "trigram":
            return self._mean(Item(item.mode, "bigram", item.value[-2:]), g)
        if item.kind == "word":
            q = 1.0
            for ch in item.value:
                q *= 1.0 - self._mean(Item(item.mode, "key", ch), g)
            return min(0.95, max(1e-4, 1.0 - q))
        return g

    def _mean(self, item: Item, default: float) -> float:
        p = self.posteriors.get(item)
        return p.mean if p else default


def fit(items: dict[Item, ItemObs], prior_global: tuple[float, float] = (1.0, 30.0)) -> BayesModel:
    """Fit the hierarchy level by level (keys first, so children can back off to them)."""
    keys = {it: o for it, o in items.items() if it.kind == "key"}
    tot_n = sum(o.n for o in keys.values())
    tot_e = sum(o.errors for o in keys.values())
    a0, b0 = prior_global
    global_rate = (tot_e + a0) / (tot_n + a0 + b0)
    model = BayesModel(global_rate, {}, {})
    for kind in ("key", "bigram", "trigram", "word"):
        level = {it: o for it, o in items.items() if it.kind == kind}
        priors = {it: model.prior_mean(it) for it in level}
        if kind == "bigram":
            # Back off to the lower-case key (bigrams are case-folded).
            priors = {
                it: model._mean(Item(it.mode, "key", it.value[-1].lower()), global_rate)
                for it in level
            }
        m = fit_strength([(o.n, o.errors, priors[it]) for it, o in level.items()])
        model.strengths[kind] = m
        for it, o in level.items():
            mu = min(0.99, max(1e-4, priors[it]))
            model.posteriors[it] = Posterior(m * mu + o.errors, m * (1 - mu) + o.n - o.errors)
    return model
