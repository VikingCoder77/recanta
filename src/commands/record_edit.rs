//! `recanta record-edit` — record an edit from stdin (PRD §8.4). Fired by the harness
//! PostToolUse hook (JSON payload) or manually (`git diff | recanta record-edit --stdin`).
//! Extracts changed files, links them to indexed symbols (CHANGED_BY), and appends a
//! redacted, idempotent event.

use std::path::Path;

use anyhow::Result;
use clap::Args;

use crate::hash;
use crate::project::Paths;
use crate::{db, eventlog, git, repo};

#[derive(Debug, Args)]
pub struct RecordEditArgs {
    /// Read the edit payload (a diff or a harness JSON event) from stdin.
    #[arg(long)]
    pub stdin: bool,

    /// Source label for provenance (e.g. git-diff, claude-code).
    #[arg(long, default_value = "edit")]
    pub source: String,
}

pub fn run(args: RecordEditArgs, project_override: Option<&Path>) -> Result<()> {
    let payload = if args.stdin { eventlog::read_stdin()? } else { String::new() };
    if payload.trim().is_empty() {
        println!("record-edit: empty payload (pass a diff or event on stdin with --stdin)");
        return Ok(());
    }

    let paths = Paths::discover(project_override)?;
    let conn = db::open_existing(&paths.db)?;
    let project_id = repo::current_project_id(&conn)?;
    let branch = git::current_branch(&paths.root);
    let repo_id = repo::ensure_repository(&conn, &project_id, &paths.root, branch.as_deref())?;

    let key = format!("record-edit:{}", hash::sha256_hex(&payload));
    let rec = eventlog::record(
        &conn, "edit", &args.source, &project_id, Some(repo_id), branch.as_deref(), &key, &payload,
    )?;
    if !rec.inserted {
        println!("record-edit: already recorded");
        return Ok(());
    }

    // Link changed files to indexed symbols (best-effort; pre-commit, no SHA).
    let files = eventlog::changed_paths(&payload);
    let symbols = link_symbols(&conn, repo_id, rec.event_id, &files)?;

    println!(
        "recorded edit ({} file{}, {} symbol{}){}",
        files.len(),
        if files.len() == 1 { "" } else { "s" },
        symbols,
        if symbols == 1 { "" } else { "s" },
        if rec.redactions > 0 {
            format!(" — redacted {} secret(s)", rec.redactions)
        } else {
            String::new()
        }
    );
    Ok(())
}

/// Insert `symbol_changes` for active symbols in the changed files (CHANGED_BY without a
/// commit SHA yet). Matches by exact path or path suffix (payloads may carry absolute paths).
fn link_symbols(conn: &rusqlite::Connection, repo_id: i64, event_id: i64, files: &[String]) -> Result<usize> {
    let mut select = conn.prepare(
        "SELECT id FROM code_symbols
         WHERE repo_id = ?1 AND status = 'active' AND symbol_type NOT IN ('import','module')
           AND (file_path = ?2 OR ?2 LIKE '%' || file_path)",
    )?;
    let mut count = 0;
    for f in files {
        let ids = select
            .query_map(rusqlite::params![repo_id, f], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for id in ids {
            conn.execute(
                "INSERT INTO symbol_changes (symbol_id, event_id, change_type) VALUES (?1, ?2, 'modified')",
                rusqlite::params![id, event_id],
            )?;
            count += 1;
        }
    }
    Ok(count)
}
