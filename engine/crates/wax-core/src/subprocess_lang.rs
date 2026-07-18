//! Subprocess-backed language pack extraction.

use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
use std::process::ExitStatus;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use thiserror::Error;
use wax_contract::{Diagnostic, ScanFacts};
use wax_lang_api::{
    ScanRequest, WIRE_API_VERSION, WireErrorCode, WireScanRequest, WireScanResponse,
};

use crate::subprocess_exchange::{ExchangeError, ExchangeRequest, run_exchange};

const STDERR_PREVIEW_BYTES: usize = 64 * 1024;

/// Extracts language scan facts for one request.
pub trait LanguageExtractor {
    /// Runs the extractor and returns validated scan facts.
    ///
    /// # Errors
    ///
    /// Returns [`LanguageError::EmptyCommand`] for an empty command;
    /// [`LanguageError::Spawn`], [`LanguageError::WriteRequest`],
    /// [`LanguageError::ReadStdout`], [`LanguageError::ReadStderr`], or
    /// [`LanguageError::Wait`] for process I/O failures;
    /// [`LanguageError::Timeout`] or [`LanguageError::WireTimeout`] for timeouts;
    /// [`LanguageError::ProcessFailed`] for an unsuccessful process without a
    /// usable response; [`LanguageError::InvalidResponse`] or
    /// [`LanguageError::UnsupportedApiVersion`] for invalid wire responses; and
    /// [`LanguageError::Wire`] for an error reported by the language pack.
    fn scan(&self, request: ScanRequest) -> Result<ScanFacts, LanguageError>;

    /// Runs the extractor unless cancellation is requested first.
    ///
    /// Extractors without cancellation support may use the default implementation,
    /// which ignores the token and delegates to [`LanguageExtractor::scan`].
    ///
    /// # Errors
    ///
    /// Returns [`LanguageError::EmptyCommand`] for an empty command;
    /// [`LanguageError::Spawn`], [`LanguageError::WriteRequest`],
    /// [`LanguageError::ReadStdout`], [`LanguageError::ReadStderr`], or
    /// [`LanguageError::Wait`] for process I/O failures;
    /// [`LanguageError::Timeout`] or [`LanguageError::WireTimeout`] for timeouts;
    /// [`LanguageError::ProcessFailed`] for an unsuccessful process without a
    /// usable response; [`LanguageError::InvalidResponse`] or
    /// [`LanguageError::UnsupportedApiVersion`] for invalid wire responses; and
    /// [`LanguageError::Wire`] for an error reported by the language pack. The
    /// default implementation ignores the cancellation token, so it does not
    /// introduce [`LanguageError::Cancelled`].
    fn scan_with_cancellation(
        &self,
        request: ScanRequest,
        _cancellation: &LanguageCancellationToken,
    ) -> Result<ScanFacts, LanguageError> {
        self.scan(request)
    }
}

/// Shared cancellation signal for an in-flight language extraction.
#[derive(Debug, Clone, Default)]
pub struct LanguageCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl LanguageCancellationToken {
    /// Creates a cancellation token in the active state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation for operations using this token.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Returns whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Manifest fields needed to run a language pack as a subprocess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubprocessLanguageManifest {
    /// Command and arguments used to launch the language pack.
    ///
    /// The first element is the executable path or name; remaining elements are
    /// passed as process arguments.
    pub command: Vec<String>,
    /// Maximum wall-clock time allowed for one scan.
    pub timeout: Duration,
}

/// [`LanguageExtractor`] implementation that speaks the wire protocol over stdio.
#[derive(Debug, Clone)]
pub struct SubprocessLanguageExtractor {
    manifest: SubprocessLanguageManifest,
}

impl SubprocessLanguageExtractor {
    /// Creates a subprocess extractor from manifest process settings.
    pub fn new(manifest: SubprocessLanguageManifest) -> Self {
        Self { manifest }
    }

    /// Runs the extractor unless cancellation is requested first.
    ///
    /// # Errors
    ///
    /// Returns [`LanguageError::Cancelled`] when cancellation wins;
    /// [`LanguageError::EmptyCommand`] for an empty command;
    /// [`LanguageError::Spawn`], [`LanguageError::WriteRequest`],
    /// [`LanguageError::ReadStdout`], [`LanguageError::ReadStderr`], or
    /// [`LanguageError::Wait`] for process I/O failures;
    /// [`LanguageError::Timeout`] or [`LanguageError::WireTimeout`] for timeouts;
    /// [`LanguageError::ProcessFailed`] for an unsuccessful process without a
    /// usable response; [`LanguageError::InvalidResponse`] or
    /// [`LanguageError::UnsupportedApiVersion`] for invalid wire responses; and
    /// [`LanguageError::Wire`] for an error reported by the language pack.
    pub fn scan_with_cancellation(
        &self,
        request: ScanRequest,
        cancellation: &LanguageCancellationToken,
    ) -> Result<ScanFacts, LanguageError> {
        run_subprocess_scan(&self.manifest, request, cancellation)
    }
}

impl LanguageExtractor for SubprocessLanguageExtractor {
    fn scan(&self, request: ScanRequest) -> Result<ScanFacts, LanguageError> {
        run_subprocess_scan(&self.manifest, request, &LanguageCancellationToken::new())
    }

    fn scan_with_cancellation(
        &self,
        request: ScanRequest,
        cancellation: &LanguageCancellationToken,
    ) -> Result<ScanFacts, LanguageError> {
        run_subprocess_scan(&self.manifest, request, cancellation)
    }
}

/// Typed errors returned by language extraction.
#[derive(Debug, Error)]
pub enum LanguageError {
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
    /// The scan request could not be written to subprocess stdin.
    #[error("failed to write scan request to language subprocess: {source}")]
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
    /// The language pack reported its own scan timeout.
    #[error("language subprocess reported timeout: {message}")]
    WireTimeout {
        /// Human-readable timeout message returned by the language pack.
        message: String,
        /// Structured diagnostics returned by the language pack.
        diagnostics: Vec<Diagnostic>,
    },
    /// Language extraction was cancelled before completion.
    #[error("language extraction was cancelled")]
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

fn run_subprocess_scan(
    manifest: &SubprocessLanguageManifest,
    request: ScanRequest,
    cancellation: &LanguageCancellationToken,
) -> Result<ScanFacts, LanguageError> {
    let request = serialize_wire_request(request)?;
    let exchange = run_exchange(ExchangeRequest {
        command: &manifest.command,
        request: &request,
        timeout: manifest.timeout,
        cancellation,
        stdout_kind: "stdout",
    })
    .map_err(map_exchange_error)?;

    match parse_wire_response(exchange.stdout_path()) {
        Ok(facts) => Ok(facts),
        Err(_parse_error) if !exchange.status.success() => Err(LanguageError::ProcessFailed {
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

fn serialize_wire_request(request: ScanRequest) -> Result<Vec<u8>, LanguageError> {
    let wire_request = WireScanRequest::from(request);
    serde_json::to_vec(&wire_request).map_err(|source| LanguageError::WriteRequest {
        source: Box::new(source),
    })
}

fn map_exchange_error(error: ExchangeError) -> LanguageError {
    match error {
        ExchangeError::EmptyCommand => LanguageError::EmptyCommand,
        ExchangeError::Spawn { program, source } => LanguageError::Spawn {
            command: program,
            source,
        },
        ExchangeError::CreateSpool { stream, source }
        | ExchangeError::ReadStream { stream, source } => map_stream_error(stream, source),
        ExchangeError::WriteRequest { source } => LanguageError::WriteRequest {
            source: Box::new(source),
        },
        ExchangeError::Wait { source } => LanguageError::Wait { source },
        ExchangeError::Timeout { timeout, .. } => LanguageError::Timeout { timeout },
        ExchangeError::Cancelled { .. } => LanguageError::Cancelled,
    }
}

fn map_stream_error(stream: &'static str, source: io::Error) -> LanguageError {
    match stream {
        "stdout" => LanguageError::ReadStdout { source },
        "stderr" => LanguageError::ReadStderr { source },
        _ => unreachable!("unexpected exchange stream: {stream}"),
    }
}

fn parse_wire_response(stdout: &Path) -> Result<ScanFacts, LanguageError> {
    let probe_file = File::open(stdout).map_err(|source| LanguageError::ReadStdout { source })?;
    let probe: VersionProbe = serde_json::from_reader(BufReader::new(probe_file))
        .map_err(|source| LanguageError::InvalidResponse { source })?;
    ensure_supported_api_version(probe.api_version)?;

    let response_file =
        File::open(stdout).map_err(|source| LanguageError::ReadStdout { source })?;
    let response: WireScanResponse = serde_json::from_reader(BufReader::new(response_file))
        .map_err(|source| LanguageError::InvalidResponse { source })?;

    match response {
        WireScanResponse::ScanFacts {
            api_version, facts, ..
        } => {
            ensure_supported_api_version(api_version)?;
            Ok(*facts)
        }
        WireScanResponse::Error {
            api_version,
            code,
            message,
            diagnostics,
            ..
        } => {
            ensure_supported_api_version(api_version)?;
            match code {
                WireErrorCode::Timeout => Err(LanguageError::WireTimeout {
                    message,
                    diagnostics,
                }),
                _ => Err(LanguageError::Wire {
                    code,
                    message,
                    diagnostics,
                }),
            }
        }
    }
}

#[derive(serde::Deserialize)]
struct VersionProbe {
    api_version: u32,
}

fn ensure_supported_api_version(api_version: u32) -> Result<(), LanguageError> {
    if api_version == WIRE_API_VERSION {
        Ok(())
    } else {
        Err(LanguageError::UnsupportedApiVersion {
            found: api_version,
            supported: WIRE_API_VERSION,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use crate::subprocess_exchange::ExchangeError;

    use super::{LanguageError, map_exchange_error};

    #[test]
    fn maps_shared_stdout_read_errors_to_language_errors() {
        let error = map_exchange_error(ExchangeError::ReadStream {
            stream: "stdout",
            source: io::Error::other("read failed"),
        });

        assert!(matches!(error, LanguageError::ReadStdout { .. }));
    }
}
