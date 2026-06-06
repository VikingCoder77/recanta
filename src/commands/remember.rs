//! `recanta remember` — record a durable memory (PRD §8.8, §8.9). Routes to the global
//! store for `user` scope and the project store otherwise.

use std::path::Path;

use anyhow::{bail, Result};
use clap::Args;

use crate::memory::{self, Importance, MemType, NewMemory, Scope};
use crate::project::{self, Paths};
use crate::{db, git};

#[derive(Debug, Args)]
pub struct RememberArgs {
    /// The memory content.
    pub content: String,

    /// Scope (default: project). `user` is stored globally and reused across projects.
    #[arg(long, value_enum, default_value = "project")]
    pub scope: Scope,

    /// Memory type (default: semantic).
    #[arg(long = "type", value_enum, default_value = "semantic")]
    pub mem_type: MemType,

    /// Importance (default: normal).
    #[arg(long, value_enum, default_value = "normal")]
    pub importance: Importance,

    /// Optional explicit title (otherwise derived from the content).
    #[arg(long)]
    pub title: Option<String>,
}

pub fn run(args: RememberArgs, project_override: Option<&Path>) -> Result<()> {
    let title = args.title.unwrap_or_else(|| derive_title(&args.content));
    let mut new = NewMemory {
        mem_type: args.mem_type,
        scope: args.scope,
        title,
        content: args.content,
        importance: args.importance,
        confidence: 1.0,
        branch: None,
    };

    // user scope → global store (no project needed); everything else → project store.
    let (conn, suffix) = if args.scope.is_global() {
        let path = project::global_db_path()?;
        (db::open(&path)?, " in global store".to_string())
    } else {
        let paths = Paths::discover(project_override)?;
        if matches!(args.scope, Scope::Branch) {
            new.branch = git::current_branch(&paths.root);
            if new.branch.is_none() {
                bail!("--scope branch requires a current branch (detached HEAD / no commits)");
            }
        }
        (db::open_existing(&paths.db)?, String::new())
    };

    let id = memory::insert(&conn, &new)?;
    println!(
        "Remembered #{id} [{}/{}]{suffix}",
        new.scope.as_str(),
        new.mem_type.as_str()
    );
    Ok(())
}

/// First sentence/line of the content, trimmed to a reasonable title length.
fn derive_title(content: &str) -> String {
    let first = content
        .split(['.', '\n'])
        .next()
        .unwrap_or(content)
        .trim();
    let base = if first.is_empty() { content.trim() } else { first };
    let mut t: String = base.chars().take(80).collect();
    if base.chars().count() > 80 {
        t.push('…');
    }
    t
}
