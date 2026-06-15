#![deny(missing_docs)]

//! Public language pack API for wax.

pub mod build_info;
pub mod import_resolution;
pub mod protocol;
pub mod root_patterns;

pub use build_info::build_version;
pub use wax_contract::Diagnostic;

pub use import_resolution::{
    FrameworkPackagesParseError, import_matches_framework_package, npm_import_package_root,
    package_matches_prefix, parse_framework_packages, resolve_import_aware_match,
};

pub use protocol::{
    DiscoverRequest, DiscoverRequestType, NotDiscoverRequest, ScanConfig, ScanRequest,
    ScanRequestType, WIRE_API_VERSION, WireErrorCode, WirePackRequest, WirePackResponse,
    WireScanRequest, WireScanResponse,
};
pub use root_patterns::{
    RootPatternKind, RootResolution, RootResolutionError, has_wildcard_segment,
    resolve_source_roots,
};
