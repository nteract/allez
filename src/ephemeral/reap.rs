//! Explicit, caller-invoked removal of every ephemeral environment for the
//! current local user account and `allez` installation.
//!
//! There is no automatic teardown, no orphan detection, and no
//! per-environment liveness tracking any more — see the GEN-24 spec's
//! "Explicit reap, no automatic reaping" decision. An environment
//! [`super::create_ephemeral_environment`] successfully creates stays on
//! disk, usable, until a caller explicitly calls
//! [`super::reap_ephemeral_environments`]; that call removes every
//! environment it finds under the root's `envs` directory
//! unconditionally — it does not attempt to determine whether one is
//! still in use elsewhere. Callers are responsible for only reaping when
//! doing so is safe (e.g. no other concurrent `allez` invocation still
//! needs a live environment).

use std::time::Instant;

use super::{
    cleanup::remove_prefix_dir,
    error::EphemeralEnvError,
    events::{EPHEMERAL_EVENT_SCHEMA_VERSION, EphemeralLifecycleEvent, emit_event},
    lifecycle::EnvironmentId,
    paths::VerifiedRoot,
};

#[cfg(unix)]
use rustix::fs::Dir;

/// One environment's outcome from a [`super::reap_ephemeral_environments`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReapOutcome {
    /// The environment's directory was removed.
    Removed {
        /// The removed environment.
        id: EnvironmentId,
    },
    /// The environment's directory could not be removed.
    RemovalFailed {
        /// The environment that could not be removed.
        id: EnvironmentId,
        /// The removal failure category.
        error: EphemeralEnvError,
    },
}

/// Removes every ephemeral environment found under `root`'s `envs`
/// directory, unconditionally — see this module's own doc comment.
pub(crate) fn reap_all(root: &VerifiedRoot) -> Result<Vec<ReapOutcome>, EphemeralEnvError> {
    let mut outcomes = Vec::new();
    for id in environment_ids(root)? {
        let started = Instant::now();
        match remove_prefix_dir(root, id) {
            Ok(()) => {
                emit_reap_event(id, started, None);
                outcomes.push(ReapOutcome::Removed { id });
            }
            Err(error) => {
                emit_reap_event(id, started, Some(&error));
                outcomes.push(ReapOutcome::RemovalFailed { id, error });
            }
        }
    }
    Ok(outcomes)
}

fn emit_reap_event(id: EnvironmentId, started: Instant, error: Option<&EphemeralEnvError>) {
    let (outcome, failure_category) = match error {
        Some(error) => (
            "failure",
            Some(crate::error::CategorizedError::category(error)),
        ),
        None => ("success", None),
    };
    emit_event(&EphemeralLifecycleEvent {
        schema_version: EPHEMERAL_EVENT_SCHEMA_VERSION,
        environment_id: id,
        operation: "teardown",
        // Unlike a create/install event, reaping doesn't know what
        // packages were installed -- there is no metadata file to read it
        // back from any more (that tracking is exactly the complexity
        // this revision removes). An empty list here is a deliberate
        // simplification, not an oversight.
        packages: Vec::new(),
        duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        outcome,
        failure_category,
    });
}

/// Every `envs/` entry whose name parses as an [`EnvironmentId`],
/// regardless of its own file type — [`remove_prefix_dir`] itself is what
/// rejects a non-directory or wrong-owner entry (reported as
/// [`ReapOutcome::RemovalFailed`]), so this listing step doesn't need its
/// own separate type-classification logic the way orphan reclamation used
/// to.
#[cfg(unix)]
fn environment_ids(root: &VerifiedRoot) -> Result<Vec<EnvironmentId>, EphemeralEnvError> {
    let mut ids = Vec::new();
    let mut entries =
        Dir::read_from(root.envs_dir()).map_err(|_| EphemeralEnvError::UnwritableLocation)?;
    for entry in &mut entries {
        let entry = entry.map_err(|_| EphemeralEnvError::UnwritableLocation)?;
        let Ok(value) = entry.file_name().to_str() else {
            continue;
        };
        if let Some(id) = EnvironmentId::parse(value) {
            ids.push(id);
        }
    }
    Ok(ids)
}

#[cfg(not(unix))]
fn environment_ids(root: &VerifiedRoot) -> Result<Vec<EnvironmentId>, EphemeralEnvError> {
    let entries = std::fs::read_dir(root.path().join("envs"))
        .map_err(|_| EphemeralEnvError::UnwritableLocation)?;
    let mut ids = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| EphemeralEnvError::UnwritableLocation)?;
        if let Some(id) = entry.file_name().to_str().and_then(EnvironmentId::parse) {
            ids.push(id);
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::{ReapOutcome, reap_all};
    use crate::ephemeral::{EnvironmentId, paths::verified_root, permissions};

    #[test]
    fn reap_all_removes_every_environment_directory() {
        // Given
        let temp = tempfile::tempdir().unwrap();
        let root = verified_root(&temp.path().join("root")).unwrap();
        let first = EnvironmentId::new();
        let second = EnvironmentId::new();
        permissions::create_environment_directory(&root, first).unwrap();
        permissions::create_environment_directory(&root, second).unwrap();

        // When
        let outcomes = reap_all(&root).unwrap();

        // Then
        assert_eq!(outcomes.len(), 2);
        assert!(
            outcomes
                .iter()
                .all(|outcome| matches!(outcome, ReapOutcome::Removed { .. }))
        );
        assert!(!root.path().join("envs").join(first.to_string()).exists());
        assert!(!root.path().join("envs").join(second.to_string()).exists());
    }

    #[test]
    fn reap_all_on_an_empty_root_returns_no_outcomes() {
        // Given
        let temp = tempfile::tempdir().unwrap();
        let root = verified_root(&temp.path().join("root")).unwrap();

        // When / Then
        assert!(reap_all(&root).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn reap_all_reports_removal_failure_without_touching_other_environments() {
        // Given
        let temp = tempfile::tempdir().unwrap();
        let root = verified_root(&temp.path().join("root")).unwrap();
        let removable = EnvironmentId::new();
        let stuck = EnvironmentId::new();
        permissions::create_environment_directory(&root, removable).unwrap();
        // A plain file (not a directory) with a ULID-parseable name:
        // `remove_prefix_dir` rejects it (it isn't a directory) without
        // needing to touch anything else under `envs/`, unlike a
        // permissions-based failure injection which would affect every
        // sibling entry too.
        std::fs::write(
            root.path().join("envs").join(stuck.to_string()),
            b"not a directory",
        )
        .unwrap();

        // When
        let outcomes = reap_all(&root).unwrap();

        // Then
        assert!(
            outcomes
                .iter()
                .any(|outcome| matches!(outcome, ReapOutcome::Removed { id } if *id == removable))
        );
        assert!(outcomes.iter().any(
            |outcome| matches!(outcome, ReapOutcome::RemovalFailed { id, .. } if *id == stuck)
        ));
        assert!(
            !root
                .path()
                .join("envs")
                .join(removable.to_string())
                .exists()
        );
    }
}
