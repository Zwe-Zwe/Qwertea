//! Client-side database access. The client writes only:
//! `sessions` start/end fields, lesson `status` (`running`/`done`/`aborted`)
//! and `key_events`. It never generates lessons.
//!
//! All writes happen between lessons; a finished lesson is flushed in one
//! transaction.

use crate::session::KeyEvent;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SCHEMA_SQL: &str = include_str!("../../schema/001_init.sql");
pub const SCHEMA_VERSION: i64 = 1;
pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub type DbResult<T> = rusqlite::Result<T>;

#[derive(Debug, Clone, PartialEq)]
pub struct PendingLesson {
    pub id: i64,
    pub order_idx: i64,
    pub mode: String,
    pub phase: String,
    pub text: String,
    pub targets_json: Option<String>,
    pub is_discarded: bool,
}

/// Calibration phases (section 7.3). Lessons in these phases run inside a
/// `kind='calibration'` session; everything else is `kind='lesson'`.
pub const CALIBRATION_PHASES: &[&str] = &[
    "warmup", "natural", "coverage", "pseudo", "special", "speed", "accuracy", "retest",
];

pub fn session_kind_for_phase(phase: &str) -> &'static str {
    if CALIBRATION_PHASES.contains(&phase) {
        "calibration"
    } else {
        "lesson"
    }
}

/// Open (creating if needed) the database in WAL mode and apply the schema
/// when `schema_version` is missing.
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

pub fn ensure_schema(conn: &Connection) -> DbResult<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_version')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        conn.execute_batch(&format!("BEGIN IMMEDIATE;\n{SCHEMA_SQL}\nCOMMIT;"))?;
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

pub fn next_pending(conn: &Connection) -> DbResult<Option<PendingLesson>> {
    conn.query_row(
        "SELECT id, order_idx, mode, phase, text, targets_json, is_discarded
         FROM lessons WHERE status='pending' ORDER BY order_idx, id LIMIT 1",
        [],
        |r| {
            Ok(PendingLesson {
                id: r.get(0)?,
                order_idx: r.get(1)?,
                mode: r.get(2)?,
                phase: r.get(3)?,
                text: r.get(4)?,
                targets_json: r.get(5)?,
                is_discarded: r.get::<_, i64>(6)? != 0,
            })
        },
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
/// if the lesson was no longer pending.
pub fn begin_lesson(conn: &Connection, lesson_id: i64, session_id: i64) -> DbResult<bool> {
    let n = conn.execute(
        "UPDATE lessons SET status='running', session_id=?2 WHERE id=?1 AND status='pending'",
        params![lesson_id, session_id],
    )?;
    Ok(n == 1)
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

/// Read back a lesson's raw events (used by tests and tools).
pub fn load_events(conn: &Connection, lesson_id: i64) -> DbResult<Vec<KeyEvent>> {
    let mut st = conn.prepare(
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
    fn schema_is_applied_once() {
        let c = mem();
        ensure_schema(&c).unwrap();
        assert_eq!(schema_version(&c).unwrap(), Some(SCHEMA_VERSION));
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn pending_lessons_come_in_order() {
        let c = mem();
        assert!(next_pending(&c).unwrap().is_none());
        insert_lesson(&c, 2, "drill", "second");
        let first = insert_lesson(&c, 1, "warmup", "first");
        let p = next_pending(&c).unwrap().unwrap();
        assert_eq!(p.id, first);
        assert_eq!(p.text, "first");
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
        let (st, fin): (String, Option<String>) = c
            .query_row(
                "SELECT status, finished_at_utc FROM lessons WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(st, "done");
        assert!(fin.is_some());
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
        assert_eq!(session_kind_for_phase("drill"), "lesson");
    }
}
