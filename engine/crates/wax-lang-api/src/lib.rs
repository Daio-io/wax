#![deny(missing_docs)]

//! Public language pack API for wax.

pub mod build_info;
pub mod protocol;

pub use build_info::build_version;
pub use wax_contract::Diagnostic;

pub use protocol::{
    ScanConfig, ScanRequest, ScanRequestType, WIRE_API_VERSION, WireErrorCode, WireScanRequest,
    WireScanResponse,
};
