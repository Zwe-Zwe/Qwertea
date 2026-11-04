# Changelog

All notable changes to Qwertea are listed here. The project follows
[Semantic Versioning](https://semver.org/).

## [1.0.2] - 2026-11-04

### Added
- **Profiles.** Each person gets their own progress, lessons, model, theme and
  settings (one database per profile).
  - The Profile screen shows today's practice against a daily goal, a streak,
    and speed and accuracy goals.
  - Switch, create, rename and delete profiles there, and export or import a
    profile as one `.qwertea` file.
  - Your existing data becomes the first profile.
- **Typing settings,** saved to the profile:
  - nine typing fonts: Atkinson Mono, JetBrains Mono, Fira Code, Cascadia
    Mono, Ubuntu Mono, IBM Plex Mono, Hack, Atkinson and Ubuntu. Each font
    card previews itself;
  - a **live demo** next to the font options: click it and type to try your
    font, size, caret, mistakes mode and key sound before a real lesson;
  - caret: underline, block or bar, with an optional smooth caret;
  - live stats: full, time and words, or hidden;
  - mistakes: move on, stop on error, or fix before the next word;
  - lesson length: short, medium, long or a custom number of characters;
  - capital letters, punctuation and numbers on or off. New lessons follow
    within seconds, and calibration is unchanged.
- **Key sounds** (optional): soft click, typewriter, deep thock, bubble pop
  or retro blip. Each has its own sound for a mistake, and there is a volume
  control. Choosing a sound plays a preview.
- **In-app updates.** Qwertea checks GitHub for a newer version at start-up.
  - A source install updates itself with one click, then restarts.
  - An Arch package install gets the command to copy and a link to the
    release notes.

### Changed
- Settings save automatically; there is no Save button.
- `config.toml` holds only this computer's settings, plus a new `[updates]`
  section.

### Fixed
- **WPM, timer and word counter** (reported by testers):
  - the clock now starts at your first keystroke, not when the screen opens;
  - the live WPM uses the same formula and clock as the result, so the two
    agree when you finish;
  - the word counter shows finished words out of the total;
  - stop-on-error lessons are scored correctly;
  - the engine's timing matches the app's.

  A hand-worked golden lesson checks all of this in both programs.
- Stopping the engine now stops the Python process too, not only the `uv`
  launcher. A stale pid file is cleared.

## [1.0.1] - 2026-11-04

### Changed
- A new visual identity of Qwertea's own, replacing a look that was too close
  to other typing sites:
  - tea themes: Sencha (default), Chai, Earl Grey and Oolong; 1.0.0 theme
    names map to them automatically;
  - Atkinson Hyperlegible type (SIL OFL);
  - a left sidebar with icons;
  - a steaming-cup app icon.
- Typing screen: the lesson sits on a sheet with a progress edge and an
  underline caret. The lesson's training targets are tinted in the text
  still to come. A labelled stat strip sits under the sheet.
- Lesson notes replace the results screen:
  - a verdict compared with your own recent lessons and personal best;
  - a per-word rhythm strip;
  - per-target results;
  - details.
- Practice home: "Up next" lists each training target with the model's
  current error rate. A "your level" card shows a sparkline, and a step
  tracker shows calibration progress.
- Progress: trend area charts, a keycap heatmap and ranked weak-spot bars.
- Settings: theme preview cards, and an explanation for each option.
- The HTML report uses the new palette.
- The keyboard heatmap is drawn as a full keyboard. Every row has the same
  width (ANSI key sizes), modifier keys are dimmed, and each scored key has a
  thin bar showing how far it is from your baseline.
- Responsive layout. Pages share one centred content column up to 1560 px, so
  maximised and full-screen windows no longer leave an empty band. Wide
  windows place cards side by side, and side-by-side cards share one height.
- Practice home adds "Recent lessons" and "Focus areas" cards.

### Fixed
- Weak-spot lists: measured label and value widths, so text no longer runs
  into the neighbouring column.
- Spacing between buttons and captions on Lesson notes, Progress and
  Settings; the rhythm-strip legend no longer runs together.

## [1.0.0] - 2026-11-03

First public release.

### App
- Desktop app with a menu (practice, stats, settings, report) and four themes:
  dark, light, nord and matcha.
- Distraction-free typing screen:
  - three scrolling lines, correct/wrong/pending colours;
  - live clock, progress and WPM;
  - Esc pause menu (resume, restart, skip, quit); pauses automatically when
    the window loses focus.
- Results after every lesson:
  - WPM, raw WPM, accuracy, consistency and time;
  - a per-second chart with error markers;
  - correct/incorrect/fixed characters, slowest letter pairs, missed keys.
- Stats: speed and accuracy history, keyboard weakness heatmap, weakest keys,
  letter pairs and words.
- Guided calibration (three sittings, phase intros, break screens,
  "Sitting 1 of 3 · Phase 2 of 6 · ~9 min left"), with an option to skip.
- Keystrokes are stamped before the UI sees them. Kernel (evdev) timestamps
  are optional on Linux. The UI repaints on demand.
- Starts the analysis engine automatically and stops it on exit.
- Installs system-wide: settings in `~/.config/qwertea/`, data in
  `~/.local/share/qwertea/`. Arch Linux PKGBUILDs (`qwertea`, `qwertea-git`).

### Engine
- Derives labelled keystrokes from raw key events:
  - time between keys and key hold time (pauses removed);
  - error type: substitution, adjacent slip, transposition, omission or
    insertion;
  - corrections and post-error slow-down.
- Weakness model:
  - hierarchical empirical-Bayes error rates (word → trigram → bigram →
    key → global);
  - robust latency baselines against the user's own median.
- Thompson-sampling target selection, never-drilled holdout items as a
  control group, and Leitner spaced review.
- Lesson generator: weakness-ranked real words, Markov pseudo-words and
  natural sentences chosen for target coverage; difficulty control toward
  92–96 % accuracy; a checkpoint passage every 10 lessons.
- Calibration planner (greedy set cover) with a coverage report.
- LightGBM insight model with TreeSHAP explanations, and a self-contained
  HTML report (learning curve, heatmap, error types, behaviour, drilled vs
  holdout).
- Bundled public-domain prose corpus; works offline.
