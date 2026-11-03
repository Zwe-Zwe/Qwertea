# Decisions and deviations from the spec

Each entry records a choice the spec left open, or a deliberate deviation, and why.

## D1: Crate versions (M0)
`winit 0.30`, `egui`/`egui-winit`/`egui-wgpu 0.37` (which pins `wgpu 30`), `rusqlite 0.41` (bundled), `evdev 0.13`.
egui 0.37 is the newest egui and is built against winit 0.30, so the versions match. `eframe` is not a dependency.

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

## D12: Engine dependencies are added per milestone
`pyproject.toml` only lists what the implemented code imports (`typer` for now). `numpy`, `scipy`, `lightgbm` and the rest arrive with the milestones that use them, to keep dependencies minimal (section 14.7).
