# Phase 1 Data Model: `allez oneshot` Command

This feature adds orchestration glue over GEN-23/GEN-24's existing public
APIs, plus one new error/outcome type those APIs don't already categorize.
"Entities" here are the Rust types that make spec.md's Key Entities (§ Key
Entities) representable in code, per Constitution VI.

## `PassThroughFailure` (new — `src/cli/pass_through.rs`)

The fixed category set for failures that occur *after* the environment is
already ready, but *before or during* running the pass-through program —
distinct from `EphemeralEnvError` (an environment-creation failure) and
mutually exclusive with it, matching spec.md's Key Entities description of
"Pass-Through Command Outcome" as "mutually exclusive with an
environment-creation failure, since the two can never both apply to the
same invocation."

```rust
/// The fixed category set for pass-through-command failures that occur
/// after the environment is already ready. Implements `CategorizedError`
/// (`src/error.rs`) — `category()` below is that trait's own method, not
/// an inherent one, exactly as `EphemeralEnvError` already implements it
/// (`src/ephemeral/error.rs`) — so `output::render_error` has exactly
/// one rendering path regardless of which subsystem raised the error
/// (Constitution IV). `CategorizedError: std::error::Error` also
/// requires `Display`/`std::error::Error` themselves — omitted from this
/// sketch for brevity, but part of this type's real contract.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassThroughFailure {
    /// The pass-through program's name could not be found (FR-008).
    NotFound,
    /// The pass-through program was found but could not be executed
    /// (FR-008) — also the fallback classification for any spawn-time
    /// `io::Error` kind FR-008 does not separately name (research.md).
    NotExecutable,
    /// The pass-through program was terminated by a signal it does not
    /// survive, on a platform where that concept exists (FR-007). Carries
    /// the raw signal number so `128 + signal` and the human-readable
    /// message can both be derived from one source of truth. **Not a
    /// pre-start failure** — unlike the other four variants, the
    /// program already started and ran; see `OneshotOutcome`'s
    /// construction rule below for why this variant's `category()`/
    /// message are used *only* for the `OneshotOutcomeEvent` tracing
    /// record, never for caller-facing rendering.
    TerminatedBySignal { signal: i32 },
    /// Computing the environment's own activation variables failed after
    /// the environment was already successfully created (research.md's
    /// "`ActivationError` mapping" decision). Carries no field: GEN-24's
    /// `ActivationError.message` is `rattler_shell`'s own raw error text
    /// and can plausibly include the environment's own filesystem path,
    /// so it is never forwarded into caller-facing output or the
    /// `OneshotOutcomeEvent` tracing record (FR-015) — both render a
    /// single fixed string for this variant instead (research.md).
    ActivationFailed,
    /// Registering a `tokio::signal::unix`/`tokio::signal::windows`
    /// listener itself failed (an `io::Result::Err`, research.md's
    /// signal-forwarding decision) — a rare, OS-resource-level failure,
    /// not a program- or environment-related one. Occurs after the
    /// environment is ready but strictly before `.spawn()`, so — like
    /// `ActivationFailed` — it is a pre-start failure and maps to
    /// `OneshotOutcome::PassThroughFailed`, never `PassThroughExited`.
    SignalSetupFailed,
}

impl CategorizedError for PassThroughFailure {
    /// The category string for this variant.
    fn category(&self) -> &'static str {
        match self {
            Self::NotFound => "pass_through_not_found",
            Self::NotExecutable => "pass_through_not_executable",
            Self::TerminatedBySignal { .. } => "pass_through_terminated_by_signal",
            Self::ActivationFailed => "activation_failed",
            Self::SignalSetupFailed => "signal_setup_failed",
        }
    }
}

impl PassThroughFailure {
    /// The exit code this failure maps to (FR-006/FR-007/FR-008, and this
    /// plan's own `activation_failed`/`signal_setup_failed` decisions).
    /// `TerminatedBySignal`'s code is computed, not fixed, so it is a
    /// method rather than a `const` table. `ActivationFailed` and
    /// `SignalSetupFailed` are deliberately their own match arms
    /// returning `1`, not folded into `NotExecutable`'s `126` arm — the
    /// categories map to different exit codes, and merging their arms
    /// would risk exactly this kind of copy-paste error.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::NotFound => 127,
            Self::NotExecutable => 126,
            Self::ActivationFailed | Self::SignalSetupFailed => 1,
            Self::TerminatedBySignal { signal } => 128 + signal,
        }
    }
}
```

## `OneshotOutcome` (new — `src/cli/oneshot.rs`)

Makes "did the pass-through program even start" and "how did it end"
mutually exclusive at the type level (Constitution VI), and is the value
`oneshot::run`'s caller (`main.rs`'s `dispatch`) uses to pick
`std::process::exit`'s real code — `oneshot::run` itself never calls
`std::process::exit`, keeping it a plain, unit-testable function. Scoped
to one subcommand's own result, not an application-wide concept, hence
`OneshotOutcome` rather than a generic `AllezOutcome`.

```rust
/// The result of one `allez oneshot` invocation, once past usage-error
/// validation (already handled at the dispatch layer, per the existing
/// `validate_pass_through` convention `run`/`sandbox` also use). `pub`,
/// not `pub(crate)`: `src/main.rs` is a *separate binary crate* that
/// consumes this library via `use allez::{cli, ...}` (confirmed against
/// `src/main.rs`'s own existing `use` line) — `pub(crate)` restricts
/// visibility to the `allez` *library* crate only, which `main.rs`'s
/// `dispatch` (this type's one real consumer) is not part of, so
/// `pub(crate)` would make this type invisible to the very code that
/// needs to match on it. `OneshotOutcome` itself needs no new `pub mod`
/// line of its own: `oneshot.rs` is already declared via the pre-existing
/// `pub mod oneshot;` (`src/cli/mod.rs`), one of the six existing sibling
/// subcommand modules, so this type is already reachable from `main.rs`
/// once it is `pub`. `pub mod pass_through;` (a one-line addition — see
/// Project Structure) is what makes `PassThroughFailure` and
/// `OneshotOutcomeEvent` — both new to `pass_through.rs` — reachable from
/// `main.rs` instead.
pub enum OneshotOutcome {
    /// The environment could not be created (FR-010). `render_ephemeral_
    /// creation_failure` (new `output.rs` function) has already been
    /// called to produce `message`. No stored `exit_code` field: it is
    /// always `1` for this variant, so `OneshotOutcome::exit_code()`
    /// (below) returns the literal `1` for this arm rather than reading
    /// a field that could otherwise be set to anything.
    EnvironmentCreationFailed { message: String },
    /// The pass-through program could not be started. Carries the
    /// classifying `PassThroughFailure` value itself — restricted by
    /// convention to its four pre-start variants only (`NotFound`,
    /// `NotExecutable`, `ActivationFailed`, `SignalSetupFailed`; see the
    /// construction rule below) — rather than a separately-stored
    /// `exit_code`/`category`, so `OneshotOutcome::exit_code()` can only
    /// ever return what `PassThroughFailure::exit_code()`'s own closed,
    /// exhaustive mapping already produces for that value. `message` has
    /// already been rendered via `output::render_error`.
    PassThroughFailed { message: String, failure: PassThroughFailure },
    /// The pass-through program started and terminated — normally
    /// (FR-006) or via a signal it did not survive (FR-007). No message
    /// in either case: FR-013 forbids wrapping the started command's own
    /// raw output/exit code in any envelope once it has started.
    PassThroughExited { exit_code: i32 },
}

impl OneshotOutcome {
    /// The exit code for this outcome (FR-006). Never a field that could
    /// independently diverge from what its own variant should carry —
    /// `EnvironmentCreationFailed` is a literal `1`, `PassThroughFailed`
    /// delegates to the wrapped `PassThroughFailure::exit_code()`, and
    /// `PassThroughExited` is the one genuinely caller/OS-determined
    /// value in this type (any `0..=255`, the child's own propagated
    /// code).
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::EnvironmentCreationFailed { .. } => 1,
            Self::PassThroughFailed { failure, .. } => failure.exit_code(),
            Self::PassThroughExited { exit_code } => *exit_code,
        }
    }
}
```

`oneshot::run`'s caller matches on this three-variant enum, calling
`.exit_code()` for the numeric value: the first two print `message` to
stderr (respecting `--human`, already computed) then exit; the third
prints nothing and exits directly — a direct, mechanical encoding of
FR-013's "no envelope" rule and FR-010/FR-008's "render a message, then
exit" rule, with no risk of accidentally printing a message for the one
case (`PassThroughExited`) where spec.md forbids it.

**Constitution VI**: this design makes an invalid `PassThroughFailed`/
`EnvironmentCreationFailed` exit code structurally impossible to
construct, not merely unlikely-in-practice — there is no `exit_code: i32`
field on either variant for any caller (inside or outside this crate,
since the type must be `pub`, above) to set independently; the numeric
value only ever exists as the *result* of calling `PassThroughFailure::
exit_code()` or returning the fixed literal `1`. The one remaining,
narrower gap this doesn't close: `PassThroughFailed`'s `failure` field is
typed as the full five-variant `PassThroughFailure`, so nothing at the
type level stops constructing `PassThroughFailed { failure:
PassThroughFailure::TerminatedBySignal { .. }, .. }`, which the
construction rule below says must never happen. Closing *that* statically
would need a second, narrower enum (the four pre-start variants only,
without `TerminatedBySignal`) purely to prevent a construction this
ticket's own single constructor (`oneshot.rs`) never performs — judged
not worth doubling the number of failure-category enums for a rule with
exactly one enforcement point. This is a deliberate, documented, narrower
simplification of Principle VI's literal "make invalid states
unrepresentable" text than the type-level exit-code guarantee above
already provides, in the same spirit as GEN-24's own documented
Principle IX/X deviations (GEN-24's `plan.md`, Constitution Check) —
flagged here rather than silently assumed.

**Critical construction rule (FR-013 compliance) — `PassThroughFailure`
does not map uniformly to `OneshotOutcome`.** `PassThroughFailure::
NotFound`, `NotExecutable`, `ActivationFailed`, and `SignalSetupFailed`
are all pre-start failures — the pass-through program never started —
and are the only four variants that may ever appear inside
`OneshotOutcome::PassThroughFailed { failure, .. }`. `TerminatedBySignal`
is different: by the time it exists, the pass-through program already
started and ran. FR-013 forbids any caller-facing message once that's
true, regardless of how the command later ended — so whoever constructs
the `OneshotOutcome` for this case MUST route `TerminatedBySignal` into
`OneshotOutcome::PassThroughExited { exit_code: failure.exit_code() }`
(i.e. `128 + signal`), never into `PassThroughFailed`, even though both
are built from the same `PassThroughFailure` type. `TerminatedBySignal`'s
`category()` and a rendered message are still produced and used — but
only to populate the `OneshotOutcomeEvent` tracing record
(`failure_category`/`message` below), never anything printed to the
caller. Folding `TerminatedBySignal` into `PassThroughFailed` uniformly
with the other four variants would violate FR-013 for every
signal-terminated pass-through command; this construction rule is what
prevents that.

## `OneshotOutcomeEvent` (new — `src/cli/pass_through.rs`)

Extends GEN-24's `EphemeralLifecycleEvent` shape/redaction discipline
(FR-012's "additive extension" requirement) with this feature's own
fields, correlated to the same invocation via `EnvironmentId` — reusing
`ReadyEnvironment::id`/`CreationFailure::id` (GEN-24 already attaches an
`EnvironmentId` to *both* outcomes) as the "value that ties every record
for one invocation together" FR-012 requires, rather than minting a
second, redundant correlation identifier.

**The usage-error, "never attempted" case is already covered without this
event type.** FR-012 also requires a record for an invocation "rejected as
a usage error" (no `--`, or `--` with nothing after it) — that rejection
happens at the dispatch layer, in `main.rs`, *before* `oneshot::run` (and
therefore this event) is ever reached, and no `EnvironmentId` exists yet
to correlate it by. `main.rs`'s existing `exit_on_invalid_pass_through`
already emits `tracing::warn!(operation, category = err.category(), ...)`
for exactly this case — that pre-existing call site is FR-012's own
answer for "was never attempted." This ticket makes one small, additive
edit to it: adding a `schema_version` field (the same schema-version
string `OneshotOutcomeEvent` below uses), so the record satisfies FR-012's
"carrying its own documented schema version" clause explicitly rather than
only in spirit. No correlation identifier is added to it: FR-012's
correlation requirement exists to tie *multiple* records for one
invocation together, and a rejected-as-usage-error invocation never
produces more than this one record — there is nothing for a correlation
value to tie it to. This edit is shared infrastructure (`run`/`sandbox`
call the same `exit_on_invalid_pass_through` function), so it benefits
all three pass-through subcommands uniformly, not just `oneshot`.

```rust
/// Structured observability data for one `allez oneshot` invocation's
/// pass-through-command outcome. Emitted via `tracing`, exactly like
/// `EphemeralLifecycleEvent` — schema-versioned, redaction-safe, and
/// emitted strictly outside the FR-004 streaming window: either
/// immediately after `create_ephemeral_environment` resolves (if it
/// failed), or immediately after the pass-through program's own outcome
/// is known (if creation succeeded) — exactly one of the two, per
/// invocation, never both; never while the pass-through program is
/// running.
pub struct OneshotOutcomeEvent {
    /// Version of this event schema.
    pub schema_version: &'static str,
    /// Correlates every record for one invocation together (FR-012) —
    /// the same `EnvironmentId` GEN-24's `ReadyEnvironment`/
    /// `CreationFailure` already carry.
    pub invocation_id: EnvironmentId,
    /// Whether the pass-through program was ever started.
    pub pass_through_started: bool,
    /// `Some(exit_code)` once the pass-through program's outcome (normal
    /// exit, signal, or could-not-start) is known; `None` for the
    /// environment-creation-failure event, and for the initial
    /// "about to start" event this feature never actually needs to emit
    /// separately (`pass_through_started` transitions directly from
    /// `false` to a terminal event with `Some(_)`, since there is
    /// nothing observable about the moment of spawning itself worth a
    /// standalone record — FR-012 only requires "whether the pass-
    /// through command started," which the terminal event's own
    /// `pass_through_started: true` already answers unconditionally).
    pub exit_code: Option<i32>,
    /// The fixed failure category, present exactly when this outcome was
    /// not a normal exit (an environment-creation failure, or one of
    /// `PassThroughFailure`'s categories).
    pub failure_category: Option<&'static str>,
    /// A human-readable message for this outcome, present whenever
    /// `failure_category` is (FR-012's "a human-readable message ...
    /// when available"). Always the same fixed/redacted text the
    /// caller-facing rendering already used — never a second, separately
    /// composed string, so this event can't diverge from what the
    /// caller was actually told.
    pub message: Option<String>,
    /// Present only for the FR-010 dual-failure case: the environment's
    /// own creation-rollback failure category, alongside the primary
    /// `failure_category` — mirrors the caller-facing
    /// `cleanup_category` JSON field (`contracts/oneshot_cli_contract.md`)
    /// so the observability channel never carries less detail than the
    /// caller-facing error already does.
    pub cleanup_category: Option<&'static str>,
    /// The dual-failure case's own human-readable cleanup message,
    /// paired with `cleanup_category`.
    pub cleanup_message: Option<String>,
}
```

**Redaction**: unlike `EphemeralLifecycleEvent`, this event carries no
`packages`/channel-URL-bearing field at all — `failure_category` is always
one of the fixed, non-credential-bearing category strings above, and
`exit_code` is a bare integer — so no `redact_channel_url()` pass is
needed on this event's own fields specifically (GEN-24's environment-
creation-side events, emitted separately by `ephemeral::mod.rs` itself,
already apply that redaction to package/channel content on their own; this
event does not duplicate or replace those).

## Reused, unmodified types (GEN-23/GEN-24)

- `condarc::ResolvedChannels`, `channel_config::ChannelConfigResolution`
  (GEN-23) — consumed via `channel_config::resolve_channel_config()`.
- `ephemeral::{RequestedPackages, PackageSpec, InvalidPackageSpec,
  EphemeralEnvError, CreationFailure, ReadyEnvironment, EnvironmentId,
  ActivationError}` (GEN-24) — consumed via `create_ephemeral_environment`
  and `ReadyEnvironment::activation_environment`. `create_ephemeral_
  environment`'s third parameter, `default_override: Option<Vec<
  PackageSpec>>`, is always passed `None` by this ticket (research.md's
  "Default package list" decision) — no mechanism to populate it from a
  caller-configured override exists yet; building one is GEN-30's scope.
- `crate::error::CategorizedError` (GEN-22/GEN-24) — implemented by the new
  `PassThroughFailure` above, exactly as `AllezError`/`EphemeralEnvError`
  already implement it.
- `crate::cli::{PackagesAndCommandArgs, PassThroughArgs}` (GEN-22) —
  unmodified; `validate_pass_through` already runs at the dispatch layer.
