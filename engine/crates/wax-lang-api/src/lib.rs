#![deny(missing_docs)]

//! Public language pack API for wax.
//!
//! # Examples
//!
//! ```
//! use wax_lang_api::{ScanConfig, ScanRequest, ScanRequestType, WIRE_API_VERSION};
//!
//! let request = ScanRequest {
//!     request_type: ScanRequestType::Scan,
//!     api_version: WIRE_API_VERSION,
//!     language_id: "react".try_into()?,
//!     repo_root: ".".to_owned(),
//!     snapshot_id: "snapshot-1".to_owned(),
//!     config: ScanConfig::new(),
//! };
//!
//! assert_eq!(request.language_id.as_str(), "react");
//! # Ok::<(), wax_contract::LanguageIdError>(())
//! ```

pub mod build_info;
pub mod discover;
pub mod import_resolution;
pub mod protocol;
pub mod root_patterns;
pub mod server;
pub mod timing;
pub mod token_registry;

pub use build_info::build_version;
pub use wax_contract::Diagnostic;

pub use discover::{
    DiscoveredRegistrySymbol, normalize_discovered_components, npm_package_name_for_path,
    npm_package_name_for_roots, swift_module_from_source_path,
};

pub use import_resolution::{npm_import_package_root, resolve_import_aware_match};

pub use protocol::{
    DiscoverRequest, DiscoverRequestType, NotDiscoverRequest, ScanConfig, ScanRequest,
    ScanRequestType, WIRE_API_VERSION, WireErrorCode, WirePackRequest, WirePackResponse,
    WireScanRequest, WireScanResponse,
};
pub use root_patterns::{
    RootPatternKind, RootResolution, RootResolutionError, has_wildcard_segment,
    resolve_source_roots,
};
pub use server::{WirePackHandler, WireServerError, serve_one};
pub use timing::parse_extract_millis;
pub use token_registry::{
    RegistryTokenIndex, TokenMatch, TokenRegistryError, find_token_matches, parse_registry_tokens,
    token_index,
};
