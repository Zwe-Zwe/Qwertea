//! M1 round-trip helper: run the next pending lesson with a scripted typist
//! and flush it exactly like the app does. Used by the engine test
//! `engine/tests/test_roundtrip.py`.
//!
//! Usage: roundtrip_fixture <db_path>
//! Prints the id of the lesson it completed, or exits 2 if none is pending.

use qwertea_client::db::{self, LessonOutcome, NewSession};
use qwertea_client::session::{KeyInput, KeyKind, LessonRun};
use std::path::PathBuf;

const MS: u64 = 1_000_000;

fn main() {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: roundtrip_fixture <db_path>"),
    );
    let mut conn = db::open(&path).expect("open db");
    db::recover_running(&conn).expect("recover");
    let Some(lesson) = db::next_pending(&conn).expect("query") else {
        eprintln!("no pending lesson");
        std::process::exit(2);
    };
    let session = db::start_session(
        &conn,
        &NewSession {
            kind: db::session_kind_for_phase(&lesson.phase),
            mode: &lesson.mode,
            sitting_no: None,
            layout: "qwerty-us",
            keyboard: None,
            timestamp_source: "winit",
        },
    )
    .expect("session");
    assert!(db::begin_lesson(&conn, lesson.id, session).expect("begin"));

    // Scripted typist: one wrong key + backspace at position 1, a pause
    // after position 2, kernel stamps 1.5 ms before receipt.
    let mut run = LessonRun::new(&lesson.text, false);
    let mut t = 0u64;
    let press = |run: &mut LessonRun, t: &mut u64, sc: u32, kind: KeyKind| {
        *t += 120 * MS;
        run.on_key(
            *t,
            KeyInput {
                scancode: sc,
                is_down: true,
                kind,
            },
        );
        *t += 40 * MS;
        run.on_key(
            *t,
            KeyInput {
                scancode: sc,
                is_down: false,
                kind,
            },
        );
    };
    let target: Vec<char> = run.target().to_vec();
    for (i, &c) in target.iter().enumerate() {
        if i == 1 {
            press(&mut run, &mut t, 999, KeyKind::Text('#'));
            press(&mut run, &mut t, 14, KeyKind::Backspace);
        }
        if i == 2 {
            press(&mut run, &mut t, 1, KeyKind::Escape);
            t += 3_000 * MS;
            press(&mut run, &mut t, 1, KeyKind::Escape);
        }
        press(&mut run, &mut t, c as u32, KeyKind::Text(c));
    }
    assert!(run.ready_to_flush(t), "lesson should be complete");
    let mut events = run.into_events();
    for e in events.iter_mut().filter(|e| !e.is_pause_marker) {
        e.t_kernel_ns = Some(e.t_ns - 1_500_000);
    }
    db::finish_lesson(&mut conn, lesson.id, session, &events, LessonOutcome::Done).expect("flush");
    println!("{}", lesson.id);
}
