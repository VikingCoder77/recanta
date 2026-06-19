//! SQLite data layer. One file per project (`.recanta/recanta.db`), opened in WAL
//! mode so readers never block the short-lived writer processes fired by hooks
//! (PRD §12.2). Opening a connection always brings the schema up to date.

pub mod migrations;

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

/// Register the sqlite-vec extension for all connections opened afterwards. Idempotent;
/// safe to call before every `open`. Enables `vec0` virtual tables + KNN (PRD §12.1).
pub fn register_vec() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        // Transmute a C fn pointer to the auto-extension type (the canonical sqlite-vec
        // registration); the target type is a long FFI signature, so annotate via allow.
        #[allow(clippy::missing_transmute_annotations)]
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

/// Open (creating if needed) the project store and apply any pending migrations.
///
/// Pragmas:
/// - `journal_mode=WAL` + `busy_timeout` — the concurrency model (PRD §12.2): many
///   short-lived writers (git hooks, harness hooks, CLI) plus non-blocking readers.
/// - `foreign_keys=ON` — provenance links are real constraints, not conventions.
/// - `synchronous=NORMAL` — safe under WAL and much faster for frequent small writes.
pub fn open(db_path: &Path) -> Result<Connection> {
    register_vec();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let conn = Connection::open(db_path)
        .with_context(|| format!("opening {}", db_path.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;",
    )
    .context("configuring SQLite pragmas")?;
    migrations::migrate(&conn)?;
    Ok(conn)
}

/// Open an existing store without migrating (read-side guard). Errors if the file
/// is missing or its schema is newer than this binary understands (PRD §11.3).
pub fn open_existing(db_path: &Path) -> Result<Connection> {
    register_vec();
    if !db_path.is_file() {
        anyhow::bail!("store not found at {}", db_path.display());
    }
    let conn = Connection::open(db_path)
        .with_context(|| format!("opening {}", db_path.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    migrations::ensure_compatible(&conn)?;
    Ok(conn)
}

#[cfg(test)]
mod vec_smoke {
    #[test]
    fn sqlite_vec_knn_works() {
        super::register_vec();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let ver: String = conn.query_row("SELECT vec_version()", [], |r| r.get(0)).unwrap();
        assert!(!ver.is_empty(), "vec_version: {ver}");
        conn.execute("CREATE VIRTUAL TABLE vt USING vec0(embedding float[4])", []).unwrap();
        let blob = |a: [f32; 4]| a.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>();
        conn.execute("INSERT INTO vt(rowid, embedding) VALUES (1, ?1)", [blob([1.0, 0.0, 0.0, 0.0])]).unwrap();
        conn.execute("INSERT INTO vt(rowid, embedding) VALUES (2, ?1)", [blob([0.0, 1.0, 0.0, 0.0])]).unwrap();
        let nearest: i64 = conn
            .query_row(
                "SELECT rowid FROM vt WHERE embedding MATCH ?1 ORDER BY distance LIMIT 1",
                [blob([0.92, 0.08, 0.0, 0.0])],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(nearest, 1, "nearest neighbor should be the [1,0,0,0] vector");
    }
}
