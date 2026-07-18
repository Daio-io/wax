//! Shared transport for one language-pack subprocess exchange.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::process_control::{configure_process_group, terminate_child_tree};
use crate::subprocess_lang::LanguageCancellationToken;

const TERMINATION_GRACE: Duration = Duration::from_secs(5);

static SPOOL_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Borrowed inputs for one language-pack process exchange.
pub(crate) struct ExchangeRequest<'a> {
    /// Command executable and arguments.
    pub(crate) command: &'a [String],
    /// Encoded wire request, without a required trailing newline.
    pub(crate) request: &'a [u8],
    /// Maximum wall-clock duration for the exchange.
    pub(crate) timeout: Duration,
    /// Signal used to cancel the exchange.
    pub(crate) cancellation: &'a LanguageCancellationToken,
    /// Filename component used for the stdout spool.
    pub(crate) stdout_kind: &'static str,
}

/// Completed subprocess exchange with owned spool files.
#[derive(Debug)]
pub(crate) struct ExchangeOutput {
    /// Exit status returned by the child process.
    pub(crate) status: ExitStatus,
    stdout: SpooledOutput,
    stderr: SpooledOutput,
}

impl ExchangeOutput {
    /// Returns the path containing the complete stdout stream.
    pub(crate) fn stdout_path(&self) -> &Path {
        self.stdout.path()
    }

    /// Returns at most `max_bytes` from the beginning of stderr.
    pub(crate) fn stderr_bytes(&self, max_bytes: usize) -> io::Result<Vec<u8>> {
        let file = File::open(self.stderr.path())?;
        let mut bytes = Vec::new();
        file.take(max_bytes as u64).read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

/// Transport failures that adapters map to their stable public errors.
#[derive(Debug, Error)]
pub(crate) enum ExchangeError {
    /// The command had no executable element.
    #[error("language subprocess command is empty")]
    EmptyCommand,
    /// The child process could not be spawned.
    #[error("failed to spawn language subprocess {program:?}: {source}")]
    Spawn {
        /// Command executable that failed to spawn.
        program: String,
        /// Underlying operating-system error.
        #[source]
        source: io::Error,
    },
    /// A stream spool file could not be created.
    #[error("failed to create language subprocess {stream} spool: {source}")]
    CreateSpool {
        /// Stream whose spool file could not be created.
        stream: &'static str,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },
    /// The request could not be written to child stdin.
    #[error("failed to write language subprocess request: {source}")]
    WriteRequest {
        /// Underlying pipe error.
        #[source]
        source: io::Error,
    },
    /// A child output stream could not be copied to its spool file.
    #[error("failed to read language subprocess {stream}: {source}")]
    ReadStream {
        /// Stream whose read failed.
        stream: &'static str,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The child status could not be polled.
    #[error("failed to wait for language subprocess: {source}")]
    Wait {
        /// Underlying process error.
        #[source]
        source: io::Error,
    },
    /// The configured timeout elapsed before the exchange completed.
    #[error("language subprocess timed out after {timeout:?}")]
    Timeout {
        /// Configured timeout that elapsed.
        timeout: Duration,
        /// Cleanup failure, if the child tree could not be reaped.
        #[source]
        cleanup: Option<io::Error>,
    },
    /// Cancellation was requested before the exchange completed.
    #[error("language subprocess exchange was cancelled")]
    Cancelled {
        /// Cleanup failure, if the child tree could not be reaped.
        #[source]
        cleanup: Option<io::Error>,
    },
}

/// Runs one newline-delimited request/response exchange with a language pack.
pub(crate) fn run_exchange(request: ExchangeRequest<'_>) -> Result<ExchangeOutput, ExchangeError> {
    let (program, args) = request
        .command
        .split_first()
        .ok_or(ExchangeError::EmptyCommand)?;

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_group(&mut command);

    let mut child = command.spawn().map_err(|source| ExchangeError::Spawn {
        program: program.clone(),
        source,
    })?;
    let child_id = child.id();

    let stdout = match SpooledOutput::create(request.stdout_kind, "json") {
        Ok(stdout) => stdout,
        Err(source) => {
            cleanup_child(&mut child, child_id, true);
            return Err(ExchangeError::CreateSpool {
                stream: "stdout",
                source,
            });
        }
    };
    let stderr = match SpooledOutput::create(stderr_kind(request.stdout_kind), "log") {
        Ok(stderr) => stderr,
        Err(source) => {
            cleanup_child(&mut child, child_id, true);
            return Err(ExchangeError::CreateSpool {
                stream: "stderr",
                source,
            });
        }
    };

    let stdin = child.stdin.take().expect("stdin was configured as piped");
    let child_stdout = child.stdout.take().expect("stdout was configured as piped");
    let child_stderr = child.stderr.take().expect("stderr was configured as piped");

    thread::scope(|scope| {
        let (stream_tx, stream_rx) = mpsc::channel();
        spawn_reader(scope, "stdout", child_stdout, stdout, stream_tx.clone());
        spawn_reader(scope, "stderr", child_stderr, stderr, stream_tx);

        let (stdin_tx, stdin_rx) = mpsc::channel();
        scope.spawn(move || {
            let result = write_request(stdin, request.request);
            let _ = stdin_tx.send(result);
        });

        wait_for_exchange(
            &mut child,
            child_id,
            stdin_rx,
            stream_rx,
            request.timeout,
            request.cancellation,
        )
    })
}

fn stderr_kind(stdout_kind: &'static str) -> String {
    stdout_kind
        .strip_suffix("stdout")
        .map_or_else(|| "stderr".to_owned(), |prefix| format!("{prefix}stderr"))
}

fn write_request(mut stdin: std::process::ChildStdin, request: &[u8]) -> io::Result<()> {
    stdin.write_all(request)?;
    if !request.ends_with(b"\n") {
        stdin.write_all(b"\n")?;
    }
    stdin.flush()
}

fn spawn_reader<'scope, R>(
    scope: &'scope thread::Scope<'scope, '_>,
    stream: &'static str,
    mut input: R,
    mut output: SpooledOutput,
    tx: mpsc::Sender<StreamRead>,
) where
    R: Read + Send + 'scope,
{
    scope.spawn(move || {
        let result = io::copy(&mut input, output.file_mut()).map(|_| output);
        let _ = tx.send(StreamRead { stream, result });
    });
}

fn wait_for_exchange(
    child: &mut std::process::Child,
    child_id: u32,
    stdin_rx: mpsc::Receiver<io::Result<()>>,
    stream_rx: mpsc::Receiver<StreamRead>,
    timeout: Duration,
    cancellation: &LanguageCancellationToken,
) -> Result<ExchangeOutput, ExchangeError> {
    let started_at = Instant::now();
    let mut stdin_done = false;
    let mut status = None;
    let mut stdout = None;
    let mut stderr = None;

    loop {
        if cancellation.is_cancelled() {
            return Err(ExchangeError::Cancelled {
                cleanup: cleanup_child(child, child_id, status.is_none()),
            });
        }

        match stdin_rx.try_recv() {
            Ok(Ok(())) => stdin_done = true,
            Ok(Err(source)) => {
                cleanup_child(child, child_id, status.is_none());
                return Err(ExchangeError::WriteRequest { source });
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => stdin_done = true,
        }

        loop {
            match stream_rx.try_recv() {
                Ok(StreamRead {
                    stream: "stdout",
                    result: Ok(output),
                }) => stdout = Some(output),
                Ok(StreamRead {
                    stream: "stderr",
                    result: Ok(output),
                }) => stderr = Some(output),
                Ok(StreamRead {
                    stream,
                    result: Err(source),
                }) => {
                    cleanup_child(child, child_id, status.is_none());
                    return Err(ExchangeError::ReadStream { stream, source });
                }
                Ok(StreamRead { stream, .. }) => unreachable!("unexpected stream kind: {stream}"),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }

        if stdin_done && stdout.is_some() && stderr.is_some() && status.is_none() {
            match child.try_wait() {
                Ok(Some(exit_status)) => status = Some(exit_status),
                Ok(None) => {}
                Err(source) if source.kind() == io::ErrorKind::Interrupted => continue,
                Err(source) => {
                    cleanup_child(child, child_id, status.is_none());
                    return Err(ExchangeError::Wait { source });
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
            return Err(ExchangeError::Timeout {
                timeout,
                cleanup: cleanup_child(child, child_id, status.is_none()),
            });
        }

        let remaining = timeout.saturating_sub(started_at.elapsed());
        thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

fn cleanup_child(
    child: &mut std::process::Child,
    child_id: u32,
    child_is_unreaped: bool,
) -> Option<io::Error> {
    if !child_is_unreaped {
        return None;
    }
    terminate_child_tree(child, child_id, TERMINATION_GRACE).err()
}

struct StreamRead {
    stream: &'static str,
    result: io::Result<SpooledOutput>,
}

#[derive(Debug)]
struct SpooledOutput {
    path: PathBuf,
    file: File,
}

impl SpooledOutput {
    fn create(kind: impl AsRef<str>, extension: &str) -> io::Result<Self> {
        let counter = SPOOL_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "wax-core-subprocess-{}-{}-{counter}.{extension}",
            kind.as_ref(),
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

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::error::Error;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use crate::subprocess_lang::LanguageCancellationToken;

    use super::{ExchangeError, ExchangeRequest, run_exchange};

    #[test]
    fn rejects_an_empty_command_before_spawning() {
        let cancellation = LanguageCancellationToken::new();

        let error = run_exchange(ExchangeRequest {
            command: &[],
            request: br#"{}"#,
            timeout: Duration::from_secs(1),
            cancellation: &cancellation,
            stdout_kind: "test-stdout",
        })
        .unwrap_err();

        assert!(matches!(error, ExchangeError::EmptyCommand));
    }

    #[test]
    fn appends_one_newline_and_closes_stdin() {
        let temp_dir = TestDir::new("request-framing");
        let request_path = temp_dir.path().join("request.json");
        let script_path = temp_dir.path().join("pack.sh");
        write_script(
            &script_path,
            r#"#!/bin/sh
cat > "$1"
printf 'done'
"#,
        );
        let command = [
            script_path.to_string_lossy().into_owned(),
            request_path.to_string_lossy().into_owned(),
        ];
        let cancellation = LanguageCancellationToken::new();

        let output = run_exchange(ExchangeRequest {
            command: &command,
            request: br#"{"request":true}"#,
            timeout: Duration::from_secs(1),
            cancellation: &cancellation,
            stdout_kind: "test-stdout",
        })
        .unwrap();

        assert!(output.status.success());
        assert_eq!(fs::read(&request_path).unwrap(), b"{\"request\":true}\n");
        assert_eq!(fs::read(output.stdout_path()).unwrap(), b"done");
    }

    #[test]
    fn drains_large_stdout_and_stderr_concurrently() {
        let temp_dir = TestDir::new("large-streams");
        let script_path = temp_dir.path().join("pack.sh");
        write_script(
            &script_path,
            r#"#!/bin/sh
dd if=/dev/zero bs=1024 count=512 2>/dev/null
dd if=/dev/zero bs=1024 count=512 1>&2 2>/dev/null
cat >/dev/null
"#,
        );
        let command = [script_path.to_string_lossy().into_owned()];
        let cancellation = LanguageCancellationToken::new();

        let output = run_exchange(ExchangeRequest {
            command: &command,
            request: b"{}\n",
            timeout: Duration::from_secs(2),
            cancellation: &cancellation,
            stdout_kind: "test-stdout",
        })
        .unwrap();

        assert_eq!(
            fs::metadata(output.stdout_path()).unwrap().len(),
            512 * 1024
        );
        assert_eq!(output.stderr_bytes(512 * 1024).unwrap().len(), 512 * 1024);
    }

    #[test]
    fn bounds_stderr_extraction_without_classifying_the_exit_status() {
        let temp_dir = TestDir::new("stderr-prefix");
        let script_path = temp_dir.path().join("pack.sh");
        write_script(
            &script_path,
            "#!/bin/sh\ncat >/dev/null\nprintf 'abcdefghij' 1>&2\nexit 9\n",
        );
        let command = [script_path.to_string_lossy().into_owned()];
        let cancellation = LanguageCancellationToken::new();

        let output = run_exchange(ExchangeRequest {
            command: &command,
            request: b"{}",
            timeout: Duration::from_secs(1),
            cancellation: &cancellation,
            stdout_kind: "test-stdout",
        })
        .unwrap();

        assert!(!output.status.success());
        assert_eq!(output.stderr_bytes(4).unwrap(), b"abcd");
    }

    #[test]
    fn cancellation_wins_before_timeout() {
        let temp_dir = TestDir::new("cancelled");
        let script_path = temp_dir.path().join("pack.sh");
        write_script(&script_path, "#!/bin/sh\ncat >/dev/null\nsleep 10\n");
        let command = [script_path.to_string_lossy().into_owned()];
        let cancellation = LanguageCancellationToken::new();
        cancellation.cancel();

        let error = run_exchange(ExchangeRequest {
            command: &command,
            request: b"{}",
            timeout: Duration::from_millis(1),
            cancellation: &cancellation,
            stdout_kind: "test-stdout",
        })
        .unwrap_err();

        assert!(matches!(error, ExchangeError::Cancelled { .. }));
    }

    #[test]
    fn timeout_reaps_the_child_tree_and_drops_its_spools() {
        let temp_dir = TestDir::new("timeout-cleanup");
        let pid_path = temp_dir.path().join("pack.pid");
        let script_path = temp_dir.path().join("pack.sh");
        write_script(
            &script_path,
            r#"#!/bin/sh
echo "$$" > "$1"
cat >/dev/null
while :; do sleep 1; done
"#,
        );
        let command = [
            script_path.to_string_lossy().into_owned(),
            pid_path.to_string_lossy().into_owned(),
        ];
        let cancellation = LanguageCancellationToken::new();

        let error = run_exchange(ExchangeRequest {
            command: &command,
            request: b"{}",
            timeout: Duration::from_secs(5),
            cancellation: &cancellation,
            stdout_kind: "timeout-cleanup-stdout",
        })
        .unwrap_err();

        assert!(matches!(error, ExchangeError::Timeout { .. }));
        assert_process_exited(fs::read_to_string(pid_path).unwrap().trim());
        assert!(spool_paths("timeout-cleanup-stdout").is_empty());
        assert!(spool_paths("timeout-cleanup-stderr").is_empty());
    }

    #[test]
    fn timeout_terminates_descendants_that_hold_output_pipes_open() {
        let temp_dir = TestDir::new("timeout-descendant-pipe");
        let script_path = temp_dir.path().join("pack.sh");
        write_script(&script_path, "#!/bin/sh\n(sleep 30) &\nexit 0\n");
        let command = [script_path.to_string_lossy().into_owned()];
        let cancellation = LanguageCancellationToken::new();
        let started_at = std::time::Instant::now();

        let error = run_exchange(ExchangeRequest {
            command: &command,
            request: b"{}",
            timeout: Duration::from_millis(100),
            cancellation: &cancellation,
            stdout_kind: "timeout-descendant-pipe-stdout",
        })
        .unwrap_err();

        assert!(matches!(error, ExchangeError::Timeout { .. }));
        assert!(started_at.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn cancellation_terminates_descendants_that_hold_output_pipes_open() {
        let temp_dir = TestDir::new("cancel-descendant-pipe");
        let script_path = temp_dir.path().join("pack.sh");
        write_script(&script_path, "#!/bin/sh\n(sleep 30) &\nexit 0\n");
        let command = [script_path.to_string_lossy().into_owned()];
        let cancellation = LanguageCancellationToken::new();
        let cancellation_for_thread = cancellation.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            cancellation_for_thread.cancel();
        });
        let started_at = std::time::Instant::now();

        let error = run_exchange(ExchangeRequest {
            command: &command,
            request: b"{}",
            timeout: Duration::from_secs(10),
            cancellation: &cancellation,
            stdout_kind: "cancel-descendant-pipe-stdout",
        })
        .unwrap_err();

        assert!(matches!(error, ExchangeError::Cancelled { .. }));
        assert!(started_at.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn timeout_terminates_descendants_that_hold_stdin_open() {
        let temp_dir = TestDir::new("timeout-descendant-stdin");
        let script_path = temp_dir.path().join("pack.sh");
        write_script(
            &script_path,
            "#!/bin/sh\n(sleep 30 <&0 >/dev/null 2>/dev/null) &\nexit 0\n",
        );
        let command = [script_path.to_string_lossy().into_owned()];
        let request = vec![b'x'; 1024 * 1024];
        let cancellation = LanguageCancellationToken::new();
        let started_at = std::time::Instant::now();

        let error = run_exchange(ExchangeRequest {
            command: &command,
            request: &request,
            timeout: Duration::from_millis(100),
            cancellation: &cancellation,
            stdout_kind: "timeout-descendant-stdin-stdout",
        })
        .unwrap_err();

        assert!(matches!(error, ExchangeError::Timeout { .. }));
        assert!(started_at.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn cancellation_terminates_descendants_that_hold_stdin_open() {
        let temp_dir = TestDir::new("cancel-descendant-stdin");
        let script_path = temp_dir.path().join("pack.sh");
        write_script(
            &script_path,
            "#!/bin/sh\n(sleep 30 <&0 >/dev/null 2>/dev/null) &\nexit 0\n",
        );
        let command = [script_path.to_string_lossy().into_owned()];
        let request = vec![b'x'; 1024 * 1024];
        let cancellation = LanguageCancellationToken::new();
        let cancellation_for_thread = cancellation.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            cancellation_for_thread.cancel();
        });
        let started_at = std::time::Instant::now();

        let error = run_exchange(ExchangeRequest {
            command: &command,
            request: &request,
            timeout: Duration::from_secs(10),
            cancellation: &cancellation,
            stdout_kind: "cancel-descendant-stdin-stdout",
        })
        .unwrap_err();

        assert!(matches!(error, ExchangeError::Cancelled { .. }));
        assert!(started_at.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn preserves_spawn_error_sources() {
        let cancellation = LanguageCancellationToken::new();
        let command = ["/definitely/not/a/wax-language-pack".to_owned()];

        let error = run_exchange(ExchangeRequest {
            command: &command,
            request: b"{}",
            timeout: Duration::from_secs(1),
            cancellation: &cancellation,
            stdout_kind: "test-stdout",
        })
        .unwrap_err();

        assert!(matches!(
            error,
            ExchangeError::Spawn { ref source, .. } if source.kind() == std::io::ErrorKind::NotFound
        ));
    }

    #[test]
    fn preserves_reader_error_sources() {
        let error = ExchangeError::ReadStream {
            stream: "stdout",
            source: std::io::Error::other("read failed"),
        };

        assert_eq!(error.source().unwrap().to_string(), "read failed");
    }

    #[test]
    fn creates_private_spool_files_and_removes_them_on_drop() {
        let temp_dir = TestDir::new("spool-cleanup");
        let script_path = temp_dir.path().join("pack.sh");
        write_script(
            &script_path,
            "#!/bin/sh\nprintf output\nprintf errors 1>&2\n",
        );
        let command = [script_path.to_string_lossy().into_owned()];
        let cancellation = LanguageCancellationToken::new();

        let output = run_exchange(ExchangeRequest {
            command: &command,
            request: b"{}",
            timeout: Duration::from_secs(1),
            cancellation: &cancellation,
            stdout_kind: "test-stdout",
        })
        .unwrap();
        let stdout_path = output.stdout_path().to_owned();
        let stderr_path = output.stderr.path().to_owned();
        let mode = stdout_path.metadata().unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        drop(output);

        assert!(!stdout_path.exists());
        assert!(!stderr_path.exists());
    }

    fn write_script(path: &Path, contents: &str) {
        fs::write(path, contents).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn spool_paths(kind: &str) -> Vec<PathBuf> {
        let prefix = format!("wax-core-subprocess-{kind}-{}-", std::process::id());
        fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix))
            })
            .collect()
    }

    #[expect(
        unsafe_code,
        reason = "the timeout cleanup regression test must probe the child pid with libc::kill(pid, 0) because std has no safe process-existence check by pid"
    )]
    fn assert_process_exited(pid: &str) {
        let pid = pid.parse::<i32>().unwrap();
        for _ in 0..100 {
            // SAFETY: signal zero only probes the recorded child PID; it does not signal or mutate the process.
            if unsafe { libc::kill(pid, 0) } == -1 {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("process {pid} was still running after timeout cleanup");
    }

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "wax-core-subprocess-exchange-{name}-{}",
                std::process::id()
            ));
            if path.exists() {
                fs::remove_dir_all(&path).unwrap();
            }
            fs::create_dir(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
