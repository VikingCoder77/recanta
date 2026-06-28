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
    /// Harness to import from: claude-code, codex, gemini, opencode, antigravity, or all.
    #[arg(long, default_value = "all")]
    pub harness: String,

    /// Override the directory of session files (single-harness imports only).
    #[arg(long)]
    pub from: Option<PathBuf>,
}

pub fn run(args: ImportSessionsArgs, project_override: Option<&Path>) -> Result<()> {
    let valid = sessions::HARNESSES.contains(&args.harness.as_str()) || args.harness == "all";
    if !valid {
        bail!(
            "unknown harness `{}` (choose: {}, or all)",
            args.harness,
            sessions::HARNESSES.join(", ")
        );
    }
    if args.from.is_some() && args.harness == "all" {
        bail!("--from requires a single --harness (it points at one harness's files)");
    }

    let paths = Paths::discover(project_override)?;
    let cfg = Config::load(&paths.config)?;
    let conn = db::open_existing(&paths.db)?;
    let project_id = repo::current_project_id(&conn)?;

    let home = home_dir()?;
    let capture_raw = cfg.capture.raw_transcripts;
    let stats = sessions::import(
        &conn,
        &project_id,
        &home,
        &paths.root,
        capture_raw,
        &args.harness,
        args.from.as_deref(),
    )?;

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
             to keep them as searchable evidence."
        );
    }
    for note in &stats.notes {
        println!("  · {note}");
    }
    Ok(())
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("cannot locate home directory (set HOME)"))
}
