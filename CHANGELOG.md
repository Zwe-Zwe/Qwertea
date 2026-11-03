# Changelog

All notable changes to Qwertea are listed here. The project follows
[Semantic Versioning](https://semver.org/).

## [Unreleased] - candidate for 1.0.1 (branch `new_ui`)

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
