//! Qwertea client entry point.
//!
//! M1 state: opens the database (applying the schema and crash recovery)
//! and reports the lesson queue. The typing UI arrives in M2; until then use
//! `cargo run --bin timing_check` for the windowed timing spike.

use qwertea_client::config::Config;
use qwertea_client::db;

fn main() {
    let cfg = Config::load();
    let path = cfg.db_path();
    let conn = match db::open(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("qwertea: cannot open {}: {e}", path.display());
            std::process::exit(1);
        }
    };
    let recovered = db::recover_running(&conn).unwrap_or(0);
    if recovered > 0 {
        println!("Marked {recovered} interrupted lesson(s) as aborted.");
    }
    println!("Database: {}", path.display());
    match db::next_pending(&conn) {
        Ok(Some(l)) => println!(
            "Next lesson #{} (phase {}, {} chars). {} pending in total.",
            l.id,
            l.phase,
            l.text.chars().count(),
            db::count_pending(&conn).unwrap_or(0)
        ),
        Ok(None) => println!("Engine is preparing your next lesson (no pending lessons)."),
        Err(e) => eprintln!("qwertea: query failed: {e}"),
    }
    println!("The typing UI lands in milestone M2.");
}
