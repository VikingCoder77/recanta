//! `recanta index` — (re)build the code graph (PRD §8.10). v0.1 performs a full
//! reindex; `--changed-only`/`--update` are accepted for forward-compatibility and
//! currently reindex the whole repo.

use std::path::Path;

use anyhow::Result;
use clap::Args;

use crate::project::{Config, Paths};
use crate::{db, git, graph, repo};

#[derive(Debug, Args)]
pub struct IndexArgs {
    /// Update the existing index (alias for the default full reindex).
    #[arg(long)]
    pub update: bool,

    /// Reindex only files changed since the last indexed commit (and uncommitted
    /// changes). Falls back to a full index if nothing was indexed yet.
    #[arg(long = "changed-only")]
    pub changed_only: bool,
}

pub fn run(args: IndexArgs, project_override: Option<&Path>) -> Result<()> {
    let paths = Paths::discover(project_override)?;
    let cfg = Config::load(&paths.config)?;
    let conn = db::open_existing(&paths.db)?;

    let project_id = repo::current_project_id(&conn)?;
    let branch = git::current_branch(&paths.root);
    let repo_id = repo::ensure_repository(&conn, &project_id, &paths.root, branch.as_deref())?;
    let head = git::head_sha(&paths.root);

    let prior = repo::indexed_commit(&conn, repo_id)?;
    let stats = if args.changed_only && prior.is_some() {
        let changed = changed_files(&paths.root, prior.as_deref());
        if changed.is_empty() {
            println!("Index already up to date (no changed files).");
            return Ok(());
        }
        graph::index_changed(&conn, repo_id, &paths.root, &changed, head.as_deref())?
    } else {
        graph::index_repo(&conn, repo_id, &paths.root, &cfg.ignore, head.as_deref())?
    };

    println!(
        "Indexed {} file(s): {} symbol(s), {} import(s){}",
        stats.files,
        stats.symbols,
        stats.imports,
        if stats.deleted > 0 {
            format!(", {} marked deleted", stats.deleted)
        } else {
            String::new()
        }
    );
    match head {
        Some(h) => println!("Index at {}", &h[..h.len().min(10)]),
        None => println!("Index built (no commits yet)"),
    }
    Ok(())
}

/// Files changed between the last indexed commit and HEAD. Committed-only by design: it
/// keeps `indexed_commit` an honest mirror of what's indexed (so reverting an uncommitted
/// edit can't leave a stale symbol). For uncommitted work, run a full `index`.
fn changed_files(root: &Path, base: Option<&str>) -> Vec<String> {
    match base {
        Some(b) => git::diff_names(root, &[b, "HEAD"]),
        None => Vec::new(),
    }
}
