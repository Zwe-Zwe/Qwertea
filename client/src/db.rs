//! Client-side database access. The client writes only:
//! `sessions` start/end fields, lesson `status` (`running`/`done`/`aborted`,
//! or back to `pending` for a lesson left before a key was typed) and
//! `key_events`. It never generates lessons.
//!
//! All writes happen between lessons; a finished lesson is flushed in one
//! transaction.

use crate::session::KeyEvent;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SCHEMA_SQL: &str = include_str!("../../schema/001_init.sql");
/// Numbered migrations applied after 001, in order.
pub const MIGRATIONS: &[(i64, &str)] = &[(2, include_str!("../../schema/002_lesson_meta.sql"))];
pub const SCHEMA_VERSION: i64 = 2;
pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub type DbResult<T> = rusqlite::Result<T>;

/// Engine-written lesson metadata (`lessons.meta_json`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct LessonMeta {
    pub title: Option<String>,
    pub instruction: Option<String>,
    pub sitting: Option<i64>,
    pub sittings: Option<i64>,
    pub phase_no: Option<i64>,
    pub phases: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Target {
    pub kind: String,
    pub item: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingLesson {
    pub id: i64,
    pub order_idx: i64,
    pub mode: String,
    pub phase: String,
    pub text: String,
    pub targets_json: Option<String>,
    pub is_discarded: bool,
    pub meta: LessonMeta,
}

impl PendingLesson {
    pub fn targets(&self) -> Vec<Target> {
        self.targets_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_default()
    }
    pub fn is_calibration(&self) -> bool {
        CALIBRATION_PHASES.contains(&self.phase.as_str())
    }
    pub fn title(&self) -> String {
        self.meta.title.clone().unwrap_or_else(|| {
            let mut s = self.phase.clone();
            if let Some(c) = s.get_mut(0..1) {
                c.make_ascii_uppercase();
            }
            s
        })
    }
}

/// Calibration phases (section 7.3). Lessons in these phases run inside a
/// `kind='calibration'` session; `probe` lessons in a `kind='probe'`
/// session; everything else is `kind='lesson'`.
pub const CALIBRATION_PHASES: &[&str] = &[
    "warmup", "natural", "coverage", "pseudo", "special", "speed", "accuracy", "retest",
];

pub fn session_kind_for_phase(phase: &str) -> &'static str {
    if CALIBRATION_PHASES.contains(&phase) {
        "calibration"
    } else if phase == "probe" {
        "probe"
    } else {
        "lesson"
    }
}

/// Open (creating if needed) the database in WAL mode and apply the schema
/// and migrations.
pub fn open(path: &Path) -> DbResult<Connection> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(dir);
        }
    }
    let conn = Connection::open(path)?;
    configure(&conn)?;
    Ok(conn)
}

pub fn configure(conn: &Connection) -> DbResult<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    ensure_schema(conn)
}

/// Apply 001 when `schema_version` is missing, then newer migrations. Each
/// step re-checks inside an IMMEDIATE transaction, so client and engine can
/// race safely.
pub fn ensure_schema(conn: &Connection) -> DbResult<()> {
    let has = |c: &Connection| -> DbResult<bool> {
        c.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_version')",
            [],
            |r| r.get(0),
        )
    };
    if !has(conn)? {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        if !has(conn)? {
            if let Err(e) = conn.execute_batch(SCHEMA_SQL) {
                conn.execute_batch("ROLLBACK")?;
                return Err(e);
            }
        }
        conn.execute_batch("COMMIT")?;
    }
    for (version, sql) in MIGRATIONS {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let current: i64 =
            conn.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))?;
        if current < *version {
            if let Err(e) = conn.execute_batch(sql) {
                conn.execute_batch("ROLLBACK")?;
                return Err(e);
            }
        }
        conn.execute_batch("COMMIT")?;
    }
    Ok(())
}

pub fn schema_version(conn: &Connection) -> DbResult<Option<i64>> {
    conn.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
}

/// Crash safety: lessons left `running` by a previous run become `aborted`.
pub fn recover_running(conn: &Connection) -> DbResult<usize> {
    conn.execute(
        "UPDATE lessons SET status='aborted' WHERE status='running'",
        [],
    )
}

fn row_to_lesson(r: &rusqlite::Row<'_>) -> rusqlite::Result<PendingLesson> {
    let meta: Option<String> = r.get(7)?;
    Ok(PendingLesson {
        id: r.get(0)?,
        order_idx: r.get(1)?,
        mode: r.get(2)?,
        phase: r.get(3)?,
        text: r.get(4)?,
        targets_json: r.get(5)?,
        is_discarded: r.get::<_, i64>(6)? != 0,
        meta: meta
            .and_then(|m| serde_json::from_str(&m).ok())
            .unwrap_or_default(),
    })
}

const LESSON_COLS: &str = "id, order_idx, mode, phase, text, targets_json, is_discarded, meta_json";

pub fn next_pending(conn: &Connection) -> DbResult<Option<PendingLesson>> {
    conn.query_row(
        &format!(
            "SELECT {LESSON_COLS} FROM lessons WHERE status='pending' ORDER BY order_idx, id LIMIT 1"
        ),
        [],
        row_to_lesson,
    )
    .optional()
}

pub fn count_pending(conn: &Connection) -> DbResult<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM lessons WHERE status='pending'",
        [],
        |r| r.get(0),
    )
}

#[derive(Debug, Clone)]
pub struct NewSession<'a> {
    pub kind: &'a str,
    pub mode: &'a str,
    pub sitting_no: Option<i64>,
    pub layout: &'a str,
    pub keyboard: Option<&'a str>,
    pub timestamp_source: &'a str,
}

pub fn start_session(conn: &Connection, s: &NewSession<'_>) -> DbResult<i64> {
    conn.execute(
        "INSERT INTO sessions (started_at_utc, kind, mode, sitting_no, layout, keyboard,
                               timestamp_source, client_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            utc_now(),
            s.kind,
            s.mode,
            s.sitting_no,
            s.layout,
            s.keyboard,
            s.timestamp_source,
            CLIENT_VERSION
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Mark a pending lesson `running` and attach it to a session. Returns false
/// if the lesson was no longer pending (e.g. the engine replaced it).
pub fn begin_lesson(conn: &Connection, lesson_id: i64, session_id: i64) -> DbResult<bool> {
    let n = conn.execute(
        "UPDATE lessons SET status='running', session_id=?2 WHERE id=?1 AND status='pending'",
        params![lesson_id, session_id],
    )?;
    Ok(n == 1)
}

/// Put a lesson the user left before typing anything back in the queue.
pub fn revert_to_pending(conn: &Connection, lesson_id: i64) -> DbResult<()> {
    conn.execute(
        "UPDATE lessons SET status='pending', session_id=NULL WHERE id=?1 AND status='running'",
        [lesson_id],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LessonOutcome {
    Done,
    Aborted,
}

/// Flush a lesson in one transaction: all raw events, the lesson status and
/// the session end time. Aborted lessons keep their raw events (lossless)
/// but are excluded from analysis by their status.
pub fn finish_lesson(
    conn: &mut Connection,
    lesson_id: i64,
    session_id: i64,
    events: &[KeyEvent],
    outcome: LessonOutcome,
) -> DbResult<()> {
    let now = utc_now();
    let tx = conn.transaction()?;
    {
        let mut ins = tx.prepare(
            "INSERT INTO key_events (lesson_id, seq, t_ns, t_kernel_ns, scancode, is_down,
                                     produced_char, is_backspace, is_pause_marker, target_idx)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        let mut buf = [0u8; 4];
        for e in events {
            let ch: Option<&str> = e.produced_char.map(|c| &*c.encode_utf8(&mut buf));
            ins.execute(params![
                lesson_id,
                e.seq,
                e.t_ns as i64,
                e.t_kernel_ns.map(|t| t as i64),
                e.scancode,
                e.is_down as i64,
                ch,
                e.is_backspace as i64,
                e.is_pause_marker as i64,
                e.target_idx,
            ])?;
        }
    }
    let status = match outcome {
        LessonOutcome::Done => "done",
        LessonOutcome::Aborted => "aborted",
    };
    tx.execute(
        "UPDATE lessons SET status=?2, finished_at_utc=?3 WHERE id=?1",
        params![lesson_id, status, now],
    )?;
    tx.execute(
        "UPDATE sessions SET ended_at_utc=?2 WHERE id=?1",
        params![session_id, now],
    )?;
    tx.commit()
}

/// Read back a lesson's raw events.
pub fn load_events(conn: &Connection, lesson_id: i64) -> DbResult<Vec<KeyEvent>> {
    let mut st = conn.prepare_cached(
        "SELECT seq, t_ns, t_kernel_ns, scancode, is_down, produced_char, is_backspace,
                is_pause_marker, target_idx
         FROM key_events WHERE lesson_id=?1 ORDER BY seq",
    )?;
    let rows = st.query_map([lesson_id], |r| {
        let ch: Option<String> = r.get(5)?;
        Ok(KeyEvent {
            seq: r.get(0)?,
            t_ns: r.get::<_, i64>(1)? as u64,
            t_kernel_ns: r.get::<_, Option<i64>>(2)?.map(|t| t as u64),
            scancode: r.get(3)?,
            is_down: r.get::<_, i64>(4)? != 0,
            produced_char: ch.and_then(|s| s.chars().next()),
            is_backspace: r.get::<_, i64>(6)? != 0,
            is_pause_marker: r.get::<_, i64>(7)? != 0,
            target_idx: r.get(8)?,
        })
    })?;
    rows.collect()
}

// --------------------------------------------------------------------- calibration

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CalibrationProgress {
    pub total: i64,
    pub done: i64,
    pub remaining: i64,
    /// Sitting of the next calibration lesson (1-based) and how many there are.
    pub sitting: i64,
    pub sittings: i64,
    pub phase_no: i64,
    pub phases: i64,
    /// Characters left in the current sitting / in all of calibration.
    pub chars_left_sitting: i64,
    pub chars_left_total: i64,
    pub chars_total: i64,
}

impl CalibrationProgress {
    pub fn is_complete(&self) -> bool {
        self.total > 0 && self.remaining == 0
    }
    pub fn fraction(&self) -> f32 {
        if self.chars_total == 0 {
            return 0.0;
        }
        1.0 - self.chars_left_total as f32 / self.chars_total as f32
    }
}

fn phase_list() -> String {
    CALIBRATION_PHASES
        .iter()
        .map(|p| format!("'{p}'"))
        .collect::<Vec<_>>()
        .join(",")
}

pub fn calibration_progress(conn: &Connection) -> DbResult<Option<CalibrationProgress>> {
    let phases = phase_list();
    let (total, done, chars_total): (i64, i64, i64) = conn.query_row(
        &format!(
            "SELECT COUNT(*), COALESCE(SUM(status IN ('done','aborted')), 0),
                    COALESCE(SUM(CASE WHEN is_discarded=0 THEN LENGTH(text) ELSE 0 END), 0)
             FROM lessons WHERE phase IN ({phases})"
        ),
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if total == 0 {
        return Ok(None);
    }
    let mut p = CalibrationProgress {
        total,
        done,
        remaining: total - done,
        chars_total,
        ..Default::default()
    };
    let next: Option<(String, Option<String>)> = conn
        .query_row(
            &format!(
                "SELECT phase, meta_json FROM lessons WHERE phase IN ({phases})
                 AND status IN ('pending','running') ORDER BY order_idx, id LIMIT 1"
            ),
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let left: Vec<(i64, Option<String>)> = {
        let mut st = conn.prepare(&format!(
            "SELECT LENGTH(text), meta_json FROM lessons WHERE phase IN ({phases})
             AND status IN ('pending','running') AND is_discarded=0"
        ))?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<DbResult<_>>()?
    };
    let meta_of = |m: &Option<String>| -> LessonMeta {
        m.as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_default()
    };
    if let Some((_, meta)) = &next {
        let m = meta_of(meta);
        p.sitting = m.sitting.unwrap_or(1);
        p.sittings = m.sittings.unwrap_or(1);
        p.phase_no = m.phase_no.unwrap_or(1);
        p.phases = m.phases.unwrap_or(1);
    }
    for (len, meta) in &left {
        p.chars_left_total += len;
        if meta_of(meta).sitting.unwrap_or(1) == p.sitting {
            p.chars_left_sitting += len;
        }
    }
    Ok(Some(p))
}

/// Skip the rest of calibration: pending calibration lessons become `aborted`
/// (the engine then starts adaptive lessons from the data collected so far).
pub fn skip_calibration(conn: &Connection) -> DbResult<usize> {
    conn.execute(
        &format!(
            "UPDATE lessons SET status='aborted', finished_at_utc=?1
             WHERE status='pending' AND phase IN ({})",
            phase_list()
        ),
        [utc_now()],
    )
}

// --------------------------------------------------------------------- history & stats

#[derive(Debug, Clone)]
pub struct DoneLesson {
    pub id: i64,
    pub phase: String,
    pub text: String,
    pub finished_at: String,
    pub is_discarded: bool,
}

/// Finished lessons, oldest first (at most `limit`, newest kept).
pub fn done_lessons(conn: &Connection, limit: i64) -> DbResult<Vec<DoneLesson>> {
    let mut st = conn.prepare(
        "SELECT id, phase, text, finished_at_utc, is_discarded FROM (
            SELECT * FROM lessons WHERE status='done' ORDER BY finished_at_utc DESC, id DESC LIMIT ?1
         ) ORDER BY finished_at_utc, id",
    )?;
    let rows = st.query_map([limit], |r| {
        Ok(DoneLesson {
            id: r.get(0)?,
            phase: r.get(1)?,
            text: r.get(2)?,
            finished_at: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            is_discarded: r.get::<_, i64>(4)? != 0,
        })
    })?;
    rows.collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct ItemStat {
    pub kind: String,
    pub item: String,
    pub n: i64,
    pub p_error: f64,
    pub latency_ms: Option<f64>,
    pub weakness: f64,
    pub is_holdout: bool,
}

pub fn item_stats(
    conn: &Connection,
    mode: &str,
    kind: &str,
    min_n: i64,
    limit: i64,
) -> DbResult<Vec<ItemStat>> {
    let mut st = conn.prepare(
        "SELECT item_kind, item, n, err_alpha, err_beta, lat_median_log, COALESCE(weakness, 0), is_holdout
         FROM item_stats WHERE mode=?1 AND item_kind=?2 AND n>=?3
         ORDER BY weakness DESC LIMIT ?4",
    )?;
    let rows = st.query_map(params![mode, kind, min_n, limit], |r| {
        let a: f64 = r.get(3)?;
        let b: f64 = r.get(4)?;
        Ok(ItemStat {
            kind: r.get(0)?,
            item: r.get(1)?,
            n: r.get(2)?,
            p_error: a / (a + b),
            latency_ms: r.get::<_, Option<f64>>(5)?.map(f64::exp),
            weakness: r.get(6)?,
            is_holdout: r.get::<_, i64>(7)? != 0,
        })
    })?;
    rows.collect()
}

pub fn key_weakness(conn: &Connection, mode: &str) -> DbResult<HashMap<String, f64>> {
    Ok(item_stats(conn, mode, "key", 8, 500)?
        .into_iter()
        .map(|s| (s.item, s.weakness))
        .collect())
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct LevelInfo {
    pub wpm: f64,
    pub accuracy: f64,
    pub lessons: i64,
}

pub fn latest_level(conn: &Connection, mode: &str) -> DbResult<Option<LevelInfo>> {
    let j: Option<Option<String>> = conn
        .query_row(
            "SELECT level_json FROM model_snapshots WHERE mode=?1 ORDER BY id DESC LIMIT 1",
            [mode],
            |r| r.get(0),
        )
        .optional()?;
    Ok(j.flatten().and_then(|s| serde_json::from_str(&s).ok()))
}

/// Current UTC time as ISO-8601 (`2026-11-03T17:49:00.123Z`).
pub fn utc_now() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_utc(d.as_secs() as i64, d.subsec_millis())
}

pub fn format_utc(secs: i64, millis: u32) -> String {
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

/// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{KeyInput, KeyKind, LessonRun};

    fn mem() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        configure(&c).unwrap();
        c
    }

    fn insert_lesson(c: &Connection, order: i64, phase: &str, text: &str) -> i64 {
        c.execute(
            "INSERT INTO lessons (order_idx, phase, text, created_at_utc) VALUES (?1, ?2, ?3, ?4)",
            params![order, phase, text, utc_now()],
        )
        .unwrap();
        c.last_insert_rowid()
    }

    fn session(c: &Connection) -> i64 {
        start_session(
            c,
            &NewSession {
                kind: "lesson",
                mode: "prose",
                sitting_no: None,
                layout: "qwerty-us",
                keyboard: None,
                timestamp_source: "winit",
            },
        )
        .unwrap()
    }

    #[test]
    fn utc_formatting() {
        assert_eq!(format_utc(0, 0), "1970-01-01T00:00:00.000Z");
        assert_eq!(format_utc(951_782_400, 5), "2000-02-29T00:00:00.005Z");
        assert_eq!(format_utc(1_793_727_000, 999), "2026-11-03T17:30:00.999Z");
    }

    #[test]
    fn schema_and_migrations_are_applied_once() {
        let c = mem();
        ensure_schema(&c).unwrap();
        assert_eq!(schema_version(&c).unwrap(), Some(SCHEMA_VERSION));
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        // meta_json exists (migration 2).
        c.execute("UPDATE lessons SET meta_json = NULL", [])
            .unwrap();
    }

    #[test]
    fn pending_lessons_come_in_order_with_meta() {
        let c = mem();
        assert!(next_pending(&c).unwrap().is_none());
        insert_lesson(&c, 2, "drill", "second");
        let first = insert_lesson(&c, 1, "warmup", "first");
        c.execute(
            "UPDATE lessons SET meta_json=?2, targets_json=?3 WHERE id=?1",
            params![
                first,
                r#"{"title":"Warm-up","sitting":1,"sittings":3,"phase_no":1,"phases":6}"#,
                r#"[{"kind":"bigram","item":"th","reason":"weak"}]"#
            ],
        )
        .unwrap();
        let p = next_pending(&c).unwrap().unwrap();
        assert_eq!(p.id, first);
        assert_eq!(p.title(), "Warm-up");
        assert_eq!(p.meta.sittings, Some(3));
        assert_eq!(p.targets()[0].item, "th");
        assert!(p.is_calibration());
        assert_eq!(count_pending(&c).unwrap(), 2);
    }

    #[test]
    fn running_lessons_are_aborted_on_startup() {
        let c = mem();
        let id = insert_lesson(&c, 1, "natural", "x");
        let s = session(&c);
        assert!(begin_lesson(&c, id, s).unwrap());
        assert!(!begin_lesson(&c, id, s).unwrap(), "already running");
        assert_eq!(recover_running(&c).unwrap(), 1);
        let st: String = c
            .query_row("SELECT status FROM lessons WHERE id=?1", [id], |r| r.get(0))
            .unwrap();
        assert_eq!(st, "aborted");
        assert!(next_pending(&c).unwrap().is_none());
    }

    #[test]
    fn revert_puts_lesson_back() {
        let c = mem();
        let id = insert_lesson(&c, 1, "drill", "x");
        let s = session(&c);
        begin_lesson(&c, id, s).unwrap();
        revert_to_pending(&c, id).unwrap();
        assert_eq!(next_pending(&c).unwrap().unwrap().id, id);
    }

    #[test]
    fn calibration_progress_and_skip() {
        let c = mem();
        for (i, (phase, sitting)) in [("warmup", 1), ("natural", 1), ("natural", 2)]
            .iter()
            .enumerate()
        {
            let id = insert_lesson(&c, i as i64, phase, "abcdefghij");
            c.execute(
                "UPDATE lessons SET meta_json=?2, is_discarded=?3 WHERE id=?1",
                params![
                    id,
                    format!(
                        r#"{{"sitting":{sitting},"sittings":2,"phase_no":{},"phases":2}}"#,
                        i + 1
                    ),
                    (*phase == "warmup") as i64
                ],
            )
            .unwrap();
        }
        insert_lesson(&c, 10, "drill", "zzz");
        let p = calibration_progress(&c).unwrap().unwrap();
        assert_eq!((p.total, p.done, p.sitting, p.sittings), (3, 0, 1, 2));
        assert_eq!(p.chars_total, 20, "warm-up is not counted");
        assert_eq!(p.chars_left_sitting, 10);
        assert!(!p.is_complete());
        assert_eq!(skip_calibration(&c).unwrap(), 3);
        let p = calibration_progress(&c).unwrap().unwrap();
        assert!(p.is_complete());
        assert_eq!(next_pending(&c).unwrap().unwrap().phase, "drill");
    }

    #[test]
    fn finished_lesson_round_trips() {
        let mut c = mem();
        let id = insert_lesson(&c, 1, "natural", "hé\n");
        let s = session(&c);
        begin_lesson(&c, id, s).unwrap();
        let mut run = LessonRun::new("hé\n", false);
        let mut t = 0;
        for (sc, ch) in [(35u32, 'h'), (18, 'é'), (28, '\n')] {
            t += 100_000_000;
            run.on_key(
                t,
                KeyInput {
                    scancode: sc,
                    is_down: true,
                    kind: KeyKind::Text(ch),
                },
            );
            t += 30_000_000;
            run.on_key(
                t,
                KeyInput {
                    scancode: sc,
                    is_down: false,
                    kind: KeyKind::Text(ch),
                },
            );
        }
        let mut events = run.into_events();
        events[0].t_kernel_ns = Some(99_000_000);
        finish_lesson(&mut c, id, s, &events, LessonOutcome::Done).unwrap();
        assert_eq!(load_events(&c, id).unwrap(), events);
        let done = done_lessons(&c, 10).unwrap();
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].id, id);
        let ended: Option<String> = c
            .query_row("SELECT ended_at_utc FROM sessions WHERE id=?1", [s], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(ended.is_some());
    }

    #[test]
    fn session_kind_follows_phase() {
        assert_eq!(session_kind_for_phase("coverage"), "calibration");
        assert_eq!(session_kind_for_phase("probe"), "probe");
        assert_eq!(session_kind_for_phase("drill"), "lesson");
    }
}
