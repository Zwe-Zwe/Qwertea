//! Profiles: each person gets their own database, so their progress, model,
//! lessons, theme and typing settings never mix with anyone else's.
//!
//! * The registry (`profiles.toml` in the data directory) lists the profiles
//!   and remembers the active one. The first profile is the database that
//!   already existed (from Qwertea 1.0.x), so upgrading loses nothing.
//! * A profile's settings live inside its own database (`profile_settings`,
//!   migration 003), so exporting a profile is a single file (decision D28).

use crate::config::Config;
use crate::session::ErrorMode;
use chrono::{DateTime, Local, NaiveDate, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// --------------------------------------------------------------------- settings

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaretStyle {
    Underline,
    Block,
    Bar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveStats {
    Full,
    Minimal,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypingFont {
    Atkinson,
    Hack,
    Proportional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LessonLength {
    Short,
    Medium,
    Long,
    Custom,
}

macro_rules! str_enum {
    ($t:ty { $($v:ident => $s:literal, $label:literal),* $(,)? }) => {
        impl $t {
            pub const ALL: &'static [$t] = &[$(<$t>::$v),*];
            pub fn key(self) -> &'static str { match self { $(<$t>::$v => $s),* } }
            pub fn label(self) -> &'static str { match self { $(<$t>::$v => $label),* } }
            pub fn parse(s: &str) -> Option<Self> { match s { $($s => Some(<$t>::$v),)* _ => None } }
        }
    };
}
str_enum!(CaretStyle { Underline => "underline", "Underline", Block => "block", "Block", Bar => "bar", "Bar" });
str_enum!(LiveStats { Full => "full", "Full", Minimal => "minimal", "Time & words", Hidden => "hidden", "Hidden" });
str_enum!(TypingFont {
    Atkinson => "atkinson", "Atkinson Mono",
    Hack => "hack", "Hack",
    Proportional => "proportional", "Atkinson (proportional)",
});
str_enum!(LessonLength { Short => "short", "Short", Medium => "medium", "Medium", Long => "long", "Long", Custom => "custom", "Custom" });
str_enum!(ErrorMode {
    Normal => "normal", "Move on",
    StopOnError => "stop", "Stop on error",
    StrictWord => "strict", "Fix before next word",
});

/// Everything a profile remembers about how its owner likes to type.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileSettings {
    pub name: String,
    pub theme: String,
    pub font_size: f32,
    pub typing_font: TypingFont,
    pub caret: CaretStyle,
    pub smooth_caret: bool,
    pub live_stats: LiveStats,
    pub error_mode: ErrorMode,
    pub sound: bool,
    pub lesson_length: LessonLength,
    pub lesson_chars: u32,
    pub capitals: bool,
    pub punctuation: bool,
    pub numbers: bool,
    pub goal_wpm: u32,
    pub goal_accuracy: u32,
    pub daily_minutes: u32,
}

impl Default for ProfileSettings {
    fn default() -> Self {
        Self {
            name: "Me".into(),
            theme: "sencha".into(),
            font_size: 28.0,
            typing_font: TypingFont::Atkinson,
            caret: CaretStyle::Underline,
            smooth_caret: false,
            live_stats: LiveStats::Full,
            error_mode: ErrorMode::Normal,
            sound: false,
            lesson_length: LessonLength::Medium,
            lesson_chars: 220,
            capitals: true,
            punctuation: true,
            numbers: false,
            goal_wpm: 60,
            goal_accuracy: 96,
            daily_minutes: 10,
        }
    }
}

fn b(v: bool) -> String {
    if v { "true" } else { "false" }.into()
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

impl ProfileSettings {
    pub fn to_pairs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("name", self.name.clone()),
            ("theme", self.theme.clone()),
            ("font_size", format!("{}", self.font_size)),
            ("typing_font", self.typing_font.key().into()),
            ("caret", self.caret.key().into()),
            ("smooth_caret", b(self.smooth_caret)),
            ("live_stats", self.live_stats.key().into()),
            ("error_mode", self.error_mode.key().into()),
            ("sound", b(self.sound)),
            ("lesson_length", self.lesson_length.key().into()),
            ("lesson_chars", self.lesson_chars.to_string()),
            ("capitals", b(self.capitals)),
            ("punctuation", b(self.punctuation)),
            ("numbers", b(self.numbers)),
            ("goal_wpm", self.goal_wpm.to_string()),
            ("goal_accuracy", self.goal_accuracy.to_string()),
            ("daily_minutes", self.daily_minutes.to_string()),
        ]
    }

    /// Apply stored values over `self`; unknown or malformed values are ignored.
    pub fn apply_pairs(&mut self, pairs: &[(String, String)]) {
        for (k, v) in pairs {
            match k.as_str() {
                "name" if !v.trim().is_empty() => self.name = v.trim().to_string(),
                "theme" => self.theme = v.clone(),
                "font_size" => self.font_size = v.parse().unwrap_or(self.font_size),
                "typing_font" => {
                    self.typing_font = TypingFont::parse(v).unwrap_or(self.typing_font)
                }
                "caret" => self.caret = CaretStyle::parse(v).unwrap_or(self.caret),
                "smooth_caret" => self.smooth_caret = parse_bool(v).unwrap_or(self.smooth_caret),
                "live_stats" => self.live_stats = LiveStats::parse(v).unwrap_or(self.live_stats),
                "error_mode" => self.error_mode = ErrorMode::parse(v).unwrap_or(self.error_mode),
                "sound" => self.sound = parse_bool(v).unwrap_or(self.sound),
                "lesson_length" => {
                    self.lesson_length = LessonLength::parse(v).unwrap_or(self.lesson_length)
                }
                "lesson_chars" => self.lesson_chars = v.parse().unwrap_or(self.lesson_chars),
                "capitals" => self.capitals = parse_bool(v).unwrap_or(self.capitals),
                "punctuation" => self.punctuation = parse_bool(v).unwrap_or(self.punctuation),
                "numbers" => self.numbers = parse_bool(v).unwrap_or(self.numbers),
                "goal_wpm" => self.goal_wpm = v.parse().unwrap_or(self.goal_wpm),
                "goal_accuracy" => self.goal_accuracy = v.parse().unwrap_or(self.goal_accuracy),
                "daily_minutes" => self.daily_minutes = v.parse().unwrap_or(self.daily_minutes),
                _ => {}
            }
        }
        self.font_size = self.font_size.clamp(14.0, 64.0);
        self.lesson_chars = self.lesson_chars.clamp(40, 2000);
        self.goal_accuracy = self.goal_accuracy.clamp(50, 100);
    }

    /// Load from the profile database. A profile without stored settings (a
    /// database from Qwertea 1.0.x) starts from the old `[client]` settings in
    /// config.toml, so nobody loses their theme or text size on upgrade.
    pub fn load(conn: &Connection, legacy: &crate::config::Client) -> rusqlite::Result<Self> {
        let mut st = conn.prepare("SELECT key, value FROM profile_settings")?;
        let pairs: Vec<(String, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut s = Self::default();
        if pairs.is_empty() {
            s.theme = legacy.theme.clone();
            s.font_size = legacy.font_size;
            s.live_stats = if legacy.live_wpm {
                LiveStats::Full
            } else {
                LiveStats::Minimal
            };
            s.error_mode = legacy.stop_on_error.into();
            s.save(conn)?;
        } else {
            s.apply_pairs(&pairs);
        }
        Ok(s)
    }

    /// Save every setting in one transaction (only between lessons).
    pub fn save(&self, conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let r = (|| {
            let mut st = conn.prepare(
                "INSERT INTO profile_settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            )?;
            for (k, v) in self.to_pairs() {
                st.execute(params![k, v])?;
            }
            Ok(())
        })();
        conn.execute_batch(if r.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
        r
    }
}

// --------------------------------------------------------------------- registry

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub id: String,
    pub name: String,
    /// Database path, relative to the data directory (or absolute).
    pub db: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Registry {
    pub active: String,
    #[serde(rename = "profile", default)]
    pub profiles: Vec<ProfileEntry>,
    #[serde(skip)]
    root: PathBuf,
}

fn slug(name: &str) -> String {
    let s: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "profile".into()
    } else {
        s.chars().take(32).collect()
    }
}

impl Registry {
    fn file(root: &Path) -> PathBuf {
        root.join("profiles.toml")
    }

    /// Load the registry, creating it on first run with the existing database
    /// as the first profile.
    pub fn ensure(cfg: &Config) -> Self {
        let root = cfg.data_dir();
        let path = Self::file(&root);
        if let Some(mut r) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| toml::from_str::<Registry>(&t).ok())
            .filter(|r| !r.profiles.is_empty())
        {
            r.root = root;
            if !r.profiles.iter().any(|p| p.id == r.active) {
                r.active = r.profiles[0].id.clone();
            }
            return r;
        }
        let db = cfg.db_path();
        let rel = db
            .strip_prefix(&root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| db.to_string_lossy().into_owned());
        let r = Registry {
            active: "default".into(),
            profiles: vec![ProfileEntry {
                id: "default".into(),
                name: "Me".into(),
                db: rel,
            }],
            root,
        };
        let _ = r.save();
        r
    }

    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(Self::file(&self.root), text)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn active(&self) -> &ProfileEntry {
        self.profiles
            .iter()
            .find(|p| p.id == self.active)
            .unwrap_or(&self.profiles[0])
    }

    pub fn db_path(&self, p: &ProfileEntry) -> PathBuf {
        let d = PathBuf::from(&p.db);
        if d.is_absolute() {
            d
        } else {
            self.root.join(d)
        }
    }

    fn unique_id(&self, name: &str) -> String {
        let base = slug(name);
        let mut id = base.clone();
        let mut n = 2;
        while self.profiles.iter().any(|p| p.id == id)
            || self.root.join("profiles").join(&id).exists()
        {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }

    /// Add a new, empty profile (its database is created when first opened).
    pub fn create(&mut self, name: &str) -> std::io::Result<ProfileEntry> {
        let id = self.unique_id(name);
        let dir = self.root.join("profiles").join(&id);
        std::fs::create_dir_all(&dir)?;
        let entry = ProfileEntry {
            id: id.clone(),
            name: name.trim().to_string(),
            db: format!("profiles/{id}/qwertea.db"),
        };
        self.profiles.push(entry.clone());
        self.save()?;
        Ok(entry)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> std::io::Result<()> {
        if let Some(p) = self.profiles.iter_mut().find(|p| p.id == id) {
            p.name = name.trim().to_string();
        }
        self.save()
    }

    /// Remove a profile and its data. The active profile cannot be removed,
    /// and only databases inside `profiles/<id>/` are deleted from disk.
    pub fn remove(&mut self, id: &str) -> std::io::Result<()> {
        if id == self.active || self.profiles.len() <= 1 {
            return Err(std::io::Error::other("cannot remove the active profile"));
        }
        let Some(idx) = self.profiles.iter().position(|p| p.id == id) else {
            return Ok(());
        };
        let entry = self.profiles.remove(idx);
        let dir = self.root.join("profiles").join(&entry.id);
        if self.db_path(&entry).starts_with(&dir) && dir.is_dir() {
            std::fs::remove_dir_all(&dir)?;
        }
        self.save()
    }

    /// Bring in a profile exported with `export`.
    pub fn import(&mut self, file: &Path) -> Result<ProfileEntry, String> {
        let conn = Connection::open_with_flags(file, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| format!("cannot open {}: {e}", file.display()))?;
        let ok: bool = conn
            .query_row(
                "SELECT COUNT(*) = 2 FROM sqlite_master WHERE type='table' AND name IN ('schema_version','key_events')",
                [],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if !ok {
            return Err(format!("{} is not a Qwertea profile", file.display()));
        }
        let name: String = conn
            .query_row(
                "SELECT value FROM profile_settings WHERE key='name'",
                [],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| {
                file.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Imported".into())
            });
        drop(conn);
        let entry = self.create(&name).map_err(|e| e.to_string())?;
        let dest = self.db_path(&entry);
        std::fs::copy(file, &dest).map_err(|e| format!("cannot copy profile: {e}"))?;
        Ok(entry)
    }
}

/// Write a consistent single-file copy of a profile (settings included).
pub fn export(conn: &Connection, dest: &Path) -> rusqlite::Result<()> {
    if dest.exists() {
        let _ = std::fs::remove_file(dest);
    }
    conn.execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()])?;
    Ok(())
}

/// Where exports go: ~/Downloads if it exists, otherwise the home folder.
pub fn export_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let dl = home.join("Downloads");
    if dl.is_dir() {
        dl
    } else {
        home
    }
}

// --------------------------------------------------------------------- daily practice

/// Local calendar day of a stored UTC timestamp.
pub fn local_day(utc: &str) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(utc)
        .ok()
        .map(|d| d.with_timezone(&Local).date_naive())
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Consecutive days with practice, ending today (or yesterday, so a streak is
/// not lost before you have had a chance to practise today).
pub fn streak(days: &[NaiveDate], today: NaiveDate) -> u32 {
    let set: std::collections::BTreeSet<NaiveDate> = days.iter().copied().collect();
    let mut day = if set.contains(&today) {
        today
    } else {
        match today.pred_opt() {
            Some(y) if set.contains(&y) => y,
            _ => return 0,
        }
    };
    let mut n = 0;
    while set.contains(&day) {
        n += 1;
        match day.pred_opt() {
            Some(d) => day = d,
            None => break,
        }
    }
    n
}

pub fn now_utc() -> DateTime<Utc> {
    Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_the_database() {
        let c = Connection::open_in_memory().unwrap();
        crate::db::configure(&c).unwrap();
        let legacy = crate::config::Client {
            theme: "chai".into(),
            font_size: 32.0,
            stop_on_error: true,
            ..Default::default()
        };
        let first = ProfileSettings::load(&c, &legacy).unwrap();
        assert_eq!(first.theme, "chai", "a 1.0.x profile keeps its theme");
        assert_eq!(first.error_mode, ErrorMode::StopOnError);
        let mut s = first.clone();
        s.name = "Aye".into();
        s.caret = CaretStyle::Block;
        s.lesson_length = LessonLength::Custom;
        s.lesson_chars = 500;
        s.numbers = true;
        s.error_mode = ErrorMode::StrictWord;
        s.save(&c).unwrap();
        let back = ProfileSettings::load(&c, &crate::config::Client::default()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn streaks_count_consecutive_days() {
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let days = [
            d("2026-11-01"),
            d("2026-11-02"),
            d("2026-11-03"),
            d("2026-10-30"),
        ];
        assert_eq!(streak(&days, d("2026-11-03")), 3);
        assert_eq!(streak(&days, d("2026-11-04")), 3, "today not practised yet");
        assert_eq!(streak(&days, d("2026-11-05")), 0);
    }

    #[test]
    fn registry_create_import_remove() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.general.db_path = dir.path().join("qwertea.db").to_string_lossy().into_owned();
        let mut reg = Registry::ensure(&cfg);
        assert_eq!(reg.profiles.len(), 1);
        assert_eq!(reg.db_path(reg.active()), dir.path().join("qwertea.db"));
        // A second profile gets its own database.
        let p = reg.create("Mya Mya").unwrap();
        assert_eq!(p.id, "mya-mya");
        let db = reg.db_path(&p);
        assert!(db.starts_with(dir.path().join("profiles/mya-mya")));
        let conn = crate::db::open(&db).unwrap();
        let s = ProfileSettings {
            name: "Mya Mya".into(),
            ..ProfileSettings::default()
        };
        s.save(&conn).unwrap();
        // Export and import as a new profile.
        let file = dir.path().join("export.qwertea");
        export(&conn, &file).unwrap();
        drop(conn);
        let imported = reg.import(&file).unwrap();
        assert_eq!(imported.name, "Mya Mya");
        assert_ne!(imported.id, p.id);
        // Reload from disk.
        let again = Registry::ensure(&cfg);
        assert_eq!(again.profiles.len(), 3);
        // Remove: not the active one, data removed.
        assert!(reg.remove("default").is_err());
        reg.remove(&p.id).unwrap();
        assert!(!dir.path().join("profiles/mya-mya").exists());
        assert!(reg.import(&dir.path().join("nope.db")).is_err());
    }
}
