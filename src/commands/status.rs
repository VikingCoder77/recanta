//! `recanta status` — a compact health/identity report (PRD §9.2, §18). Shows
//! project, branch + dirty state, schema version, store size, and item counts. Later
//! milestones add hook-mechanism detection, capture policy, and staleness.

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

use crate::project::{Config, Paths};
use crate::{db, git};

pub fn run(project_override: Option<&Path>) -> Result<()> {
    let paths = Paths::discover(project_override)?;
    let cfg = Config::load(&paths.config)?;
    let conn = db::open_existing(&paths.db)?;

    let schema = db::migrations::current_version(&conn)?;
    let memories: i64 = count(&conn, "memory_items")?;
    let events: i64 = count(&conn, "events")?;
    let symbols: i64 = count(&conn, "code_symbols")?;

    println!("project   {} ({})", cfg.name, cfg.id);
    println!("root      {}", paths.root.display());

    // Git state.
    if git::is_repo(&paths.root) {
        let branch = git::current_branch(&paths.root).unwrap_or_else(|| "(detached)".into());
        let dirty = match git::is_dirty(&paths.root) {
            Some(true) => "dirty",
            Some(false) => "clean",
            None => "unknown",
        };
        let head = git::head_sha(&paths.root)
            .map(|s| short(&s).to_string())
            .unwrap_or_else(|| "(no commits)".into());
        println!("git       {branch} @ {head} ({dirty})");
        match &cfg.root_commit_sha {
            Some(sha) => println!("identity  root-commit {}", short(sha)),
            None => println!("identity  uuid (no root commit yet)"),
        }
    } else {
        println!("git       not a git repository");
    }

    println!("schema    v{schema} (binary supports v{})", db::migrations::latest_version());
    println!("store     {} ({})", paths.db.display(), human_size(file_size(&paths.db)));
    println!("memory    {memories} item(s)");
    println!("events    {events} recorded");
    println!("symbols   {symbols} indexed");
    Ok(())
}

fn count(conn: &Connection, table: &str) -> Result<i64> {
    // Table name is a fixed literal from this module, never user input.
    let sql = format!("SELECT COUNT(*) FROM {table}");
    Ok(conn.query_row(&sql, [], |r| r.get(0))?)
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(10)]
}
