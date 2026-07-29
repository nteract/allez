#[cfg(unix)]
use std::time::UNIX_EPOCH;
use std::{
    fs::{File, OpenOptions},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use allez::ephemeral::{EnvironmentId, OrphanReclamationOutcome, reclaim_orphaned_environments};
use fs4::FileExt;

use crate::{
    orphan_support::{
        ChildGuard, HeldFileLock, METADATA_FILE, OWNER_LOCK_FILE, ROOT_LOCK_FILE,
        create_orphan_candidate, wait_for_file,
    },
    support::{EventCapture, TestContext},
};

#[cfg(unix)]
use crate::removal_failure::PermissionsGuard;

fn assert_reclamation_event(capture: &EventCapture, id: &str) {
    let events = capture.events();
    let teardown = events
        .iter()
        .find(|event| {
            event.environment_id.as_deref() == Some(id)
                && event.operation.as_deref() == Some("teardown")
        })
        .unwrap();
    assert_eq!(teardown.outcome.as_deref(), Some("success"));
    assert_eq!(teardown.packages.as_deref(), Some("[\"fixture-probe\"]"));
    assert!(teardown.duration_ms.is_some());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn simulated_unlocked_orphan_is_reclaimed() {
    // Given
    let context = TestContext::new("simulated-orphan");
    let capture = EventCapture::install();
    assert!(reclaim_orphaned_environments().unwrap().is_empty());
    let id = EnvironmentId::new();
    let location = create_orphan_candidate(context.root(), id, &["fixture-probe"]);

    // When
    let outcomes = reclaim_orphaned_environments().unwrap();

    // Then
    assert_eq!(outcomes, vec![OrphanReclamationOutcome::Removed { id }]);
    assert!(!location.exists());
    assert_reclamation_event(&capture, &id.to_string());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn crashed_owner_process_orphan_is_reclaimed() {
    // Given
    let context = TestContext::new("crashed-owner-orphan");
    let capture = EventCapture::install();
    assert!(reclaim_orphaned_environments().unwrap().is_empty());
    let id = EnvironmentId::new();
    let location = context.environment_location(id);
    let ready_file = context.root().join("child-ready");
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "user_story_2_orphans::crashed_owner_process_helper",
            "--nocapture",
        ])
        .env("ALLEZ_CRASH_HELPER_ROOT", context.root())
        .env("ALLEZ_CRASH_HELPER_ID", id.to_string())
        .env("ALLEZ_CRASH_HELPER_READY", &ready_file)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let child = ChildGuard::new(child);
    wait_for_file(&ready_file);
    assert!(location.join(OWNER_LOCK_FILE).is_file());

    // When
    child.kill_and_wait();
    let outcomes = reclaim_orphaned_environments().unwrap();

    // Then
    assert_eq!(outcomes, vec![OrphanReclamationOutcome::Removed { id }]);
    assert!(!location.exists());
    assert_reclamation_event(&capture, &id.to_string());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn crashed_owner_process_helper() {
    let Some(root) = std::env::var_os("ALLEZ_CRASH_HELPER_ROOT") else {
        return;
    };
    let id = std::env::var("ALLEZ_CRASH_HELPER_ID").unwrap();
    let ready_file = std::env::var_os("ALLEZ_CRASH_HELPER_READY").unwrap();
    let location = std::path::PathBuf::from(root).join("envs").join(&id);
    std::fs::create_dir(&location).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(&location, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let owner_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(location.join(OWNER_LOCK_FILE))
        .unwrap();
    FileExt::try_lock(&owner_lock).unwrap();
    let metadata = serde_json::json!({
        "pid": std::process::id(),
        "created_at": 1,
        "environment_id": id,
        "packages": ["fixture-probe"],
    });
    std::fs::write(
        location.join(METADATA_FILE),
        serde_json::to_vec(&metadata).unwrap(),
    )
    .unwrap();
    std::fs::write(ready_file, b"ready").unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn live_owner_lock_is_still_active_despite_old_modification_time() {
    // Given
    let context = TestContext::new("still-active-orphan");
    assert!(reclaim_orphaned_environments().unwrap().is_empty());
    let id = EnvironmentId::new();
    let location = create_orphan_candidate(context.root(), id, &["fixture-probe"]);
    #[cfg(unix)]
    File::open(&location)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1)))
        .unwrap();
    // std::fs has no safe Windows directory-open equivalent with backup semantics;
    // the same lock-based StillActive assertion runs there without backdating.
    let _owner_lock = HeldFileLock::acquire(location.join(OWNER_LOCK_FILE));

    // When
    let outcomes = reclaim_orphaned_environments().unwrap();

    // Then
    assert_eq!(outcomes, vec![OrphanReclamationOutcome::StillActive { id }]);
    assert!(location.is_dir());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn inaccessible_owner_lock_is_unknown_and_preserved() {
    // Given
    let context = TestContext::new("unknown-orphan");
    assert!(reclaim_orphaned_environments().unwrap().is_empty());
    let id = EnvironmentId::new();
    let location = create_orphan_candidate(context.root(), id, &["fixture-probe"]);
    let owner_lock = location.join(OWNER_LOCK_FILE);
    #[cfg(unix)]
    let _permissions = PermissionsGuard::set_mode(&owner_lock, 0o000);
    #[cfg(windows)]
    {
        std::fs::remove_file(&owner_lock).unwrap();
        std::fs::create_dir(&owner_lock).unwrap();
    }

    // When
    let outcomes = reclaim_orphaned_environments().unwrap();

    // Then
    assert_eq!(outcomes, vec![OrphanReclamationOutcome::Unknown { id }]);
    assert!(location.is_dir());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn reclamation_waits_for_root_lock_and_preserves_midpublication_environment() {
    // Given
    let context = TestContext::new("root-lock-serialization");
    assert!(reclaim_orphaned_environments().unwrap().is_empty());
    let root_lock = HeldFileLock::acquire(context.root().join("envs").join(ROOT_LOCK_FILE));
    let id = EnvironmentId::new();
    let location = context.environment_location(id);
    std::fs::create_dir(&location).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(&location, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let (started_sender, started_receiver) = mpsc::sync_channel(1);
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let scanner = std::thread::spawn(move || {
        started_sender.send(()).unwrap();
        result_sender.send(reclaim_orphaned_environments()).unwrap();
    });
    started_receiver.recv().unwrap();

    // When
    assert!(matches!(
        result_receiver.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    File::create(location.join(OWNER_LOCK_FILE)).unwrap();
    std::fs::write(
        location.join(METADATA_FILE),
        serde_json::to_vec(&serde_json::json!({
            "pid": std::process::id(),
            "created_at": 1,
            "environment_id": id.to_string(),
            "packages": ["fixture-probe"],
        }))
        .unwrap(),
    )
    .unwrap();
    let _owner_lock = HeldFileLock::acquire(location.join(OWNER_LOCK_FILE));
    drop(root_lock);
    let outcomes = result_receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    scanner.join().unwrap();

    // Then
    assert_eq!(outcomes, vec![OrphanReclamationOutcome::StillActive { id }]);
    assert!(location.is_dir());
}
