use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Barrier},
};

use super::{
    error::{ActivationError, CreationFailure, EphemeralEnvError},
    lifecycle::{
        CleanupOutcome, CreationOutcomeCell, EnvironmentId, EphemeralEnvironmentHandle,
        LifecycleState, ReadyEnvironment, TeardownOutcome,
    },
};

fn ready_environment(location: &str) -> ReadyEnvironment {
    ReadyEnvironment::test_with_location(location)
}

#[test]
fn activation_environment_when_prefix_has_executables_includes_prefix_path() {
    // Given
    let temporary_directory = tempfile::tempdir().unwrap();
    let prefix = temporary_directory.path().join("environment");
    #[cfg(windows)]
    let executable_directory = prefix.join("Scripts");
    #[cfg(not(windows))]
    let executable_directory = prefix.join("bin");
    fs::create_dir_all(&executable_directory).unwrap();
    let environment = ReadyEnvironment::test_with_location(&prefix);

    // When
    let overlay = environment.activation_environment().unwrap();

    // Then
    let path = overlay
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| value)
        .unwrap();
    assert!(std::env::split_paths(path).any(|entry| entry == executable_directory));
}

#[test]
fn activation_environment_when_prefix_state_is_malformed_returns_activation_error() {
    // Given
    let temporary_directory = tempfile::tempdir().unwrap();
    let prefix = temporary_directory.path().join("environment");
    fs::create_dir_all(prefix.join("conda-meta")).unwrap();
    fs::write(prefix.join("conda-meta/state"), "{").unwrap();
    let environment = ReadyEnvironment::test_with_location(&prefix);

    // When
    let result = environment.activation_environment();

    // Then
    assert!(matches!(result, Err(ActivationError { .. })));
}

#[test]
fn signal_teardown_transitions_creating_to_queued() {
    let mut state = LifecycleState::Creating;

    assert_eq!(state.signal_teardown(), None);
    assert!(matches!(state, LifecycleState::CreatingTeardownQueued));
}

#[test]
fn signal_teardown_captures_ready_location_before_tearing_down() {
    let mut state = LifecycleState::Ready(ready_environment("/tmp/ready"));

    assert_eq!(state.signal_teardown(), Some("/tmp/ready".into()));
    assert!(matches!(state, LifecycleState::TearingDown));
}

#[test]
fn signal_teardown_folds_all_terminal_and_in_progress_states() {
    let states = [
        LifecycleState::CreatingTeardownQueued,
        LifecycleState::TearingDown,
        LifecycleState::TornDown(TeardownOutcome::Succeeded),
        LifecycleState::TornDown(TeardownOutcome::Failed(EphemeralEnvError::TeardownFailed)),
        LifecycleState::CreationFailed {
            error: EphemeralEnvError::UnwritableLocation,
            cleanup: CleanupOutcome::Running,
        },
        LifecycleState::CreationFailed {
            error: EphemeralEnvError::UnwritableLocation,
            cleanup: CleanupOutcome::Succeeded,
        },
        LifecycleState::CreationFailed {
            error: EphemeralEnvError::UnwritableLocation,
            cleanup: CleanupOutcome::Failed(EphemeralEnvError::TeardownFailed),
        },
    ];

    for mut state in states {
        assert_eq!(state.signal_teardown(), None);
        assert!(!matches!(state, LifecycleState::Creating));
        assert!(!matches!(state, LifecycleState::Ready(_)));
    }
}

#[tokio::test]
async fn creation_outcome_cell_wait_observes_a_racing_set() {
    let cell = Arc::new(CreationOutcomeCell::new());
    let waiting_cell = Arc::clone(&cell);
    let waiter = tokio::spawn(async move { waiting_cell.wait().await });

    tokio::task::yield_now().await;
    cell.set(Ok(ready_environment("/tmp/ready")));

    assert!(waiter.await.unwrap().is_ok());
}

#[tokio::test]
async fn success_completion_publishes_outcome_before_teardown_transition() {
    let handle = EphemeralEnvironmentHandle::new_for_test(EnvironmentId::new());
    handle.signal_teardown();
    handle.complete_creation_success(ready_environment("/tmp/queued"));

    assert!(matches!(
        handle.lifecycle_state(),
        LifecycleState::TearingDown
    ));
    assert_eq!(
        handle.await_ready().await.unwrap().location,
        PathBuf::from("/tmp/queued")
    );
}

#[tokio::test]
async fn concurrent_signal_and_completion_never_lose_the_creation_outcome() {
    let handle = Arc::new(EphemeralEnvironmentHandle::new_for_test(
        EnvironmentId::new(),
    ));
    let barrier = Arc::new(Barrier::new(2));
    let signaling_handle = Arc::clone(&handle);
    let signaling_barrier = Arc::clone(&barrier);
    let signal = std::thread::spawn(move || {
        signaling_barrier.wait();
        signaling_handle.signal_teardown();
    });

    barrier.wait();
    handle.complete_creation_success(ready_environment("/tmp/concurrent"));
    signal.join().unwrap();

    assert_eq!(
        handle.await_ready().await.unwrap().location,
        PathBuf::from("/tmp/concurrent")
    );
}

#[tokio::test]
async fn signal_teardown_from_a_thread_without_a_runtime_uses_the_creation_runtime() {
    // Given
    let handle = Arc::new(EphemeralEnvironmentHandle::new_for_teardown_test(
        EnvironmentId::new(),
    ));
    handle.complete_creation_success(ready_environment("/tmp/cross-thread"));
    let signaling_handle = Arc::clone(&handle);

    // When
    let signal = std::thread::spawn(move || signaling_handle.signal_teardown());

    // Then
    assert!(signal.join().is_ok());
    assert_eq!(
        handle.await_torn_down().await,
        Err(EphemeralEnvError::TeardownFailed)
    );
}

#[tokio::test]
async fn creation_task_panic_completes_outcome_with_generic_error() {
    // Given
    let handle = EphemeralEnvironmentHandle::new_for_test(EnvironmentId::new());
    let failure_handle = handle.clone();

    // When
    super::spawn_lifecycle_task(
        &tokio::runtime::Handle::current(),
        "create",
        async { panic!("creation task panic") },
        move || {
            failure_handle.complete_creation_task_failure(EphemeralEnvError::UnwritableLocation);
        },
    );
    let failure = handle.await_ready().await.unwrap_err();

    // Then
    assert_eq!(failure.error, EphemeralEnvError::UnwritableLocation);
    assert_eq!(failure.cleanup_error, None);
}

#[tokio::test]
async fn blocking_task_panic_returns_fallback_error() {
    // Given
    let fallback = EphemeralEnvError::TeardownFailed;

    // When
    let outcome: Result<(), EphemeralEnvError> =
        super::run_blocking(fallback.clone(), || panic!("blocking task panic")).await;

    // Then
    assert_eq!(outcome, Err(fallback));
}

#[tokio::test]
async fn teardown_task_panic_completes_outcome_with_generic_error() {
    // Given
    let handle = EphemeralEnvironmentHandle::new_for_test(EnvironmentId::new());

    // When
    handle.start_panicking_teardown_for_test();

    // Then
    assert_eq!(
        handle.await_torn_down().await,
        Err(EphemeralEnvError::TeardownFailed)
    );
}

#[tokio::test]
async fn completed_creation_cleanup_publishes_both_failure_outcomes() {
    let handle = EphemeralEnvironmentHandle::new_for_test(EnvironmentId::new());
    handle.complete_creation_failure(EphemeralEnvError::UnwritableLocation);
    handle.complete_creation_cleanup(CleanupOutcome::Failed(EphemeralEnvError::TeardownFailed));

    assert_eq!(
        handle.await_ready().await.unwrap_err().cleanup_error,
        Some(EphemeralEnvError::TeardownFailed)
    );
}

#[test]
fn creation_failure_type_remains_the_cell_error() {
    let failure: CreationFailure = CreationFailure {
        error: EphemeralEnvError::UnwritableLocation,
        cleanup_error: None,
    };

    assert_eq!(failure.error, EphemeralEnvError::UnwritableLocation);
}
