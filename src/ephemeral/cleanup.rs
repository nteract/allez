//! Anchored, verified environment-prefix removal.
//!
//! There is no RAII cleanup guard and no automatic removal any more — see
//! the GEN-24 spec's "Explicit reap, no automatic reaping" decision. This
//! module now holds exactly one thing: the mechanics of removing one
//! environment's directory tree, anchored to an already-verified root, so
//! neither the failed-creation rollback in `mod.rs` nor the explicit
//! `reap` module has to reimplement that anchoring/verification itself.

use super::{error::EphemeralEnvError, lifecycle::EnvironmentId, paths::VerifiedRoot};

#[cfg(unix)]
use rustix::{
    fd::OwnedFd,
    fs::{AtFlags, Dir, FileType, Mode, OFlags, fstat, openat, statat, unlinkat},
    process::geteuid,
};

/// Removes one environment tree anchored to the verified root's `envs` directory.
pub(crate) fn remove_prefix_dir(
    root: &VerifiedRoot,
    id: EnvironmentId,
) -> Result<(), EphemeralEnvError> {
    remove_prefix_dir_platform(root, id).map_err(|_| EphemeralEnvError::TeardownFailed)
}

#[cfg(unix)]
fn remove_prefix_dir_platform(root: &VerifiedRoot, id: EnvironmentId) -> rustix::io::Result<()> {
    let name = id.to_string();
    let target = openat(
        root.envs_dir(),
        &name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    let metadata = fstat(&target)?;
    if !FileType::from_raw_mode(metadata.st_mode).is_dir() || metadata.st_uid != geteuid().as_raw()
    {
        return Err(rustix::io::Errno::PERM);
    }
    remove_directory_contents(&target)?;
    unlinkat(root.envs_dir(), &name, AtFlags::REMOVEDIR)
}

#[cfg(unix)]
fn remove_directory_contents(directory: &OwnedFd) -> rustix::io::Result<()> {
    let mut entries = Dir::read_from(directory)?;
    for entry in &mut entries {
        let entry = entry?;
        let name = entry.file_name();
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        let metadata = statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)?;
        if FileType::from_raw_mode(metadata.st_mode).is_dir() {
            let child = openat(
                directory,
                name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )?;
            remove_directory_contents(&child)?;
            unlinkat(directory, name, AtFlags::REMOVEDIR)?;
        } else {
            unlinkat(directory, name, AtFlags::empty())?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn remove_prefix_dir_platform(root: &VerifiedRoot, id: EnvironmentId) -> std::io::Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            GetFileInformationByHandle, OPEN_EXISTING,
        },
    };

    let location = root.path().join("envs").join(id.to_string());
    let path = location
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: Category 8 FFI. `path` is a nul-terminated UTF-16 buffer that
    // lives through the call; the returned handle is checked and closed once.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: Category 8 FFI. `information` is a valid writable out-pointer
    // and `handle` was checked above; it remains live until `CloseHandle`.
    let inspected = unsafe { GetFileInformationByHandle(handle, &mut information) } != 0;
    // SAFETY: Category 12 FFI. This branch owns `handle` and closes it exactly once.
    let closed = unsafe { CloseHandle(handle) } != 0;
    if !inspected
        || !closed
        || information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    std::fs::remove_dir_all(location)
}
