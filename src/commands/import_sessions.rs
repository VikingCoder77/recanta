//! `recanta import-sessions` — import agent transcripts for this project (PRD §8.6,
//! general AIOS memory). v0.1 supports Claude Code. Explicit opt-in: derived memories
//! are always extracted; the raw transcript is stored only if the capture policy
//! enables it (`recanta capture enable`).

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use clap::Args;

use crate::project::{Config, Paths};
use crate::{db, repo, sessions};

#[derive(Debug, Args)]
pub struct ImportSessionsArgs {
    /// Harness to import from (v0.1: claude-code).
    #[arg(long, default_value = "claude-code")]
    pub harness: String,

    /// Override the directory of session files (otherwise derived from the project).
    #[arg(long)]
    pub from: Option<PathBuf>,
}

pub fn run(args: ImportSessionsArgs, project_override: Option<&Path>) -> Result<()> {
    if args.harness != "claude-code" {
        bail!("harness `{}` not supported yet (v0.1: claude-code)", args.harness);
    }
    let paths = Paths::discover(project_override)?;
    let cfg = Config::load(&paths.config)?;
    let conn = db::open_existing(&paths.db)?;
    let project_id = repo::current_project_id(&conn)?;

    let home = home_dir()?;
    let capture_raw = cfg.capture.raw_transcripts;
    let stats = sessions::import_claude(
        &conn,
        &project_id,
        &home,
        &paths.root,
        capture_raw,
        args.from.as_deref(),
    )?;

    if let Some(dir) = &stats.no_session_dir {
        println!("No Claude Code sessions found at {}", dir.display());
        println!("(point at a directory with --from, or this project has no recorded sessions yet)");
        return Ok(());
    }

    println!(
        "Imported {} session(s) ({} skipped as already imported); created {} memory item(s).",
        stats.imported, stats.skipped, stats.memories
    );
    if stats.redactions > 0 {
        println!("Redacted {} secret(s) before storage.", stats.redactions);
    }
    if !capture_raw && stats.imported > 0 {
        println!(
            "Raw transcripts were NOT stored (capture off). Run `recanta capture enable` \
             to keep them as evidence."
        );
    }
    Ok(())
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("cannot locate home directory (set HOME)"))
}
