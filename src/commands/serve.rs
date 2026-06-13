//! `recanta serve` — start the local, read-only Explorer web UI (PRD §15). Binds
//! `127.0.0.1` by default; no auth, no telemetry, no write endpoints.

use std::path::Path;

use anyhow::Result;
use clap::Args;

use crate::explorer;
use crate::project::Paths;

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
}

pub fn run(args: ServeArgs, project_override: Option<&Path>) -> Result<()> {
    let paths = Paths::discover(project_override)?;
    explorer::run(&paths, &args.host, args.port, !args.no_open)
}
