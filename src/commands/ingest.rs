//! `recanta ingest` — bring documents (Markdown/text/PDF/Word) into project memory as
//! redacted evidence (general AIOS memory). Explicit and user-directed, so always
//! allowed; redaction runs on every document (PRD §8.12a).

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Args;

use crate::project::Paths;
use crate::{db, documents, repo};

#[derive(Debug, Args)]
pub struct IngestArgs {
    /// Files or directories to ingest.
    #[arg(required = true)]
    pub paths: Vec<PathBuf>,

    /// Recurse into subdirectories when a path is a directory.
    #[arg(long, short)]
    pub recursive: bool,
}

pub fn run(args: IngestArgs, project_override: Option<&Path>) -> Result<()> {
    let paths = Paths::discover(project_override)?;
    let conn = db::open_existing(&paths.db)?;
    let project_id = repo::current_project_id(&conn)?;

    let stats = documents::ingest_paths(&conn, &project_id, &args.paths, args.recursive)?;

    println!(
        "Ingested {} new, {} updated, {} unchanged, {} skipped.",
        stats.ingested, stats.updated, stats.unchanged, stats.skipped
    );
    if stats.redactions > 0 {
        println!("Redacted {} secret(s) before storage.", stats.redactions);
    }
    Ok(())
}
