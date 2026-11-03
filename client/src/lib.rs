//! Qwertea client library: keystroke capture, the lesson state machine,
//! database access and rendering plumbing. Binaries: `qwertea` (the app),
//! `timing_check` (M0 timing spike) and `roundtrip_fixture` (M1 test helper).

pub mod capture;
pub mod config;
pub mod db;
pub mod gfx;
pub mod session;
pub mod timing;
