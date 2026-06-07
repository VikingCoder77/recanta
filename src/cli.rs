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

    /// Compact, task-aware fresh-session briefing (token-budgeted).
    Brief(commands::brief::BriefArgs),

    /// Hybrid full-text search over memory, documents, and chat transcripts.
    Search(commands::search::SearchArgs),

    /// Record a durable memory (user/project/repo/branch/symbol scope).
    Remember(commands::remember::RememberArgs),

    /// Record a commit's metadata (fired by the post-commit hook).
    RecordCommit(commands::record_commit::RecordCommitArgs),

    /// Record an edit from stdin (a diff or harness event; fired by the post-edit hook).
    RecordEdit(commands::record_edit::RecordEditArgs),

    /// Record a harness/workflow event from stdin (fired by the session-stop hook).
    RecordEvent(commands::record_event::RecordEventArgs),

    /// Install non-destructive hooks/adapters (git post-commit + Claude Code).
    Install(crate::install::InstallArgs),

    /// Remove Recanta managed blocks installed by `install`.
    Uninstall(crate::install::UninstallArgs),

    /// Inspect a function/class/file from the code graph.
    Inspect(commands::inspect::InspectArgs),

    /// (Re)build the code graph index.
    Index(commands::index::IndexArgs),

    /// Ingest documents (Markdown/text/PDF/Word/.doc) into project memory.
    Ingest(commands::ingest::IngestArgs),

    /// Import agent session transcripts (Claude Code, Codex, Gemini, OpenCode).
    ImportSessions(commands::import_sessions::ImportSessionsArgs),

    /// Manage the capture policy (raw transcript capture; on by default).
    Capture(commands::capture::CaptureArgs),

    /// Apply pending SQLite schema migrations.
    Migrate(commands::migrate::MigrateArgs),
}

impl Cli {
    /// Parse process arguments and execute. Used by `main`.
    pub fn parse_and_run() -> Result<()> {
        Cli::parse().run()
    }

    /// Execute an already-parsed CLI. Separated for testing.
    pub fn run(self) -> Result<()> {
        let project = self.project.as_deref();
        match self.command {
            Command::Init(args) => commands::init::run(args, project),
            Command::Status => commands::status::run(project),
            Command::Brief(args) => commands::brief::run(args, project),
            Command::Search(args) => commands::search::run(args, project),
            Command::Remember(args) => commands::remember::run(args, project),
            Command::RecordCommit(args) => commands::record_commit::run(args, project),
            Command::RecordEdit(args) => commands::record_edit::run(args, project),
            Command::RecordEvent(args) => commands::record_event::run(args, project),
            Command::Install(args) => crate::install::run(args, project),
            Command::Uninstall(args) => crate::install::uninstall(args, project),
            Command::Inspect(args) => commands::inspect::run(args, project),
            Command::Index(args) => commands::index::run(args, project),
            Command::Ingest(args) => commands::ingest::run(args, project),
            Command::ImportSessions(args) => commands::import_sessions::run(args, project),
            Command::Capture(args) => commands::capture::run(args, project),
            Command::Migrate(args) => commands::migrate::run(args, project),
        }
    }
}

use crate::commands;
