use std::io;
use std::process::{Child, Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

const REAP_TIMEOUT: Duration = Duration::from_secs(1);

/// Configures a command so the spawned child leads a dedicated process group.
pub(crate) fn configure_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        command.process_group(0);
    }

    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

/// Stops a child and its process group, then reaps the direct child process.
pub(crate) fn terminate_child_tree(
    child: &mut Child,
    child_id: u32,
    grace: Duration,
) -> io::Result<ExitStatus> {
    #[cfg(unix)]
    {
        terminate_unix(child, child_id, grace)
    }

    #[cfg(not(unix))]
    {
        let _ = child_id;
        terminate_non_unix(child)
    }
}

#[cfg(unix)]
fn terminate_unix(child: &mut Child, child_id: u32, grace: Duration) -> io::Result<ExitStatus> {
    let process_group_id = match i32::try_from(child_id) {
        Ok(process_group_id) if process_group_id > 0 => process_group_id,
        Ok(_) | Err(_) => {
            let error = io::Error::new(
                io::ErrorKind::InvalidInput,
                "child process ID cannot be represented as a process group ID",
            );
            return match reap_child(child, None) {
                Ok(_) => Err(error),
                Err(reap_error) => Err(io::Error::new(
                    error.kind(),
                    format!("{error}; reaping failed: {reap_error}"),
                )),
            };
        }
    };

    let mut cleanup_error = None;
    if let Err(error) = signal_process_group(process_group_id, libc::SIGTERM) {
        cleanup_error = Some(error);
    }

    // Keep the direct child unreaped for the entire grace period. Its zombie
    // process-group leader pins the numeric PGID, so the forced signal cannot
    // be redirected to a reused process group.
    thread::sleep(grace);

    if let Err(error) = signal_process_group(process_group_id, libc::SIGKILL) {
        remember_first_error(&mut cleanup_error, error);
    }

    reap_child(child, cleanup_error)
}

#[cfg(unix)]
#[expect(
    unsafe_code,
    reason = "std cannot signal a Unix process group, so cleanup must call libc::kill with a negative pgid"
)]
fn signal_process_group(process_group_id: i32, signal: i32) -> io::Result<()> {
    // SAFETY: `process_group_id` was checked as a positive `i32` converted from
    // `Child::id()`, so negating it selects that child's process group. Callers pass
    // only SIGTERM or SIGKILL; `kill` receives integer values, no pointers, borrowed
    // data, or ownership transfers, and its errno result is read immediately.
    let result = unsafe { libc::kill(-process_group_id, signal) };
    if result == 0 {
        return Ok(());
    }

    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(not(unix))]
fn terminate_non_unix(child: &mut Child) -> io::Result<ExitStatus> {
    reap_child(child, None)
}

fn reap_child(child: &mut Child, cleanup_error: Option<io::Error>) -> io::Result<ExitStatus> {
    let mut cleanup_error = cleanup_error;
    if let Err(error) = child.kill() {
        record_cleanup_error(&mut cleanup_error, error);
    }

    let deadline = Instant::now() + REAP_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if let Some(error) = cleanup_error {
                    eprintln!("child process cleanup reported an error: {error}");
                }
                return Ok(status);
            }
            Ok(None) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                remember_first_error(&mut cleanup_error, error);
                break;
            }
        }

        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    Err(cleanup_error.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::TimedOut, "timed out reaping child process")
    }))
}

fn remember_first_error(slot: &mut Option<io::Error>, error: io::Error) {
    if slot.is_none() {
        *slot = Some(error);
    }
}

fn record_cleanup_error(slot: &mut Option<io::Error>, error: io::Error) {
    if let Some(previous) = slot.take() {
        *slot = Some(io::Error::new(
            previous.kind(),
            format!("{previous}; additional cleanup error: {error}"),
        ));
    } else {
        *slot = Some(error);
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::fs;
    use std::io;
    use std::os::unix::process::ExitStatusExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::thread;
    use std::time::Duration;

    use super::{configure_process_group, signal_process_group, terminate_child_tree};

    #[test]
    fn treats_a_missing_process_group_as_successfully_signaled() {
        let result: io::Result<()> = signal_process_group(i32::MAX, libc::SIGTERM);

        result.unwrap();
    }

    #[test]
    fn terminates_a_process_group_gracefully_after_sigterm() {
        let temp_dir = TestDir::new("graceful-term");
        let ready_path = temp_dir.path().join("ready");
        let terminated_path = temp_dir.path().join("terminated");
        let mut command = shell_command(
            r#"
trap 'touch "$2"; exit 0' TERM
touch "$1"
while :; do :; done
"#,
        );
        command.arg(&ready_path).arg(&terminated_path);
        configure_process_group(&mut command);

        let mut child = command.spawn().unwrap();
        let child_id = child.id();
        wait_for_file(&ready_path);

        let status = terminate_child_tree(&mut child, child_id, Duration::from_secs(1)).unwrap();

        assert!(status.success());
        wait_for_file(&terminated_path);
    }

    #[test]
    fn force_kills_a_process_group_that_ignores_sigterm() {
        let temp_dir = TestDir::new("forced-kill");
        let ready_path = temp_dir.path().join("ready");
        let mut command = shell_command(
            r#"
trap '' TERM
touch "$1"
while :; do sleep 1; done
"#,
        );
        command.arg(&ready_path);
        configure_process_group(&mut command);

        let mut child = command.spawn().unwrap();
        let child_id = child.id();
        wait_for_file(&ready_path);

        let status = terminate_child_tree(&mut child, child_id, Duration::from_millis(50)).unwrap();

        assert_eq!(status.signal(), Some(libc::SIGKILL));
    }

    #[test]
    fn treats_an_already_exited_process_group_as_successful_cleanup() {
        let mut command = shell_command("exit 0");
        configure_process_group(&mut command);
        let mut child = command.spawn().unwrap();
        let child_id = child.id();
        assert!(child.wait().unwrap().success());

        let status = terminate_child_tree(&mut child, child_id, Duration::ZERO).unwrap();

        assert!(status.success());
    }

    #[test]
    fn rejects_an_unrepresentable_process_group_id_after_reaping_the_child() {
        let temp_dir = TestDir::new("overflow");
        let ready_path = temp_dir.path().join("ready");
        let mut command = shell_command(
            r#"
trap '' TERM
touch "$1"
while :; do sleep 1; done
"#,
        );
        command.arg(&ready_path);
        configure_process_group(&mut command);
        let mut child = command.spawn().unwrap();
        wait_for_file(&ready_path);

        let error =
            terminate_child_tree(&mut child, u32::MAX, Duration::from_millis(50)).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn force_kill_reaches_descendants_in_the_process_group() {
        let temp_dir = TestDir::new("descendant-cleanup");
        let ready_path = temp_dir.path().join("ready");
        let descendant_pid_path = temp_dir.path().join("descendant-pid");
        let mut command = shell_command(
            r#"
trap '' TERM
sh -c 'trap "" TERM; while :; do sleep 1; done' sh &
echo "$!" > "$2"
touch "$1"
while :; do sleep 1; done
"#,
        );
        command.arg(&ready_path).arg(&descendant_pid_path);
        configure_process_group(&mut command);
        let mut child = command.spawn().unwrap();
        let child_id = child.id();
        wait_for_file(&ready_path);
        wait_for_file(&descendant_pid_path);
        let descendant_pid = fs::read_to_string(&descendant_pid_path)
            .unwrap()
            .trim()
            .parse::<i32>()
            .unwrap();

        terminate_child_tree(&mut child, child_id, Duration::from_millis(50)).unwrap();

        wait_for_process_exit(descendant_pid);
    }

    fn shell_command(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.arg("-c").arg(script).arg("sh");
        command
    }

    fn wait_for_file(path: &Path) {
        for _ in 0..100 {
            if path.exists() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for {}", path.display());
    }

    #[expect(
        unsafe_code,
        reason = "the process-control test probes descendant liveness with libc::kill signal zero"
    )]
    fn wait_for_process_exit(pid: i32) {
        for _ in 0..100 {
            // SAFETY: `kill` with signal zero only probes the recorded child PID;
            // it does not deliver a signal or mutate process state.
            if unsafe { libc::kill(pid, 0) } == -1 {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("process {pid} was still running after group cleanup");
    }

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "wax-core-process-control-{name}-{}",
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
