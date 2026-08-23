//! Process ownership for a Riffra data root.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

const LOCK_FILE_NAME: &str = ".riffra.lock";

/// An exclusive process lease for one Riffra data root.
///
/// The lock file is intentionally retained after the lease is dropped. The
/// operating system lock, rather than file existence, determines ownership,
/// so a crash cannot leave a stale file that blocks a later host.
#[derive(Debug)]
pub struct DataRootLease {
    file: File,
    path: PathBuf,
}

impl DataRootLease {
    /// Acquires exclusive ownership of `data_root` for the lifetime of the
    /// returned lease.
    ///
    /// # Errors
    /// Returns an explicit in-use error when another process owns the root, or
    /// an I/O error when the lock file cannot be opened or locked.
    pub fn acquire(data_root: &Path) -> io::Result<Self> {
        fs::create_dir_all(data_root)?;
        let path = data_root.join(LOCK_FILE_NAME);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        lock_file(&file).map_err(|error| {
            if error.kind() == io::ErrorKind::WouldBlock {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "The Riffra data root is already in use by another process.",
                )
            } else {
                error
            }
        })?;
        Ok(Self { file, path })
    }

    /// Returns the lock file path used by this lease.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DataRootLease {
    fn drop(&mut self) {
        unlock_file(&self.file);
    }
}

#[cfg(unix)]
fn lock_file(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        Ok(())
    } else {
        let error = io::Error::last_os_error();
        if error
            .raw_os_error()
            .is_some_and(|code| code == libc::EWOULDBLOCK || code == libc::EAGAIN)
        {
            Err(io::Error::from(io::ErrorKind::WouldBlock))
        } else {
            Err(error)
        }
    }
}

#[cfg(unix)]
fn unlock_file(file: &File) {
    use std::os::fd::AsRawFd;
    let _ = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
}

#[cfg(windows)]
fn lock_file(file: &File) -> io::Result<()> {
    use std::mem::zeroed;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;

    let mut overlapped: OVERLAPPED = unsafe { zeroed() };
    let success = unsafe {
        LockFileEx(
            file.as_raw_handle() as _,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &mut overlapped,
        )
    };
    if success == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32) {
            Err(io::Error::from(io::ErrorKind::WouldBlock))
        } else {
            Err(error)
        }
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn unlock_file(file: &File) {
    use std::mem::zeroed;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
    use windows_sys::Win32::System::IO::OVERLAPPED;

    let mut overlapped: OVERLAPPED = unsafe { zeroed() };
    let _ = unsafe { UnlockFileEx(file.as_raw_handle() as _, 0, 1, 0, &mut overlapped) };
}

#[cfg(test)]
mod tests {
    use super::DataRootLease;
    use std::fs;

    #[test]
    fn one_process_cannot_acquire_the_same_root_twice() {
        let root = std::env::temp_dir().join(format!("riffra-lease-{}", std::process::id()));
        let first = DataRootLease::acquire(&root).unwrap();
        let second = DataRootLease::acquire(&root).unwrap_err();

        assert_eq!(second.kind(), std::io::ErrorKind::WouldBlock);
        drop(first);
        DataRootLease::acquire(&root).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stale_lock_file_does_not_block_a_new_lease() {
        let root = std::env::temp_dir().join(format!("riffra-stale-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(".riffra.lock"), b"stale").unwrap();

        DataRootLease::acquire(&root).unwrap();
        let _ = fs::remove_dir_all(root);
    }
}
