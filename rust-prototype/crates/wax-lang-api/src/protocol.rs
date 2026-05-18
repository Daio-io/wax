//! Wire protocol between engine and language pack subprocess (v1).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use wax_contract::ScanFacts;

pub const WIRE_API_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireScanRequest {
    Scan {
        api_version: u32,
        language_id: String,
        repo_root: PathBuf,
        snapshot_id: String,
        config: serde_json::Value,
    },
}

/// Success is a bare `ScanFacts` object. Failure uses `type: "error"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WireScanResponse {
    Success(ScanFacts),
    Error {
        #[serde(rename = "type")]
        kind: String,
        api_version: u32,
        language_id: String,
        code: String,
        message: String,
        #[serde(default)]
        diagnostics: Vec<wax_contract::Diagnostic>,
    },
}
