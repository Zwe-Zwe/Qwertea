"""Level model: current WPM and accuracy, speed-accuracy trade-off, difficulty control,
and the behavioural profile (post-error slowdown, rhythm, fatigue, bursts)."""

from __future__ import annotations

import math
import statistics
from dataclasses import asdict, dataclass
from typing import Any

from ..generators.base import Difficulty
from .observations import LessonObs, Observations

LEVEL_PHASES = {
    "natural",
    "mixed",
    "drill",
    "review",
    "probe",
    "retest",
    "coverage",
    "special",
    "pseudo",
}


@dataclass
class Level:
    wpm: float
    accuracy: float
    lessons: int
    # accuracy = intercept + slope * wpm, fitted over all lessons (and the speed/accuracy probes)
    sa_intercept: float | None
    sa_slope: float | None
    speed_probe_wpm: float | None
    accuracy_probe_wpm: float | None
    learning_curve: dict[str, float] | None

    def to_json(self) -> dict[str, Any]:
        return asdict(self)


def _linfit(xs: list[float], ys: list[float]) -> tuple[float, float] | None:
    if len(xs) < 3 or len(set(xs)) < 2:
        return None
    mx, my = statistics.fmean(xs), statistics.fmean(ys)
    sxx = sum((x - mx) ** 2 for x in xs)
    sxy = sum((x - mx) * (y - my) for x, y in zip(xs, ys, strict=True))
    slope = sxy / sxx
    return my - slope * mx, slope


def power_law(lessons: list[LessonObs]) -> dict[str, float] | None:
    """Fit wpm = a * k^b over lesson index k (log-log least squares)."""
    pts = [(i + 1, x.wpm) for i, x in enumerate(lessons) if x.wpm > 0]
    if len(pts) < 4:
        return None
    fit = _linfit([math.log(k) for k, _ in pts], [math.log(w) for _, w in pts])
    if fit is None:
        return None
    ln_a, b = fit
    return {"a": math.exp(ln_a), "b": b}


def estimate(obs: Observations) -> Level:
    ls = [x for x in obs.lessons if x.first_attempts >= 20]
    regular = [x for x in ls if x.phase in LEVEL_PHASES]
    recent = regular[-5:] or ls[-5:]
    wpm = statistics.median([x.wpm for x in recent]) if recent else 0.0
    acc = statistics.fmean([x.accuracy for x in recent]) if recent else 1.0
    fit = _linfit([x.wpm for x in ls], [x.accuracy for x in ls])
    sp = [x.wpm for x in ls if x.phase == "speed"]
    ap = [x.wpm for x in ls if x.phase == "accuracy"]
    return Level(
        wpm=wpm,
        accuracy=acc,
        lessons=len(ls),
        sa_intercept=fit[0] if fit else None,
        sa_slope=fit[1] if fit else None,
        speed_probe_wpm=statistics.fmean(sp) if sp else None,
        accuracy_probe_wpm=statistics.fmean(ap) if ap else None,
        learning_curve=power_law(regular),
    )


def behavior(obs: Observations) -> dict[str, Any]:
    f = obs.flights
    out: dict[str, Any] = {}
    if len(f) >= 20:
        med = statistics.median(f)
        out["median_flight_ms"] = med
        out["rhythm_cv"] = statistics.pstdev(f) / statistics.fmean(f)
        out["burst_share"] = sum(1 for x in f if x < 0.6 * med) / len(f)
    if obs.slowdowns:
        out["post_error_slowdown_ms"] = statistics.fmean(obs.slowdowns)
    if len(obs.session_pos) >= 50:
        fit = _linfit([p for p, _ in obs.session_pos], [lat for _, lat in obs.session_pos])
        if fit:
            # Relative slow-down per hour in the session (positive = fatigue).
            out["fatigue_slope_per_hour"] = math.exp(fit[1]) - 1.0
    totals: dict[str, int] = {}
    for les in obs.lessons:
        for k, v in les.error_types.items():
            totals[k] = totals.get(k, 0) + v
    out["error_types"] = totals
    return out


def difficulty(
    level: Level, acc_band: tuple[float, float], length_range: tuple[int, int]
) -> Difficulty:
    """Keep accuracy inside the target band by adjusting length, word length and drilling."""
    lo, hi = acc_band
    lmin, lmax = length_range
    if level.lessons < 3:
        return Difficulty(length=(lmin + lmax) // 2, max_word_len=8, drill_share=0.6)
    if level.accuracy < lo:
        return Difficulty(length=lmin, max_word_len=6, drill_share=0.5)
    if level.accuracy > hi:
        return Difficulty(
            length=lmax, max_word_len=11, drill_share=0.7, capitals=True, punctuation=True
        )
    mid = int(lmin + (lmax - lmin) * (level.accuracy - lo) / (hi - lo))
    return Difficulty(
        length=mid, max_word_len=9, drill_share=0.6, capitals=level.accuracy > (lo + hi) / 2
    )
