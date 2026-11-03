//! `config.toml` loading and saving. Only the sections the client needs are
//! typed; unknown sections (engine settings) are ignored on load and left
//! untouched (comments included) on save.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct General {
    pub mode: String,
    pub layout: String,
    pub db_path: String,
}

impl Default for General {
    fn default() -> Self {
        Self {
            mode: "prose".into(),
            layout: "qwerty-us".into(),
            db_path: "data/qwertea.db".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresentModePref {
    Mailbox,
    Immediate,
    Fifo,
}

impl PresentModePref {
    pub const ALL: [Self; 3] = [Self::Mailbox, Self::Immediate, Self::Fifo];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mailbox => "mailbox",
            Self::Immediate => "immediate",
            Self::Fifo => "fifo",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimestampSourceKind {
    Winit,
    Evdev,
}

impl TimestampSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Winit => "winit",
            Self::Evdev => "evdev",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Client {
    pub font_size: f32,
    pub theme: String,
    pub stop_on_error: bool,
    pub present_mode: PresentModePref,
    pub timestamp_source: TimestampSourceKind,
    pub keyboard_device: String,
    /// Show live WPM in the typing HUD.
    pub live_wpm: bool,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            font_size: 28.0,
            theme: "dark".into(),
            stop_on_error: false,
            present_mode: PresentModePref::Mailbox,
            timestamp_source: TimestampSourceKind::Winit,
            keyboard_device: String::new(),
            live_wpm: true,
        }
    }
}

/// How the client starts the analysis engine (section D15).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Engine {
    pub autostart: bool,
    /// Command run from the repository root.
    pub command: Vec<String>,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            autostart: true,
            command: [
                "uv",
                "run",
                "--project",
                "engine",
                "qwertea-engine",
                "serve",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub client: Client,
    pub engine: Engine,
    /// Directory containing `config.toml`; relative paths resolve against it.
    #[serde(skip)]
    pub root: PathBuf,
    /// The file this was loaded from, if any (settings are saved back to it).
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

impl Config {
    pub fn parse(text: &str, root: PathBuf) -> Result<Self, toml::de::Error> {
        let mut cfg: Config = toml::from_str(text)?;
        cfg.root = root;
        Ok(cfg)
    }

    /// Load `config.toml` from `$QWERTEA_CONFIG`, or the first `config.toml`
    /// found walking up from the current directory, then from the executable's
    /// directory. Falls back to defaults.
    pub fn load() -> Self {
        let path = std::env::var_os("QWERTEA_CONFIG")
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok().and_then(|d| find_config(&d)))
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|e| e.parent().and_then(find_config))
            });
        let Some(p) = path else {
            return Self::default_at(PathBuf::from("."));
        };
        let root = p
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let mut cfg = match std::fs::read_to_string(&p) {
            Ok(text) => Self::parse(&text, root.clone()).unwrap_or_else(|e| {
                eprintln!("qwertea: invalid {}: {e}; using defaults", p.display());
                Self::default_at(root)
            }),
            Err(e) => {
                eprintln!("qwertea: cannot read {}: {e}; using defaults", p.display());
                Self::default_at(root)
            }
        };
        cfg.path = Some(p);
        cfg
    }

    fn default_at(root: PathBuf) -> Self {
        Self {
            root,
            ..Self::default()
        }
    }

    pub fn db_path(&self) -> PathBuf {
        let p = PathBuf::from(&self.general.db_path);
        if p.is_absolute() {
            p
        } else {
            self.root.join(p)
        }
    }

    pub fn data_dir(&self) -> PathBuf {
        self.db_path()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.root.clone())
    }

    pub fn layout_file(&self) -> PathBuf {
        self.root
            .join("engine/data/layouts")
            .join(format!("{}.toml", self.general.layout))
    }

    /// Write the `[client]` and `[engine].autostart` settings back to the
    /// config file, preserving everything else (including comments).
    pub fn save(&self) -> Result<PathBuf, String> {
        let path = self
            .path
            .clone()
            .unwrap_or_else(|| self.root.join("config.toml"));
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let new = update_toml(&text, self)?;
        std::fs::write(&path, new).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }
}

pub fn update_toml(text: &str, cfg: &Config) -> Result<String, String> {
    use toml_edit::{DocumentMut, Item, Table, Value};
    /// Set `table[key]`, keeping the old value's surrounding whitespace and comment.
    fn set(table: &mut Item, key: &str, new: impl Into<Value>) {
        let mut v: Value = new.into();
        if let Some(old) = table.get(key).and_then(Item::as_value) {
            *v.decor_mut() = old.decor().clone();
        }
        table[key] = Item::Value(v);
    }
    let mut doc: DocumentMut = text.parse().map_err(|e| format!("config.toml: {e}"))?;
    for section in ["client", "engine"] {
        if !doc.contains_key(section) {
            doc[section] = Item::Table(Table::new());
        }
    }
    let c = &cfg.client;
    let t = &mut doc["client"];
    set(t, "font_size", c.font_size as f64);
    set(t, "theme", c.theme.as_str());
    set(t, "stop_on_error", c.stop_on_error);
    set(t, "present_mode", c.present_mode.as_str());
    set(t, "timestamp_source", c.timestamp_source.as_str());
    set(t, "keyboard_device", c.keyboard_device.as_str());
    set(t, "live_wpm", c.live_wpm);
    set(&mut doc["engine"], "autostart", cfg.engine.autostart);
    Ok(doc.to_string())
}

fn find_config(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join("config.toml");
        if candidate.is_file() && dir.join("schema").is_dir() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repo_config() {
        let text = include_str!("../../config.toml");
        let cfg = Config::parse(text, PathBuf::from("/repo")).unwrap();
        assert_eq!(cfg.general.mode, "prose");
        assert_eq!(cfg.client.present_mode, PresentModePref::Mailbox);
        assert_eq!(cfg.client.timestamp_source, TimestampSourceKind::Winit);
        assert!(!cfg.client.stop_on_error);
        assert_eq!(cfg.db_path(), PathBuf::from("/repo/data/qwertea.db"));
        assert!(cfg.engine.autostart);
    }

    #[test]
    fn missing_sections_use_defaults() {
        let cfg = Config::parse("", PathBuf::from(".")).unwrap();
        assert_eq!(cfg.client.font_size, 28.0);
        assert_eq!(cfg.general.layout, "qwerty-us");
        assert_eq!(cfg.engine.command[0], "uv");
    }

    #[test]
    fn saving_keeps_comments_and_other_sections() {
        let text = include_str!("../../config.toml");
        let mut cfg = Config::parse(text, PathBuf::from(".")).unwrap();
        cfg.client.font_size = 32.0;
        cfg.client.theme = "light".into();
        cfg.client.stop_on_error = true;
        let out = update_toml(text, &cfg).unwrap();
        assert!(out.contains("# mailbox | immediate | fifo"));
        assert!(out.contains("weight_error = 0.6"));
        let back = Config::parse(&out, PathBuf::from(".")).unwrap();
        assert_eq!(back.client.font_size, 32.0);
        assert_eq!(back.client.theme, "light");
        assert!(back.client.stop_on_error);
    }
}
