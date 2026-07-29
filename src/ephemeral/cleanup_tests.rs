use std::{path::Path, sync::Arc};

use super::{
    cleanup::{CleanupGuard, remove_prefix_dir},
    lifecycle::EnvironmentId,
    orphan::publish_environment,
    paths::verified_root,
};

#[test]
fn remove_prefix_dir_removes_a_nested_environment_tree() {
    let temp = tempfile::tempdir().unwrap();
    let root = verified_root(&temp.path().join("root")).unwrap();
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

    let temp = tempfile::tempdir().unwrap();
    let root = verified_root(&temp.path().join("root")).unwrap();
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
fn cleanup_guard_waits_for_the_last_arc_before_removing_the_environment() {
    let temp = tempfile::tempdir().unwrap();
    let root = verified_root(&temp.path().join("root")).unwrap();
    let id = EnvironmentId::new();
    let published = publish_environment(&root, id, &[]).unwrap();
    let location = root.path().join("envs").join(id.to_string());
    let root = Arc::new(root);
    let guard = Arc::new(CleanupGuard::new(
        Arc::clone(&root),
        id,
        published.owner_lock,
        Vec::new(),
    ));
    let ready_environment_keep_alive = Arc::clone(&guard);

    drop(guard);
    assert!(Path::new(&location).exists());
    drop(ready_environment_keep_alive);
    assert!(!location.exists());
}

#[test]
fn cleanup_guard_debug_redacts_credential_bearing_package_specs() {
    let temp = tempfile::tempdir().unwrap();
    let root = verified_root(&temp.path().join("root")).unwrap();
    let id = EnvironmentId::new();
    let published = publish_environment(&root, id, &[]).unwrap();
    let guard = CleanupGuard::new(
        Arc::new(root),
        id,
        published.owner_lock,
        vec!["https://user:password@repo.example/t/token-123/numpy".to_string()],
    );

    let debug_output = format!("{guard:?}");

    assert!(!debug_output.contains("user:password"));
    assert!(!debug_output.contains("token-123"));
    assert!(debug_output.contains("https://repo.example/numpy"));
}
