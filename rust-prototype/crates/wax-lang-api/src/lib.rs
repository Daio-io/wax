//! Stable boundary between the Rust engine and language packs.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use wax_contract::ScanFacts;

pub mod protocol;

pub use wax_contract;

/// In-process scan input. The engine validates `language_id` and `api_version` before calling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRequest {
    pub repo_root: PathBuf,
    /// Engine-assigned id; implementations MUST echo into `ScanFacts.snapshot_id`.
    pub snapshot_id: String,
    /// Per-language section from `.waxrc` (opaque to the engine).
    pub config: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum LanguageError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("parse: {0}")]
    Parse(String),
    #[error("cancelled")]
    Cancelled,
    #[error("timeout after {0}s")]
    Timeout(u64),
    #[error("{0}")]
    Other(String),
}

pub type LanguageResult<T> = Result<T, LanguageError>;

/// Language pack: discovery + extraction only. Reporting stays in the engine.
///
/// Implementations may block; the engine runs each pack on its own thread (see
/// `engine.scan_concurrency` in the spec). Implementations should respect cancellation
/// when invoked from a subprocess adapter with a deadline.
pub trait LanguageExtractor: Send + Sync {
    fn metadata(&self) -> wax_contract::LanguageMetadata;

    fn scan(&self, request: &ScanRequest) -> LanguageResult<ScanFacts>;
}
