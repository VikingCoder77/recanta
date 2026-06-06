//! Non-destructive installer (PRD §8.5, §10). Plans changes, shows a dry-run by
//! default, backs up every file it touches, edits only inside managed blocks (or, for
//! JSON, marker-tagged entries), chains existing hooks, and records what it did in
//! `hook_installations` so uninstall is exact.

pub mod managed;
mod claude;
mod git_hook;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args;
use rusqlite::Connection;

use crate::project::{self, Paths};
use crate::db;

/// One planned file change.
pub struct Action {
    pub harness: &'static str,
    pub mechanism: String,
    pub path: PathBuf,
    pub verb: Verb,
    pub new_content: String,
    pub executable: bool,
    pub block_id: String,
    /// Concise human description of the change.
    pub preview: String,
}

#[derive(Clone, Copy)]
pub enum Verb {
    Create,
    Update,
}

/// The accumulated result of planning one or more harnesses.
#[derive(Default)]
pub struct Plan {
    pub actions: Vec<Action>,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
}

impl Plan {
    fn extend(&mut self, other: Plan) {
        self.actions.extend(other.actions);
        self.warnings.extend(other.warnings);
        self.notes.extend(other.notes);
    }
}

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Comma-separated harnesses to install (v0.1 supports `git`, `claude-code`).
    #[arg(long, default_value = "git,claude-code")]
    pub harness: String,

    /// Apply the changes (default is a dry-run preview).
    #[arg(long)]
    pub apply: bool,

    /// Preview only, even if `--apply` is also given.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct UninstallArgs {
    /// Preview only (default for uninstall is to apply, with backups).
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: InstallArgs, project_override: Option<&Path>) -> Result<()> {
    let paths = Paths::discover(project_override)?;
    let root = &paths.root;

    let mut plan = Plan::default();
    for harness in args.harness.split(',').map(str::trim).filter(|h| !h.is_empty()) {
        match harness {
            "git" => plan.extend(git_hook::plan(root)),
            "claude-code" => plan.extend(claude::plan(root)?),
            other => plan
                .warnings
                .push(format!("unknown harness `{other}` (v0.1: git, claude-code)")),
        }
    }

    println!("Recanta install plan for {}\n", root.display());
    let apply = args.apply && !args.dry_run;

    if plan.actions.is_empty() {
        print_messages(&plan);
        println!("\nNothing to install.");
        return Ok(());
    }

    if !apply {
        for a in &plan.actions {
            println!("  [{}] {}", verb_word(a.verb), a.preview);
        }
        print_messages(&plan);
        println!("\nDry run — re-run with --apply to write these changes.");
        return Ok(());
    }

    let conn = db::open_existing(&paths.db)?;
    let project_id = current_project_id(&conn)?;
    let stamp = file_stamp();
    for a in &plan.actions {
        apply_action(&conn, &project_id, &paths, a, &stamp)?;
        println!("  ✓ {}", a.preview);
    }
    print_messages(&plan);
    println!("\nInstalled. Backups (if any) are under {}", paths.backups.display());
    Ok(())
}

pub fn uninstall(args: UninstallArgs, project_override: Option<&Path>) -> Result<()> {
    let paths = Paths::discover(project_override)?;
    let conn = db::open_existing(&paths.db)?;
    let project_id = current_project_id(&conn)?;

    let rows = installed_rows(&conn, &project_id)?;
    if rows.is_empty() {
        println!("No Recanta installations recorded for this project.");
        return Ok(());
    }

    println!("Recanta uninstall plan for {}\n", paths.root.display());
    let stamp = file_stamp();
    let mut removed = 0;
    for row in &rows {
        let Ok(text) = std::fs::read_to_string(&row.file_path) else {
            println!("  - {} (file gone, skipping)", row.file_path.display());
            continue;
        };
        let new_content = if row.harness == "claude-code" {
            claude::strip(&text)?
        } else {
            managed::remove_block(&text)
        };
        match new_content {
            Some(content) => {
                if args.dry_run {
                    println!("  [remove] managed block from {}", row.file_path.display());
                } else {
                    managed::backup(&row.file_path, &paths.backups, &stamp)?;
                    std::fs::write(&row.file_path, content)
                        .with_context(|| format!("writing {}", row.file_path.display()))?;
                    conn.execute(
                        "UPDATE hook_installations SET status='removed' WHERE id=?1",
                        [row.id],
                    )?;
                    println!("  ✓ removed managed block from {}", row.file_path.display());
                    removed += 1;
                }
            }
            None => println!("  - {} (no managed block found)", row.file_path.display()),
        }
    }
    if args.dry_run {
        println!("\nDry run — re-run without --dry-run to apply.");
    } else {
        println!("\nRemoved {removed} managed block(s).");
    }
    Ok(())
}

fn apply_action(
    conn: &Connection,
    project_id: &str,
    paths: &Paths,
    a: &Action,
    stamp: &str,
) -> Result<()> {
    if let Some(parent) = a.path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let backup = managed::backup(&a.path, &paths.backups, stamp)?;
    std::fs::write(&a.path, &a.new_content)
        .with_context(|| format!("writing {}", a.path.display()))?;
    if a.executable {
        make_executable(&a.path)?;
    }
    record_installation(conn, project_id, a, backup.as_deref())?;
    Ok(())
}

fn record_installation(
    conn: &Connection,
    project_id: &str,
    a: &Action,
    backup: Option<&Path>,
) -> Result<()> {
    // Idempotent on re-apply: replace any prior record for this exact block.
    conn.execute(
        "DELETE FROM hook_installations
         WHERE project_id=?1 AND file_path=?2 AND managed_block_id=?3",
        rusqlite::params![project_id, a.path.to_string_lossy(), a.block_id],
    )?;
    conn.execute(
        "INSERT INTO hook_installations
            (project_id, harness, file_path, mechanism, managed_block_id, backup_path, version, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'installed')",
        rusqlite::params![
            project_id,
            a.harness,
            a.path.to_string_lossy(),
            a.mechanism,
            a.block_id,
            backup.map(|p| p.to_string_lossy().to_string()),
            env!("CARGO_PKG_VERSION"),
        ],
    )?;
    Ok(())
}

struct InstalledRow {
    id: i64,
    harness: String,
    file_path: PathBuf,
}

fn installed_rows(conn: &Connection, project_id: &str) -> Result<Vec<InstalledRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, harness, file_path FROM hook_installations
         WHERE project_id=?1 AND status='installed' ORDER BY id",
    )?;
    let rows = stmt
        .query_map([project_id], |r| {
            Ok(InstalledRow {
                id: r.get(0)?,
                harness: r.get(1)?,
                file_path: PathBuf::from(r.get::<_, String>(2)?),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn current_project_id(conn: &Connection) -> Result<String> {
    conn.query_row("SELECT id FROM projects LIMIT 1", [], |r| r.get(0))
        .context("no project row (run `recanta init`)")
}

fn print_messages(plan: &Plan) {
    for n in &plan.notes {
        println!("  · {n}");
    }
    for w in &plan.warnings {
        println!("  ! {w}");
    }
}

fn verb_word(v: Verb) -> &'static str {
    match v {
        Verb::Create => "create",
        Verb::Update => "update",
    }
}

/// Filesystem-safe timestamp for backup filenames.
fn file_stamp() -> String {
    project::now_rfc3339().replace([':', '.'], "-")
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
        .with_context(|| format!("chmod +x {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}
