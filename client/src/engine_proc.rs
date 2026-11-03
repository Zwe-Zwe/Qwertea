//! Starts and supervises the analysis engine (`qwertea-engine serve`).
//!
//! The two programs still only talk through the database (section 2); the
//! client merely launches the engine so users run one program. If an engine
//! is already serving the same database (`data/engine.pid` names a live
//! process), the client leaves it alone.

use crate::config::Config;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineStatus {
    /// Started by this client and running.
    Running,
    /// Another engine process serves the database.
    External,
    /// Disabled in settings.
    Disabled,
    /// Could not be started or exited; the message says why.
    Failed(String),
}

impl EngineStatus {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Running | Self::External)
    }
    pub fn label(&self) -> String {
        match self {
            Self::Running => "engine running".into(),
            Self::External => "engine running (external)".into(),
            Self::Disabled => "engine autostart off".into(),
            Self::Failed(m) => format!("engine not running: {m}"),
        }
    }
}

pub struct EngineProc {
    child: Option<Child>,
    pub status: EngineStatus,
    log_path: PathBuf,
    pid_path: PathBuf,
    last_check: Instant,
    restarts: u32,
}

fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
            || Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

impl EngineProc {
    pub fn start(cfg: &Config) -> Self {
        let data = cfg.data_dir();
        let _ = std::fs::create_dir_all(&data);
        let mut me = Self {
            child: None,
            status: EngineStatus::Disabled,
            log_path: data.join("engine.log"),
            pid_path: data.join("engine.pid"),
            last_check: Instant::now(),
            restarts: 0,
        };
        if !cfg.engine.autostart {
            return me;
        }
        me.spawn(cfg);
        me
    }

    fn external_pid(&self) -> Option<u32> {
        let pid: u32 = std::fs::read_to_string(&self.pid_path)
            .ok()?
            .trim()
            .parse()
            .ok()?;
        let ours = self.child.as_ref().map(Child::id);
        (Some(pid) != ours && pid_alive(pid)).then_some(pid)
    }

    fn spawn(&mut self, cfg: &Config) {
        if self.external_pid().is_some() {
            self.status = EngineStatus::External;
            return;
        }
        let Some((prog, args)) = cfg.engine.command.split_first() else {
            self.status = EngineStatus::Failed("empty engine command".into());
            return;
        };
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path);
        let mut cmd = Command::new(prog);
        cmd.args(args)
            .current_dir(&cfg.root)
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        if let Some(path) = &cfg.path {
            cmd.env("QWERTEA_CONFIG", path);
        }
        // The engine exits by itself if this app dies without stopping it.
        cmd.env("QWERTEA_PARENT_PID", std::process::id().to_string());
        match log {
            Ok(f) => cmd.stderr(f),
            Err(_) => cmd.stderr(Stdio::null()),
        };
        match cmd.spawn() {
            Ok(child) => {
                self.child = Some(child);
                self.status = EngineStatus::Running;
            }
            Err(e) => {
                self.status = EngineStatus::Failed(format!(
                    "cannot run `{}` ({e}). Install uv and run `make setup`, or start the engine yourself.",
                    cfg.engine.command.join(" ")
                ));
            }
        }
    }

    /// Call periodically (cheap; checks at most once a second).
    pub fn poll(&mut self, cfg: &Config) {
        if self.last_check.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_check = Instant::now();
        if let Some(child) = &mut self.child {
            if let Ok(Some(status)) = child.try_wait() {
                self.child = None;
                let tail = std::fs::read_to_string(&self.log_path)
                    .ok()
                    .and_then(|t| {
                        t.lines()
                            .rev()
                            .find(|l| !l.trim().is_empty())
                            .map(String::from)
                    })
                    .unwrap_or_default();
                self.status = EngineStatus::Failed(format!("exited ({status}). {tail}"));
                if self.restarts < 3 && cfg.engine.autostart {
                    self.restarts += 1;
                    self.spawn(cfg);
                }
            }
        } else if self.status == EngineStatus::External && self.external_pid().is_none() {
            self.spawn(cfg);
        }
    }

    pub fn log_path(&self) -> &PathBuf {
        &self.log_path
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            #[cfg(unix)]
            {
                // SIGINT first so the engine removes its pid file.
                let _ = Command::new("kill")
                    .args(["-INT", &child.id().to_string()])
                    .status();
                let deadline = Instant::now() + Duration::from_millis(1500);
                while Instant::now() < deadline {
                    if let Ok(Some(_)) = child.try_wait() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for EngineProc {
    fn drop(&mut self) {
        self.stop();
    }
}
