//! Optional key sounds (off by default; a profile setting).
//!
//! The key handler only bumps an atomic counter; the audio callback notices
//! the change and synthesises a short click there. Nothing on the hot path
//! allocates, locks or talks to the sound system, so turning sounds on does
//! not change how keystrokes are timed.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

#[derive(Default)]
struct Shared {
    clicks: AtomicU32,
    errors: AtomicU32,
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

/// Click synthesiser state, owned by the audio callback.
#[cfg_attr(not(feature = "sound"), allow(dead_code))]
struct Voice {
    rate: f32,
    seen_clicks: u32,
    seen_errors: u32,
    /// Seconds since the current click started (`None`: silent).
    t: Option<f32>,
    freq: f32,
    noise: u32,
}

#[cfg_attr(not(feature = "sound"), allow(dead_code))]
impl Voice {
    const LEN: f32 = 0.035;

    fn new(rate: f32) -> Self {
        Self {
            rate,
            seen_clicks: 0,
            seen_errors: 0,
            t: None,
            freq: 1900.0,
            noise: 0x9e37_79b9,
        }
    }

    fn poll(&mut self, shared: &Shared) {
        let c = shared.clicks.load(Ordering::Relaxed);
        let e = shared.errors.load(Ordering::Relaxed);
        if e != self.seen_errors {
            self.seen_errors = e;
            self.seen_clicks = c;
            self.t = Some(0.0);
            self.freq = 520.0;
        } else if c != self.seen_clicks {
            self.seen_clicks = c;
            self.t = Some(0.0);
            self.freq = 1900.0;
        }
    }

    fn next(&mut self) -> f32 {
        let Some(t) = self.t else { return 0.0 };
        if t > Self::LEN {
            self.t = None;
            return 0.0;
        }
        self.t = Some(t + 1.0 / self.rate);
        // xorshift noise for the "tick", a decaying tone for the body.
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        let n = (self.noise as f32 / u32::MAX as f32) * 2.0 - 1.0;
        let tone = (std::f32::consts::TAU * self.freq * t).sin();
        let env = (-t / 0.006).exp();
        0.22 * env * (0.65 * tone + 0.35 * n * (-t / 0.002).exp())
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
    fn a_click_is_short_and_bounded() {
        let shared = Shared::default();
        let mut v = Voice::new(48_000.0);
        v.poll(&shared);
        assert_eq!(v.next(), 0.0, "silent until a key");
        shared.clicks.fetch_add(1, Ordering::Relaxed);
        v.poll(&shared);
        let samples: Vec<f32> = (0..4800).map(|_| v.next()).collect();
        assert!(samples.iter().all(|s| s.abs() <= 0.25));
        assert!(samples[..200].iter().any(|s| s.abs() > 0.01));
        assert!(
            samples[2000..].iter().all(|s| *s == 0.0),
            "ends within 35 ms"
        );
        let s = KeySound::off();
        s.click(false);
        s.click(true);
        assert_eq!(s.shared.clicks.load(Ordering::Relaxed), 1);
        assert_eq!(s.shared.errors.load(Ordering::Relaxed), 1);
    }
}
