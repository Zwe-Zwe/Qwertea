//! Keystroke timestamping.
//!
//! The primary stamp is taken on the first line of `window_event`, before the
//! event reaches egui or any other logic (`WinitReceiptStamp`). On Linux with
//! the `evdev-capture` feature, a dedicated thread additionally records kernel
//! timestamps (`CLOCK_MONOTONIC`) straight from `/dev/input/event*`; these are
//! correlated with the UI events *after* the lesson ends, so the key handler
//! never does any matching work (see `docs/decisions.md`, D3).

use std::cell::RefCell;
use std::time::Instant;

/// What the key handler knows about a key event at receipt time.
#[derive(Debug, Clone, Copy)]
pub struct RawKeyInfo {
    /// `Instant::now()` taken at the top of `window_event`.
    pub received_at: Instant,
    /// Physical scancode (on Linux: the evdev key code).
    pub scancode: u32,
    pub is_down: bool,
}

pub trait TimestampSource {
    /// Monotonic nanoseconds since lesson start for a key event.
    ///
    /// Returns `None` when the source has no observation for this event
    /// (only possible for kernel sources; see `docs/decisions.md`, D2).
    fn stamp(&self, ev: &RawKeyInfo) -> Option<u64>;
}

/// Default source: the receipt time of the winit event. Works everywhere.
#[derive(Debug, Clone, Copy)]
pub struct WinitReceiptStamp {
    pub origin: Instant,
}

impl WinitReceiptStamp {
    pub fn new(origin: Instant) -> Self {
        Self { origin }
    }
}

impl TimestampSource for WinitReceiptStamp {
    #[inline]
    fn stamp(&self, ev: &RawKeyInfo) -> Option<u64> {
        Some(
            ev.received_at
                .saturating_duration_since(self.origin)
                .as_nanos() as u64,
        )
    }
}

/// A key event observed by the kernel, in nanoseconds of `CLOCK_MONOTONIC`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KernelEvent {
    pub scancode: u32,
    pub is_down: bool,
    pub mono_ns: u64,
}

/// Kernel-timestamp source over the kernel events recorded during a lesson.
///
/// Each kernel event is used at most once; events are matched in order per
/// `(scancode, is_down)`. A kernel event must precede the winit receipt
/// (with a small tolerance for clock-read jitter) and be no older than
/// `MAX_LAG_NS`, otherwise the UI event is left unmatched.
pub struct EvdevStamp {
    origin: Instant,
    origin_mono_ns: u64,
    kernel: Vec<KernelEvent>,
    used: RefCell<Vec<bool>>,
}

impl EvdevStamp {
    pub const MAX_LAG_NS: u64 = 500_000_000;
    pub const LEAD_TOLERANCE_NS: u64 = 2_000_000;

    /// `origin` and `origin_mono_ns` must be read back-to-back at lesson start.
    pub fn new(origin: Instant, origin_mono_ns: u64, kernel: Vec<KernelEvent>) -> Self {
        let used = RefCell::new(vec![false; kernel.len()]);
        Self {
            origin,
            origin_mono_ns,
            kernel,
            used,
        }
    }
}

impl TimestampSource for EvdevStamp {
    fn stamp(&self, ev: &RawKeyInfo) -> Option<u64> {
        let winit_ns = ev
            .received_at
            .saturating_duration_since(self.origin)
            .as_nanos() as u64;
        let winit_mono = self.origin_mono_ns + winit_ns;
        let mut used = self.used.borrow_mut();
        for (i, k) in self.kernel.iter().enumerate() {
            if used[i] || k.scancode != ev.scancode || k.is_down != ev.is_down {
                continue;
            }
            if k.mono_ns > winit_mono + Self::LEAD_TOLERANCE_NS {
                return None; // kernel events are ordered; nothing later can match
            }
            if winit_mono.saturating_sub(k.mono_ns) > Self::MAX_LAG_NS {
                used[i] = true; // stale (e.g. key pressed while unfocused)
                continue;
            }
            used[i] = true;
            return Some(k.mono_ns.saturating_sub(self.origin_mono_ns));
        }
        None
    }
}

/// Translate a winit key event into what `LessonRun` needs. Cheap, no allocation.
///
/// * `produced_char` comes from the OS/layout (`event.text`), so shifted
///   forms, AltGr and completed dead-key sequences are handled by the OS.
/// * Enter maps to '\n' and Tab to '\t' (multi-line text support).
/// * Dead keys and other keys without text are `Other`: logged, no cursor move.
/// * Text produced together with Ctrl/Alt/Super is not typing; it is `Other`.
pub fn classify(
    event: &winit::event::KeyEvent,
    mods: winit::keyboard::ModifiersState,
) -> crate::session::KeyInput {
    use crate::session::{KeyInput, KeyKind};
    use winit::keyboard::{Key, NamedKey};
    use winit::platform::scancode::PhysicalKeyExtScancode;

    let is_down = event.state == winit::event::ElementState::Pressed;
    let scancode = event.physical_key.to_scancode().unwrap_or(0);
    let kind = match &event.logical_key {
        Key::Named(NamedKey::Backspace) => KeyKind::Backspace,
        Key::Named(NamedKey::Escape) => KeyKind::Escape,
        Key::Named(NamedKey::Enter) => KeyKind::Text('\n'),
        Key::Named(NamedKey::Tab) => KeyKind::Text('\t'),
        Key::Named(NamedKey::Space) => KeyKind::Text(' '),
        _ if mods.control_key() || mods.super_key() => KeyKind::Other,
        _ => match event.text.as_ref().and_then(|t| t.chars().next()) {
            Some(c) if !c.is_control() => KeyKind::Text(c),
            _ => match &event.logical_key {
                Key::Character(s) => match s.chars().next() {
                    Some(c) if !c.is_control() => KeyKind::Text(c),
                    _ => KeyKind::Other,
                },
                _ => KeyKind::Other,
            },
        },
    };
    KeyInput {
        scancode,
        is_down,
        kind,
    }
}

/// `CLOCK_MONOTONIC` now, in nanoseconds (the clock `Instant` uses on Linux).
#[cfg(all(target_os = "linux", feature = "evdev-capture"))]
pub fn monotonic_now_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: valid pointer to a timespec; CLOCK_MONOTONIC is always supported.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

#[cfg(all(target_os = "linux", feature = "evdev-capture"))]
pub mod evdev_reader {
    //! Background thread reading kernel key events from one evdev device.

    use super::KernelEvent;
    use std::os::fd::AsRawFd;
    use std::sync::{Arc, Mutex};
    use std::time::UNIX_EPOCH;

    /// `_IOW('E', 0xa0, int)`: select the clock used for event timestamps.
    const EVIOCSCLOCKID: libc::c_ulong = 0x4004_45a0;

    /// Shared buffer filled by the reader thread. The UI thread only takes
    /// it between lessons, never in the key handler.
    #[derive(Clone, Default)]
    pub struct KernelLog(Arc<Mutex<Vec<KernelEvent>>>);

    impl KernelLog {
        /// Take every event recorded so far, leaving the buffer empty.
        pub fn take(&self) -> Vec<KernelEvent> {
            let mut g = self.0.lock().unwrap_or_else(|p| p.into_inner());
            std::mem::take(&mut *g)
        }

        /// Discard everything recorded so far (call at lesson start).
        pub fn clear(&self) {
            self.0.lock().unwrap_or_else(|p| p.into_inner()).clear();
        }
    }

    /// Open `path` (or the first device that looks like a keyboard when
    /// `path` is empty), switch it to `CLOCK_MONOTONIC` timestamps and start
    /// the reader thread. Errors (typically EACCES when the user is not in
    /// the `input` group) are returned so the caller can fall back to winit.
    pub fn spawn(path: &str) -> std::io::Result<(KernelLog, String)> {
        let (mut dev, name) = if path.is_empty() {
            find_keyboard()?
        } else {
            let d = evdev::Device::open(path)?;
            (d, path.to_string())
        };
        let mut clock: libc::c_int = libc::CLOCK_MONOTONIC;
        // SAFETY: valid fd and pointer to an int, per the evdev ioctl ABI.
        let rc = unsafe { libc::ioctl(dev.as_raw_fd(), EVIOCSCLOCKID, &mut clock) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let log = KernelLog::default();
        let sink = log.clone();
        std::thread::Builder::new()
            .name("qwertea-evdev".into())
            .spawn(move || loop {
                let Ok(events) = dev.fetch_events() else {
                    return;
                };
                let batch: Vec<KernelEvent> = events
                    .filter(|e| e.event_type() == evdev::EventType::KEY)
                    .map(|e| KernelEvent {
                        scancode: e.code() as u32,
                        is_down: e.value() != 0, // 1 = press, 2 = autorepeat
                        mono_ns: e
                            .timestamp()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_nanos() as u64)
                            .unwrap_or(0),
                    })
                    .collect();
                if !batch.is_empty() {
                    sink.0
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .extend(batch);
                }
            })?;
        Ok((log, name))
    }

    fn find_keyboard() -> std::io::Result<(evdev::Device, String)> {
        for (path, dev) in evdev::enumerate() {
            let has_letters = dev.supported_keys().is_some_and(|k| {
                k.contains(evdev::KeyCode::KEY_A) && k.contains(evdev::KeyCode::KEY_Z)
            });
            if has_letters {
                let p = path.display().to_string();
                return Ok((evdev::Device::open(&path)?, p));
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no readable keyboard under /dev/input (is the user in the `input` group?)",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn info(origin: Instant, ms: u64, scancode: u32, is_down: bool) -> RawKeyInfo {
        RawKeyInfo {
            received_at: origin + Duration::from_millis(ms),
            scancode,
            is_down,
        }
    }

    #[test]
    fn winit_stamp_is_ns_since_origin() {
        let origin = Instant::now();
        let s = WinitReceiptStamp::new(origin);
        assert_eq!(s.stamp(&info(origin, 12, 30, true)), Some(12_000_000));
    }

    #[test]
    fn evdev_matches_in_order_per_key_and_direction() {
        let origin = Instant::now();
        let base = 1_000_000_000_000u64; // mono ns at origin
        let k = |ms: u64, sc, d| KernelEvent {
            scancode: sc,
            is_down: d,
            mono_ns: base + ms * 1_000_000,
        };
        let kernel = vec![
            k(9, 30, true),
            k(19, 30, false),
            k(29, 30, true),
            k(30, 31, true),
        ];
        let s = EvdevStamp::new(origin, base, kernel);
        assert_eq!(s.stamp(&info(origin, 10, 30, true)), Some(9_000_000));
        assert_eq!(s.stamp(&info(origin, 20, 30, false)), Some(19_000_000));
        assert_eq!(s.stamp(&info(origin, 31, 31, true)), Some(30_000_000));
        assert_eq!(s.stamp(&info(origin, 30, 30, true)), Some(29_000_000));
        // Every kernel event is used once.
        assert_eq!(s.stamp(&info(origin, 40, 30, true)), None);
    }

    #[test]
    fn evdev_rejects_kernel_events_after_receipt_and_stale_ones() {
        let origin = Instant::now();
        let base = 5_000_000_000u64;
        let kernel = vec![
            KernelEvent {
                scancode: 1,
                is_down: true,
                mono_ns: base + 100_000_000,
            },
            KernelEvent {
                scancode: 2,
                is_down: true,
                mono_ns: base + 200_000_000,
            },
        ];
        let s = EvdevStamp::new(origin, base, kernel);
        // Kernel event 100 ms *after* the UI saw it: impossible, unmatched.
        assert_eq!(s.stamp(&info(origin, 0, 1, true)), None);
        // Kernel event 900 ms before receipt: stale, unmatched.
        assert_eq!(s.stamp(&info(origin, 1_100, 2, true)), None);
    }
}
