//! Lesson results computed from the raw events (used for the summary screen
//! and the history chart). Runs between lessons, never in the key handler.
//!
//! Definitions (paused time excluded everywhere):
//! * time: from the first to the last accepted keystroke;
//! * WPM: characters correct at the end / 5 per minute;
//! * raw WPM: every typed character (right or wrong) / 5 per minute;
//! * accuracy: share of positions whose *first* attempt was correct
//!   (the same definition the engine uses);
//! * consistency: 100 × (1 − coefficient of variation of per-second raw WPM).

use crate::session::{normalize_text, KeyEvent};
use std::collections::HashMap;

const THINK_PAUSE_NS: u64 = 2_500_000_000;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SecondSample {
    pub second: u32,
    pub wpm: f64,
    pub raw: f64,
    pub errors: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LessonStats {
    pub chars: usize,
    pub seconds: f64,
    pub wpm: f64,
    pub raw_wpm: f64,
    pub accuracy: f64,
    pub consistency: f64,
    pub correct: usize,
    pub incorrect: usize,
    pub corrected: usize,
    pub errors: usize,
    pub keystrokes: usize,
    pub backspaces: usize,
    pub per_second: Vec<SecondSample>,
    /// (bigram, median ms) — the three slowest transitions of the lesson.
    pub slowest_bigrams: Vec<(String, f64)>,
    /// Characters whose first attempt was wrong, with counts, most first.
    pub missed_keys: Vec<(char, usize)>,
    /// Speed and first-attempt errors of every word, in order.
    pub words: Vec<WordSample>,
    /// Per text position: was the first attempt wrong?
    pub first_wrong: Vec<bool>,
    /// Words in the lesson text.
    pub word_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WordSample {
    pub text: String,
    pub wpm: f64,
    pub errors: u32,
}

fn pause_intervals(events: &[KeyEvent]) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut start = None;
    for e in events.iter().filter(|e| e.is_pause_marker) {
        match (e.is_down, start) {
            (true, None) => start = Some(e.t_ns),
            (false, Some(s)) => {
                out.push((s, e.t_ns));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, events.last().map(|e| e.t_ns).unwrap_or(s)));
    }
    out
}

fn active(t: u64, pauses: &[(u64, u64)]) -> u64 {
    let mut removed = 0;
    for &(a, b) in pauses {
        if t >= b {
            removed += b - a;
        } else if t > a {
            removed += t - a;
        }
    }
    t - removed
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

pub fn compute(text: &str, events: &[KeyEvent]) -> LessonStats {
    let target = normalize_text(text);
    let pauses = pause_intervals(events);
    let mut typed: Vec<Option<char>> = vec![None; target.len()];
    let mut first_wrong: Vec<Option<bool>> = vec![None; target.len()];
    // Active time at which each position was first typed.
    let mut first_time: Vec<Option<u64>> = vec![None; target.len()];
    let mut paused = false;
    // (active time, correct?) for each text keystroke, accepted or blocked.
    let mut strokes: Vec<(u64, bool)> = Vec::new();
    let mut flights: HashMap<String, Vec<f64>> = HashMap::new();
    let mut last_down: Option<u64> = None;
    let mut backspaces = 0;
    let mut corrected = 0;
    for (k, e) in events.iter().enumerate() {
        if e.is_pause_marker {
            paused = e.is_down;
            continue;
        }
        if !e.is_down || paused {
            continue;
        }
        let t = active(e.t_ns, &pauses);
        // The cursor position is taken from the event itself, so lessons
        // typed with stop-on-error or strict-word replay correctly.
        let i = e.target_idx as usize;
        if e.is_backspace {
            backspaces += 1;
            last_down = Some(t);
            if i > 0 && i <= target.len() {
                typed[i - 1] = None;
            }
            continue;
        }
        let Some(c) = e.produced_char else { continue };
        if i >= target.len() {
            continue;
        }
        let ok = c == target[i];
        // Did the caret move on? The next event's cursor tells (blocked keys
        // leave it in place); the final keystroke has no successor.
        let advanced = events[k + 1..]
            .iter()
            .find(|n| !n.is_pause_marker)
            .map_or(i + 1 == target.len(), |n| n.target_idx as usize == i + 1);
        match first_wrong[i] {
            None => {
                first_wrong[i] = Some(!ok);
                first_time[i] = Some(t);
                if ok && i > 0 {
                    if let Some(prev) = last_down {
                        let f = t.saturating_sub(prev);
                        if f > 0 && f < THINK_PAUSE_NS {
                            let bigram: String = target[i - 1..=i]
                                .iter()
                                .map(|c| c.to_ascii_lowercase())
                                .collect();
                            if !bigram.contains(['\n', '\t']) {
                                flights.entry(bigram).or_default().push(f as f64 / 1e6);
                            }
                        }
                    }
                }
            }
            Some(true) if ok => corrected += 1,
            _ => {}
        }
        if advanced {
            typed[i] = Some(c);
        }
        strokes.push((t, ok));
        last_down = Some(t);
    }

    let mut s = LessonStats {
        chars: target.len(),
        keystrokes: strokes.len(),
        backspaces,
        corrected,
        ..Default::default()
    };
    s.correct = (0..target.len())
        .filter(|&i| typed[i] == Some(target[i]))
        .count();
    s.incorrect = typed.iter().flatten().count() - s.correct;
    let attempts: Vec<bool> = first_wrong.iter().flatten().copied().collect();
    s.errors = attempts.iter().filter(|w| **w).count();
    s.accuracy = if attempts.is_empty() {
        1.0
    } else {
        1.0 - s.errors as f64 / attempts.len() as f64
    };
    let mut missed: HashMap<char, usize> = HashMap::new();
    for (i, w) in first_wrong.iter().enumerate() {
        if *w == Some(true) {
            *missed.entry(target[i]).or_default() += 1;
        }
    }
    s.missed_keys = missed.into_iter().collect();
    s.missed_keys
        .sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    if let (Some(&(t0, _)), Some(&(t1, _))) = (strokes.first(), strokes.last()) {
        // Time runs from the first keystroke to the last (decision D27).
        let secs = (t1 - t0) as f64 / 1e9;
        s.seconds = secs;
        if secs > 0.0 {
            s.wpm = s.correct as f64 / 5.0 / (secs / 60.0);
            s.raw_wpm = strokes.len() as f64 / 5.0 / (secs / 60.0);
        }
        // Per-second samples.
        let total_secs = secs.ceil().max(1.0) as usize;
        let mut chars = vec![0u32; total_secs];
        let mut errs = vec![0u32; total_secs];
        let mut good = vec![0u32; total_secs];
        for &(t, ok) in &strokes {
            let b = (((t - t0) as f64 / 1e9) as usize).min(total_secs - 1);
            chars[b] += 1;
            if ok {
                good[b] += 1;
            } else {
                errs[b] += 1;
            }
        }
        let mut cum_good = 0u32;
        for i in 0..total_secs {
            cum_good += good[i];
            let elapsed = ((i + 1) as f64).min(secs.max(1.0));
            s.per_second.push(SecondSample {
                second: i as u32 + 1,
                wpm: cum_good as f64 / 5.0 / (elapsed / 60.0),
                raw: chars[i] as f64 * 12.0,
                errors: errs[i],
            });
        }
        let raws: Vec<f64> = s.per_second.iter().map(|p| p.raw).collect();
        if raws.len() >= 2 {
            let mean = raws.iter().sum::<f64>() / raws.len() as f64;
            let var = raws.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / raws.len() as f64;
            s.consistency = if mean > 0.0 {
                (100.0 * (1.0 - var.sqrt() / mean)).clamp(0.0, 100.0)
            } else {
                0.0
            };
        }
    }

    let mut slow: Vec<(String, f64)> = flights
        .into_iter()
        .map(|(k, mut v)| (k, median(&mut v)))
        .collect();
    slow.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    slow.truncate(3);
    s.slowest_bigrams = slow;
    s.first_wrong = first_wrong.iter().map(|w| *w == Some(true)).collect();
    s.words = word_samples(&target, &first_time, &s.first_wrong);
    s.word_count = count_words(&target);
    s
}

/// Whitespace-separated words in the text.
pub fn count_words(target: &[char]) -> usize {
    let mut n = 0;
    let mut in_word = false;
    for c in target {
        if c.is_whitespace() {
            in_word = false;
        } else if !in_word {
            n += 1;
            in_word = true;
        }
    }
    n
}

/// Per-word speed: the time from the keystroke before the word (usually the
/// space) to its last letter. The first word has no lead-in and is skipped
/// when it has fewer than two timed letters.
fn word_samples(target: &[char], times: &[Option<u64>], wrong: &[bool]) -> Vec<WordSample> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < target.len() {
        if target[i].is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < target.len() && !target[i].is_whitespace() {
            i += 1;
        }
        let end = i; // exclusive
        let text: String = target[start..end].iter().collect();
        let errors = wrong[start..end].iter().filter(|w| **w).count() as u32;
        let (from, chars) = if start > 0 {
            (times[start - 1], end - start + 1)
        } else {
            (times[start], end - start)
        };
        let wpm = match (from, times[end - 1]) {
            (Some(a), Some(b)) if b > a && chars > 0 => {
                (chars as f64 / 5.0) / ((b - a) as f64 / 60e9)
            }
            _ => 0.0,
        };
        if times[end - 1].is_some() {
            out.push(WordSample { text, wpm, errors });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{KeyInput, KeyKind, LessonRun};
    use serde_json;

    const MS: u64 = 1_000_000;

    fn run(text: &str, keys: &[(KeyKind, u64)]) -> Vec<KeyEvent> {
        let mut r = LessonRun::new(text, false);
        let mut t = 0;
        for &(k, gap) in keys {
            t += gap * MS;
            r.on_key(
                t,
                KeyInput {
                    scancode: 1,
                    is_down: true,
                    kind: k,
                },
            );
            r.on_key(
                t + 10 * MS,
                KeyInput {
                    scancode: 1,
                    is_down: false,
                    kind: k,
                },
            );
        }
        r.into_events()
    }

    fn chars(s: &str, gap: u64) -> Vec<(KeyKind, u64)> {
        s.chars().map(|c| (KeyKind::Text(c), gap)).collect()
    }

    #[test]
    fn perfect_lesson() {
        // 10 chars, one every 120 ms: first key at 0.12 s, last at 1.20 s,
        // so 1.08 s of typing => 10/5 / (1.08/60) = 111.1 WPM.
        let ev = run("abcde fghi", &chars("abcde fghi", 120));
        let s = compute("abcde fghi", &ev);
        assert!((s.seconds - 1.08).abs() < 1e-9);
        assert!((s.wpm - 2.0 / (1.08 / 60.0)).abs() < 1e-6, "{}", s.wpm);
        assert_eq!(s.raw_wpm, s.wpm);
        assert_eq!(s.accuracy, 1.0);
        assert_eq!((s.correct, s.incorrect, s.errors), (10, 0, 0));
        assert_eq!(s.per_second.len(), 2);
        assert_eq!(s.slowest_bigrams.len(), 3);
    }

    #[test]
    fn errors_corrections_and_slow_bigram() {
        let mut keys = chars("ax", 100);
        keys.push((KeyKind::Backspace, 100));
        keys.push((KeyKind::Text('b'), 100));
        keys.push((KeyKind::Text('c'), 900)); // slow b->c
        let ev = run("abc", &keys);
        let s = compute("abc", &ev);
        assert_eq!(s.errors, 1);
        assert_eq!(s.corrected, 1);
        assert_eq!(s.correct, 3);
        assert_eq!(s.backspaces, 1);
        assert!((s.accuracy - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(s.slowest_bigrams[0], ("bc".to_string(), 900.0));
        assert_eq!(s.missed_keys, vec![('b', 1)]);
        assert!(s.raw_wpm > s.wpm);
    }

    #[test]
    fn pauses_do_not_count() {
        let mut keys = chars("ab", 100);
        keys.push((KeyKind::Escape, 50));
        keys.push((KeyKind::Escape, 10_000));
        keys.push((KeyKind::Text('c'), 100));
        let ev = run("abc", &keys);
        let s = compute("abc", &ev);
        // a@100, b@200, pause 250..10250 (10 s removed), c@10350 -> active 350.
        assert!((s.seconds - 0.25).abs() < 1e-9, "{}", s.seconds);
        assert_eq!(
            s.slowest_bigrams[0].1, 150.0,
            "b -> c flight excludes the pause"
        );
    }

    #[test]
    fn per_word_samples() {
        // "ab cd": every key 100 ms apart; 'x' typed for 'c'.
        let ev = run("ab cd", &chars("ab xd", 100));
        let s = compute("ab cd", &ev);
        assert_eq!(s.words.len(), 2);
        assert_eq!(s.words[0].text, "ab");
        // "ab": 2 chars over 100 ms (a -> b) = 240 wpm.
        assert!((s.words[0].wpm - 240.0).abs() < 1e-6);
        // " cd": 3 chars over 200 ms (b -> d) = 180 wpm, one error.
        assert!((s.words[1].wpm - 180.0).abs() < 1e-6);
        assert_eq!(s.words[1].errors, 1);
        assert_eq!(s.first_wrong, vec![false, false, false, true, false]);
    }

    #[derive(serde::Deserialize)]
    struct Golden {
        text: String,
        events: Vec<GoldenEvent>,
        expected: Expected,
    }
    #[derive(serde::Deserialize)]
    struct GoldenEvent {
        t_ns: u64,
        scancode: u32,
        is_down: bool,
        produced_char: Option<String>,
        is_backspace: bool,
        is_pause_marker: bool,
    }
    #[derive(serde::Deserialize)]
    struct Expected {
        seconds: f64,
        wpm: f64,
        raw_wpm: f64,
        accuracy: f64,
        correct: usize,
        errors: usize,
        keystrokes: usize,
        backspaces: usize,
        words: usize,
    }

    /// The shared, hand-worked golden lesson (testdata/wpm_golden.json): the
    /// engine's test checks the same numbers.
    #[test]
    fn golden_lesson_matches_hand_worked_numbers() {
        let g: Golden =
            serde_json::from_str(include_str!("../../testdata/wpm_golden.json")).unwrap();
        // Drive the real LessonRun with the fixture's key sequence.
        let mut r = LessonRun::new(&g.text, false);
        for e in g.events.iter().filter(|e| !e.is_pause_marker) {
            let kind = if e.is_backspace {
                KeyKind::Backspace
            } else if let Some(c) = e.produced_char.as_ref().and_then(|s| s.chars().next()) {
                KeyKind::Text(c)
            } else if e.scancode == 1 {
                KeyKind::Escape
            } else {
                KeyKind::Other
            };
            r.on_key(
                e.t_ns,
                KeyInput {
                    scancode: e.scancode,
                    is_down: e.is_down,
                    kind,
                },
            );
        }
        let x = &g.expected;
        let live_wpm = r.live_wpm(u64::MAX);
        let live_secs = r.elapsed_ns(u64::MAX) as f64 / 1e9;
        let s = compute(&g.text, r.events());
        assert!(
            (s.seconds - x.seconds).abs() < 1e-9,
            "seconds {}",
            s.seconds
        );
        assert!((s.wpm - x.wpm).abs() < 1e-9, "wpm {}", s.wpm);
        assert!((s.raw_wpm - x.raw_wpm).abs() < 1e-9, "raw {}", s.raw_wpm);
        assert!((s.accuracy - x.accuracy).abs() < 1e-12);
        assert_eq!(
            (
                s.correct,
                s.errors,
                s.keystrokes,
                s.backspaces,
                s.word_count
            ),
            (x.correct, x.errors, x.keystrokes, x.backspaces, x.words)
        );
        // The live counter at the end equals the result screen.
        assert!((live_wpm - x.wpm).abs() < 1e-9, "live wpm {live_wpm}");
        assert!(
            (live_secs - x.seconds).abs() < 1e-9,
            "live secs {live_secs}"
        );
        assert_eq!(r.word_progress(), (2, 2));
    }

    #[test]
    fn stop_on_error_lessons_replay_correctly() {
        let mut r = LessonRun::new("ab", true);
        let mut t = 0;
        for c in ['a', 'x', 'x', 'b'] {
            t += 100 * MS;
            r.on_key(
                t,
                KeyInput {
                    scancode: 1,
                    is_down: true,
                    kind: KeyKind::Text(c),
                },
            );
            r.on_key(
                t + 10 * MS,
                KeyInput {
                    scancode: 1,
                    is_down: false,
                    kind: KeyKind::Text(c),
                },
            );
        }
        let s = compute("ab", r.events());
        // Both positions end correct; the blocked 'x' keys are slips, not typed text.
        assert_eq!(
            (s.correct, s.incorrect, s.errors, s.keystrokes),
            (2, 0, 1, 4)
        );
        assert!((s.wpm - r.live_wpm(u64::MAX)).abs() < 1e-9);
    }

    #[test]
    fn empty_lesson() {
        let s = compute("abc", &[]);
        assert_eq!(s.wpm, 0.0);
        assert_eq!(s.accuracy, 1.0);
    }
}
