use super::{
    cleanup::remove_prefix_dir,
    lifecycle::EnvironmentId,
    paths::{VerifiedRoot, verified_root},
};

/// Shared by every test below: a fresh temporary, verified root. The
/// returned `TempDir` must be kept alive alongside `VerifiedRoot` --
/// dropping it early removes the directory `VerifiedRoot` still refers to.
fn test_root() -> (tempfile::TempDir, VerifiedRoot) {
    let temp = tempfile::tempdir().unwrap();
    let root = verified_root(&temp.path().join("root")).unwrap();
    (temp, root)
}

#[test]
fn remove_prefix_dir_removes_a_nested_environment_tree() {
    let (_temp, root) = test_root();
    let id = EnvironmentId::new();
    let location = super::permissions::create_environment_directory(&root, id).unwrap();
    let nested = location.join("nested");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(nested.join("payload"), "payload").unwrap();

    remove_prefix_dir(&root, id).unwrap();
    assert!(!location.exists());
}

#[cfg(unix)]
#[test]
fn remove_prefix_dir_rejects_a_symlink_target_without_following_it() {
    use std::os::unix::fs::symlink;

    let (temp, root) = test_root();
    let id = EnvironmentId::new();
    let target = temp.path().join("outside");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("keep"), "keep").unwrap();
    symlink(&target, root.path().join("envs").join(id.to_string())).unwrap();

    assert!(remove_prefix_dir(&root, id).is_err());
    assert_eq!(
        std::fs::read_to_string(target.join("keep")).unwrap(),
        "keep"
    );
}

#[test]
fn remove_prefix_dir_on_a_never_created_environment_fails() {
    let (_temp, root) = test_root();
    let id = EnvironmentId::new();

    let error = remove_prefix_dir(&root, id).unwrap_err();

    assert_eq!(error, super::error::EphemeralEnvError::TeardownFailed);
}
