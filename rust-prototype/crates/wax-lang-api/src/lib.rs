//! Stable boundary between the Rust engine and language packs.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use wax_contract::ScanFacts;

pub use wax_contract;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRequest {
    pub fixture_root: PathBuf,
    pub mode: String,
    pub snapshot_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LanguageError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("parse: {0}")]
    Parse(String),
    #[error("{0}")]
    Other(String),
}

pub type LanguageResult<T> = Result<T, LanguageError>;

/// Language pack: discovery + extraction only. Reporting stays in the engine.
pub trait LanguageExtractor: Send + Sync {
    fn metadata(&self) -> wax_contract::LanguageMetadata;

    fn scan(&self, request: &ScanRequest) -> LanguageResult<ScanFacts>;
}
