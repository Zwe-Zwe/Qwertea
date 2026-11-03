"""Static, self-contained HTML report (`qwertea-engine report`, section 8.7).

Sections: level and learning curve, keyboard weakness heatmap, weakest
bigrams/trigrams with trend, error types, behaviour and baselines,
drilled-vs-holdout comparison, and the LightGBM/SHAP insight summary.
"""

from __future__ import annotations

import base64
import html
import io
import json
import math
import sqlite3
import statistics
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.colors import LinearSegmentedColormap
from matplotlib.patches import FancyBboxPatch

from ..items import Item
from ..layouts import Layout
from ..model import gbdt
from ..model.observations import collect

BG, FG, MUTED, ACCENT, GRID = "#1e1f24", "#e8e6e3", "#9a9ca5", "#e2b714", "#34363d"
CMAP = LinearSegmentedColormap.from_list("weak", ["#2c6e49", "#3a3d45", "#c0392b"])


def _png(fig: Any) -> str:
    buf = io.BytesIO()
    fig.savefig(buf, format="png", dpi=110, bbox_inches="tight", facecolor=BG)
    plt.close(fig)
    return "data:image/png;base64," + base64.b64encode(buf.getvalue()).decode()


def _style(ax: Any) -> None:
    ax.set_facecolor(BG)
    for s in ax.spines.values():
        s.set_color(GRID)
    ax.tick_params(colors=MUTED)
    ax.yaxis.label.set_color(MUTED)
    ax.xaxis.label.set_color(MUTED)
    ax.grid(color=GRID, linewidth=0.6)


def learning_chart(lessons: list[Any], curve: dict[str, float] | None) -> str | None:
    pts = [(i + 1, x.wpm, x.accuracy) for i, x in enumerate(lessons) if x.first_attempts >= 20]
    if len(pts) < 2:
        return None
    fig, ax = plt.subplots(figsize=(9, 3.4), facecolor=BG)
    _style(ax)
    xs = [p[0] for p in pts]
    ax.plot(xs, [p[1] for p in pts], "o", color=ACCENT, ms=3.5, alpha=0.8, label="WPM per lesson")
    if curve:
        fx = list(range(1, max(xs) + 1))
        ax.plot(
            fx,
            [curve["a"] * k ** curve["b"] for k in fx],
            color=FG,
            lw=1.5,
            label=f"power law  {curve['a']:.1f}·k^{curve['b']:.3f}",
        )
    ax.set_xlabel("lesson")
    ax.set_ylabel("WPM")
    ax2 = ax.twinx()
    ax2.plot(xs, [100 * p[2] for p in pts], color="#5dade2", lw=1, alpha=0.7, label="accuracy %")
    ax2.set_ylim(70, 100.5)
    ax2.tick_params(colors=MUTED)
    ax2.set_ylabel("accuracy %", color=MUTED)
    lines = ax.get_legend_handles_labels()
    lines2 = ax2.get_legend_handles_labels()
    leg = ax.legend(
        lines[0] + lines2[0], lines[1] + lines2[1], loc="lower right", facecolor=BG, edgecolor=GRID
    )
    for t in leg.get_texts():
        t.set_color(FG)
    return _png(fig)


def keyboard_heatmap(layout: Layout, weakness: dict[str, float]) -> str:
    fig, ax = plt.subplots(figsize=(10, 3.6), facecolor=BG)
    ax.set_facecolor(BG)
    ax.set_xlim(0, 15)
    ax.set_ylim(-5.2, 0.4)
    ax.axis("off")
    vals = [v for v in weakness.values() if not math.isnan(v)]
    lim = max(1.0, max((abs(v) for v in vals), default=1.0))
    seen: set[int] = set()
    for ch in layout.chars():
        k = layout.lookup(ch)
        if k is None or k.shifted or k.code in seen or ch in "\n\t":
            continue
        seen.add(k.code)
        w = 6.0 if ch == " " else 0.94
        x = k.x - w / 2
        y = -k.row - 0.47
        v = weakness.get(ch, float("nan"))
        color = "#2a2c33" if math.isnan(v) else CMAP(0.5 + 0.5 * max(-1.0, min(1.0, v / lim)))
        ax.add_patch(
            FancyBboxPatch(
                (x, y), w, 0.94, boxstyle="round,pad=0,rounding_size=0.12", fc=color, ec=GRID, lw=1
            )
        )
        label = "space" if ch == " " else ch
        ax.text(
            k.x,
            y + 0.55,
            label,
            ha="center",
            va="center",
            color=FG,
            fontsize=11,
            family="monospace",
        )
        if not math.isnan(v):
            ax.text(k.x, y + 0.2, f"{v:+.1f}", ha="center", va="center", color=MUTED, fontsize=6.5)
    return _png(fig)


def bar_chart(
    data: dict[str, float], title: str, color: str = ACCENT, fmt: str = "{:.3f}"
) -> str | None:
    if not data:
        return None
    items = list(data.items())[:12][::-1]
    fig, ax = plt.subplots(figsize=(6, 0.32 * len(items) + 0.8), facecolor=BG)
    _style(ax)
    ax.barh([k for k, _ in items], [v for _, v in items], color=color)
    for i, (_, v) in enumerate(items):
        ax.text(v, i, " " + fmt.format(v), va="center", color=MUTED, fontsize=8)
    ax.set_title(title, color=FG, fontsize=10, loc="left")
    return _png(fig)


def _trend(history: list[tuple[int, int]]) -> tuple[float | None, float | None]:
    if len(history) < 8:
        return None, None
    half = len(history) // 2
    a = [e for _, e in history[:half]]
    b = [e for _, e in history[half:]]
    return statistics.fmean(a), statistics.fmean(b)


def holdout_comparison(conn: sqlite3.Connection, obs: Any) -> dict[str, Any]:
    """Error rate before vs after drilling started, drilled items vs holdouts.

    Lesson indices follow `obs.lessons`, the same order as `ItemObs.history`.
    """
    items: dict[Item, Any] = obs.items
    drilled: set[tuple[str, str]] = set()
    first_drill: int | None = None
    for i, les in enumerate(obs.lessons):
        if les.targets_json:
            for t in json.loads(les.targets_json):
                if t.get("reason") == "weak":
                    drilled.add((t["kind"], t["item"]))
                    first_drill = i if first_drill is None else first_drill
    holdouts = {
        (r[0], r[1])
        for r in conn.execute("SELECT item_kind, item FROM item_stats WHERE is_holdout=1")
    }
    if first_drill is None:
        return {}

    def group(keys: set[tuple[str, str]]) -> dict[str, Any] | None:
        before = after = 0
        nb = na = 0
        for kind, val in keys:
            o = items.get(Item("prose", kind, val))
            if o is None:
                continue
            for idx, e in o.history:
                if idx < first_drill:
                    before += e
                    nb += 1
                else:
                    after += e
                    na += 1
        if nb < 10 or na < 10:
            return None
        return {
            "items": len(keys),
            "before": before / nb,
            "after": after / na,
            "n_before": nb,
            "n_after": na,
        }

    return {"drilled": group(drilled - holdouts), "holdout": group(holdouts)}


def _calibration_coverage(conn: sqlite3.Connection, layout: Layout) -> dict[str, Any] | None:
    from ..corpus import Corpus
    from ..generators.base import LessonSpec
    from ..generators.prose import ProseGenerator

    rows = conn.execute(
        "SELECT phase, text, is_discarded, meta_json FROM lessons WHERE generator='prose/calibration'"
    ).fetchall()
    if not rows:
        return None
    sittings = max((json.loads(r["meta_json"] or "{}").get("sittings", 1) for r in rows), default=1)
    gen = ProseGenerator(Corpus.load(), layout, sittings=sittings)
    specs = [LessonSpec(r["phase"], r["text"], is_discarded=bool(r["is_discarded"])) for r in rows]
    return gen.coverage_report(specs)


def build(
    conn: sqlite3.Connection,
    out_dir: Path,
    layout: Layout,
    mode: str = "prose",
    coverage: dict[str, Any] | None = None,
) -> Path:
    out_dir.mkdir(parents=True, exist_ok=True)
    obs = collect(conn, mode)
    snap = conn.execute(
        "SELECT * FROM model_snapshots WHERE mode=? ORDER BY id DESC LIMIT 1", (mode,)
    ).fetchone()
    lvl = json.loads(snap["level_json"]) if snap and snap["level_json"] else {}
    beh = json.loads(snap["behavior_json"]) if snap and snap["behavior_json"] else {}
    stats = conn.execute(
        "SELECT item_kind, item, n, err_alpha, err_beta, lat_median_log, weakness, is_holdout, next_review_utc "
        "FROM item_stats WHERE mode=?",
        (mode,),
    ).fetchall()
    key_weak = {r["item"]: r["weakness"] for r in stats if r["item_kind"] == "key" and r["n"] >= 8}
    total_keys = sum(x.first_attempts for x in obs.lessons)
    calib_left = conn.execute(
        "SELECT COUNT(*) FROM lessons WHERE generator='prose/calibration' AND status IN ('pending','running')"
    ).fetchone()[0]

    def table(kind: str, n_min: int, limit: int = 15) -> str:
        rows = sorted(
            (r for r in stats if r["item_kind"] == kind and r["n"] >= n_min),
            key=lambda r: -(r["weakness"] or 0),
        )[:limit]
        out = []
        for r in rows:
            p = r["err_alpha"] / (r["err_alpha"] + r["err_beta"])
            ms = math.exp(r["lat_median_log"]) if r["lat_median_log"] is not None else None
            o = obs.items.get(Item(mode, kind, r["item"]))
            a, b = _trend(o.history if o is not None else [])
            trend = (
                ""
                if a is None or b is None
                else ("▼ improving" if b < a * 0.8 else ("▲ worse" if b > a * 1.25 else "■ steady"))
            )
            tag = " <span class=tag>holdout</span>" if r["is_holdout"] else ""
            out.append(
                f"<tr><td class=mono>{html.escape(repr(r['item']))}{tag}</td><td>{r['n']}</td>"
                f"<td>{100 * p:.1f}%</td><td>{'' if ms is None else f'{ms:.0f} ms'}</td>"
                f"<td>{r['weakness']:+.2f}</td><td>{trend}</td></tr>"
            )
        return (
            "<table><tr><th>item</th><th>n</th><th>P(error)</th><th>median latency</th>"
            "<th>weakness</th><th>trend</th></tr>" + "".join(out) + "</table>"
        )

    if coverage is None:
        coverage = _calibration_coverage(conn, layout)
    insight = gbdt.train(conn, layout, mode)
    hc = holdout_comparison(conn, obs)
    imgs = {
        "learning": learning_chart(obs.lessons, lvl.get("learning_curve")),
        "keyboard": keyboard_heatmap(layout, key_weak),
        "errors": bar_chart(
            dict(sorted(beh.get("error_types", {}).items(), key=lambda t: -t[1])),
            "Error types (first attempts)",
            fmt="{:.0f}",
        ),
        "shap_err": bar_chart(insight.error_importance, "What drives errors (mean |SHAP|)")
        if insight
        else None,
        "shap_lat": bar_chart(
            insight.latency_importance, "What drives latency (mean |SHAP|)", "#5dade2"
        )
        if insight
        else None,
    }

    def img(k: str) -> str:
        return (
            f'<img src="{imgs[k]}">' if imgs.get(k) else "<p class=muted>Not enough data yet.</p>"
        )

    def fmt(v: Any, f: str = "{:.1f}") -> str:
        return "–" if v is None else f.format(v)

    base = beh.get("baseline", {})
    hc_rows = ""
    for name in ("drilled", "holdout"):
        g = hc.get(name)
        if g:
            delta = g["after"] - g["before"]
            hc_rows += (
                f"<tr><td>{name}</td><td>{g['items']}</td><td>{100 * g['before']:.1f}%</td>"
                f"<td>{100 * g['after']:.1f}%</td><td>{100 * delta:+.1f} pts</td></tr>"
            )
    cov = ""
    if coverage:
        cov = (
            f"<p>Calibration plan: {coverage['keystrokes']} keystrokes in {coverage['lessons']} lessons "
            f"over {coverage['sittings']} sittings; {coverage['units_short']} of {coverage['units_required']} "
            f"coverage targets short.</p>"
        )
    now = datetime.now(UTC).strftime("%Y-%m-%d %H:%M UTC")
    page = f"""<!doctype html><html lang=en><head><meta charset=utf-8>
<meta name=viewport content="width=device-width,initial-scale=1"><title>Qwertea report</title>
<style>
:root{{color-scheme:dark}}body{{background:{BG};color:{FG};font:15px/1.5 system-ui,sans-serif;margin:0;padding:24px;max-width:1080px;margin:auto}}
h1{{color:{ACCENT};font-weight:600;margin:0 0 4px}}h2{{font-weight:500;border-bottom:1px solid {GRID};padding-bottom:4px;margin-top:36px}}
.muted{{color:{MUTED}}}.cards{{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:12px;margin:20px 0}}
.card{{background:#26282e;border-radius:10px;padding:14px}}.card b{{display:block;font-size:26px;color:{ACCENT};font-weight:600}}
table{{border-collapse:collapse;width:100%;font-size:14px}}td,th{{padding:5px 8px;border-bottom:1px solid {GRID};text-align:left}}
th{{color:{MUTED};font-weight:500}}.mono{{font-family:ui-monospace,monospace}}img{{max-width:100%}}
.tag{{background:#3a3d45;border-radius:4px;padding:0 5px;font-size:11px;color:{MUTED}}}.grid2{{display:grid;grid-template-columns:1fr 1fr;gap:16px}}
@media(max-width:700px){{.grid2{{grid-template-columns:1fr}}}}
</style></head><body>
<h1>Qwertea</h1><div class=muted>Report generated {now}. Everything here was computed on your machine.</div>
<div class=cards>
<div class=card><b>{fmt(lvl.get("wpm"))}</b>WPM (recent)</div>
<div class=card><b>{fmt(100 * lvl["accuracy"] if "accuracy" in lvl else None)}%</b>accuracy (recent)</div>
<div class=card><b>{len(obs.lessons)}</b>lessons analysed</div>
<div class=card><b>{total_keys:,}</b>keystrokes analysed</div>
<div class=card><b>{"done" if not calib_left else f"{calib_left} left"}</b>calibration</div>
</div>
<h2>Progress</h2>{img("learning")}
<p class=muted>Speed probe {fmt(lvl.get("speed_probe_wpm"))} WPM · accuracy probe {fmt(lvl.get("accuracy_probe_wpm"))} WPM ·
speed–accuracy fit: accuracy = {fmt(lvl.get("sa_intercept"), "{:.3f}")} + {fmt(lvl.get("sa_slope"), "{:.4f}")} × WPM</p>
<h2>Keyboard weakness</h2><p class=muted>Combined weakness score per key against your own baseline (red = weaker, green = stronger).</p>{img("keyboard")}
<div class=grid2><div><h2>Weakest bigrams</h2>{table("bigram", 5)}</div><div><h2>Weakest trigrams</h2>{table("trigram", 4)}</div></div>
<h2>Weakest words</h2>{table("word", 3, 10)}
<div class=grid2><div><h2>Error profile</h2>{img("errors")}</div><div><h2>Behaviour</h2><table>
<tr><td>median time between keys</td><td>{fmt(beh.get("median_flight_ms"), "{:.0f} ms")}</td></tr>
<tr><td>rhythm variability (CV)</td><td>{fmt(beh.get("rhythm_cv"), "{:.2f}")}</td></tr>
<tr><td>burst share (fast runs)</td><td>{fmt(100 * beh["burst_share"] if "burst_share" in beh else None, "{:.0f}%")}</td></tr>
<tr><td>slow-down after an error</td><td>{fmt(beh.get("post_error_slowdown_ms"), "{:+.0f} ms")}</td></tr>
<tr><td>fatigue (slow-down per hour)</td><td>{fmt(100 * beh["fatigue_slope_per_hour"] if "fatigue_slope_per_hour" in beh else None, "{:+.1f}%")}</td></tr>
<tr><td>baseline latency</td><td>{fmt(base.get("lat_median_ms"), "{:.0f} ms")}</td></tr>
<tr><td>baseline error rate</td><td>{fmt(100 * base["err_rate"] if "err_rate" in base else None, "{:.2f}%")}</td></tr>
</table></div></div>
<h2>Does drilling help?</h2><p class=muted>Error rate of drilled items vs. holdout items (never drilled), before and after adaptive lessons began.</p>
{"<table><tr><th>group</th><th>items</th><th>before</th><th>after</th><th>change</th></tr>" + hc_rows + "</table>" if hc_rows else "<p class=muted>Not enough adaptive lessons yet.</p>"}
<h2>Insight model (LightGBM + SHAP)</h2>
{f"<p class=muted>Trained on {insight.rows:,} keystrokes. Held-out error AUC {fmt(insight.error_auc, '{:.3f}')}, latency R² {fmt(insight.latency_r2, '{:.3f}')}. The insight model explains; lesson choice relies on the Bayesian estimates.</p><div class=grid2><div>{img('shap_err')}</div><div>{img('shap_lat')}</div></div>" if insight else "<p class=muted>Needs about 1,500 analysed keystrokes (and the optional <code>lightgbm</code> package).</p>"}
<h2>Calibration</h2>{cov or "<p class=muted>–</p>"}
</body></html>"""
    path = out_dir / "index.html"
    path.write_text(page, encoding="utf-8")
    return path
