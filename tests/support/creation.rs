use std::process::Command;

use allez::ephemeral::create_ephemeral_environment;
use condarc::{ChannelPriority, ResolvedChannels};

use crate::support::{
    EventCapture, TestContext, explicit_only, fixture_channel, installed_names, root_fixture_config,
};

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn resolvable_packages_are_installed_and_the_probe_is_usable() {
    // Given
    let _context = TestContext::new("resolvable-and-usable");
    let requested = ["fixture-default-alpha", "fixture-probe"];

    // When
    let ready = create_ephemeral_environment(
        explicit_only(&requested),
        root_fixture_config(ChannelPriority::Strict),
    )
    .await
    .unwrap();

    // Then
    assert!(ready.location.is_dir());
    let installed = installed_names(&ready);
    assert!(requested.iter().all(|package| installed.contains(package)));
    let overlay = ready.activation_environment().unwrap();
    #[cfg(unix)]
    let mut command = Command::new("fixture-probe");
    #[cfg(windows)]
    let mut command = Command::new("fixture-probe.cmd");
    command.envs(overlay);
    assert!(command.status().unwrap().success());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn flexible_channel_priority_solves_successfully() {
    // Given
    let _context = TestContext::new("flexible-priority");

    // When
    let ready = create_ephemeral_environment(
        explicit_only(&["fixture-probe"]),
        root_fixture_config(ChannelPriority::Flexible),
    )
    .await
    .unwrap();

    // Then
    assert_eq!(ready.installed_packages[0].name, "fixture-probe");
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn strict_channel_priority_selects_the_first_channels_version() {
    // Given
    let _context = TestContext::new("strict-priority-order");
    let config = ResolvedChannels::from_channels(vec![
        fixture_channel("priority-a"),
        fixture_channel("priority-b"),
    ]);

    // When
    let ready = create_ephemeral_environment(explicit_only(&["fixture-priority"]), config)
        .await
        .unwrap();

    // Then
    assert_eq!(ready.installed_packages[0].name, "fixture-priority");
    assert_eq!(ready.installed_packages[0].version, "1.0.0");
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn lifecycle_events_include_consistent_ids_packages_and_durations() {
    // Given
    let _context = TestContext::new("lifecycle-events");
    let capture = EventCapture::install();

    // When
    let ready = create_ephemeral_environment(
        explicit_only(&["fixture-probe"]),
        root_fixture_config(ChannelPriority::Strict),
    )
    .await
    .unwrap();
    let successful_id = ready.id.to_string();
    let failure = create_ephemeral_environment(
        explicit_only(&["fixture-corrupt-checksum"]),
        root_fixture_config(ChannelPriority::Strict),
    )
    .await
    .unwrap_err();
    let failed_id = failure.id.to_string();

    // Then
    let events = capture.events();
    let successful_steps = events
        .iter()
        .filter(|event| event.environment_id.as_deref() == Some(successful_id.as_str()))
        .filter(|event| matches!(event.operation.as_deref(), Some("create" | "install")))
        .collect::<Vec<_>>();
    assert_eq!(successful_steps.len(), 2);
    assert!(successful_steps.iter().all(|event| {
        event.packages.as_deref() == Some("[\"fixture-probe\"]") && event.duration_ms.is_some()
    }));
    let failed_steps = events
        .iter()
        .filter(|event| event.environment_id.as_deref() == Some(failed_id.as_str()))
        .filter(|event| matches!(event.operation.as_deref(), Some("create" | "install")))
        .collect::<Vec<_>>();
    assert_eq!(failed_steps.len(), 2);
    assert!(failed_steps.iter().all(|event| {
        event.packages.as_deref() == Some("[\"fixture-corrupt-checksum\"]")
            && event.duration_ms.is_some()
    }));
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn a_solve_stage_failure_still_emits_an_install_failure_event() {
    // Given
    let _context = TestContext::new("solve-stage-install-event");
    let capture = EventCapture::install();

    // When
    let failure = create_ephemeral_environment(
        explicit_only(&["fixture-does-not-exist"]),
        root_fixture_config(ChannelPriority::Strict),
    )
    .await
    .unwrap_err();
    let id = failure.id.to_string();

    // Then
    let install_failure = capture.events().into_iter().find(|event| {
        event.environment_id.as_deref() == Some(id.as_str())
            && event.operation.as_deref() == Some("install")
    });
    assert!(
        install_failure.is_some(),
        "expected an \"install\" event for a solve-stage failure, found none"
    );
    let install_failure = install_failure.unwrap();
    assert_eq!(install_failure.outcome.as_deref(), Some("failure"));
    assert_eq!(
        install_failure.failure_category.as_deref(),
        Some("unresolvable_package")
    );
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn a_ready_environment_is_not_torn_down_on_its_own() {
    // Given
    let _context = TestContext::new("no-automatic-teardown");

    // When
    let ready = create_ephemeral_environment(
        explicit_only(&["fixture-probe"]),
        root_fixture_config(ChannelPriority::Strict),
    )
    .await
    .unwrap();
    let location = ready.location.clone();
    drop(ready);

    // Then: dropping every reference to the `ReadyEnvironment` has no
    // effect -- there is no RAII cleanup guard, and (since GEN-24's later
    // revision removed `reap_ephemeral_environments`) no removal API at all.
    assert!(location.is_dir());
}

#[cfg(feature = "network-tests")]
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn resolved_defaults_channel_installs_a_real_package() {
    // Given
    let _context = TestContext::new("real-defaults-channel");

    // When
    let ready = create_ephemeral_environment(
        explicit_only(&["zlib"]),
        ResolvedChannels::from_channels(vec!["https://repo.anaconda.com/pkgs/main".to_string()]),
    )
    .await
    .unwrap();

    // Then
    assert!(
        ready
            .installed_packages
            .iter()
            .any(|package| package.name == "zlib")
    );
}
