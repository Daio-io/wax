//! Library surface for the wax CLI crate, used by integration tests and the binary.

pub mod cli;
pub mod commands {
    pub mod diagnostic_output;
    pub mod init;
    pub mod language;
    pub mod registry;
    pub mod scan;
    pub mod sync;
    pub mod uninstall;
    pub mod validate;
}

pub mod progress;

#[cfg(test)]
pub mod testing;
