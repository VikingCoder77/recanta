//! Shared helpers for the single project row and its repository rows. Used by
//! `record-commit`, the installer, and the code-graph indexer so identity resolution
//! lives in one place (PRD §11.2).

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::git;

/// The single project row's id (mirrors `project.json`).
pub fn current_project_id(conn: &Connection) -> Result<String> {
    conn.query_row("SELECT id FROM projects LIMIT 1", [], |r| r.get(0))
        .context("no project row (run `recanta init`)")
}

/// Get or create the repository row for `root` (keyed by root path), returning its id.
pub fn ensure_repository(
    conn: &Connection,
    project_id: &str,
    root: &Path,
    default_branch: Option<&str>,
) -> Result<i64> {
    let root_str = root.to_string_lossy().to_string();
    if let Ok(id) = conn.query_row(
        "SELECT id FROM repositories WHERE project_id = ?1 AND root_path = ?2",
        rusqlite::params![project_id, root_str],
        |r| r.get::<_, i64>(0),
    ) {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO repositories (project_id, root_path, root_commit_sha, default_branch)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![project_id, root_str, git::root_commit_sha(root), default_branch],
    )
    .context("creating repository row")?;
    Ok(conn.last_insert_rowid())
}

/// The commit the repo's code graph was last indexed at, if any.
pub fn indexed_commit(conn: &Connection, repo_id: i64) -> Result<Option<String>> {
    Ok(conn.query_row(
        "SELECT indexed_commit FROM repositories WHERE id = ?1",
        [repo_id],
        |r| r.get(0),
    )?)
}

/// Staleness of the code graph relative to the work tree's HEAD (PRD §8.11).
pub enum Freshness {
    /// No commits / not a repo — staleness is undefined.
    Unknown,
    NotIndexed,
    Fresh,
    /// Indexed at `indexed`, but HEAD has moved to `head`.
    Stale { indexed: String, head: String },
}

pub fn freshness(conn: &Connection, repo_id: i64, root: &Path) -> Result<Freshness> {
    let Some(head) = git::head_sha(root) else {
        return Ok(Freshness::Unknown);
    };
    match indexed_commit(conn, repo_id)? {
        None => Ok(Freshness::NotIndexed),
        Some(indexed) if indexed == head => Ok(Freshness::Fresh),
        Some(indexed) => Ok(Freshness::Stale { indexed, head }),
    }
}
