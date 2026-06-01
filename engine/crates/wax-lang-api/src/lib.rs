#![deny(missing_docs)]

//! Public language pack API for wax.

pub mod protocol;
pub mod root_patterns;

pub use wax_contract::Diagnostic;

pub use protocol::{
    ScanConfig, ScanRequest, ScanRequestType, WIRE_API_VERSION, WireErrorCode, WireScanRequest,
    WireScanResponse,
};
pub use root_patterns::{
    RootPatternKind, RootResolution, RootResolutionError, has_wildcard_segment,
    resolve_source_roots,
};
