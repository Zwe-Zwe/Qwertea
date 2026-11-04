//! `LessonRun`: the typing state machine for one lesson.
//!
//! It is driven from the key handler, so it must stay cheap: no I/O, no
//! logging, no locking, and no heap allocation in `on_key` (the event buffer
//! is pre-allocated; it only grows past `EVENT_CAPACITY`, which a lesson of
//! a few hundred characters never reaches).

pub const EVENT_CAPACITY: usize = 32_768;

/// After the final character, wait at most this long for held keys to be
/// released so the last keystrokes still get a key-up (dwell) event.
pub const RELEASE_GRACE_NS: u64 = 1_000_000_000;

/// One raw key event, mirroring a `key_events` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub seq: u32,
    pub t_ns: u64,
    pub t_kernel_ns: Option<u64>,
    pub scancode: u32,
    pub is_down: bool,
    pub produced_char: Option<char>,
    pub is_backspace: bool,
    pub is_pause_marker: bool,
    pub target_idx: u32,
}

/// What a key press means for cursor logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// A key that produced text (already mapped: Enter -> '\n', Tab -> '\t').
    Text(char),
    Backspace,
    /// Toggles pause.
    Escape,
    /// Arrows, function keys, modifiers, dead keys, ...: logged, ignored.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyInput {
    pub scancode: u32,
    pub is_down: bool,
    pub kind: KeyKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Running,
    Paused,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharState {
    Pending,
    Correct,
    Wrong,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunSummary {
    pub chars: usize,
    pub active_ns: u64,
    pub wpm: f64,
    /// Share of target positions whose *first* attempt was correct.
    pub accuracy: f64,
    pub errors: usize,
}

/// What happens when a wrong key is pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorMode {
    /// The caret moves on; the mistake is marked and can be fixed with Backspace.
    #[default]
    Normal,
    /// The caret waits until the right key is pressed.
    StopOnError,
    /// Mistakes may be made, but a word cannot be finished (space, Enter or
    /// the last character) until its mistakes are fixed.
    StrictWord,
}

impl From<bool> for ErrorMode {
    fn from(stop_on_error: bool) -> Self {
        if stop_on_error {
            Self::StopOnError
        } else {
            Self::Normal
        }
    }
}

pub struct LessonRun {
    target: Vec<char>,
    typed: Vec<Option<char>>,
    /// Whether the first attempt at each position was wrong.
    first_wrong: Vec<Option<bool>>,
    cursor: usize,
    events: Vec<KeyEvent>,
    state: RunState,
    mode: ErrorMode,
    pause_started_ns: u64,
    paused_total_ns: u64,
    finished_at_ns: u64,
    last_t_ns: u64,
    held: Vec<u32>,
    /// Active time (pause excluded) of the first and the latest accepted
    /// text key-down, and how many there were (right or wrong).
    first_key_active: Option<u64>,
    last_key_active: u64,
    text_keys: u32,
    /// Set when the error mode blocked a key; cleared on the next press.
    pub blocked_error: bool,
}

/// Normalise lesson text: CRLF/CR become '\n'.
pub fn normalize_text(text: &str) -> Vec<char> {
    let s = text.replace("\r\n", "\n").replace('\r', "\n");
    s.chars().collect()
}

impl LessonRun {
    pub fn new(text: &str, mode: impl Into<ErrorMode>) -> Self {
        let target = normalize_text(text);
        let n = target.len();
        Self {
            target,
            typed: vec![None; n],
            first_wrong: vec![None; n],
            cursor: 0,
            events: Vec::with_capacity(EVENT_CAPACITY),
            state: if n == 0 {
                RunState::Finished
            } else {
                RunState::Running
            },
            mode: mode.into(),
            pause_started_ns: 0,
            paused_total_ns: 0,
            finished_at_ns: 0,
            last_t_ns: 0,
            held: Vec::with_capacity(32),
            first_key_active: None,
            last_key_active: 0,
            text_keys: 0,
            blocked_error: false,
        }
    }

    pub fn target(&self) -> &[char] {
        &self.target
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn state(&self) -> RunState {
        self.state
    }
    pub fn events(&self) -> &[KeyEvent] {
        &self.events
    }
    pub fn into_events(self) -> Vec<KeyEvent> {
        self.events
    }

    pub fn char_state(&self, i: usize) -> CharState {
        match self.typed.get(i).copied().flatten() {
            None => CharState::Pending,
            Some(c) if c == self.target[i] => CharState::Correct,
            Some(_) => CharState::Wrong,
        }
    }

    /// Feed one key event. `t_ns` is the receipt stamp (ns since lesson start).
    pub fn on_key(&mut self, t_ns: u64, input: KeyInput) {
        self.last_t_ns = self.last_t_ns.max(t_ns);
        if input.is_down {
            if !self.held.contains(&input.scancode) {
                self.held.push(input.scancode);
            }
        } else if let Some(i) = self.held.iter().position(|&s| s == input.scancode) {
            self.held.swap_remove(i);
        }

        if self.state == RunState::Finished {
            // Only the releases of keys held at the end are still recorded.
            if !input.is_down {
                self.push(t_ns, input, None, false);
            }
            return;
        }

        if !input.is_down {
            self.push(t_ns, input, None, false);
            return;
        }

        match (self.state, input.kind) {
            (_, KeyKind::Escape) => {
                self.push(t_ns, input, None, false);
                self.toggle_pause(t_ns);
            }
            (RunState::Paused, KeyKind::Text(c)) => self.push(t_ns, input, Some(c), false),
            (RunState::Paused, _) => self.push(t_ns, input, None, false),
            (_, KeyKind::Text(c)) => {
                self.push(t_ns, input, Some(c), false);
                self.type_char(t_ns, c);
            }
            (_, KeyKind::Backspace) => {
                self.push(t_ns, input, None, true);
                self.blocked_error = false;
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.typed[self.cursor] = None;
                }
            }
            (_, KeyKind::Other) => self.push(t_ns, input, None, false),
        }
    }

    fn type_char(&mut self, t_ns: u64, c: char) {
        let i = self.cursor;
        let active = t_ns.saturating_sub(self.paused_total_ns);
        if self.first_key_active.is_none() {
            self.first_key_active = Some(active);
        }
        self.last_key_active = active;
        self.text_keys += 1;
        let correct = c == self.target[i];
        if self.first_wrong[i].is_none() {
            self.first_wrong[i] = Some(!correct);
        }
        let blocked = match self.mode {
            ErrorMode::Normal => false,
            ErrorMode::StopOnError => !correct,
            ErrorMode::StrictWord => self.word_blocks(i, correct),
        };
        if blocked {
            self.blocked_error = true;
            return;
        }
        self.blocked_error = false;
        self.typed[i] = Some(c);
        self.cursor += 1;
        if self.cursor == self.target.len() {
            self.state = RunState::Finished;
            self.finished_at_ns = t_ns;
        }
    }

    /// Strict-word rule: finishing a word (typing at a space/newline/tab, or
    /// the last character) is refused while that word still has mistakes.
    fn word_blocks(&self, i: usize, correct: bool) -> bool {
        let at_boundary = self.target[i].is_whitespace();
        let last = i + 1 == self.target.len();
        if !at_boundary && !last {
            return false;
        }
        if last && !at_boundary && !correct {
            return true;
        }
        let start = self.target[..i]
            .iter()
            .rposition(|c| c.is_whitespace())
            .map_or(0, |p| p + 1);
        (start..i).any(|j| self.typed[j] != Some(self.target[j]))
    }

    fn toggle_pause(&mut self, t_ns: u64) {
        let marker = KeyInput {
            scancode: 0,
            is_down: self.state == RunState::Running,
            kind: KeyKind::Other,
        };
        self.push(t_ns, marker, None, false);
        let last = self.events.len() - 1;
        self.events[last].is_pause_marker = true;
        match self.state {
            RunState::Running => {
                self.state = RunState::Paused;
                self.pause_started_ns = t_ns;
            }
            RunState::Paused => {
                self.state = RunState::Running;
                self.paused_total_ns += t_ns.saturating_sub(self.pause_started_ns);
            }
            RunState::Finished => {}
        }
    }

    #[inline]
    fn push(&mut self, t_ns: u64, input: KeyInput, ch: Option<char>, is_backspace: bool) {
        let seq = self.events.len() as u32;
        self.events.push(KeyEvent {
            seq,
            t_ns,
            t_kernel_ns: None,
            scancode: input.scancode,
            is_down: input.is_down,
            produced_char: ch,
            is_backspace,
            is_pause_marker: false,
            target_idx: self.cursor as u32,
        });
    }

    /// Pause without a key press (e.g. the window lost focus): only the
    /// pause marker is logged. No-op unless running.
    pub fn pause(&mut self, t_ns: u64) {
        if self.state == RunState::Running {
            self.last_t_ns = self.last_t_ns.max(t_ns);
            self.toggle_pause(t_ns);
        }
    }

    /// Resume after `pause` (or an Esc pause) without a key press.
    pub fn resume(&mut self, t_ns: u64) {
        if self.state == RunState::Paused {
            self.last_t_ns = self.last_t_ns.max(t_ns);
            self.toggle_pause(t_ns);
        }
    }

    /// True once at least one text key was accepted.
    pub fn has_typed(&self) -> bool {
        self.first_wrong.iter().any(Option::is_some)
    }

    /// Time of the last recorded event (ns since lesson start).
    pub fn last_event_ns(&self) -> u64 {
        self.last_t_ns
    }

    /// Active (non-paused) time at `now_ns`.
    pub fn active_ns(&self, now_ns: u64) -> u64 {
        let end = match self.state {
            RunState::Finished => self.finished_at_ns,
            RunState::Paused => self.pause_started_ns,
            RunState::Running => now_ns,
        };
        end.saturating_sub(self.paused_total_ns)
    }

    /// True once the lesson is over and held keys were released (or the
    /// grace period passed): the events can be flushed.
    pub fn ready_to_flush(&self, now_ns: u64) -> bool {
        self.state == RunState::Finished
            && (self.held.is_empty()
                || now_ns.saturating_sub(self.finished_at_ns) >= RELEASE_GRACE_NS)
    }

    /// Typing time so far: from the first keystroke to now (or to the last
    /// keystroke once finished), paused time excluded. This is the clock the
    /// HUD shows and the time WPM is based on (decision D27).
    pub fn elapsed_ns(&self, now_ns: u64) -> u64 {
        let Some(first) = self.first_key_active else {
            return 0;
        };
        let end = match self.state {
            RunState::Finished => self.last_key_active,
            _ => self.active_ns(now_ns),
        };
        end.saturating_sub(first)
    }

    /// Characters currently correct.
    pub fn correct_chars(&self) -> usize {
        (0..self.cursor)
            .filter(|&i| self.char_state(i) == CharState::Correct)
            .count()
    }

    /// Net WPM: correct characters / 5 per minute of `elapsed_ns`.
    pub fn live_wpm(&self, now_ns: u64) -> f64 {
        let t = self.elapsed_ns(now_ns);
        if t == 0 {
            return 0.0;
        }
        (self.correct_chars() as f64 / 5.0) / (t as f64 / 60e9)
    }

    /// (words fully typed, words in the lesson). A word counts as typed once
    /// the caret has passed its last character.
    pub fn word_progress(&self) -> (usize, usize) {
        let (mut done, mut total) = (0, 0);
        let mut in_word = false;
        for (i, c) in self.target.iter().enumerate() {
            if c.is_whitespace() {
                in_word = false;
                continue;
            }
            if !in_word {
                total += 1;
                in_word = true;
            }
            let ends_word = self.target.get(i + 1).is_none_or(|n| n.is_whitespace());
            if ends_word && i < self.cursor {
                done += 1;
            }
        }
        (done, total)
    }

    pub fn summary(&self) -> RunSummary {
        let attempted: Vec<bool> = self.first_wrong.iter().flatten().copied().collect();
        let errors = attempted.iter().filter(|w| **w).count();
        let accuracy = if attempted.is_empty() {
            1.0
        } else {
            1.0 - errors as f64 / attempted.len() as f64
        };
        let active_ns = self.elapsed_ns(self.last_t_ns);
        RunSummary {
            chars: self.target.len(),
            active_ns,
            wpm: self.live_wpm(self.last_t_ns),
            accuracy,
            errors,
        }
    }

    /// Mark the run aborted-in-memory (used when the window closes mid-lesson).
    pub fn is_unfinished(&self) -> bool {
        self.state != RunState::Finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    struct Driver {
        run: LessonRun,
        t: u64,
    }

    impl Driver {
        fn new(text: &str, mode: impl Into<ErrorMode>) -> Self {
            Self {
                run: LessonRun::new(text, mode),
                t: 0,
            }
        }
        fn key(&mut self, scancode: u32, kind: KeyKind) {
            self.t += 50 * MS;
            self.run.on_key(
                self.t,
                KeyInput {
                    scancode,
                    is_down: true,
                    kind,
                },
            );
            self.t += 20 * MS;
            self.run.on_key(
                self.t,
                KeyInput {
                    scancode,
                    is_down: false,
                    kind,
                },
            );
        }
        fn ch(&mut self, c: char) {
            self.key(c as u32, KeyKind::Text(c));
        }
        fn s(&mut self, s: &str) {
            for c in s.chars() {
                self.ch(c);
            }
        }
    }

    #[test]
    fn correct_typing_finishes_and_logs_down_and_up() {
        let mut d = Driver::new("abc", false);
        d.s("abc");
        assert_eq!(d.run.state(), RunState::Finished);
        assert!(d.run.ready_to_flush(d.t));
        let ev = d.run.events();
        assert_eq!(ev.len(), 6);
        assert!(ev.iter().enumerate().all(|(i, e)| e.seq == i as u32));
        assert_eq!(ev[0].produced_char, Some('a'));
        assert_eq!(ev[0].target_idx, 0);
        assert!(!ev[1].is_down);
        assert_eq!(ev[4].target_idx, 2);
        let s = d.run.summary();
        assert_eq!(s.errors, 0);
        assert_eq!(s.accuracy, 1.0);
    }

    #[test]
    fn wrong_chars_advance_and_are_marked() {
        let mut d = Driver::new("abc", false);
        d.s("axc");
        assert_eq!(d.run.state(), RunState::Finished);
        assert_eq!(d.run.char_state(1), CharState::Wrong);
        assert_eq!(d.run.summary().errors, 1);
    }

    #[test]
    fn backspace_moves_back_and_unmarks() {
        let mut d = Driver::new("abcd", false);
        d.s("ax");
        d.key(14, KeyKind::Backspace);
        assert_eq!(d.run.cursor(), 1);
        assert_eq!(d.run.char_state(1), CharState::Pending);
        d.s("bcd");
        assert_eq!(d.run.state(), RunState::Finished);
        // The first attempt at position 1 was wrong; correction does not erase it.
        assert_eq!(d.run.summary().errors, 1);
        let bs: Vec<_> = d.run.events().iter().filter(|e| e.is_backspace).collect();
        assert_eq!(bs.len(), 1, "only the key-down is flagged");
        assert_eq!(bs[0].target_idx, 2);
    }

    #[test]
    fn backspace_chain_stops_at_zero() {
        let mut d = Driver::new("ab", false);
        d.ch('a');
        for _ in 0..3 {
            d.key(14, KeyKind::Backspace);
        }
        assert_eq!(d.run.cursor(), 0);
        assert_eq!(d.run.state(), RunState::Running);
    }

    #[test]
    fn stop_on_error_blocks_wrong_keys() {
        let mut d = Driver::new("ab", true);
        d.s("ax");
        assert_eq!(d.run.cursor(), 1);
        assert!(d.run.blocked_error);
        d.ch('b');
        assert_eq!(d.run.state(), RunState::Finished);
        assert_eq!(d.run.summary().errors, 1);
    }

    #[test]
    fn non_text_keys_are_logged_but_ignored() {
        let mut d = Driver::new("ab", false);
        d.key(105, KeyKind::Other); // arrow
        d.key(42, KeyKind::Other); // shift
        assert_eq!(d.run.cursor(), 0);
        assert_eq!(d.run.events().len(), 4);
    }

    #[test]
    fn pause_is_marked_and_excluded_from_active_time() {
        let mut d = Driver::new("abc", false);
        d.ch('a'); // t = 70 ms
        d.t += 10 * MS;
        d.run.on_key(
            d.t,
            KeyInput {
                scancode: 1,
                is_down: true,
                kind: KeyKind::Escape,
            },
        ); // 80
        assert_eq!(d.run.state(), RunState::Paused);
        d.ch('b'); // ignored while paused
        assert_eq!(d.run.cursor(), 1);
        d.t += 5_000 * MS;
        d.run.on_key(
            d.t,
            KeyInput {
                scancode: 1,
                is_down: true,
                kind: KeyKind::Escape,
            },
        );
        assert_eq!(d.run.state(), RunState::Running);
        let paused = d.t - 80 * MS;
        d.s("bc");
        let markers: Vec<_> = d
            .run
            .events()
            .iter()
            .filter(|e| e.is_pause_marker)
            .collect();
        assert_eq!(markers.len(), 2);
        assert!(markers[0].is_down && !markers[1].is_down);
        let s = d.run.summary();
        assert_eq!(s.active_ns, d.run.elapsed_ns(d.t));
        // Typing time: from the first key-down ('a' at 50 ms) to the last
        // ('c', 20 ms before the final key-up), minus the pause.
        assert_eq!(s.active_ns, d.t - 20 * MS - paused - 50 * MS);
    }

    #[test]
    fn releases_after_finish_are_recorded_then_flush() {
        let mut d = Driver::new("a", false);
        d.run.on_key(
            10 * MS,
            KeyInput {
                scancode: 30,
                is_down: true,
                kind: KeyKind::Text('a'),
            },
        );
        assert_eq!(d.run.state(), RunState::Finished);
        assert!(!d.run.ready_to_flush(20 * MS), "key still held");
        // A new key press after the end is not part of the lesson.
        d.run.on_key(
            30 * MS,
            KeyInput {
                scancode: 31,
                is_down: true,
                kind: KeyKind::Text('b'),
            },
        );
        d.run.on_key(
            40 * MS,
            KeyInput {
                scancode: 30,
                is_down: false,
                kind: KeyKind::Text('a'),
            },
        );
        assert_eq!(d.run.events().len(), 2);
        // Key 31 is still held, so we wait for the grace period.
        assert!(!d.run.ready_to_flush(50 * MS));
        assert!(d.run.ready_to_flush(10 * MS + RELEASE_GRACE_NS));
    }

    #[test]
    fn focus_pause_logs_only_markers() {
        let mut d = Driver::new("ab", false);
        d.ch('a');
        let n = d.run.events().len();
        d.run.pause(d.t + MS);
        d.run.pause(d.t + 2 * MS); // already paused: no-op
        assert_eq!(d.run.state(), RunState::Paused);
        d.run.resume(d.t + 1_000 * MS);
        assert_eq!(d.run.state(), RunState::Running);
        let ev = &d.run.events()[n..];
        assert_eq!(ev.len(), 2);
        assert!(ev.iter().all(|e| e.is_pause_marker));
        assert!(d.run.has_typed());
    }

    #[test]
    fn clock_starts_at_the_first_keystroke() {
        let mut d = Driver::new("abc", false);
        d.t = 3_000 * MS; // three seconds reading the text before typing
        d.s("ab"); // a down @3050, b down @3120
        assert_eq!(d.run.elapsed_ns(3_120 * MS), 70 * MS);
        // Live WPM: 2 correct chars over 70 ms.
        let expected = (2.0 / 5.0) / (0.07 / 60.0);
        assert!((d.run.live_wpm(3_120 * MS) - expected).abs() < 1e-6);
        d.ch('c'); // finishes @3190
        let s = d.run.summary();
        assert_eq!(s.active_ns, 140 * MS);
        assert!((s.wpm - (3.0 / 5.0) / (0.14 / 60.0)).abs() < 1e-6);
        // Time keeps counting while idle mid-lesson, not after the end.
        assert_eq!(d.run.elapsed_ns(99_000 * MS), 140 * MS);
    }

    #[test]
    fn word_progress_counts_finished_words() {
        let mut d = Driver::new("to be or", false);
        assert_eq!(d.run.word_progress(), (0, 3));
        d.s("t");
        assert_eq!(d.run.word_progress(), (0, 3));
        d.s("o");
        assert_eq!(d.run.word_progress(), (1, 3));
        d.s(" be or");
        assert_eq!(d.run.word_progress(), (3, 3));
    }

    #[test]
    fn strict_word_requires_fixing_before_the_space() {
        let mut d = Driver::new("ab cd", ErrorMode::StrictWord);
        d.s("ax"); // the mistake itself is allowed
        assert_eq!(d.run.cursor(), 2);
        d.ch(' '); // but the word cannot be finished
        assert_eq!(d.run.cursor(), 2);
        assert!(d.run.blocked_error);
        d.key(14, KeyKind::Backspace);
        d.s("b cd");
        assert_eq!(d.run.state(), RunState::Finished);
        // The last word is checked too.
        let mut d = Driver::new("ab", ErrorMode::StrictWord);
        d.s("ax");
        assert_eq!(d.run.state(), RunState::Running);
        d.s("b");
        assert_eq!(d.run.state(), RunState::Finished);
    }

    #[test]
    fn multiline_and_tabs_are_supported() {
        let mut d = Driver::new("a\r\n\tb", false);
        assert_eq!(d.run.target(), &['a', '\n', '\t', 'b']);
        d.s("a\n\tb");
        assert_eq!(d.run.state(), RunState::Finished);
        assert_eq!(d.run.summary().errors, 0);
    }
}
