//! Library surface for the wax CLI crate, used by integration tests and the binary.
//!
//! # Examples
//!
//! Parse arguments without allowing clap to exit the process:
//!
//! ```
//! use clap::Parser;
//! use wax_cli::cli::{Cli, Commands};
//!
//! let cli = Cli::try_parse_from(["wax", "scan", "--concurrency", "2"])?;
//! let Commands::Scan(args) = cli.command else {
//!     unreachable!("scan arguments produce the scan command");
//! };
//! assert_eq!(args.scan_concurrency, Some(2));
//! # Ok::<(), clap::Error>(())
//! ```

pub mod cli;
pub mod commands {
    pub mod diagnostic_output;
    pub mod init;
    pub mod language;
    pub mod registry;
    pub mod scan;
    mod state_path;
    pub mod sync;
    pub mod uninstall;
    pub mod validate;
}

pub mod progress;

#[cfg(test)]
pub mod testing;
