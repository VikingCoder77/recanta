//! Command implementations. Each module owns one `recanta` subcommand. Argument
//! structs live next to their handler so the CLI surface and behavior stay together.

pub mod brief;
pub mod capture;
pub mod import_sessions;
pub mod index;
pub mod ingest;
pub mod init;
pub mod inspect;
pub mod record_commit;
pub mod remember;
pub mod search;
pub mod status;
