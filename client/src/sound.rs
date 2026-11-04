//! Optional key sounds (off by default; a profile setting), in several
//! styles with a volume control.
//!
//! The key handler only bumps an atomic counter; the audio callback notices
//! the change and synthesises the sound there (decision D30). Nothing on the hot path
//! allocates, locks or talks to the sound system, so turning sounds on does
//! not change how keystrokes are timed.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// The sound a key makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundStyle {
    Off,
    Click,
    Typewriter,
    Thock,
    Pop,
    Blip,
}

impl SoundStyle {
    pub const ALL: &'static [SoundStyle] = &[
        Self::Off,
        Self::Click,
        Self::Typewriter,
        Self::Thock,
        Self::Pop,
        Self::Blip,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Click => "click",
            Self::Typewriter => "typewriter",
            Self::Thock => "thock",
            Self::Pop => "pop",
            Self::Blip => "blip",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Click => "Soft click",
            Self::Typewriter => "Typewriter",
            Self::Thock => "Deep thock",
            Self::Pop => "Bubble pop",
            Self::Blip => "Retro blip",
        }
    }
    /// Also accepts 1.0.2 previews' `true` / `false`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "true" | "on" => Some(Self::Click),
            "false" => Some(Self::Off),
            _ => Self::ALL.iter().copied().find(|v| v.key() == s),
        }
    }
    fn index(self) -> u32 {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0) as u32
    }
    fn from_index(i: u32) -> Self {
        Self::ALL.get(i as usize).copied().unwrap_or(Self::Click)
    }
}

struct Shared {
    clicks: AtomicU32,
    errors: AtomicU32,
    style: AtomicU32,
    /// Volume in percent (0–100).
    volume: AtomicU32,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            clicks: AtomicU32::new(0),
            errors: AtomicU32::new(0),
            style: AtomicU32::new(SoundStyle::Click.index()),
            volume: AtomicU32::new(70),
        }
    }
}

pub struct KeySound {
    shared: Arc<Shared>,
    #[cfg(feature = "sound")]
    stream: Option<cpal::Stream>,
    /// Why sound is unavailable, if it is.
    pub error: Option<String>,
}

impl Default for KeySound {
    fn default() -> Self {
        Self::off()
    }
}

impl KeySound {
    pub fn off() -> Self {
        Self {
            shared: Arc::default(),
            #[cfg(feature = "sound")]
            stream: None,
            error: None,
        }
    }

    pub fn is_on(&self) -> bool {
        #[cfg(feature = "sound")]
        {
            self.stream.is_some()
        }
        #[cfg(not(feature = "sound"))]
        {
            false
        }
    }

    /// Open the default output device (slow; never call while typing).
    pub fn enable(&mut self) {
        if self.is_on() {
            return;
        }
        #[cfg(feature = "sound")]
        match open_stream(self.shared.clone()) {
            Ok(s) => {
                self.stream = Some(s);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        #[cfg(not(feature = "sound"))]
        {
            self.error = Some("this build has no sound support".into());
        }
    }

    pub fn disable(&mut self) {
        #[cfg(feature = "sound")]
        {
            self.stream = None;
        }
    }

    /// Sound style and volume (0–100) for the next keys.
    pub fn configure(&self, style: SoundStyle, volume: u32) {
        self.shared.style.store(style.index(), Ordering::Relaxed);
        self.shared.volume.store(volume.min(100), Ordering::Relaxed);
    }

    /// HOT PATH: request a click. `error` gives a lower, duller one.
    #[inline]
    pub fn click(&self, error: bool) {
        if error {
            self.shared.errors.fetch_add(1, Ordering::Relaxed);
        } else {
            self.shared.clicks.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Synthesiser state, owned by the audio callback.
#[cfg_attr(not(feature = "sound"), allow(dead_code))]
struct Voice {
    rate: f32,
    seen_clicks: u32,
    seen_errors: u32,
    /// Seconds since the current sound started (`None`: silent).
    t: Option<f32>,
    style: SoundStyle,
    error: bool,
    gain: f32,
    phase: f32,
    phase2: f32,
    noise: u32,
}

#[cfg_attr(not(feature = "sound"), allow(dead_code))]
impl Voice {
    fn new(rate: f32) -> Self {
        Self {
            rate,
            seen_clicks: 0,
            seen_errors: 0,
            t: None,
            style: SoundStyle::Click,
            error: false,
            gain: 0.7,
            phase: 0.0,
            phase2: 0.0,
            noise: 0x9e37_79b9,
        }
    }

    /// Length of one sound, in seconds.
    fn len(style: SoundStyle) -> f32 {
        match style {
            SoundStyle::Off => 0.0,
            SoundStyle::Click => 0.035,
            SoundStyle::Typewriter => 0.06,
            SoundStyle::Thock => 0.07,
            SoundStyle::Pop => 0.045,
            SoundStyle::Blip => 0.04,
        }
    }

    fn poll(&mut self, shared: &Shared) {
        let c = shared.clicks.load(Ordering::Relaxed);
        let e = shared.errors.load(Ordering::Relaxed);
        let start = if e != self.seen_errors {
            Some(true)
        } else if c != self.seen_clicks {
            Some(false)
        } else {
            None
        };
        self.seen_errors = e;
        self.seen_clicks = c;
        if let Some(error) = start {
            self.style = SoundStyle::from_index(shared.style.load(Ordering::Relaxed));
            self.gain = shared.volume.load(Ordering::Relaxed).min(100) as f32 / 100.0;
            self.error = error;
            self.t = Some(0.0);
            self.phase = 0.0;
            self.phase2 = 0.0;
        }
    }

    fn noise(&mut self) -> f32 {
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        (self.noise as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// Advance an oscillator phase by `freq` Hz and return its sine.
    fn osc(phase: &mut f32, freq: f32, rate: f32) -> f32 {
        *phase = (*phase + std::f32::consts::TAU * freq / rate) % std::f32::consts::TAU;
        phase.sin()
    }

    fn next(&mut self) -> f32 {
        let Some(t) = self.t else { return 0.0 };
        if t > Self::len(self.style) {
            self.t = None;
            return 0.0;
        }
        self.t = Some(t + 1.0 / self.rate);
        let (rate, err) = (self.rate, self.error);
        let n = self.noise();
        let v = match self.style {
            SoundStyle::Off => 0.0,
            // A light tick: short tone with a burst of noise.
            SoundStyle::Click => {
                let f = if err { 520.0 } else { 1900.0 };
                let tone = Self::osc(&mut self.phase, f, rate);
                0.28 * (-t / 0.006).exp() * (0.65 * tone + 0.35 * n * (-t / 0.002).exp())
            }
            // A sharp strike: metallic partials over a noisy hit and a low body.
            SoundStyle::Typewriter => {
                let (f1, f2) = if err {
                    (1250.0, 1870.0)
                } else {
                    (3100.0, 4730.0)
                };
                let ring = Self::osc(&mut self.phase, f1, rate)
                    + 0.6 * Self::osc(&mut self.phase2, f2, rate);
                let body = (std::f32::consts::TAU * 140.0 * t).sin();
                0.30 * (0.55 * n * (-t / 0.003).exp()
                    + 0.18 * ring * (-t / 0.009).exp()
                    + 0.30 * body * (-t / 0.015).exp())
            }
            // A deep, rounded thud whose pitch falls (mechanical keyboard).
            SoundStyle::Thock => {
                let base = if err { 120.0 } else { 190.0 };
                let f = base * (1.0 + 0.6 * (-t / 0.01).exp());
                let tone = Self::osc(&mut self.phase, f, rate);
                0.27 * (0.85 * tone * (-t / 0.016).exp() + 0.3 * n * (-t / 0.0015).exp())
            }
            // A bubble: a sine sweeping up (down for a mistake).
            SoundStyle::Pop => {
                let k = (t / 0.025).min(1.0);
                let f = if err {
                    900.0 - 500.0 * k
                } else {
                    500.0 + 800.0 * k
                };
                let tone = Self::osc(&mut self.phase, f, rate);
                let attack = (t / 0.002).min(1.0);
                0.30 * tone * attack * (-t / 0.012).exp()
            }
            // An 8-bit beep: a soft square wave.
            SoundStyle::Blip => {
                let f = if err { 330.0 } else { 1320.0 };
                let sq = Self::osc(&mut self.phase, f, rate).signum();
                0.17 * sq
                    * if t < 0.025 {
                        1.0
                    } else {
                        (-(t - 0.025) / 0.004).exp()
                    }
            }
        };
        v * self.gain
    }
}

#[cfg(feature = "sound")]
fn open_stream(shared: Arc<Shared>) -> Result<cpal::Stream, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no sound output device".to_string())?;
    let supported = device
        .default_output_config()
        .map_err(|e| format!("sound output: {e}"))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, shared),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, shared),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, shared),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, shared),
        other => return Err(format!("unsupported sample format {other}")),
    }?;
    stream.play().map_err(|e| format!("sound output: {e}"))?;
    Ok(stream)
}

#[cfg(feature = "sound")]
fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    shared: Arc<Shared>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    use cpal::traits::DeviceTrait;
    let channels = config.channels.max(1) as usize;
    let mut voice = Voice::new(config.sample_rate.0 as f32);
    // Pick up the counters' current values so nothing plays at start.
    voice.poll(&shared);
    voice.t = None;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                voice.poll(&shared);
                for frame in data.chunks_mut(channels) {
                    let v = T::from_sample(voice.next());
                    for s in frame {
                        *s = v;
                    }
                }
            },
            |e| eprintln!("qwertea: sound: {e}"),
            None,
        )
        .map_err(|e| format!("sound output: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_is_short_bounded_and_audible() {
        for &style in SoundStyle::ALL {
            for error in [false, true] {
                let shared = Shared::default();
                shared.style.store(style.index(), Ordering::Relaxed);
                shared.volume.store(100, Ordering::Relaxed);
                let mut v = Voice::new(48_000.0);
                v.poll(&shared);
                assert_eq!(v.next(), 0.0, "silent until a key");
                let counter = if error {
                    &shared.errors
                } else {
                    &shared.clicks
                };
                counter.fetch_add(1, Ordering::Relaxed);
                v.poll(&shared);
                let samples: Vec<f32> = (0..9600).map(|_| v.next()).collect();
                assert!(samples.iter().all(|s| s.abs() <= 0.4), "{style:?} too loud");
                let audible = samples[..1000].iter().any(|s| s.abs() > 0.01);
                assert_eq!(audible, style != SoundStyle::Off, "{style:?}");
                assert!(
                    samples[4800..].iter().all(|s| *s == 0.0),
                    "{style:?} ends within 100 ms"
                );
            }
        }
        assert_eq!(SoundStyle::parse("true"), Some(SoundStyle::Click));
        assert_eq!(SoundStyle::parse("thock"), Some(SoundStyle::Thock));
        assert_eq!(SoundStyle::parse("nope"), None);
        let s = KeySound::off();
        s.click(false);
        s.click(true);
        assert_eq!(s.shared.clicks.load(Ordering::Relaxed), 1);
        assert_eq!(s.shared.errors.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn volume_scales_the_sound() {
        let loudest = |vol: u32| {
            let shared = Shared::default();
            shared.volume.store(vol, Ordering::Relaxed);
            let mut v = Voice::new(48_000.0);
            v.poll(&shared);
            shared.clicks.fetch_add(1, Ordering::Relaxed);
            v.poll(&shared);
            (0..2000).map(|_| v.next().abs()).fold(0.0, f32::max)
        };
        assert!(loudest(100) > 1.9 * loudest(50));
        assert_eq!(loudest(0), 0.0);
    }

    /// Listen to the styles: `QWERTEA_SOUND_DIR=/tmp/q cargo test --lib
    /// sound -- --ignored` writes one WAV per style (a short phrase with a
    /// mistake in it).
    #[test]
    #[ignore]
    fn write_samples() {
        let Some(dir) = std::env::var_os("QWERTEA_SOUND_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rate = 44_100u32;
        for &style in &SoundStyle::ALL[1..] {
            let shared = Shared::default();
            shared.style.store(style.index(), Ordering::Relaxed);
            let mut v = Voice::new(rate as f32);
            v.poll(&shared);
            let mut pcm: Vec<i16> = Vec::new();
            // Twelve keys at ~95 wpm with a mistake as the eighth.
            for k in 0..12 {
                let c = if k == 7 {
                    &shared.errors
                } else {
                    &shared.clicks
                };
                c.fetch_add(1, Ordering::Relaxed);
                v.poll(&shared);
                let gap = if k % 4 == 3 { 0.16 } else { 0.11 };
                for _ in 0..(gap * rate as f32) as usize {
                    pcm.push((v.next() * 32_000.0) as i16);
                }
            }
            let mut wav = Vec::new();
            let data_len = (pcm.len() * 2) as u32;
            wav.extend_from_slice(b"RIFF");
            wav.extend_from_slice(&(36 + data_len).to_le_bytes());
            wav.extend_from_slice(b"WAVEfmt ");
            wav.extend_from_slice(&16u32.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes());
            wav.extend_from_slice(&rate.to_le_bytes());
            wav.extend_from_slice(&(rate * 2).to_le_bytes());
            wav.extend_from_slice(&2u16.to_le_bytes());
            wav.extend_from_slice(&16u16.to_le_bytes());
            wav.extend_from_slice(b"data");
            wav.extend_from_slice(&data_len.to_le_bytes());
            for s in pcm {
                wav.extend_from_slice(&s.to_le_bytes());
            }
            std::fs::write(dir.join(format!("qwertea-{}.wav", style.key())), wav).unwrap();
        }
    }
}
