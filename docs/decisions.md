# Decisions and deviations from the spec

Each entry records a choice the spec left open, or a deliberate deviation, and why.

## D1: Crate versions (M0)
`winit 0.30`, `egui`/`egui-winit`/`egui-wgpu 0.37` (which pins `wgpu 30`), `rusqlite 0.41` (bundled), `evdev 0.13`.
egui 0.37 is the newest egui and is built against winit 0.30, so the versions match. `eframe` is not a dependency.

egui 0.37 no longer bundles fonts unless the `default_fonts` feature is on (`eframe` normally turns it on). The client enables it explicitly; without it every glyph renders as an empty box.

## D2: `TimestampSource::stamp` returns `Option<u64>`
The spec's signature returns `u64`. A kernel source can legitimately have no observation for a UI event: the event was synthetic, came from another device, or the reader thread dropped it. Returning `Option` avoids a magic sentinel value. `WinitReceiptStamp` always returns `Some`.

## D3: evdev stamps are correlated after the lesson, not in the key handler
The evdev reader thread appends `(code, down/up, CLOCK_MONOTONIC ns)` to its own buffer. The key handler only records the winit receipt stamp. After the lesson ends, and before the flush, `EvdevStamp` matches events in order per `(scancode, is_down)`. A match requires the kernel event to come before the receipt (with 2 ms tolerance) and to be at most 500 ms older than it.

This keeps matching, locking and any waiting for the reader thread out of the hot path. The spec already forbids locking there.

`CLOCK_MONOTONIC` is selected with the `EVIOCSCLOCKID` ioctl, because evdev defaults to `CLOCK_REALTIME`. The lesson origin is read as both an `Instant` and a `clock_gettime(CLOCK_MONOTONIC)` value, back to back, so the two clocks share an origin. On Linux, `Instant` is `CLOCK_MONOTONIC`.

## D4: `t_ns` is always the winit receipt stamp; `t_kernel_ns` is the evdev stamp
Section 5.1 asks for both stamps so the UI delay can be validated. That needs the winit stamp stored even when evdev is active. So `t_ns` always holds the winit receipt stamp and `t_kernel_ns` holds the kernel stamp when one matched.

`sessions.timestamp_source` records which source was configured. When it is `'evdev'`, the engine should prefer `t_kernel_ns` where it is non-NULL and fall back to `t_ns`.

## D5: `t_ns` is raw time since lesson start; pauses are excluded downstream
Raw events stay lossless. `t_ns` is not shifted to remove paused time. Instead, each pause writes a marker pair: `is_pause_marker=1`, `is_down=1` when the pause starts and `is_down=0` when it ends, with `scancode=0`. The Esc key's own down and up events are logged normally too.

The client's HUD and summary exclude paused time. Derivation (M3) must do the same using the marker pairs. Keys pressed while paused are logged but do not move the cursor.

## D6: Lesson start, end, and the release grace period
- The lesson origin (`t_ns = 0`) is when the typing view starts the lesson, not the first key press. Per section 8.1, the first keystroke's flight time is ignored anyway.
- The lesson ends on the key-down that passes the final character, and the summary appears at once. Releases of keys still held at that moment are recorded for up to 1 s more, so the last keystrokes get dwell times. The flush (one transaction) happens after that.
- New key-downs after the end are not part of the lesson and are not stored.

## D7: Session kind is derived from the lesson phase
`lessons` has no column saying whether a lesson belongs to calibration, but `sessions.kind` needs one. The client uses `kind='calibration'` for the calibration phases (`warmup`, `natural`, `coverage`, `pseudo`, `special`, `speed`, `accuracy`, `retest`, from section 7.3) and `'lesson'` otherwise. How sittings map to sessions (`sitting_no`) will be settled in M4. This needs no schema change.

## D8: Aborted lessons keep their raw events
If the client closes mid-lesson, the lesson's events are still flushed, with `status='aborted'`. Raw data stays lossless and the status excludes the lesson from analysis.

A lesson left `running` by a crash has no events, because nothing touches disk during typing. It becomes `aborted` on the next client start.

## D9: Key classification
- `produced_char` is the OS/layout text (`KeyEvent.text`). Shift, AltGr and completed dead-key sequences are resolved by the OS. Enter becomes `'\n'` and Tab becomes `'\t'`.
- Keys with no text are logged with `produced_char = NULL` and do not move the cursor. That covers dead keys before they complete, arrows, F-keys and modifiers.
- Text typed with Ctrl or Super held down counts as a shortcut, not typing.
- Only the first character of a multi-character text event is used. That is very rare for prose.
- IME is not enabled; the window keeps winit's default `ime_allowed = false`.
- OS auto-repeat key-downs count as ordinary presses, which matches what an editor does. A key-down with no key-up in between marks them in the raw data.

## D10: Repaint on demand, not continuously (M0 result)
The M0 spike showed that when the window renders continuously, the UI thread is busy 80% or more of the time. Key events then wait in the queue until the current frame finishes, so receipt stamps arrive late by up to one render time. With on-demand repaint (after input, plus a 10 Hz HUD tick), the thread is busy about 5% of the time.

The typing view (M2) will therefore repaint only on input and on a slow HUD tick. See `docs/timing-validation.md`.

## D11: Extra files not in the section 4 layout
- `client/src/gfx.rs`: the manual winit, egui-winit and egui-wgpu plumbing, shared by the app and `timing_check`.
- `client/src/timing.rs`: analysis for `timing_check`.
- `client/src/bin/roundtrip_fixture.rs`: drives the real client code for the M1 round-trip test.
- `client/scripts/xtest_typist.py`: a scripted X11 typist for headless timing runs.
- `engine/qwertea_engine/paths.py`: repository paths and `config.toml` loading.
- `client/src/app.rs`, `stats.rs`, `layout.rs`, `engine_proc.rs`, `ui/settings.rs`, `ui/stats_view.rs`, `ui/widgets.rs`: app state, lesson results, keyboard geometry, engine supervision, and the extra screens and widgets.
- `packaging/`: desktop entry, icon and Arch PKGBUILDs (D25).
- `engine/qwertea_engine/corpus_build.py`: the corpus builder (formerly `engine/scripts/fetch_corpus.py`, now a thin wrapper).
- `engine/qwertea_engine/pipeline.py`, `corpus.py`, `typist.py`, `model/observations.py`: the engine loop, the corpus, a reference implementation of the client's cursor rules (used by tests and the simulator), and observation extraction.

## D12: Engine dependencies are minimal
Core: `typer`, `numpy`, `scipy`, `matplotlib`. `lightgbm` is an optional extra (`insight`), installed by `make setup`. Without it the engine works and the report says the insight model is unavailable. `pandas` and `shap` are not used: plain Python handles the data sizes involved, and LightGBM computes exact TreeSHAP values itself (`pred_contrib=True`). See D21.

## D13: Migration 002 adds `lessons.meta_json`
Calibration lessons must tell the client their sitting and phase ("Sitting 1 of 3 · Phase 4 of 7"), and probes need instructions ("as fast as you can"). The original schema had nowhere to put this. `schema/002_lesson_meta.sql` adds one nullable JSON column and bumps `schema_version` to 2. Both sides apply numbered migrations in order. Each step runs in an `IMMEDIATE` transaction that re-checks the version, so the client and engine can start at the same time safely.

## D14: Calibration plan size and coverage
- The planner budgets the coverage phase so the plan stays near the 12,000-keystroke ceiling. Plans come out at about 12.2–12.5k keystrokes (warm-ups excluded) in roughly 58 lessons. The required minimum samples already add up to almost all of it.
- With the bundled corpus, every key, digit, symbol, Enter and bigram target is met. About 25 of the top-150 trigrams end 1–6 samples short of 8. `plan-calibration` prints the shortfall, and the Bayesian backoff (trigram → bigram) covers it.
- The spec gives no count for Shift-capitals, so each capital letter needs 10 samples.
- "Numbers" means each digit 20 times plus at least 20 multi-digit numbers.
- Pseudo-words are built by walking the graph of still-needed letter pairs, so each extra character adds a needed sample.
- The phase order rotates across sittings, and the speed/accuracy probe order alternates.
- `calibration.sittings` can be set from 1 to 3. "Skip calibration" (on Home and in Settings) marks the remaining calibration lessons `aborted`, and adaptive lessons start from the data collected so far.

## D15: The client starts the engine
So that users run a single program, the client launches the command in `[engine].command` (default `uv run --project engine qwertea-engine serve`) at startup and stops it on exit. The two programs still never call each other; everything goes through the database.
- `serve` writes `data/engine.pid`. If a live engine already serves the database, the client leaves it alone.
- `[engine].autostart = false` turns this off.
- Engine output goes to `data/engine.log`, which the app shows when the engine is not running.

## D16: Leaving a lesson part-way
- **Pause** (Esc, or automatically when the window loses focus): marker pair only, no time counted.
- **Restart** (from the pause menu): the in-memory attempt is discarded and the lesson starts again. The user explicitly threw that attempt away, and a lesson row can hold only one event sequence.
- **Skip**: saved as `aborted`, with its raw events if anything was typed.
- **Quit to menu, or closing the window**:
  - if anything was typed, the lesson is saved as `aborted` with its events;
  - if nothing was typed, it goes back to `pending`, so an accidental Start costs nothing.
- `probe` lessons run in sessions with `kind='probe'`.

## D17: Bundled corpus
Project Gutenberg cannot be reached from every network, so the repository ships a prebuilt corpus in `engine/data/bundled/`: 38k sentences and a 14k-word frequency table, about 1.5 MB. It is built by `engine/scripts/fetch_corpus.py` from 11 public-domain prose books in NLTK's Gutenberg mirror (no verse, plays or scripture).

`qwertea-engine fetch-corpus` rebuilds it into the git-ignored `engine/data/corpora/prose/`, which takes precedence when present. Word frequencies come from the same books, so no third-party word list or licence is involved.

## D18: Pending adaptive lessons are regenerated after every refit
Each finished lesson triggers a refit. Pending adaptive lessons generated from an older model snapshot are deleted and replaced, so the next lesson always reflects the latest lesson rather than lagging 3 behind.

Only engine-generated, never-started adaptive lessons are replaced. Calibration and probe lessons are kept. If the client starts a lesson the engine is replacing at that moment, `begin_lesson` returns false and the client picks the fresh one.

## D19: Item and statistics details
- Bigrams and trigrams are lower-cased, so "Th" and "th" pool their evidence; the cost of Shift shows up in `key` items.
- An n-gram's error and latency are those of its final keystroke.
- A word's error rate (any wrong letter) is turned into a per-keystroke equivalent, `1 - (1-p)^(1/len)`, before it is compared with the baseline. Otherwise words always look weaker than keys.
- Targets in one lesson must not share a letter pair, so one weakness cannot fill a whole lesson.
- `item_stats` is fully recomputed on each refit, which takes well under a second for tens of thousands of keystrokes. So it is always exactly what `rebuild` would produce.
- Review state (last drilled, Leitner box, next review) is derived from `lessons.targets_json` history, not stored separately.
- The holdout set is a control assignment, not derived data. It is kept in `model_snapshots.notes`, so it survives `rebuild`, and is rotated every 20 adaptive lessons.
- A mini-recalibration "Checkpoint" (the same fixed text every time) is queued after every 10 adaptive lessons.

## D20: `session_pos`
`session_pos` is the fraction of an hour into the session, clipped to [0, 1]: lesson start minus session start, plus the keystroke's active time. It depends only on stored timestamps, so derivation stays deterministic. The fatigue slope is reported as relative slow-down per hour.

## D21: Insight model
LightGBM predicts first-attempt errors (binary) and log latency (regression) from context features:
- finger, hand and row;
- previous finger, same-finger and hand alternation, row jump;
- previous log-flight time and session position;
- whether Shift is involved, position in the word;
- the character and the previous character, as categorical features.

Explanations are exact TreeSHAP values from `pred_contrib=True`. Quality is measured on the last 20 % of lessons: error AUC about 0.75 on the synthetic user. The model is used for the report only; lesson choice uses the Bayesian estimates (section 8.3.4). It needs at least 1,500 analysed keystrokes.

## D22: Optional LLM sentence generator not built
Section 8.5.4 describes it as optional, behind a flag that defaults to off. It is not implemented: Qwertea makes no network calls. The natural-sentence and word-ranking generators cover the same need.

## D23: Accuracy
Everywhere (client summary, history and engine level model), accuracy means the share of text positions whose *first* attempt was correct. Correcting a mistake does not erase it. WPM counts characters that are correct at the end, over the time from the first to the last keystroke, with pauses excluded.

## D24: Extra configuration keys
- `client.live_wpm`: show live WPM while typing.
- `[engine] autostart` and `command`: see D15.

The settings screen writes `config.toml` with `toml_edit`, keeping comments, formatting and the engine sections.

## D25: Installed layout (1.0.0)
Qwertea runs from a source checkout or as an installed package, and the client and engine resolve the config the same way:
1. `$QWERTEA_CONFIG`;
2. a source checkout, i.e. a `config.toml` next to a `schema/` directory. The client walks up from the current directory, then from the executable;
3. `~/.config/qwertea/config.toml` (XDG; `%APPDATA%\qwertea` on Windows, `~/Library/Application Support/qwertea` on macOS). It is created on first run from the default config, with an absolute `db_path` in `~/.local/share/qwertea/` and `engine.command = ["qwertea-engine", "serve"]`.

The engine wheel carries what it needs at run time:
- `schema/*.sql` → `qwertea_engine/schema`;
- the layouts and the bundled corpus → `qwertea_engine/data/`;
- `config.toml` → `qwertea_engine/default_config.toml`.

These are added with hatch `force-include`, so the repository stays the single source of truth. A fetched corpus goes to `~/.local/share/qwertea/corpora/` when installed. The client embeds the schema and the default layout at compile time.

The window's app id / WM_CLASS is `qwertea`, matching `qwertea.desktop`.

## D26: Qwertea's own visual identity (branch `new_ui`)
The 1.0.0 interface was too close to an existing typing site: the colours, the layout of the typing screen and the results page, and two theme names. The redesign keeps the function and gives Qwertea a look of its own, built around its name and its adaptive engine.
- **Palette:** four original tea palettes. Each has a primary accent for actions and progress, and a secondary accent reserved for *training targets*.
- **Type:** Atkinson Hyperlegible and Atkinson Hyperlegible Mono, designed for legibility (SIL OFL, embedded in the binary). egui's fonts remain as fallbacks for arrows.
- **Structure:** a left sidebar (hidden while typing) instead of a top bar.
- **Typing screen:** the text sits on a card with a progress edge and an underline caret with a soft cell highlight. The lesson's targets are tinted where they appear in the upcoming text. This is the one visual element that exists because the lessons are adaptive.
- **Results:** compared against the user's own history, not shown as bare numbers. A per-word rhythm strip and per-target outcomes replace the per-second line chart.

Old theme names in existing configs map to the new palettes.

## D27: One timing definition everywhere (1.0.2)
Testers reported that the live WPM, the timer and the word counter did not agree with the results. The causes were: the clock started when the typing screen opened, not at the first key; the live number used a different formula from the result; the engine capped key flights differently; and stop-on-error lessons were replayed by counting keys, not by the position each key was typed at. Now:
- **Clock:** from the first text keystroke to the last one (or to now while typing). Paused time is excluded. Time spent reading before the first key is not counted.
- **WPM** = characters correct at the end ÷ 5 per minute of that clock. **Raw WPM** counts every text keystroke.
- **The live WPM at the moment you finish equals the result.** Both use `LessonRun`'s clock and the same formula.
- **Replay** uses each event's stored `target_idx`, so stop-on-error and fix-before-next-word lessons are scored by where each key landed.
- **Words** is the number of whitespace-separated words of the text that are fully typed.

`testdata/wpm_golden.json` is a hand-worked lesson ("the cat", with an error, a correction and a 5-second pause) with its expected numbers. Both the Rust client (`stats.rs`) and the Python engine (`derive.lesson_timing`) are tested against it.

## D28: Profiles (1.0.2)
Several people can share one computer, each with their own progress.
- **One database per profile.** The first profile is the existing database, so upgrading loses nothing. New profiles live in `profiles/<id>/qwertea.db` under the data directory. Separate databases keep the model, lessons, holdouts and reports apart, with no `profile_id` threaded through every table and query, and they make export a single file.
- **Registry:** `profiles.toml` in the data directory lists the profiles (id, name, database) and the active one.
- **Settings live in the profile.** Migration 003 adds `profile_settings (key, value)`: name, theme, text size, typing font, caret, smooth caret, live stats, mistakes mode, key sounds, lesson length, capitals / punctuation / numbers, and goals (WPM, accuracy, daily minutes). A 1.0.x database without settings is seeded from the old `[client]` values in `config.toml`. `config.toml` keeps only this computer's settings: present mode, timestamp source, keyboard device, engine autostart, update checks.
- **One engine per profile.** The client starts the engine with `QWERTEA_DB=<profile database>`. The engine's pid file, log and report sit next to that database. Switching profile stops the old engine (SIGINT to its process group, SIGKILL after 3 s) and starts a new one.
- **The engine reads the lesson settings.** Lesson length sets the character range; capitals, punctuation and numbers transform adaptive lessons. Each adaptive lesson records a fingerprint of the settings it was brewed with. When the settings change, pending adaptive lessons with an old fingerprint are replaced within a poll. Calibration lessons are never transformed, so they still sample every key.
- **Settings save themselves** 0.4 s after the last change, and never while a lesson runs. The Save button is gone.
- **Export** is `VACUUM INTO` a `.qwertea` file in ~/Downloads (a consistent copy even while the engine runs). **Import** checks that the file is a Qwertea database, then copies it in as a new profile.
- **Streak and today's minutes** use local calendar days of `finished_at_utc` and the same per-lesson seconds as the results (D27).

## D29: In-app updates (1.0.2)
Non-technical users should not need a terminal to update.
- **Check:** at start-up (`[updates] check = true`, on by default, with a switch in Settings), a background thread asks the GitHub API for the repository's tags with `curl` and compares the newest `vX.Y.Z` with the build's version. Pre-release tags are ignored. Nothing else is sent. `curl` avoids pulling a TLS stack into the client; it ships with Linux, macOS and Windows 10+.
- **Source checkout** (the running binary is inside the repository): **Update now** runs `git pull --ff-only`, `cargo build --release` and `uv sync` in the background. Tools are also looked for in `~/.cargo/bin` and `~/.local/bin`, because desktop launchers often lack them on PATH. The app then offers **Restart now**, which starts the new binary after this one has shut down. A failure (local changes, no network) leaves the running version untouched and says what failed.
- **Arch package:** pacman needs root, so the app does not update itself. It shows the update command (`yay -Syu qwertea`) with a Copy button and links to the release notes.
- **Anything else:** a link to the release page. Self-contained installers that can replace themselves are planned.

## D30: Key sounds (1.0.2)
Optional (off by default) and per profile, in five synthesised styles with a volume control (`sound`, `sound_volume`; the 1.0.2 preview's `true`/`false` map to soft click/off). The key handler only increments an atomic counter (one for correct keys, one for mistakes). The audio callback (cpal) notices the change, reads the style and volume from atomics, and synthesises 35–70 ms of sound. Each style has its own variant for a mistake:
- **soft click:** a decaying tone and a noise tick;
- **typewriter:** a noisy strike, metallic partials and a low body;
- **deep thock:** a low thud whose pitch falls;
- **bubble pop:** a rising sine sweep (falling for a mistake);
- **retro blip:** a short square wave.

The styles are synthesised rather than recorded, so there are no audio files to license or ship. `cargo test --lib sound -- --ignored`, with `QWERTEA_SOUND_DIR` set, writes each style to a WAV file for listening. Nothing on the hot path allocates, locks or calls the audio system, so key timing is unaffected. The output stream is opened when sounds are switched on, never while typing. Builds without the default `sound` feature have no audio dependency.

## D31: Typing fonts and the live demo (1.0.2)
- **Fonts:** nine typing fonts, all free to redistribute and embedded in the binary, so they work offline and look the same everywhere:
  - Atkinson Hyperlegible Mono (default) and Atkinson Hyperlegible;
  - JetBrains Mono, Fira Code, Cascadia Mono, IBM Plex Mono (SIL OFL);
  - Ubuntu Mono and Ubuntu (Ubuntu Font Licence);
  - Hack (shipped with egui).

  Each font gets its own egui family, with Atkinson and egui's fonts as fallbacks for missing glyphs. The files are the official releases as packaged by Debian/Ubuntu. Their copyright files are in `client/assets/fonts/licenses/` and installed with the Arch package. Ubuntu and Ubuntu Mono are variable fonts, rendered at their default (regular) instance. Embedding adds about 2.3 MB to the binary.
- **Font cards** preview each font in its own typeface.
- **Live demo:** a two-line sheet under the font cards. Click it and type: it uses the open profile's font, size, caret, smooth caret, mistakes mode and key sound, and shows WPM and accuracy at the end. It is drawn by the same function as the real typing screen (`typing_view::draw_text`), so what you see is what lessons look like. Demo keys come from egui's input events and are not stored or analysed. Esc leaves the demo.
