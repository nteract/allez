use allez::ephemeral::{ChannelPriorityMode, ReclamationStatus, create_ephemeral_environment};

use crate::support::{EventCapture, TestContext, package_specs, root_fixture_config};

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn automatic_reclamation_status_becomes_stable_terminal_value() {
    // Given
    let _context = TestContext::new("automatic-reclamation-status");

    // When
    let handle = create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let initial = handle.reclamation_outcomes();
    let _ready = handle.await_ready().await.unwrap();
    let terminal = handle.reclamation_outcomes();
    let repeated = handle.reclamation_outcomes();

    // Then
    assert_eq!(initial, ReclamationStatus::Scanning);
    assert!(matches!(terminal, ReclamationStatus::Complete(_)));
    assert_eq!(repeated, terminal);
    handle.signal_teardown();
    assert_eq!(handle.await_torn_down().await, Ok(()));
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn await_ready_survives_teardown_signaled_after_creation_completed() {
    // Given
    let context = TestContext::new("ready-outcome-survives-teardown");
    let capture = EventCapture::install();
    let handle = create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let id = handle.id();
    let expected_id = id.to_string();
    let location = context.environment_location(id);
    let mut creation_completed = false;
    for _ in 0..10_000 {
        creation_completed = capture.events().iter().any(|event| {
            event.environment_id.as_deref() == Some(expected_id.as_str())
                && event.operation.as_deref() == Some("create")
                && event.outcome.as_deref() == Some("success")
        });
        if creation_completed {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(creation_completed);

    // When
    handle.signal_teardown();
    let ready = handle.await_ready().await.unwrap();
    let teardown = handle.await_torn_down().await;

    // Then
    assert_eq!(ready.id, id);
    assert!(
        ready
            .installed_packages
            .iter()
            .any(|package| package.name == "fixture-probe")
    );
    assert_eq!(teardown, Ok(()));
    assert!(!location.exists());
}
