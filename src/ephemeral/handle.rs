use std::{
    future::Future,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use tokio::runtime::Handle;
use tokio::sync::Notify;

use super::{
    cleanup::CleanupGuard,
    defaults::PackageSpec,
    error::{CreationFailure, EphemeralEnvError},
    lifecycle::{EnvironmentId, ReadyEnvironment},
    orphan::OrphanReclamationOutcome,
    state::{CleanupOutcome, CreationOutcomeCell, LifecycleState, TeardownOutcome},
};

/// A handle for observing creation and requesting teardown of an environment.
#[derive(Clone)]
pub struct EphemeralEnvironmentHandle {
    id: EnvironmentId,
    lifecycle: Arc<Mutex<LifecycleState>>,
    creation_outcome: Arc<CreationOutcomeCell>,
    lifecycle_notify: Arc<Notify>,
    cleanup_guard: Arc<OnceLock<Arc<CleanupGuard>>>,
    effective_packages: Arc<OnceLock<Vec<PackageSpec>>>,
    reclamation_status: Arc<OnceLock<ReclamationStatus>>,
    runtime: Handle,
}

impl EphemeralEnvironmentHandle {
    pub(crate) fn new(id: EnvironmentId, runtime: Handle) -> Self {
        Self {
            id,
            lifecycle: Arc::new(Mutex::new(LifecycleState::Creating)),
            creation_outcome: Arc::new(CreationOutcomeCell::new()),
            lifecycle_notify: Arc::new(Notify::new()),
            cleanup_guard: Arc::new(OnceLock::new()),
            effective_packages: Arc::new(OnceLock::new()),
            reclamation_status: Arc::new(OnceLock::new()),
            runtime,
        }
    }

    pub(crate) fn set_effective_packages(&self, packages: Vec<PackageSpec>) {
        let _ignored_duplicate = self.effective_packages.set(packages);
    }

    pub(crate) fn set_cleanup_guard(&self, cleanup_guard: Arc<CleanupGuard>) {
        let _ignored_duplicate = self.cleanup_guard.set(cleanup_guard);
    }

    /// Stores the terminal result of this handle's automatic orphan scan.
    pub(crate) fn set_reclamation_status(&self, status: ReclamationStatus) {
        let _ignored_duplicate = self.reclamation_status.set(status);
    }

    /// Returns this handle's stable environment identifier.
    pub fn id(&self) -> EnvironmentId {
        self.id
    }

    /// Requests non-blocking teardown, folding duplicate requests into one attempt.
    pub fn signal_teardown(&self) {
        let should_start_removal = self.lock_lifecycle().signal_teardown().is_some();
        self.lifecycle_notify.notify_waiters();
        if should_start_removal {
            self.start_teardown();
        }
    }

    /// Waits for the immutable creation outcome.
    pub async fn await_ready(&self) -> Result<ReadyEnvironment, CreationFailure> {
        self.creation_outcome.wait().await
    }

    /// Waits until explicit teardown or creation-failure cleanup completes.
    pub async fn await_torn_down(&self) -> Result<(), EphemeralEnvError> {
        loop {
            if let Some(outcome) = self.lock_lifecycle().completed_teardown() {
                return outcome;
            }
            let notified = self.lifecycle_notify.notified();
            if let Some(outcome) = self.lock_lifecycle().completed_teardown() {
                return outcome;
            }
            notified.await;
        }
    }

    /// Returns the non-blocking status of this request's automatic orphan scan.
    pub fn reclamation_outcomes(&self) -> ReclamationStatus {
        self.reclamation_status
            .get()
            .cloned()
            .unwrap_or(ReclamationStatus::Scanning)
    }

    pub(crate) fn complete_creation_success(&self, environment: ReadyEnvironment) {
        let mut state = self.lock_lifecycle();
        let should_start_removal = match &*state {
            LifecycleState::Creating => {
                self.creation_outcome.set(Ok(environment.clone()));
                *state = LifecycleState::Ready(environment);
                false
            }
            LifecycleState::CreatingTeardownQueued => {
                self.creation_outcome.set(Ok(environment));
                *state = LifecycleState::TearingDown;
                true
            }
            LifecycleState::Ready(_)
            | LifecycleState::TearingDown
            | LifecycleState::TornDown(_)
            | LifecycleState::CreationFailed { .. } => false,
        };
        drop(state);
        self.lifecycle_notify.notify_waiters();
        if should_start_removal {
            self.start_teardown();
        }
    }

    pub(crate) fn complete_creation_failure(&self, error: EphemeralEnvError) {
        let mut state = self.lock_lifecycle();
        match &*state {
            LifecycleState::Creating | LifecycleState::CreatingTeardownQueued => {
                *state = LifecycleState::CreationFailed {
                    error,
                    cleanup: CleanupOutcome::Running,
                };
            }
            LifecycleState::Ready(_)
            | LifecycleState::TearingDown
            | LifecycleState::TornDown(_)
            | LifecycleState::CreationFailed { .. } => {}
        }
        drop(state);
        self.lifecycle_notify.notify_waiters();
    }

    pub(crate) fn complete_creation_cleanup(&self, cleanup_outcome: CleanupOutcome) {
        let mut state = self.lock_lifecycle();
        let LifecycleState::CreationFailed { error, cleanup } = &mut *state else {
            return;
        };
        if !matches!(cleanup, CleanupOutcome::Running) {
            return;
        }

        let failure = match &cleanup_outcome {
            CleanupOutcome::Running => return,
            CleanupOutcome::Succeeded => CreationFailure {
                error: error.clone(),
                cleanup_error: None,
            },
            CleanupOutcome::Failed(cleanup_error) => CreationFailure {
                error: error.clone(),
                cleanup_error: Some(cleanup_error.clone()),
            },
        };
        self.creation_outcome.set(Err(failure));
        *cleanup = cleanup_outcome;
        drop(state);
        self.lifecycle_notify.notify_waiters();
    }

    pub(crate) fn complete_creation_task_failure(&self, error: EphemeralEnvError) {
        self.complete_creation_failure(error);
        self.complete_creation_cleanup(CleanupOutcome::Succeeded);
    }

    fn start_teardown(&self) {
        let Some(cleanup_guard) = self.cleanup_guard.get().cloned() else {
            self.complete_teardown(Err(EphemeralEnvError::TeardownFailed));
            return;
        };
        if !cleanup_guard.claim_removal() {
            return;
        }
        let packages = self.effective_package_strings();
        self.spawn_teardown_task(async move {
            // Runs on Tokio's blocking thread pool via `super::run_blocking`,
            // not this task's own async worker thread: the actual removal
            // is a recursive, synchronous filesystem walk that could
            // otherwise stall every other task sharing that worker for its
            // whole duration.
            super::run_claimed_cleanup(cleanup_guard, packages).await
        });
    }

    fn spawn_teardown_task(
        &self,
        task: impl Future<Output = Result<(), EphemeralEnvError>> + Send + 'static,
    ) {
        let completion_handle = self.clone();
        let failure_handle = self.clone();
        super::spawn_lifecycle_task(
            &self.runtime,
            "teardown",
            async move { completion_handle.complete_teardown(task.await) },
            move || {
                failure_handle.complete_teardown(Err(EphemeralEnvError::TeardownFailed));
            },
        );
    }

    fn effective_package_strings(&self) -> Vec<String> {
        self.effective_packages
            .get()
            .map_or_else(Vec::new, |packages| {
                packages
                    .iter()
                    .map(|package| package.as_str().to_string())
                    .collect()
            })
    }

    fn complete_teardown(&self, outcome: Result<(), EphemeralEnvError>) {
        let mut state = self.lock_lifecycle();
        if !matches!(*state, LifecycleState::TearingDown) {
            return;
        }
        *state = LifecycleState::TornDown(match outcome {
            Ok(()) => TeardownOutcome::Succeeded,
            Err(error) => TeardownOutcome::Failed(error),
        });
        drop(state);
        self.lifecycle_notify.notify_waiters();
    }

    fn lock_lifecycle(&self) -> MutexGuard<'_, LifecycleState> {
        match self.lifecycle.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(id: EnvironmentId) -> Self {
        let handle = Self::new(id, Handle::current());
        handle.set_cleanup_guard(Arc::new(CleanupGuard::test_only()));
        handle
    }

    #[cfg(test)]
    pub(crate) fn new_for_teardown_test(id: EnvironmentId) -> Self {
        let handle = Self::new(id, Handle::current());
        handle.set_cleanup_guard(Arc::new(CleanupGuard::test_only_removable()));
        handle
    }

    #[cfg(test)]
    pub(crate) fn start_panicking_teardown_for_test(&self) {
        *self.lock_lifecycle() = LifecycleState::TearingDown;
        self.spawn_teardown_task(async { panic!("teardown task panic") });
    }

    #[cfg(test)]
    pub(crate) fn lifecycle_state(&self) -> LifecycleState {
        self.lock_lifecycle().clone()
    }
}

/// Progress and results from automatic orphan reclamation for one creation request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReclamationStatus {
    /// The automatic scan has not completed yet.
    Scanning,
    /// The scan completed with one outcome per inspected environment.
    Complete(Vec<OrphanReclamationOutcome>),
    /// The scan could not start.
    Failed(EphemeralEnvError),
}
