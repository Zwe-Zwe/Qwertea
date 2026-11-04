//! Qwertea client library: keystroke capture, the lesson state machine,
//! database access, the app's screens and rendering plumbing.
//! Binaries: `qwertea` (the app), `timing_check` (M0 timing spike) and
//! `roundtrip_fixture` (M1 test helper).

pub mod app;
pub mod capture;
pub mod config;
pub mod db;
pub mod engine_proc;
pub mod gfx;
pub mod layout;
pub mod profile;
pub mod session;
pub mod stats;
pub mod timing;
pub mod ui;
