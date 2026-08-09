use crate::channel_config::{self, ChannelConfigResolution};
use crate::cli::PackagesAndCommandArgs;
use crate::cli::pass_through::{
    self, OneshotOutcomeEvent, PASS_THROUGH_EVENT_SCHEMA_VERSION, PassThroughExit,
    PassThroughFailure, emit_outcome_event,
};
use crate::default_packages_config;
use crate::ephemeral::{
    CreationFailure, EnvironmentId, EphemeralEnvError, PackageRequest,
    create_ephemeral_environment, parse_explicit_packages,
};
use crate::error::CategorizedError;
use crate::output;

/// The result of one `allez oneshot` invocation, once past usage-error
/// validation (already handled at the dispatch layer). `pub`, not
/// `pub(crate)`: `src/main.rs` is a separate binary crate that consumes
/// this library via `use allez::{cli, ...}` — `pub(crate)` would make this
/// type invisible to `dispatch`, this type's one real consumer.
pub enum OneshotOutcome {
    /// The environment could not be created (FR-010). No stored
    /// `exit_code` field: it is always `1` for this variant.
    EnvironmentCreationFailed {
        /// The already-rendered caller-facing message
        /// (`output::render_ephemeral_creation_failure`).
        message: String,
    },
    /// The pass-through program could not be started. Carries the
    /// classifying [`PassThroughFailure`] value — restricted by
    /// convention to its four pre-start variants only (`NotFound`,
    /// `NotExecutable`, `ActivationFailed`, `SignalSetupFailed`; see the
    /// construction rule below).
    PassThroughFailed {
        /// The already-rendered caller-facing message
        /// (`output::render_error`).
        message: String,
        /// The classifying failure.
        failure: PassThroughFailure,
    },
    /// The pass-through program started and terminated — normally
    /// (FR-006) or via a signal it did not survive (FR-007). No message
    /// in either case: FR-013 forbids wrapping the started command's own
    /// raw output/exit code in any envelope once it has started.
    PassThroughExited {
        /// The pass-through program's own propagated exit code.
        exit_code: i32,
    },
}

impl OneshotOutcome {
    /// The exit code for this outcome (FR-006).
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::EnvironmentCreationFailed { .. } => 1,
            Self::PassThroughFailed { failure, .. } => failure.exit_code(),
            Self::PassThroughExited { exit_code } => *exit_code,
        }
    }
}

/// Renders `failure`, emits its `OneshotOutcomeEvent`
/// (`pass_through_started: false`), and returns the resulting
/// [`OneshotOutcome::EnvironmentCreationFailed`]. Shared by every
/// environment-creation-failure path — the two synthesized ahead of
/// `create_ephemeral_environment` (an invalid package spec; zero usable
/// channels) and the real one that function itself returns — so all three
/// render/emit identically (Constitution IV).
fn environment_creation_failed(failure: CreationFailure, human: bool) -> OneshotOutcome {
    let message = output::render_ephemeral_creation_failure(&failure, human);
    emit_outcome_event(&OneshotOutcomeEvent {
        schema_version: PASS_THROUGH_EVENT_SCHEMA_VERSION,
        invocation_id: failure.id,
        pass_through_started: false,
        exit_code: None,
        failure_category: Some(failure.error.category()),
        message: Some(failure.error.to_string()),
        cleanup_category: failure
            .cleanup_error
            .as_ref()
            .map(CategorizedError::category),
        cleanup_message: failure.cleanup_error.as_ref().map(ToString::to_string),
    });
    OneshotOutcome::EnvironmentCreationFailed { message }
}

/// Shared by the `Signaled`/`WaitFailed` arms below: both are outcomes
/// the pass-through program reaches only after it already started, so
/// FR-013's construction rule requires routing them to
/// [`OneshotOutcome::PassThroughExited`] — never `PassThroughFailed` —
/// with `failure`'s category/message used only for this tracing event,
/// never for caller-facing rendering.
fn post_start_outcome(failure: PassThroughFailure, invocation_id: EnvironmentId) -> OneshotOutcome {
    let exit_code = failure.exit_code();
    emit_outcome_event(&OneshotOutcomeEvent {
        schema_version: PASS_THROUGH_EVENT_SCHEMA_VERSION,
        invocation_id,
        pass_through_started: true,
        exit_code: Some(exit_code),
        failure_category: Some(failure.category()),
        message: Some(failure.to_string()),
        cleanup_category: None,
        cleanup_message: None,
    });
    OneshotOutcome::PassThroughExited { exit_code }
}

/// This handler does not call `validate_pass_through()` itself — that
/// already happened at the dispatch layer, before this handler is ever
/// invoked. Orchestrates: parse per-invocation packages → read `.condarc`
/// once → resolve channels and the default package set from that one
/// document → create the environment → run the pass-through program —
/// translating every pre-start failure into a rendered `String` plus
/// [`OneshotOutcome`] the dispatch layer uses to pick
/// `std::process::exit`'s code. Never calls `std::process::exit`
/// directly, keeping it unit-testable.
pub async fn run(args: &PackagesAndCommandArgs, human: bool, _verbose: bool) -> OneshotOutcome {
    let explicit = match parse_explicit_packages(args.packages.clone()) {
        Ok(explicit) => explicit,
        Err(invalid) => {
            return environment_creation_failed(
                CreationFailure {
                    id: EnvironmentId::new(),
                    error: EphemeralEnvError::UnresolvablePackage {
                        package: invalid.index.map_or_else(
                            || "<invalid command-line package>".to_string(),
                            |index| format!("<command-line package {}>", index + 1),
                        ),
                    },
                    cleanup_error: None,
                },
                human,
            );
        }
    };

    let document = channel_config::resolve_document();

    let channels = match channel_config::channels_from_document(&document) {
        ChannelConfigResolution::Ready { config, .. } => config,
        ChannelConfigResolution::NoChannels => {
            return environment_creation_failed(
                CreationFailure {
                    id: EnvironmentId::new(),
                    error: EphemeralEnvError::NoChannelsConfigured,
                    cleanup_error: None,
                },
                human,
            );
        }
    };

    let defaults = default_packages_config::create_default_packages_from_document(&document);

    let request = PackageRequest { explicit, defaults };

    let environment = match create_ephemeral_environment(request, channels).await {
        Ok(environment) => environment,
        Err(failure) => return environment_creation_failed(failure, human),
    };

    match pass_through::run_pass_through(&environment, &args.pass_through).await {
        Ok(PassThroughExit::Normal { exit_code }) => {
            emit_outcome_event(&OneshotOutcomeEvent {
                schema_version: PASS_THROUGH_EVENT_SCHEMA_VERSION,
                invocation_id: environment.id,
                pass_through_started: true,
                exit_code: Some(exit_code),
                failure_category: None,
                message: None,
                cleanup_category: None,
                cleanup_message: None,
            });
            OneshotOutcome::PassThroughExited { exit_code }
        }
        Ok(PassThroughExit::Signaled { signal }) => post_start_outcome(
            PassThroughFailure::TerminatedBySignal { signal },
            environment.id,
        ),
        Ok(PassThroughExit::WaitFailed) => {
            post_start_outcome(PassThroughFailure::WaitFailed, environment.id)
        }
        Err(failure) => {
            let message = output::render_error(failure.category(), &failure.to_string(), human);
            emit_outcome_event(&OneshotOutcomeEvent {
                schema_version: PASS_THROUGH_EVENT_SCHEMA_VERSION,
                invocation_id: environment.id,
                pass_through_started: false,
                exit_code: None,
                failure_category: Some(failure.category()),
                message: Some(failure.to_string()),
                cleanup_category: None,
                cleanup_message: None,
            });
            OneshotOutcome::PassThroughFailed { message, failure }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_creation_failed_exit_code_is_one() {
        let outcome = OneshotOutcome::EnvironmentCreationFailed {
            message: "x".to_string(),
        };
        assert_eq!(outcome.exit_code(), 1);
    }

    #[test]
    fn pass_through_failed_delegates_to_not_found_exit_code() {
        let outcome = OneshotOutcome::PassThroughFailed {
            message: "x".to_string(),
            failure: PassThroughFailure::NotFound,
        };
        assert_eq!(outcome.exit_code(), 127);
    }

    #[test]
    fn pass_through_failed_delegates_to_not_executable_exit_code() {
        let outcome = OneshotOutcome::PassThroughFailed {
            message: "x".to_string(),
            failure: PassThroughFailure::NotExecutable,
        };
        assert_eq!(outcome.exit_code(), 126);
    }

    #[test]
    fn pass_through_exited_uses_its_own_exit_code() {
        let outcome = OneshotOutcome::PassThroughExited { exit_code: 37 };
        assert_eq!(outcome.exit_code(), 37);
    }

    #[test]
    fn environment_creation_failed_dual_failure_renders_both_categories_distinctly() {
        let failure = CreationFailure {
            id: EnvironmentId::new(),
            error: EphemeralEnvError::IntegrityVerificationFailed {
                package: "fixture-corrupt-checksum".to_string(),
            },
            cleanup_error: Some(EphemeralEnvError::TeardownFailed),
        };

        let outcome = environment_creation_failed(failure, false);

        let OneshotOutcome::EnvironmentCreationFailed { message } = &outcome else {
            panic!("expected EnvironmentCreationFailed");
        };
        let body: serde_json::Value = serde_json::from_str(message).expect("valid JSON");
        assert_eq!(body["category"], "integrity_verification_failed");
        assert_eq!(body["cleanup_category"], "teardown_failed");
        assert_ne!(body["category"], body["cleanup_category"]);
        assert_eq!(outcome.exit_code(), 1);
    }

    #[test]
    fn post_start_outcome_wait_failed_maps_to_exit_code_one() {
        let outcome = post_start_outcome(PassThroughFailure::WaitFailed, EnvironmentId::new());
        assert_eq!(outcome.exit_code(), 1);
    }

    #[test]
    fn post_start_outcome_terminated_by_signal_maps_to_128_plus_signal() {
        let outcome = post_start_outcome(
            PassThroughFailure::TerminatedBySignal { signal: 15 },
            EnvironmentId::new(),
        );
        assert_eq!(outcome.exit_code(), 143);
    }
}
