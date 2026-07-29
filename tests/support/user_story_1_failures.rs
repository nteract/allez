use allez::ephemeral::{
    ChannelConfig, ChannelPriorityMode, ChannelSpec, EphemeralEnvError,
    create_ephemeral_environment, reclaim_orphaned_environments,
};

use crate::{
    orphan_support::{HeldFileLock, ROOT_LOCK_FILE},
    support::{EventCapture, TestContext, fixture_channel, package_specs, root_fixture_config},
};

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn unresolvable_package_returns_failure_without_a_partial_directory() {
    // Given
    let context = TestContext::new("unresolvable-package");
    let handle = create_ephemeral_environment(
        package_specs(&["fixture-does-not-exist"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let location = context.environment_location(handle.id());

    // When
    let failure = handle.await_ready().await.unwrap_err();

    // Then
    assert!(matches!(
        failure.error,
        EphemeralEnvError::UnresolvablePackage { .. }
    ));
    assert_eq!(failure.cleanup_error, None);
    assert!(!location.exists());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn denied_configured_channel_returns_no_channels() {
    // Given
    let context = TestContext::new("denied-channel");
    let channel = fixture_channel("");
    let config = ChannelConfig {
        channels: vec![ChannelSpec {
            url_or_name: channel.clone(),
        }],
        channel_priority: ChannelPriorityMode::Strict,
        allowed_channels: Vec::new(),
        denied_channels: vec![channel],
    };

    // When
    let handle = create_ephemeral_environment(package_specs(&["fixture-probe"]), config, None);
    let location = context.environment_location(handle.id());
    let failure = handle.await_ready().await.unwrap_err();

    // Then
    assert_eq!(failure.error, EphemeralEnvError::NoChannelsConfigured);
    assert!(!location.exists());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn channel_absent_from_non_empty_allowlist_returns_no_channels() {
    // Given
    let context = TestContext::new("channel-not-allowed");
    let config = ChannelConfig {
        channels: vec![ChannelSpec {
            url_or_name: fixture_channel(""),
        }],
        channel_priority: ChannelPriorityMode::Strict,
        allowed_channels: vec!["different-channel".to_string()],
        denied_channels: Vec::new(),
    };

    // When
    let handle = create_ephemeral_environment(package_specs(&["fixture-probe"]), config, None);
    let location = context.environment_location(handle.id());
    let failure = handle.await_ready().await.unwrap_err();

    // Then
    assert_eq!(failure.error, EphemeralEnvError::NoChannelsConfigured);
    assert!(!location.exists());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn corrupt_checksum_returns_integrity_failure_without_a_partial_directory() {
    // Given
    let context = TestContext::new("corrupt-checksum");
    let handle = create_ephemeral_environment(
        package_specs(&["fixture-corrupt-checksum"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let location = context.environment_location(handle.id());

    // When
    let failure = handle.await_ready().await.unwrap_err();

    // Then
    assert_eq!(
        failure.error,
        EphemeralEnvError::IntegrityVerificationFailed {
            package: "fixture-corrupt-checksum".to_string(),
        }
    );
    assert_eq!(failure.cleanup_error, None);
    assert!(!location.exists());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn denied_defaults_fallback_returns_no_channels_without_network() {
    // Given
    let context = TestContext::new("denied-defaults-fallback");
    let config = ChannelConfig {
        channels: Vec::new(),
        channel_priority: ChannelPriorityMode::Strict,
        allowed_channels: Vec::new(),
        denied_channels: vec!["defaults".to_string()],
    };

    // When
    let handle = create_ephemeral_environment(package_specs(&["fixture-probe"]), config, None);
    let location = context.environment_location(handle.id());
    let failure = handle.await_ready().await.unwrap_err();

    // Then
    assert_eq!(failure.error, EphemeralEnvError::NoChannelsConfigured);
    assert!(!location.exists());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn root_resolution_failure_emits_no_fabricated_teardown_event() {
    // Given
    let capture = EventCapture::install();
    let temporary_directory = tempfile::tempdir().unwrap();
    let target = temporary_directory.path().join("target");
    std::fs::create_dir(&target).unwrap();
    let root_link = temporary_directory.path().join("root-link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &root_link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&target, &root_link).unwrap();
    // SAFETY: Category 13, library contract. This test holds serial_test's
    // process-wide lock, so no other test can access the process
    // environment while this mutation runs.
    unsafe {
        std::env::set_var("ALLEZ_EPHEMERAL_ROOT", &root_link);
    }

    // When
    let handle = create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let id = handle.id().to_string();
    let failure = handle.await_ready().await.unwrap_err();
    let teardown_result = handle.await_torn_down().await;

    // Then
    assert_eq!(failure.error, EphemeralEnvError::UnwritableLocation);
    assert_eq!(failure.cleanup_error, None);
    assert_eq!(teardown_result, Ok(()));
    let teardown_events = capture
        .events()
        .into_iter()
        .filter(|event| {
            event.environment_id.as_deref() == Some(id.as_str())
                && event.operation.as_deref() == Some("teardown")
        })
        .count();
    assert_eq!(
        teardown_events, 0,
        "root resolution itself failed before any teardown ran; expected zero teardown events"
    );
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn publication_root_lock_contention_emits_no_fabricated_teardown_event() {
    // Given: the root's own `envs` directory materialized (a single
    // reclamation scan is enough), then its `.root.lock` held externally so
    // `publish_environment`'s own root-lock acquisition -- not directory
    // creation -- is what fails; no environment directory is ever created
    // for this attempt.
    let context = TestContext::new("publication-root-lock-contention");
    assert!(reclaim_orphaned_environments().unwrap().is_empty());
    let capture = EventCapture::install();
    let held_root_lock = HeldFileLock::acquire(context.root().join("envs").join(ROOT_LOCK_FILE));

    // When
    let handle = create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let id = handle.id().to_string();
    let failure = handle.await_ready().await.unwrap_err();
    let teardown_result = handle.await_torn_down().await;
    drop(held_root_lock);

    // Then
    assert_eq!(failure.error, EphemeralEnvError::UnwritableLocation);
    assert_eq!(
        failure.cleanup_error, None,
        "no directory was ever created for this attempt, so there is nothing whose cleanup could have failed"
    );
    assert_eq!(teardown_result, Ok(()));
    assert!(!context.environment_location(&id).exists());
    let teardown_events = capture
        .events()
        .into_iter()
        .filter(|event| {
            event.environment_id.as_deref() == Some(id.as_str())
                && event.operation.as_deref() == Some("teardown")
        })
        .count();
    assert_eq!(
        teardown_events, 0,
        "publish_environment's own root-lock acquisition failed before any directory was \
         created; expected zero teardown events, not a fabricated one from a second, \
         redundant removal attempt on a directory that never existed"
    );
}
