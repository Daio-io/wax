//! Shared hex digest helpers.

use sha2::{Digest, Sha256};

/// Returns the lowercase hex SHA-256 digest of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

// ci: path-filter retrigger
