//! `recanta brief` — compact, task-aware fresh-session briefing (PRD §8.1, §18).
//!
//! Assembles, in priority order so the essentials survive a tight budget: a header
//! (project + branch + dirty state), the active task, known risks, active decisions,
//! durable user preferences, and recent changes. Everything is packed under `--budget`.

use std::path::Path;

use anyhow::Result;
use clap::Args;
use rusqlite::Connection;

use crate::memory::{self, MemType};
use crate::output;
use crate::project::{self, Config, Paths};
use crate::{db, git};

#[derive(Debug, Args)]
pub struct BriefArgs {
    /// Output budget in characters.
    #[arg(long, default_value_t = output::BRIEF_DEFAULT)]
    pub budget: usize,

    /// Optional task focus; surfaces memories relevant to it.
    #[arg(long)]
    pub task: Option<String>,
}

pub fn run(args: BriefArgs, project_override: Option<&Path>) -> Result<()> {
    let paths = Paths::discover(project_override)?;
    let cfg = Config::load(&paths.config)?;
    let conn = db::open_existing(&paths.db)?;

    // Blocks are emitted in this order; `output::pack` keeps as many as fit, so the
    // most important context (header, task) must come first.
    let mut blocks: Vec<String> = Vec::new();
    let branch = git::current_branch(&paths.root);
    let branch = branch.as_deref();

    blocks.push(header(&cfg, &paths.root));
    blocks.push(active_task(&conn, branch)?);

    let risks = section(&conn, &[MemType::Warning, MemType::Bug], "risk", 3, branch)?;
    blocks.extend(risks);

    let decisions = section(&conn, &[MemType::Decision], "decision", 3, branch)?;
    blocks.extend(decisions);

    blocks.extend(user_prefs()?);

    if let Some(task) = &args.task {
        blocks.extend(relevant(&conn, task, branch)?);
    }

    println!("{}", output::pack(blocks, args.budget));
    Ok(())
}

fn header(cfg: &Config, root: &Path) -> String {
    if git::is_repo(root) {
        let branch = git::current_branch(root).unwrap_or_else(|| "(detached)".into());
        let head = git::head_sha(root)
            .map(|s| s[..s.len().min(10)].to_string())
            .unwrap_or_else(|| "(no commits)".into());
        let dirty = match git::is_dirty(root) {
            Some(true) => "dirty",
            Some(false) => "clean",
            None => "unknown",
        };
        format!("project {} · {branch} @ {head} ({dirty})", cfg.name)
    } else {
        format!("project {} · (not a git repository)", cfg.name)
    }
}

fn active_task(conn: &Connection, branch: Option<&str>) -> Result<String> {
    let tasks = memory::by_types(conn, &[MemType::Task], None, branch, 1)?;
    Ok(match tasks.first() {
        Some(t) => format!("task: {} (#{})", t.title, t.id),
        None => "task: none".to_string(),
    })
}

/// Render up to `limit` memories of the given types as `label:` lines.
fn section(
    conn: &Connection,
    types: &[MemType],
    label: &str,
    limit: usize,
    branch: Option<&str>,
) -> Result<Vec<String>> {
    Ok(memory::by_types(conn, types, None, branch, limit)?
        .into_iter()
        .map(|m| format!("{label}: {} (#{})", m.title, m.id))
        .collect())
}

/// Top durable user preferences from the global store (skipped if it doesn't exist).
fn user_prefs() -> Result<Vec<String>> {
    let path = project::global_db_path()?;
    if !path.is_file() {
        return Ok(vec![]);
    }
    let conn = db::open_existing(&path)?;
    Ok(memory::top_user_prefs(&conn, 3)?
        .into_iter()
        .map(|m| format!("pref: {} (#{})", m.title, m.id))
        .collect())
}

/// Memories relevant to a `--task` focus, via FTS.
fn relevant(conn: &Connection, task: &str, branch: Option<&str>) -> Result<Vec<String>> {
    let Some(expr) = memory::fts_query(task) else {
        return Ok(vec![]);
    };
    Ok(memory::search(conn, &expr, None, branch, 3)?
        .into_iter()
        .map(|h| format!("relevant: {} (#{})", h.row.title, h.row.id))
        .collect())
}
