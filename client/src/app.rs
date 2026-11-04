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
use crate::profile::{self, ProfileSettings, Registry};
use crate::session::{CharState, KeyKind, LessonRun, RunState};
use crate::sound::KeySound;
use crate::stats::{self, LessonStats};
use crate::ui::theme::{self, Palette};
use crate::update::Updater;
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
    Profile,
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
    /// Your averages over the last 10 lessons *before* this one: (wpm, accuracy).
    pub recent: Option<(f64, f64)>,
    /// Best WPM before this lesson.
    pub best_before: Option<f64>,
}

/// One finished lesson, as the progress screens show it.
#[derive(Debug, Clone, PartialEq)]
pub struct HistLesson {
    /// 1-based position in the history.
    pub n: usize,
    pub wpm: f64,
    pub accuracy: f64,
    pub seconds: f64,
    pub phase: String,
    /// Local calendar day it was finished.
    pub day: Option<chrono::NaiveDate>,
}

/// Lesson history for the stats screen and home overview.
#[derive(Default)]
pub struct History {
    pub lessons: Vec<HistLesson>,
    pub loaded_at: Option<Instant>,
}

impl History {
    pub fn recent(&self, n: usize) -> &[HistLesson] {
        let k = self.lessons.len().saturating_sub(n);
        &self.lessons[k..]
    }
    pub fn best_wpm(&self) -> Option<f64> {
        self.lessons.iter().map(|l| l.wpm).reduce(f64::max)
    }
    pub fn total_seconds(&self) -> f64 {
        self.lessons
            .iter()
            .map(|l| l.seconds)
            .fold(0.0, |a, b| a + b)
    }
    /// Seconds typed on `day`.
    pub fn seconds_on(&self, day: chrono::NaiveDate) -> f64 {
        self.lessons
            .iter()
            .filter(|l| l.day == Some(day))
            .map(|l| l.seconds)
            .fold(0.0, |a, b| a + b)
    }
    /// Days in a row with practice, ending today or yesterday.
    pub fn streak(&self, today: chrono::NaiveDate) -> u32 {
        let days: Vec<_> = self.lessons.iter().filter_map(|l| l.day).collect();
        profile::streak(&days, today)
    }
}

/// The live typing demo in Settings: try fonts, carets and sounds.
pub struct Demo {
    pub run: LessonRun,
    pub origin: Instant,
    pub mode: crate::session::ErrorMode,
}

impl Demo {
    pub const TEXT: &'static str = "The quick brown fox jumps over the lazy dog. \
        Pack my box with 5 dozen liquor jugs! (Sphinx of black quartz, judge my vow.)";

    pub fn new(mode: crate::session::ErrorMode) -> Self {
        Self {
            run: LessonRun::new(Self::TEXT, mode),
            origin: Instant::now(),
            mode,
        }
    }

    /// Feed one key (from egui input; the demo is not timed precisely).
    pub fn key(&mut self, kind: KeyKind, sound: &KeySound, sound_on: bool) {
        if self.run.state() == RunState::Finished {
            *self = Self::new(self.mode);
        }
        let t = self.origin.elapsed().as_nanos() as u64;
        let n = self.run.events().len();
        let down = crate::session::KeyInput {
            scancode: 0,
            is_down: true,
            kind,
        };
        self.run.on_key(t, down);
        self.run.on_key(
            t + 1,
            crate::session::KeyInput {
                is_down: false,
                ..down
            },
        );
        if sound_on && self.run.events().len() > n {
            if let Some(ev) = self.run.events().get(n) {
                let wrong = ev.produced_char.is_some()
                    && (self.run.blocked_error
                        || self.run.char_state(ev.target_idx as usize) == CharState::Wrong);
                sound.click(wrong);
            }
        }
    }
}

/// Text fields and confirmations on the profile screen.
#[derive(Default)]
pub struct ProfileUi {
    pub rename: Option<String>,
    pub new_name: String,
    pub import_path: String,
    pub confirm_delete: Option<String>,
}

pub struct App {
    pub cfg: Config,
    /// All profiles on this computer and which one is open.
    pub registry: Registry,
    /// The open profile's settings (saved in its database).
    pub profile: ProfileSettings,
    /// When profile settings / device settings last changed (saved shortly after).
    pub profile_dirty_at: Option<Instant>,
    pub cfg_dirty_at: Option<Instant>,
    pub profile_ui: ProfileUi,
    pub demo: Option<Demo>,
    pub sound: KeySound,
    pub updater: Updater,
    /// Start this program after quitting (restart after an update).
    pub restart_with: Option<std::path::PathBuf>,
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
    /// The next lesson's targets with what the model knows about each.
    pub next_targets: Vec<(db::Target, Option<db::ItemStat>)>,
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
    pub theme_applied: Option<String>,
    pub quit_requested: bool,
    #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
    kernel: Option<crate::capture::evdev_reader::KernelLog>,
}

impl App {
    pub fn new(cfg: Config) -> Self {
        let registry = Registry::ensure(&cfg);
        let db_path = registry.db_path(registry.active());
        let (conn, db_error, profile) = Self::open_profile_db(&db_path, &cfg);
        let engine = EngineProc::start(&cfg, &db_path);
        let palette = theme::palette(&profile.theme);
        let layout = Layout::load(&cfg.layout_file());
        let updater = Updater::new(&cfg);
        #[allow(unused_mut)]
        let mut app = Self {
            registry,
            profile,
            profile_dirty_at: None,
            cfg_dirty_at: None,
            profile_ui: ProfileUi::default(),
            demo: None,
            sound: KeySound::off(),
            updater,
            restart_with: None,
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
            next_targets: vec![],
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
            theme_applied: None,
            quit_requested: false,
            #[cfg(all(target_os = "linux", feature = "evdev-capture"))]
            kernel: None,
            cfg,
        };
        app.start_evdev();
        app.sync_sound();
        if app.cfg.updates.check {
            app.updater.check();
        }
        app.refresh_queue();
        app.refresh_history();
        app
    }

    /// Open a profile database: recover an interrupted lesson and load its settings.
    fn open_profile_db(
        path: &std::path::Path,
        cfg: &Config,
    ) -> (Option<Connection>, Option<String>, ProfileSettings) {
        match db::open(path) {
            Ok(c) => {
                let _ = db::recover_running(&c);
                let s = ProfileSettings::load(&c, &cfg.client).unwrap_or_default();
                (Some(c), None, s)
            }
            Err(e) => (
                None,
                Some(format!("cannot open {}: {e}", path.display())),
                ProfileSettings::default(),
            ),
        }
    }

    /// The open profile's database.
    pub fn db_path(&self) -> std::path::PathBuf {
        self.registry.db_path(self.registry.active())
    }

    /// Close the open profile (saving it) and open `id`: its own database,
    /// engine, settings, theme and history.
    pub fn switch_profile(&mut self, id: &str) {
        if id == self.registry.active || !self.registry.profiles.iter().any(|p| p.id == id) {
            return;
        }
        self.abandon_active();
        self.flush_settings();
        self.engine.stop();
        self.registry.active = id.to_string();
        if let Err(e) = self.registry.save() {
            self.toast(format!("could not save profiles: {e}"));
        }
        let path = self.db_path();
        let (conn, err, profile) = Self::open_profile_db(&path, &self.cfg);
        self.conn = conn;
        self.db_error = err;
        self.profile = profile;
        // Keep the registry's name in step with the profile's own.
        let name = self.profile.name.clone();
        if self.registry.active().name != name {
            let _ = self.registry.rename(id, &name);
        }
        self.engine = EngineProc::start(&self.cfg, &path);
        self.theme_applied = None;
        self.session = None;
        self.last_calib = None;
        self.last = None;
        self.intro = None;
        self.confirm_skip_calibration = false;
        self.profile_ui = ProfileUi::default();
        self.sync_sound();
        self.refresh_queue();
        self.refresh_history();
        self.toast(format!("Switched to {name}"));
    }

    /// Create a profile and switch to it.
    pub fn create_profile(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        match self.registry.create(name) {
            Ok(entry) => {
                // Seed the new profile's settings with its name.
                let path = self.registry.db_path(&entry);
                if let Ok(c) = db::open(&path) {
                    let s = ProfileSettings {
                        name: name.to_string(),
                        ..ProfileSettings::default()
                    };
                    let _ = s.save(&c);
                }
                self.switch_profile(&entry.id);
            }
            Err(e) => self.toast(format!("could not create profile: {e}")),
        }
    }

    pub fn rename_profile(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        self.profile.name = name.to_string();
        self.mark_profile_dirty();
        let id = self.registry.active.clone();
        if let Err(e) = self.registry.rename(&id, name) {
            self.toast(format!("could not save profiles: {e}"));
        }
    }

    pub fn delete_profile(&mut self, id: &str) {
        let name = self
            .registry
            .profiles
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        match self.registry.remove(id) {
            Ok(()) => self.toast(format!("Deleted {name}")),
            Err(e) => self.toast(format!("could not delete: {e}")),
        }
    }

    /// Save the open profile as one file in ~/Downloads (or home).
    pub fn export_profile(&mut self) {
        self.flush_settings();
        let Some(conn) = &self.conn else { return };
        let stamp = chrono::Local::now().format("%Y-%m-%d");
        let file = profile::export_dir().join(format!(
            "qwertea-{}-{stamp}.qwertea",
            self.registry.active().id
        ));
        match profile::export(conn, &file) {
            Ok(()) => self.toast(format!("Saved {}", file.display())),
            Err(e) => self.toast(format!("export failed: {e}")),
        }
    }

    pub fn import_profile(&mut self, path: &str) {
        let p = std::path::PathBuf::from(path.trim().trim_matches('"'));
        let p = match p.strip_prefix("~") {
            Ok(rest) => std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_default()
                .join(rest),
            Err(_) => p,
        };
        match self.registry.import(&p) {
            Ok(entry) => {
                self.profile_ui.import_path.clear();
                self.toast(format!("Imported {}", entry.name));
            }
            Err(e) => self.toast(e),
        }
    }

    /// A profile setting changed in the UI; saved shortly after (see `tick`).
    pub fn mark_profile_dirty(&mut self) {
        self.profile_dirty_at = Some(Instant::now());
    }

    /// A device setting (config.toml) changed in the UI.
    pub fn mark_cfg_dirty(&mut self) {
        self.cfg_dirty_at = Some(Instant::now());
    }

    /// Write pending setting changes now.
    pub fn flush_settings(&mut self) {
        if self.profile_dirty_at.take().is_some() {
            if let Some(conn) = &self.conn {
                if let Err(e) = self.profile.save(conn) {
                    self.toast(format!("Could not save settings: {e}"));
                }
            }
        }
        if self.cfg_dirty_at.take().is_some() {
            if let Err(e) = self.cfg.save() {
                self.toast(format!("Could not save settings: {e}"));
            }
        }
    }

    /// Open or close the sound output to match the profile setting.
    pub fn sync_sound(&mut self) {
        self.sound
            .configure(self.profile.sound, self.profile.sound_volume);
        if self.profile.sound != crate::sound::SoundStyle::Off {
            self.sound.enable();
        } else {
            self.sound.disable();
        }
    }

    /// Start the updated program after this one exits.
    pub fn restart(&mut self) {
        self.restart_with = self.updater.restart_target();
        self.quit_requested = true;
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
        let mode = self.cfg.general.mode.clone();
        self.next_targets = self
            .next
            .as_ref()
            .map(|l| {
                l.targets()
                    .into_iter()
                    .map(|t| {
                        let st = db::item_stat(conn, &mode, &t.kind, &t.item).ok().flatten();
                        (t, st)
                    })
                    .collect()
            })
            .unwrap_or_default();
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
                    out.push(HistLesson {
                        n: out.len() + 1,
                        wpm: s.wpm,
                        accuracy: s.accuracy,
                        seconds: s.seconds,
                        phase: l.phase.clone(),
                        day: profile::local_day(&l.finished_at),
                    });
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
        let run = LessonRun::new(&lesson.text, self.profile.error_mode);
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
        let n_before = a.run.events().len();
        a.run.on_key(t, input);
        if self.profile.sound != crate::sound::SoundStyle::Off
            && input.is_down
            && !was_paused
            && a.run.events().len() > n_before
        {
            if let Some(ev) = a.run.events().last() {
                if ev.produced_char.is_some() || ev.is_backspace {
                    let wrong = ev.produced_char.is_some()
                        && (a.run.blocked_error
                            || a.run.char_state(ev.target_idx as usize) == CharState::Wrong);
                    self.sound.click(wrong);
                }
            }
        }
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
                let recent = self.history.recent(10);
                let recent = (!recent.is_empty()).then(|| {
                    let n = recent.len() as f64;
                    (
                        recent.iter().map(|l| l.wpm).sum::<f64>() / n,
                        recent.iter().map(|l| l.accuracy).sum::<f64>() / n,
                    )
                });
                self.last = Some(Finished {
                    lesson: a.lesson.clone(),
                    stats,
                    saved: false,
                    recent,
                    best_before: self.history.best_wpm(),
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
        // Settings save themselves a moment after the last change (no Save
        // button), never while a lesson is running.
        let settle = Duration::from_millis(400);
        if self.active.is_none() {
            let due = |t: &Option<Instant>| t.is_some_and(|t| t.elapsed() >= settle);
            if due(&self.profile_dirty_at) || due(&self.cfg_dirty_at) {
                self.flush_settings();
            }
        }
        if self.profile_dirty_at.is_some() || self.cfg_dirty_at.is_some() {
            wake = Some(wake.map_or(settle, |w| w.min(settle)));
        }
        if self.updater.busy() {
            wake = Some(wake.map_or(Duration::from_millis(500), |w| {
                w.min(Duration::from_millis(500))
            }));
        }
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
                    a.run = LessonRun::new(&a.lesson.text, self.profile.error_mode);
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
        if self.theme_applied.is_none() {
            theme::install_fonts(ctx);
        }
        if self.theme_applied.as_deref() != Some(self.profile.theme.as_str()) {
            self.palette = theme::palette(&self.profile.theme);
            theme::apply(ctx, &self.palette);
            self.theme_applied = Some(self.profile.theme.clone());
        }
    }

    /// The engine writes each profile's report next to its database.
    pub fn report_path(&self) -> std::path::PathBuf {
        self.db_path()
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| self.cfg.data_dir())
            .join("reports")
            .join("index.html")
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
        self.flush_settings();
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
