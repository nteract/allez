//! Ephemeral environment core (GEN-24): create and populate an unnamed,
//! caller-unpathed conda environment. See
//! `specs/GEN-24_ephemeral_env_core/` for the full spec/plan/contract.
//!
//! **There is no teardown.** An environment this module successfully
//! creates stays on disk, usable, indefinitely — not just for the
//! lifetime of the creating process, but past its normal exit or an
//! abrupt crash too — since there is no per-environment RAII cleanup
//! guard, no lock-file-based liveness tracking, no automatic orphan
//! reclamation, and no caller-invoked removal API. Only a failed creation
//! attempt is rolled back (see `fail_and_roll_back`).

use std::time::Instant;

use crate::error::CategorizedError;

mod channel_auth;
mod channels;
mod cleanup;
mod defaults;
mod error;
mod events;
mod install;
mod lifecycle;
mod paths;
mod permissions;
mod solve;

#[cfg(test)]
mod cleanup_tests;

pub use channels::redact_channel_url;
pub use defaults::{DEFAULT_PACKAGES, InvalidPackageSpec, PackageSpec, RequestedPackages};
pub use error::{ActivationError, CreationFailure, EphemeralEnvError};
pub use lifecycle::{EnvironmentId, InstalledPackage, ReadyEnvironment};

/// Test-only seam (feature-gated, see `Cargo.toml`'s `test-config-override`):
/// lets `tests/oneshot_exec.rs`'s harness pre-create an already-owner-only
/// `ALLEZ_EPHEMERAL_ROOT` directory on Windows before handing it to a real
/// `allez` invocation, mirroring the root-reuse scenarios it also exercises
/// on Unix (there, a plain `std::fs::set_permissions` narrows a
/// harness-created directory to `0700`). Windows has no such direct
/// chmod-equivalent for a directory `std::fs::create_dir` already made, so
/// this seam reuses [`permissions::create_directory_with_owner_only_acl`]
/// — the same routine `verified_root` itself calls — instead of
/// duplicating its Win32 SDDL/ACL logic in test code. A release build has
/// no code path that calls this at all.
#[cfg(all(windows, feature = "test-config-override"))]
pub fn test_create_owner_only_directory(path: &std::path::Path) -> Result<(), EphemeralEnvError> {
    permissions::create_directory_with_owner_only_acl(path)
}

use cleanup::remove_prefix_dir;
use events::{EPHEMERAL_EVENT_SCHEMA_VERSION, EphemeralLifecycleEvent, emit_event};
use paths::VerifiedRoot;

/// Creates a new ephemeral environment: a system-managed temporary/cache
/// location (no name or path supplied by the caller), populated with the
/// requested (or default/overridden) packages, resolved and installed
/// against `channels`. Resolves once creation finishes, one way or the
/// other (FR-001).
///
/// `requested` may be `RequestedPackages::Explicit(vec![])` — this is
/// treated identically to `UseDefaultOrOverride`.
///
/// The returned [`ReadyEnvironment`] is **not** torn down when it (or its
/// last clone) is dropped, and there is no way to signal teardown for a
/// single environment any more — see this module's own doc comment.
pub async fn create_ephemeral_environment(
    requested: RequestedPackages,
    channels: condarc::ResolvedChannels,
    default_override: Option<Vec<PackageSpec>>,
) -> Result<ReadyEnvironment, CreationFailure> {
    let started = Instant::now();
    let id = EnvironmentId::new();
    let effective_packages = defaults::effective_packages(&requested, default_override.as_deref());
    let packages: Vec<String> = effective_packages
        .iter()
        .map(|package| package.as_str().to_string())
        .collect();

    let root = match run_blocking(EphemeralEnvError::UnwritableLocation, paths::resolve_root).await
    {
        Ok(root) => root,
        Err(error) => {
            emit_failure(id, "create", started, &packages, &error);
            return Err(CreationFailure {
                id,
                error,
                cleanup_error: None,
            });
        }
    };

    let root = match run_blocking(EphemeralEnvError::UnwritableLocation, move || {
        permissions::create_environment_directory(&root, id)?;
        Ok(root)
    })
    .await
    {
        Ok(root) => root,
        Err(error) => {
            emit_failure(id, "create", started, &packages, &error);
            return Err(CreationFailure {
                id,
                error,
                cleanup_error: None,
            });
        }
    };

    let solve_started = Instant::now();
    let solution = match solve::solve_packages(&root, &channels, &effective_packages).await {
        Ok(solution) => solution,
        Err(error) => {
            emit_failure(id, "install", solve_started, &packages, &error);
            return Err(fail_and_roll_back(root, id, started, &packages, error).await);
        }
    };

    let prefix = root.path().join("envs").join(id.to_string());
    let install_started = Instant::now();
    let installed_packages = match install::install_packages(&root, &prefix, solution).await {
        Ok(installed_packages) => {
            emit_success(id, "install", install_started, &packages);
            installed_packages
        }
        Err(error) => {
            emit_failure(id, "install", install_started, &packages, &error);
            return Err(fail_and_roll_back(root, id, started, &packages, error).await);
        }
    };

    emit_success(id, "create", started, &packages);
    Ok(ReadyEnvironment::new(id, prefix, installed_packages))
}

/// Reports a solve/install failure and rolls back whatever directory
/// `create_environment_directory` created for this attempt — the closed
/// question of whether that rollback itself succeeded is reported
/// alongside the original failure (FR-010), never masking it and never
/// silently dropped.
async fn fail_and_roll_back(
    root: VerifiedRoot,
    id: EnvironmentId,
    started: Instant,
    packages: &[String],
    error: EphemeralEnvError,
) -> CreationFailure {
    emit_failure(id, "create", started, packages, &error);
    let cleanup_started = Instant::now();
    let cleanup_result = run_blocking(EphemeralEnvError::TeardownFailed, move || {
        remove_prefix_dir(&root, id)
    })
    .await;
    match cleanup_result {
        Ok(()) => {
            emit_success(id, "teardown", cleanup_started, packages);
            CreationFailure {
                id,
                error,
                cleanup_error: None,
            }
        }
        Err(cleanup_error) => {
            emit_failure(id, "teardown", cleanup_started, packages, &cleanup_error);
            CreationFailure {
                id,
                error,
                cleanup_error: Some(cleanup_error),
            }
        }
    }
}

/// Shared by [`emit_success`]/[`emit_failure`]: both construct an
/// identical `EphemeralLifecycleEvent`, differing only in `outcome`/`failure_category`.
fn emit_outcome(
    id: EnvironmentId,
    operation: &'static str,
    started: Instant,
    packages: &[String],
    outcome: &'static str,
    failure_category: Option<&'static str>,
) {
    emit_event(&EphemeralLifecycleEvent {
        schema_version: EPHEMERAL_EVENT_SCHEMA_VERSION,
        environment_id: id,
        operation,
        packages: packages.to_vec(),
        duration_ms: elapsed_milliseconds(started),
        outcome,
        failure_category,
    });
}

fn emit_success(id: EnvironmentId, operation: &'static str, started: Instant, packages: &[String]) {
    emit_outcome(id, operation, started, packages, "success", None);
}

fn emit_failure(
    id: EnvironmentId,
    operation: &'static str,
    started: Instant,
    packages: &[String],
    error: &EphemeralEnvError,
) {
    emit_outcome(
        id,
        operation,
        started,
        packages,
        "failure",
        Some(CategorizedError::category(error)),
    );
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
