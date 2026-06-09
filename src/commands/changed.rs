//! `recanta changed` — show what changed and which indexed symbols it touches (PRD §8.4,
//! §8.10). Defaults to uncommitted working-tree changes; `--against <ref>` compares HEAD
//! to a ref. Useful as a pre-commit / pre-review risk surface.

use std::path::Path;

use anyhow::Result;
use clap::Args;
use rusqlite::Connection;

use crate::output;
use crate::project::Paths;
use crate::repo::{self, Freshness};
use crate::{db, git};

#[derive(Debug, Args)]
pub struct ChangedArgs {
    /// Compare HEAD against this ref instead of showing uncommitted changes.
    #[arg(long)]
    pub against: Option<String>,

    /// Output budget in characters.
    #[arg(long, default_value_t = 1000)]
    pub budget: usize,
}

pub fn run(args: ChangedArgs, project_override: Option<&Path>) -> Result<()> {
    let paths = Paths::discover(project_override)?;
    if !git::is_repo(&paths.root) {
        anyhow::bail!("{} is not a git repository", paths.root.display());
    }
    let conn = db::open_existing(&paths.db)?;
    let project_id = repo::current_project_id(&conn)?;
    let repo_id = repo::ensure_repository(&conn, &project_id, &paths.root, None)?;

    let files: Vec<String> = match &args.against {
        Some(r) => git::diff_names(&paths.root, &[r, "HEAD"]),
        None => {
            let mut v = git::diff_names(&paths.root, &["HEAD"]);
            for f in git::untracked(&paths.root) {
                if !v.contains(&f) {
                    v.push(f);
                }
            }
            v
        }
    };

    if files.is_empty() {
        println!("No changes{}.", args.against.as_ref().map(|r| format!(" vs {r}")).unwrap_or_default());
        return Ok(());
    }

    let mut blocks = Vec::new();
    for file in &files {
        let syms = symbols_in(&conn, repo_id, file)?;
        if syms.is_empty() {
            blocks.push(file.clone());
        } else {
            blocks.push(format!("{file}\n   {}", syms.join(", ")));
        }
    }

    // Warn if the graph is stale, since symbol mapping relies on it.
    if let Freshness::Stale { .. } | Freshness::NotIndexed = repo::freshness(&conn, repo_id, &paths.root)? {
        blocks.push("⚠ code graph may be stale — run `recanta index --changed-only`".into());
    }

    println!("{}", output::pack(blocks, args.budget));
    Ok(())
}

/// Active, non-import/module symbols defined in a file, as `name [type]` strings.
fn symbols_in(conn: &Connection, repo_id: i64, file: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT qualified_name, symbol_type FROM code_symbols
         WHERE repo_id = ?1 AND file_path = ?2 AND status = 'active'
           AND symbol_type NOT IN ('import', 'module')
         ORDER BY start_line",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![repo_id, file], |r| {
            Ok(format!("{} [{}]", r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
