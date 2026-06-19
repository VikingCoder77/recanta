//! `recanta status` — a compact health/identity report (PRD §9.2, §18). Shows
//! project, branch + dirty state, schema version, store size, and item counts. Later
//! milestones add hook-mechanism detection, capture policy, and staleness.

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

use crate::project::{Config, Paths};
use crate::repo::{self, Freshness};
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

    // Code graph freshness (PRD §8.11, §18).
    if let Ok(project_id) = repo::current_project_id(&conn) {
        if let Ok(repo_id) = repo::ensure_repository(&conn, &project_id, &paths.root, None) {
            let line = match repo::freshness(&conn, repo_id, &paths.root)? {
                Freshness::Fresh => "fresh".to_string(),
                Freshness::NotIndexed => "not built (run `recanta index`)".to_string(),
                Freshness::Unknown => "n/a (no commits)".to_string(),
                Freshness::Stale { indexed, head } => format!(
                    "stale: indexed {} vs HEAD {} (run `recanta index`)",
                    short(&indexed),
                    short(&head)
                ),
            };
            println!("index     {line}");
        }
    }

    println!("schema    v{schema} (binary supports v{})", db::migrations::latest_version());
    println!("store     {} ({})", paths.db.display(), human_size(file_size(&paths.db)));
    println!("memory    {memories} item(s)");
    println!("events    {events} recorded");
    println!("symbols   {symbols} indexed");

    // Capture policy + redaction audit (PRD §8.12, §18).
    let raw = if cfg.capture.raw_transcripts { "raw transcripts ON" } else { "summaries only" };
    println!("capture   {raw} (retention {}d)", cfg.capture.retention_days);
    let redactions: i64 = count(&conn, "redaction_audit")?;
    println!("redacted  {redactions} secret(s) caught before storage");

    // Embeddings / semantic search (PRD §12.1).
    if cfg.embeddings.enabled {
        let vecs: i64 = count(&conn, "memory_vec").unwrap_or(0);
        println!(
            "embed     on · {} vectors via {}/{} (dim {})",
            vecs, cfg.embeddings.provider, cfg.embeddings.model, cfg.embeddings.dim
        );
    } else {
        println!("embed     off (FTS-only; run `recanta embed` to enable semantic search)");
    }
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
