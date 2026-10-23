//! Cross-process exclusive advisory locks for cache mutation.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Exclusive advisory lock held for the lifetime of this value.
pub(crate) struct AdvisoryLock {
    file: File,
}

impl AdvisoryLock {
    /// Opens `path` (creating it when absent) and takes an exclusive lock.
    pub(crate) fn acquire(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        lock_exclusive(&file)?;
        Ok(Self { file })
    }
}

impl Drop for AdvisoryLock {
    fn drop(&mut self) {
        let _ = unlock_exclusive(&self.file);
    }
}

#[cfg(unix)]
#[expect(
    unsafe_code,
    reason = "std has no cross-process file lock; flock serializes cache mutation"
)]
fn lock_exclusive(file: &File) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;

    // SAFETY: `file` owns a valid open descriptor for the duration of this call.
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
#[expect(
    unsafe_code,
    reason = "pairs with lock_exclusive; std cannot unlock an advisory flock"
)]
fn unlock_exclusive(file: &File) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;

    // SAFETY: `file` owns a valid open descriptor for the duration of this call.
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "std has no cross-process file lock; LockFileEx serializes cache mutation"
)]
fn lock_exclusive(file: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;

    let mut overlapped = WindowsOverlapped::zeroed();
    // SAFETY: `file` owns a valid handle; `overlapped` is a zeroed stack value
    // whose address remains valid for this synchronous LockFileEx call.
    let locked = unsafe {
        lock_file_ex(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK,
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    };
    if locked == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "pairs with lock_exclusive; std cannot unlock LockFileEx ranges"
)]
fn unlock_exclusive(file: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;

    let mut overlapped = WindowsOverlapped::zeroed();
    // SAFETY: matches the LockFileEx call site; unlocks the same byte range.
    let unlocked =
        unsafe { unlock_file_ex(file.as_raw_handle(), 0, u32::MAX, u32::MAX, &mut overlapped) };
    if unlocked == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x0000_0002;

#[cfg(windows)]
#[repr(C)]
struct WindowsOverlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: *mut core::ffi::c_void,
}

#[cfg(windows)]
impl WindowsOverlapped {
    fn zeroed() -> Self {
        Self {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            event: std::ptr::null_mut(),
        }
    }
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "LockFileEx/UnlockFileEx are not exposed by std; declarations match the Windows ABI"
)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "LockFileEx"]
    fn lock_file_ex(
        file: *mut core::ffi::c_void,
        flags: u32,
        reserved: u32,
        number_of_bytes_to_lock_low: u32,
        number_of_bytes_to_lock_high: u32,
        overlapped: *mut WindowsOverlapped,
    ) -> i32;

    #[link_name = "UnlockFileEx"]
    fn unlock_file_ex(
        file: *mut core::ffi::c_void,
        reserved: u32,
        number_of_bytes_to_unlock_low: u32,
        number_of_bytes_to_unlock_high: u32,
        overlapped: *mut WindowsOverlapped,
    ) -> i32;
}
