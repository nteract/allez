//! Explicit reap behavior (replaces GEN-24's original User Story 2
//! "automatic teardown/orphan reclamation" tests — see the GEN-24 spec's
//! "Explicit reap, no automatic reaping" decision). An ephemeral
//! environment is no longer torn down for the caller: it stays on disk,
//! usable, until [`reap_ephemeral_environments`] is called explicitly,
//! which removes every environment it finds unconditionally.

use allez::ephemeral::{ReapOutcome, create_ephemeral_environment, reap_ephemeral_environments};
use condarc::ChannelPriority;

use crate::support::{EventCapture, TestContext, package_specs, root_fixture_config};

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn a_ready_environment_is_not_torn_down_on_its_own() {
    // Given
    let _context = TestContext::new("no-automatic-teardown");

    // When
    let ready = create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriority::Strict),
        None,
    )
    .await
    .unwrap();
    let location = ready.location.clone();
    drop(ready);

    // Then: dropping every reference to the `ReadyEnvironment` has no
    // effect -- there is no RAII cleanup guard any more.
    assert!(location.is_dir());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn reap_removes_a_previously_created_environment_and_emits_a_teardown_event() {
    // Given
    let context = TestContext::new("reap-single-environment");
    let capture = EventCapture::install();
    let ready = create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriority::Strict),
        None,
    )
    .await
    .unwrap();
    let id = ready.id;
    let location = ready.location.clone();

    // When
    let outcomes = reap_ephemeral_environments().unwrap();

    // Then
    assert_eq!(outcomes, vec![ReapOutcome::Removed { id }]);
    assert!(!location.exists());
    assert!(!context.environment_location(id).exists());
    let teardown = capture
        .events()
        .into_iter()
        .find(|event| {
            event.environment_id.as_deref() == Some(id.to_string().as_str())
                && event.operation.as_deref() == Some("teardown")
        })
        .unwrap();
    assert_eq!(teardown.outcome.as_deref(), Some("success"));
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn reap_removes_every_environment_regardless_of_how_many_exist() {
    // Given
    let _context = TestContext::new("reap-multiple-environments");
    let first = create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriority::Strict),
        None,
    )
    .await
    .unwrap();
    let second = create_ephemeral_environment(
        package_specs(&["fixture-default-alpha"]),
        root_fixture_config(ChannelPriority::Strict),
        None,
    )
    .await
    .unwrap();

    // When
    let outcomes = reap_ephemeral_environments().unwrap();

    // Then
    assert_eq!(outcomes.len(), 2);
    assert!(
        outcomes
            .iter()
            .all(|outcome| matches!(outcome, ReapOutcome::Removed { .. }))
    );
    assert!(!first.location.exists());
    assert!(!second.location.exists());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn reaping_an_empty_root_returns_no_outcomes() {
    // Given
    let _context = TestContext::new("reap-empty-root");

    // When / Then
    assert!(reap_ephemeral_environments().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn reaping_twice_in_a_row_is_a_no_op_the_second_time() {
    // Given
    let _context = TestContext::new("reap-idempotent");
    create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriority::Strict),
        None,
    )
    .await
    .unwrap();

    // When
    let first = reap_ephemeral_environments().unwrap();
    let second = reap_ephemeral_environments().unwrap();

    // Then
    assert_eq!(first.len(), 1);
    assert!(second.is_empty());
}
