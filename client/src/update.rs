//! In-app updates (decision D29).
//!
//! At start-up (unless `[updates] check = false`) a background thread asks
//! GitHub for the repository's release tags and compares the newest with this
//! build's version. What "Update" does depends on how Qwertea was installed:
//!
//! * **Source checkout** (the app runs from `<repo>/client/target/`): the app
//!   updates itself — `git pull --ff-only`, `cargo build --release`, `uv sync`
//!   — then offers to restart. No terminal needed.
//! * **Arch package**: packages are updated by pacman, which needs your
//!   password, so the app shows the one command to run (with a Copy button)
//!   and a link to the release notes.
//! * **Anything else**: a link to the release page.
//!
//! Nothing is sent except the request for the public tag list; nothing runs
//! without you pressing a button.

use crate::config::Config;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// Running from a git checkout; `root` holds `client/` and `engine/`.
    Source {
        root: PathBuf,
    },
    /// Installed by pacman (`/usr/bin/qwertea`).
    ArchPackage,
    Other,
}

impl InstallKind {
    pub fn detect(cfg: &Config) -> Self {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|e| e.canonicalize().ok());
        let root = cfg.root.canonicalize().unwrap_or_else(|_| cfg.root.clone());
        match exe {
            Some(e) if root.join(".git").exists() && e.starts_with(&root) => Self::Source { root },
            Some(e) if e.starts_with("/usr") && Path::new("/usr/bin/pacman").exists() => {
                Self::ArchPackage
            }
            _ => Self::Other,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Source { .. } => "source checkout",
            Self::ArchPackage => "Arch package",
            Self::Other => "standalone",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Idle,
    Checking,
    UpToDate,
    Available {
        version: String,
    },
    CheckFailed(String),
    /// Updating a source checkout; `step` says what is running.
    Installing {
        version: String,
        step: String,
    },
    /// Built and ready; restart to use it.
    Installed {
        version: String,
    },
    InstallFailed(String),
}

pub struct Updater {
    state: Arc<Mutex<State>>,
    pub kind: InstallKind,
    pub repo: String,
}

/// `v1.2.3` / `1.2.3` → (1, 2, 3). Pre-release tags (`v1.3.0-rc1`) are ignored.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches('v');
    let mut it = s.split('.');
    let v = (
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
    );
    it.next().is_none().then_some(v)
}

pub fn current_version() -> (u32, u32, u32) {
    parse_version(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0))
}

/// The newest release tag in a GitHub `tags` API response.
pub fn newest_tag(json: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Tag {
        name: String,
    }
    let tags: Vec<Tag> = serde_json::from_str(json).ok()?;
    tags.into_iter()
        .filter_map(|t| parse_version(&t.name).map(|v| (v, t.name)))
        .max_by_key(|(v, _)| *v)
        .map(|(_, n)| n)
}

fn fmt_version(v: (u32, u32, u32)) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

/// Find a build tool even when the app was started from a desktop launcher
/// whose PATH lacks `~/.cargo/bin` or `~/.local/bin`.
fn tool(prog: &str) -> PathBuf {
    let in_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(prog).is_file()))
        .unwrap_or(false);
    if !in_path {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            for dir in [".cargo/bin", ".local/bin"] {
                let p = home.join(dir).join(prog);
                if p.is_file() {
                    return p;
                }
            }
        }
    }
    PathBuf::from(prog)
}

fn run(dir: &Path, prog: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(tool(prog))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {prog}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let last = err
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string();
    Err(format!("`{prog} {}` failed: {last}", args.join(" ")))
}

impl Updater {
    pub fn new(cfg: &Config) -> Self {
        Self {
            state: Arc::new(Mutex::new(State::Idle)),
            kind: InstallKind::detect(cfg),
            repo: cfg.updates.repo.clone(),
        }
    }

    pub fn state(&self) -> State {
        self.state.lock().map(|s| s.clone()).unwrap_or(State::Idle)
    }

    fn set(&self, s: State) {
        if let Ok(mut g) = self.state.lock() {
            *g = s;
        }
    }

    pub fn busy(&self) -> bool {
        matches!(self.state(), State::Checking | State::Installing { .. })
    }

    pub fn release_url(&self, version: &str) -> String {
        format!("https://github.com/{}/releases/tag/v{version}", self.repo)
    }

    /// The command that updates this kind of install, for the user to run.
    pub fn manual_command(&self) -> Option<&'static str> {
        match self.kind {
            InstallKind::ArchPackage => Some("yay -Syu qwertea"),
            _ => None,
        }
    }

    /// Ask GitHub for the newest tag (in the background).
    pub fn check(&self) {
        if self.busy() {
            return;
        }
        self.set(State::Checking);
        let state = self.state.clone();
        let url = format!(
            "https://api.github.com/repos/{}/tags?per_page=100",
            self.repo
        );
        std::thread::spawn(move || {
            let res = Command::new("curl")
                .args([
                    "-fsSL",
                    "--max-time",
                    "15",
                    "-H",
                    "Accept: application/vnd.github+json",
                    "-A",
                    "qwertea-update-check",
                    &url,
                ])
                .stdin(Stdio::null())
                .output();
            let new = match res {
                Err(e) => State::CheckFailed(format!("cannot run curl: {e}")),
                Ok(o) if !o.status.success() => {
                    State::CheckFailed("could not reach GitHub (offline?)".to_string())
                }
                Ok(o) => match newest_tag(&String::from_utf8_lossy(&o.stdout)) {
                    None => State::CheckFailed("no releases found".into()),
                    Some(tag) => match parse_version(&tag) {
                        Some(v) if v > current_version() => State::Available {
                            version: fmt_version(v),
                        },
                        _ => State::UpToDate,
                    },
                },
            };
            if let Ok(mut g) = state.lock() {
                *g = new;
            }
        });
    }

    /// Update a source checkout in place (in the background).
    pub fn install(&self) {
        let InstallKind::Source { root } = self.kind.clone() else {
            return;
        };
        let version = match self.state() {
            State::Available { version } => version,
            _ => return,
        };
        self.set(State::Installing {
            version: version.clone(),
            step: "Downloading the new version".into(),
        });
        let state = self.state.clone();
        std::thread::spawn(move || {
            let step = |s: &str| {
                if let Ok(mut g) = state.lock() {
                    *g = State::Installing {
                        version: version.clone(),
                        step: s.to_string(),
                    };
                }
            };
            let steps: [(&str, &Path, &str, &[&str]); 3] = [
                (
                    "Downloading the new version",
                    &root,
                    "git",
                    &["pull", "--ff-only"],
                ),
                (
                    "Building the app (a few minutes)",
                    &root.join("client"),
                    "cargo",
                    &["build", "--release", "--bin", "qwertea"],
                ),
                (
                    "Updating the engine",
                    &root,
                    "uv",
                    &["--project", "engine", "sync", "--all-extras"],
                ),
            ];
            for (label, dir, prog, args) in steps {
                step(label);
                if let Err(e) = run(dir, prog, args) {
                    if let Ok(mut g) = state.lock() {
                        *g = State::InstallFailed(e);
                    }
                    return;
                }
            }
            // Report the version that was actually built.
            let built = std::fs::read_to_string(root.join("client/Cargo.toml"))
                .ok()
                .and_then(|t| {
                    t.lines()
                        .find_map(|l| l.strip_prefix("version = "))
                        .map(|v| v.trim_matches('"').to_string())
                })
                .unwrap_or_else(|| version.clone());
            if let Ok(mut g) = state.lock() {
                *g = State::Installed { version: built };
            }
        });
    }

    /// The program to start for "Restart": the freshly built binary for a
    /// source checkout, otherwise this one.
    pub fn restart_target(&self) -> Option<PathBuf> {
        match &self.kind {
            InstallKind::Source { root } => {
                let exe = root.join("client/target/release/qwertea");
                exe.is_file().then_some(exe)
            }
            _ => std::env::current_exe().ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("v1.0.10"), Some((1, 0, 10)));
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("v1.3.0-rc1"), None);
        assert_eq!(parse_version("nightly"), None);
        assert!(parse_version("v1.0.10") > parse_version("v1.0.9"));
        let json =
            r#"[{"name":"v1.0.9"},{"name":"v1.0.10"},{"name":"v2.0.0-rc1"},{"name":"junk"}]"#;
        assert_eq!(newest_tag(json).as_deref(), Some("v1.0.10"));
        assert_eq!(newest_tag("not json"), None);
    }
}
