//! `recanta init` — create `.recanta/`, `project.json`, and the SQLite store for a
//! project. Installs no hooks (PRD §9.2: `init` creates identity + defaults; `install`
//! wires hooks separately).

use std::path::Path;

use anyhow::{bail, Context, Result};
use clap::Args;

use crate::db;
use crate::project::{self, Config, Paths};

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Project name (defaults to the directory name).
    #[arg(long)]
    pub name: Option<String>,

    /// Re-initialize even if `.recanta/` already exists (rewrites project.json,
    /// leaves the existing store and its data intact).
    #[arg(long)]
    pub force: bool,
}

pub fn run(args: InitArgs, project_override: Option<&Path>) -> Result<()> {
    let root = match project_override {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir().context("cannot read current directory")?,
    };
    if !root.is_dir() {
        bail!("project root {} is not a directory", root.display());
    }
    let root = root
        .canonicalize()
        .with_context(|| format!("resolving {}", root.display()))?;
    let paths = Paths::for_root(&root);

    if paths.is_initialized() && !args.force {
        bail!(
            "{} already initialized (use --force to rewrite project.json)",
            paths.root.display()
        );
    }

    std::fs::create_dir_all(&paths.dir)
        .with_context(|| format!("creating {}", paths.dir.display()))?;
    std::fs::create_dir_all(&paths.backups)
        .with_context(|| format!("creating {}", paths.backups.display()))?;

    // Preserve identity (id/created_at) across a --force re-init.
    let mut config = if paths.is_initialized() {
        let mut existing = Config::load(&paths.config)?;
        existing.root_commit_sha = crate::git::root_commit_sha(&root);
        existing
    } else {
        project::new_config(&root)?
    };
    if let Some(name) = args.name {
        config.name = name;
    }
    config.save(&paths.config)?;

    // Opening the store creates the file and applies all migrations.
    let conn = db::open(&paths.db)?;
    upsert_project_row(&conn, &config, &root)?;

    // Keep the local store out of version control (it is per-machine, local-first).
    let ignored = ensure_gitignored(&root)?;

    println!("Initialized Recanta in {}", paths.dir.display());
    println!("  project: {} ({})", config.name, config.id);
    match &config.root_commit_sha {
        Some(sha) => println!("  identity: root-commit {}", short(sha)),
        None => println!("  identity: uuid (no git commits yet)"),
    }
    println!("  store:   {}", paths.db.display());
    if ignored {
        println!("  added .recanta/ to .gitignore");
    }
    println!("\nNext: `recanta status`, then `recanta install` to wire up hooks.");
    Ok(())
}

/// Ensure `.recanta/` is git-ignored (the store is local-first, never committed).
/// Non-destructive: appends to an existing `.gitignore`, never rewrites it. Returns
/// whether a change was made.
fn ensure_gitignored(root: &Path) -> Result<bool> {
    if !crate::git::is_repo(root) {
        return Ok(false);
    }
    let path = root.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| {
        let l = l.trim().trim_end_matches('/');
        l == ".recanta"
    }) {
        return Ok(false);
    }
    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str("# Recanta local memory store (per-machine, local-first)\n.recanta/\n");
    std::fs::write(&path, out).with_context(|| format!("updating {}", path.display()))?;
    Ok(true)
}

/// Insert or refresh the single project row mirroring `project.json`.
fn upsert_project_row(conn: &rusqlite::Connection, cfg: &Config, root: &Path) -> Result<()> {
    conn.execute(
        "INSERT INTO projects (id, name, root_path, root_commit_sha, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            root_path = excluded.root_path,
            root_commit_sha = excluded.root_commit_sha,
            last_seen_at = datetime('now')",
        rusqlite::params![
            cfg.id,
            cfg.name,
            root.to_string_lossy(),
            cfg.root_commit_sha,
        ],
    )
    .context("recording project row")?;
    Ok(())
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(10)]
}
