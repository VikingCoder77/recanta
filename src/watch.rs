//! Incremental document auto-updater (v0.5). Watches folders and re-ingests **only the
//! differences** on change — `documents::ingest_paths` is content-hash idempotent, so
//! unchanged files are no-ops and only new/modified files do work.
//!
//! This is optional-daemon territory (sanctioned by the "daemon only for file-watch /
//! Explorer" principle): the synchronous core never depends on it. `recanta watch` runs it
//! headless; `recanta serve --watch` runs it in a background thread and live-refreshes the
//! Explorer.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use notify::{RecursiveMode, Watcher};

use crate::documents::{self, IngestStats};
use crate::project::Paths;
use crate::{db, repo};

/// Folders to watch on behalf of one project store.
#[derive(Debug, Clone)]
pub struct WatchTarget {
    pub project_root: PathBuf,
    pub dirs: Vec<PathBuf>,
}

/// Events arriving within this window are coalesced into one re-ingest pass, so a burst of
/// editor writes triggers a single update rather than dozens.
const DEBOUNCE: Duration = Duration::from_millis(400);

/// Incrementally ingest a target's folders once (used for the initial catch-up and for
/// `--once`). Re-ingesting a whole folder is cheap because unchanged files are skipped.
pub fn catch_up(target: &WatchTarget) -> Result<IngestStats> {
    let paths = Paths::for_root(&target.project_root);
    let conn = db::open_existing(&paths.db)?;
    let project_id = repo::current_project_id(&conn)?;
    documents::ingest_paths(&conn, &project_id, &target.dirs, true)
}

/// Watch every target's folders and re-ingest the affected target on change (after a short
/// debounce). Blocks until interrupted. `on_change` is called after the initial catch-up
/// and after each subsequent re-ingest, with the project root and the resulting stats.
pub fn run(targets: Vec<WatchTarget>, mut on_change: impl FnMut(&Path, &IngestStats)) -> Result<()> {
    // Initial catch-up so anything added while we were away is picked up immediately.
    for t in &targets {
        let stats = catch_up(t)?;
        on_change(&t.project_root, &stats);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .context("starting filesystem watcher")?;
    for t in &targets {
        for d in &t.dirs {
            // A folder that can't be watched (e.g. removed) shouldn't sink the others.
            if let Err(e) = watcher.watch(d, RecursiveMode::Recursive) {
                eprintln!("cannot watch {}: {e}", d.display());
            }
        }
    }

    // Coalesce a burst of events, then re-ingest each touched target once. The loop ends
    // when the watcher is dropped and the channel closes.
    while let Ok(first) = rx.recv() {
        let mut changed: Vec<PathBuf> = Vec::new();
        gather(&mut changed, first);
        while let Ok(ev) = rx.recv_timeout(DEBOUNCE) {
            gather(&mut changed, ev);
        }

        let mut touched: HashSet<usize> = HashSet::new();
        for p in &changed {
            if let Some(i) = targets.iter().position(|t| t.dirs.iter().any(|d| p.starts_with(d))) {
                touched.insert(i);
            }
        }
        for i in touched {
            let t = &targets[i];
            match catch_up(t) {
                Ok(stats) => on_change(&t.project_root, &stats),
                Err(e) => eprintln!("watch ingest error in {}: {e}", t.project_root.display()),
            }
        }
    }
    Ok(())
}

/// Collect the paths from one filesystem event (ignoring watcher errors).
fn gather(out: &mut Vec<PathBuf>, res: notify::Result<notify::Event>) {
    if let Ok(ev) = res {
        out.extend(ev.paths);
    }
}
