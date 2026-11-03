"""Robust log-latency statistics: medians and MAD (section 8.1)."""

from __future__ import annotations

import statistics
from dataclasses import dataclass

MAD_SCALE = 1.4826  # MAD -> standard deviation for a normal distribution


@dataclass(frozen=True)
class LatencyStats:
    n: int
    median_log: float
    mad_log: float


def mad(values: list[float], center: float | None = None) -> float:
    if not values:
        return 0.0
    c = statistics.median(values) if center is None else center
    return statistics.median(abs(v - c) for v in values)


def stats(log_lat: list[float]) -> LatencyStats | None:
    if not log_lat:
        return None
    m = statistics.median(log_lat)
    return LatencyStats(len(log_lat), m, mad(log_lat, m))
