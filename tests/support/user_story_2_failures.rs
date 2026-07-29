use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use allez::ephemeral::{ChannelPriorityMode, EphemeralEnvError, create_ephemeral_environment};

use crate::{
    removal_failure::RemovalFailureGuard,
    support::{EventCapture, TestContext, package_specs, root_fixture_config},
};

fn assert_failure_teardown_event(capture: &EventCapture, id: &str, packages: &str) {
    let events = capture.events();
    let teardown = events
        .iter()
        .find(|event| {
            event.environment_id.as_deref() == Some(id)
                && event.operation.as_deref() == Some("teardown")
        })
        .unwrap();
    assert_eq!(teardown.outcome.as_deref(), Some("failure"));
    assert_eq!(teardown.packages.as_deref(), Some(packages));
    assert!(teardown.duration_ms.is_some());
    assert_eq!(
        teardown.failure_category.as_deref(),
        Some("teardown_failed")
    );
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn concurrent_teardown_outcomes_remain_isolated() {
    // Given
    let _context = TestContext::new("isolated-teardown-outcomes");
    let failing = create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let successful = create_ephemeral_environment(
        package_specs(&["fixture-default-alpha"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let failing_ready = failing.await_ready().await.unwrap();
    let successful_ready = successful.await_ready().await.unwrap();
    let failure_injection = RemovalFailureGuard::inject(&failing_ready.location);

    // When
    failing.signal_teardown();
    successful.signal_teardown();
    let (failing_outcome, successful_outcome) =
        tokio::join!(failing.await_torn_down(), successful.await_torn_down());

    // Then
    assert_eq!(failing_outcome, Err(EphemeralEnvError::TeardownFailed));
    assert_eq!(successful_outcome, Ok(()));
    assert!(failing_ready.location.exists());
    assert!(!successful_ready.location.exists());
    drop(failure_injection);
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn failed_teardown_returns_distinct_error_and_emits_failure_event() {
    // Given
    let _context = TestContext::new("failed-teardown");
    let capture = EventCapture::install();
    let handle = create_ephemeral_environment(
        package_specs(&["fixture-probe"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let id = handle.id().to_string();
    let ready = handle.await_ready().await.unwrap();
    let failure_injection = RemovalFailureGuard::inject(&ready.location);

    // When
    handle.signal_teardown();
    let outcome = handle.await_torn_down().await;

    // Then
    assert_eq!(outcome, Err(EphemeralEnvError::TeardownFailed));
    assert!(ready.location.exists());
    assert_failure_teardown_event(&capture, &id, "[\"fixture-probe\"]");
    drop(failure_injection);
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn creation_and_cleanup_failures_are_both_reported() {
    // Given
    let context = TestContext::new("creation-and-cleanup-failures");
    let capture = EventCapture::install();
    let handle = create_ephemeral_environment(
        package_specs(&["fixture-does-not-exist"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let id = handle.id().to_string();
    let location = context.environment_location(handle.id());
    let injected = Arc::new(Mutex::new(None));
    let injected_from_hook = Arc::clone(&injected);
    let location_from_hook = location.clone();
    let (cleanup_running_sender, cleanup_running_receiver) = mpsc::sync_channel(1);
    let (release_sender, release_receiver) = mpsc::sync_channel(1);
    capture.on_creation_failure(move || {
        *injected_from_hook.lock().unwrap() =
            Some(RemovalFailureGuard::inject(&location_from_hook));
        cleanup_running_sender.send(()).unwrap();
        release_receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
    });
    let waiter_completed = Arc::new(AtomicBool::new(false));
    let waiter_completed_from_thread = Arc::clone(&waiter_completed);
    let waiter_handle = handle.clone();
    let waiter = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(waiter_handle.await_ready());
        waiter_completed_from_thread.store(true, Ordering::Release);
        result
    });
    let waiter_completed_during_cleanup = Arc::clone(&waiter_completed);
    let controller = std::thread::spawn(move || {
        cleanup_running_receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(!waiter_completed_during_cleanup.load(Ordering::Acquire));
        release_sender.send(()).unwrap();
    });

    // When
    for _ in 0..10_000 {
        if waiter_completed.load(Ordering::Acquire) {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(waiter_completed.load(Ordering::Acquire));
    let failure = waiter.join().unwrap().unwrap_err();
    controller.join().unwrap();

    // Then
    assert!(matches!(
        failure.error,
        EphemeralEnvError::UnresolvablePackage { .. }
    ));
    assert_eq!(
        failure.cleanup_error,
        Some(EphemeralEnvError::TeardownFailed)
    );
    assert_failure_teardown_event(&capture, &id, "[\"fixture-does-not-exist\"]");
    drop(injected.lock().unwrap().take().unwrap());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn teardown_signal_during_creation_failure_cleanup_folds_into_single_attempt() {
    // Given
    let context = TestContext::new("signal-during-failure-cleanup");
    let capture = EventCapture::install();
    let handle = create_ephemeral_environment(
        package_specs(&["fixture-does-not-exist"]),
        root_fixture_config(ChannelPriorityMode::Strict),
        None,
    );
    let id = handle.id().to_string();
    let location = context.environment_location(handle.id());
    let injected = Arc::new(Mutex::new(None));
    let injected_from_hook = Arc::clone(&injected);
    let signaled_handle = handle.clone();
    capture.on_creation_failure(move || {
        *injected_from_hook.lock().unwrap() = Some(RemovalFailureGuard::inject(&location));
        signaled_handle.signal_teardown();
    });

    // When
    let failure = handle.await_ready().await.unwrap_err();
    let teardown = handle.await_torn_down().await;

    // Then
    assert!(matches!(
        failure.error,
        EphemeralEnvError::UnresolvablePackage { .. }
    ));
    assert_eq!(
        failure.cleanup_error,
        Some(EphemeralEnvError::TeardownFailed)
    );
    assert_eq!(teardown, Err(EphemeralEnvError::TeardownFailed));
    let teardown_events = capture
        .events()
        .into_iter()
        .filter(|event| {
            event.environment_id.as_deref() == Some(id.as_str())
                && event.operation.as_deref() == Some("teardown")
        })
        .count();
    assert_eq!(teardown_events, 1);
    drop(injected.lock().unwrap().take().unwrap());
}
