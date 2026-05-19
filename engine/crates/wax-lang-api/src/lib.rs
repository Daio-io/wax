#![deny(missing_docs)]

//! Public language pack API for wax.

pub mod protocol;

pub use protocol::{
    RequestConversionError, ScanConfig, ScanRequest, ScanRequestType, WIRE_API_VERSION,
    WireErrorCode, WireScanRequest, WireScanResponse,
};
