//! Subprocess-backed language pack registry discovery.
//!
//! The shared subprocess exchange owns stdio transport, spooling, and cleanup.

use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
use std::process::ExitStatus;
use std::time::Duration;

use thiserror::Error;
use wax_contract::Diagnostic;
use wax_lang_api::{
    DiscoverRequest, DiscoveredRegistrySymbol, WIRE_API_VERSION, WireErrorCode, WirePackRequest,
    WirePackResponse, normalize_discovered_components,
};

use crate::subprocess_exchange::{ExchangeError, ExchangeRequest, run_exchange};
use crate::subprocess_lang::{LanguageCancellationToken, SubprocessLanguageManifest};

const STDERR_PREVIEW_BYTES: usize = 64 * 1024;

/// Successful discover response from a language pack subprocess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverSymbolsResult {
    /// Discovered design-system symbols with optional package identity.
    pub components: Vec<DiscoveredRegistrySymbol>,
    /// Structured diagnostics emitted with the result.
    pub diagnostics: Vec<Diagnostic>,
}

/// [`SubprocessLanguageDiscoverer`] runs registry discovery over the wire protocol.
#[derive(Debug, Clone)]
pub struct SubprocessLanguageDiscoverer {
    manifest: SubprocessLanguageManifest,
}

impl SubprocessLanguageDiscoverer {
    /// Creates a subprocess discoverer from manifest process settings.
    pub fn new(manifest: SubprocessLanguageManifest) -> Self {
        Self { manifest }
    }

    /// Runs registry discovery for one request.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoverError::EmptyCommand`] for an empty command;
    /// [`DiscoverError::Spawn`], [`DiscoverError::WriteRequest`],
    /// [`DiscoverError::ReadStdout`], [`DiscoverError::ReadStderr`], or
    /// [`DiscoverError::Wait`] for process I/O failures;
    /// [`DiscoverError::Timeout`] or [`DiscoverError::WireTimeout`] for timeouts;
    /// [`DiscoverError::ProcessFailed`] for an unsuccessful process without a
    /// usable response; [`DiscoverError::InvalidResponse`],
    /// [`DiscoverError::UnsupportedApiVersion`], or
    /// [`DiscoverError::UnexpectedResponseType`] for invalid wire responses; and
    /// [`DiscoverError::Unsupported`] or [`DiscoverError::Wire`] for errors
    /// reported by the language pack.
    pub fn discover(
        &self,
        request: DiscoverRequest,
    ) -> Result<DiscoverSymbolsResult, DiscoverError> {
        run_subprocess_discover(&self.manifest, request, &LanguageCancellationToken::new())
    }

    /// Runs registry discovery unless cancellation is requested first.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoverError::Cancelled`] when cancellation wins;
    /// [`DiscoverError::EmptyCommand`] for an empty command;
    /// [`DiscoverError::Spawn`], [`DiscoverError::WriteRequest`],
    /// [`DiscoverError::ReadStdout`], [`DiscoverError::ReadStderr`], or
    /// [`DiscoverError::Wait`] for process I/O failures;
    /// [`DiscoverError::Timeout`] or [`DiscoverError::WireTimeout`] for timeouts;
    /// [`DiscoverError::ProcessFailed`] for an unsuccessful process without a
    /// usable response; [`DiscoverError::InvalidResponse`],
    /// [`DiscoverError::UnsupportedApiVersion`], or
    /// [`DiscoverError::UnexpectedResponseType`] for invalid wire responses; and
    /// [`DiscoverError::Unsupported`] or [`DiscoverError::Wire`] for errors
    /// reported by the language pack.
    pub fn discover_with_cancellation(
        &self,
        request: DiscoverRequest,
        cancellation: &LanguageCancellationToken,
    ) -> Result<DiscoverSymbolsResult, DiscoverError> {
        run_subprocess_discover(&self.manifest, request, cancellation)
    }
}

/// Typed errors returned by subprocess registry discovery.
#[derive(Debug, Error)]
pub enum DiscoverError {
    /// The subprocess manifest did not contain an executable command.
    #[error("language subprocess command is empty")]
    EmptyCommand,
    /// The subprocess could not be spawned.
    #[error("failed to spawn language subprocess {command:?}: {source}")]
    Spawn {
        /// Command executable that failed to spawn.
        command: String,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The discover request could not be written to subprocess stdin.
    #[error("failed to write discover request to language subprocess: {source}")]
    WriteRequest {
        /// Underlying I/O or serialization error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The subprocess stdout stream could not be read.
    #[error("failed to read language subprocess stdout: {source}")]
    ReadStdout {
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The subprocess stderr stream could not be read.
    #[error("failed to read language subprocess stderr: {source}")]
    ReadStderr {
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The subprocess status could not be polled.
    #[error("failed to wait for language subprocess: {source}")]
    Wait {
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The subprocess exceeded its configured timeout.
    #[error("language subprocess timed out after {timeout:?}")]
    Timeout {
        /// Configured timeout that elapsed.
        timeout: Duration,
    },
    /// The language pack reported its own discover timeout.
    #[error("language subprocess reported timeout: {message}")]
    WireTimeout {
        /// Human-readable timeout message returned by the language pack.
        message: String,
        /// Structured diagnostics returned by the language pack.
        diagnostics: Vec<Diagnostic>,
    },
    /// Registry discovery was cancelled before completion.
    #[error("registry discovery was cancelled")]
    Cancelled,
    /// The subprocess exited unsuccessfully without a usable wire response.
    #[error("language subprocess exited with {status}: {stderr}")]
    ProcessFailed {
        /// Process exit status.
        status: ExitStatus,
        /// Captured stderr, decoded lossily as UTF-8.
        stderr: String,
    },
    /// The subprocess output was not a valid wire response.
    #[error("invalid language subprocess response: {source}")]
    InvalidResponse {
        /// Underlying JSON parsing error.
        #[source]
        source: serde_json::Error,
    },
    /// The subprocess returned an unsupported wire API version.
    #[error("unsupported language wire API version {found}; engine supports {supported}")]
    UnsupportedApiVersion {
        /// API version found in the subprocess response.
        found: u32,
        /// API version supported by this engine.
        supported: u32,
    },
    /// The language pack does not implement registry discovery.
    #[error("language pack does not implement registry discovery: {message}")]
    Unsupported {
        /// Human-readable message returned by the language pack.
        message: String,
        /// Structured diagnostics returned by the language pack.
        diagnostics: Vec<Diagnostic>,
    },
    /// The subprocess returned a wire response of the wrong type.
    #[error("language subprocess returned unexpected response type: {found}")]
    UnexpectedResponseType {
        /// Response type tag found in the wire message.
        found: &'static str,
    },
    /// The language pack returned a typed wire error.
    #[error("language subprocess returned {code:?}: {message}")]
    Wire {
        /// Stable machine-readable wire error code.
        code: WireErrorCode,
        /// Human-readable message returned by the language pack.
        message: String,
        /// Structured diagnostics returned by the language pack.
        diagnostics: Vec<Diagnostic>,
    },
}

fn run_subprocess_discover(
    manifest: &SubprocessLanguageManifest,
    request: DiscoverRequest,
    cancellation: &LanguageCancellationToken,
) -> Result<DiscoverSymbolsResult, DiscoverError> {
    let request = serialize_wire_request(request)?;
    let exchange = run_exchange(ExchangeRequest {
        command: &manifest.command,
        request: &request,
        timeout: manifest.timeout,
        cancellation,
        stdout_kind: "discover-stdout",
    })
    .map_err(map_exchange_error)?;

    match parse_wire_response(exchange.stdout_path()) {
        Ok(result) => Ok(result),
        Err(_parse_error) if !exchange.status.success() => Err(DiscoverError::ProcessFailed {
            status: exchange.status,
            stderr: String::from_utf8_lossy(
                &exchange
                    .stderr_bytes(STDERR_PREVIEW_BYTES)
                    .unwrap_or_default(),
            )
            .trim()
            .to_owned(),
        }),
        Err(parse_error) => Err(parse_error),
    }
}

fn serialize_wire_request(request: DiscoverRequest) -> Result<Vec<u8>, DiscoverError> {
    let wire_request = WirePackRequest::from(request);
    serde_json::to_vec(&wire_request).map_err(|source| DiscoverError::WriteRequest {
        source: Box::new(source),
    })
}

fn map_exchange_error(error: ExchangeError) -> DiscoverError {
    match error {
        ExchangeError::EmptyCommand => DiscoverError::EmptyCommand,
        ExchangeError::Spawn { program, source } => DiscoverError::Spawn {
            command: program,
            source,
        },
        ExchangeError::CreateSpool { stream, source }
        | ExchangeError::ReadStream { stream, source } => map_stream_error(stream, source),
        ExchangeError::WriteRequest { source } => DiscoverError::WriteRequest {
            source: Box::new(source),
        },
        ExchangeError::Wait { source } => DiscoverError::Wait { source },
        ExchangeError::Timeout { timeout, .. } => DiscoverError::Timeout { timeout },
        ExchangeError::Cancelled { .. } => DiscoverError::Cancelled,
    }
}

fn map_stream_error(stream: &'static str, source: io::Error) -> DiscoverError {
    match stream {
        "stdout" => DiscoverError::ReadStdout { source },
        "stderr" => DiscoverError::ReadStderr { source },
        _ => unreachable!("unexpected exchange stream: {stream}"),
    }
}

fn parse_wire_response(stdout: &Path) -> Result<DiscoverSymbolsResult, DiscoverError> {
    let probe_file = File::open(stdout).map_err(|source| DiscoverError::ReadStdout { source })?;
    let probe: VersionProbe = serde_json::from_reader(BufReader::new(probe_file))
        .map_err(|source| DiscoverError::InvalidResponse { source })?;
    ensure_supported_api_version(probe.api_version)?;

    let response_file =
        File::open(stdout).map_err(|source| DiscoverError::ReadStdout { source })?;
    let response: WirePackResponse = serde_json::from_reader(BufReader::new(response_file))
        .map_err(|source| DiscoverError::InvalidResponse { source })?;

    match response {
        WirePackResponse::DiscoverSymbols {
            api_version,
            symbols,
            components,
            diagnostics,
            ..
        } => {
            ensure_supported_api_version(api_version)?;
            Ok(DiscoverSymbolsResult {
                components: normalize_discovered_components(symbols, components),
                diagnostics,
            })
        }
        WirePackResponse::Error {
            api_version,
            code,
            message,
            diagnostics,
            ..
        } => {
            ensure_supported_api_version(api_version)?;
            match code {
                WireErrorCode::Timeout => Err(DiscoverError::WireTimeout {
                    message,
                    diagnostics,
                }),
                WireErrorCode::DiscoverUnsupported => Err(DiscoverError::Unsupported {
                    message,
                    diagnostics,
                }),
                _ => Err(DiscoverError::Wire {
                    code,
                    message,
                    diagnostics,
                }),
            }
        }
        WirePackResponse::ScanFacts { .. } => Err(DiscoverError::UnexpectedResponseType {
            found: "scan_facts",
        }),
    }
}

#[derive(Debug, serde::Deserialize)]
struct VersionProbe {
    api_version: u32,
}

fn ensure_supported_api_version(api_version: u32) -> Result<(), DiscoverError> {
    if api_version == WIRE_API_VERSION {
        Ok(())
    } else {
        Err(DiscoverError::UnsupportedApiVersion {
            found: api_version,
            supported: WIRE_API_VERSION,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use crate::subprocess_exchange::ExchangeError;

    use super::{DiscoverError, map_exchange_error};

    #[test]
    fn maps_shared_stdout_read_errors_to_discover_errors() {
        let error = map_exchange_error(ExchangeError::ReadStream {
            stream: "stdout",
            source: io::Error::other("read failed"),
        });

        assert!(matches!(error, DiscoverError::ReadStdout { .. }));
    }
}
