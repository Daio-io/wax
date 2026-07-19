use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

const MAX_TEMP_ATTEMPTS: u32 = 32;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::ptr;

#[cfg(any(windows, test))]
const ERROR_UNABLE_TO_REMOVE_REPLACED: i32 = 1175;
#[cfg(any(windows, test))]
const ERROR_UNABLE_TO_MOVE_REPLACEMENT: i32 = 1176;
#[cfg(any(windows, test))]
const ERROR_UNABLE_TO_MOVE_REPLACEMENT_2: i32 = 1177;

/// Options that control a newly created destination file.
#[derive(Debug, Clone, Copy, Default)]
pub struct AtomicWriteOptions {
    /// Unix permission bits applied when the destination does not yet exist.
    pub new_file_mode: Option<u32>,
}

/// A failure while durably replacing a file with complete byte contents.
#[derive(Debug, Error)]
pub enum AtomicWriteError {
    /// The destination directory could not be created.
    #[error("failed to create parent directory for {path}: {source}")]
    CreateParent {
        /// Destination path.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// A unique temporary file could not be created.
    #[error("failed to create temporary file {temp_path} for {path}: {source}")]
    CreateTemp {
        /// Destination path.
        path: PathBuf,
        /// Temporary path attempted.
        temp_path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// The destination appeared while publishing a no-clobber write.
    #[error("destination already exists at {path}")]
    DestinationExists {
        /// Existing destination path.
        path: PathBuf,
    },
    /// The temporary file could not be linked into place without overwriting.
    #[error("failed to publish {temp_path} without overwriting {path}: {source}")]
    PublishNoClobber {
        /// Destination path.
        path: PathBuf,
        /// Temporary path.
        temp_path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// Existing destination permissions could not be copied to the temporary file.
    #[error("failed to apply permissions for {path} to temporary file {temp_path}: {source}")]
    SetPermissions {
        /// Destination path.
        path: PathBuf,
        /// Temporary path.
        temp_path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// The complete contents could not be written to the temporary file.
    #[error("failed to write temporary file {temp_path} for {path}: {source}")]
    WriteTemp {
        /// Destination path.
        path: PathBuf,
        /// Temporary path.
        temp_path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// The temporary file could not be synced before replacement.
    #[error("failed to sync temporary file {temp_path} for {path}: {source}")]
    SyncTemp {
        /// Destination path.
        path: PathBuf,
        /// Temporary path.
        temp_path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// The temporary file could not replace the destination.
    #[error("failed to replace {path} with {temp_path}: {source}")]
    Replace {
        /// Destination path.
        path: PathBuf,
        /// Temporary path.
        temp_path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// The destination directory could not be synced after replacement.
    #[error("failed to sync parent directory for {path}: {source}")]
    SyncParent {
        /// Destination path.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
}

/// Writes bytes through a local temporary file and atomically replaces `path`.
///
/// The parent directory is created when needed. Existing destination
/// permissions are preserved; `new_file_mode` applies only to newly created
/// files on Unix.
///
/// # Errors
///
/// Returns [`AtomicWriteError`] when temporary-file creation, writing,
/// synchronization, replacement, or parent-directory syncing fails.
pub fn write_atomically(
    path: impl AsRef<Path>,
    contents: &[u8],
    options: AtomicWriteOptions,
) -> Result<(), AtomicWriteError> {
    let path = path.as_ref();
    let temp_path = write_temp_file(path, options, |file| file.write_all(contents))?;

    replace_file(&temp_path, path)?;
    sync_parent_directory(path)
}

/// Writes bytes atomically, failing if the destination already exists.
///
/// The final publish uses a hard link so an existing destination is never
/// overwritten, including when it is created after a caller's preflight check.
///
/// # Errors
///
/// Returns [`AtomicWriteError`] when temporary-file creation, writing,
/// synchronization, publication, or parent-directory syncing fails.
pub(crate) fn write_atomically_no_clobber(
    path: impl AsRef<Path>,
    contents: &[u8],
    options: AtomicWriteOptions,
) -> Result<(), AtomicWriteError> {
    let path = path.as_ref();
    let temp_path = write_temp_file(path, options, |file| file.write_all(contents))?;

    publish_file_no_clobber(&temp_path, path)?;
    sync_parent_directory(path)
}

fn write_temp_file<F>(
    path: &Path,
    options: AtomicWriteOptions,
    write_contents: F,
) -> Result<PathBuf, AtomicWriteError>
where
    F: FnOnce(&mut File) -> io::Result<()>,
{
    fs::create_dir_all(parent_directory(path)).map_err(|source| {
        AtomicWriteError::CreateParent {
            path: path.to_owned(),
            source,
        }
    })?;

    let (temp_path, mut temp_file) = create_temp_file(path)?;
    if let Err(error) = apply_permissions(path, &temp_path, &temp_file, options) {
        drop(temp_file);
        remove_temp_file(&temp_path);
        return Err(error);
    }
    if let Err(source) = write_contents(&mut temp_file) {
        drop(temp_file);
        remove_temp_file(&temp_path);
        return Err(AtomicWriteError::WriteTemp {
            path: path.to_owned(),
            temp_path,
            source,
        });
    }
    if let Err(source) = temp_file.sync_all() {
        drop(temp_file);
        remove_temp_file(&temp_path);
        return Err(AtomicWriteError::SyncTemp {
            path: path.to_owned(),
            temp_path,
            source,
        });
    }
    drop(temp_file);

    Ok(temp_path)
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn create_temp_file(path: &Path) -> Result<(PathBuf, File), AtomicWriteError> {
    let mut last_path = temp_path(path, 0);
    for attempt in 0..MAX_TEMP_ATTEMPTS {
        let candidate = temp_path(path, attempt);
        last_path = candidate.clone();
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(AtomicWriteError::CreateTemp {
                    path: path.to_owned(),
                    temp_path: candidate,
                    source,
                });
            }
        }
    }

    Err(AtomicWriteError::CreateTemp {
        path: path.to_owned(),
        temp_path: last_path,
        source: io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique temporary file",
        ),
    })
}

fn temp_path(path: &Path, attempt: u32) -> PathBuf {
    let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file = path.file_name().and_then(OsStr::to_str).unwrap_or("wax");
    parent_directory(path).join(format!(
        ".{file}.{}.{}.{attempt}.tmp",
        process::id(),
        sequence
    ))
}

fn apply_permissions(
    path: &Path,
    temp_path: &Path,
    file: &File,
    options: AtomicWriteOptions,
) -> Result<(), AtomicWriteError> {
    let permissions = match fs::metadata(path) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(AtomicWriteError::SetPermissions {
                path: path.to_owned(),
                temp_path: temp_path.to_owned(),
                source,
            });
        }
    };

    if let Some(permissions) = permissions {
        file.set_permissions(permissions)
            .map_err(|source| AtomicWriteError::SetPermissions {
                path: path.to_owned(),
                temp_path: temp_path.to_owned(),
                source,
            })?;
    } else {
        set_new_file_mode(path, temp_path, file, options)?;
    }

    Ok(())
}

#[cfg(unix)]
fn set_new_file_mode(
    path: &Path,
    temp_path: &Path,
    file: &File,
    options: AtomicWriteOptions,
) -> Result<(), AtomicWriteError> {
    use std::os::unix::fs::PermissionsExt;

    if let Some(mode) = options.new_file_mode {
        file.set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|source| AtomicWriteError::SetPermissions {
                path: path.to_owned(),
                temp_path: temp_path.to_owned(),
                source,
            })?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_new_file_mode(
    _path: &Path,
    _temp_path: &Path,
    _file: &File,
    _options: AtomicWriteOptions,
) -> Result<(), AtomicWriteError> {
    Ok(())
}

fn remove_temp_file(temp_path: &Path) {
    let _ = fs::remove_file(temp_path);
}

fn publish_file_no_clobber(temp_path: &Path, path: &Path) -> Result<(), AtomicWriteError> {
    match fs::hard_link(temp_path, path) {
        Ok(()) => {
            remove_temp_file(temp_path);
            Ok(())
        }
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            remove_temp_file(temp_path);
            Err(AtomicWriteError::DestinationExists {
                path: path.to_owned(),
            })
        }
        Err(source) => {
            remove_temp_file(temp_path);
            Err(AtomicWriteError::PublishNoClobber {
                path: path.to_owned(),
                temp_path: temp_path.to_owned(),
                source,
            })
        }
    }
}

#[cfg(not(windows))]
fn replace_file(temp_path: &Path, path: &Path) -> Result<(), AtomicWriteError> {
    fs::rename(temp_path, path).map_err(|source| {
        remove_temp_file(temp_path);
        AtomicWriteError::Replace {
            path: path.to_owned(),
            temp_path: temp_path.to_owned(),
            source,
        }
    })
}

#[cfg(windows)]
fn replace_file(temp_path: &Path, path: &Path) -> Result<(), AtomicWriteError> {
    if !path.exists() {
        return rename_temp_file(temp_path, path);
    }

    replace_existing_file(temp_path, path)
}

#[cfg(windows)]
fn rename_temp_file(temp_path: &Path, path: &Path) -> Result<(), AtomicWriteError> {
    fs::rename(temp_path, path).map_err(|source| {
        remove_temp_file(temp_path);
        AtomicWriteError::Replace {
            path: path.to_owned(),
            temp_path: temp_path.to_owned(),
            source,
        }
    })
}

#[cfg(windows)]
fn replace_existing_file(temp_path: &Path, path: &Path) -> Result<(), AtomicWriteError> {
    let replaced = wide_null(path.as_os_str());
    let replacement = wide_null(temp_path.as_os_str());

    // SAFETY: `wide_null` creates owned, null-terminated UTF-16 buffers that stay
    // live for this synchronous call. `0` is the documented no-options value; the
    // three null pointers request no backup or optional structures. ReplaceFileW
    // neither retains these pointers nor transfers ownership of their buffers.
    let replaced = unsafe {
        replace_file_w(
            replaced.as_ptr(),
            replacement.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };

    if replaced == 0 {
        let source = io::Error::last_os_error();
        return handle_windows_replace_failure(source, temp_path, path);
    }

    Ok(())
}

#[cfg(any(windows, test))]
fn handle_windows_replace_failure(
    source: io::Error,
    temp_path: &Path,
    path: &Path,
) -> Result<(), AtomicWriteError> {
    match recover_windows_partial_replace_failure(&source, temp_path, path) {
        Ok(true) => Ok(()),
        Ok(false) => {
            remove_temp_file(temp_path);
            Err(AtomicWriteError::Replace {
                path: path.to_owned(),
                temp_path: temp_path.to_owned(),
                source,
            })
        }
        Err(source) => Err(AtomicWriteError::Replace {
            path: path.to_owned(),
            temp_path: temp_path.to_owned(),
            source,
        }),
    }
}

#[cfg(any(windows, test))]
fn recover_windows_partial_replace_failure(
    source: &io::Error,
    temp_path: &Path,
    path: &Path,
) -> Result<bool, io::Error> {
    if is_recoverable_windows_partial_replace_failure(source)
        && !path.exists()
        && temp_path.exists()
    {
        fs::rename(temp_path, path)?;
        return Ok(true);
    }

    Ok(false)
}

#[cfg(any(windows, test))]
fn is_recoverable_windows_partial_replace_failure(source: &io::Error) -> bool {
    matches!(
        source.raw_os_error(),
        Some(ERROR_UNABLE_TO_MOVE_REPLACEMENT | ERROR_UNABLE_TO_MOVE_REPLACEMENT_2)
    )
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), AtomicWriteError> {
    File::open(parent_directory(path))
        .and_then(|directory| directory.sync_all())
        .map_err(|source| AtomicWriteError::SyncParent {
            path: path.to_owned(),
            source,
        })
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<(), AtomicWriteError> {
    Ok(())
}

#[cfg(windows)]
fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
#[link(name = "kernel32")]
// SAFETY: the declaration matches the Windows ReplaceFileW ABI and parameter
// types. Call sites provide valid UTF-16 pointers and retain all buffer ownership.
unsafe extern "system" {
    #[link_name = "ReplaceFileW"]
    fn replace_file_w(
        replaced_file_name: *const u16,
        replacement_file_name: *const u16,
        backup_file_name: *const u16,
        replace_flags: u32,
        exclude: *mut core::ffi::c_void,
        reserved: *mut core::ffi::c_void,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::{
        AtomicWriteOptions, write_atomically, write_atomically_no_clobber, write_temp_file,
    };
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::{Duration, Instant};

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let sequence = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "wax-atomic-file-{name}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create test directory");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn wait_for_read_count(read_count: &AtomicU64, minimum: u64) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while read_count.load(Ordering::Acquire) < minimum {
            assert!(
                Instant::now() < deadline,
                "reader did not observe the destination during replacement"
            );
            thread::yield_now();
        }
    }

    #[test]
    fn write_atomically_never_exposes_a_torn_replacement_to_readers() {
        let directory = TestDir::new("torn-read");
        let path = directory.path.join("state.json");
        let old_contents = vec![b'o'; 1024 * 1024];
        let new_contents = vec![b'n'; 1024 * 1024];
        fs::write(&path, &old_contents).expect("seed destination");

        let reading = Arc::new(AtomicBool::new(true));
        let replacing = Arc::new(AtomicBool::new(false));
        let successful_reads = Arc::new(AtomicU64::new(0));
        let start = Arc::new(Barrier::new(2));
        let reader_path = path.clone();
        let reader_old = old_contents.clone();
        let reader_new = new_contents.clone();
        let reader_reading = Arc::clone(&reading);
        let reader_replacing = Arc::clone(&replacing);
        let reader_successful_reads = Arc::clone(&successful_reads);
        let reader_start = Arc::clone(&start);
        let reader = thread::spawn(move || {
            reader_start.wait();
            while reader_reading.load(Ordering::Acquire) {
                if let Ok(contents) = fs::read(&reader_path) {
                    assert!(
                        contents == reader_old || contents == reader_new,
                        "reader observed a partial replacement"
                    );
                    if reader_replacing.load(Ordering::Acquire) {
                        reader_successful_reads.fetch_add(1, Ordering::Release);
                    }
                }
            }
        });

        start.wait();
        replacing.store(true, Ordering::Release);
        for (index, contents) in [&new_contents, &old_contents]
            .into_iter()
            .cycle()
            .take(20)
            .enumerate()
        {
            write_atomically(&path, contents, AtomicWriteOptions::default())
                .expect("replace destination atomically");
            if index == 0 {
                wait_for_read_count(&successful_reads, 1);
            }
        }

        replacing.store(false, Ordering::Release);
        reading.store(false, Ordering::Release);
        reader.join().expect("reader should not observe torn data");
        assert!(successful_reads.load(Ordering::Acquire) > 0);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomically_preserves_existing_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TestDir::new("permissions");
        let path = directory.path.join("state.json");
        fs::write(&path, b"old").expect("seed destination");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640))
            .expect("set destination permissions");

        write_atomically(&path, b"new", AtomicWriteOptions::default())
            .expect("replace destination");

        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_atomically_applies_requested_mode_to_new_file() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TestDir::new("new-file-mode");
        let path = directory.path.join("state.json");

        write_atomically(
            &path,
            b"new",
            AtomicWriteOptions {
                new_file_mode: Some(0o600),
            },
        )
        .expect("create destination");

        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn write_atomically_no_clobber_keeps_existing_destination() {
        let directory = TestDir::new("no-clobber");
        let path = directory.path.join("state.json");
        fs::write(&path, b"old").expect("seed destination");

        let error = write_atomically_no_clobber(&path, b"new", AtomicWriteOptions::default())
            .expect_err("existing destination should not be replaced");

        assert!(matches!(
            error,
            super::AtomicWriteError::DestinationExists { .. }
        ));
        assert_eq!(fs::read(path).expect("read destination"), b"old");
    }

    #[test]
    fn write_temp_file_removes_temp_file_after_pre_replacement_write_failure() {
        let directory = TestDir::new("pre-replacement-cleanup");
        let destination = directory.path.join("destination");

        let error = write_temp_file(&destination, AtomicWriteOptions::default(), |_| {
            Err(io::Error::other("injected write failure"))
        })
        .expect_err("injected write failure");

        assert!(matches!(error, super::AtomicWriteError::WriteTemp { .. }));
        assert!(
            fs::read_dir(&directory.path)
                .expect("read test directory")
                .all(|entry| !entry
                    .expect("read directory entry")
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp"))
        );
    }

    #[test]
    fn write_atomically_removes_temp_file_after_replacement_failure() {
        let directory = TestDir::new("cleanup");
        let destination = directory.path.join("destination");
        fs::create_dir(&destination).expect("create conflicting destination directory");

        let error = write_atomically(&destination, b"contents", AtomicWriteOptions::default())
            .expect_err("replacing a directory should fail");

        assert!(error.to_string().contains("destination"));
        assert!(
            fs::read_dir(&directory.path)
                .expect("read test directory")
                .all(|entry| !entry
                    .expect("read directory entry")
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp"))
        );
    }

    #[cfg(test)]
    mod windows_recovery_tests {
        use super::super::{
            AtomicWriteError, ERROR_UNABLE_TO_MOVE_REPLACEMENT, ERROR_UNABLE_TO_MOVE_REPLACEMENT_2,
            ERROR_UNABLE_TO_REMOVE_REPLACED, handle_windows_replace_failure,
            is_recoverable_windows_partial_replace_failure,
        };
        use super::TestDir;
        use std::fs;
        use std::io;

        #[cfg(windows)]
        #[test]
        fn write_atomically_overwrites_an_existing_file() {
            let directory = TestDir::new("windows-overwrite");
            let path = directory.path.join("state.json");
            fs::write(&path, b"old").expect("seed destination");

            super::write_atomically(&path, b"new", super::AtomicWriteOptions::default())
                .expect("overwrite destination");

            assert_eq!(fs::read(path).expect("read destination"), b"new");
        }

        #[test]
        fn partial_move_failures_recover_by_renaming_the_temp_file() {
            let directory = TestDir::new("windows-partial-recovery");
            for code in [
                ERROR_UNABLE_TO_MOVE_REPLACEMENT,
                ERROR_UNABLE_TO_MOVE_REPLACEMENT_2,
            ] {
                let path = directory.path.join(format!("state-{code}.json"));
                let temp_path = directory.path.join(format!("replacement-{code}.tmp"));
                fs::write(&temp_path, b"new").expect("seed replacement");

                handle_windows_replace_failure(
                    io::Error::from_raw_os_error(code),
                    &temp_path,
                    &path,
                )
                .expect("recover partial move");

                assert_eq!(fs::read(path).expect("read recovered destination"), b"new");
                assert!(!temp_path.exists(), "recovery should consume the temp file");
            }
        }

        #[test]
        fn failed_partial_move_recovery_preserves_the_temp_file() {
            let directory = TestDir::new("windows-recovery-failure");
            for code in [
                ERROR_UNABLE_TO_MOVE_REPLACEMENT,
                ERROR_UNABLE_TO_MOVE_REPLACEMENT_2,
            ] {
                let temp_path = directory.path.join(format!("replacement-{code}.tmp"));
                let path = directory
                    .path
                    .join(format!("missing-parent-{code}"))
                    .join("state.json");
                fs::write(&temp_path, b"new").expect("seed replacement");

                let error = handle_windows_replace_failure(
                    io::Error::from_raw_os_error(code),
                    &temp_path,
                    &path,
                )
                .expect_err("recovery rename should fail");

                assert!(
                    matches!(error, AtomicWriteError::Replace { source, .. } if source.kind() == io::ErrorKind::NotFound)
                );
                assert!(
                    temp_path.exists(),
                    "failed recovery should preserve the temp file"
                );
            }
        }

        #[test]
        fn non_recoverable_replace_failures_remove_the_temp_file() {
            let directory = TestDir::new("windows-non-recoverable-cleanup");
            let path = directory.path.join("state.json");
            fs::write(&path, b"old").expect("seed destination");

            for code in [ERROR_UNABLE_TO_REMOVE_REPLACED, 87] {
                let temp_path = directory.path.join(format!("replacement-{code}.tmp"));
                fs::write(&temp_path, b"new").expect("seed replacement");

                let error = handle_windows_replace_failure(
                    io::Error::from_raw_os_error(code),
                    &temp_path,
                    &path,
                )
                .expect_err("non-recoverable failure should be returned");

                assert!(
                    matches!(error, AtomicWriteError::Replace { source, .. } if source.raw_os_error() == Some(code))
                );
                assert!(!temp_path.exists(), "failure should remove the temp file");
            }

            assert_eq!(fs::read(path).expect("read original destination"), b"old");
        }

        #[test]
        fn only_partial_move_errors_are_recoverable() {
            for code in [
                ERROR_UNABLE_TO_MOVE_REPLACEMENT,
                ERROR_UNABLE_TO_MOVE_REPLACEMENT_2,
            ] {
                assert!(is_recoverable_windows_partial_replace_failure(
                    &io::Error::from_raw_os_error(code)
                ));
            }
            for code in [ERROR_UNABLE_TO_REMOVE_REPLACED, 87] {
                assert!(!is_recoverable_windows_partial_replace_failure(
                    &io::Error::from_raw_os_error(code)
                ));
            }
        }
    }
}
