//! `config.toml` loading. Only the sections the client needs are typed;
//! unknown sections (engine settings) are ignored.

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
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub client: Client,
    /// Directory containing `config.toml`; relative paths resolve against it.
    #[serde(skip)]
    pub root: PathBuf,
}

impl Config {
    pub fn parse(text: &str, root: PathBuf) -> Result<Self, toml::de::Error> {
        let mut cfg: Config = toml::from_str(text)?;
        cfg.root = root;
        Ok(cfg)
    }

    /// Load `config.toml` from `$QWERTEA_CONFIG`, or the first `config.toml`
    /// found walking up from the current directory. Falls back to defaults.
    pub fn load() -> Self {
        let path = std::env::var_os("QWERTEA_CONFIG")
            .map(PathBuf::from)
            .or_else(find_config);
        match path {
            Some(p) => {
                let root = p
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."));
                match std::fs::read_to_string(&p) {
                    Ok(text) => Self::parse(&text, root).unwrap_or_else(|e| {
                        eprintln!("qwertea: invalid {}: {e}; using defaults", p.display());
                        Self::default_at(PathBuf::from("."))
                    }),
                    Err(e) => {
                        eprintln!("qwertea: cannot read {}: {e}; using defaults", p.display());
                        Self::default_at(PathBuf::from("."))
                    }
                }
            }
            None => Self::default_at(PathBuf::from(".")),
        }
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

    /// `schema/001_init.sql` next to the config. The schema is also compiled
    /// into the binary, so this is only informational.
    pub fn schema_dir(&self) -> PathBuf {
        self.root.join("schema")
    }
}

fn find_config() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join("config.toml");
        if candidate.is_file() {
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
    }

    #[test]
    fn missing_sections_use_defaults() {
        let cfg = Config::parse("", PathBuf::from(".")).unwrap();
        assert_eq!(cfg.client.font_size, 28.0);
        assert_eq!(cfg.general.layout, "qwerty-us");
    }
}
