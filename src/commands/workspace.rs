//! `recanta workspace` — manage the cross-project registry the Explorer aggregates.
//!
//! `init` registers projects automatically; this command is for inspecting and adjusting
//! that list, and for turning workspace mode off entirely (so `serve` shows only the
//! current project).

use std::path::Path;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use crate::project::Paths;
use crate::workspace::Workspace;

#[derive(Debug, Args)]
pub struct WorkspaceArgs {
    #[command(subcommand)]
    pub command: WorkspaceCmd,
}

#[derive(Debug, Subcommand)]
pub enum WorkspaceCmd {
    /// List registered projects and whether workspace mode is on.
    List,
    /// Register a project directory (must already be `recanta init`-ed).
    Add { path: std::path::PathBuf },
    /// Remove a project directory from the workspace.
    Remove { path: std::path::PathBuf },
    /// Turn workspace mode on (the default): `serve` shows all registered projects.
    Enable,
    /// Turn workspace mode off: `serve` shows only the current project.
    Disable,
}

pub fn run(args: WorkspaceArgs, _project_override: Option<&Path>) -> Result<()> {
    let mut ws = Workspace::load()?;
    match args.command {
        WorkspaceCmd::List => list(&ws),
        WorkspaceCmd::Add { path } => {
            let root = path.canonicalize().with_context(|| format!("resolving {}", path.display()))?;
            if !Paths::for_root(&root).is_initialized() {
                anyhow::bail!("{} is not a Recanta project (run `recanta init` there first)", root.display());
            }
            if ws.register(&root) {
                ws.save()?;
                println!("Added {} to the workspace.", root.display());
            } else {
                println!("{} is already in the workspace.", root.display());
            }
        }
        WorkspaceCmd::Remove { path } => {
            // Don't require canonicalization to succeed — a deleted dir should still be removable.
            let root = path.canonicalize().unwrap_or(path);
            if ws.remove(&root) {
                ws.save()?;
                println!("Removed {} from the workspace.", root.display());
            } else {
                println!("{} was not in the workspace.", root.display());
            }
        }
        WorkspaceCmd::Enable => {
            ws.enabled = true;
            ws.save()?;
            println!("Workspace mode on — `recanta serve` shows all registered projects.");
        }
        WorkspaceCmd::Disable => {
            ws.enabled = false;
            ws.save()?;
            println!("Workspace mode off — `recanta serve` shows only the current project.");
        }
    }
    Ok(())
}

fn list(ws: &Workspace) {
    println!("Workspace mode: {}", if ws.enabled { "on" } else { "off (serve shows current project only)" });
    if ws.projects.is_empty() {
        println!("No projects registered yet (run `recanta init` in a project, or `recanta workspace add <path>`).");
        return;
    }
    println!("Registered projects ({}):", ws.projects.len());
    for p in &ws.projects {
        let live = if Paths::for_root(&p.path).is_initialized() { " " } else { " (missing store) " };
        println!("  {}{}{}", p.name, live, p.path.display());
    }
}
