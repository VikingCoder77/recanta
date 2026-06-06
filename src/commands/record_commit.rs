//! `recanta record-commit` — ingest a commit's metadata (PRD §8.4). Fired by the
//! installed `post-commit` hook. Writes an idempotent append-only event plus a row in
//! `commits`; re-firing the same SHA is a no-op (PRD §12.2, §18).
//!
//! Changed-symbol extraction arrives with the tree-sitter code graph (PRD §8.10); for
//! now this records SHA, branch, author (hashed), message, and changed files.

use std::path::Path;

use anyhow::{bail, Context, Result};
use clap::Args;
use rusqlite::Connection;

use crate::project::Paths;
use crate::{db, git, hash};

#[derive(Debug, Args)]
pub struct RecordCommitArgs {
    /// Revision to record (default: HEAD).
    #[arg(long, default_value = "HEAD")]
    pub commit: String,
}

pub fn run(args: RecordCommitArgs, project_override: Option<&Path>) -> Result<()> {
    let paths = Paths::discover(project_override)?;
    if !git::is_repo(&paths.root) {
        bail!("{} is not a git repository", paths.root.display());
    }
    let sha = git::resolve_commit(&paths.root, &args.commit)
        .with_context(|| format!("resolving commit {:?}", args.commit))?;
    let meta = git::commit_meta(&paths.root, &sha)
        .with_context(|| format!("reading commit {sha}"))?;
    let files = git::changed_files(&paths.root, &sha);
    let branch = git::current_branch(&paths.root);

    let conn = db::open_existing(&paths.db)?;
    let project_id = current_project_id(&conn)?;
    let repo_id = ensure_repository(&conn, &project_id, &paths.root, branch.as_deref())?;

    // Idempotency: the SHA already is a content digest, so key on it directly. A
    // duplicate hook fire collides on the UNIQUE key and inserts nothing (PRD §12.2).
    let idempotency_key = format!("record-commit:{sha}");
    let payload = serde_json::json!({
        "subject": meta.subject,
        "changed_files": files,
        "file_count": files.len(),
    })
    .to_string();
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO events
            (idempotency_key, type, source, project_id, repo_id, branch, commit_sha,
             payload_redacted, capture_mode)
         VALUES (?1, 'git.commit', 'git-hook', ?2, ?3, ?4, ?5, ?6, 'summary')",
        rusqlite::params![idempotency_key, project_id, repo_id, branch, sha, payload],
    )?;

    if inserted == 0 {
        println!("commit {} already recorded", short(&sha));
        return Ok(());
    }

    let author_hash = hash::short_token(&meta.author_email);
    let parents = serde_json::to_string(&meta.parents)?;
    conn.execute(
        "INSERT INTO commits (sha, repo_id, branch, author_hash, message, timestamp, parent_shas)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(sha) DO UPDATE SET
            repo_id = excluded.repo_id,
            branch = excluded.branch,
            author_hash = excluded.author_hash,
            message = excluded.message,
            timestamp = excluded.timestamp,
            parent_shas = excluded.parent_shas",
        rusqlite::params![
            sha,
            repo_id,
            branch,
            author_hash,
            meta.subject,
            meta.timestamp,
            parents
        ],
    )
    .context("recording commit row")?;

    println!(
        "recorded commit {} \"{}\" ({} file{})",
        short(&sha),
        truncate(&meta.subject, 60),
        files.len(),
        if files.len() == 1 { "" } else { "s" }
    );
    Ok(())
}

/// The single project row's id (mirrors `project.json`).
fn current_project_id(conn: &Connection) -> Result<String> {
    conn.query_row("SELECT id FROM projects LIMIT 1", [], |r| r.get(0))
        .context("no project row (run `recanta init`)")
}

/// Get or create the repository row for this work tree, keyed by root path.
fn ensure_repository(
    conn: &Connection,
    project_id: &str,
    root: &Path,
    default_branch: Option<&str>,
) -> Result<i64> {
    let root_str = root.to_string_lossy().to_string();
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM repositories WHERE project_id = ?1 AND root_path = ?2",
            rusqlite::params![project_id, root_str],
            |r| r.get(0),
        )
        .ok();
    if let Some(id) = existing {
        return Ok(id);
    }
    let root_commit = git::root_commit_sha(root);
    conn.execute(
        "INSERT INTO repositories (project_id, root_path, root_commit_sha, default_branch)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![project_id, root_str, root_commit, default_branch],
    )
    .context("creating repository row")?;
    Ok(conn.last_insert_rowid())
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(10)]
}

fn truncate(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or("");
    if line.chars().count() <= max {
        line.to_string()
    } else {
        line.chars().take(max).collect::<String>() + "…"
    }
}
