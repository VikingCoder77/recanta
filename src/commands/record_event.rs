//! `recanta record-event` — record a harness/workflow event from stdin (PRD §8.4).
//! Fired by harness Stop/SessionEnd hooks (`recanta record-event --type harness.stop
//! --stdin`). Appends a redacted, idempotent event; payload on stdin is optional.

use std::path::Path;

use anyhow::Result;
use clap::Args;

use crate::hash;
use crate::project::Paths;
use crate::{db, eventlog, git, repo};

#[derive(Debug, Args)]
pub struct RecordEventArgs {
    /// Event type (e.g. harness.stop, harness.session_end).
    #[arg(long = "type", default_value = "harness.event")]
    pub event_type: String,

    /// Read an optional payload from stdin.
    #[arg(long)]
    pub stdin: bool,

    /// Source label for provenance (e.g. claude-code).
    #[arg(long, default_value = "harness")]
    pub source: String,
}

pub fn run(args: RecordEventArgs, project_override: Option<&Path>) -> Result<()> {
    let payload = if args.stdin { eventlog::read_stdin()? } else { String::new() };

    let paths = Paths::discover(project_override)?;
    let conn = db::open_existing(&paths.db)?;
    let project_id = repo::current_project_id(&conn)?;
    let branch = git::current_branch(&paths.root);
    let repo_id = repo::ensure_repository(&conn, &project_id, &paths.root, branch.as_deref())?;

    // Key on type + payload digest so repeated identical fires collapse, but distinct
    // events (different payloads) are all kept.
    let key = format!("record-event:{}:{}", args.event_type, hash::sha256_hex(&payload));
    let rec = eventlog::record(
        &conn, &args.event_type, &args.source, &project_id, Some(repo_id), branch.as_deref(), &key, &payload,
    )?;

    if rec.inserted {
        println!(
            "recorded event {}{}",
            args.event_type,
            if rec.redactions > 0 { format!(" — redacted {} secret(s)", rec.redactions) } else { String::new() }
        );
    } else {
        println!("event {} already recorded", args.event_type);
    }
    Ok(())
}
