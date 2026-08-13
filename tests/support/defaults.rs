use std::collections::BTreeSet;

use allez::ephemeral::{PackageRequest, create_ephemeral_environment};
use condarc::ChannelPriority;

use crate::support::{TestContext, installed_names, package_specs, root_fixture_config};

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn create_ephemeral_environment_supersedes_matching_default_entry_by_bare_name() {
    // Given: a default entry pinning a version the fixture channel does not
    // publish, so the solve can only succeed if supersede dropped it.
    let _context = TestContext::new("supersede-by-bare-name");
    let request = PackageRequest {
        explicit: package_specs(&["fixture-default-alpha"]),
        defaults: package_specs(&["fixture-default-alpha=9.9.9"]),
    };

    // When
    let ready = create_ephemeral_environment(request, root_fixture_config(ChannelPriority::Strict))
        .await
        .unwrap();

    // Then
    assert_eq!(
        installed_names(&ready),
        BTreeSet::from(["fixture-default-alpha"])
    );
    assert_eq!(ready.installed_packages[0].version, "1.0.0");
}
