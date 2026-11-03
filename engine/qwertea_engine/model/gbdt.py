"""LightGBM context model (insight layer, section 8.3.4).

Predicts P(error) and log-latency of a keystroke from its context. It sits on
top of the Bayesian estimates: it explains *why* keystrokes go wrong (which
context features matter) and is never the sole source of truth for lesson
selection.

Explanations use LightGBM's built-in TreeSHAP (`pred_contrib=True`), which
computes exact SHAP values for tree ensembles without the `shap` package.
Quality is reported with a held-out split (the last 20 % of lessons): AUC for
errors, R² for latency.

`lightgbm` is an optional dependency (`uv sync --extra insight`); without it,
`train` returns None and the rest of the engine is unaffected.
"""

from __future__ import annotations

import math
import sqlite3
from dataclasses import dataclass
from typing import Any

import numpy as np

from ..layouts import Layout

FEATURES = [
    "finger",
    "hand",
    "row",
    "prev_finger",
    "same_finger",
    "same_hand",
    "hand_alternation",
    "row_jump",
    "prev_log_flight",
    "session_pos",
    "shift",
    "pos_in_word",
    "is_space",
    "char_freq_rank",
    "char",
    "prev_char",
]
CATEGORICAL = ["finger", "prev_finger", "char", "prev_char"]
FINGERS = [
    "l_pinky",
    "l_ring",
    "l_middle",
    "l_index",
    "thumb",
    "r_index",
    "r_middle",
    "r_ring",
    "r_pinky",
]
MIN_ROWS = 1500


@dataclass
class InsightResult:
    rows: int
    error_auc: float | None
    latency_r2: float | None
    error_importance: dict[str, float]  # mean |SHAP| per feature
    latency_importance: dict[str, float]
    error_base_rate: float


def available() -> bool:
    try:
        import lightgbm  # noqa: F401
    except Exception:
        return False
    return True


def build_matrix(
    conn: sqlite3.Connection, layout: Layout, mode: str = "prose", think_pause_ms: float = 2500.0
) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    """Feature matrix X, error labels, log-flight (NaN when unusable), lesson index."""
    rows = conn.execute(
        """SELECT k.lesson_id, k.expected, k.prev1, k.error_type, k.flight_ms, k.session_pos,
                  k.target_idx, l.text
           FROM keystrokes k JOIN lessons l ON l.id = k.lesson_id
           WHERE l.status='done' AND l.is_discarded=0 AND l.mode=? AND k.is_first_attempt=1
           ORDER BY l.finished_at_utc, k.lesson_id, k.seq""",
        (mode,),
    ).fetchall()
    order = "etaoinshrdlcumwfgypbvkjxqz"
    X: list[list[float]] = []
    y_err: list[int] = []
    y_lat: list[float] = []
    lesson: list[int] = []
    prev_flight: dict[int, float] = {}
    for r in rows:
        exp = r["expected"]
        if exp is None:
            continue
        k = layout.lookup(exp)
        p = layout.lookup(r["prev1"]) if r["prev1"] else None
        text: str = r["text"]
        i = r["target_idx"]
        j = i
        while j > 0 and text[j - 1].isalpha():
            j -= 1
        f = r["flight_ms"]
        lf = math.log(f) if f is not None and 0 < f <= think_pause_ms else float("nan")
        pf = prev_flight.get(r["lesson_id"], float("nan"))
        X.append(
            [
                FINGERS.index(k.finger) if k and k.finger in FINGERS else float("nan"),
                {"left": 0, "right": 1}.get(k.hand, 2) if k else 2,
                k.row if k else -1,
                FINGERS.index(p.finger) if p and p.finger in FINGERS else float("nan"),
                float(bool(k and p and k.finger == p.finger and k.code != p.code)),
                float(bool(k and p and k.hand == p.hand)),
                float(bool(k and p and k.hand != p.hand and "either" not in (k.hand, p.hand))),
                float(abs(k.row - p.row)) if k and p else float("nan"),
                pf,
                r["session_pos"] if r["session_pos"] is not None else float("nan"),
                float(bool(k and k.shifted)),
                float(i - j),
                float(exp == " "),
                float(order.index(exp.lower())) if exp.lower() in order else 30.0,
                float(min(ord(exp), 127)),
                float(min(ord(r["prev1"]), 127)) if r["prev1"] else float("nan"),
            ]
        )
        y_err.append(int(r["error_type"] is not None))
        y_lat.append(lf)
        lesson.append(r["lesson_id"])
        prev_flight[r["lesson_id"]] = lf
    return np.array(X, dtype=float), np.array(y_err), np.array(y_lat), np.array(lesson)


def _auc(y: np.ndarray, s: np.ndarray) -> float | None:
    pos, neg = s[y == 1], s[y == 0]
    if len(pos) == 0 or len(neg) == 0:
        return None
    ranks = np.argsort(np.argsort(np.concatenate([pos, neg])))
    r_pos = ranks[: len(pos)].sum() + len(pos)  # 1-based ranks
    return float((r_pos - len(pos) * (len(pos) + 1) / 2) / (len(pos) * len(neg)))


def _importance(contrib: np.ndarray) -> dict[str, float]:
    m = np.abs(contrib[:, :-1]).mean(axis=0)  # last column is the bias term
    return {f: float(v) for f, v in sorted(zip(FEATURES, m, strict=True), key=lambda t: -t[1])}


def train(
    conn: sqlite3.Connection, layout: Layout, mode: str = "prose", seed: int = 0
) -> InsightResult | None:
    if not available():
        return None
    import lightgbm as lgb

    X, y_err, y_lat, lesson = build_matrix(conn, layout, mode)
    if len(X) < MIN_ROWS or y_err.sum() < 30:
        return None
    lessons = np.unique(lesson)
    cut = lessons[int(len(lessons) * 0.8)] if len(lessons) >= 5 else lessons[-1]
    tr, te = lesson < cut, lesson >= cut
    params: dict[str, Any] = {
        "objective": "binary",
        "learning_rate": 0.05,
        "num_leaves": 15,
        "min_data_in_leaf": 40,
        "feature_fraction": 0.9,
        "bagging_fraction": 0.8,
        "bagging_freq": 1,
        "lambda_l2": 1.0,
        "verbose": -1,
        "seed": seed,
        "is_unbalance": True,
    }
    cat_idx = [FEATURES.index(c) for c in CATEGORICAL]
    d = lgb.Dataset(X[tr], y_err[tr], categorical_feature=cat_idx, free_raw_data=False)
    clf = lgb.train(params, d, num_boost_round=200)
    auc = _auc(y_err[te], np.asarray(clf.predict(X[te]))) if te.any() else None
    err_imp = _importance(np.asarray(clf.predict(X, pred_contrib=True)))

    ok = ~np.isnan(y_lat)
    r2 = None
    lat_imp: dict[str, float] = {}
    if ok.sum() >= MIN_ROWS // 2:
        rp = {**params, "objective": "regression"}
        rp.pop("is_unbalance")
        reg = lgb.train(
            rp,
            lgb.Dataset(X[ok & tr], y_lat[ok & tr], categorical_feature=cat_idx),
            num_boost_round=200,
        )
        if (ok & te).any():
            pred = np.asarray(reg.predict(X[ok & te]))
            yt = y_lat[ok & te]
            ss_res = float(((yt - pred) ** 2).sum())
            ss_tot = float(((yt - yt.mean()) ** 2).sum())
            r2 = 1 - ss_res / ss_tot if ss_tot > 0 else None
        lat_imp = _importance(np.asarray(reg.predict(X[ok], pred_contrib=True)))
    return InsightResult(len(X), auc, r2, err_imp, lat_imp, float(y_err.mean()))
