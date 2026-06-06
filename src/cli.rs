//! Command-line surface. Mirrors the command list in PRD §9.2. v0.1 implements a
//! subset; commands scheduled for later milestones are declared here so the surface
//! is visible and stable, but return a "not implemented" error until built.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Recanta — durable memory & knowledge substrate for AI agents.
#[derive(Debug, Parser)]
#[command(name = "recanta", version, about, long_about = None)]
pub struct Cli {
    /// Path to the project root (defaults to the current directory, walking up to
    /// the nearest `.recanta/` for commands that need an existing project).
    #[arg(long, global = true, value_name = "PATH")]
    pub project: Option<PathBuf>,

    /// Suppress non-essential output.
    #[arg(long, global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize Recanta in a project: create `.recanta/`, `project.json`, and
    /// the SQLite store with the current schema. Installs no hooks (run `install`).
    Init(commands::init::InitArgs),

    /// Report project, branch, dirty state, schema version, DB health and size.
    Status,

    // --- Declared for later milestones (PRD §17); not yet implemented. ---
    /// Install non-destructive hooks/adapters (v0.1: git post-commit + Claude Code).
    Install,
    /// Remove Recanta managed blocks installed by `install`.
    Uninstall,
    /// Compact, task-aware fresh-session briefing (token-budgeted).
    Brief,
    /// Hybrid search over memory and code (FTS in v0.1).
    Search,
    /// Inspect a file/function/class/module/memory/commit.
    Inspect,
    /// Record a durable memory (user/project/repo/branch/symbol scope).
    Remember,
    /// Record a harness/workflow event from stdin.
    RecordEvent,
    /// Record an edit (e.g. `git diff | recanta record-edit --stdin`).
    RecordEdit,
    /// Record a commit's metadata + changed symbols (fired by post-commit hook).
    RecordCommit,
    /// (Re)build the code graph index.
    Index,
    /// Run pending SQLite schema migrations.
    Migrate,
}

impl Cli {
    /// Parse process arguments and execute. Used by `main`.
    pub fn parse_and_run() -> Result<()> {
        Cli::parse().run()
    }

    /// Execute an already-parsed CLI. Separated for testing.
    pub fn run(self) -> Result<()> {
        match self.command {
            Command::Init(args) => commands::init::run(args, self.project.as_deref()),
            Command::Status => commands::status::run(self.project.as_deref()),
            other => Err(anyhow::anyhow!(
                "`{}` is not implemented in this build (planned milestone, PRD §17)",
                other.name()
            )),
        }
    }
}

impl Command {
    /// Stable lowercase name, for diagnostics.
    fn name(&self) -> &'static str {
        match self {
            Command::Init(_) => "init",
            Command::Status => "status",
            Command::Install => "install",
            Command::Uninstall => "uninstall",
            Command::Brief => "brief",
            Command::Search => "search",
            Command::Inspect => "inspect",
            Command::Remember => "remember",
            Command::RecordEvent => "record-event",
            Command::RecordEdit => "record-edit",
            Command::RecordCommit => "record-commit",
            Command::Index => "index",
            Command::Migrate => "migrate",
        }
    }
}

use crate::commands;
