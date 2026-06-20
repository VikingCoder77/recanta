//! `recanta watch` — keep a project's document folders up to date automatically. Folders
//! passed here are remembered (in `project.json`'s `watch_paths`) so `serve --watch` and a
//! later bare `recanta watch` reuse them. Ingestion is incremental: only new/changed files
//! are (re)stored.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Args;

use crate::project::{Config, Paths};
use crate::watch::{self, WatchTarget};

#[derive(Debug, Args)]
pub struct WatchArgs {
    /// Folders to watch (added to this project's saved watch list). Omit to use the saved list.
    pub paths: Vec<PathBuf>,

    /// Do one incremental ingest of the watched folders and exit (don't keep watching).
    #[arg(long)]
    pub once: bool,
}

pub fn run(args: WatchArgs, project_override: Option<&Path>) -> Result<()> {
    let paths = Paths::discover(project_override)?;
    let mut cfg = Config::load(&paths.config)?;

    // Register any new folders and persist them, so the watch list is sticky.
    if !args.paths.is_empty() {
        for p in &args.paths {
            let canon = p
                .canonicalize()
                .with_context(|| format!("resolving {}", p.display()))?;
            if !canon.is_dir() {
                bail!("not a directory: {}", canon.display());
            }
            if !cfg.watch_paths.contains(&canon) {
                cfg.watch_paths.push(canon);
            }
        }
        cfg.save(&paths.config)?;
    }

    if cfg.watch_paths.is_empty() {
        bail!("nothing to watch — pass a folder: `recanta watch <dir>`");
    }
    let target = WatchTarget { project_root: paths.root.clone(), dirs: cfg.watch_paths.clone() };

    if args.once {
        let stats = watch::catch_up(&target)?;
        report(&stats);
        return Ok(());
    }

    println!("Watching {} folder(s) for {} — Ctrl-C to stop:", target.dirs.len(), cfg.name);
    for d in &target.dirs {
        println!("  {}", d.display());
    }
    watch::run(vec![target], |_root, stats| report(stats))
}

fn report(stats: &crate::documents::IngestStats) {
    if stats.ingested + stats.updated > 0 {
        let mut line = format!("updated: {} new, {} changed", stats.ingested, stats.updated);
        if stats.redactions > 0 {
            line.push_str(&format!(" ({} redacted)", stats.redactions));
        }
        println!("{line}");
    }
}
