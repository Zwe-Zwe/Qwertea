"""The engine loop: derive new lessons, refit the model, keep the queue full.

`run_once` is idempotent: running it twice in a row does nothing new.
"""

from __future__ import annotations

import json
import random
import sqlite3
from dataclasses import dataclass, field
from datetime import UTC, datetime
from typing import Any

from . import db, derive
from .corpus import Corpus
from .generators.base import Difficulty, LessonSpec, Target
from .generators.prose import ProseGenerator, apply_content
from .items import Item
from .layouts import Layout
from .model import baseline, bayes, level
from .model.observations import Observations, collect
from .paths import Config
from .review import ReviewState, due, review_state, target_history
from .select import pick_holdouts, thompson_select

CALIBRATION_PHASES = (
    "warmup",
    "natural",
    "coverage",
    "pseudo",
    "special",
    "speed",
    "accuracy",
    "retest",
)
ADAPTIVE_GEN = "prose/adaptive"
PROBE_GEN = "prose/probe"
CALIB_GEN = "prose/calibration"
PROBE_EVERY = 10
HOLDOUT_ROTATE_EVERY = 20


@dataclass(frozen=True)
class Settings:
    mode: str = "prose"
    layout: str = "qwerty-us"
    think_pause_ms: float = 2500.0
    w_err: float = 0.6
    w_lat: float = 0.4
    bonus: float = 0.25
    holdout_fraction: float = 0.05
    mix_weak: float = 0.6
    mix_review: float = 0.2
    acc_band: tuple[float, float] = (0.92, 0.96)
    min_queue: int = 3
    lesson_chars: tuple[int, int] = (150, 300)
    sittings: int = 3

    @classmethod
    def from_config(cls, cfg: Config) -> Settings:
        mix = cfg.get("lesson", "target_mix", {}) or {}
        acc = cfg.get("lesson", "accuracy_target", [0.92, 0.96])
        chars = cfg.get("lesson", "lesson_chars", [150, 300])
        return cls(
            mode=cfg.mode,
            layout=cfg.layout,
            think_pause_ms=float(cfg.get("derive", "think_pause_ms", 2500)),
            w_err=float(cfg.get("model", "weight_error", 0.6)),
            w_lat=float(cfg.get("model", "weight_latency", 0.4)),
            bonus=float(cfg.get("model", "uncertainty_bonus", 0.25)),
            holdout_fraction=float(cfg.get("model", "holdout_fraction", 0.05)),
            mix_weak=float(mix.get("weak", 0.6)),
            mix_review=float(mix.get("review", 0.2)),
            acc_band=(float(acc[0]), float(acc[1])),
            min_queue=int(cfg.get("lesson", "min_pending_queue", 3)),
            lesson_chars=(int(chars[0]), int(chars[1])),
            sittings=int(cfg.get("calibration", "sittings", 3)),
        )


LESSON_LENGTHS: dict[str, tuple[int, int]] = {
    "short": (90, 160),
    "medium": (150, 300),
    "long": (280, 460),
}


@dataclass(frozen=True)
class ProfilePrefs:
    """Lesson preferences the app stores in the profile (decision D28)."""

    lesson_chars: tuple[int, int] | None = None  # None = config.toml default
    capitals: bool = True
    punctuation: bool = True
    numbers: bool = False

    @classmethod
    def from_settings(cls, kv: dict[str, str]) -> ProfilePrefs:
        def flag(key: str, default: bool) -> bool:
            v = kv.get(key)
            return default if v is None else v.strip().lower() in ("1", "true", "yes", "on")

        length = kv.get("lesson_length", "medium")
        chars: tuple[int, int] | None = LESSON_LENGTHS.get(length)
        if length == "custom":
            try:
                n = max(40, min(2000, int(kv.get("lesson_chars", "220"))))
                chars = (n, n)
            except ValueError:
                chars = None
        elif length == "medium":
            chars = None
        return cls(chars, flag("capitals", True), flag("punctuation", True), flag("numbers", False))

    def fingerprint(self) -> str:
        return f"{self.lesson_chars}|{int(self.capitals)}{int(self.punctuation)}{int(self.numbers)}"


@dataclass
class ModelResult:
    obs: Observations
    bayes: bayes.BayesModel
    base: baseline.Baseline
    scores: dict[Item, baseline.ItemScore]
    level: level.Level
    behavior: dict[str, Any]


@dataclass
class RunSummary:
    derived: int = 0
    refit: bool = False
    queued: int = 0
    removed_stale: int = 0
    calibration_planned: int = 0
    snapshot_id: int | None = None
    notes: list[str] = field(default_factory=list)


class Engine:
    def __init__(
        self, conn: sqlite3.Connection, settings: Settings, *, seed: int | None = None
    ) -> None:
        self.conn = conn
        self.s = settings
        self.layout = Layout.load(settings.layout)
        self.corpus = Corpus.load()
        self.rng = random.Random(seed)
        self.gen = ProseGenerator(
            self.corpus, self.layout, seed=self.rng.randrange(1 << 30), sittings=settings.sittings
        )

    # ------------------------------------------------------------------ queue helpers

    def _next_order(self) -> int:
        return int(
            self.conn.execute("SELECT COALESCE(MAX(order_idx), 0) + 1 FROM lessons").fetchone()[0]
        )

    def _insert(
        self,
        spec: LessonSpec,
        generator: str,
        snapshot_id: int | None,
        order_idx: int | None = None,
    ) -> int:
        lid = db.insert_pending_lesson(
            self.conn,
            text=spec.text,
            phase=spec.phase,
            order_idx=order_idx if order_idx is not None else self._next_order(),
            mode=self.s.mode,
            targets_json=json.dumps(spec.targets) if spec.targets else None,
            is_discarded=spec.is_discarded,
            generator=generator,
            generator_version=self.gen.version,
            model_snapshot_id=snapshot_id,
        )
        if spec.meta:
            self.conn.execute(
                "UPDATE lessons SET meta_json=? WHERE id=?", (json.dumps(spec.meta), lid)
            )
        return lid

    def calibration_pending(self) -> int:
        q = ",".join("?" * len(CALIBRATION_PHASES))
        row = self.conn.execute(
            f"SELECT COUNT(*) FROM lessons "
            f"WHERE phase IN ({q}) AND status IN ('pending','running')",
            CALIBRATION_PHASES,
        ).fetchone()
        return int(row[0])

    # ------------------------------------------------------------------ calibration

    def plan_calibration(self, force: bool = False) -> tuple[int, dict[str, Any]]:
        existing = self.conn.execute(
            "SELECT COUNT(*) FROM lessons WHERE generator=?", (CALIB_GEN,)
        ).fetchone()[0]
        specs = self.gen.calibration_plan()
        report = self.gen.coverage_report(specs)
        if existing and not force:
            return 0, report
        with db.transaction(self.conn):
            order = self._next_order()
            for i, spec in enumerate(specs):
                self._insert(spec, CALIB_GEN, None, order + i)
        return len(specs), report

    # ------------------------------------------------------------------ model

    def fit(self) -> ModelResult:
        obs = collect(self.conn, self.s.mode, self.s.think_pause_ms)
        bm = bayes.fit(obs.items)
        base = baseline.fit_baseline(obs.items, bm)
        scores = {
            it: baseline.score(it, o, bm, base, w_err=self.s.w_err, w_lat=self.s.w_lat)
            for it, o in obs.items.items()
        }
        return ModelResult(obs, bm, base, scores, level.estimate(obs), level.behavior(obs))

    def _latest_snapshot(self) -> sqlite3.Row | None:
        row: sqlite3.Row | None = self.conn.execute(
            "SELECT * FROM model_snapshots WHERE mode=? ORDER BY id DESC LIMIT 1", (self.s.mode,)
        ).fetchone()
        return row

    def _adaptive_done(self) -> int:
        row = self.conn.execute(
            "SELECT COUNT(*) FROM lessons WHERE status='done' AND generator=?", (ADAPTIVE_GEN,)
        ).fetchone()
        return int(row[0])

    def _holdouts(self, result: ModelResult) -> tuple[set[Item], int]:
        snap = self._latest_snapshot()
        done = self._adaptive_done()
        if snap is not None and snap["notes"]:
            try:
                notes = json.loads(snap["notes"])
                hs = {Item(self.s.mode, k, v) for k, v in notes.get("holdouts", [])}
                since = int(notes.get("holdouts_since", 0))
                if hs and done - since < HOLDOUT_ROTATE_EVERY:
                    return hs, since
            except (ValueError, TypeError):
                pass
        scores = sorted(result.scores.values(), key=lambda s: s.item)
        return pick_holdouts(scores, self.s.holdout_fraction, self.rng), done

    def store(
        self, result: ModelResult, holdouts: set[Item], since: int, reviews: dict[Item, ReviewState]
    ) -> int:
        now = db.utc_now()
        with db.transaction(self.conn):
            self.conn.execute("DELETE FROM item_stats WHERE mode=?", (self.s.mode,))
            rows = []
            for it, sc in result.scores.items():
                post = result.bayes.posterior(it)
                rs = reviews.get(it)
                o = result.obs.items[it]
                rows.append(
                    (
                        it.mode,
                        it.kind,
                        it.value,
                        sc.n,
                        post.alpha,
                        post.beta,
                        sc.latency.median_log if sc.latency else None,
                        sc.latency.mad_log if sc.latency else None,
                        sc.weakness,
                        o.last_seen,
                        rs.last_drilled if rs else None,
                        rs.next_review if rs else None,
                        int(it in holdouts),
                    )
                )
            self.conn.executemany("INSERT INTO item_stats VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)", rows)
            beh = dict(result.behavior)
            beh["baseline"] = {
                "lat_median_ms": float(2.718281828**result.base.lat_median),
                "lat_scale_log": result.base.lat_scale,
                "err_rate": result.base.err_rate,
                "err_scale": result.base.err_scale,
                "prior_strengths": result.bayes.strengths,
            }
            notes = {
                "holdouts": sorted([it.kind, it.value] for it in holdouts),
                "holdouts_since": since,
            }
            cur = self.conn.execute(
                "INSERT INTO model_snapshots "
                "(created_at_utc, mode, level_json, behavior_json, notes) VALUES (?,?,?,?,?)",
                (
                    now,
                    self.s.mode,
                    json.dumps(result.level.to_json()),
                    json.dumps(beh),
                    json.dumps(notes),
                ),
            )
        assert cur.lastrowid is not None
        return cur.lastrowid

    # ------------------------------------------------------------------ lessons

    def prefs(self) -> ProfilePrefs:
        try:
            return ProfilePrefs.from_settings(db.profile_settings(self.conn))
        except sqlite3.OperationalError:  # database from before migration 3
            return ProfilePrefs()

    def difficulty(self, result: ModelResult) -> Difficulty:
        chars = self.prefs().lesson_chars or self.s.lesson_chars
        return level.difficulty(result.level, self.s.acc_band, chars)

    def build_lesson(
        self,
        result: ModelResult,
        holdouts: set[Item],
        reviews: dict[Item, ReviewState],
        exclude: set[Item],
    ) -> LessonSpec:
        diff = self.difficulty(result)
        scores = list(result.scores.values())
        picks = thompson_select(
            scores,
            self.corpus,
            self.rng,
            k=4,
            w_err=self.s.w_err,
            w_lat=self.s.w_lat,
            err_rate=result.base.err_rate,
            err_scale=result.base.err_scale,
            bonus=self.s.bonus,
            holdouts=holdouts,
            exclude=exclude,
        )
        review_items = [it for it in due(reviews, datetime.now(UTC)) if it not in holdouts][:2]
        targets = [Target(p.item, "weak", p.benefit) for p in picks]
        targets += [
            Target(it, "review") for it in review_items if all(t.item != it for t in targets)
        ]
        weights: dict[str, float] = {}
        for sc in scores:
            if (
                sc.item.kind in ("key", "bigram", "trigram")
                and sc.weakness > 0
                and sc.item not in holdouts
            ):
                weights[sc.item.value.lower() if sc.item.kind != "key" else sc.item.value] = (
                    sc.weakness
                )
        for t in targets:
            weights[t.item.value] = weights.get(t.item.value, 0.0) + 3.0
        for it in holdouts:
            weights[it.value] = -2.0  # keep holdouts out of drill words
        self.gen.weights = weights
        share = self.s.mix_weak + (self.s.mix_review if review_items else 0.0)
        diff = Difficulty(
            diff.length,
            diff.max_word_len,
            min(0.85, share * diff.drill_share / 0.6),
            diff.capitals,
            diff.punctuation,
        )
        prefs = self.prefs()
        diff = Difficulty(
            diff.length,
            diff.max_word_len,
            diff.drill_share,
            diff.capitals and prefs.capitals,
            diff.punctuation and prefs.punctuation,
        )
        spec = self.gen.generate(targets, diff.length, diff)
        spec.text = apply_content(
            spec.text, prefs.capitals, prefs.punctuation, prefs.numbers, self.rng
        )
        spec.meta["settings"] = prefs.fingerprint()
        spec.meta["difficulty"] = {
            "length": diff.length,
            "max_word_len": diff.max_word_len,
            "drill_share": round(diff.drill_share, 2),
        }
        spec.meta["level_wpm"] = round(result.level.wpm, 1)
        spec.meta["level_accuracy"] = round(result.level.accuracy, 3)
        return spec

    def refill(
        self,
        result: ModelResult,
        snapshot_id: int,
        holdouts: set[Item],
        reviews: dict[Item, ReviewState],
    ) -> tuple[int, int]:
        """Replace stale adaptive lessons and top the queue up to `min_queue`."""
        if self.calibration_pending():
            return 0, 0
        with db.transaction(self.conn):
            removed = self.conn.execute(
                "DELETE FROM lessons WHERE status='pending' AND generator=? "
                "AND COALESCE(model_snapshot_id, 0) < ?",
                (ADAPTIVE_GEN, snapshot_id),
            ).rowcount
            removed += self._delete_other_settings()
            pending = db.count_pending(self.conn)
            queued = 0
            probe_pending = self.conn.execute(
                "SELECT COUNT(*) FROM lessons WHERE generator=? AND status='pending'", (PROBE_GEN,)
            ).fetchone()[0]
            last_probe = (
                self.conn.execute(
                    "SELECT MAX(order_idx) FROM lessons WHERE generator=? AND status='done'",
                    (PROBE_GEN,),
                ).fetchone()[0]
                or 0
            )
            since_probe = self.conn.execute(
                "SELECT COUNT(*) FROM lessons "
                "WHERE generator=? AND status='done' AND order_idx > ?",
                (ADAPTIVE_GEN, last_probe),
            ).fetchone()[0]
            if not probe_pending and since_probe >= PROBE_EVERY:
                self._insert(self.gen.probe_lesson(), PROBE_GEN, snapshot_id)
                pending += 1
                queued += 1
            used: set[Item] = set()
            while pending < self.s.min_queue:
                spec = self.build_lesson(result, holdouts, reviews, exclude=used)
                for t in spec.targets:
                    used.add(Item(self.s.mode, t["kind"], t["item"]))
                self._insert(spec, ADAPTIVE_GEN, snapshot_id)
                pending += 1
                queued += 1
        return queued, removed

    def _other_settings_ids(self) -> list[int]:
        """Pending adaptive lessons brewed under different profile settings."""
        fp = self.prefs().fingerprint()
        ids = []
        for r in self.conn.execute(
            "SELECT id, meta_json FROM lessons WHERE status='pending' AND generator=?",
            (ADAPTIVE_GEN,),
        ):
            try:
                meta = json.loads(r["meta_json"] or "{}")
            except ValueError:
                meta = {}
            if meta.get("settings") != fp:
                ids.append(int(r["id"]))
        return ids

    def _pending_other_settings(self) -> int:
        return len(self._other_settings_ids())

    def _delete_other_settings(self) -> int:
        ids = self._other_settings_ids()
        for lid in ids:
            self.conn.execute("DELETE FROM lessons WHERE id=? AND status='pending'", (lid,))
        return len(ids)

    # ------------------------------------------------------------------ main entry

    def run_once(self, *, force_refit: bool = False) -> RunSummary:
        out = RunSummary()
        if not self.conn.execute("SELECT 1 FROM lessons LIMIT 1").fetchone():
            out.calibration_planned, _ = self.plan_calibration()
        new = derive.underived_lessons(self.conn)
        for lid in new:
            derive.derive_and_store(self.conn, lid, self.layout)
        out.derived = len(new)
        snap = self._latest_snapshot()
        needs_queue = not self.calibration_pending() and (
            db.count_pending(self.conn) < self.s.min_queue or self._pending_other_settings() > 0
        )
        if not (new or force_refit or snap is None or needs_queue):
            return out
        result = self.fit()
        holdouts, since = self._holdouts(result)
        history = target_history(self.conn, self.s.mode)
        reviews = {
            it: review_state(sc, history.get(it, []), result.base.err_rate)
            for it, sc in result.scores.items()
            if it in history
        }
        if new or force_refit or snap is None:
            out.snapshot_id = self.store(result, holdouts, since, reviews)
            out.refit = True
        else:
            out.snapshot_id = int(snap["id"])
        out.queued, out.removed_stale = self.refill(result, out.snapshot_id, holdouts, reviews)
        return out

    def rebuild(self) -> RunSummary:
        n = derive.rebuild_all(self.conn, self.layout)
        out = self.run_once(force_refit=True)
        out.notes.append(f"re-derived {n} keystrokes")
        return out

    def recalibrate(self) -> int:
        """Queue a mini-recalibration block (fixed probe text) at the front of the queue."""
        first = self.conn.execute(
            "SELECT MIN(order_idx) FROM lessons WHERE status='pending'"
        ).fetchone()[0]
        order = (first - 1) if first is not None else self._next_order()
        spec = self.gen.probe_lesson()
        with db.transaction(self.conn):
            self._insert(spec, PROBE_GEN, None, order)
        return 1
