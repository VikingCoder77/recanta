//! SQLite data layer. One file per project (`.recanta/recanta.db`), opened in WAL
//! mode so readers never block the short-lived writer processes fired by hooks
//! (PRD §12.2). Opening a connection always brings the schema up to date.

pub mod migrations;

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

/// Open (creating if needed) the project store and apply any pending migrations.
///
/// Pragmas:
/// - `journal_mode=WAL` + `busy_timeout` — the concurrency model (PRD §12.2): many
///   short-lived writers (git hooks, harness hooks, CLI) plus non-blocking readers.
/// - `foreign_keys=ON` — provenance links are real constraints, not conventions.
/// - `synchronous=NORMAL` — safe under WAL and much faster for frequent small writes.
pub fn open(db_path: &Path) -> Result<Connection> {
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
