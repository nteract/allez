use std::fs::File;

#[cfg(not(unix))]
use std::fs::OpenOptions;

use super::{error::EphemeralEnvError, lifecycle::EnvironmentId, paths::VerifiedRoot};

#[cfg(unix)]
use rustix::{
    fd::OwnedFd,
    fs::{AtFlags, Dir, FileType, Mode, OFlags, openat, statat},
};

const OWNER_LOCK_FILE: &str = ".owner.lock";
const ROOT_LOCK_FILE: &str = ".root.lock";
pub(super) const METADATA_FILE: &str = ".metadata.json";

/// One `envs/` entry found while enumerating reclamation candidates.
pub(super) enum EnvironmentCandidate {
    /// Confirmed to be a directory; safe to proceed to owner-lock inspection.
    Directory(EnvironmentId),
    /// The entry's own type could not be determined (e.g. a `stat` failure
    /// on that one entry); reported as `Unknown` rather than silently
    /// dropped, so it is never left undetected (FR-008).
    Unknown(EnvironmentId),
}

/// Classifies one already-named `envs/` entry from its directory-type
/// check outcome. `Ok(false)` (present, but not a directory) yields `None`
/// -- it is not a reclamation candidate at all, not an unknown one.
fn classify_candidate(
    id: EnvironmentId,
    is_directory: Result<bool, ()>,
) -> Option<EnvironmentCandidate> {
    match is_directory {
        Ok(true) => Some(EnvironmentCandidate::Directory(id)),
        Ok(false) => None,
        Err(()) => Some(EnvironmentCandidate::Unknown(id)),
    }
}

#[cfg(unix)]
pub(super) fn root_lock_file(root: &VerifiedRoot) -> std::io::Result<File> {
    file_from_fd(openat(
        root.envs_dir(),
        ROOT_LOCK_FILE,
        OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_raw_mode(0o600),
    )?)
}

#[cfg(unix)]
pub(super) fn owner_lock_file(
    root: &VerifiedRoot,
    id: EnvironmentId,
    create: bool,
) -> std::io::Result<File> {
    let mut flags = OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    if create {
        flags |= OFlags::CREATE;
    }
    file_from_fd(openat(
        &environment_directory(root, id)?,
        OWNER_LOCK_FILE,
        flags,
        Mode::from_raw_mode(0o600),
    )?)
}

#[cfg(unix)]
pub(super) fn metadata_file(root: &VerifiedRoot, id: EnvironmentId) -> std::io::Result<File> {
    file_from_fd(openat(
        &environment_directory(root, id)?,
        METADATA_FILE,
        OFlags::WRONLY | OFlags::CREATE | OFlags::TRUNC | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_raw_mode(0o600),
    )?)
}

#[cfg(unix)]
fn environment_directory(root: &VerifiedRoot, id: EnvironmentId) -> rustix::io::Result<OwnedFd> {
    openat(
        root.envs_dir(),
        id.to_string(),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
}

#[cfg(unix)]
fn file_from_fd(descriptor: OwnedFd) -> std::io::Result<File> {
    Ok(File::from(descriptor))
}

#[cfg(unix)]
pub(super) fn environment_candidates(
    root: &VerifiedRoot,
) -> Result<Vec<EnvironmentCandidate>, EphemeralEnvError> {
    let mut candidates = Vec::new();
    let mut entries =
        Dir::read_from(root.envs_dir()).map_err(|_| EphemeralEnvError::UnwritableLocation)?;
    for entry in &mut entries {
        let entry = entry.map_err(|_| EphemeralEnvError::UnwritableLocation)?;
        let name = entry.file_name();
        let Ok(value) = name.to_str() else {
            continue;
        };
        let Some(id) = EnvironmentId::parse(value) else {
            continue;
        };
        let is_directory = statat(root.envs_dir(), name, AtFlags::SYMLINK_NOFOLLOW)
            .map(|metadata| FileType::from_raw_mode(metadata.st_mode).is_dir())
            .map_err(|_| ());
        if let Some(candidate) = classify_candidate(id, is_directory) {
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

#[cfg(not(unix))]
pub(super) fn root_lock_file(root: &VerifiedRoot) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.path().join("envs").join(ROOT_LOCK_FILE))
}

#[cfg(not(unix))]
pub(super) fn owner_lock_file(
    root: &VerifiedRoot,
    id: EnvironmentId,
    create: bool,
) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(create)
        .truncate(false);
    options.open(
        root.path()
            .join("envs")
            .join(id.to_string())
            .join(OWNER_LOCK_FILE),
    )
}

#[cfg(not(unix))]
pub(super) fn metadata_file(root: &VerifiedRoot, id: EnvironmentId) -> std::io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(
            root.path()
                .join("envs")
                .join(id.to_string())
                .join(METADATA_FILE),
        )
}

#[cfg(not(unix))]
pub(super) fn environment_candidates(
    root: &VerifiedRoot,
) -> Result<Vec<EnvironmentCandidate>, EphemeralEnvError> {
    let entries = std::fs::read_dir(root.path().join("envs"))
        .map_err(|_| EphemeralEnvError::UnwritableLocation)?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| EphemeralEnvError::UnwritableLocation)?;
        let Some(id) = entry.file_name().to_str().and_then(EnvironmentId::parse) else {
            continue;
        };
        let is_directory = entry.file_type().map(|kind| kind.is_dir()).map_err(|_| ());
        if let Some(candidate) = classify_candidate(id, is_directory) {
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::{EnvironmentCandidate, classify_candidate};
    use crate::ephemeral::EnvironmentId;

    #[test]
    fn classify_candidate_reports_a_confirmed_directory() {
        let id = EnvironmentId::new();
        assert!(matches!(
            classify_candidate(id, Ok(true)),
            Some(EnvironmentCandidate::Directory(actual_id)) if actual_id == id
        ));
    }

    #[test]
    fn classify_candidate_is_not_a_candidate_when_confirmed_not_a_directory() {
        assert!(classify_candidate(EnvironmentId::new(), Ok(false)).is_none());
    }

    #[test]
    fn classify_candidate_reports_unknown_when_its_type_cannot_be_determined() {
        let id = EnvironmentId::new();
        assert!(matches!(
            classify_candidate(id, Err(())),
            Some(EnvironmentCandidate::Unknown(actual_id)) if actual_id == id
        ));
    }
}
