//! `recanta serve` — start the local, read-only Explorer web UI (PRD §15). Binds
//! `127.0.0.1` by default; no auth, no telemetry, no write endpoints.

use std::path::Path;

use anyhow::{Context, Result};
use clap::Args;

use crate::explorer::{self, ProjectStore};
use crate::project::Paths;
use crate::workspace::Workspace;

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Port to bind.
    #[arg(long, default_value_t = 7077)]
    pub port: u16,

    /// Host to bind (keep `127.0.0.1` for local-only access).
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Don't open a browser window.
    #[arg(long)]
    pub no_open: bool,

    /// Show only the current project, even if workspace mode is on.
    #[arg(long)]
    pub single: bool,
}

pub fn run(args: ServeArgs, project_override: Option<&Path>) -> Result<()> {
    let stores = build_stores(project_override, args.single)?;
    explorer::run(stores, &args.host, args.port, !args.no_open)
}

/// Decide which stores to serve. Workspace mode (the default) opens every registered,
/// still-initialized project; it degrades to a single project when `--project`/`--single`
/// is given, the registry is empty or disabled, or nothing in it can be opened.
fn build_stores(project_override: Option<&Path>, single: bool) -> Result<Vec<ProjectStore>> {
    // An explicit project, or `--single`, means just that one project.
    if project_override.is_some() || single {
        let paths = Paths::discover(project_override)?;
        return Ok(vec![ProjectStore::open(&paths.root, "p0".into())?]);
    }

    let ws = Workspace::load().unwrap_or_default();
    if ws.enabled {
        let mut roots: Vec<std::path::PathBuf> =
            ws.live_projects().into_iter().map(|p| p.path).collect();
        // Include the current project even if it isn't registered yet (e.g. `--no-register`),
        // so `serve` here always shows at least where you're standing.
        if let Ok(here) = Paths::discover(None) {
            let canon = std::fs::canonicalize(&here.root).unwrap_or(here.root.clone());
            if !roots.iter().any(|r| std::fs::canonicalize(r).unwrap_or_else(|_| r.clone()) == canon) {
                roots.push(here.root);
            }
        }

        let mut stores = Vec::new();
        for (i, root) in roots.iter().enumerate() {
            match ProjectStore::open(root, format!("p{i}")) {
                Ok(s) => stores.push(s),
                // A registered project whose store is unreadable shouldn't sink the whole UI.
                Err(e) => eprintln!("skipping {}: {e}", root.display()),
            }
        }
        if !stores.is_empty() {
            return Ok(stores);
        }
    }

    // Workspace empty/disabled/unopenable → fall back to the current project.
    let paths = Paths::discover(project_override)
        .context("no project here and no workspace to serve (run `recanta init`)")?;
    Ok(vec![ProjectStore::open(&paths.root, "p0".into())?])
}
