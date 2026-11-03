//! Application state and screen flow.
//!
//! Key handling is split in two:
//! * `on_key` runs inside `window_event` right after the timestamp was taken.
//!   It only feeds `LessonRun` (no I/O, no allocation, no analysis).
//! * `tick` runs from `about_to_wait`, between events. It notices a finished
//!   lesson, computes results, flushes events to the database, polls the
//!   engine and refreshes the queue.

use crate::config::{Config, TimestampSourceKind};
use crate::db::{self, CalibrationProgress, LessonOutcome, LevelInfo, PendingLesson};
use crate::engine_proc::EngineProc;
use crate::layout::Layout;
use crate::session::{KeyKind, LessonRun, RunState};
use crate::stats::{self, LessonStats};
use crate::ui::theme::{self, Palette};
use rusqlite::Connection;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Home,
    /// Phase introduction (calibration) shown before a lesson.
    Intro,
    Typing,
    Summary,
    /// A calibration sitting just ended.
    SittingDone,
    Stats,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PausedAction {
    Restart,
    Skip,
    Quit,
}

pub struct Active {
    pub lesson: PendingLesson,
    pub run: LessonRun,
    pub origin: Instant,
    #[allow(dead_code)]
    pub origin_mono_ns: u64,
    pub session_id: i64,
    /// Results shown as soon as the last character is typed.
    pub finished_shown: bool,
}

pub struct Finished {
    pub lesson: PendingLesson,
    pub stats: LessonStats,
    pub saved: bool,
}

/// Lesson history for the stats screen and home overview.
#[derive(Default)]
pub struct History {
    /// (lesson index, wpm, accuracy, seconds, phase)
    pub lessons: Vec<(usize, f64, f64, f64, String)>,
    pub loaded_at: Option<Instant>,
}

impl History {
    pub fn recent(&self, n: usize) -> &[(usize, f64, f64, f64, String)] {
        let k = self.lessons.len().saturating_sub(n);
        &self.lessons[k..]
    }
    pub fn best_wpm(&self) -> Option<f64> {
        self.lessons.iter().map(|l| l.1).reduce(f64::max)
    }
    pub fn total_seconds(&self) -> f64 {
        self.lessons.iter().map(|l| l.3).sum()
    }
}

pub struct App {
    pub cfg: Config,
    pub conn: Option<Connection>,
    pub db_error: Option<String>,
    pub palette: Palette,
    pub layout: Layout,
    pub screen: Screen,
    pub prev_screen: Screen,
    pub active: Option<Active>,
    pub intro: Option<PendingLesson>,
    pub last: Option<Finished>,
    pub next: Option<PendingLesson>,
    next_checked: Instant,
    pub calib: Option<CalibrationProgress>,
    pub level: Option<LevelInfo>,
    pub history: History,
    pub key_weakness: std::collections::HashMap<String, f64>,
    pub weak_bigrams: Vec<db::ItemStat>,
    pub weak_keys: Vec<db::ItemStat>,
    pub weak_words: Vec<db::ItemStat>,
    /// (session id, kind, sitting)
    session: Option<(i64, &'static str, Option<i64>)>,
    /// Phase and sitting of the last calibration lesson started this run.
    last_calib: Option<(String, Option<i64>)>,
    pub engine: EngineProc,
    pub toast: Option<(String, Instant)>,
    pub banner: Option<String>,
    pub mods: winit::keyboard::ModifiersState,
    pub paused_action: Option<PausedAction>,
    pub confirm_skip_calibration: bool,
    pub settings_dirty: bool,
    pub theme_applied: Option<String>,
    pub quit_requested: bool,
    #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
    kernel: Option<crate::capture::evdev_reader::KernelLog>,
}

impl App {
    pub fn new(cfg: Config) -> Self {
        let (conn, db_error) = match db::open(&cfg.db_path()) {
            Ok(c) => {
                let _ = db::recover_running(&c);
                (Some(c), None)
            }
            Err(e) => (
                None,
                Some(format!("cannot open {}: {e}", cfg.db_path().display())),
            ),
        };
        let engine = EngineProc::start(&cfg);
        let palette = theme::palette(&cfg.client.theme);
        let layout = Layout::load(&cfg.layout_file());
        #[allow(unused_mut)]
        let mut app = Self {
            conn,
            db_error,
            palette,
            layout,
            screen: Screen::Home,
            prev_screen: Screen::Home,
            active: None,
            intro: None,
            last: None,
            next: None,
            next_checked: Instant::now() - Duration::from_secs(10),
            calib: None,
            level: None,
            history: History::default(),
            key_weakness: Default::default(),
            weak_bigrams: vec![],
            weak_keys: vec![],
            weak_words: vec![],
            session: None,
            last_calib: None,
            engine,
            toast: None,
            banner: None,
            mods: Default::default(),
            paused_action: None,
            confirm_skip_calibration: false,
            settings_dirty: false,
            theme_applied: None,
            quit_requested: false,
            #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
            kernel: None,
            cfg,
        };
        app.start_evdev();
        app.refresh_queue();
        app.refresh_history();
        app
    }

    fn start_evdev(&mut self) {
        if self.cfg.client.timestamp_source != TimestampSourceKind::Evdev {
            return;
        }
        #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
        match crate::capture::evdev_reader::spawn(&self.cfg.client.keyboard_device) {
            Ok((log, _)) => self.kernel = Some(log),
            Err(e) => {
                self.banner = Some(format!(
                    "Kernel timestamps unavailable ({e}). Using window timestamps. \
                     Add yourself to the `input` group to enable them."
                ))
            }
        }
        #[cfg(not(all(target_os = "linux", feature = "evdev-capture")))]
        {
            self.banner = Some(
                "timestamp_source = evdev needs a Linux build with `--features evdev-capture`; \
                 using window timestamps."
                    .into(),
            );
        }
    }

    fn timestamp_source(&self) -> &'static str {
        #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
        if self.kernel.is_some() {
            return "evdev";
        }
        "winit"
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    // ------------------------------------------------------------------ data refresh

    pub fn refresh_queue(&mut self) {
        self.next_checked = Instant::now();
        let Some(conn) = &self.conn else { return };
        self.next = db::next_pending(conn).ok().flatten();
        self.calib = db::calibration_progress(conn).ok().flatten();
        self.level = db::latest_level(conn, &self.cfg.general.mode)
            .ok()
            .flatten();
    }

    pub fn refresh_history(&mut self) {
        let Some(conn) = &self.conn else { return };
        let mode = self.cfg.general.mode.clone();
        let mut out = Vec::new();
        if let Ok(lessons) = db::done_lessons(conn, 500) {
            for l in lessons.iter().filter(|l| !l.is_discarded) {
                let Ok(ev) = db::load_events(conn, l.id) else {
                    continue;
                };
                let s = stats::compute(&l.text, &ev);
                if s.keystrokes >= 10 && s.seconds > 0.0 {
                    out.push((out.len() + 1, s.wpm, s.accuracy, s.seconds, l.phase.clone()));
                }
            }
        }
        self.history = History {
            lessons: out,
            loaded_at: Some(Instant::now()),
        };
        self.key_weakness = db::key_weakness(conn, &mode).unwrap_or_default();
        self.weak_bigrams = db::item_stats(conn, &mode, "bigram", 5, 10).unwrap_or_default();
        self.weak_keys = db::item_stats(conn, &mode, "key", 10, 10).unwrap_or_default();
        self.weak_words = db::item_stats(conn, &mode, "word", 3, 10).unwrap_or_default();
    }

    // ------------------------------------------------------------------ lesson flow

    /// Go to the next lesson: an intro screen for a new calibration phase or
    /// sitting, otherwise straight into typing.
    pub fn start_next(&mut self) {
        self.refresh_queue();
        let Some(lesson) = self.next.clone() else {
            self.screen = Screen::Home;
            return;
        };
        let key = (lesson.phase.clone(), lesson.meta.sitting);
        let needs_intro = lesson.is_calibration() && self.last_calib.as_ref() != Some(&key)
            || lesson.phase == "probe";
        if needs_intro {
            if lesson.is_calibration() {
                if let Some((_, Some(prev_sitting))) = &self.last_calib {
                    if lesson.meta.sitting.unwrap_or(1) > *prev_sitting
                        && self.screen != Screen::SittingDone
                        && self.screen != Screen::Home
                    {
                        self.screen = Screen::SittingDone;
                        return;
                    }
                }
            }
            self.intro = Some(lesson);
            self.screen = Screen::Intro;
        } else {
            self.begin(lesson);
        }
    }

    /// Start typing `lesson` now.
    pub fn begin(&mut self, lesson: PendingLesson) {
        let ts = self.timestamp_source();
        let Some(conn) = &self.conn else { return };
        let kind = db::session_kind_for_phase(&lesson.phase);
        let sitting = if lesson.is_calibration() {
            lesson.meta.sitting
        } else {
            None
        };
        let session_id = match self.session {
            Some((id, k, s)) if k == kind && s == sitting => id,
            _ => {
                let device = (!self.cfg.client.keyboard_device.is_empty())
                    .then_some(self.cfg.client.keyboard_device.as_str());
                match db::start_session(
                    conn,
                    &db::NewSession {
                        kind,
                        mode: &lesson.mode,
                        sitting_no: sitting,
                        layout: &self.cfg.general.layout,
                        keyboard: device,
                        timestamp_source: ts,
                    },
                ) {
                    Ok(id) => {
                        self.session = Some((id, kind, sitting));
                        id
                    }
                    Err(e) => {
                        self.toast(format!("database error: {e}"));
                        return;
                    }
                }
            }
        };
        match db::begin_lesson(conn, lesson.id, session_id) {
            Ok(true) => {}
            Ok(false) => {
                // The engine replaced this lesson with a fresher one; take that.
                self.refresh_queue();
                if let Some(n) = self.next.clone() {
                    if n.id != lesson.id {
                        self.begin(n);
                        return;
                    }
                }
                self.screen = Screen::Home;
                return;
            }
            Err(e) => {
                self.toast(format!("database error: {e}"));
                return;
            }
        }
        if lesson.is_calibration() {
            self.last_calib = Some((lesson.phase.clone(), lesson.meta.sitting));
        }
        let run = LessonRun::new(&lesson.text, self.cfg.client.stop_on_error);
        #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
        if let Some(k) = &self.kernel {
            k.clear();
        }
        let origin = Instant::now();
        #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
        let origin_mono_ns = crate::capture::monotonic_now_ns();
        #[cfg(not(all(target_os = "linux", feature = "evdev-capture")))]
        let origin_mono_ns = 0;
        self.active = Some(Active {
            lesson,
            run,
            origin,
            origin_mono_ns,
            session_id,
            finished_shown: false,
        });
        self.paused_action = None;
        self.screen = Screen::Typing;
    }

    pub fn typing_captures_keys(&self) -> bool {
        self.screen == Screen::Typing && self.active.is_some()
    }

    /// HOT PATH: called right after the timestamp in `window_event`.
    #[inline]
    pub fn on_key(
        &mut self,
        received_at: Instant,
        event: &winit::event::KeyEvent,
        mods: winit::keyboard::ModifiersState,
    ) {
        let Some(a) = self.active.as_mut() else {
            return;
        };
        let input = crate::capture::classify(event, mods);
        let t = received_at.saturating_duration_since(a.origin).as_nanos() as u64;
        let was_paused = a.run.state() == RunState::Paused;
        a.run.on_key(t, input);
        if was_paused && input.is_down {
            if let KeyKind::Text(c) = input.kind {
                self.paused_action = match c.to_ascii_lowercase() {
                    'r' => Some(PausedAction::Restart),
                    's' => Some(PausedAction::Skip),
                    'q' => Some(PausedAction::Quit),
                    _ => self.paused_action,
                };
            }
        }
    }

    /// Window focus changed: pause a running lesson when focus is lost.
    pub fn on_focus(&mut self, focused: bool, now: Instant) {
        if focused {
            return;
        }
        if let Some(a) = self.active.as_mut() {
            let t = now.saturating_duration_since(a.origin).as_nanos() as u64;
            a.run.pause(t);
        }
    }

    pub fn resume(&mut self) {
        if let Some(a) = self.active.as_mut() {
            let t = a.origin.elapsed().as_nanos() as u64;
            a.run.resume(t);
        }
    }

    /// Between events: lesson end, flush, paused-menu actions, engine and queue.
    /// Returns how soon it wants to be called again.
    pub fn tick(&mut self) -> Option<Duration> {
        if let Some(action) = self.paused_action.take() {
            self.do_paused_action(action);
        }
        let mut wake = None;
        if let Some(a) = self.active.as_mut() {
            let now_ns = a.origin.elapsed().as_nanos() as u64;
            if a.run.state() == RunState::Finished && !a.finished_shown {
                a.finished_shown = true;
                let stats = stats::compute(&a.lesson.text, a.run.events());
                self.last = Some(Finished {
                    lesson: a.lesson.clone(),
                    stats,
                    saved: false,
                });
                self.screen = Screen::Summary;
            }
            if a.run.ready_to_flush(now_ns) {
                self.flush_active(LessonOutcome::Done);
            } else if a.run.state() == RunState::Finished {
                wake = Some(Duration::from_millis(50));
            } else {
                wake = Some(Duration::from_millis(100)); // HUD clock
            }
        }
        self.engine.poll(&self.cfg);
        let waiting_for_lesson = self.active.is_none()
            && matches!(
                self.screen,
                Screen::Home | Screen::Summary | Screen::SittingDone
            );
        if waiting_for_lesson && self.next_checked.elapsed() > Duration::from_millis(1500) {
            let had = self.next.as_ref().map(|l| l.id);
            self.refresh_queue();
            if had.is_none() && self.next.is_some() {
                self.refresh_history();
            }
        }
        if waiting_for_lesson {
            wake = Some(wake.map_or(Duration::from_millis(1500), |w| {
                w.min(Duration::from_millis(1500))
            }));
        }
        if let Some((_, at)) = &self.toast {
            if at.elapsed() > Duration::from_secs(4) {
                self.toast = None;
            } else {
                wake = Some(Duration::from_millis(500));
            }
        }
        wake
    }

    fn flush_active(&mut self, outcome: LessonOutcome) {
        let Some(a) = self.active.take() else { return };
        #[allow(unused_mut)]
        let mut events = a.run.into_events();
        #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
        if let Some(k) = &self.kernel {
            use crate::capture::{EvdevStamp, RawKeyInfo, TimestampSource};
            let src = EvdevStamp::new(a.origin, a.origin_mono_ns, k.take());
            for e in events.iter_mut().filter(|e| !e.is_pause_marker) {
                e.t_kernel_ns = src.stamp(&RawKeyInfo {
                    received_at: a.origin + Duration::from_nanos(e.t_ns),
                    scancode: e.scancode,
                    is_down: e.is_down,
                });
            }
        }
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        match db::finish_lesson(conn, a.lesson.id, a.session_id, &events, outcome) {
            Ok(()) => {
                if let Some(f) = self.last.as_mut() {
                    if f.lesson.id == a.lesson.id {
                        f.saved = true;
                    }
                }
            }
            Err(e) => self.toast(format!("could not save lesson: {e}")),
        }
        self.next_checked = Instant::now() - Duration::from_secs(10);
        self.refresh_queue();
        if outcome == LessonOutcome::Done {
            self.refresh_history();
        }
    }

    /// Leave the current lesson: saved as aborted if anything was typed,
    /// otherwise put back in the queue untouched.
    pub fn abandon_active(&mut self) {
        let Some(a) = self.active.as_ref() else {
            return;
        };
        if a.run.state() == RunState::Finished {
            self.flush_active(LessonOutcome::Done);
        } else if a.run.has_typed() {
            self.flush_active(LessonOutcome::Aborted);
        } else if let Some(a) = self.active.take() {
            if let Some(conn) = &self.conn {
                let _ = db::revert_to_pending(conn, a.lesson.id);
            }
            self.refresh_queue();
        }
    }

    pub fn do_paused_action(&mut self, action: PausedAction) {
        match action {
            PausedAction::Restart => {
                if let Some(a) = self.active.as_mut() {
                    // The abandoned attempt is discarded (decision D16).
                    a.run = LessonRun::new(&a.lesson.text, self.cfg.client.stop_on_error);
                    a.origin = Instant::now();
                    #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
                    {
                        a.origin_mono_ns = crate::capture::monotonic_now_ns();
                        if let Some(k) = &self.kernel {
                            k.clear();
                        }
                    }
                }
            }
            PausedAction::Skip => {
                if self.active.as_ref().is_some_and(|a| a.run.has_typed()) {
                    self.flush_active(LessonOutcome::Aborted);
                } else {
                    self.abandon_active();
                }
                self.start_next();
            }
            PausedAction::Quit => {
                self.abandon_active();
                self.screen = Screen::Home;
            }
        }
    }

    pub fn skip_calibration(&mut self) {
        if let Some(conn) = &self.conn {
            match db::skip_calibration(conn) {
                Ok(n) => self.toast(format!(
                    "Skipped {n} calibration lessons. Adaptive lessons start from your data so far."
                )),
                Err(e) => self.toast(format!("database error: {e}")),
            }
        }
        self.refresh_queue();
    }

    pub fn apply_theme(&mut self, ctx: &egui::Context) {
        if self.theme_applied.as_deref() != Some(self.cfg.client.theme.as_str()) {
            self.palette = theme::palette(&self.cfg.client.theme);
            theme::apply(ctx, &self.palette);
            self.theme_applied = Some(self.cfg.client.theme.clone());
        }
    }

    pub fn save_settings(&mut self) {
        match self.cfg.save() {
            Ok(p) => {
                self.settings_dirty = false;
                self.toast(format!("Settings saved to {}", p.display()));
            }
            Err(e) => self.toast(format!("Could not save settings: {e}")),
        }
    }

    pub fn report_path(&self) -> std::path::PathBuf {
        self.cfg.data_dir().join("reports").join("index.html")
    }

    pub fn open_report(&mut self) {
        let p = self.report_path();
        if !p.exists() {
            self.toast("No report yet: the engine writes it after your first analysed lesson.");
            return;
        }
        if let Err(e) = open_in_browser(&p) {
            self.toast(format!("Could not open {}: {e}", p.display()));
        }
    }

    pub fn shutdown(&mut self) {
        self.abandon_active();
        self.engine.stop();
    }

    /// Minutes left in the current calibration sitting at the user's pace.
    pub fn minutes_left(&self, chars: i64) -> f64 {
        let wpm = self
            .level
            .as_ref()
            .map(|l| l.wpm)
            .filter(|w| *w > 5.0)
            .unwrap_or(35.0);
        chars as f64 / (wpm * 5.0)
    }
}

pub fn open_in_browser(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    let mut cmd = std::process::Command::new("xdg-open");
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    };
    cmd.arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}
