//! Qwertea: the typing tutor app.
//!
//! The winit event loop is driven by hand (no `eframe`). Every key event is
//! stamped on the first line of `window_event`, before egui or anything else
//! sees it; while a lesson is running, keys go straight to the lesson state
//! machine and never through egui. The UI repaints on demand (after input,
//! plus a slow tick for the clock), so the UI thread is idle when keys arrive.

use qwertea_client::app::App;
use qwertea_client::config::Config;
use qwertea_client::gfx::Gfx;
use qwertea_client::ui;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::{Icon, Window, WindowId};

struct Runner {
    cfg: Option<Config>,
    app: Option<App>,
    gfx: Option<Gfx>,
    mods: ModifiersState,
    next_tick: Instant,
    next_repaint: Option<Instant>,
}

/// A 64×64 icon drawn in code: a jade tile with an ink "q" (a cup and its
/// handle) and a wisp of steam; the same mark as packaging/qwertea.svg.
fn icon() -> Option<Icon> {
    const N: u32 = 64;
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let r = 14.0;
            let cx = fx.clamp(r, N as f32 - r);
            let cy = fy.clamp(r, N as f32 - r);
            let inside = (fx - cx).hypot(fy - cy) <= r;
            let d = (fx - 29.0).hypot(fy - 34.0);
            let ring = (8.7..=14.3).contains(&d);
            let stem = (38.0..=43.5).contains(&fx) && (23.0..=54.0).contains(&fy);
            // Steam: two small sine wisps above the cup.
            let t = (19.0 - fy) / 12.0;
            let wisp = |x0: f32| (fx - (x0 + 1.6 * (t * std::f32::consts::TAU).sin())).abs() <= 1.2;
            let steam = (0.0..=1.0).contains(&t) && (wisp(24.5) || wisp(32.0));
            let px = if !inside {
                [0, 0, 0, 0]
            } else if ring || stem || steam {
                [0x14, 0x1c, 0x18, 255]
            } else {
                [0x8e, 0xd1, 0xa5, 255]
            };
            rgba.extend_from_slice(&px);
        }
    }
    Icon::from_rgba(rgba, N, N).ok()
}

impl ApplicationHandler for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let cfg = self.cfg.take().unwrap_or_else(Config::load);
        let attrs = Window::default_attributes()
            .with_title("Qwertea")
            .with_inner_size(winit::dpi::LogicalSize::new(1120.0, 760.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(760.0, 520.0))
            .with_window_icon(icon());
        // App id / WM_CLASS, so desktops match the window to qwertea.desktop.
        #[cfg(all(unix, not(target_os = "macos")))]
        let attrs = {
            use winit::platform::wayland::WindowAttributesExtWayland;
            use winit::platform::x11::WindowAttributesExtX11;
            let a = WindowAttributesExtX11::with_name(attrs, "qwertea", "qwertea");
            WindowAttributesExtWayland::with_name(a, "qwertea", "qwertea")
        };
        match Gfx::new(event_loop, attrs, cfg.client.present_mode) {
            Ok(g) => {
                g.window.request_redraw();
                self.gfx = Some(g);
            }
            Err(e) => {
                eprintln!("qwertea: cannot create the window/GPU surface: {e}");
                event_loop.exit();
                return;
            }
        }
        self.app = Some(App::new(cfg));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // FIRST LINE: stamp before anything else touches the event.
        let received_at = Instant::now();

        let (Some(app), Some(gfx)) = (self.app.as_mut(), self.gfx.as_mut()) else {
            return;
        };
        if let WindowEvent::KeyboardInput { event: ke, .. } = &event {
            if app.typing_captures_keys() {
                app.on_key(received_at, ke, self.mods);
                gfx.window.request_redraw();
                return;
            }
        }
        match &event {
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::Focused(f) => {
                app.on_focus(*f, received_at);
                gfx.window.request_redraw();
            }
            WindowEvent::CloseRequested => {
                app.shutdown();
                event_loop.exit();
                return;
            }
            WindowEvent::RedrawRequested => {
                let delay = gfx.frame(|ui| ui::show(app, ui));
                self.next_repaint =
                    (delay < Duration::from_secs(3600)).then(|| Instant::now() + delay);
                if app.quit_requested {
                    app.shutdown();
                    event_loop.exit();
                }
                return;
            }
            _ => {}
        }
        let resp = gfx.on_window_event(&event);
        if resp.repaint {
            gfx.window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(app), Some(gfx)) = (self.app.as_mut(), self.gfx.as_ref()) else {
            return;
        };
        let now = Instant::now();
        if now >= self.next_tick {
            let wake = app.tick();
            gfx.window.request_redraw();
            self.next_tick = now + wake.unwrap_or(Duration::from_secs(2));
        }
        if let Some(r) = self.next_repaint {
            if r <= now {
                self.next_repaint = None;
                gfx.window.request_redraw();
            }
        }
        let deadline = self
            .next_repaint
            .map_or(self.next_tick, |r| r.min(self.next_tick));
        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
    }
}

const HELP: &str = "\
Qwertea - an adaptive typing tutor

USAGE: qwertea [--version] [--help]

Settings live in config.toml: $QWERTEA_CONFIG, a source checkout's config.toml,
or ~/.config/qwertea/config.toml (created on first run). Your typing data is
kept in ~/.local/share/qwertea/ and never leaves this computer.";

fn main() {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "-V" | "--version" => {
                println!("qwertea {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "-h" | "--help" => {
                println!("{HELP}");
                return;
            }
            other => {
                eprintln!("qwertea: unknown argument {other}\n\n{HELP}");
                std::process::exit(2);
            }
        }
    }
    let cfg = Config::load();
    let event_loop = match EventLoop::new() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("qwertea: cannot start the event loop: {e}");
            std::process::exit(1);
        }
    };
    let mut runner = Runner {
        cfg: Some(cfg),
        app: None,
        gfx: None,
        mods: ModifiersState::default(),
        next_tick: Instant::now(),
        next_repaint: None,
    };
    if let Err(e) = event_loop.run_app(&mut runner) {
        eprintln!("qwertea: event loop error: {e}");
    }
    if let Some(app) = runner.app.as_mut() {
        app.shutdown();
        // Restart after an in-app update: start the new build.
        if let Some(exe) = app.restart_with.take() {
            if let Err(e) = std::process::Command::new(&exe).spawn() {
                eprintln!("qwertea: cannot restart {}: {e}", exe.display());
            }
        }
    }
}
