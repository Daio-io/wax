//! Subprocess-backed language pack registry discovery.
//!
//! The shared subprocess exchange owns stdio transport, spooling, and cleanup.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
use std::process::ExitStatus;
use std::time::Duration;

use thiserror::Error;
use wax_contract::{
    Diagnostic, RegistryComponent, RegistryDocument, RegistryExtracted, RegistryExtractionStatus,
};
use wax_lang_api::{
    DiscoverRegistryRequest, DiscoverRegistryResult, DiscoverRequest, DiscoverRequestType,
    DiscoveredRegistrySymbol, WIRE_API_VERSION, WireErrorCode, WirePackRequest, WirePackResponse,
    normalize_discovered_components,
};

use crate::subprocess_exchange::{ExchangeError, ExchangeOutput, ExchangeRequest, run_exchange};
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

    /// Probes for rich discovery and returns an enriched registry document when supported.
    ///
    /// Packs that explicitly report no rich-discovery capability, or legacy packs that reject the
    /// fixed capability probe with [`WireErrorCode::ConfigInvalid`], are queried through the legacy
    /// symbol-discovery envelope. The supplied registry is then returned with extraction marked
    /// unavailable.
    ///
    /// # Errors
    ///
    /// Returns the same process, transport, timeout, cancellation, protocol, and typed wire errors
    /// as [`Self::discover`]. Only an explicit lack of capability or the legacy
    /// `ConfigInvalid` response to the capability probe enables fallback.
    pub fn discover_registry(
        &self,
        request: DiscoverRegistryRequest,
    ) -> Result<DiscoverRegistryResult, DiscoverError> {
        self.discover_registry_with_cancellation(request, &LanguageCancellationToken::new())
    }

    /// Runs rich registry discovery unless cancellation is requested first.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoverError::Cancelled`] when cancellation wins, plus the process, transport,
    /// protocol, and typed wire errors documented by [`Self::discover_registry`].
    pub fn discover_registry_with_cancellation(
        &self,
        request: DiscoverRegistryRequest,
        cancellation: &LanguageCancellationToken,
    ) -> Result<DiscoverRegistryResult, DiscoverError> {
        run_subprocess_registry_discover(&self.manifest, request, cancellation)
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
    /// Two discovered symbols collapsed to the same generated registry id.
    #[error(
        "discovered symbols `{first_symbol}` and `{second_symbol}` both generate registry id `{id}`"
    )]
    IdCollision {
        /// Generated registry id that collided.
        id: String,
        /// First symbol that produced the id.
        first_symbol: String,
        /// Second symbol that produced the id.
        second_symbol: String,
    },
}

fn run_subprocess_discover(
    manifest: &SubprocessLanguageManifest,
    request: DiscoverRequest,
    cancellation: &LanguageCancellationToken,
) -> Result<DiscoverSymbolsResult, DiscoverError> {
    let response = exchange_wire_request(
        manifest,
        WirePackRequest::from(request),
        cancellation,
        "discover-stdout",
        false,
    )?;
    parse_discover_response(response)
}

fn run_subprocess_registry_discover(
    manifest: &SubprocessLanguageManifest,
    request: DiscoverRegistryRequest,
    cancellation: &LanguageCancellationToken,
) -> Result<DiscoverRegistryResult, DiscoverError> {
    let capability_response = exchange_wire_request(
        manifest,
        WirePackRequest::Capabilities {
            api_version: request.api_version,
            language_id: request.language_id.clone(),
        },
        cancellation,
        "capabilities-stdout",
        true,
    )?;

    match capability_response {
        WirePackResponse::Capabilities {
            registry_details_v1: true,
            ..
        } => {
            let response = exchange_wire_request(
                manifest,
                WirePackRequest::from(request),
                cancellation,
                "registry-details-stdout",
                true,
            )?;
            parse_registry_details_response(response)
        }
        WirePackResponse::Capabilities {
            registry_details_v1: false,
            ..
        }
        | WirePackResponse::Error {
            code: WireErrorCode::ConfigInvalid,
            ..
        } => run_legacy_registry_fallback(manifest, request, cancellation),
        response => Err(response_error(response)),
    }
}

fn run_legacy_registry_fallback(
    manifest: &SubprocessLanguageManifest,
    request: DiscoverRegistryRequest,
    cancellation: &LanguageCancellationToken,
) -> Result<DiscoverRegistryResult, DiscoverError> {
    let legacy_request = DiscoverRequest {
        request_type: DiscoverRequestType::Discover,
        api_version: request.api_version,
        language_id: request.language_id,
        repo_root: request.repo_root,
        roots: request.roots,
    };
    let legacy = run_subprocess_discover(manifest, legacy_request, cancellation)?;
    Ok(DiscoverRegistryResult {
        document: normalize_legacy_registry(request.registry, &legacy.components)?,
        diagnostics: legacy.diagnostics,
    })
}

fn normalize_legacy_registry(
    mut document: RegistryDocument,
    discovered: &[DiscoveredRegistrySymbol],
) -> Result<RegistryDocument, DiscoverError> {
    let discovered = normalized_symbol_components(discovered.to_vec());
    let mut seen_ids = document
        .components
        .iter()
        .map(|component| (component.id.clone(), component.symbol.clone()))
        .collect::<BTreeMap<_, _>>();

    for candidate in discovered {
        if let Some(component) = document
            .components
            .iter_mut()
            .find(|component| component.symbol == candidate.symbol)
        {
            if component.package.is_none() {
                component.package.clone_from(&candidate.package);
            }
            component.extracted = Some(unavailable_extraction());
            continue;
        }

        let id = format!("ds.{}", kebab_case_symbol(&candidate.symbol));
        if let Some(first_symbol) = seen_ids.insert(id.clone(), candidate.symbol.clone()) {
            return Err(DiscoverError::IdCollision {
                id,
                first_symbol,
                second_symbol: candidate.symbol,
            });
        }
        document.components.push(RegistryComponent {
            id,
            symbol: candidate.symbol,
            package: candidate.package,
            aliases: Vec::new(),
            metadata: None,
            extracted: Some(unavailable_extraction()),
            extra: serde_json::Map::new(),
        });
    }
    Ok(document)
}

fn normalized_symbol_components(
    components: Vec<DiscoveredRegistrySymbol>,
) -> Vec<DiscoveredRegistrySymbol> {
    components
        .into_iter()
        .fold(BTreeMap::new(), |mut acc, component| {
            acc.entry(component.symbol.clone()).or_insert(component);
            acc
        })
        .into_values()
        .collect()
}

fn kebab_case_symbol(symbol: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = symbol.chars().collect();
    for (index, character) in chars.iter().copied().enumerate() {
        if character.is_ascii_uppercase() {
            let previous = index
                .checked_sub(1)
                .and_then(|previous| chars.get(previous));
            let next = chars.get(index + 1);
            if !result.is_empty()
                && (previous
                    .is_some_and(|value| value.is_ascii_lowercase() || value.is_ascii_digit())
                    || (previous.is_some_and(|value| value.is_ascii_uppercase())
                        && next.is_some_and(|value| value.is_ascii_lowercase())))
            {
                result.push('-');
            }
            result.push(character.to_ascii_lowercase());
        } else if character.is_ascii_alphanumeric() {
            result.push(character.to_ascii_lowercase());
        } else if !result.ends_with('-') && !result.is_empty() {
            result.push('-');
        }
    }
    result.trim_matches('-').to_owned()
}

fn unavailable_extraction() -> RegistryExtracted {
    RegistryExtracted {
        status: RegistryExtractionStatus::Unavailable,
        source_evidence: None,
        documentation: None,
        signature: None,
        deprecation: None,
        attachments: Vec::new(),
        field_gaps: Vec::new(),
        best_practices: Vec::new(),
        extra: serde_json::Map::new(),
    }
}

fn serialize_wire_request(request: &WirePackRequest) -> Result<Vec<u8>, DiscoverError> {
    serde_json::to_vec(request).map_err(|source| DiscoverError::WriteRequest {
        source: Box::new(source),
    })
}

fn exchange_wire_request(
    manifest: &SubprocessLanguageManifest,
    request: WirePackRequest,
    cancellation: &LanguageCancellationToken,
    stdout_kind: &'static str,
    fail_on_nonzero_exit: bool,
) -> Result<WirePackResponse, DiscoverError> {
    let request = serialize_wire_request(&request)?;
    let exchange = run_exchange(ExchangeRequest {
        command: &manifest.command,
        request: &request,
        timeout: manifest.timeout,
        cancellation,
        stdout_kind,
    })
    .map_err(map_exchange_error)?;
    parse_exchange_response(exchange, fail_on_nonzero_exit)
}

fn parse_exchange_response(
    exchange: ExchangeOutput,
    fail_on_nonzero_exit: bool,
) -> Result<WirePackResponse, DiscoverError> {
    if fail_on_nonzero_exit && !exchange.status.success() {
        return Err(process_failure(exchange));
    }
    match read_wire_response(exchange.stdout_path()) {
        Ok(response) => Ok(response),
        Err(_parse_error) if !exchange.status.success() => Err(process_failure(exchange)),
        Err(parse_error) => Err(parse_error),
    }
}

fn process_failure(exchange: ExchangeOutput) -> DiscoverError {
    DiscoverError::ProcessFailed {
        status: exchange.status,
        stderr: String::from_utf8_lossy(
            &exchange
                .stderr_bytes(STDERR_PREVIEW_BYTES)
                .unwrap_or_default(),
        )
        .trim()
        .to_owned(),
    }
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
        ExchangeError::MissingPipe { stream } => DiscoverError::WriteRequest {
            source: Box::new(io::Error::new(
                io::ErrorKind::BrokenPipe,
                format!("{stream} pipe was not configured"),
            )),
        },
    }
}

fn map_stream_error(stream: &'static str, source: io::Error) -> DiscoverError {
    match stream {
        "stdout" => DiscoverError::ReadStdout { source },
        "stderr" => DiscoverError::ReadStderr { source },
        _ => unreachable!("unexpected exchange stream: {stream}"),
    }
}

fn read_wire_response(stdout: &Path) -> Result<WirePackResponse, DiscoverError> {
    let probe_file = File::open(stdout).map_err(|source| DiscoverError::ReadStdout { source })?;
    let probe: VersionProbe = serde_json::from_reader(BufReader::new(probe_file))
        .map_err(|source| DiscoverError::InvalidResponse { source })?;
    ensure_supported_api_version(probe.api_version)?;

    let response: WirePackResponse = serde_json::from_reader(BufReader::new(
        File::open(stdout).map_err(|source| DiscoverError::ReadStdout { source })?,
    ))
    .map_err(|source| DiscoverError::InvalidResponse { source })?;
    Ok(response)
}

fn parse_discover_response(
    response: WirePackResponse,
) -> Result<DiscoverSymbolsResult, DiscoverError> {
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
            Err(map_wire_error(code, message, diagnostics))
        }
        WirePackResponse::ScanFacts { .. } => Err(DiscoverError::UnexpectedResponseType {
            found: "scan_facts",
        }),
        WirePackResponse::Capabilities { .. } => Err(DiscoverError::UnexpectedResponseType {
            found: "capabilities",
        }),
        WirePackResponse::RegistryDetails { .. } => Err(DiscoverError::UnexpectedResponseType {
            found: "registry_details",
        }),
    }
}

fn parse_registry_details_response(
    response: WirePackResponse,
) -> Result<DiscoverRegistryResult, DiscoverError> {
    match response {
        WirePackResponse::RegistryDetails {
            api_version,
            document,
            diagnostics,
            ..
        } => {
            ensure_supported_api_version(api_version)?;
            Ok(DiscoverRegistryResult {
                document,
                diagnostics,
            })
        }
        response => Err(response_error(response)),
    }
}

fn response_error(response: WirePackResponse) -> DiscoverError {
    match response {
        WirePackResponse::Error {
            code,
            message,
            diagnostics,
            ..
        } => map_wire_error(code, message, diagnostics),
        WirePackResponse::Capabilities { .. } => DiscoverError::UnexpectedResponseType {
            found: "capabilities",
        },
        WirePackResponse::ScanFacts { .. } => DiscoverError::UnexpectedResponseType {
            found: "scan_facts",
        },
        WirePackResponse::DiscoverSymbols { .. } => DiscoverError::UnexpectedResponseType {
            found: "discover_symbols",
        },
        WirePackResponse::RegistryDetails { .. } => DiscoverError::UnexpectedResponseType {
            found: "registry_details",
        },
    }
}

fn map_wire_error(
    code: WireErrorCode,
    message: String,
    diagnostics: Vec<Diagnostic>,
) -> DiscoverError {
    match code {
        WireErrorCode::Timeout => DiscoverError::WireTimeout {
            message,
            diagnostics,
        },
        WireErrorCode::DiscoverUnsupported => DiscoverError::Unsupported {
            message,
            diagnostics,
        },
        _ => DiscoverError::Wire {
            code,
            message,
            diagnostics,
        },
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
