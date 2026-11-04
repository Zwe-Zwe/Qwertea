-- Migration 3: per-profile settings, stored in the profile's own database so
-- that each profile (and an exported profile file) carries its theme, typing
-- settings and goals. Written by the client; the engine reads the lesson keys
-- (lesson_length, lesson_chars, capitals, punctuation, numbers). See D28.
CREATE TABLE profile_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
UPDATE schema_version SET version = 3;
