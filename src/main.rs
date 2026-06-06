//! `recanta` binary entry point. Kept deliberately thin: parse args, dispatch into
//! the library, and translate the result into a process exit code.
//!
//! Exit codes are meaningful (PRD §9.1): 0 = success, 1 = error. Hook-invoked
//! commands are expected to be run fail-open by the caller (the installed hook
//! swallows non-zero), so we do not need special-casing here.

use std::process::ExitCode;

use recanta::Cli;

fn main() -> ExitCode {
    match Cli::parse_and_run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // `{:#}` prints the full anyhow context chain on one logical message.
            eprintln!("recanta: {err:#}");
            ExitCode::FAILURE
        }
    }
}
