"""Robust z-scores against the user's OWN baseline (always on, explainable).

* latency z: (item median log-latency - global median) / (1.4826 * global MAD),
  shrunk toward 0 by n / (n + k) so a handful of samples cannot dominate;
* error z: (posterior mean - global rate) / spread of key-level error rates.

The combined score is `weakness = w_e * z_err + w_l * z_lat` (section 8.3.3).
"""

from __future__ import annotations

import statistics
from dataclasses import dataclass

from ..items import Item
from .bayes import BayesModel
from .latency import MAD_SCALE, LatencyStats, mad, stats
from .observations import ItemObs

SHRINK_K = 5.0


@dataclass(frozen=True)
class Baseline:
    lat_median: float
    lat_scale: float
    err_rate: float
    err_scale: float


@dataclass(frozen=True)
class ItemScore:
    item: Item
    n: int
    p_err: float
    p_sd: float
    z_err: float
    z_lat: float
    weakness: float
    uncertainty: float
    latency: LatencyStats | None
    alpha: float = 1.0
    beta: float = 1.0


def fit_baseline(items: dict[Item, ItemObs], bayes: BayesModel) -> Baseline:
    key_lats = [x for it, o in items.items() if it.kind == "key" for x in o.log_lat]
    med = statistics.median(key_lats) if key_lats else 5.3  # ~200 ms
    scale = MAD_SCALE * mad(key_lats, med) if key_lats else 0.3
    means = [bayes.posterior(it).mean for it in items if it.kind == "key" and items[it].n >= 5]
    err_scale = MAD_SCALE * mad(means) if len(means) >= 5 else 0.0
    # Never let the error spread collapse: floor at a fraction of the global rate.
    err_scale = max(err_scale, 0.5 * bayes.global_rate, 0.01)
    return Baseline(med, max(scale, 0.05), bayes.global_rate, err_scale)


def score(
    item: Item,
    obs: ItemObs,
    bayes: BayesModel,
    base: Baseline,
    *,
    w_err: float = 0.6,
    w_lat: float = 0.4,
    p_err: float | None = None,
) -> ItemScore:
    post = bayes.posterior(item)
    p = post.mean if p_err is None else p_err
    z_err = (p - base.err_rate) / base.err_scale
    lat = stats(obs.log_lat)
    z_lat = 0.0
    if lat is not None:
        shrink = lat.n / (lat.n + SHRINK_K)
        z_lat = shrink * (lat.median_log - base.lat_median) / base.lat_scale
    weakness = w_err * z_err + w_lat * z_lat
    uncertainty = post.sd / base.err_scale
    return ItemScore(
        item, obs.n, p, post.sd, z_err, z_lat, weakness, uncertainty, lat, post.alpha, post.beta
    )
