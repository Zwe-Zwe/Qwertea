//! Analysis for the M0 timing spike (`cargo run --bin timing_check`).
//!
//! Question 1: are receipt stamps frame-quantized? If key stamps were taken
//! inside egui they would coincide with frame starts. We compare each stamp
//! with the frame boundaries recorded during the run: the *phase* of a stamp
//! inside its frame should be spread over [0, 1) and stamps must be distinct.
//!
//! Question 2 (evdev only): how far behind the kernel is the winit receipt?

#[derive(Debug, Clone, PartialEq)]
pub struct Dist {
    pub n: usize,
    pub min: f64,
    pub median: f64,
    pub p90: f64,
    pub p99: f64,
    pub max: f64,
}

pub fn dist(values: &[f64]) -> Option<Dist> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    let q = |p: f64| {
        let idx = p * (v.len() - 1) as f64;
        let (lo, hi) = (idx.floor() as usize, idx.ceil() as usize);
        v[lo] + (v[hi] - v[lo]) * (idx - lo as f64)
    };
    Some(Dist {
        n: v.len(),
        min: v[0],
        median: q(0.5),
        p90: q(0.9),
        p99: q(0.99),
        max: v[v.len() - 1],
    })
}

/// One rendered frame on the UI thread, ns since origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Clone)]
pub struct TimingReport {
    pub key_events: usize,
    pub frames: usize,
    pub frame_interval_ms: Option<Dist>,
    /// Time the UI thread spent inside one frame (egui pass + submit + present).
    /// While it renders, key events wait in the queue, so this bounds the
    /// receipt delay rendering can add.
    pub render_ms: Option<Dist>,
    /// Share of wall time the UI thread was busy rendering.
    pub render_busy_fraction: f64,
    /// Stamp position inside its frame interval, 0..1.
    pub phase: Option<Dist>,
    /// Share of stamps within 50 us after a frame start (what stamping inside
    /// egui would produce). Quantized stamps -> ~1.0.
    pub share_at_frame_start: f64,
    /// Share of consecutive events with identical stamps.
    pub share_duplicate_stamps: f64,
    /// Delay egui-time stamping would add (next frame start - receipt), ms.
    pub egui_quantization_error_ms: Option<Dist>,
    /// Phase histogram in 10 bins.
    pub phase_histogram: [usize; 10],
    pub verdict_not_quantized: bool,
    /// winit receipt minus kernel timestamp, ms (evdev only).
    pub winit_minus_kernel_ms: Option<Dist>,
    pub unmatched_kernel: usize,
    /// Smallest observed non-zero step of `Instant::now()`, ns.
    pub clock_resolution_ns: u64,
    pub clock_read_cost_ns: f64,
}

/// `stamps`: receipt stamps (ns, same origin as `frames`), in order.
/// `frames`: every rendered frame, ascending.
pub fn analyze(stamps: &[u64], frames: &[Frame]) -> TimingReport {
    let starts: Vec<u64> = frames.iter().map(|f| f.start).collect();
    let intervals: Vec<f64> = starts
        .windows(2)
        .map(|w| (w[1] - w[0]) as f64 / 1e6)
        .collect();
    let renders: Vec<f64> = frames
        .iter()
        .map(|f| f.end.saturating_sub(f.start) as f64 / 1e6)
        .collect();
    let span_ms = match (frames.first(), frames.last()) {
        (Some(a), Some(b)) if b.end > a.start => (b.end - a.start) as f64 / 1e6,
        _ => 0.0,
    };
    let render_busy_fraction = if span_ms > 0.0 {
        renders.iter().sum::<f64>() / span_ms
    } else {
        0.0
    };
    let mut phases = Vec::new();
    let mut errors = Vec::new();
    let mut at_start = 0usize;
    let mut hist = [0usize; 10];
    for &t in stamps {
        // Frame interval containing t: last start <= t, next start > t.
        let i = starts.partition_point(|&f| f <= t);
        if i == 0 || i >= starts.len() {
            continue;
        }
        let (a, b) = (starts[i - 1], starts[i]);
        let ph = (t - a) as f64 / (b - a) as f64;
        phases.push(ph);
        hist[((ph * 10.0) as usize).min(9)] += 1;
        errors.push((b - t) as f64 / 1e6);
        if t - a < 50_000 {
            at_start += 1;
        }
    }
    let dup = stamps.windows(2).filter(|w| w[0] == w[1]).count();
    let share_at_frame_start = if phases.is_empty() {
        0.0
    } else {
        at_start as f64 / phases.len() as f64
    };
    let share_duplicate_stamps = if stamps.len() < 2 {
        0.0
    } else {
        dup as f64 / (stamps.len() - 1) as f64
    };
    let verdict = phases.len() >= 20 && share_at_frame_start < 0.1 && share_duplicate_stamps < 0.05;
    let (res, cost) = clock_probe();
    TimingReport {
        key_events: stamps.len(),
        frames: frames.len(),
        frame_interval_ms: dist(&intervals),
        render_ms: dist(&renders),
        render_busy_fraction,
        phase: dist(&phases),
        share_at_frame_start,
        share_duplicate_stamps,
        egui_quantization_error_ms: dist(&errors),
        phase_histogram: hist,
        verdict_not_quantized: verdict,
        winit_minus_kernel_ms: None,
        unmatched_kernel: 0,
        clock_resolution_ns: res,
        clock_read_cost_ns: cost,
    }
}

fn clock_probe() -> (u64, f64) {
    use std::time::Instant;
    let mut min_step = u64::MAX;
    let start = Instant::now();
    let mut prev = start;
    let mut reads = 0u64;
    while reads < 200_000 {
        let now = Instant::now();
        reads += 1;
        let d = now.duration_since(prev).as_nanos() as u64;
        if d > 0 {
            min_step = min_step.min(d);
        }
        prev = now;
    }
    let cost = start.elapsed().as_nanos() as f64 / reads as f64;
    (min_step, cost)
}

impl TimingReport {
    pub fn to_markdown(&self) -> String {
        let f = |d: &Option<Dist>| match d {
            Some(d) => format!(
                "n={} min={:.3} median={:.3} p90={:.3} p99={:.3} max={:.3}",
                d.n, d.min, d.median, d.p90, d.p99, d.max
            ),
            None => "n/a".into(),
        };
        let mut s = String::new();
        s += &format!(
            "- key events: {}, frames: {}\n",
            self.key_events, self.frames
        );
        s += &format!("- frame interval (ms): {}\n", f(&self.frame_interval_ms));
        s += &format!("- render time per frame (ms): {}\n", f(&self.render_ms));
        s += &format!(
            "- UI thread busy rendering: {:.1}% of the time\n",
            100.0 * self.render_busy_fraction
        );
        s += &format!("- stamp phase within frame (0..1): {}\n", f(&self.phase));
        s += &format!("- phase histogram (10 bins): {:?}\n", self.phase_histogram);
        s += &format!(
            "- share of stamps within 50 us after a frame start: {:.3}\n",
            self.share_at_frame_start
        );
        s += &format!(
            "- share of duplicate consecutive stamps: {:.3}\n",
            self.share_duplicate_stamps
        );
        s += &format!(
            "- error egui-time stamping would add (ms): {}\n",
            f(&self.egui_quantization_error_ms)
        );
        s += &format!(
            "- Instant::now(): resolution {} ns, cost {:.1} ns/read\n",
            self.clock_resolution_ns, self.clock_read_cost_ns
        );
        s += &format!(
            "- winit − kernel (ms): {}\n",
            f(&self.winit_minus_kernel_ms)
        );
        if self.winit_minus_kernel_ms.is_some() {
            s += &format!("- unmatched UI events: {}\n", self.unmatched_kernel);
        }
        s += &format!(
            "- **verdict: {}**\n",
            if self.verdict_not_quantized {
                "stamps are NOT frame-quantized"
            } else {
                "INCONCLUSIVE or quantized (need >= 20 events inside recorded frames)"
            }
        );
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(n: u64, period: u64) -> Vec<Frame> {
        (0..n)
            .map(|i| Frame {
                start: i * period,
                end: i * period + period / 4,
            })
            .collect()
    }

    #[test]
    fn quantiles() {
        let d = dist(&[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
        assert_eq!(d.median, 3.0);
        assert_eq!(d.min, 1.0);
        assert_eq!(d.max, 5.0);
        assert!(dist(&[]).is_none());
    }

    #[test]
    fn spread_stamps_are_not_quantized() {
        let period = 16_666_667;
        let f = frames(400, period);
        // Pseudo-random positions within frames (LCG), like real typing.
        let mut x: u64 = 12345;
        let stamps: Vec<u64> = (5..300)
            .map(|i| {
                x = x
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                i * period + (x >> 33) % period
            })
            .collect();
        let r = analyze(&stamps, &f);
        assert!(r.verdict_not_quantized, "{}", r.to_markdown());
        let p = r.phase.unwrap();
        assert!(p.median > 0.3 && p.median < 0.7);
    }

    #[test]
    fn frame_aligned_stamps_are_detected() {
        let period = 16_666_667;
        let f = frames(400, period);
        let stamps: Vec<u64> = (5..300).map(|i| i * period + 10_000).collect();
        assert!((analyze(&stamps, &f).render_busy_fraction - 0.25).abs() < 0.01);
        let r = analyze(&stamps, &f);
        assert!(!r.verdict_not_quantized);
        assert!(r.share_at_frame_start > 0.9);
    }
}
