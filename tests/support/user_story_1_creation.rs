use std::{collections::BTreeSet, process::Command};

use allez::ephemeral::{
    ChannelConfig, ChannelPriorityMode, DEFAULT_PACKAGES, RequestedPackages,
    create_ephemeral_environment,
};

use crate::support::{
    EventCapture, TestContext, fixture_channel, package_specs, root_fixture_config,
};

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn resolvable_packages_are_installed_and_the_probe_is_usable() {
    // Given
    let _context = TestContext::new("resolvable-and-usable");
    let requested = ["fixture-default-alpha", "fixture-probe"];

    // When
    let handle = create_ephemeral_environment(
        package_specs(&requested),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let ready = handle.await_ready().await.unwrap();

    // Then
    assert!(ready.location.is_dir());
    let installed = ready
        .installed_packages
        .iter()
        .map(|package| package.name.as_str())
        .collect::<BTreeSet<_>>();
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
async fn empty_package_list_installs_built_in_defaults() {
    // Given
    let _context = TestContext::new("built-in-defaults");

    // When
    let ready = create_ephemeral_environment(
        RequestedPackages::Explicit(Vec::new()),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    )
    .await_ready()
    .await
    .unwrap();

    // Then
    let installed = ready
        .installed_packages
        .iter()
        .map(|package| package.name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(installed, DEFAULT_PACKAGES.iter().copied().collect());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn flexible_channel_priority_solves_successfully() {
    // Given
    let _context = TestContext::new("flexible-priority");

    // When
    let ready = create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriorityMode::Flexible),
        None,
    )
    .await_ready()
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
    let config = ChannelConfig::from_urls(vec![
        fixture_channel("priority-a"),
        fixture_channel("priority-b"),
    ]);

    // When
    let ready = create_ephemeral_environment(package_specs(&["fixture-priority"]), config, None)
        .await_ready()
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
    let successful_handle = create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let successful_id = successful_handle.id().to_string();
    let _ready = successful_handle.await_ready().await.unwrap();
    let failed_handle = create_ephemeral_environment(
        package_specs(&["fixture-corrupt-checksum"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let failed_id = failed_handle.id().to_string();
    let _failure = failed_handle.await_ready().await.unwrap_err();

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
    let handle = create_ephemeral_environment(
        package_specs(&["fixture-does-not-exist"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let id = handle.id().to_string();
    let _failure = handle.await_ready().await.unwrap_err();

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

#[cfg(feature = "network-tests")]
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn empty_channels_fallback_resolves_against_real_defaults_channel() {
    // Given
    let _context = TestContext::new("real-defaults-channel");

    // When
    let ready = create_ephemeral_environment(
        package_specs(&["zlib"]),
        ChannelConfig::from_urls(Vec::new()),
        None,
    )
    .await_ready()
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
