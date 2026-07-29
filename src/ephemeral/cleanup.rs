//! Handle-anchored prefix removal and the cleanup ownership guard.

use std::{
    collections::HashSet,
    fmt,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use super::{
    channels::redact_channel_url,
    error::EphemeralEnvError,
    events::{EPHEMERAL_EVENT_SCHEMA_VERSION, EphemeralLifecycleEvent, emit_event},
    lifecycle::EnvironmentId,
    orphan::OwnerLock,
    paths::VerifiedRoot,
};

#[cfg(unix)]
use rustix::{
    fd::OwnedFd,
    fs::{AtFlags, Dir, FileType, Mode, OFlags, fstat, openat, statat, unlinkat},
    process::geteuid,
};

static LIVE_ENVIRONMENTS: OnceLock<Mutex<HashSet<EnvironmentId>>> = OnceLock::new();

fn live_environments() -> &'static Mutex<HashSet<EnvironmentId>> {
    LIVE_ENVIRONMENTS.get_or_init(|| Mutex::new(HashSet::new()))
}

enum CleanupTarget {
    Environment {
        root: Arc<VerifiedRoot>,
        _owner_lock: OwnerLock,
    },
    #[cfg(test)]
    TestOnly,
}

/// Owns an environment's liveness lock and removes its prefix on last drop.
///
/// `std::process::exit()` and `std::process::abort()` bypass `Drop`; a later
/// orphan-reclamation scan is responsible for environments left by those exits.
pub(crate) struct CleanupGuard {
    target: CleanupTarget,
    id: EnvironmentId,
    packages: Vec<String>,
    claimed: AtomicBool,
    removal_outcome: Mutex<Option<Result<(), EphemeralEnvError>>>,
}

impl fmt::Debug for CleanupGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let redacted_packages = self
            .packages
            .iter()
            .map(|package| redact_channel_url(package))
            .collect::<Vec<_>>();
        formatter
            .debug_struct("CleanupGuard")
            .field("id", &self.id)
            .field("packages", &redacted_packages)
            .field("claimed", &self.claimed.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl CleanupGuard {
    /// Creates the guard that retains an environment's owner lock for its lifetime.
    pub(crate) fn new(
        root: Arc<VerifiedRoot>,
        id: EnvironmentId,
        owner_lock: OwnerLock,
        packages: Vec<String>,
    ) -> Self {
        live_environments()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(id);
        Self {
            target: CleanupTarget::Environment {
                root,
                _owner_lock: owner_lock,
            },
            id,
            packages,
            claimed: AtomicBool::new(false),
            removal_outcome: Mutex::new(None),
        }
    }

    /// Claims this environment's single removal attempt.
    pub(crate) fn claim_removal(&self) -> bool {
        !self.claimed.swap(true, Ordering::AcqRel)
    }

    /// Runs an already-claimed removal and emits its teardown event.
    pub(crate) fn remove_claimed(&self, packages: Vec<String>) -> Result<(), EphemeralEnvError> {
        let root = match &self.target {
            CleanupTarget::Environment { root, .. } => root,
            #[cfg(test)]
            CleanupTarget::TestOnly => return Err(EphemeralEnvError::TeardownFailed),
        };
        let started = Instant::now();
        let result = remove_prefix_dir(root, self.id);
        *self
            .removal_outcome
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(result.clone());
        let (outcome, failure_category) = match &result {
            Ok(()) => ("success", None),
            Err(error) => (
                "failure",
                Some(crate::error::CategorizedError::category(error)),
            ),
        };
        emit_event(&EphemeralLifecycleEvent {
            schema_version: EPHEMERAL_EVENT_SCHEMA_VERSION,
            environment_id: self.id,
            operation: "teardown",
            packages,
            duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            outcome,
            failure_category,
        });
        result
    }

    pub(crate) fn removal_outcome(&self) -> Option<Result<(), EphemeralEnvError>> {
        self.removal_outcome
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn test_only() -> Self {
        Self {
            target: CleanupTarget::TestOnly,
            id: EnvironmentId::new(),
            packages: Vec::new(),
            claimed: AtomicBool::new(true),
            removal_outcome: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_only_removable() -> Self {
        Self {
            target: CleanupTarget::TestOnly,
            id: EnvironmentId::new(),
            packages: Vec::new(),
            claimed: AtomicBool::new(false),
            removal_outcome: Mutex::new(None),
        }
    }
}

impl Drop for CleanupGuard {
    /// Runs the removal synchronously on whatever thread drops the last
    /// reference, by design -- unlike every OTHER removal path in this
    /// crate (which run via `mod.rs`'s `run_blocking`/`tokio::task::spawn_blocking`
    /// specifically so they never block a Tokio worker thread), this one
    /// cannot: `Drop` has no `async` equivalent, and the only way to
    /// offload this call onto the blocking-thread pool without changing
    /// its own synchronous, complete-before-return contract would be to
    /// block this same thread waiting for that offloaded work anyway,
    /// which helps nothing. A genuinely non-blocking `Drop` would instead
    /// have to become fire-and-forget, silently breaking the
    /// complete-by-return guarantee `cleanup_tests.rs`'s own
    /// `cleanup_guard_waits_for_the_last_arc_before_removing_the_environment`
    /// test (and this type's broader caller contract) deliberately relies
    /// on -- a larger, accepted trade-off, not an oversight.
    fn drop(&mut self) {
        live_environments()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.id);
        if !self.claim_removal() {
            return;
        }
        let _result = self.remove_claimed(self.packages.clone());
    }
}

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
