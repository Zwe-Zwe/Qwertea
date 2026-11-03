# Qwertea: Project Specification for the Coding Agent

> **Qwertea** is a personal, local-first, adaptive typing tutor. It runs a large calibration session to learn the user's level, weaknesses and error behavior, then brews each new lesson from the data collected in all past sessions.
>
> This file is the single source of truth. Read it fully before writing code. Where it says MUST, it is a requirement. Where it says SHOULD, deviate only with a written reason in `docs/decisions.md`.

---

## 1. Context and goals

- **Audience:** one user (the author). Internal hobby project. No accounts, no cloud, no telemetry, no multi-user support.
- **Primary goal:** measurably improve the user's typing speed and accuracy by generating each lesson from a model of their personal weaknesses.
- **Secondary goals:** low input latency, trustworthy keystroke timing, and a clean codebase that is easy to extend.
- **Scope now:** one mode, `prose` (standard English typing).
- **Scope later (NOT now):** a `code` mode for programming-syntax typing. Do not build it, but the design MUST NOT block it (see section 13).

### Non-goals
- No web app, no mobile app, no multiplayer, no leaderboards, no gamification beyond simple progress charts.
- No deep learning or reinforcement learning in v1.

---

## 2. Architecture overview

Two programs share one SQLite database. They never call each other directly.

```
┌──────────────────────────┐        ┌──────────────────────────────┐
│  client/  (Rust)         │        │  engine/  (Python)           │
│  UI + keystroke capture  │        │  analysis + lesson brewing   │
│                          │        │                              │
│  reads pending lessons   │        │  watches for finished        │
│  runs the typing session │        │  sessions, derives features, │
│  writes RAW key events   │        │  updates the model, writes   │
│  marks lesson done       │        │  new pending lessons         │
└────────────┬─────────────┘        └───────────────┬──────────────┘
             │                                      │
             └──────────────►  data/qwertea.db  ◄──┘
                              (SQLite, WAL mode)
```

**Rules**
1. The Rust client MUST be the only writer of `key_events`, `sessions` start/end fields and lesson `status='done'`.
2. The Python engine MUST be the only writer of `keystrokes`, `item_stats`, `model_snapshots` and new `pending` lessons.
3. The client MUST write to the database only between lessons, in one transaction. Nothing touches disk during typing.
4. Raw events are stored losslessly. Everything else is derived and MUST be rebuildable from raw events with one command (`qwertea-engine rebuild`).

---

## 3. Tech stack

### Client (Rust, stable toolchain, edition 2021+)
| Concern | Choice |
|---|---|
| Windowing and input | `winit` (own event loop, see section 5.1) |
| UI | `egui` via `egui-winit` + `egui-wgpu` driven manually (NOT `eframe`) |
| Database | `rusqlite` (bundled feature) |
| Linux kernel timestamps | `evdev` crate, behind a cargo feature `evdev-capture` |
| Config | `serde` + `toml` |
| Time | `std::time::Instant` (monotonic) |

### Engine (Python 3.11+, managed with `uv`)
| Concern | Choice |
|---|---|
| Data | `numpy`, `pandas` (or `polars`) |
| Stats / Bayes | `scipy` for v1 (empirical-Bayes Beta-Binomial). `pymc` is optional and later |
| ML | `lightgbm`, `shap`, `scikit-learn` |
| CLI | `typer` |
| Plots | `matplotlib` (reports and heatmaps saved as PNG/HTML) |
| Tests | `pytest`, `hypothesis` |
| Lint/type | `ruff`, `mypy` |

### Platform
- **Primary target: Linux** (X11 or Wayland). Must also compile and run on Windows and macOS, but with `evdev-capture` disabled and a documented timing caveat.
- Keyboard layout default: **QWERTY US**, configurable in `config.toml`. The finger/hand/row map MUST live in a data file (`engine/data/layouts/qwerty-us.toml`), not hard-coded.

---

## 4. Repository layout

```
qwertea/
├── README.md
├── QWERTEA_SPEC.md              # this file
├── config.toml                   # layout, thresholds, paths
├── Makefile                      # setup, run-client, run-engine, test, lint
├── docs/
│   ├── decisions.md              # any deviation from this spec + why
│   └── timing-validation.md      # results of the timing spike (M0)
├── schema/
│   └── 001_init.sql              # authoritative schema, applied by both sides
├── client/                       # Rust crate
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs               # event loop, wiring
│       ├── capture.rs            # TimestampSource trait + implementations
│       ├── session.rs            # LessonRun state machine
│       ├── ui/
│       │   ├── mod.rs
│       │   ├── typing_view.rs    # the hot typing screen
│       │   ├── home.rs
│       │   ├── summary.rs
│       │   └── theme.rs
│       ├── db.rs                 # read pending lessons, flush events
│       └── config.rs
├── engine/                       # Python package `qwertea_engine`
│   ├── pyproject.toml
│   ├── qwertea_engine/
│   │   ├── cli.py                # typer entry: init, plan-calibration, run-once, serve, rebuild, report
│   │   ├── db.py
│   │   ├── layouts.py            # loads layout TOML, finger/hand/row lookup
│   │   ├── derive.py             # key_events -> keystrokes
│   │   ├── items.py              # Item dataclass, n-gram extraction (prose)
│   │   ├── model/
│   │   │   ├── baseline.py       # robust z-scores per item
│   │   │   ├── bayes.py          # empirical-Bayes error posteriors + backoff
│   │   │   ├── latency.py        # log-latency medians/MAD per item
│   │   │   ├── gbdt.py           # LightGBM + SHAP (v3)
│   │   │   └── level.py          # WPM/accuracy/speed-accuracy curve
│   │   ├── select.py             # Thompson sampling, benefit scoring
│   │   ├── review.py             # spaced-repetition scheduling
│   │   ├── generators/
│   │   │   ├── base.py           # LessonGenerator Protocol
│   │   │   ├── prose.py          # ProseGenerator (only one for now)
│   │   │   └── setcover.py       # greedy set-cover for calibration text
│   │   └── report/               # dashboards, heatmaps, learning curves
│   ├── data/
│   │   ├── layouts/qwerty-us.toml
│   │   ├── corpora/prose/        # fetched by scripts, git-ignored
│   │   └── wordfreq/             # frequency list
│   ├── scripts/fetch_corpus.py   # downloads public-domain books + word frequencies
│   └── tests/
└── data/                         # runtime data, git-ignored
    └── qwertea.db
```

---

## 5. Client requirements (Rust)

### 5.1 Input timing: the most important requirement
Timing accuracy is the product. The agent MUST NOT use `eframe`, because egui collects input events per frame, so any timestamp taken inside egui code is quantized to the frame time and corrupts latency data.

**Required design**
- Run the `winit` event loop directly, and drive `egui-winit` / `egui-wgpu` manually.
- Take the timestamp **in `window_event`, on the first line, before the event is passed to egui or any other logic**.
- Define a trait in `capture.rs`:

```rust
pub trait TimestampSource {
    /// Monotonic nanoseconds since lesson start for a key event.
    fn stamp(&self, ev: &RawKeyInfo) -> u64;
}
```

  with two implementations:
  1. `WinitReceiptStamp`: `Instant::now()` taken at the top of `window_event`. Default, works everywhere.
  2. `EvdevStamp` (feature `evdev-capture`, Linux only): a dedicated thread reads `/dev/input/event*` for the configured keyboard and records **kernel timestamps** (`CLOCK_MONOTONIC`) for key down/up. Events are correlated with the UI-thread events by scancode and order. Needs the user in the `input` group, so handle permission failure gracefully by falling back to `WinitReceiptStamp` and showing a banner.
- Both stamps SHOULD be stored when available: `t_ns` (primary) and `t_kernel_ns` (nullable). This lets the user validate that the UI adds no delay.

**Hot path rules**
- Pre-allocate the event buffer (`Vec::with_capacity(32_768)`). No heap allocation, disk I/O, logging, or locking in the key handler.
- Only repaint what changed (the typed character and caret) where practical, and never block the event loop.
- Present mode: prefer `Mailbox`, then `Immediate`, then `Fifo`, configurable in `config.toml`.
- Record both key-down and key-up for every key, including Shift, Backspace, and modifiers.

### 5.2 Typing semantics
- The lesson text is shown with the cursor on the next expected character.
- Any printable key press advances the cursor, whether correct or wrong (wrong chars are shown in a distinct style). It does NOT block on errors by default (`stop_on_error = false` in config; implement the flag).
- Backspace moves the cursor back one position and un-marks that character. Backspace events are logged as normal events.
- A lesson ends when the cursor passes the final character. The client ends the lesson immediately and shows the summary.
- A key press that is not text (arrows, function keys) is logged but ignored for cursor logic.
- Use the character produced by the OS/layout (`produced_char`) plus the physical `scancode`. Handle dead keys and IME composition by logging them but not advancing the cursor on partial composition.
- Pause: `Esc` pauses and discards nothing. Time spent paused MUST be excluded (log a `pause` marker event pair).

### 5.3 Screens
1. **Home**: shows the next pending lesson info, calibration progress (sitting X of 3, coverage %), last-session WPM/accuracy, and buttons: Start, Reports (opens generated report in the browser), Settings.
2. **Typing view**: the text, the caret, a minimal live HUD (elapsed time, optional WPM). Everything else hidden to avoid distraction. Large, high-contrast monospace font. Configurable font size.
3. **Lesson summary**: WPM, accuracy, errors, top 3 slowest bigrams of the lesson. Continue / Quit.
4. **Calibration flow**: a guided sequence with clear phase titles and a break screen between phases (see section 7). Always show "Sitting 1 of 3 · Phase 4 of 7 · ~6 min left".
5. **Settings**: layout, font size, stop-on-error, present mode, timestamp source, keyboard device (for evdev).

Design: clean, calm, dark theme by default, with a light theme option. Keep the UI minimal.

### 5.4 Database access (client side)
- On start: open the DB in WAL mode, apply `schema/001_init.sql` if the schema version is missing, and read the next `pending` lesson ordered by `lessons.order_idx`.
- After a lesson ends: in **one transaction**, insert all `key_events`, update `lessons.status='done'` with `finished_at_utc`, and update the session row.
- If no `pending` lesson exists, show "Engine is preparing your next lesson" and offer a button that re-checks the DB. The client never generates lessons.
- Crash safety: on startup, a lesson in `status='running'` is marked `status='aborted'` and excluded from analysis.

---

## 6. Database contract (SQLite)

The schema file `schema/001_init.sql` is authoritative. Add a `schema_version` table. Use WAL mode and `PRAGMA foreign_keys=ON`.

```sql
CREATE TABLE schema_version (version INTEGER NOT NULL);

CREATE TABLE sessions (
  id INTEGER PRIMARY KEY,
  started_at_utc TEXT NOT NULL,
  ended_at_utc TEXT,
  kind TEXT NOT NULL,                 -- 'calibration' | 'lesson' | 'probe'
  mode TEXT NOT NULL DEFAULT 'prose', -- later: 'code'
  sitting_no INTEGER,                 -- calibration sitting 1..3, else NULL
  layout TEXT NOT NULL,
  keyboard TEXT,
  timestamp_source TEXT NOT NULL,     -- 'winit' | 'evdev'
  client_version TEXT
);

CREATE TABLE lessons (
  id INTEGER PRIMARY KEY,
  session_id INTEGER REFERENCES sessions(id),  -- set by client when run
  order_idx INTEGER NOT NULL,                  -- run order
  mode TEXT NOT NULL DEFAULT 'prose',
  language TEXT,                               -- NULL now; 'rust','python',... later
  phase TEXT NOT NULL,   -- warmup|natural|coverage|pseudo|special|speed|accuracy|retest|drill|review|mixed
  text TEXT NOT NULL,
  targets_json TEXT,                           -- [{"kind":"bigram","item":"th","reason":"weak"}]
  is_discarded INTEGER NOT NULL DEFAULT 0,     -- warmup etc., excluded from model fitting
  status TEXT NOT NULL DEFAULT 'pending',      -- pending|running|done|aborted
  created_at_utc TEXT NOT NULL,
  finished_at_utc TEXT,
  generator TEXT, generator_version TEXT, model_snapshot_id INTEGER
);

-- RAW capture: written only by the client, never modified
CREATE TABLE key_events (
  lesson_id INTEGER NOT NULL REFERENCES lessons(id),
  seq INTEGER NOT NULL,
  t_ns INTEGER NOT NULL,             -- monotonic ns since lesson start (primary source)
  t_kernel_ns INTEGER,               -- optional evdev timestamp, same origin
  scancode INTEGER NOT NULL,
  is_down INTEGER NOT NULL,
  produced_char TEXT,                -- NULL for non-text keys
  is_backspace INTEGER NOT NULL DEFAULT 0,
  is_pause_marker INTEGER NOT NULL DEFAULT 0,
  target_idx INTEGER NOT NULL,       -- cursor index in lesson text when the event happened
  PRIMARY KEY (lesson_id, seq)
) WITHOUT ROWID;

-- DERIVED: written only by the engine, rebuildable
CREATE TABLE keystrokes (
  lesson_id INTEGER NOT NULL,
  seq INTEGER NOT NULL,              -- seq of the key-down event this row describes
  target_idx INTEGER NOT NULL,
  expected TEXT, typed TEXT,
  is_first_attempt INTEGER NOT NULL, -- first attempt at this target position
  error_type TEXT,                   -- NULL|substitution|transposition|omission|insertion|adjacent_slip
  dwell_ms REAL, flight_ms REAL,     -- flight = time since previous key-down
  prev1 TEXT, prev2 TEXT,
  finger TEXT, hand TEXT, row INTEGER,
  corrected INTEGER, time_to_notice_ms REAL, time_to_correct_ms REAL,
  post_error_slowdown_ms REAL,       -- mean extra latency over next 3 keys
  session_pos REAL,                  -- 0..1 position within the session (fatigue/learning)
  PRIMARY KEY (lesson_id, seq)
) WITHOUT ROWID;

CREATE TABLE item_stats (
  mode TEXT NOT NULL,
  item_kind TEXT NOT NULL,           -- key|bigram|trigram|word (later: token|syntax_pattern)
  item TEXT NOT NULL,
  n INTEGER NOT NULL,
  err_alpha REAL NOT NULL, err_beta REAL NOT NULL,   -- Beta posterior for P(error)
  lat_median_log REAL, lat_mad_log REAL,
  weakness REAL,                     -- combined score, higher = weaker
  last_seen_utc TEXT, last_drilled_utc TEXT,
  next_review_utc TEXT,
  is_holdout INTEGER NOT NULL DEFAULT 0,  -- control item, never drilled
  PRIMARY KEY (mode, item_kind, item)
);

CREATE TABLE model_snapshots (
  id INTEGER PRIMARY KEY,
  created_at_utc TEXT NOT NULL,
  mode TEXT NOT NULL,
  level_json TEXT,                   -- WPM, accuracy, speed-accuracy curve params
  behavior_json TEXT,                -- post-error slowdown, rhythm variance, fatigue slope
  notes TEXT
);

CREATE INDEX idx_events_lesson ON key_events(lesson_id);
CREATE INDEX idx_keystrokes_lesson ON keystrokes(lesson_id);
```

**Handoff protocol**
1. The engine inserts `pending` lessons with increasing `order_idx`.
2. The client runs the lowest `order_idx` pending lesson, sets `running`, then `done` on completion.
3. The engine (running `serve` or `run-once`) detects newly `done`, non-discarded lessons, derives keystrokes, updates stats and model, and inserts new `pending` lessons so there are always at least **3 pending** lessons queued.

---

## 7. Calibration session (a core feature)

Calibration exists to collect enough dense data up front to avoid cold start. The engine creates the whole calibration plan once, via `qwertea-engine plan-calibration`, as `pending` lessons with `session kind='calibration'`.

### 7.1 Size
- **8,000-12,000 keystrokes total** (about 1,600-2,400 words), split into **3 sittings of 10-15 minutes**. The client MUST allow stopping between phases and resuming later. Sittings are separate sessions with `sitting_no` 1..3.

### 7.2 Coverage targets (the planner MUST verify these and report any shortfall)
| Item | Minimum samples |
|---|---|
| Every single key: letters, digits, punctuation, space, Enter, Shift-capitals | 40 each (letters), 20 each (digits/punctuation) |
| Top 300 English bigrams | 15 each |
| Remaining letter bigrams | 3-5 each (via pseudo-words) |
| Top 150 trigrams | 8 each |
| Numbers and number-row sequences | 20 each |

### 7.3 Phases
Order is counterbalanced across sittings (the plan records the phase order used).
1. **warmup** (about 1 min): easy text, `is_discarded=1`.
2. **natural**: real sentences from the corpus, to measure realistic baseline.
3. **coverage**: words chosen by **greedy set-cover** over the dictionary: repeatedly pick the word (or short phrase) that adds the most still-undersampled n-grams, with a diversity penalty, until targets are met.
4. **pseudo**: Markov-generated pseudo-words for rare bigrams and trigrams that real words don't cover.
5. **special**: capitals, digits, punctuation, mixed.
6. **speed / accuracy probes**: one block "as fast as possible", one "as accurately as possible", on matched text. Gives the speed-accuracy tradeoff.
7. **retest** (end of the final sitting): a small fixed set of items repeated from earlier to measure practice effect and noise.

### 7.4 Calibration outputs (produced by the engine after the final sitting)
1. **Level estimate**: natural-pace WPM and accuracy, plus the speed-accuracy curve. Sets starting difficulty.
2. **Baseline distributions** per key, bigram, finger, hand (median log-latency, MAD, error rate). All later weakness scores are measured against the user's OWN baseline.
3. **Error profile**: error-type frequencies and the contexts that trigger them.
4. **Behavioral profile**: post-error slowdown, rhythm variance, fatigue slope, burst-vs-steady behavior.
5. **Initial item_stats** with backoff trigram → bigram → unigram where data is thin.
6. A human-readable **calibration report** (HTML) with a weakness heatmap on the keyboard layout.

---

## 8. Engine requirements (Python)

### 8.1 Derivation (`derive.py`)
Turns `key_events` into `keystrokes`. MUST be deterministic and idempotent.
- **dwell_ms** = key-up time − key-down time for the same scancode.
- **flight_ms** = time since the previous key-down.
- **First attempt** at each target position determines the error label. Later corrections are tracked via `corrected`, `time_to_notice_ms` (time from error to the first backspace), and `time_to_correct_ms`.
- **Error classification**: `substitution`, `transposition` (two adjacent expected chars swapped), `omission`, `insertion`, `adjacent_slip` (substitution where typed key is physically adjacent to expected on the layout). Define precedence in code and document it.
- **Cleaning rules** (configurable in `config.toml`): drop latencies above `think_pause_ms` (default 2500) from latency stats but keep them as events; exclude `is_discarded` lessons; exclude `aborted` lessons; ignore the first keystroke of each lesson for flight time.
- Use **log-latency** and **medians/MAD** for all latency statistics.

### 8.2 Items (`items.py`)
```python
@dataclass(frozen=True)
class Item:
    mode: str       # 'prose'
    kind: str       # 'key' | 'bigram' | 'trigram' | 'word'
    value: str
```
Extraction for prose: unigrams (keys, including shifted forms), bigrams, trigrams, words.

### 8.3 Weakness model (`model/`)
Implement in this order. Each layer MUST work without the next.

1. **Baseline z-scores** (`baseline.py`): per item, robust z of log-latency versus the user's global baseline, plus error-rate versus the user's overall rate. Explainable and always on.
2. **Empirical-Bayes error posteriors** (`bayes.py`): per item a Beta posterior for P(error). Priors come from a hierarchy: trigram shrinks toward its bigram, bigram toward its key/finger, key toward the global rate. Fit hyperparameters from calibration data (method of moments is fine). Provide `sample_posterior(item)` for Thompson sampling.
3. **Weakness score**: `weakness = w_e * z(error_rate) + w_l * z(latency)`, weights in config, with an uncertainty bonus from posterior width.
4. **LightGBM context model** (`gbdt.py`, milestone M6): predict P(error) and log-latency from context features (prev1, prev2, finger, hand, row, finger-transition type, same-finger flag, hand alternation, previous latency, position in session, shift involved, etc.). Use `shap` to explain. It acts as a residual/insight layer on top of the Bayesian estimates and is never the sole source of truth.
5. **Level model** (`level.py`): current WPM, accuracy, and a speed-accuracy tradeoff fit (accuracy as a function of speed) used for difficulty control.

### 8.4 Selection (`select.py`)
- **Target selection:** benefit = `weakness * real_text_frequency * (1 + uncertainty_bonus)`, then **Thompson sampling** from the posteriors to pick the top-k items per lesson. Real-text frequency comes from the corpus.
- **Holdouts:** reserve about 5% of weak items as `is_holdout=1`, never drilled, used to verify that drilling helps. Rotate the holdout set every N lessons.
- **Spaced review (`review.py`):** after an item improves (posterior mean error and latency below thresholds), schedule `next_review_utc` with expanding intervals (start with a simple Leitner-style ladder; FSRS-style later). Items due for review are included in lessons.

### 8.5 Lesson generation (`generators/`)
Protocol every generator implements:
```python
class LessonGenerator(Protocol):
    mode: str
    def item_kinds(self) -> list[str]: ...
    def extract_items(self, text: str) -> list[Item]: ...
    def calibration_plan(self) -> list[LessonSpec]: ...
    def generate(self, targets: list[Item], budget: int, difficulty: Difficulty) -> Lesson: ...
```
`ProseGenerator` MUST support:
1. **Word ranking**: score each dictionary word by the summed weakness of its n-grams, pick greedily with a diversity penalty so a single pattern does not dominate.
2. **Markov pseudo-words**: n-gram-rich drill strings for isolating a specific bigram/trigram.
3. **Natural sentences**: sentences pulled from the corpus that contain target items, chosen by coverage per character.
4. **LLM sentences (optional, behind a flag, default OFF):** generate readable text with an LLM, then verify programmatically that target coverage is met; fall back to 1-3 if not. Do not build this until M7.

**Lesson mix** (configurable): about 60% targeted weak spots, 20% spaced review, 20% normal prose.
**Difficulty control:** aim for 92-96% accuracy; adjust lesson length, word length, and the percentage of drilling based on the level model. Typical lesson: 150-300 characters.

### 8.6 Engine CLI
```
qwertea-engine init                 # create DB from schema, load layout
qwertea-engine fetch-corpus         # run scripts/fetch_corpus.py
qwertea-engine plan-calibration     # create calibration lessons (pending)
qwertea-engine run-once             # process all new 'done' lessons, refill queue
qwertea-engine serve                # poll the DB every 2s and run-once as needed
qwertea-engine rebuild              # drop and re-derive keystrokes, item_stats from raw events
qwertea-engine report               # write HTML reports to data/reports/
qwertea-engine recalibrate          # queue a mini-recalibration block (2-3 min of fixed probes)
```

### 8.7 Reports (`report/`)
HTML (static, self-contained) with: WPM and accuracy over time with a power-law learning-curve fit, keyboard heatmap of weakness, top weak bigrams/trigrams with trend, error type breakdown, holdout versus drilled improvement comparison, and (after M6) SHAP summary.

---

## 9. Keeping the data honest
- **Mini-recalibration** every 10 lessons: 2-3 minutes of fixed probe text, to detect drift and practice effects.
- **Fixed-position probes**: periodically retest identical strings so the learning curve is not confounded by content.
- **Holdout items** as described in section 8.4.
- Log which `model_snapshot_id` and generator version produced each lesson so results can be traced.

---

## 10. Configuration (`config.toml`)
```toml
[general]
mode = "prose"
layout = "qwerty-us"
db_path = "data/qwertea.db"

[client]
font_size = 28
theme = "dark"
stop_on_error = false
present_mode = "mailbox"        # mailbox | immediate | fifo
timestamp_source = "winit"      # winit | evdev
keyboard_device = ""            # e.g. /dev/input/event3 (for evdev)

[derive]
think_pause_ms = 2500
min_samples_for_stats = 3

[model]
weight_error = 0.6
weight_latency = 0.4
uncertainty_bonus = 0.25
holdout_fraction = 0.05

[lesson]
target_mix = { weak = 0.6, review = 0.2, normal = 0.2 }
accuracy_target = [0.92, 0.96]
min_pending_queue = 3
lesson_chars = [150, 300]

[calibration]
sittings = 3
```

---

## 11. Testing and quality

**Engine (pytest)**
- Derivation: hand-built `key_events` fixtures covering each error type, corrections, backspace chains, pauses, and non-text keys. Property tests (hypothesis): derivation is deterministic and idempotent; `rebuild` equals incremental processing.
- Set-cover planner: meets all coverage targets for the bundled corpus; reports shortfalls.
- Bayes: posteriors shrink correctly with little data and converge with a lot of data; backoff order is respected.
- Selection: holdouts are never drilled; targets are diverse.
- Synthetic user simulator (`tests/sim.py`): generate fake keystroke data from a known per-item error/latency profile, run the pipeline, and assert the model recovers the true weak items. This doubles as the end-to-end test.

**Client (cargo test)**
- `LessonRun` state machine: cursor logic, backspace, pause exclusion, end-of-lesson.
- A **timing test binary** (`cargo run --bin timing_check`): measures timestamp quantization and optionally compares `winit` stamps with `evdev` stamps.
- DB round-trip: events written by the client are readable by the engine.

**Quality gates:** `cargo clippy -- -D warnings`, `cargo fmt --check`, `ruff`, `mypy` clean. CI-style `make test` runs everything.

---

## 12. Milestones (build in this order, and stop for review after each)

| # | Milestone | Acceptance criteria |
|---|---|---|
| **M0** | **Timing spike** | Minimal Rust window that stamps key events in `window_event` and optionally via `evdev`. `docs/timing-validation.md` reports median and p99 difference between sources, and confirms timestamps are NOT frame-quantized. Decision recorded in `docs/decisions.md`. |
| **M1** | **Schema + DB layer** | `schema/001_init.sql` applied by both sides. Rust can read a pending lesson and write `key_events`. Python can read them. Round-trip test passes. |
| **M2** | **Typing client** | Home, typing view, summary. Correct cursor/backspace/pause semantics, raw event flush after the lesson, crash recovery. Works on manually inserted lessons. |
| **M3** | **Derivation** | `derive.py` + `rebuild`, all error types, cleaning rules, full test coverage with fixtures. |
| **M4** | **Calibration planner + flow** | Corpus fetch script, set-cover + Markov generators, `plan-calibration` produces the 3-sitting plan with a coverage verification report. Client runs the full guided calibration with breaks and resumes between sittings. |
| **M5** | **Baseline + Bayes model + reports** | Level estimate, baselines, hierarchical Beta posteriors, weakness scores, calibration HTML report with keyboard heatmap. |
| **M6** | **Adaptive lessons** | `serve` loop: Thompson-sampling selection, word-ranking generator, lesson mix, difficulty control, queue refill, holdouts, spaced review. The simulator test shows the model recovers planted weaknesses. |
| **M7** | **Insight layer** | LightGBM + SHAP, mini-recalibration, learning-curve fits, holdout-vs-drilled comparison, optional LLM sentence generator behind a flag. |
| **M8** | **Polish** | Settings screen, themes, README with setup, packaging scripts, `Makefile` targets. |

---

## 13. Future coding mode (design constraints only, do NOT build)

The user plans a `code` mode later for practicing programming syntax. To avoid blocking it:
- `mode` is a column on `sessions`, `lessons` and `item_stats`, and all statistics are keyed by `mode` so prose and code baselines never mix.
- `item_kind` is a free string. Code mode will add kinds such as `token` (`=>`, `::`, `&mut`), `syntax_pattern`, `identifier`, and bracket pairing.
- Item extraction lives inside each `LessonGenerator`, so a future `CodeGenerator` plugs in without touching the estimator, selector, or scheduler.
- `lessons.language` plus `data/corpora/<mode>/<language>/` is reserved for language-specific corpora.
- The client MUST NOT assume lesson text is only prose: support multi-line text, tabs/indentation, and symbols in the typing view (render and capture them correctly), even though v1 lessons will be single-paragraph prose.
- Derivation and error semantics are per-mode and extensible.

For now, every row uses `mode='prose'`.

---

## 14. Working rules for the agent

1. Work milestone by milestone. At the end of each, summarize what was built, how to run it, and what is untested.
2. Do not add features that are not in this spec. If something is ambiguous, choose the simplest option that keeps the data contract intact, and note it in `docs/decisions.md`.
3. Never change the schema silently. Schema changes require a new numbered migration file and a note in `docs/decisions.md`.
4. Never let analysis code run in the client's key-handling path.
5. Keep derived data rebuildable from `key_events` at all times.
6. Write tests with each module, not afterwards.
7. Keep dependencies minimal and pinned (`Cargo.lock`, `uv.lock` committed).
8. Do not collect or transmit any data outside the local machine.
9. Document setup (including `input` group permissions for evdev) in `README.md`.
