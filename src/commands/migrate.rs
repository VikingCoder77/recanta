//! `recanta migrate` — apply pending SQLite schema migrations (PRD §11.3). Opening the
//! store runs the forward-only migration runner; this command exists so the upgrade path
//! is explicit (`open_existing` refers users here when the schema is behind).

use std::path::Path;

use anyhow::Result;
use clap::Args;

use crate::db::migrations;
use crate::project::Paths;
use crate::db;

#[derive(Debug, Args)]
pub struct MigrateArgs {}

pub fn run(_args: MigrateArgs, project_override: Option<&Path>) -> Result<()> {
    let paths = Paths::discover(project_override)?;
    // `db::open` applies any pending migrations as a side effect.
    let conn = db::open(&paths.db)?;
    let version = migrations::current_version(&conn)?;
    println!("Schema up to date at v{version} (binary supports v{})", migrations::latest_version());
    Ok(())
}
