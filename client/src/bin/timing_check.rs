//! M0 timing spike: a minimal window driven by a hand-written winit event
//! loop with egui rendered every frame. Key events are stamped on the first
//! line of `window_event`. Press Esc (or close the window) to finish; the
//! report is printed as Markdown and written to `data/timing_check.json`.
//!
//! Usage:
//!   cargo run --release --bin timing_check -- [--present mailbox|immediate|fifo]
//!                                              [--evdev [/dev/input/eventN]]
//!                                              [--max-events N] [--out PATH]

use qwertea_client::capture::{RawKeyInfo, TimestampSource, WinitReceiptStamp};
use qwertea_client::config::{Config, PresentModePref};
use qwertea_client::gfx::Gfx;
use qwertea_client::timing::{self, Frame};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::scancode::PhysicalKeyExtScancode;
use winit::window::{Window, WindowId};

struct Args {
    present: PresentModePref,
    evdev: Option<String>,
    max_events: usize,
    out: PathBuf,
    continuous: bool,
}

fn parse_args(cfg: &Config) -> Args {
    let mut a = Args {
        present: cfg.client.present_mode,
        evdev: None,
        max_events: usize::MAX,
        out: cfg.db_path().with_file_name("timing_check.json"),
        continuous: false,
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--present" => {
                i += 1;
                a.present = match argv.get(i).map(String::as_str) {
                    Some("immediate") => PresentModePref::Immediate,
                    Some("fifo") => PresentModePref::Fifo,
                    _ => PresentModePref::Mailbox,
                };
            }
            "--evdev" => {
                let path = argv.get(i + 1).filter(|p| !p.starts_with("--")).cloned();
                if path.is_some() {
                    i += 1;
                }
                a.evdev = Some(path.unwrap_or_else(|| cfg.client.keyboard_device.clone()));
            }
            "--max-events" => {
                i += 1;
                a.max_events = argv
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(usize::MAX);
            }
            "--render" => {
                i += 1;
                a.continuous = argv.get(i).map(String::as_str) == Some("continuous");
            }
            "--out" => {
                i += 1;
                if let Some(p) = argv.get(i) {
                    a.out = PathBuf::from(p);
                }
            }
            other => eprintln!("timing_check: ignoring unknown argument {other}"),
        }
        i += 1;
    }
    a
}

#[derive(Clone, Copy)]
struct Rec {
    info: RawKeyInfo,
}

struct App {
    args: Args,
    gfx: Option<Gfx>,
    origin: Instant,
    #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
    origin_mono_ns: u64,
    keys: Vec<Rec>,
    frames: Vec<Frame>,
    downs: usize,
    next_tick: Instant,
    status: String,
    #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
    kernel: Option<qwertea_client::capture::evdev_reader::KernelLog>,
    done: bool,
}

impl App {
    fn finish(&mut self, event_loop: &ActiveEventLoop) {
        if self.done {
            return;
        }
        self.done = true;
        let stamper = WinitReceiptStamp::new(self.origin);
        // Key-downs only: that is what latency statistics are built on.
        let stamps: Vec<u64> = self
            .keys
            .iter()
            .filter(|r| r.info.is_down)
            .filter_map(|r| stamper.stamp(&r.info))
            .collect();
        #[allow(unused_mut)]
        let mut report = timing::analyze(&stamps, &self.frames);

        #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
        if let Some(log) = &self.kernel {
            use qwertea_client::capture::EvdevStamp;
            let ev = EvdevStamp::new(self.origin, self.origin_mono_ns, log.take());
            let mut diffs = Vec::new();
            let mut unmatched = 0;
            for r in &self.keys {
                let w = stamper.stamp(&r.info).unwrap_or(0) as f64;
                match ev.stamp(&r.info) {
                    Some(k) => diffs.push((w - k as f64) / 1e6),
                    None => unmatched += 1,
                }
            }
            report.winit_minus_kernel_ms = timing::dist(&diffs);
            report.unmatched_kernel = unmatched;
        }

        let present = self
            .gfx
            .as_ref()
            .map(|g| format!("{:?} on {}", g.present_mode, g.adapter_info))
            .unwrap_or_default();
        println!(
            "## timing_check run\n\n- present mode: {present}\n- render: {}\n{}",
            if self.args.continuous {
                "continuous"
            } else {
                "on-demand (input + 10 Hz tick)"
            },
            report.to_markdown()
        );
        if let Some(dir) = self.args.out.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let json = serde_json_like(&report, &present);
        match std::fs::write(&self.args.out, json) {
            Ok(()) => println!("\nwrote {}", self.args.out.display()),
            Err(e) => eprintln!("cannot write {}: {e}", self.args.out.display()),
        }
        event_loop.exit();
    }
}

/// Tiny JSON writer (avoids a serde_json dependency for one file).
fn serde_json_like(r: &timing::TimingReport, present: &str) -> String {
    let d = |d: &Option<timing::Dist>| match d {
        Some(d) => format!(
            "{{\"n\":{},\"min\":{},\"median\":{},\"p90\":{},\"p99\":{},\"max\":{}}}",
            d.n, d.min, d.median, d.p90, d.p99, d.max
        ),
        None => "null".into(),
    };
    format!(
        "{{\"present\":{:?},\"key_events\":{},\"frames\":{},\"frame_interval_ms\":{},\"render_ms\":{},\"render_busy_fraction\":{},\"phase\":{},\
         \"share_at_frame_start\":{},\"share_duplicate_stamps\":{},\"egui_quantization_error_ms\":{},\
         \"phase_histogram\":{:?},\"verdict_not_quantized\":{},\"winit_minus_kernel_ms\":{},\
         \"unmatched_kernel\":{},\"clock_resolution_ns\":{},\"clock_read_cost_ns\":{}}}\n",
        present,
        r.key_events,
        r.frames,
        d(&r.frame_interval_ms),
        d(&r.render_ms),
        r.render_busy_fraction,
        d(&r.phase),
        r.share_at_frame_start,
        r.share_duplicate_stamps,
        d(&r.egui_quantization_error_ms),
        r.phase_histogram,
        r.verdict_not_quantized,
        d(&r.winit_minus_kernel_ms),
        r.unmatched_kernel,
        r.clock_resolution_ns,
        r.clock_read_cost_ns
    )
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Qwertea timing check")
            .with_inner_size(winit::dpi::LogicalSize::new(720.0, 360.0));
        match Gfx::new(event_loop, attrs, self.args.present) {
            Ok(g) => {
                g.window.request_redraw();
                self.gfx = Some(g);
            }
            Err(e) => {
                eprintln!("timing_check: cannot create window/GPU surface: {e}");
                event_loop.exit();
                return;
            }
        }
        #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
        if let Some(path) = &self.args.evdev {
            match qwertea_client::capture::evdev_reader::spawn(path) {
                Ok((log, name)) => {
                    self.status = format!("evdev: recording kernel timestamps from {name}");
                    self.kernel = Some(log);
                }
                Err(e) => self.status = format!("evdev unavailable ({e}); winit stamps only"),
            }
        }
        #[cfg(not(all(target_os = "linux", feature = "evdev-capture")))]
        if self.args.evdev.is_some() {
            self.status = "evdev requested but built without the `evdev-capture` feature".into();
        }
        // Origin for both clocks, read back to back.
        self.origin = Instant::now();
        #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
        {
            self.origin_mono_ns = qwertea_client::capture::monotonic_now_ns();
            if let Some(k) = &self.kernel {
                k.clear();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // FIRST LINE: stamp before anything else touches the event.
        let received_at = Instant::now();

        if let WindowEvent::KeyboardInput { event: ke, .. } = &event {
            if self.keys.len() < self.keys.capacity() {
                self.keys.push(Rec {
                    info: RawKeyInfo {
                        received_at,
                        scancode: ke.physical_key.to_scancode().unwrap_or(0),
                        is_down: ke.state == ElementState::Pressed,
                    },
                });
            }
            if ke.state == ElementState::Pressed {
                self.downs += 1;
            }
            if (ke.state == ElementState::Pressed && ke.logical_key == Key::Named(NamedKey::Escape))
                || self.downs >= self.args.max_events
            {
                self.finish(event_loop);
                return;
            }
        }
        match &event {
            WindowEvent::CloseRequested => {
                self.finish(event_loop);
                return;
            }
            WindowEvent::RedrawRequested => {
                let start = received_at.duration_since(self.origin).as_nanos() as u64;
                let n = self.downs;
                let continuous = self.args.continuous;
                let status = self.status.clone();
                if let Some(g) = self.gfx.as_mut() {
                    g.frame(|ui| {
                        egui::CentralPanel::default().show(ui, |ui| {
                            ui.heading("Qwertea timing check");
                            ui.label("Type naturally for a minute (aim for 200+ keys). Press Esc to finish.");
                            ui.add_space(12.0);
                            ui.monospace(format!("key presses recorded: {n}"));
                            if !status.is_empty() {
                                ui.label(&status);
                            }
                        });
                    });
                    if continuous {
                        g.window.request_redraw();
                    }
                }
                let end = self.origin.elapsed().as_nanos() as u64;
                if self.frames.len() < self.frames.capacity() {
                    self.frames.push(Frame { start, end });
                }
                return;
            }
            _ => {}
        }
        if let Some(g) = self.gfx.as_mut() {
            let _ = g.on_window_event(&event);
            // On-demand mode (like the typing view): repaint after input.
            if matches!(event, WindowEvent::KeyboardInput { .. }) {
                g.window.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.args.continuous {
            event_loop.set_control_flow(ControlFlow::Poll);
        } else if let Some(g) = &self.gfx {
            // On-demand mode: a 10 Hz HUD tick, otherwise idle.
            let now = Instant::now();
            if now >= self.next_tick {
                g.window.request_redraw();
                self.next_tick = now + Duration::from_millis(100);
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_tick));
        }
    }
}

fn main() {
    let cfg = Config::load();
    let args = parse_args(&cfg);
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        args,
        gfx: None,
        origin: Instant::now(),
        #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
        origin_mono_ns: 0,
        keys: Vec::with_capacity(32_768),
        frames: Vec::with_capacity(1 << 20),
        downs: 0,
        next_tick: Instant::now(),
        status: String::new(),
        #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
        kernel: None,
        done: false,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("timing_check: event loop error: {e}");
    }
}
