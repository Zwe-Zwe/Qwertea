# Timing validation (M0)

Timing accuracy is the product (spec section 5.1). This file records what the timing spike measured and what is still open.

## The spike
`cargo run --release --bin timing_check` opens a small window. It runs a hand-written winit event loop that drives egui-winit and egui-wgpu directly, with no `eframe`. Every `KeyboardInput` is stamped with `Instant::now()` on the first line of `window_event`, before egui sees it. The program also records the start and end of every rendered frame.

Press Esc to finish. The report is printed as Markdown and written to `data/timing_check.json`.

```
timing_check [--present mailbox|immediate|fifo] [--render on-demand|continuous]
             [--evdev [/dev/input/eventN]] [--max-events N] [--out PATH]
```

What the report shows:

| Metric | What it tells you |
|---|---|
| share of stamps within 50 µs after a frame start | Stamping inside egui gives every event its frame's start time, so this would be about 1.0. Receipt stamps should be about 0. |
| share of duplicate consecutive stamps | Frame-quantized stamps collide whenever two keys land in the same frame. |
| error egui-time stamping would add | Next frame start minus receipt: the error that stamping inside egui would add. |
| render time per frame, busy % | Time the UI thread spends rendering. Key events wait in the queue during it, so it bounds the delay rendering adds to the receipt stamp. |
| winit − kernel (evdev builds only) | The real end-to-end delay between the kernel seeing the key and the UI thread receiving it. **This is the number M0 asks for.** |

## Results so far: headless Xvfb (synthetic keys)
Environment: cloud container, Xvfb with no window manager and no vsync, software Vulkan (llvmpipe, LLVM 20.1.2). Keys come from `client/scripts/xtest_typist.py`: 264 key-downs with 30–90 ms holds and 30–150 ms gaps.

| Render | Present | Frames | Render ms (median / p99) | UI busy | At frame start | Duplicate stamps | Verdict |
|---|---|---|---|---|---|---|---|
| on-demand | Mailbox | 953 | 2.06 / 4.94 | 4.9 % | 0.000 | 0.000 | not quantized |
| on-demand | Fifo | 953 | 2.03 / 4.61 | 4.9 % | 0.000 | 0.000 | not quantized |
| continuous | Mailbox | 18 753 | 1.76 / 4.04 | 81.9 % | 0.000 | 0.000 | not quantized |
| continuous | Fifo | 19 110 | 1.71 / 3.27 | 80.5 % | 0.000 | 0.000 | not quantized |

`Instant::now()`: about 31 ns resolution and about 40 ns per read.

**Findings**
1. **The stamps are not frame-quantized.** No stamp coincides with a frame start and no two stamps are identical. Stamping in `window_event` before egui works as designed.
2. **Continuous rendering delays receipt.** In continuous mode, stamp phases bunch up at the end of the frame interval (histogram `[0,0,0,0,1,4,6,83,55,116]`). Events that arrive during a render wait until the render returns.
   On a real display with Fifo, a blocking present can hold the loop for up to a whole vsync interval (16.7 ms at 60 Hz). Decision **D10**: the typing view repaints only on input and on a 10 Hz HUD tick. That cut UI busy time from about 81 % to about 5 %.
3. Xvfb has no vsync, so Fifo behaves like Mailbox here. The Fifo rows say nothing about blocking presents on real hardware.

## First real-machine run (winit only)
The first run on the author's Linux machine had `--evdev` requested but no `input` group membership, so only window stamps were recorded. The verdict was again "not frame-quantized".

It also reported `Instant::now()` at about 1.2 µs resolution and 1.4 µs per read, roughly 40× slower than the cloud VM. That usually means the kernel clock source is not the TSC:

```
cat /sys/devices/system/clocksource/clocksource0/current_clocksource
```

If it says `hpet` or `acpi_pm`, the kernel distrusted the TSC; virtual machines often do this. Microsecond resolution is still far finer than keystroke timing needs, so it is not a problem for Qwertea, just worth knowing.

## Still open: run on the real machine
These steps need a physical keyboard and display, so they can't be done from the cloud container:

1. Add yourself to the `input` group (see README), then log out and back in.
2. Build and run with kernel timestamps:
   ```
   cd client
   cargo run --release --features evdev-capture --bin timing_check -- --evdev
   ```
   Type naturally for a minute (200+ keys), then press Esc. Repeat with `--present fifo` and with `--render continuous`.
3. Paste each printed report below. The M0 acceptance numbers are the median and p99 of `winit − kernel`.

| Machine / compositor | Present | Render | winit − kernel median (ms) | p99 (ms) | Unmatched | Notes |
|---|---|---|---|---|---|---|
| _todo_ | Mailbox | on-demand | | | | |
| _todo_ | Fifo | on-demand | | | | |
| _todo_ | Mailbox | continuous | | | | |

**Acceptance:** M0 passes when the on-demand median of `winit − kernel` is about 1 ms or less and the p99 is a few ms, with no frame quantization. Then the default `timestamp_source = "winit"` is good enough, and `evdev` remains an optional check. If the p99 is large (for example under a busy compositor), switch `timestamp_source` to `evdev` in `config.toml` on that machine.
