# Qwertea

[![CI](https://github.com/Zwe-Zwe/Qwertea/actions/workflows/ci.yml/badge.svg)](https://github.com/Zwe-Zwe/Qwertea/actions/workflows/ci.yml) · version 1.0.0 · [changelog](CHANGELOG.md)

**A typing tutor that learns your weak spots.** Qwertea looks like the typing sites you know: clean text, a caret, live WPM. But its lessons are not fixed. Each one is brewed from *your* history: the keys, letter pairs and words you get wrong or type slowly, measured against your own baseline by a statistical model that updates after every lesson.

Everything runs locally. No accounts, no cloud, no telemetry.

![Typing](docs/screenshots/typing.png)

| | |
|---|---|
| ![Home](docs/screenshots/home.png) | ![Results](docs/screenshots/summary.png) |
| ![Stats](docs/screenshots/stats.png) | ![Weak spots](docs/screenshots/stats_keyboard.png) |

## Features

- **Distraction-free typing screen** in the style of popular typing sites: three visible lines that scroll with the caret, correct, wrong and pending characters in different colours, and a live clock, progress and WPM. Esc pauses; paused time is never counted. Losing window focus pauses automatically.
- **Results after every lesson:** WPM, raw WPM, accuracy, consistency, a per-second speed chart with error markers, correct/incorrect/fixed characters, your slowest letter pairs, and the keys you missed.
- **Dynamic lessons, not fixed exercises.** After a one-off calibration (3 short sittings that sample every key, letter pair and symbol), every lesson mixes:
  - **focused drill:** real words chosen because they contain your weakest patterns, plus generated pseudo-words for rare letter pairs;
  - **review:** patterns you improved, brought back on an expanding schedule so they stick;
  - **natural prose:** real sentences picked because they contain your targets.
- **Difficulty that follows you:** lesson length, word length and drill share adjust to keep your accuracy in the 92–96 % band.
- **Stats:** speed and accuracy history, a keyboard heatmap of weakness, and your weakest keys, letter pairs and words.
- **A full HTML report** with:
  - a learning curve with a power-law fit;
  - error types;
  - behaviour (slow-down after errors, rhythm, fatigue);
  - whether drilling actually helps, measured against held-out control items;
  - an explainable LightGBM model of what drives your errors and latency.
- **Accurate timing:** keystrokes are timestamped before the UI touches them, and kernel timestamps are optional on Linux. See [docs/timing-validation.md](docs/timing-validation.md).
- **Four themes** (dark, light, nord, matcha), adjustable text size, and an optional stop-on-error mode.

## Install

### Arch Linux

```bash
git clone https://github.com/Zwe-Zwe/Qwertea.git
cd Qwertea/packaging/arch
makepkg -si
```

Then start **Qwertea** from your application menu, or run `qwertea`. Once the package is published to the AUR, `yay -S qwertea` works too. See [packaging/arch/README.md](packaging/arch/README.md) for details, the `qwertea-git` package and optional extras.

An installed Qwertea keeps your settings in `~/.config/qwertea/config.toml` and your data in `~/.local/share/qwertea/`.

### From source (any OS)

You need [Rust](https://rustup.rs) (stable) and [uv](https://docs.astral.sh/uv/getting-started/installation/) (Python 3.11+ is installed by uv if needed).

```bash
git clone https://github.com/Zwe-Zwe/Qwertea.git
cd Qwertea
make run
```

`make run` installs the engine's Python dependencies, builds the app in release mode and starts it. The app starts the analysis engine in the background by itself. After the first build, `make run` (or `./client/target/release/qwertea` from the repository folder) starts in a second. Run from a source checkout, Qwertea uses the repository's `config.toml` and keeps its data in `data/`.

Without `make` (e.g. on Windows):

```bash
uv --project engine sync --all-extras
cd client
cargo run --release --bin qwertea
```

**Linux packages:** winit and wgpu need the usual desktop libraries, e.g. on Debian/Ubuntu `libxkbcommon-x11-0`, `libwayland-client0` and a Vulkan or OpenGL driver (`mesa-vulkan-drivers`). They are present on almost every desktop install.

## How to use it

1. **Calibrate** (about 3 × 15 minutes). Home shows "Sitting 1 of 3 · Phase 2 of 6 · ~9 min left". Each phase has a short intro; stop between phases whenever you like, and progress is kept. If you are impatient, *skip calibration* starts adaptive lessons from whatever you have typed so far.
2. **Practise.** Press Enter for the next lesson. The *focus* chips on Home show what the lesson targets.
3. **Check progress** under *stats*, or open the full *report*.

| Key | Action |
|---|---|
| Enter | start / next lesson |
| Esc | pause while typing (then r restart · s skip · q quit to menu); Esc again resumes |
| Backspace | move back one character |

## How it works

Qwertea is two programs that share one SQLite database (`data/qwertea.db`):

```
client/ (Rust)                         engine/ (Python)
window, typing screen,     ──────►     derives keystrokes, fits the model,
keystroke capture          SQLite      writes the next lessons and the report
           ◄──────────────────────────
```

- **Capture:** the client stamps every key event the moment the OS delivers it, before the UI runs. While you type it writes nothing to disk. When the lesson ends, it saves the raw key-down and key-up events in one transaction.
- **Derivation:** the engine turns raw events into labelled keystrokes. That covers:
  - timing: time between keys and how long each key is held, with pauses removed;
  - error type: substitution, slip onto a neighbouring key, transposition, omission or insertion;
  - corrections: how long you took to notice an error and fix it, and how much you slowed down after it.
- **Weakness model:** every key, letter pair, triple and word gets:
  - a **Bayesian error rate** (Beta posterior) that borrows strength from simpler patterns when data is thin (triple → pair → key → your overall rate);
  - a **robust latency score** against your own median.

  The weakness score combines the two.
- **Lesson choice:** **Thompson sampling** draws from those posteriors, so uncertain patterns get explored and confirmed weak ones get drilled. A few weak patterns are deliberately held out as a control group, so the report can show whether drilling really helps. Improved patterns go on a spaced-review ladder.
- **Insight model:** a **LightGBM** model predicts errors and latency from context (finger, hand, row, previous key, rhythm, fatigue). Its SHAP values in the report explain *why* you make mistakes.

The model is checked by a simulated typist with planted weaknesses. The test suite asserts that calibration recovers them and that adaptive lessons target them (`engine/tests/test_sim.py`).

The full design is in [QWERTEA_SPEC.md](QWERTEA_SPEC.md). Every deviation from it is recorded in [docs/decisions.md](docs/decisions.md).

![Report](docs/screenshots/report.png)

## Configuration

Settings changed in the app are saved to `config.toml` (comments kept). Other options:

| Key | Default | Meaning |
|---|---|---|
| `general.layout` | `qwerty-us` | keyboard layout (data file in `engine/data/layouts/`) |
| `client.stop_on_error` | `false` | caret waits for the right key |
| `client.present_mode` | `mailbox` | `mailbox` / `immediate` / `fifo` |
| `client.timestamp_source` | `winit` | `evdev` for kernel timestamps (Linux, see below) |
| `lesson.accuracy_target` | `[0.92, 0.96]` | difficulty control band |
| `lesson.lesson_chars` | `[150, 300]` | lesson length range |
| `calibration.sittings` | `3` | 1–3 calibration sittings |
| `engine.autostart` | `true` | start the engine with the app |

### Kernel timestamps (Linux, optional)

For the most accurate timing, build with `--features evdev-capture` and set `timestamp_source = "evdev"`. Reading `/dev/input` requires the `input` group:

```bash
sudo usermod -aG input "$USER"   # then log out and back in
```

Membership lets your user read all input devices, so only do this on a machine you trust. Without it, Qwertea falls back to window timestamps and shows a notice.

## Engine commands

The app runs `qwertea-engine serve` for you; the other commands are for power users.

```bash
uv --project engine run qwertea-engine --help
```

| Command | What it does |
|---|---|
| `serve` | watch the database, analyse finished lessons, keep 3 lessons queued, refresh the report |
| `run-once` | the same, once |
| `plan-calibration` | (re)create the calibration plan and print its coverage report |
| `rebuild` | re-derive everything from the raw key events |
| `report` | write `data/reports/index.html` |
| `recalibrate` | queue a short checkpoint passage |
| `fetch-corpus` | rebuild the prose corpus from public-domain books |

## Development

```bash
make test    # Rust unit tests + engine tests (incl. Rust↔Python round trip and the simulator)
make lint    # cargo fmt/clippy -D warnings, ruff, mypy --strict
make timing-check
```

```
client/   Rust app: capture, LessonRun state machine, DB, screens (ui/), timing spike
engine/   Python package qwertea_engine: derive, model/, select, review, generators/, report/
schema/   001_init.sql + numbered migrations (authoritative, applied by both sides)
docs/     decisions, timing validation, screenshots
packaging/ desktop entry, icon and Arch Linux PKGBUILDs
data/     your local database, report and logs when run from a checkout (git-ignored)
```

## Privacy

Your typing data lives on your computer and is never sent anywhere: in `~/.local/share/qwertea/` for an installed copy, or in `data/` in a source checkout (delete it with `make clean-data`).

## Licence

MIT. The bundled prose corpus is derived from public-domain Project Gutenberg books.
