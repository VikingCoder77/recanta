//! Recanta — a CLI-first, local-first, Git-aware memory substrate for AI agents.
//!
//! The library half holds everything that is testable without going through `main`:
//! argument definitions, the SQLite data layer, Git helpers, and the command
//! implementations. `main.rs` is a thin shell that parses args and dispatches here.
//!
//! Design constraints that pervade this crate (see `Recanta_PRD_v4.md`):
//! - every core feature is a `recanta` subcommand (CLI-first);
//! - the entire store is a single SQLite file with no required daemon;
//! - commands are short-lived processes invoked frequently by hooks, so startup
//!   work is kept minimal.

pub mod cli;
pub mod commands;
pub mod db;
pub mod documents;
pub mod embed;
pub mod eventlog;
pub mod explorer;
pub mod extract;
pub mod git;
pub mod graph;
pub mod hash;
pub mod install;
pub mod memory;
pub mod output;
pub mod project;
pub mod redact;
pub mod repo;
pub mod sessions;

pub use cli::Cli;
