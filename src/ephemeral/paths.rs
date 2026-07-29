//! `$ALLEZ_EPHEMERAL_ROOT` resolution, the secure open-or-create routine,
//! and `VerifiedRoot`.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use super::error::EphemeralEnvError;

#[cfg(unix)]
use rustix::{
    fd::OwnedFd,
    fs::{CWD, Mode, OFlags, fstat, mkdirat, openat},
    process::geteuid,
};

/// A root directory opened and verified before anchored operations use it.
///
/// Filesystem operations implemented in this crate use the retained directory
/// descriptors where the platform permits. Rattler's gateway, package-cache,
/// and installer APIs accept only `Path`/`PathBuf`, however, so those operations
/// must re-resolve [`Self::path`]. A local account that can rename entries in the
/// root's parent could replace the verified pathname between verification and a
/// rattler open. This TOCTOU exposure is accepted until rattler offers
/// handle-relative APIs; callers should place an explicit root under a parent
/// that untrusted local accounts cannot modify.
pub(crate) struct VerifiedRoot {
    path: PathBuf,
    #[cfg(unix)]
    _root_dir: OwnedFd,
    #[cfg(unix)]
    envs_dir: OwnedFd,
    #[cfg(unix)]
    _packages_dir: OwnedFd,
    #[cfg(unix)]
    _repodata_dir: OwnedFd,
}

impl VerifiedRoot {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(unix)]
    pub(crate) fn envs_dir(&self) -> &OwnedFd {
        &self.envs_dir
    }
}

pub(crate) fn resolve_root() -> Result<VerifiedRoot, EphemeralEnvError> {
    let path = match std::env::var_os("ALLEZ_EPHEMERAL_ROOT") {
        Some(configured) => PathBuf::from(configured),
        None => {
            let user = std::env::var("USER")
                .or_else(|_| std::env::var("LOGNAME"))
                .unwrap_or_else(|_| "unknown".to_string());
            let executable = std::env::current_exe()
                .and_then(std::fs::canonicalize)
                .map_err(|_| EphemeralEnvError::UnwritableLocation)?;
            let mut hasher = DefaultHasher::new();
            executable.hash(&mut hasher);
            fallback_root_path(
                &std::env::temp_dir(),
                &user,
                &format!("{:x}", hasher.finish()),
            )
        }
    };
    verified_root(&path)
}

pub(crate) fn fallback_root_path(temp_dir: &Path, user: &str, install_hash: &str) -> PathBuf {
    let sanitized_user = user
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        .collect::<String>();
    temp_dir.join(format!("allez-{sanitized_user}-{install_hash}"))
}

#[cfg(unix)]
pub(crate) fn verified_root(path: &Path) -> Result<VerifiedRoot, EphemeralEnvError> {
    let parent = path.parent().ok_or(EphemeralEnvError::UnwritableLocation)?;
    let name = path
        .file_name()
        .ok_or(EphemeralEnvError::UnwritableLocation)?;
    // Ordinary, symlink-following open: `parent` is an OS-conventional
    // ancestor chain (e.g. macOS's `/tmp` -> `/private/tmp`), not the
    // security-relevant component -- only `name` (this root's own,
    // predictable directory) is what an attacker could plant a symlink at,
    // so only its own open below uses `O_NOFOLLOW`.
    let parent_dir = openat(
        CWD,
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| EphemeralEnvError::UnwritableLocation)?;
    let root_dir = open_or_create_directory(&parent_dir, name)?;
    verify_directory(&root_dir)?;
    let envs_dir = open_or_create_directory(&root_dir, std::ffi::OsStr::new("envs"))?;
    let cache_dir = open_or_create_directory(&root_dir, std::ffi::OsStr::new("cache"))?;
    let packages_dir = open_or_create_directory(&cache_dir, std::ffi::OsStr::new("packages"))?;
    let repodata_dir = open_or_create_directory(&cache_dir, std::ffi::OsStr::new("repodata"))?;

    Ok(VerifiedRoot {
        path: path.to_path_buf(),
        _root_dir: root_dir,
        envs_dir,
        _packages_dir: packages_dir,
        _repodata_dir: repodata_dir,
    })
}

#[cfg(windows)]
pub(crate) fn verified_root(path: &Path) -> Result<VerifiedRoot, EphemeralEnvError> {
    ensure_owner_only_directory(path)?;
    ensure_owner_only_directory(&path.join("envs"))?;
    let cache = path.join("cache");
    ensure_owner_only_directory(&cache)?;
    ensure_owner_only_directory(&cache.join("packages"))?;
    ensure_owner_only_directory(&cache.join("repodata"))?;
    Ok(VerifiedRoot {
        path: path.to_path_buf(),
    })
}

/// Creates `path` with an owner-only ACL if absent, or verifies an
/// already-existing `path` is genuinely owner-only and not a reparse point
/// otherwise -- the Windows equivalent of the Unix pair this function
/// mirrors (`open_or_create_directory` and `verify_directory`), applied to
/// the root itself and each of its long-lived, reused-across-invocations
/// children (`envs`, `cache`, `cache/packages`, `cache/repodata`).
#[cfg(windows)]
fn ensure_owner_only_directory(path: &Path) -> Result<(), EphemeralEnvError> {
    super::permissions::create_directory_with_owner_only_acl(path)?;
    super::permissions::verify_owner_only_directory(path)
}

#[cfg(unix)]
fn open_or_create_directory(
    parent: &impl rustix::fd::AsFd,
    name: &std::ffi::OsStr,
) -> Result<OwnedFd, EphemeralEnvError> {
    let _mkdir_result = mkdirat(parent, name, Mode::RWXU);
    let directory = open_directory(parent, name)?;
    verify_directory(&directory)?;
    Ok(directory)
}

#[cfg(unix)]
fn open_directory(
    parent: impl rustix::fd::AsFd,
    name: impl rustix::path::Arg,
) -> Result<OwnedFd, EphemeralEnvError> {
    openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| EphemeralEnvError::UnwritableLocation)
}

#[cfg(unix)]
fn verify_directory(directory: &OwnedFd) -> Result<(), EphemeralEnvError> {
    let metadata = fstat(directory).map_err(|_| EphemeralEnvError::UnwritableLocation)?;
    let owner_matches = metadata.st_uid == geteuid().as_raw();
    let permissions_are_owner_only = metadata.st_mode & 0o777 == 0o700;
    if owner_matches && permissions_are_owner_only {
        Ok(())
    } else {
        Err(EphemeralEnvError::UnwritableLocation)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{fallback_root_path, verified_root};

    #[test]
    fn fallback_root_keeps_an_untrusted_username_to_one_component() {
        let resolver = super::resolve_root;
        let _ = resolver;
        let temp = tempfile::tempdir().unwrap();
        let root = fallback_root_path(temp.path(), "../other/user", "install");

        assert_eq!(root.parent(), Some(temp.path()));
        assert_eq!(
            root.file_name(),
            Some(std::ffi::OsStr::new("allez-otheruser-install"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn verified_root_rejects_an_explicit_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = temp.path().join("root-link");
        symlink(&target, &link).unwrap();

        assert!(verified_root(&link).is_err());
    }

    #[test]
    fn verified_root_creates_the_anchored_layout() {
        let temp = tempfile::tempdir().unwrap();
        let root_path = temp.path().join("root");
        let root = verified_root(&root_path).unwrap();

        assert_eq!(root.path(), Path::new(&root_path));
        #[cfg(unix)]
        {
            let _ = &root._root_dir;
            let _ = &root.envs_dir;
            let _ = &root._packages_dir;
            let _ = &root._repodata_dir;
        }
        assert!(root_path.join("envs").is_dir());
        assert!(root_path.join("cache/packages").is_dir());
        assert!(root_path.join("cache/repodata").is_dir());
    }
}
