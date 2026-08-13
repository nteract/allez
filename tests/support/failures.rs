use allez::ephemeral::{EphemeralEnvError, create_ephemeral_environment};
use condarc::{ChannelPriority, ResolvedChannels};

use crate::support::{TestContext, explicit_only, fixture_channel, root_fixture_config};

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn unresolvable_package_returns_failure_without_a_partial_directory() {
    // Given
    let context = TestContext::new("unresolvable-package");

    // When
    let failure = create_ephemeral_environment(
        explicit_only(&["fixture-does-not-exist"]),
        root_fixture_config(ChannelPriority::Strict),
    )
    .await
    .unwrap_err();

    // Then
    assert!(matches!(
        failure.error,
        EphemeralEnvError::UnresolvablePackage { .. }
    ));
    assert_eq!(failure.cleanup_error, None);
    assert!(!context.environment_location(failure.id).exists());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn deny_filtered_channel_list_returns_no_channels() {
    // Given
    let context = TestContext::new("denied-channel");
    let channel = fixture_channel("");
    let condarc = format!("channels: [\"{channel}\"]\ndenylist_channels: [\"{channel}\"]\n");
    let config = condarc::parse(&condarc).unwrap();
    let deny_filtered_config = condarc::expand_channels(&config).unwrap();
    assert!(deny_filtered_config.channels.is_empty());

    // When
    let failure =
        create_ephemeral_environment(explicit_only(&["fixture-probe"]), deny_filtered_config)
            .await
            .unwrap_err();

    // Then
    assert_eq!(failure.error, EphemeralEnvError::NoChannelsConfigured);
    assert!(!context.environment_location(failure.id).exists());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn allow_filtered_channel_list_returns_no_channels() {
    // Given
    let context = TestContext::new("channel-not-allowed");
    let channel = fixture_channel("");
    let condarc = format!(
        "channels: [\"{channel}\"]\nallowlist_channels: [\"https://repo.example.org/different-channel\"]\n"
    );
    let config = condarc::parse(&condarc).unwrap();
    let allow_filtered_config = condarc::expand_channels(&config).unwrap();
    assert!(allow_filtered_config.channels.is_empty());

    // When
    let failure =
        create_ephemeral_environment(explicit_only(&["fixture-probe"]), allow_filtered_config)
            .await
            .unwrap_err();

    // Then
    assert_eq!(failure.error, EphemeralEnvError::NoChannelsConfigured);
    assert!(!context.environment_location(failure.id).exists());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn corrupt_checksum_returns_integrity_failure_without_a_partial_directory() {
    // Given
    let context = TestContext::new("corrupt-checksum");

    // When
    let failure = create_ephemeral_environment(
        explicit_only(&["fixture-corrupt-checksum"]),
        root_fixture_config(ChannelPriority::Strict),
    )
    .await
    .unwrap_err();

    // Then
    assert_eq!(
        failure.error,
        EphemeralEnvError::IntegrityVerificationFailed {
            package: "fixture-corrupt-checksum".to_string(),
        }
    );
    assert_eq!(failure.cleanup_error, None);
    assert!(!context.environment_location(failure.id).exists());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn empty_resolved_channels_return_no_channels_without_network() {
    // Given
    let context = TestContext::new("denied-defaults-fallback");
    let config = ResolvedChannels::from_channels(Vec::new());

    // When
    let failure = create_ephemeral_environment(explicit_only(&["fixture-probe"]), config)
        .await
        .unwrap_err();

    // Then
    assert_eq!(failure.error, EphemeralEnvError::NoChannelsConfigured);
    assert!(!context.environment_location(failure.id).exists());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn root_resolution_failure_does_not_attempt_cleanup() {
    // Given
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
    let failure = create_ephemeral_environment(
        explicit_only(&["fixture-probe"]),
        root_fixture_config(ChannelPriority::Strict),
    )
    .await
    .unwrap_err();

    // Then
    assert_eq!(failure.error, EphemeralEnvError::UnwritableLocation);
    assert_eq!(failure.cleanup_error, None);
}
