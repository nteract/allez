use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
pub(crate) struct PermissionsGuard {
    path: PathBuf,
    original: std::fs::Permissions,
}

#[cfg(unix)]
impl PermissionsGuard {
    pub(crate) fn set_mode(path: &Path, mode: u32) -> Self {
        let original = std::fs::metadata(path).unwrap().permissions();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        Self {
            path: path.to_path_buf(),
            original,
        }
    }
}

#[cfg(unix)]
impl Drop for PermissionsGuard {
    fn drop(&mut self) {
        let _result = std::fs::set_permissions(&self.path, self.original.clone());
    }
}

#[cfg(unix)]
pub(crate) struct RemovalFailureGuard {
    _permissions: PermissionsGuard,
}

#[cfg(unix)]
impl RemovalFailureGuard {
    pub(crate) fn inject(prefix: &Path) -> Self {
        Self {
            _permissions: PermissionsGuard::set_mode(prefix, 0o555),
        }
    }
}

#[cfg(windows)]
pub(crate) struct RemovalFailureGuard {
    _locked_file: std::fs::File,
}

#[cfg(windows)]
impl RemovalFailureGuard {
    pub(crate) fn inject(prefix: &Path) -> Self {
        use std::os::windows::fs::OpenOptionsExt;

        use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

        let locked_file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(prefix.join(".owner.lock"))
            .unwrap();
        Self {
            _locked_file: locked_file,
        }
    }
}
