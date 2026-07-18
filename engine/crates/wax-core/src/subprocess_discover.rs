//! Subprocess-backed language pack registry discovery.
//!
//! The stdio exchange, spooling, and cleanup logic intentionally mirrors
//! [`crate::subprocess_lang`] for this task. Extract shared helpers once the
//! discover path stabilizes.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;
use wax_contract::Diagnostic;
use wax_lang_api::{
    DiscoverRequest, DiscoveredRegistrySymbol, WIRE_API_VERSION, WireErrorCode, WirePackRequest,
    WirePackResponse, normalize_discovered_components,
};

use crate::process_control::{configure_process_group, terminate_child_tree};
use crate::subprocess_lang::{LanguageCancellationToken, SubprocessLanguageManifest};

const TERMINATION_GRACE: Duration = Duration::from_secs(5);
const STDERR_PREVIEW_BYTES: u64 = 64 * 1024;

static SPOOL_COUNTER: AtomicU64 = AtomicU64::new(0);

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
    /// Returns a typed [`DiscoverError`] for command, process, timeout, or wire
    /// protocol failures.
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
    /// Returns a typed [`DiscoverError`] for command, process, cancellation,
    /// timeout, or wire protocol failures.
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
    let (program, args) = manifest
        .command
        .split_first()
        .ok_or(DiscoverError::EmptyCommand)?;
    let request = serialize_wire_request(request)?;

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_group(&mut command);

    let mut child = command.spawn().map_err(|source| DiscoverError::Spawn {
        command: program.clone(),
        source,
    })?;
    let child_id = child.id();

    let stdin = child.stdin.take().expect("stdin was configured as piped");
    let stdout = child.stdout.take().expect("stdout was configured as piped");
    let stderr = child.stderr.take().expect("stderr was configured as piped");

    let stdin_rx = write_request_async(stdin, request);
    let stream_rx = read_streams_async(stdout, stderr);

    let exchange = wait_for_exchange(
        &mut child,
        child_id,
        stdin_rx,
        stream_rx,
        manifest.timeout,
        cancellation,
    )?;

    match parse_wire_response(exchange.stdout.path()) {
        Ok(result) => Ok(result),
        Err(_parse_error) if !exchange.status.success() => Err(DiscoverError::ProcessFailed {
            status: exchange.status,
            stderr: read_lossy_prefix(exchange.stderr.path(), STDERR_PREVIEW_BYTES)
                .trim()
                .to_owned(),
        }),
        Err(parse_error) => Err(parse_error),
    }
}

fn serialize_wire_request(request: DiscoverRequest) -> Result<Vec<u8>, DiscoverError> {
    let wire_request = WirePackRequest::from(request);
    let mut request =
        serde_json::to_vec(&wire_request).map_err(|source| DiscoverError::WriteRequest {
            source: Box::new(source),
        })?;
    request.push(b'\n');
    Ok(request)
}

fn write_request_async(
    mut stdin: std::process::ChildStdin,
    request: Vec<u8>,
) -> mpsc::Receiver<io::Result<()>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = stdin.write_all(&request).and_then(|_| stdin.flush());
        drop(stdin);
        let _ = tx.send(result);
    });
    rx
}

fn read_streams_async(
    stdout: std::process::ChildStdout,
    stderr: std::process::ChildStderr,
) -> mpsc::Receiver<StreamRead> {
    let (tx, rx) = mpsc::channel();
    spawn_reader(StreamKind::Stdout, stdout, tx.clone());
    spawn_reader(StreamKind::Stderr, stderr, tx);
    rx
}

fn spawn_reader<R>(kind: StreamKind, mut stream: R, tx: mpsc::Sender<StreamRead>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let result = SpooledOutput::create(kind).and_then(|mut output| {
            io::copy(&mut stream, output.file_mut())?;
            Ok(output)
        });
        let _ = tx.send(StreamRead { kind, result });
    });
}

struct ExchangeOutput {
    status: ExitStatus,
    stdout: SpooledOutput,
    stderr: SpooledOutput,
}

#[derive(Clone, Copy)]
enum StreamKind {
    Stdout,
    Stderr,
}

struct StreamRead {
    kind: StreamKind,
    result: io::Result<SpooledOutput>,
}

struct SpooledOutput {
    path: PathBuf,
    file: File,
}

impl SpooledOutput {
    fn create(kind: StreamKind) -> io::Result<Self> {
        let (kind, extension) = match kind {
            StreamKind::Stdout => ("discover-stdout", "json"),
            StreamKind::Stderr => ("discover-stderr", "log"),
        };

        let counter = SPOOL_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "wax-core-subprocess-{kind}-{}-{counter}.{extension}",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;

            options.mode(0o600);
        }
        options.open(&path).map(|file| Self { path, file })
    }

    fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SpooledOutput {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn wait_for_exchange(
    child: &mut std::process::Child,
    child_id: u32,
    stdin_rx: mpsc::Receiver<io::Result<()>>,
    stream_rx: mpsc::Receiver<StreamRead>,
    timeout: Duration,
    cancellation: &LanguageCancellationToken,
) -> Result<ExchangeOutput, DiscoverError> {
    let started_at = Instant::now();
    let mut stdin_done = false;
    let mut status = None;
    let mut stdout = None;
    let mut stderr = None;

    loop {
        if cancellation.is_cancelled() {
            cleanup_child(child, child_id);
            return Err(DiscoverError::Cancelled);
        }

        match stdin_rx.try_recv() {
            Ok(Ok(())) => stdin_done = true,
            Ok(Err(source)) => {
                cleanup_child(child, child_id);
                return Err(DiscoverError::WriteRequest {
                    source: Box::new(source),
                });
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                stdin_done = true;
            }
        }

        loop {
            match stream_rx.try_recv() {
                Ok(read) => match (read.kind, read.result) {
                    (StreamKind::Stdout, Ok(output)) => stdout = Some(output),
                    (StreamKind::Stderr, Ok(output)) => stderr = Some(output),
                    (StreamKind::Stdout, Err(source)) => {
                        cleanup_child(child, child_id);
                        return Err(DiscoverError::ReadStdout { source });
                    }
                    (StreamKind::Stderr, Err(source)) => {
                        cleanup_child(child, child_id);
                        return Err(DiscoverError::ReadStderr { source });
                    }
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        if status.is_none() {
            match child.try_wait() {
                Ok(Some(exit_status)) => status = Some(exit_status),
                Ok(None) => {}
                Err(source) if source.kind() == io::ErrorKind::Interrupted => continue,
                Err(source) => {
                    cleanup_child(child, child_id);
                    return Err(DiscoverError::Wait { source });
                }
            }
        }

        if stdin_done && let Some(exit_status) = status.take() {
            match (stdout.take(), stderr.take()) {
                (Some(stdout), Some(stderr)) => {
                    return Ok(ExchangeOutput {
                        status: exit_status,
                        stdout,
                        stderr,
                    });
                }
                (stdout_value, stderr_value) => {
                    status = Some(exit_status);
                    stdout = stdout_value;
                    stderr = stderr_value;
                }
            }
        }

        if started_at.elapsed() >= timeout {
            cleanup_child(child, child_id);
            return Err(DiscoverError::Timeout { timeout });
        }

        let remaining = timeout.saturating_sub(started_at.elapsed());
        thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

fn cleanup_child(child: &mut std::process::Child, child_id: u32) {
    if let Err(error) = terminate_child_tree(child, child_id, TERMINATION_GRACE) {
        eprintln!("failed to clean up discovery subprocess: {error}");
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

fn read_lossy_prefix(path: &Path, max_bytes: u64) -> String {
    let Ok(file) = File::open(path) else {
        return String::new();
    };
    let mut buffer = Vec::new();
    if file.take(max_bytes).read_to_end(&mut buffer).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buffer).into_owned()
}
