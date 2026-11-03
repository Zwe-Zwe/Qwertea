-- Qwertea schema, version 1. Authoritative: applied by both the Rust client and the Python engine.
-- Apply only when schema_version is missing. Both sides set: PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;

CREATE TABLE schema_version (version INTEGER NOT NULL);

CREATE TABLE sessions (
  id INTEGER PRIMARY KEY,
  started_at_utc TEXT NOT NULL,
  ended_at_utc TEXT,
  kind TEXT NOT NULL,                 -- 'calibration' | 'lesson' | 'probe'
  mode TEXT NOT NULL DEFAULT 'prose', -- later: 'code'
  sitting_no INTEGER,                 -- calibration sitting 1..3, else NULL
  layout TEXT NOT NULL,
  keyboard TEXT,
  timestamp_source TEXT NOT NULL,     -- 'winit' | 'evdev'
  client_version TEXT
);

CREATE TABLE lessons (
  id INTEGER PRIMARY KEY,
  session_id INTEGER REFERENCES sessions(id),  -- set by client when run
  order_idx INTEGER NOT NULL,                  -- run order
  mode TEXT NOT NULL DEFAULT 'prose',
  language TEXT,                               -- NULL now; 'rust','python',... later
  phase TEXT NOT NULL,   -- warmup|natural|coverage|pseudo|special|speed|accuracy|retest|drill|review|mixed
  text TEXT NOT NULL,
  targets_json TEXT,                           -- [{"kind":"bigram","item":"th","reason":"weak"}]
  is_discarded INTEGER NOT NULL DEFAULT 0,     -- warmup etc., excluded from model fitting
  status TEXT NOT NULL DEFAULT 'pending',      -- pending|running|done|aborted
  created_at_utc TEXT NOT NULL,
  finished_at_utc TEXT,
  generator TEXT, generator_version TEXT, model_snapshot_id INTEGER
);

-- RAW capture: written only by the client, never modified
CREATE TABLE key_events (
  lesson_id INTEGER NOT NULL REFERENCES lessons(id),
  seq INTEGER NOT NULL,
  t_ns INTEGER NOT NULL,             -- monotonic ns since lesson start (primary source)
  t_kernel_ns INTEGER,               -- optional evdev timestamp, same origin
  scancode INTEGER NOT NULL,
  is_down INTEGER NOT NULL,
  produced_char TEXT,                -- NULL for non-text keys
  is_backspace INTEGER NOT NULL DEFAULT 0,
  is_pause_marker INTEGER NOT NULL DEFAULT 0,
  target_idx INTEGER NOT NULL,       -- cursor index in lesson text when the event happened
  PRIMARY KEY (lesson_id, seq)
) WITHOUT ROWID;

-- DERIVED: written only by the engine, rebuildable
CREATE TABLE keystrokes (
  lesson_id INTEGER NOT NULL,
  seq INTEGER NOT NULL,              -- seq of the key-down event this row describes
  target_idx INTEGER NOT NULL,
  expected TEXT, typed TEXT,
  is_first_attempt INTEGER NOT NULL, -- first attempt at this target position
  error_type TEXT,                   -- NULL|substitution|transposition|omission|insertion|adjacent_slip
  dwell_ms REAL, flight_ms REAL,     -- flight = time since previous key-down
  prev1 TEXT, prev2 TEXT,
  finger TEXT, hand TEXT, row INTEGER,
  corrected INTEGER, time_to_notice_ms REAL, time_to_correct_ms REAL,
  post_error_slowdown_ms REAL,       -- mean extra latency over next 3 keys
  session_pos REAL,                  -- 0..1 position within the session (fatigue/learning)
  PRIMARY KEY (lesson_id, seq)
) WITHOUT ROWID;

CREATE TABLE item_stats (
  mode TEXT NOT NULL,
  item_kind TEXT NOT NULL,           -- key|bigram|trigram|word (later: token|syntax_pattern)
  item TEXT NOT NULL,
  n INTEGER NOT NULL,
  err_alpha REAL NOT NULL, err_beta REAL NOT NULL,   -- Beta posterior for P(error)
  lat_median_log REAL, lat_mad_log REAL,
  weakness REAL,                     -- combined score, higher = weaker
  last_seen_utc TEXT, last_drilled_utc TEXT,
  next_review_utc TEXT,
  is_holdout INTEGER NOT NULL DEFAULT 0,  -- control item, never drilled
  PRIMARY KEY (mode, item_kind, item)
);

CREATE TABLE model_snapshots (
  id INTEGER PRIMARY KEY,
  created_at_utc TEXT NOT NULL,
  mode TEXT NOT NULL,
  level_json TEXT,                   -- WPM, accuracy, speed-accuracy curve params
  behavior_json TEXT,                -- post-error slowdown, rhythm variance, fatigue slope
  notes TEXT
);

CREATE INDEX idx_events_lesson ON key_events(lesson_id);
CREATE INDEX idx_keystrokes_lesson ON keystrokes(lesson_id);

INSERT INTO schema_version (version) VALUES (1);
