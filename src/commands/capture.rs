//! `recanta capture` — manage the capture policy (PRD §8.12b). Raw transcript capture
//! is off by default; enabling it is the explicit, auditable opt-in that lets session
//! import (and, later, harness hooks) store raw transcripts as evidence.

use std::path::Path;

use anyhow::Result;
use clap::{Args, Subcommand};

use crate::project::{Config, Paths};

#[derive(Debug, Args)]
pub struct CaptureArgs {
    #[command(subcommand)]
    pub action: Option<CaptureAction>,
}

#[derive(Debug, Subcommand)]
pub enum CaptureAction {
    /// Enable raw transcript capture (stored redacted, under retention).
    Enable,
    /// Disable raw transcript capture (the default).
    Disable,
    /// Show the current capture policy.
    Status,
}

pub fn run(args: CaptureArgs, project_override: Option<&Path>) -> Result<()> {
    let paths = Paths::discover(project_override)?;
    let mut cfg = Config::load(&paths.config)?;

    match args.action.unwrap_or(CaptureAction::Status) {
        CaptureAction::Enable => {
            cfg.capture.raw_transcripts = true;
            cfg.save(&paths.config)?;
            println!(
                "Raw transcript capture ENABLED (retention {} days). Captured material is \
                 redacted before storage.",
                cfg.capture.retention_days
            );
        }
        CaptureAction::Disable => {
            cfg.capture.raw_transcripts = false;
            cfg.save(&paths.config)?;
            println!("Raw transcript capture DISABLED. Only derived summaries are kept.");
        }
        CaptureAction::Status => {
            let state = if cfg.capture.raw_transcripts { "ON" } else { "OFF (default)" };
            println!("raw transcript capture: {state}");
            println!("retention:              {} days", cfg.capture.retention_days);
        }
    }
    Ok(())
}
