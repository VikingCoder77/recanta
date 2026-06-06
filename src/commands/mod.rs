//! Command implementations. Each module owns one `recanta` subcommand. Argument
//! structs live next to their handler so the CLI surface and behavior stay together.

pub mod init;
pub mod status;
