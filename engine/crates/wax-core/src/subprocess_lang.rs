//! Subprocess-backed language pack extraction.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;
use wax_contract::{Diagnostic, ScanFacts};
use wax_lang_api::{
    ScanRequest, WIRE_API_VERSION, WireErrorCode, WireScanRequest, WireScanResponse,
};

const TERMINATION_GRACE: Duration = Duration::from_secs(5);
const STDERR_PREVIEW_BYTES: u64 = 64 * 1024;

static SPOOL_COUNTER: AtomicU64 = AtomicU64::new(0);

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
    let (program, args) = manifest
        .command
        .split_first()
        .ok_or(LanguageError::EmptyCommand)?;
    let request = serialize_wire_request(request)?;

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_command(&mut command);

    let mut child = command.spawn().map_err(|source| LanguageError::Spawn {
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
        Ok(facts) => Ok(facts),
        Err(_parse_error) if !exchange.status.success() => Err(LanguageError::ProcessFailed {
            status: exchange.status,
            stderr: read_lossy_prefix(exchange.stderr.path(), STDERR_PREVIEW_BYTES)
                .trim()
                .to_owned(),
        }),
        Err(parse_error) => Err(parse_error),
    }
}

fn serialize_wire_request(request: ScanRequest) -> Result<Vec<u8>, LanguageError> {
    let wire_request = WireScanRequest::from(request);
    let mut request =
        serde_json::to_vec(&wire_request).map_err(|source| LanguageError::WriteRequest {
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
            StreamKind::Stdout => ("stdout", "json"),
            StreamKind::Stderr => ("stderr", "log"),
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
) -> Result<ExchangeOutput, LanguageError> {
    let started_at = Instant::now();
    let mut stdin_done = false;
    let mut status = None;
    let mut stdout = None;
    let mut stderr = None;

    loop {
        if cancellation.is_cancelled() {
            cleanup_child(child, child_id);
            return Err(LanguageError::Cancelled);
        }

        match stdin_rx.try_recv() {
            Ok(Ok(())) => stdin_done = true,
            Ok(Err(source)) => {
                cleanup_child(child, child_id);
                return Err(LanguageError::WriteRequest {
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
                        return Err(LanguageError::ReadStdout { source });
                    }
                    (StreamKind::Stderr, Err(source)) => {
                        cleanup_child(child, child_id);
                        return Err(LanguageError::ReadStderr { source });
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
                    return Err(LanguageError::Wait { source });
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
            return Err(LanguageError::Timeout { timeout });
        }

        let remaining = timeout.saturating_sub(started_at.elapsed());
        thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

#[cfg(unix)]
fn configure_command(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_command(_command: &mut Command) {}

#[cfg(unix)]
fn cleanup_child(child: &mut std::process::Child, child_id: u32) {
    if let Ok(process_group_id) = i32::try_from(child_id) {
        // NOTE: We apply the spec's SIGTERM grace window uniformly for every
        // cleanup path, including timeouts and pipe errors, so packs get one
        // consistent shutdown contract.
        signal_process_group(process_group_id, libc::SIGTERM);
        let grace_started_at = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if grace_started_at.elapsed() < TERMINATION_GRACE => {
                    thread::sleep(Duration::from_millis(25));
                }
                Ok(None) => break,
                Err(source) if source.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        signal_process_group(process_group_id, libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
#[expect(
    unsafe_code,
    reason = "std cannot signal a Unix process group, so cleanup must call libc::kill with a negative pgid to terminate the spawned pack group"
)]
fn signal_process_group(process_group_id: i32, signal: libc::c_int) {
    // SAFETY: `process_group_id` comes from a spawned child id that fit in `i32`.
    // Passing its negated value asks `kill` to signal that process group; the
    // signal constants are provided by libc for the current Unix target.
    unsafe {
        libc::kill(-process_group_id, signal);
    }
}

#[cfg(not(unix))]
fn cleanup_child(child: &mut std::process::Child, _child_id: u32) {
    let _ = child.kill();
    let _ = child.wait();
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

#[cfg(test)]
mod tests {
    use super::{SpooledOutput, StreamKind};

    #[cfg(unix)]
    #[test]
    fn subprocess_spool_files_are_user_only() {
        use std::os::unix::fs::PermissionsExt;

        let output = SpooledOutput::create(StreamKind::Stdout).unwrap();

        let mode = output.path().metadata().unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
