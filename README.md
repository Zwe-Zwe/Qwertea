# Qwertea

A personal, local-first, adaptive typing tutor. It runs a calibration session to learn your level, weak spots and error habits. After that, it builds each new lesson from the data collected in all your past sessions.

- **`client/`** (Rust): the window, keystroke capture and typing screen.
- **`engine/`** (Python): analysis and lesson generation.
- The two programs never call each other. They share one SQLite database, `data/qwertea.db`, in WAL mode.

`QWERTEA_SPEC.md` is the full specification. `docs/decisions.md` lists every deviation from it and the reason.

## Status

| Milestone | State |
|---|---|
| M0 Timing spike | Built. Verified headless; the real-keyboard evdev numbers are still to be recorded in `docs/timing-validation.md` |
| M1 Schema + DB layer | Done. Round-trip test passes |
| M2–M8 | Not started |

## Requirements
- Rust (stable, 1.95 or newer)
- Python 3.11+ and [uv](https://docs.astral.sh/uv/)
- **Linux** (primary target). You need the usual winit and wgpu runtime libraries: `libxkbcommon`, `libxkbcommon-x11` on X11, `libwayland-client` on Wayland, and a Vulkan or GL driver.
- Windows and macOS: builds without the `evdev-capture` feature. Timing then relies only on the winit receipt stamp (see the caveat below).

## Setup
```bash
make setup        # fetch crates, create the engine venv, create data/qwertea.db
make test         # Rust unit tests + engine tests (including the Rust↔Python round trip)
make lint         # cargo fmt/clippy -D warnings, ruff, mypy
```

You can also run things by hand:
```bash
uv --project engine run qwertea-engine init     # create DB from schema, load layout
(cd client && cargo run --bin qwertea)
```

`config.toml` in the repository root holds the layout, thresholds and paths. Set `QWERTEA_CONFIG` to use a different file.

## Timing check (M0)
```bash
make timing-check
# or, on Linux with kernel timestamps:
cd client && cargo run --release --features evdev-capture --bin timing_check -- --evdev
```
Type for about a minute, then press Esc. Copy the printed report into `docs/timing-validation.md`.

### evdev permissions (Linux)
Kernel timestamps are read from `/dev/input/event*`, which by default only the `input` group can read:
```bash
sudo usermod -aG input "$USER"   # then log out and back in
ls /dev/input/by-id/*-kbd        # find your keyboard; optionally set client.keyboard_device
```
If the device can't be opened, Qwertea falls back to winit stamps. Being in the `input` group lets your user read all input devices, so only do this on a machine you trust.

### Platform timing caveat
Without evdev (on Windows, macOS, or Linux without the feature), the stamp is when the window event *arrived* at the app. It is not frame-quantized, but it includes OS and compositor delivery delay. That delay is roughly constant and mostly cancels out in per-key comparisons, but absolute latencies carry it.

## Repository layout
```
client/   Rust crate: capture.rs, session.rs (LessonRun), db.rs, gfx.rs, config.rs, timing.rs
engine/   Python package qwertea_engine (db, layouts, cli), tests, data/layouts/qwerty-us.toml
schema/   001_init.sql, the authoritative schema, applied by both sides
docs/     decisions.md, timing-validation.md
data/     runtime data (git-ignored)
```

Everything stays on your machine. Nothing is collected or sent anywhere.
