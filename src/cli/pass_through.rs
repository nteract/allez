//! `allez oneshot`'s process-lifecycle scope: building the pass-through
//! program's `tokio::process::Command`, registering and forwarding
//! termination signals, and classifying its outcome once it starts or
//! fails to start.

use std::fmt;
use std::io;

use tokio::process::{Child, Command};

use super::PassThroughArgs;
use crate::ephemeral::{EnvironmentId, ReadyEnvironment};
use crate::error::CategorizedError;

/// The fixed category set for pass-through-command failures that occur
/// after the environment is already ready, but before or during running
/// the pass-through program — distinct from
/// [`crate::ephemeral::EphemeralEnvError`] (an environment-creation
/// failure) and mutually exclusive with it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassThroughFailure {
    /// The pass-through program's name could not be found (FR-008).
    NotFound,
    /// The pass-through program was found but could not be executed
    /// (FR-008) — also the fallback classification for any spawn-time
    /// `io::Error` kind FR-008 does not separately name.
    NotExecutable,
    /// The pass-through program was terminated by a signal it does not
    /// survive, on a platform where that concept exists (FR-007). Not a
    /// pre-start failure: the program already started and ran; see
    /// `OneshotOutcome`'s construction rule (`src/cli/oneshot.rs`) for why
    /// this variant is used only for the `OneshotOutcomeEvent` tracing
    /// record, never for caller-facing rendering.
    TerminatedBySignal {
        /// The raw signal number that terminated the pass-through program.
        signal: i32,
    },
    /// Computing the environment's own activation variables failed after
    /// the environment was already successfully created. Carries no
    /// field: the underlying `ActivationError.message` is `rattler_shell`'s
    /// own raw error text and can plausibly include the environment's own
    /// filesystem path, so it is never forwarded into caller-facing output
    /// or the `OneshotOutcomeEvent` tracing record (FR-015).
    ActivationFailed,
    /// Registering a `tokio::signal::unix`/`tokio::signal::windows`
    /// listener itself failed — a rare, OS-resource-level failure, not a
    /// program- or environment-related one. Occurs after the environment
    /// is ready but strictly before `.spawn()`, so it is a pre-start
    /// failure.
    SignalSetupFailed,
    /// `Child::wait()` itself returned `Err(io::Error)` — an
    /// OS-resource-level failure distinct from every other variant here:
    /// the program already started, so this is not a pre-start failure,
    /// but its real termination outcome could not be observed. Like
    /// `TerminatedBySignal`, used only for the `OneshotOutcomeEvent`
    /// tracing record, never for caller-facing rendering (FR-013).
    WaitFailed,
}

impl fmt::Display for PassThroughFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(formatter, "pass-through program not found"),
            Self::NotExecutable => {
                write!(
                    formatter,
                    "pass-through program found but could not be executed"
                )
            }
            Self::TerminatedBySignal { signal } => {
                write!(
                    formatter,
                    "pass-through program terminated by signal {signal}"
                )
            }
            Self::ActivationFailed => {
                write!(
                    formatter,
                    "failed to prepare the created environment for use"
                )
            }
            Self::SignalSetupFailed => {
                write!(
                    formatter,
                    "failed to register a termination-signal listener"
                )
            }
            Self::WaitFailed => {
                write!(
                    formatter,
                    "failed to observe the pass-through program's own termination"
                )
            }
        }
    }
}

impl std::error::Error for PassThroughFailure {}

impl CategorizedError for PassThroughFailure {
    fn category(&self) -> &'static str {
        match self {
            Self::NotFound => "pass_through_not_found",
            Self::NotExecutable => "pass_through_not_executable",
            Self::TerminatedBySignal { .. } => "pass_through_terminated_by_signal",
            Self::ActivationFailed => "activation_failed",
            Self::SignalSetupFailed => "signal_setup_failed",
            Self::WaitFailed => "pass_through_wait_failed",
        }
    }
}

impl PassThroughFailure {
    /// The exit code this failure maps to (FR-006/FR-007/FR-008).
    /// `TerminatedBySignal`'s code is computed, not fixed, so it is a
    /// method rather than a `const` table. `ActivationFailed` and
    /// `SignalSetupFailed` are their own match arms returning `1`, not
    /// folded into `NotExecutable`'s `126` arm — the categories map to
    /// different exit codes.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::NotFound => 127,
            Self::NotExecutable => 126,
            Self::ActivationFailed | Self::SignalSetupFailed | Self::WaitFailed => 1,
            Self::TerminatedBySignal { signal } => 128 + signal,
        }
    }
}

/// The pass-through program's outcome once it has actually started
/// running — distinct from the four pre-start [`PassThroughFailure`]
/// variants, which never reach this type. `Signaled` still carries the
/// raw signal number (rather than the already-computed `128 + signal`
/// exit code) so the caller can also build a
/// [`PassThroughFailure::TerminatedBySignal`] for the `OneshotOutcomeEvent`
/// tracing record (FR-012) without recomputing it.
pub(crate) enum PassThroughExit {
    /// The program exited normally with this code (FR-006).
    Normal {
        /// The pass-through program's own exit code.
        exit_code: i32,
    },
    /// The program was terminated by a signal it did not survive (FR-007,
    /// Unix only — `classify_exit_status` never produces this on Windows).
    Signaled {
        /// The raw signal number that terminated the program.
        signal: i32,
    },
    /// `Child::wait()` itself failed after the program had already
    /// started; see [`PassThroughFailure::WaitFailed`].
    WaitFailed,
}

/// Builds the pass-through program's `Command`: applies
/// [`ReadyEnvironment::activation_environment`]'s overlay on top of
/// `Command`'s own default (inherit the parent's full environment) —
/// never `.env_clear()` — satisfying FR-003's "merge, not replace".
fn build_command(
    environment: &ReadyEnvironment,
    pass_through: &PassThroughArgs,
) -> Result<Command, PassThroughFailure> {
    // Invariant: `validate_pass_through()` already guaranteed `Some` at the
    // dispatch layer before `oneshot::run` (this function's only caller's
    // caller) is ever invoked; this explicit fallback replaces what would
    // otherwise be an `.unwrap()` (Constitution V).
    let Some(program) = pass_through.program() else {
        return Err(PassThroughFailure::NotFound);
    };
    let overlay = environment
        .activation_environment()
        .map_err(|_| PassThroughFailure::ActivationFailed)?;

    let mut command = Command::new(program);
    command.args(pass_through.args());
    for (key, value) in overlay {
        command.env(key, value);
    }
    #[cfg(windows)]
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP);
    Ok(command)
}

/// Classifies `Command::spawn()`'s `Err(io::Error)` by `.kind()` (FR-008):
/// `NotFound` is the one kind every platform reliably reports for "no such
/// program on `PATH`"; every other kind (including `PermissionDenied`)
/// folds into `NotExecutable`.
fn classify_spawn_error(error: &io::Error) -> PassThroughFailure {
    match error.kind() {
        io::ErrorKind::NotFound => PassThroughFailure::NotFound,
        _ => PassThroughFailure::NotExecutable,
    }
}

/// Classifies a resolved `ExitStatus` into a normal exit or (Unix only) a
/// signal-terminated one (research.md's "Exit-code classification"
/// decision).
fn classify_exit_status(status: std::process::ExitStatus) -> PassThroughExit {
    match status.code() {
        Some(exit_code) => PassThroughExit::Normal { exit_code },
        None => {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                match status.signal() {
                    Some(signal) => PassThroughExit::Signaled { signal },
                    // `code()` returning `None` on Unix is documented to
                    // mean the process was signal-terminated, so
                    // `signal()` also returning `None` here would mean
                    // that documented contract was violated by the
                    // platform. An explicit, named fallback rather than
                    // `.unwrap()`/`.expect()`/`.unwrap_or()`
                    // (Constitution V).
                    None => PassThroughExit::Normal { exit_code: 128 },
                }
            }
            #[cfg(windows)]
            {
                unreachable!("Windows ExitStatus::code() is documented to always return Some")
            }
        }
    }
}

/// Classifies `Child::wait()`'s own outcome: `Err` becomes
/// [`PassThroughExit::WaitFailed`] (an explicit, named case rather than
/// `.unwrap()`/`.expect()`/`.unwrap_or()` — Constitution V) instead of
/// being silently folded into a normal exit.
fn classify_wait_result(result: io::Result<std::process::ExitStatus>) -> PassThroughExit {
    match result {
        Ok(status) => classify_exit_status(status),
        Err(_) => PassThroughExit::WaitFailed,
    }
}

#[cfg(unix)]
struct SignalListeners {
    interrupt: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
    hangup: tokio::signal::unix::Signal,
    quit: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl SignalListeners {
    /// Registers every listener *before* the pass-through program is ever
    /// spawned (research.md): a signal arriving in the gap between spawn
    /// and listener registration would otherwise have no forwarder alive
    /// to relay it.
    fn register() -> io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Self {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
            hangup: signal(SignalKind::hangup())?,
            quit: signal(SignalKind::quit())?,
        })
    }

    /// Forwards every intercepted termination signal to `child`'s own PID
    /// only (never its process group — research.md's forwarding-scope
    /// decision), then keeps waiting for its real outcome: `allez` itself
    /// never exits early on receiving a signal (FR-014).
    async fn wait_with_forwarding(mut self, child: &mut Child) -> PassThroughExit {
        use tokio::signal::unix::SignalKind;
        loop {
            tokio::select! {
                result = child.wait() => {
                    return classify_wait_result(result);
                }
                Some(()) = self.interrupt.recv() => forward_signal(child, SignalKind::interrupt()),
                Some(()) = self.terminate.recv() => forward_signal(child, SignalKind::terminate()),
                Some(()) = self.hangup.recv() => forward_signal(child, SignalKind::hangup()),
                Some(()) = self.quit.recv() => forward_signal(child, SignalKind::quit()),
            }
        }
    }
}

/// Sends `kind`'s underlying signal to `child`'s own PID via `kill(2)`
/// (`rustix::process::kill_process`, already a direct dependency). A
/// failure here (e.g. the child already exited between the signal
/// arriving and this call running) is deliberately ignored: the
/// `tokio::select!` loop's own next iteration observes the child's real
/// outcome regardless.
#[cfg(unix)]
fn forward_signal(child: &Child, kind: tokio::signal::unix::SignalKind) {
    let Some(pid) = child.id() else { return };
    let Some(pid) = rustix::process::Pid::from_raw(pid as i32) else {
        return;
    };
    let Some(signal) = rustix::process::Signal::from_named_raw(kind.as_raw_value()) else {
        return;
    };
    let _ = rustix::process::kill_process(pid, signal);
}

#[cfg(windows)]
struct SignalListeners {
    ctrl_c: tokio::signal::windows::CtrlC,
    ctrl_break: tokio::signal::windows::CtrlBreak,
}

#[cfg(windows)]
impl SignalListeners {
    fn register() -> io::Result<Self> {
        Ok(Self {
            ctrl_c: tokio::signal::windows::ctrl_c()?,
            ctrl_break: tokio::signal::windows::ctrl_break()?,
        })
    }

    async fn wait_with_forwarding(mut self, child: &mut Child) -> PassThroughExit {
        loop {
            tokio::select! {
                result = child.wait() => {
                    return classify_wait_result(result);
                }
                // An intercepted Ctrl-C always hard-kills directly, never
                // attempts a group-scoped `CTRL_C_EVENT` (research.md's
                // Windows Ctrl-C limitation: `CTRL_C_EVENT` can't be
                // safely group-scoped, unlike `CTRL_BREAK_EVENT`).
                Some(()) = self.ctrl_c.recv() => { let _ = child.kill().await; }
                Some(()) = self.ctrl_break.recv() => forward_ctrl_break(child).await,
            }
        }
    }
}

/// The bounded wait after a successful `GenerateConsoleCtrlEvent` call
/// before rechecking whether the child is still running (research.md,
/// plan.md Complexity Tracking) — a fixed, named constant, not a
/// deployment setting.
#[cfg(windows)]
const CTRL_BREAK_GRACE_PERIOD: std::time::Duration = std::time::Duration::from_millis(1000);

/// Best-effort graceful forwarding of an intercepted Ctrl-Break to the
/// child's own process group, falling back to a hard `child.kill()` per
/// [`ctrl_break_fallback_decision`].
#[cfg(windows)]
async fn forward_ctrl_break(child: &mut Child) {
    let event_sent = attempt_ctrl_break(child);
    if ctrl_break_fallback_decision(event_sent, child).await {
        let _ = child.kill().await;
    }
}

/// `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child_pid)` — this ticket's
/// one `unsafe` FFI call site (plan.md Complexity Tracking). Returns
/// whether the OS call itself reported success; the caller
/// ([`ctrl_break_fallback_decision`]) never trusts this alone.
#[cfg(windows)]
fn attempt_ctrl_break(child: &Child) -> bool {
    let Some(pid) = child.id() else { return false };
    // SAFETY: FFI category 8. (1) `child` (this call's own spawned
    // process) is still unreaped here, so no concurrent drop races it --
    // this reduces, but per Microsoft's own documentation does not fully
    // eliminate, the risk of the OS recycling `pid` after termination, so
    // `ctrl_break_fallback_decision` never trusts this `BOOL` return
    // alone. (2) `child` was spawned with `CREATE_NEW_PROCESS_GROUP`
    // (`build_command`) so the event targets its own group without also
    // signaling `allez`'s own process, and shares `allez`'s own console
    // session (no `CREATE_NO_WINDOW`/detached-console spawn), which
    // delivery requires. (3) A falsy return here, or a still-running
    // child after the bounded recheck, both fall back to `child.kill()`
    // (`forward_ctrl_break`) rather than assuming success or looping
    // indefinitely.
    unsafe {
        windows_sys::Win32::System::Console::GenerateConsoleCtrlEvent(
            windows_sys::Win32::System::Console::CTRL_BREAK_EVENT,
            pid,
        ) != 0
    }
}

/// The Ctrl-Break fallback-to-`child.kill()` decision, factored out of
/// [`attempt_ctrl_break`]'s own unsafe FFI call so it is directly
/// unit-testable (T035a) against a real, independently spawned child —
/// no console-control event needed.
#[cfg(windows)]
async fn ctrl_break_fallback_decision(event_sent: bool, child: &mut Child) -> bool {
    if !event_sent {
        return true;
    }
    tokio::time::sleep(CTRL_BREAK_GRACE_PERIOD).await;
    // Only a confirmed exit (`Ok(Some(_))`) skips the fallback kill. Both
    // "still running" (`Ok(None)`) and "unknown" (`Err(_)`, e.g. the OS
    // call itself failed) fall back to `child.kill()` — never assuming
    // the child is gone without positive confirmation.
    !matches!(child.try_wait(), Ok(Some(_)))
}

/// Registers termination-signal listeners, builds and spawns the
/// pass-through program's `Command`, then races its `wait()` against
/// those listeners — forwarding every intercepted signal and continuing
/// to wait for the child's real outcome (FR-014) — until it resolves.
/// Returns `Err` for one of the four pre-start [`PassThroughFailure`]
/// variants; the program's own started-and-terminated outcome (normal or
/// signaled) is always `Ok`, never `Err`.
pub(crate) async fn run_pass_through(
    environment: &ReadyEnvironment,
    pass_through: &PassThroughArgs,
) -> Result<PassThroughExit, PassThroughFailure> {
    let listeners =
        SignalListeners::register().map_err(|_| PassThroughFailure::SignalSetupFailed)?;
    let mut command = build_command(environment, pass_through)?;
    let mut child = command
        .spawn()
        .map_err(|error| classify_spawn_error(&error))?;
    Ok(listeners.wait_with_forwarding(&mut child).await)
}

/// Version of the `OneshotOutcomeEvent` schema — also reused by
/// `main.rs`'s shared `exit_on_invalid_pass_through` usage-error event
/// (the `oneshot`/`run`/`sandbox` pass-through subcommands' one common
/// pre-dispatch rejection path), hence the name scoped to "pass-through"
/// rather than "oneshot" specifically.
pub const PASS_THROUGH_EVENT_SCHEMA_VERSION: &str = "1";

/// Structured observability data for one `allez oneshot` invocation's
/// pass-through-command outcome. Emitted via `tracing`, exactly like
/// `EphemeralLifecycleEvent` — schema-versioned and emitted strictly
/// outside the FR-004 streaming window: either immediately after
/// `create_ephemeral_environment` resolves (if it failed), or immediately
/// after the pass-through program's own outcome is known (if creation
/// succeeded) — exactly one of the two, per invocation, never both.
pub(crate) struct OneshotOutcomeEvent {
    /// Version of this event schema.
    pub(crate) schema_version: &'static str,
    /// Correlates every record for one invocation together (FR-012) — the
    /// same `EnvironmentId` GEN-24's `ReadyEnvironment`/`CreationFailure`
    /// already carry.
    pub(crate) invocation_id: EnvironmentId,
    /// Whether the pass-through program was ever started.
    pub(crate) pass_through_started: bool,
    /// The pass-through program's own exit code, once its outcome
    /// (normal exit, signal, or could-not-start) is known; `None` for the
    /// environment-creation-failure event.
    pub(crate) exit_code: Option<i32>,
    /// The fixed failure category, present exactly when this outcome was
    /// not a normal exit.
    pub(crate) failure_category: Option<&'static str>,
    /// A human-readable message for this outcome, present whenever
    /// `failure_category` is.
    pub(crate) message: Option<String>,
    /// Present only for the FR-010 dual-failure case: the environment's
    /// own creation-rollback failure category, alongside the primary
    /// `failure_category`.
    pub(crate) cleanup_category: Option<&'static str>,
    /// The dual-failure case's own human-readable cleanup message,
    /// paired with `cleanup_category`.
    pub(crate) cleanup_message: Option<String>,
}

/// Emits one [`OneshotOutcomeEvent`] through `tracing` — `tracing::error!`
/// when `failure_category` is present, `tracing::info!` otherwise,
/// mirroring `ephemeral::events::emit_event`'s own success/failure split.
pub(crate) fn emit_outcome_event(event: &OneshotOutcomeEvent) {
    if event.failure_category.is_some() {
        tracing::error!(
            schema_version = event.schema_version,
            invocation_id = %event.invocation_id,
            pass_through_started = event.pass_through_started,
            exit_code = ?event.exit_code,
            failure_category = ?event.failure_category,
            message = ?event.message,
            cleanup_category = ?event.cleanup_category,
            cleanup_message = ?event.cleanup_message,
            "oneshot pass-through outcome"
        );
    } else {
        tracing::info!(
            schema_version = event.schema_version,
            invocation_id = %event.invocation_id,
            pass_through_started = event.pass_through_started,
            exit_code = ?event.exit_code,
            failure_category = ?event.failure_category,
            message = ?event.message,
            cleanup_category = ?event.cleanup_category,
            cleanup_message = ?event.cleanup_message,
            "oneshot pass-through outcome"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_category_and_exit_code() {
        assert_eq!(
            PassThroughFailure::NotFound.category(),
            "pass_through_not_found"
        );
        assert_eq!(PassThroughFailure::NotFound.exit_code(), 127);
    }

    #[test]
    fn not_executable_category_and_exit_code() {
        assert_eq!(
            PassThroughFailure::NotExecutable.category(),
            "pass_through_not_executable"
        );
        assert_eq!(PassThroughFailure::NotExecutable.exit_code(), 126);
    }

    #[test]
    fn terminated_by_signal_category_and_exit_code() {
        let failure = PassThroughFailure::TerminatedBySignal { signal: 15 };
        assert_eq!(failure.category(), "pass_through_terminated_by_signal");
        assert_eq!(failure.exit_code(), 143);
    }

    #[test]
    fn activation_failed_category_and_exit_code() {
        assert_eq!(
            PassThroughFailure::ActivationFailed.category(),
            "activation_failed"
        );
        assert_eq!(PassThroughFailure::ActivationFailed.exit_code(), 1);
    }

    #[test]
    fn signal_setup_failed_category_and_exit_code() {
        assert_eq!(
            PassThroughFailure::SignalSetupFailed.category(),
            "signal_setup_failed"
        );
        assert_eq!(PassThroughFailure::SignalSetupFailed.exit_code(), 1);
    }

    #[test]
    fn wait_failed_category_and_exit_code() {
        assert_eq!(
            PassThroughFailure::WaitFailed.category(),
            "pass_through_wait_failed"
        );
        assert_eq!(PassThroughFailure::WaitFailed.exit_code(), 1);
    }

    #[test]
    fn classify_wait_result_err_maps_to_wait_failed() {
        let error = io::Error::from(io::ErrorKind::Other);
        assert!(matches!(
            classify_wait_result(Err(error)),
            PassThroughExit::WaitFailed
        ));
    }

    #[test]
    fn classify_spawn_error_not_found_maps_to_not_found() {
        let error = io::Error::from(io::ErrorKind::NotFound);
        assert_eq!(classify_spawn_error(&error), PassThroughFailure::NotFound);
    }

    #[test]
    fn classify_spawn_error_permission_denied_maps_to_not_executable() {
        let error = io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(
            classify_spawn_error(&error),
            PassThroughFailure::NotExecutable
        );
    }

    #[test]
    fn classify_spawn_error_other_kind_maps_to_not_executable() {
        let error = io::Error::from(io::ErrorKind::Other);
        assert_eq!(
            classify_spawn_error(&error),
            PassThroughFailure::NotExecutable
        );
    }

    #[cfg(unix)]
    #[test]
    fn classify_exit_status_normal_exit_maps_to_normal() {
        use std::os::unix::process::ExitStatusExt;
        let status = std::process::ExitStatus::from_raw(0);
        assert!(matches!(
            classify_exit_status(status),
            PassThroughExit::Normal { exit_code: 0 }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn classify_exit_status_signal_maps_to_signaled() {
        use std::os::unix::process::ExitStatusExt;
        for signal in [15, 9] {
            // `raw` values below 256 are always a normal exit on Linux's
            // `waitpid` encoding; a signal is encoded in the low 7 bits
            // with the high byte zero, so `raw = signal` here.
            let status = std::process::ExitStatus::from_raw(signal);
            assert!(
                matches!(classify_exit_status(status), PassThroughExit::Signaled { signal: s } if s == signal)
            );
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn ctrl_break_fallback_decision_returns_true_when_event_not_sent() {
        // `ping` (unlike `cmd /C timeout`) doesn't require an interactive
        // console, so it reliably stays alive for the ~5 seconds this test
        // needs on a headless CI runner too.
        let mut child = tokio::process::Command::new("ping")
            .args(["-n", "6", "127.0.0.1"])
            .spawn()
            .unwrap();
        assert!(ctrl_break_fallback_decision(false, &mut child).await);
        let _ = child.kill().await;
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn ctrl_break_fallback_decision_returns_true_when_still_running_after_grace_period() {
        // See comment in the sibling test above on why `ping` and not
        // `cmd /C timeout` is used here.
        let mut child = tokio::process::Command::new("ping")
            .args(["-n", "6", "127.0.0.1"])
            .spawn()
            .unwrap();
        assert!(ctrl_break_fallback_decision(true, &mut child).await);
        let _ = child.kill().await;
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn ctrl_break_fallback_decision_returns_false_when_child_already_exited() {
        let mut child = tokio::process::Command::new("cmd")
            .args(["/C", "exit", "0"])
            .spawn()
            .unwrap();
        let _ = child.wait().await;
        assert!(!ctrl_break_fallback_decision(true, &mut child).await);
    }
}
