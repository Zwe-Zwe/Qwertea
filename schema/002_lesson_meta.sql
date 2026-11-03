-- Migration 2: per-lesson metadata written by the engine (see docs/decisions.md, D13).
-- JSON object, e.g. {"sitting": 1, "sittings": 3, "phase_no": 2, "phases": 7,
--                    "title": "Natural text", "instruction": "Type at your natural pace."}
ALTER TABLE lessons ADD COLUMN meta_json TEXT;
UPDATE schema_version SET version = 2;
