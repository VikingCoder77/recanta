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
    /// Update the existing index (v0.1: full reindex).
    #[arg(long)]
    pub update: bool,

    /// Reindex only changed files (v0.1: full reindex).
    #[arg(long = "changed-only")]
    pub changed_only: bool,
}

pub fn run(_args: IndexArgs, project_override: Option<&Path>) -> Result<()> {
    let paths = Paths::discover(project_override)?;
    let cfg = Config::load(&paths.config)?;
    let conn = db::open_existing(&paths.db)?;

    let project_id = repo::current_project_id(&conn)?;
    let branch = git::current_branch(&paths.root);
    let repo_id = repo::ensure_repository(&conn, &project_id, &paths.root, branch.as_deref())?;
    let head = git::head_sha(&paths.root);

    let stats = graph::index_repo(&conn, repo_id, &paths.root, &cfg.ignore, head.as_deref())?;

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
