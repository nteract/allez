//! Publication and orphan reclamation using advisory owner locks.

use std::{
    fs::File,
    io::Write,
    sync::OnceLock,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use fs4::{FileExt, TryLockError};
use serde::{Deserialize, Serialize};

use super::{
    cleanup::remove_prefix_dir,
    defaults::PackageSpec,
    error::EphemeralEnvError,
    events::{EPHEMERAL_EVENT_SCHEMA_VERSION, EphemeralLifecycleEvent, emit_event},
    lifecycle::EnvironmentId,
    orphan_files::{
        EnvironmentCandidate, METADATA_FILE, environment_candidates, metadata_file,
        owner_lock_file, root_lock_file,
    },
    paths::VerifiedRoot,
    permissions::create_environment_directory,
};

const ROOT_LOCK_TIMEOUT_DEFAULT: Duration = Duration::from_millis(2_000);
const ROOT_LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(10);

static ROOT_LOCK_TIMEOUT: OnceLock<Duration> = OnceLock::new();

/// Holds an environment's exclusive advisory owner lock for its full lifetime.
pub(crate) struct OwnerLock {
    _file: File,
}

impl std::fmt::Debug for OwnerLock {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("OwnerLock").finish_non_exhaustive()
    }
}

/// A fully published environment whose owner lock must be retained by its caller.
#[derive(Debug)]
pub(crate) struct PublishedEnvironment {
    /// The exclusive liveness lock retained for the environment lifetime.
    pub(crate) owner_lock: OwnerLock,
}

/// A failure to publish a newly-created environment: root-lock acquisition,
/// directory creation, owner-lock acquisition, or metadata write.
#[derive(Debug)]
pub(crate) struct PublicationFailure {
    /// Why publication failed.
    pub(crate) error: EphemeralEnvError,
    /// This function's own best-effort removal of the directory it created,
    /// and how long that removal took -- present only when a directory was
    /// actually created before something afterward failed. `None` means no
    /// directory was ever created (the root lock or the directory creation
    /// itself is what failed), so there is nothing for a caller to report a
    /// teardown outcome for; reporting one anyway would be a fabricated
    /// event for a removal that never ran.
    pub(crate) cleanup: Option<PublicationCleanup>,
}

/// The outcome and duration of [`publish_environment`]'s own removal
/// attempt after a publication failure with a directory already created.
#[derive(Debug)]
pub(crate) struct PublicationCleanup {
    pub(crate) outcome: Result<(), EphemeralEnvError>,
    pub(crate) duration_ms: u64,
}

/// The result of classifying and reclaiming one previously-published environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrphanReclamationOutcome {
    /// The environment was unlocked and its directory was removed.
    Removed {
        /// The removed environment.
        id: EnvironmentId,
    },
    /// The environment was unlocked but removal failed.
    RemovalFailed {
        /// The environment that could not be removed.
        id: EnvironmentId,
        /// The removal failure category.
        error: EphemeralEnvError,
    },
    /// A currently-live process holds the owner lock.
    StillActive {
        /// The environment whose owner lock is still held.
        id: EnvironmentId,
    },
    /// The owner lock could not be opened or inspected safely.
    Unknown {
        /// The environment whose owner lock could not be inspected.
        id: EnvironmentId,
    },
}

struct RootLock {
    _file: File,
}

#[derive(Serialize)]
struct EnvironmentMetadata<'a> {
    pid: u32,
    created_at: u64,
    environment_id: String,
    packages: Vec<&'a str>,
}

#[derive(Deserialize)]
struct ReclaimedMetadata {
    packages: Vec<String>,
}

enum OwnerLockInspection {
    Acquired(OwnerLock),
    StillActive,
    Unknown,
}

/// Publishes a newly-created environment and returns its retained owner lock.
pub(crate) fn publish_environment(
    root: &VerifiedRoot,
    id: EnvironmentId,
    packages: &[PackageSpec],
) -> Result<PublishedEnvironment, PublicationFailure> {
    let _root_lock = acquire_root_lock(root).map_err(|error| PublicationFailure {
        error,
        cleanup: None,
    })?;
    create_environment_directory(root, id).map_err(|error| PublicationFailure {
        error,
        cleanup: None,
    })?;
    let published = (|| {
        let owner_lock =
            acquire_owner_lock(root, id).map_err(|_| EphemeralEnvError::UnwritableLocation)?;
        write_metadata(root, id, packages)?;
        Ok(PublishedEnvironment { owner_lock })
    })();
    published.map_err(|error| {
        let cleanup_started = Instant::now();
        PublicationFailure {
            error,
            cleanup: Some(PublicationCleanup {
                outcome: remove_prefix_dir(root, id),
                duration_ms: cleanup_started
                    .elapsed()
                    .as_millis()
                    .try_into()
                    .unwrap_or(u64::MAX),
            }),
        }
    })
}

/// Reclaims environment directories whose owner locks are no longer held.
pub(crate) fn reclaim_orphaned_environments(
    root: &VerifiedRoot,
) -> Result<Vec<OrphanReclamationOutcome>, EphemeralEnvError> {
    let _root_lock = acquire_root_lock(root)?;
    let mut outcomes = Vec::new();
    for candidate in environment_candidates(root)? {
        let id = match candidate {
            EnvironmentCandidate::Unknown(id) => {
                outcomes.push(OrphanReclamationOutcome::Unknown { id });
                continue;
            }
            EnvironmentCandidate::Directory(id) => id,
        };
        match inspect_owner_lock(root, id) {
            OwnerLockInspection::StillActive => {
                outcomes.push(OrphanReclamationOutcome::StillActive { id })
            }
            OwnerLockInspection::Unknown => outcomes.push(OrphanReclamationOutcome::Unknown { id }),
            OwnerLockInspection::Acquired(owner_lock) => {
                drop(owner_lock);
                let packages = read_metadata_packages(root, id);
                let started = Instant::now();
                match remove_prefix_dir(root, id) {
                    Ok(()) => {
                        emit_reclamation_event(id, packages, started, None);
                        outcomes.push(OrphanReclamationOutcome::Removed { id });
                    }
                    Err(error) => {
                        emit_reclamation_event(id, packages, started, Some(&error));
                        outcomes.push(OrphanReclamationOutcome::RemovalFailed { id, error });
                    }
                }
            }
        }
    }
    Ok(outcomes)
}

fn acquire_root_lock(root: &VerifiedRoot) -> Result<RootLock, EphemeralEnvError> {
    let file = root_lock_file(root).map_err(|_| EphemeralEnvError::UnwritableLocation)?;
    let deadline = Instant::now() + root_lock_timeout();
    loop {
        match FileExt::try_lock(&file) {
            Ok(()) => return Ok(RootLock { _file: file }),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                thread::sleep(ROOT_LOCK_RETRY_INTERVAL);
            }
            Err(TryLockError::WouldBlock | TryLockError::Error(_)) => {
                return Err(EphemeralEnvError::UnwritableLocation);
            }
        }
    }
}

fn root_lock_timeout() -> Duration {
    *ROOT_LOCK_TIMEOUT.get_or_init(|| {
        std::env::var("ALLEZ_ROOT_LOCK_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_millis)
            .unwrap_or(ROOT_LOCK_TIMEOUT_DEFAULT)
    })
}

fn acquire_owner_lock(root: &VerifiedRoot, id: EnvironmentId) -> Result<OwnerLock, TryLockError> {
    let file = owner_lock_file(root, id, true).map_err(TryLockError::Error)?;
    FileExt::try_lock(&file)?;
    Ok(OwnerLock { _file: file })
}

fn inspect_owner_lock(root: &VerifiedRoot, id: EnvironmentId) -> OwnerLockInspection {
    match owner_lock_file(root, id, false) {
        Ok(file) => match FileExt::try_lock(&file) {
            Ok(()) => OwnerLockInspection::Acquired(OwnerLock { _file: file }),
            Err(TryLockError::WouldBlock) => OwnerLockInspection::StillActive,
            Err(TryLockError::Error(_)) => OwnerLockInspection::Unknown,
        },
        Err(_) => OwnerLockInspection::Unknown,
    }
}

fn write_metadata(
    root: &VerifiedRoot,
    id: EnvironmentId,
    packages: &[PackageSpec],
) -> Result<(), EphemeralEnvError> {
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    let metadata = EnvironmentMetadata {
        pid: std::process::id(),
        created_at,
        environment_id: id.to_string(),
        packages: packages.iter().map(PackageSpec::as_str).collect(),
    };
    let serialized =
        serde_json::to_vec(&metadata).map_err(|_| EphemeralEnvError::UnwritableLocation)?;
    let mut file = metadata_file(root, id).map_err(|_| EphemeralEnvError::UnwritableLocation)?;
    file.write_all(&serialized)
        .map_err(|_| EphemeralEnvError::UnwritableLocation)
}

fn read_metadata_packages(root: &VerifiedRoot, id: EnvironmentId) -> Vec<String> {
    let location = root
        .path()
        .join("envs")
        .join(id.to_string())
        .join(METADATA_FILE);
    std::fs::read(location)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ReclaimedMetadata>(&bytes).ok())
        .map_or_else(Vec::new, |metadata| metadata.packages)
}

fn emit_reclamation_event(
    id: EnvironmentId,
    packages: Vec<String>,
    started: Instant,
    error: Option<&EphemeralEnvError>,
) {
    let (outcome, failure_category) = match error {
        Some(error) => (
            "failure",
            Some(crate::error::CategorizedError::category(error)),
        ),
        None => ("success", None),
    };
    emit_event(&EphemeralLifecycleEvent {
        schema_version: EPHEMERAL_EVENT_SCHEMA_VERSION,
        environment_id: id,
        operation: "teardown",
        packages,
        duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        outcome,
        failure_category,
    });
}
