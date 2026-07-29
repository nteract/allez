use allez::ephemeral::{ChannelPriorityMode, create_ephemeral_environment};

use crate::support::{
    EventCapture, TestContext, package_specs, root_fixture_config, wait_until_removed,
};

fn assert_success_teardown_event(capture: &EventCapture, id: &str, packages: &str) {
    let events = capture.events();
    let teardown = events
        .iter()
        .find(|event| {
            event.environment_id.as_deref() == Some(id)
                && event.operation.as_deref() == Some("teardown")
        })
        .unwrap();
    assert_eq!(teardown.outcome.as_deref(), Some("success"));
    assert_eq!(teardown.packages.as_deref(), Some(packages));
    assert!(teardown.duration_ms.is_some());
    assert_eq!(teardown.failure_category, None);
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn explicit_teardown_removes_directory_and_emits_success_event() {
    // Given
    let _context = TestContext::new("explicit-teardown");
    let capture = EventCapture::install();
    let handle = create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let id = handle.id().to_string();
    let ready = handle.await_ready().await.unwrap();
    let location = ready.location.clone();

    // When
    handle.signal_teardown();
    let outcome = handle.await_torn_down().await;

    // Then
    assert_eq!(outcome, Ok(()));
    assert!(!location.exists());
    assert_success_teardown_event(&capture, &id, "[\"fixture-probe\"]");
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn dropping_last_environment_reference_removes_directory_and_emits_success_event() {
    // Given
    let _context = TestContext::new("drop-teardown");
    let capture = EventCapture::install();

    // When
    let (id, location) = {
        let handle = create_ephemeral_environment(
            package_specs(&["fixture-probe"]),
            root_fixture_config(ChannelPriorityMode::Strict),
            None,
        );
        let id = handle.id().to_string();
        let ready = handle.await_ready().await.unwrap();
        let location = ready.location.clone();

        // Normal return from main runs this Drop path. process::exit and abort skip it;
        // those callers rely on the next process's orphan-reclamation scan instead.
        (id, location)
    };
    wait_until_removed(&location).await;

    // Then
    assert_success_teardown_event(&capture, &id, "[\"fixture-probe\"]");
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn duplicate_teardown_after_completion_is_a_no_op() {
    // Given
    let _context = TestContext::new("duplicate-teardown");
    let handle = create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let _ready = handle.await_ready().await.unwrap();
    handle.signal_teardown();
    let first = handle.await_torn_down().await;

    // When
    handle.signal_teardown();
    let second = handle.await_torn_down().await;

    // Then
    assert_eq!(first, Ok(()));
    assert_eq!(second, first);
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn early_teardown_waits_for_creation_then_removes_environment() {
    // Given
    let _context = TestContext::new("early-teardown");
    let handle = create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );

    // When
    handle.signal_teardown();
    let ready = handle.await_ready().await.unwrap();
    let outcome = handle.await_torn_down().await;

    // Then
    assert!(
        ready
            .installed_packages
            .iter()
            .any(|package| package.name == "fixture-probe")
    );
    assert_eq!(outcome, Ok(()));
    assert!(!ready.location.exists());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn tearing_down_one_environment_preserves_concurrent_sibling() {
    // Given
    let _context = TestContext::new("isolated-siblings");
    let first = create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let second = create_ephemeral_environment(
        package_specs(&["fixture-default-alpha"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let first_ready = first.await_ready().await.unwrap();
    let second_ready = second.await_ready().await.unwrap();

    // When
    first.signal_teardown();
    let first_outcome = first.await_torn_down().await;

    // Then
    assert_eq!(first_outcome, Ok(()));
    assert!(!first_ready.location.exists());
    assert!(second_ready.location.is_dir());
    assert!(
        second_ready
            .installed_packages
            .iter()
            .any(|package| package.name == "fixture-default-alpha")
    );
    assert_eq!(
        second.await_ready().await.unwrap().location,
        second_ready.location
    );
    second.signal_teardown();
    assert_eq!(second.await_torn_down().await, Ok(()));
}
