//! In-process and wire protocol types shared by the wax engine and language packs.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;
use wax_contract::{LanguageId, ScanFacts};

/// Current wire API version for scan requests.
pub const WIRE_API_VERSION: u32 = 1;

/// Opaque per-language config payload forwarded by the engine.
pub type ScanConfig = Map<String, Value>;

/// In-process scan request used by the engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScanRequest {
    /// Request kind discriminator.
    #[serde(rename = "type")]
    pub request_type: ScanRequestType,
    /// Wire API version expected by the engine.
    pub api_version: u32,
    /// Language pack identifier being scanned.
    pub language_id: LanguageId,
    /// Absolute path to the repository root.
    pub repo_root: String,
    /// Engine-generated snapshot identifier.
    pub snapshot_id: String,
    /// Opaque language configuration.
    pub config: ScanConfig,
}

/// Request kind for scan requests.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScanRequestType {
    /// Execute a scan request.
    Scan,
}

/// Wire protocol request envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireScanRequest {
    /// Scan command issued over stdio.
    Scan {
        /// Wire API version expected by the engine.
        api_version: u32,
        /// Language pack identifier being scanned.
        language_id: LanguageId,
        /// Absolute path to the repository root.
        repo_root: String,
        /// Engine-generated snapshot identifier.
        snapshot_id: String,
        /// Opaque language configuration.
        config: ScanConfig,
    },
}

/// Wire protocol response envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireScanResponse {
    /// Successful scan response.
    ScanFacts {
        /// Emitted scan facts payload.
        scan_facts: Box<ScanFacts>,
    },
    /// Failed scan response.
    Error {
        /// Stable machine-readable error code.
        code: WireErrorCode,
        /// Human-readable diagnostic message.
        message: String,
    },
}

/// Stable wire error codes for v1 responses.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WireErrorCode {
    /// Engine and language pack API versions do not match.
    ApiVersionUnsupported,
    /// Language-specific config payload was invalid.
    ConfigInvalid,
    /// Design-system registry could not be located.
    RegistryNotFound,
    /// Parser failed to initialize.
    ParserInitFailed,
    /// Scan timed out.
    Timeout,
    /// Scan failed for a non-timeout runtime reason.
    ScanFailed,
    /// Unexpected internal failure.
    InternalError,
}

/// Error raised when converting between in-process and wire requests.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RequestConversionError {
    /// The in-process request did not use the scan request type.
    #[error("unsupported in-process request type: {0:?}")]
    UnsupportedRequestType(ScanRequestType),
}

impl TryFrom<ScanRequest> for WireScanRequest {
    type Error = RequestConversionError;

    fn try_from(request: ScanRequest) -> Result<Self, Self::Error> {
        match request.request_type {
            ScanRequestType::Scan => Ok(Self::Scan {
                api_version: request.api_version,
                language_id: request.language_id,
                repo_root: request.repo_root,
                snapshot_id: request.snapshot_id,
                config: request.config,
            }),
        }
    }
}

impl From<WireScanRequest> for ScanRequest {
    fn from(request: WireScanRequest) -> Self {
        match request {
            WireScanRequest::Scan {
                api_version,
                language_id,
                repo_root,
                snapshot_id,
                config,
            } => Self {
                request_type: ScanRequestType::Scan,
                api_version,
                language_id,
                repo_root,
                snapshot_id,
                config,
            },
        }
    }
}
