//! Ephemeral environment core (GEN-24): create, populate, and tear down an
//! unnamed, caller-unpathed conda environment. See
//! `specs/GEN-24_ephemeral_env_core/` for the full spec/plan/contract.

use std::{future::Future, sync::Arc, time::Instant};

use crate::error::CategorizedError;

mod channels;
mod cleanup;
mod defaults;
mod error;
mod events;
mod handle;
mod install;
mod lifecycle;
mod orphan;
mod orphan_files;
mod paths;
mod permissions;
mod solve;
mod state;

#[cfg(test)]
mod cleanup_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod orphan_tests;

pub use channels::{ChannelConfig, ChannelPriorityMode, ChannelSpec};
pub use defaults::{DEFAULT_PACKAGES, InvalidPackageSpec, PackageSpec, RequestedPackages};
pub use error::{ActivationError, CreationFailure, EphemeralEnvError};
pub use lifecycle::{
    EnvironmentId, EphemeralEnvironmentHandle, InstalledPackage, ReadyEnvironment,
    ReclamationStatus,
};
pub use orphan::OrphanReclamationOutcome;

use cleanup::{CleanupGuard, remove_prefix_dir};
use events::{EPHEMERAL_EVENT_SCHEMA_VERSION, EphemeralLifecycleEvent, emit_event};
use lifecycle::CleanupOutcome;

/// Requests creation of a new ephemeral environment. Returns immediately
/// with a handle usable to signal teardown right away (FR-001) — creation
/// itself proceeds asynchronously (spawned onto the Tokio runtime this
/// function requires to already be running, matching the rest of the
/// crate's async boundary).
///
/// `requested` may be `RequestedPackages::Explicit(vec![])` — this is
/// treated identically to `UseDefaultOrOverride`.
pub fn create_ephemeral_environment(
    requested: RequestedPackages,
    channels: ChannelConfig,
    default_override: Option<Vec<PackageSpec>>,
) -> EphemeralEnvironmentHandle {
    let id = EnvironmentId::new();
    let runtime = tokio::runtime::Handle::current();
    let handle = EphemeralEnvironmentHandle::new(id, runtime.clone());
    let creation_handle = handle.clone();
    let failure_handle = handle.clone();
    let creation_task = async move {
        let started = Instant::now();
        let root = run_blocking(EphemeralEnvError::UnwritableLocation, paths::resolve_root)
            .await
            .map(Arc::new);
        let reclamation_status = match &root {
            Ok(root) => {
                let root = Arc::clone(root);
                match run_blocking(EphemeralEnvError::UnwritableLocation, move || {
                    orphan::reclaim_orphaned_environments(&root)
                })
                .await
                {
                    Ok(outcomes) => ReclamationStatus::Complete(outcomes),
                    Err(error) => ReclamationStatus::Failed(error),
                }
            }
            Err(error) => ReclamationStatus::Failed(error.clone()),
        };
        creation_handle.set_reclamation_status(reclamation_status);
        let effective_packages =
            defaults::effective_packages(&requested, default_override.as_deref());
        creation_handle.set_effective_packages(effective_packages.clone());
        let attempt = CreationAttempt {
            handle: creation_handle,
            id,
            packages: effective_packages
                .iter()
                .map(|package| package.as_str().to_string())
                .collect(),
            started,
        };
        let root = match root {
            Ok(root) => root,
            Err(error) => {
                attempt.finish_failure_without_prefix(error);
                return;
            }
        };
        let published = {
            let root = Arc::clone(&root);
            let packages = effective_packages.clone();
            run_blocking(
                orphan::PublicationFailure {
                    error: EphemeralEnvError::UnwritableLocation,
                    cleanup: None,
                },
                move || orphan::publish_environment(&root, id, &packages),
            )
            .await
        };
        let published = match published {
            Ok(published) => published,
            Err(failure) => {
                attempt.finish_publication_failure(failure);
                return;
            }
        };
        let cleanup_guard = Arc::new(CleanupGuard::new(
            Arc::clone(&root),
            id,
            published.owner_lock,
            attempt.packages.clone(),
        ));
        attempt.handle.set_cleanup_guard(Arc::clone(&cleanup_guard));

        let solve_started = Instant::now();
        let solution = match solve::solve_packages(&root, &channels, &effective_packages).await {
            Ok(solution) => solution,
            Err(error) => {
                attempt.emit_failure("install", solve_started, &error);
                attempt.finish_failure(root, &cleanup_guard, error).await;
                return;
            }
        };
        let prefix = root.path().join("envs").join(id.to_string());
        let install_started = Instant::now();
        let installed_packages = match install::install_packages(&root, &prefix, solution).await {
            Ok(installed_packages) => {
                attempt.emit_success("install", install_started);
                installed_packages
            }
            Err(error) => {
                attempt.emit_failure("install", install_started, &error);
                attempt.finish_failure(root, &cleanup_guard, error).await;
                return;
            }
        };
        let ready = ReadyEnvironment::new(id, prefix, installed_packages, cleanup_guard);
        attempt.emit_success("create", attempt.started);
        attempt.handle.complete_creation_success(ready);
    };
    spawn_lifecycle_task(&runtime, "create", creation_task, move || {
        failure_handle.complete_creation_task_failure(EphemeralEnvError::UnwritableLocation);
    });

    handle
}

pub(crate) fn spawn_lifecycle_task(
    runtime: &tokio::runtime::Handle,
    operation: &'static str,
    task: impl Future<Output = ()> + Send + 'static,
    on_join_failure: impl FnOnce() + Send + 'static,
) {
    let lifecycle_task = runtime.spawn(task);
    drop(runtime.spawn(async move {
        if let Err(join_error) = lifecycle_task.await {
            tracing::error!(
                operation,
                panicked = join_error.is_panic(),
                cancelled = join_error.is_cancelled(),
                "ephemeral environment lifecycle task did not complete normally"
            );
            on_join_failure();
        }
    }));
}

/// Scans for and removes/reports any ephemeral environment left behind by
/// a prior, no-longer-running process for the same local user account and
/// `allez` installation (FR-008/SC-003).
///
/// # Errors
///
/// Returns [`EphemeralEnvError::UnwritableLocation`] if the root or its
/// reclamation lock cannot be securely opened or acquired.
pub fn reclaim_orphaned_environments() -> Result<Vec<OrphanReclamationOutcome>, EphemeralEnvError> {
    let root = paths::resolve_root()?;
    orphan::reclaim_orphaned_environments(&root)
}

struct CreationAttempt {
    handle: EphemeralEnvironmentHandle,
    id: EnvironmentId,
    packages: Vec<String>,
    started: Instant,
}

impl CreationAttempt {
    fn finish_failure_without_prefix(&self, error: EphemeralEnvError) {
        self.handle.complete_creation_failure(error.clone());
        self.emit_failure("create", self.started, &error);
        self.handle
            .complete_creation_cleanup(CleanupOutcome::Succeeded);
    }

    /// Reports a publication failure (`orphan::publish_environment`), whose
    /// own internal, best-effort removal attempt -- present only when a
    /// directory was actually created before something afterward failed --
    /// is this attempt's sole cleanup step; this never calls
    /// `remove_prefix_dir` a second time itself, closing a real double-removal
    /// / fabricated-teardown-event hazard a review found: a second attempt
    /// on an already-removed (or never-created) directory would otherwise
    /// report a spurious `TeardownFailed`.
    fn finish_publication_failure(&self, failure: orphan::PublicationFailure) {
        self.handle.complete_creation_failure(failure.error.clone());
        self.emit_failure("create", self.started, &failure.error);
        let Some(cleanup) = failure.cleanup else {
            self.handle
                .complete_creation_cleanup(CleanupOutcome::Succeeded);
            return;
        };
        match cleanup.outcome {
            Ok(()) => {
                self.emit_success_after("teardown", cleanup.duration_ms);
                self.handle
                    .complete_creation_cleanup(CleanupOutcome::Succeeded);
            }
            Err(cleanup_error) => {
                self.emit_failure_after("teardown", cleanup.duration_ms, &cleanup_error);
                self.handle
                    .complete_creation_cleanup(CleanupOutcome::Failed(cleanup_error));
            }
        }
    }

    async fn finish_failure(
        &self,
        root: Arc<paths::VerifiedRoot>,
        cleanup_guard: &CleanupGuard,
        error: EphemeralEnvError,
    ) {
        self.handle.complete_creation_failure(error.clone());
        self.emit_failure("create", self.started, &error);
        let _claimed = cleanup_guard.claim_removal();
        let cleanup_started = Instant::now();
        let id = self.id;
        match run_blocking(EphemeralEnvError::TeardownFailed, move || {
            remove_prefix_dir(&root, id)
        })
        .await
        {
            Ok(()) => {
                self.handle
                    .complete_creation_cleanup(CleanupOutcome::Succeeded);
                self.emit_success("teardown", cleanup_started);
            }
            Err(cleanup_error) => {
                self.handle
                    .complete_creation_cleanup(CleanupOutcome::Failed(cleanup_error.clone()));
                self.emit_failure("teardown", cleanup_started, &cleanup_error);
            }
        }
    }

    fn emit_success(&self, operation: &'static str, started: Instant) {
        self.emit_success_after(operation, elapsed_milliseconds(started));
    }

    fn emit_failure(&self, operation: &'static str, started: Instant, error: &EphemeralEnvError) {
        self.emit_failure_after(operation, elapsed_milliseconds(started), error);
    }

    fn emit_success_after(&self, operation: &'static str, duration_ms: u64) {
        emit_event(&EphemeralLifecycleEvent {
            schema_version: EPHEMERAL_EVENT_SCHEMA_VERSION,
            environment_id: self.id,
            operation,
            packages: self.packages.clone(),
            duration_ms,
            outcome: "success",
            failure_category: None,
        });
    }

    fn emit_failure_after(
        &self,
        operation: &'static str,
        duration_ms: u64,
        error: &EphemeralEnvError,
    ) {
        emit_event(&EphemeralLifecycleEvent {
            schema_version: EPHEMERAL_EVENT_SCHEMA_VERSION,
            environment_id: self.id,
            operation,
            packages: self.packages.clone(),
            duration_ms,
            outcome: "failure",
            failure_category: Some(error.category()),
        });
    }
}

pub(crate) async fn run_claimed_cleanup(
    cleanup_guard: Arc<CleanupGuard>,
    packages: Vec<String>,
) -> Result<(), EphemeralEnvError> {
    let removal_guard = Arc::clone(&cleanup_guard);
    let reported_outcome = run_blocking(EphemeralEnvError::TeardownFailed, move || {
        removal_guard.remove_claimed(packages)
    })
    .await;
    cleanup_guard.removal_outcome().unwrap_or(reported_outcome)
}

fn elapsed_milliseconds(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

/// Runs synchronous filesystem work on Tokio's blocking thread pool.
/// Returns `fallback` if the blocking task panics or is cancelled.
pub(crate) async fn run_blocking<T, E>(
    fallback: E,
    task: impl FnOnce() -> Result<T, E> + Send + 'static,
) -> Result<T, E>
where
    T: Send + 'static,
    E: Send + 'static,
{
    match tokio::task::spawn_blocking(task).await {
        Ok(outcome) => outcome,
        Err(join_error) => {
            tracing::error!(
                panicked = join_error.is_panic(),
                cancelled = join_error.is_cancelled(),
                "ephemeral environment blocking filesystem task did not complete normally"
            );
            Err(fallback)
        }
    }
}
