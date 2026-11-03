//! Keyboard geometry for the on-screen heatmap, read from the same data file
//! the engine uses (`engine/data/layouts/<name>.toml`).

use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Key {
    pub base: String,
    #[serde(default)]
    pub shifted: Option<String>,
    pub row: i32,
    pub x: f32,
    #[serde(default)]
    pub finger: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Layout {
    pub name: String,
    pub keys: Vec<Key>,
}

const BUILTIN: &str = include_str!("../../engine/data/layouts/qwerty-us.toml");

impl Layout {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_else(Self::builtin)
    }

    pub fn builtin() -> Self {
        toml::from_str(BUILTIN).expect("bundled layout parses")
    }

    /// Keys drawn on the heatmap: printable keys on rows 0..=4 (no Tab/Enter).
    pub fn visible_keys(&self) -> impl Iterator<Item = &Key> {
        self.keys
            .iter()
            .filter(|k| k.base != "\n" && k.base != "\t")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_layout_has_all_letters() {
        let l = Layout::builtin();
        for c in 'a'..='z' {
            assert!(l.keys.iter().any(|k| k.base == c.to_string()), "{c}");
        }
        let space = l.keys.iter().find(|k| k.base == " ").unwrap();
        assert_eq!(space.row, 4);
    }
}
