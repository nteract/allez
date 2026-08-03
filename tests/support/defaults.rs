use std::collections::BTreeSet;

use allez::ephemeral::{DEFAULT_PACKAGES, RequestedPackages, create_ephemeral_environment};
use condarc::ChannelPriority;

use crate::support::{
    TestContext, explicit_package_specs, installed_names, package_specs, root_fixture_config,
};

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn no_packages_with_an_override_installs_the_override_instead_of_defaults() {
    // Given
    let _context = TestContext::new("override-instead-of-defaults");
    let override_packages = explicit_package_specs(&["fixture-default-beta"]);

    // When
    let ready = create_ephemeral_environment(
        RequestedPackages::UseDefaultOrOverride,
        root_fixture_config(ChannelPriority::Strict),
        Some(override_packages),
    )
    .await
    .unwrap();

    // Then
    assert_eq!(
        installed_names(&ready),
        BTreeSet::from(["fixture-default-beta"])
    );
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn explicit_packages_alongside_an_override_ignore_the_override_entirely() {
    // Given
    let _context = TestContext::new("explicit-wins-over-override");
    let override_packages = explicit_package_specs(&["fixture-default-beta"]);

    // When
    let ready = create_ephemeral_environment(
        package_specs(&["fixture-default-alpha"]),
        root_fixture_config(ChannelPriority::Strict),
        Some(override_packages),
    )
    .await
    .unwrap();

    // Then
    assert_eq!(
        installed_names(&ready),
        BTreeSet::from(["fixture-default-alpha"])
    );
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn an_override_resolving_to_empty_falls_back_to_default_packages() {
    // Given
    let _context = TestContext::new("empty-override-falls-back");

    // When
    let ready = create_ephemeral_environment(
        RequestedPackages::UseDefaultOrOverride,
        root_fixture_config(ChannelPriority::Strict),
        Some(Vec::new()),
    )
    .await
    .unwrap();

    // Then
    assert_eq!(
        installed_names(&ready),
        DEFAULT_PACKAGES.iter().copied().collect()
    );
}
