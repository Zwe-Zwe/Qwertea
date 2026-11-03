# Changelog

All notable changes to Qwertea are listed here. The project follows
[Semantic Versioning](https://semver.org/).

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
