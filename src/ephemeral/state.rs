use std::path::PathBuf;

use tokio::sync::{Notify, OnceCell};

use super::{
    error::{CreationFailure, EphemeralEnvError},
    lifecycle::ReadyEnvironment,
};

#[derive(Debug, Clone)]
pub(crate) enum TeardownOutcome {
    Succeeded,
    Failed(EphemeralEnvError),
}

#[derive(Debug, Clone)]
pub(crate) enum CleanupOutcome {
    Running,
    Succeeded,
    Failed(EphemeralEnvError),
}

#[derive(Debug, Clone)]
pub(crate) enum LifecycleState {
    Creating,
    CreatingTeardownQueued,
    Ready(ReadyEnvironment),
    TearingDown,
    TornDown(TeardownOutcome),
    CreationFailed {
        error: EphemeralEnvError,
        cleanup: CleanupOutcome,
    },
}

impl LifecycleState {
    pub(crate) fn signal_teardown(&mut self) -> Option<PathBuf> {
        match self {
            Self::Creating => {
                *self = Self::CreatingTeardownQueued;
                None
            }
            Self::Ready(environment) => {
                let location = environment.location.clone();
                *self = Self::TearingDown;
                Some(location)
            }
            Self::CreatingTeardownQueued
            | Self::TearingDown
            | Self::TornDown(_)
            | Self::CreationFailed { .. } => None,
        }
    }

    /// Returns the completed teardown result represented by this state.
    pub(crate) fn completed_teardown(&self) -> Option<Result<(), EphemeralEnvError>> {
        match self {
            Self::TornDown(TeardownOutcome::Succeeded)
            | Self::CreationFailed {
                cleanup: CleanupOutcome::Succeeded,
                ..
            } => Some(Ok(())),
            Self::TornDown(TeardownOutcome::Failed(error))
            | Self::CreationFailed {
                cleanup: CleanupOutcome::Failed(error),
                ..
            } => Some(Err(error.clone())),
            Self::Creating
            | Self::CreatingTeardownQueued
            | Self::Ready(_)
            | Self::TearingDown
            | Self::CreationFailed {
                cleanup: CleanupOutcome::Running,
                ..
            } => None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct CreationOutcomeCell {
    cell: OnceCell<Result<ReadyEnvironment, CreationFailure>>,
    notify: Notify,
}

impl CreationOutcomeCell {
    pub(crate) fn new() -> Self {
        Self {
            cell: OnceCell::new(),
            notify: Notify::new(),
        }
    }

    pub(crate) fn set(&self, outcome: Result<ReadyEnvironment, CreationFailure>) {
        let _ignored_duplicate = self.cell.set(outcome);
        self.notify.notify_waiters();
    }

    pub(crate) async fn wait(&self) -> Result<ReadyEnvironment, CreationFailure> {
        loop {
            if let Some(outcome) = self.cell.get() {
                return outcome.clone();
            }
            let notified = self.notify.notified();
            if let Some(outcome) = self.cell.get() {
                return outcome.clone();
            }
            notified.await;
        }
    }
}
