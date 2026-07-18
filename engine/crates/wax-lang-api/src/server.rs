//! Single-request stdio server support for language packs.

use std::io::{self, BufRead, Write};

use thiserror::Error;
use wax_contract::LanguageId;

use crate::{
    DiscoverRequest, DiscoverRequestType, ScanRequest, ScanRequestType, WIRE_API_VERSION,
    WireErrorCode, WirePackRequest, WirePackResponse,
};

/// Handles language-specific scan and registry discovery requests over the wire protocol.
///
/// Implementations keep their language-specific request handling and error-to-wire-response
/// mappings local, while [`serve_one`] owns framing, protocol validation, and response output.
pub trait WirePackHandler {
    /// Returns the language identifier used for protocol errors without a valid request.
    fn language_id(&self) -> LanguageId;

    /// Handles one validated scan request.
    ///
    /// The request's API version is compatible with [`WIRE_API_VERSION`]. Implementations must
    /// convert all language-specific outcomes into a [`WirePackResponse`].
    fn scan(&self, request: ScanRequest) -> WirePackResponse;

    /// Handles one validated registry discovery request.
    ///
    /// The request's API version is compatible with [`WIRE_API_VERSION`]. Implementations must
    /// convert all language-specific outcomes into a [`WirePackResponse`].
    fn discover(&self, request: DiscoverRequest) -> WirePackResponse;
}

/// Host failures that prevent the stdio server from producing a wire response.
#[derive(Debug, Error)]
pub enum WireServerError {
    /// Reading the request from the host stream failed.
    #[error("failed to read wire pack request: {source}")]
    Read {
        /// The underlying host IO failure.
        #[source]
        source: io::Error,
    },
    /// Serializing the wire response failed.
    #[error("failed to serialize wire pack response: {source}")]
    Serialize {
        /// The underlying JSON serialization failure.
        #[source]
        source: serde_json::Error,
    },
    /// Writing the serialized response to the host stream failed.
    #[error("failed to write wire pack response: {source}")]
    Write {
        /// The underlying host IO failure.
        #[source]
        source: io::Error,
    },
    /// Flushing the response to the host stream failed.
    #[error("failed to flush wire pack response: {source}")]
    Flush {
        /// The underlying host IO failure.
        #[source]
        source: io::Error,
    },
}

/// Serves the first nonblank wire request from a reader.
///
/// The function emits a protocol error response for EOF before a request, malformed JSON, and an
/// incompatible API version. It dispatches exactly one valid request, writes exactly one newline
/// after the serialized response, flushes the writer, and returns without reading another request.
///
/// # Errors
///
/// Returns [`WireServerError`] only when host IO or response serialization prevents a response
/// from being completed. Protocol failures are represented as [`WirePackResponse::Error`].
pub fn serve_one<R, W, H>(mut reader: R, mut writer: W, handler: &H) -> Result<(), WireServerError>
where
    R: BufRead,
    W: Write,
    H: WirePackHandler,
{
    let response = match read_request_line(&mut reader)? {
        Some(line) => match serde_json::from_str(&line) {
            Ok(request) => dispatch(request, handler),
            Err(error) => invalid_request_response(
                handler.language_id(),
                format!("invalid pack request JSON: {error}"),
            ),
        },
        None => invalid_request_response(
            handler.language_id(),
            "invalid pack request JSON: expected one nonblank request line before EOF".to_owned(),
        ),
    };

    let serialized =
        serde_json::to_vec(&response).map_err(|source| WireServerError::Serialize { source })?;
    writer
        .write_all(&serialized)
        .map_err(|source| WireServerError::Write { source })?;
    writer
        .write_all(b"\n")
        .map_err(|source| WireServerError::Write { source })?;
    writer
        .flush()
        .map_err(|source| WireServerError::Flush { source })?;

    Ok(())
}

fn read_request_line<R: BufRead>(reader: &mut R) -> Result<Option<String>, WireServerError> {
    loop {
        let mut line = String::new();
        let bytes_read = reader
            .read_line(&mut line)
            .map_err(|source| WireServerError::Read { source })?;
        if bytes_read == 0 {
            return Ok(None);
        }
        if !line.trim().is_empty() {
            return Ok(Some(line));
        }
    }
}

fn dispatch<H: WirePackHandler>(request: WirePackRequest, handler: &H) -> WirePackResponse {
    match request {
        WirePackRequest::Scan {
            api_version,
            language_id,
            repo_root,
            snapshot_id,
            config,
        } => {
            if api_version != WIRE_API_VERSION {
                unsupported_api_version_response(api_version, language_id)
            } else {
                handler.scan(ScanRequest {
                    request_type: ScanRequestType::Scan,
                    api_version,
                    language_id,
                    repo_root,
                    snapshot_id,
                    config,
                })
            }
        }
        WirePackRequest::Discover {
            api_version,
            language_id,
            repo_root,
            roots,
        } => {
            if api_version != WIRE_API_VERSION {
                unsupported_api_version_response(api_version, language_id)
            } else {
                handler.discover(DiscoverRequest {
                    request_type: DiscoverRequestType::Discover,
                    api_version,
                    language_id,
                    repo_root,
                    roots,
                })
            }
        }
    }
}

fn invalid_request_response(language_id: LanguageId, message: String) -> WirePackResponse {
    WirePackResponse::Error {
        api_version: WIRE_API_VERSION,
        language_id,
        code: WireErrorCode::ConfigInvalid,
        message,
        diagnostics: Vec::new(),
    }
}

fn unsupported_api_version_response(api_version: u32, language_id: LanguageId) -> WirePackResponse {
    WirePackResponse::Error {
        api_version: WIRE_API_VERSION,
        language_id,
        code: WireErrorCode::ApiVersionUnsupported,
        message: format!(
            "wire api_version {api_version} is unsupported; expected {WIRE_API_VERSION}"
        ),
        diagnostics: Vec::new(),
    }
}
