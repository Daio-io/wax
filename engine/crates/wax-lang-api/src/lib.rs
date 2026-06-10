#![deny(missing_docs)]

//! Public language pack API for wax.

pub mod build_info;
pub mod protocol;
pub mod root_patterns;

pub use build_info::build_version;
pub use wax_contract::Diagnostic;

pub use protocol::{
    DiscoverRequest, DiscoverRequestType, NotDiscoverRequest, ScanConfig, ScanRequest,
    ScanRequestType, WIRE_API_VERSION, WireErrorCode, WirePackRequest, WirePackResponse,
    WireScanRequest, WireScanResponse,
};
pub use root_patterns::{
    RootPatternKind, RootResolution, RootResolutionError, has_wildcard_segment,
    resolve_source_roots,
};
