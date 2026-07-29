use fs4::{FileExt, TryLockError};

use super::{
    defaults::PackageSpec,
    lifecycle::EnvironmentId,
    orphan::{OrphanReclamationOutcome, publish_environment, reclaim_orphaned_environments},
    paths::verified_root,
};

#[test]
fn publish_environment_keeps_owner_lock_and_writes_diagnostics_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let root = verified_root(&temp.path().join("root")).unwrap();
    let id = EnvironmentId::new();
    let packages = [PackageSpec::parse("fixture-default-alpha").unwrap()];
    let published = publish_environment(&root, id, &packages).unwrap();
    let location = root.path().join("envs").join(id.to_string());
    let contender = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(location.join(".owner.lock"))
        .unwrap();

    assert!(matches!(
        FileExt::try_lock(&contender),
        Err(TryLockError::WouldBlock)
    ));
    let metadata = std::fs::read_to_string(location.join(".metadata.json")).unwrap();
    assert!(metadata.starts_with("{\"pid\":"));
    assert!(metadata.contains("\"created_at\":"));
    assert!(metadata.contains(&format!("\"environment_id\":\"{id}\"")));
    assert!(metadata.contains("\"packages\":[\"fixture-default-alpha\"]"));
    drop(published);
}

#[test]
fn reclamation_reports_live_environment_as_still_active_then_removes_it_after_lock_drop() {
    let temp = tempfile::tempdir().unwrap();
    let root = verified_root(&temp.path().join("root")).unwrap();
    let id = EnvironmentId::new();
    let published = publish_environment(&root, id, &[]).unwrap();
    assert!(
        matches!(reclaim_orphaned_environments(&root).unwrap().as_slice(), [OrphanReclamationOutcome::StillActive { id: actual_id }] if *actual_id == id)
    );

    drop(published);
    assert!(
        matches!(reclaim_orphaned_environments(&root).unwrap().as_slice(), [OrphanReclamationOutcome::Removed { id: actual_id }] if *actual_id == id)
    );
    assert!(!root.path().join("envs").join(id.to_string()).exists());
}

#[test]
fn reclamation_leaves_a_directory_without_an_owner_lock_unknown() {
    let temp = tempfile::tempdir().unwrap();
    let root = verified_root(&temp.path().join("root")).unwrap();
    let id = EnvironmentId::new();
    let location = super::permissions::create_environment_directory(&root, id).unwrap();
    assert!(
        matches!(reclaim_orphaned_environments(&root).unwrap().as_slice(), [OrphanReclamationOutcome::Unknown { id: actual_id }] if *actual_id == id)
    );
    assert!(location.exists());
}
