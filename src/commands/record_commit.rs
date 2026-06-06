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
use crate::{db, git, hash, redact, repo};

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

    // Redact the commit message before it is stored anywhere (PRD §8.12a).
    let subject = redact::redact(&meta.subject);

    let conn = db::open_existing(&paths.db)?;
    let project_id = repo::current_project_id(&conn)?;
    let repo_id = repo::ensure_repository(&conn, &project_id, &paths.root, branch.as_deref())?;

    // Idempotency: the SHA already is a content digest, so key on it directly. A
    // duplicate hook fire collides on the UNIQUE key and inserts nothing (PRD §12.2).
    let idempotency_key = format!("record-commit:{sha}");
    let payload = serde_json::json!({
        "subject": subject.text,
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
    let event_id = conn.last_insert_rowid();

    // Record what was redacted, for the audit trail (PRD §8.12a).
    for hit in &subject.hits {
        conn.execute(
            "INSERT INTO redaction_audit (event_id, pattern_id, path, span)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                event_id,
                hit.pattern_id,
                "commit-message",
                format!("{}-{}", hit.start, hit.end)
            ],
        )?;
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
            subject.text,
            meta.timestamp,
            parents
        ],
    )
    .context("recording commit row")?;

    // Link the commit to indexed symbols in the changed files (CHANGED_BY, PRD §8.10).
    // Best-effort: only matches symbols already in the code graph.
    let changed_symbols = link_changed_symbols(&conn, repo_id, &sha, event_id, &files)?;

    println!(
        "recorded commit {} \"{}\" ({} file{}, {} symbol{}){}",
        short(&sha),
        truncate(&subject.text, 60),
        files.len(),
        if files.len() == 1 { "" } else { "s" },
        changed_symbols,
        if changed_symbols == 1 { "" } else { "s" },
        if subject.hits.is_empty() {
            String::new()
        } else {
            format!(" — redacted {} secret(s)", subject.hits.len())
        }
    );
    Ok(())
}

/// Insert `symbol_changes` rows linking the commit to active symbols defined in the
/// changed files. Returns the number of symbols linked.
fn link_changed_symbols(
    conn: &Connection,
    repo_id: i64,
    sha: &str,
    event_id: i64,
    files: &[String],
) -> Result<usize> {
    let mut count = 0;
    let mut select = conn.prepare(
        "SELECT id FROM code_symbols
         WHERE repo_id = ?1 AND file_path = ?2 AND status = 'active'
           AND symbol_type NOT IN ('import', 'module')",
    )?;
    for file in files {
        let ids = select
            .query_map(rusqlite::params![repo_id, file], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for id in ids {
            conn.execute(
                "INSERT INTO symbol_changes (symbol_id, commit_sha, event_id, change_type)
                 VALUES (?1, ?2, ?3, 'modified')",
                rusqlite::params![id, sha, event_id],
            )?;
            count += 1;
        }
    }
    Ok(count)
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
