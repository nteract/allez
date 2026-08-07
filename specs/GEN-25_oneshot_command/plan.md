# Implementation Plan: `allez oneshot` Command

**Branch**: `GEN-25_oneshot_command` | **Date**: 2026-08-03 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/GEN-25_oneshot_command/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command; its definition describes the execution workflow.

## Summary

Wire the existing `allez oneshot` CLI stub (`src/cli/oneshot.rs`) to GEN-24's
already-implemented `allez::ephemeral` library: resolve the caller's channel
configuration (GEN-23's `channel_config::resolve_channel_config()`), call
`create_ephemeral_environment()` with the packages named before `--` (or the
built-in default set), and — once that succeeds — `exec`-equivalent the
pass-through program named after `--` inside the new environment with its
`PATH`/activation variables merged on top of `allez`'s own inherited
environment, its stdio inherited (streamed, not buffered, stdin forwarded
unchanged), and its real exit code propagated as `allez`'s own exit code.
See `research.md` for the rationale behind each decision below.

Technical approach: promote `main.rs` to an async `#[tokio::main]` entry
point (the ephemeral core is already `async`; nothing in this codebase
currently runs a Tokio runtime); spawn the pass-through program via
`tokio::process::Command` so a `tokio::select!` loop can race the child's
`wait()` against Tokio's own cross-platform signal listeners for FR-014's
forward-and-keep-waiting behavior, forwarding to the direct child PID via
`rustix::process::kill_process` on Unix and `windows-sys`'s
`GenerateConsoleCtrlEvent` on Windows, without adding a new dependency.
Every pre-start failure (usage error, channel resolution producing zero
channels, package resolution/integrity failure, program-not-found/
not-executable) renders through this project's existing JSON/human
dual-format convention (`output::render_error`, extended additively for
FR-010's dual-failure case); once the pass-through program successfully
starts, `allez` prints nothing of its own on stdout/stderr ever again
(FR-013) — its propagated exit code is the entire result. `DEFAULT_PACKAGES`
(GEN-24) changes to `["python"]`, a documented stopgap until GEN-30 provides
a real default/override-authoring mechanism — see `research.md`.

## Technical Context

**Language/Version**: Rust, `edition = "2024"` (unchanged from GEN-24).

**Primary Dependencies**: No new crates. Reuses `allez::ephemeral::*`
(GEN-24) and `allez::channel_config::resolve_channel_config()` (GEN-23) as
in-process library calls. Two existing direct dependencies gain feature
flags this ticket newly relies on:

- `tokio`: adds the `process` feature (`tokio::process::Command`/`Child`)
  and the `signal` feature (`tokio::signal::unix::signal`,
  `tokio::signal::windows::{ctrl_c, ctrl_break}`) — both stock Tokio 1.53
  features; see `research.md`.
- `windows-sys` (Windows-only): adds the `Win32_System_Console` feature for
  `GenerateConsoleCtrlEvent`/`CTRL_BREAK_EVENT` — `Win32_System_Threading`
  (already enabled, for `CREATE_NEW_PROCESS_GROUP`) stays unchanged.

`rustix` (already a direct dependency with the `process` feature, currently
used only for `mkdirat`/`geteuid` in `src/ephemeral/`) gains a second call
site: `rustix::process::kill_process` for Unix signal forwarding, targeting
the pass-through child's own direct PID (not its process group — see
`research.md` for why group-wide forwarding was rejected) — no
`Cargo.toml` feature change needed, since `process` already covers it. No
new `Cargo.toml` dependency entries of any kind (see `research.md`'s
signal-forwarding decision for why `nix`/`signal-hook` — both present only
*transitively* in `Cargo.lock` — are deliberately not promoted to direct
dependencies).

**Storage**: N/A — unchanged from GEN-24; this ticket adds no new
filesystem footprint of its own beyond what `create_ephemeral_environment`
already owns.

**Testing**: `cargo test --all --features test-config-override` becomes
the project's own default test invocation (`Makefile`'s `test` target and
`.github/workflows/ci.yml`'s `test`/`coverage` jobs all gain the new
flag — see Project Structure); a bare `cargo test --all` with no explicit
features still compiles and runs everything except `tests/oneshot_exec.rs`
itself. New integration tests in `tests/oneshot_exec.rs`, spawning the real
compiled `allez` binary
via `assert_cmd` (matching `tests/cli_scaffold.rs`'s existing pattern) against
the same checked-in local `file://` fixture channel GEN-24's
`tests/fixtures/ephemeral_channel/` already provides — each test process
sets a new, test-only environment variable, `ALLEZ_CONDARC_PATH` (pointing
`channel_config::default_condarc_path()` at an explicit `.condarc` file
instead of `dirs::home_dir()`'s own default) plus `ALLEZ_EPHEMERAL_ROOT`,
both via `assert_cmd`'s per-invocation `Command::env()` (not a
process-wide env var), so tests stay independent without needing
`serial_test`, unlike GEN-24's own in-process integration suite.
`ALLEZ_CONDARC_PATH`'s check exists only behind a new, non-default Cargo
feature, `test-config-override` — see `research.md` § Test strategy and
`quickstart.md`. Not part of this ticket's public contract, and not
compiled into any release build.

**Target Platform**: Windows amd64, macOS aarch64, Linux aarch64, Linux
amd64 — unchanged from GEN-24/the parent epic (GEN-19). Signal-forwarding
(FR-014) and the 128-plus-signal exit convention (FR-007) are Unix-only
concepts; Windows uses `Ctrl-C`/`Ctrl-Break` console events as its nearest
equivalent, with a real but bounded forwarding mechanism (see
`research.md`).

**Project Type**: CLI (single binary, single library target — unchanged
from GEN-22/GEN-24's existing `src/lib.rs` + `src/main.rs` split).

**Performance Goals**: Not defined by this ticket (tracked separately per
GEN-24's own Performance Goals note); SC-002 requires observably streamed
output, not a specific latency target.

**Constraints**: Once the pass-through program starts, `allez` MUST NOT
write anything of its own to stdout/stderr (FR-013) — every `tracing`
observability emission for that invocation happens strictly before spawning
or strictly after `wait()` resolves, never concurrently with it (FR-012).
`allez`'s own exit code for this subcommand is either the pass-through
program's own propagated code (any value `0..=255`, authoritative once it
starts, per FR-006) or one of the fixed pre-start/abnormal-termination
values `{1, 2, 126, 127, 128..=128+max_signal}`; every other subcommand's
exit codes are unaffected. No environment teardown of any kind on a
successful outcome (FR-009) — GEN-24 exposes no environment-removal API of
any kind for this ticket to call in the first place, not merely one this
ticket chooses to abstain from. No disclosure of the created environment's
location/identifier in caller-facing stdout/stderr (FR-015) — satisfied by
`EphemeralEnvError`'s existing `Display` impls (GEN-24), carried forward
unchanged, plus this ticket's own `PassThroughFailure::ActivationFailed`
rendering a fixed, non-path-bearing message rather than forwarding
`rattler_shell`'s raw activation-error text (see `research.md`). FR-003's
"merge, not replace" environment inheritance also forwards whatever
GEN-29 (private-channel authentication, separately scoped) may eventually
inject into `allez`'s own process — e.g. a phantom auth-token environment
variable — to the pass-through program unchanged; spec.md's Operating
Context explicitly accepts this as adding no new trust boundary beyond
the sandbox's own, so this ticket does not add one either.

**Scale/Scope**: One CLI subcommand's real implementation, replacing its
existing stub; no new library module tree (this ticket is orchestration
glue over GEN-23/GEN-24's existing public APIs, plus one small new
observability/error surface for outcomes those APIs don't already
categorize — see Complexity Tracking).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|---|---|---|
| I. Code Quality | PASS | `src/cli/oneshot.rs` becomes a thin orchestrator (parse → resolve channels → create environment → spawn → wait/forward-signals → exit); process-spawning/signal-forwarding logic lives in its own module (`src/cli/pass_through.rs`), keeping `oneshot.rs` single-responsibility — see Project Structure. One new `unsafe` FFI call site (Windows `GenerateConsoleCtrlEvent`, see Complexity Tracking) — the fourth site under GEN-24's existing Win32-FFI exception category, not a new one. |
| II. Testing Standards | PASS | TDD; `tests/oneshot_exec.rs` (new) exercises the real end-to-end path (environment creation → pass-through spawn → exit code/stdio) via `assert_cmd`, isolated per test via per-invocation env vars (no shared mutable state, no `#[serial]`). Unit tests co-located in `pass_through.rs`/`oneshot.rs` cover the pure logic (exit-code classification, signal-number mapping, spawn-error → category mapping) that needs no real child process. |
| III. Dual-Primary Interface | PASS | Every pre-start failure still renders through `output::render_error` (JSON default / `--human`); FR-010's dual-failure case is an additive JSON field (`cleanup_category`/`cleanup_message`), not a breaking schema change. FR-013's "no envelope once the pass-through starts" is this principle's one spec-mandated exception — the propagated exit code is the machine-actionable contract for that outcome, so there is no separate payload to render twice. |
| IV. DRY | PASS | Reuses `CategorizedError`/`EphemeralEnvError`/`channel_config::ChannelConfigResolution` unchanged rather than re-deriving categories. The new category set (`PassThroughFailure`: `pass_through_not_found`, `pass_through_not_executable`, `pass_through_terminated_by_signal`, `activation_failed`, `signal_setup_failed`) implements the same `CategorizedError` trait GEN-22/GEN-24 already established. |
| V. Explicit Over Implicit | PASS | No `.unwrap()`/`.expect()` outside tests. Exit-code/category decisions are table-driven and exhaustively matched (`#[non_exhaustive] enum PassThroughFailure`), not inferred from string content. Environment-variable merge is explicit: `tokio::process::Command` inherits the parent environment by default, with the activation overlay applied via explicit `.env(key, value)` calls on top — no `.env_clear()`. |
| VI. Documentation and Type Safety | PASS (one documented, narrower simplification) | Every new public/`pub(crate)` type and function gets a doc comment; `cargo doc` must build clean. `OneshotOutcome` (see `data-model.md`) makes "did the program start" and "how did it end" mutually exclusive at the type level, and makes an invalid exit code for its failure variants structurally impossible to construct. One narrower gap remains unclosed statically — `PassThroughFailed`'s wrapped `PassThroughFailure` could theoretically be `TerminatedBySignal`, which the construction rule forbids — a deliberate, documented simplification (`data-model.md`'s own Constitution VI note), not a silent assumption. |
| VII. No Hardcoded Values | PASS | No new magic numbers beyond the shell-standard `128 + signal_number`/`126`/`127` convention FR-007/FR-008 already name in the spec itself, plus one new named constant: the Windows Ctrl-Break bounded wait, fixed at `100ms` (see Complexity Tracking/`research.md`) — not configurable, since it's an internal FFI-liveness-check timing detail, not a deployment setting. `$ALLEZ_EPHEMERAL_ROOT` and channel configuration stay fully caller-configurable via GEN-23/GEN-24's existing mechanisms; this ticket adds no new environment variable to any release build — `ALLEZ_CONDARC_PATH` exists only behind the non-default `test-config-override` Cargo feature (see Project Structure/`research.md`). |
| VIII. Mandatory 100% Spec Test Coverage | PASS (planned) | `quickstart.md` enumerates the full User-Story-1/2/3 acceptance-scenario → test mapping, including every FR-010 category, `pass_through_not_executable`, environment persistence (FR-009), caller-facing non-disclosure (FR-015), and no-separator usage. Exact test IDs are the task-breakdown phase's own job; this row covers scenario existence. Coverage is only real once `tests/oneshot_exec.rs` runs — see Project Structure's `Makefile`/`.github/workflows/ci.yml` entries for how `--features test-config-override` reaches every platform leg and the coverage job by default. |
| IX. Determinism & Idempotency | PASS | This ticket only calls `create_ephemeral_environment`, which already treats each call as a new operation, not a deduplicated retry (GEN-24's own Constitution Check). Repeating an identical `allez oneshot` invocation deterministically creates its own new, independent environment, matching spec.md's Acceptance Scenario 1.3. |
| X. Security & Supply-Chain Integrity | PASS | No new post-install-script or credential-handling surface. The pass-through program is the caller's own explicitly-directed command (spec Operating Context) — `allez` executes it directly, without an intermediate shell, exactly as supplied; no new consent gate is needed. |
| XI. Structured Observability | PASS | One new event, `OneshotOutcomeEvent` (`src/cli/pass_through.rs`), reuses `EphemeralLifecycleEvent`'s shape/redaction discipline: `schema_version`, an `invocation_id` (the same `EnvironmentId`/ULID `create_ephemeral_environment` already returns or attaches to `CreationFailure`), and fields for whether the pass-through started plus its termination outcome. Emitted exactly once per invocation that reaches `oneshot::run` — either the environment-creation-failure event or the terminal pass-through-outcome event, never both — outside the FR-004 streaming window. The "rejected as a usage error, never attempted" case is covered separately, by one additive `schema_version` field on `main.rs`'s existing `exit_on_invalid_pass_through` warning — see `data-model.md`. |

No constitution violations beyond the one documented `unsafe` FFI
exception (Windows `GenerateConsoleCtrlEvent`, sharing GEN-24's existing
accepted category — see Complexity Tracking).

One plan-level interpretation, not a constitution violation or spec.md
gap: an `ActivationError` from `ReadyEnvironment::
activation_environment()` (GEN-24; reachable per its own
`activation_environment_when_prefix_state_is_malformed_returns_
activation_error` test) occurs after environment creation already
succeeded but before the pass-through program can be spawned. FR-010's
four-category list is scoped to "environment-creation failure" — a state
this case never reaches — so mapping it to a new, additive
`activation_failed` category (exit code `1`) fills a gap the spec
doesn't address, rather than extending a closed enumeration. See
Complexity Tracking and `research.md` for the full reasoning and the two
rejected alternatives.

## Project Structure

### Documentation (this feature)

```text
specs/GEN-25_oneshot_command/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output (/speckit.plan command)
│   └── oneshot_cli_contract.md
└── tasks.md             # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)

Single Rust package within the existing `allez`/`crates/condarc` workspace
(unchanged from GEN-24). This ticket touches the existing CLI scaffold
(`src/cli/`, `src/main.rs`), `src/output.rs`, `Cargo.toml` (one new
`[features]` entry), `Makefile`, and `.github/workflows/ci.yml` (so
`test-config-override` reaches every default test run, not just a
manual invocation — see below), and adds one new module. It also makes
two small changes to GEN-23/GEN-24-owned files — `src/ephemeral/
defaults.rs`'s `DEFAULT_PACKAGES` constant, and a new, feature-gated
`ALLEZ_CONDARC_PATH` check in `src/channel_config/mod.rs` — see below
for why. No new workspace member.

```text
Cargo.toml                  # UPDATED — one new `[features]` entry, `test-config-override = []`,
                             #   not part of any `default = [...]` list (there is none today); gates the
                             #   `ALLEZ_CONDARC_PATH` check below out of any build that doesn't explicitly
                             #   request it. Also gains a new `[[test]]` block for `tests/oneshot_exec.rs`
                             #   with `required-features = ["test-config-override"]` (mirroring the existing
                             #   `condarc_conformance` `[[test]]` entry's own shape) — autodiscovery alone
                             #   cannot carry `required-features`, so both additions are needed together;
                             #   see the `tests/oneshot_exec.rs` entry below, which shows the literal
                             #   `[[test]]` block added alongside this `[features]` line.
Makefile                    # UPDATED — the `test` target becomes `cargo test --all --features
                             #   test-config-override`. Unlike `conformance-tests` (a genuinely slow,
                             #   external-oracle-dependent tier deliberately kept opt-in/separate), this
                             #   ticket's own tests are fully local/fixture-based and fast — the feature
                             #   flag exists only for the `ALLEZ_CONDARC_PATH` security reason (research.md),
                             #   not test speed, so it belongs on the *default* test invocation, not a new
                             #   dedicated target.
.github/workflows/ci.yml    # UPDATED — the `test` job's `cargo test --all --locked` (all four platform
                             #   legs) and the `coverage` job's `cargo llvm-cov --all --locked
                             #   --fail-under-lines 85` both gain `--features test-config-override`, for
                             #   the same reason as the `Makefile` change above — without this, Cargo
                             #   silently skips `tests/oneshot_exec.rs`'s `required-features`-gated
                             #   `[[test]]` target on every CI run, and the `pass_through.rs`/`oneshot.rs`
                             #   code it exercises would count as uncovered against the coverage floor. No
                             #   new CI job is added — this reuses the existing jobs' own invocations,
                             #   unlike `conformance`'s dedicated job, per the same rationale.
src/
├── main.rs                  # UPDATED — becomes `#[tokio::main(flavor = "multi_thread")] async fn main()`;
│                             #   `dispatch` becomes `async fn`; every non-Oneshot arm is unchanged
│                             #   (calling a sync fn from inside an async fn is legal and adds no behavior
│                             #   change for them); the Oneshot arm awaits `cli::oneshot::run` and calls
│                             #   `std::process::exit(code)` directly with its real, non-`emit_stub_success`
│                             #   exit code instead of the shared stub-success `println!` path every other
│                             #   arm still uses. `exit_on_invalid_pass_through`'s existing `tracing::warn!`
│                             #   gains one additive `schema_version` field (shared by `run`/`sandbox` too,
│                             #   since they call the same function) — see `data-model.md`'s
│                             #   `OneshotOutcomeEvent` section for why.
├── output.rs                # UPDATED — one new function, `render_ephemeral_creation_failure(failure:
│                             #   &CreationFailure, human: bool) -> String`, additive to the existing
│                             #   `{schema_version, category, message}` JSON shape with two optional fields
│                             #   (`cleanup_category`, `cleanup_message`, present only for FR-010's
│                             #   dual-failure case) — `render_error`/`render_success`/`render_pass_through`
│                             #   are otherwise untouched, so every existing call site/test keeps compiling
│                             #   unmodified.
├── error.rs                 # UNCHANGED — `AllezError`/`CategorizedError` reused as-is; no new `AllezError`
│                             #   variant (the pre-start failure categories this feature needs already live
│                             #   on `EphemeralEnvError` (GEN-24) or the new `PassThroughFailure` below, both
│                             #   of which already implement the shared `CategorizedError` trait this module
│                             #   defines).
├── ephemeral/                # UPDATED (one-line change) — `defaults.rs`'s `DEFAULT_PACKAGES` constant
│                             #   becomes `["python"]` (research.md's documented stopgap pending GEN-30);
│                             #   everything else UNCHANGED — consumed via `create_ephemeral_environment`,
│                             #   `ReadyEnvironment::activation_environment`, `RequestedPackages::from_cli`.
├── channel_config/            # UPDATED (one new, feature-gated env-var check) — `default_condarc_path()`
│                             #   reads `ALLEZ_CONDARC_PATH` before falling back to `dirs::home_dir()`'s own
│                             #   `~/.condarc`, but only when compiled with `--features
│                             #   test-config-override` (see `Cargo.toml` above) — a release build has no
│                             #   such check at all; everything else UNCHANGED (GEN-23).
└── cli/
    ├── mod.rs                  # UPDATED (one-line addition) — gains `pub mod pass_through;` (matching the
    │                             #   existing six sibling `pub mod` declarations) so `main.rs` — a separate
    │                             #   binary crate consuming `allez` via `use allez::{cli, ...}` — can actually
    │                             #   reach `PassThroughFailure` from `pass_through.rs` below; `OneshotOutcome`
    │                             #   itself lives in `oneshot.rs` (an already-`pub`, already-reachable
    │                             #   sibling module) and needs no new `pub mod` line of its own.
    │                             #   `PackagesAndCommandArgs`/`PassThroughArgs`/`validate_pass_through`
    │                             #   already exist and already run at the dispatch layer before this
    │                             #   feature's handler is ever invoked, unchanged.
    ├── oneshot.rs               # REWRITTEN (was a stub) — the orchestrator: `RequestedPackages::from_cli`
    │                             #   → `channel_config::resolve_channel_config()` → `create_ephemeral_
    │                             #   environment()` → on success, `pass_through::run_pass_through()`;
    │                             #   translates every pre-start failure into a rendered `String` +
    │                             #   `OneshotOutcome` (see `data-model.md`) the dispatch layer uses to pick
    │                             #   `std::process::exit`'s code — this handler itself never calls
    │                             #   `std::process::exit` directly, keeping it unit-testable.
    └── pass_through.rs          # NEW — this feature's entire process-lifecycle scope: registers Tokio
                                  #   signal listeners (classifying a listener-registration `io::Result::
                                  #   Err` as `PassThroughFailure::SignalSetupFailed`, category
                                  #   `signal_setup_failed`, exit code `1`) *before* building the
                                  #   `tokio::process::Command` (stdio inherited, activation-overlay env
                                  #   applied on top of the inherited environment; no Unix process-group
                                  #   change, `#[cfg(windows)]` `.creation_flags(CREATE_NEW_
                                  #   PROCESS_GROUP)` — required only because `GenerateConsoleCtrlEvent`
                                  #   itself needs a process-group-ID target, unrelated to the Unix
                                  #   decision); the `tokio::select!` loop racing `Child::wait()` against
                                  #   those listeners and forwarding via `rustix::process::kill_process`
                                  #   targeting the direct child PID (Unix) / best-effort
                                  #   `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, ...)` with a bounded
                                  #   still-running check then `child.kill()` fallback for Ctrl-Break only
                                  #   — an intercepted Ctrl-C always calls `child.kill()` directly, never
                                  #   attempts `CTRL_C_EVENT` (Windows — the one `unsafe` FFI call site
                                  #   this ticket adds); spawn-error → `PassThroughFailure`
                                  #   classification; `ExitStatus` → normal-exit or `PassThroughFailure::
                                  #   TerminatedBySignal` classification (128-plus-signal on Unix) — the
                                  #   latter maps into `OneshotOutcome::PassThroughExited`, never
                                  #   `PassThroughFailed` (see `data-model.md`'s construction rule); and
                                  #   `OneshotOutcomeEvent` emission (FR-012): once if environment creation
                                  #   fails (pre-spawn), or once after the pass-through outcome is known
                                  #   (whether that's a pre-start failure or a post-start exit/signal) —
                                  #   never both for one invocation, never during execution.
                                  #
                                  #   Note for /speckit.tasks: because oneshot.rs and pass_through.rs
                                  #   are each built out incrementally across User Story 1/2/3's own
                                  #   task phases, US2/US3's tasks are sequential-on-file with US1's,
                                  #   not independently implementable in isolation from it — an
                                  #   intentional deviation from the tasks-template's default
                                  #   story-independence guidance, driven by this single-file design.

tests/
├── cli_scaffold.rs          # UPDATED — 8 existing `oneshot`-stub assertions break against the real
│                             #   implementation: `t016`, `t016a` (asserted the retired stub `parsed`
│                             #   packages/pass-through envelope — removed outright, since FR-013
│                             #   guarantees no `oneshot` outcome ever produces that envelope once real,
│                             #   and this file has no `ALLEZ_CONDARC_PATH`-based isolation to exercise a
│                             #   real invocation deterministically; the packages-before-`--`/pass-through-
│                             #   after-`--` behavior they exercised moves to `tests/oneshot_exec.rs`),
│                             #   `t023a` (the one that asserts the literal `status: "stub"` JSON shape),
│                             #   plus `t023f`, `t023g`, `t038a`, `t038b` (`#[rstest]` cases *shared* with
│                             #   `run`/`sandbox`, which stay stubs — the `oneshot` case must be split out
│                             #   of each shared parametrization, not edited in place) and `t023i` (a
│                             #   plain, `oneshot`-only `#[test]`, not part of any shared parametrization).
│                             #   No redaction assertion can be preserved for `oneshot`: FR-013 forbids any
│                             #   `allez`-authored stdout once the pass-through starts, so there is no
│                             #   payload left in which to redact anything. `run`/`sandbox`'s own cases in
│                             #   the four tests that share a parametrization with `oneshot` (`t023f`,
│                             #   `t023g`, `t038a`, `t038b`) are unaffected and stay exactly as they are;
│                             #   the other four (`t016`, `t016a`, `t023a`, `t023i`) are `oneshot`-only, so
│                             #   this doesn't apply to them.
├── support/
│   ├── creation.rs          # UPDATED (one existing test) — `empty_package_list_installs_built_in_defaults`
│   │                         #   calls `create_ephemeral_environment` against the offline fixture channel
│   │                         #   and asserted the installed set equals the live `DEFAULT_PACKAGES` value;
│   │                         #   once `DEFAULT_PACKAGES` becomes `["python"]` (see `ephemeral/defaults.rs`
│   │                         #   above) this fails, since the fixture channel has no `python` package.
│   │                         #   Updated to assert `.unwrap_err()` naming `python`, proving the empty-list
│   │                         #   path reached `DEFAULT_PACKAGES` without requiring it to resolve offline.
│   └── defaults.rs          # UPDATED (one existing test) — same issue, same fix, for
│                             #   `an_override_resolving_to_empty_falls_back_to_default_packages`: kept its
│                             #   own empty override, updated to assert `.unwrap_err()` naming `python`
│                             #   rather than a successful install, so the fallback-to-default code path it
│                             #   exists to cover stays intact (never given a non-empty override, which
│                             #   would duplicate the existing override-is-used test instead).
└── oneshot_exec.rs          # NEW — end-to-end `assert_cmd` tests against the real compiled binary,
                              #   pointed at GEN-24's checked-in `tests/fixtures/ephemeral_channel/` via
                              #   the new, feature-gated `ALLEZ_CONDARC_PATH` env var (pointing directly at
                              #   a temporary condarc file — no fake `$HOME` directory needed) and
                              #   `ALLEZ_EPHEMERAL_ROOT`, both set through `Command::env(..)` on each
                              #   individual `assert_cmd::Command` — no shared process-wide state, so no
                              #   `#[serial]` is needed even though `cargo test` runs test functions
                              #   concurrently within one binary. Requires this literal `Cargo.toml`
                              #   addition (mirroring the existing `condarc_conformance` `[[test]]` entry's
                              #   own shape exactly):
                              #
                              #   [[test]]
                              #   name = "oneshot_exec"
                              #   path = "tests/oneshot_exec.rs"
                              #   required-features = ["test-config-override"]
```

**Structure Decision**: Extend the existing flat `src/cli/` module with one
new sibling file (`pass_through.rs`) rather than a new module directory or
workspace member — this feature's entire new surface area is "how to run
one already-resolved pass-through command inside one already-ready
environment," small, single-purpose, and used from exactly one call site
(`oneshot.rs`). No future consumer is assumed: `run`/`sandbox` are
explicitly out of this ticket's own scope per spec Assumptions, and
`sandbox`'s own prior process-lifecycle ticket (GEN-28) was closed with no
action, its scope moved to a separate tool (`ana`). `pass_through.rs`
(and its types, `PassThroughFailure`/`OneshotOutcomeEvent`) is declared
via `pub mod pass_through;` in `src/cli/mod.rs`, matching the six existing
sibling subcommand modules' own visibility — not `pub(crate)`, because
`main.rs` is a separate binary crate that needs cross-crate access to
consume these types in `dispatch` (see `data-model.md`'s `OneshotOutcome`
note for why `pub(crate)` doesn't work here). This is a visibility
consequence of the workspace's existing lib/bin split, not a signal that
`pass_through.rs`'s functions are meant for any consumer beyond
`oneshot.rs` itself.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| `unsafe` FFI call in `src/cli/pass_through.rs` (Windows: `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child_pid)` via `windows-sys`, attempting best-effort graceful forwarding of an intercepted Ctrl-Break to the child's process group before falling back to `child.kill()`; an intercepted Ctrl-C always calls `child.kill()` directly and never attempts this FFI call at all, since `CTRL_C_EVENT` can't be safely group-scoped — see `research.md`) | FR-014 requires forwarding an interceptable termination signal to the pass-through command "on a platform where that concept exists"; Windows's nearest equivalent (console control events) is only reachable via the raw Win32 FFI — there is no safe `std`/`tokio` abstraction for it. | Skipping graceful forwarding and always hard-killing the child on Windows was considered and rejected: it would make Windows silently weaker than Unix for the exact behavior FR-014 asks for, when a real, bounded forwarding path exists for Ctrl-Break specifically (see `research.md`'s `watchexec`/`zellij` precedent). Shares GEN-24's existing Win32-FFI `// SAFETY:` exception category
(Constitution Check, Row I; GEN-24's `permissions.rs`/`paths.rs`/
`cleanup.rs`) — a fourth site, not a new exception. The `// SAFETY:` comment MUST document: (1) the FFI call happens while this call's own spawned `Child` is still unreaped, so no concurrent drop races it — this reduces, but per Microsoft's own docs does not fully eliminate, the risk of the OS recycling `child_pid` after termination, so the liveness recheck in (3) never trusts the PID alone; (2) the child was created with `CREATE_NEW_PROCESS_GROUP` so the event targets its group without also signaling `allez`'s own process, and the child must share `allez`'s own console session for delivery to be possible at all (no `CREATE_NO_WINDOW`/detached-console spawn); (3) a falsy `BOOL` return, or a still-running child observed `100ms` after a truthy one, both fall back to `child.kill()` rather than assuming success or looping indefinitely. |
| Plan-level interpretation: `ActivationError` (from `ReadyEnvironment::activation_environment()`, GEN-24) mapped to a new, additive `activation_failed` category under exit code `1` — FR-010 names exactly four environment-creation-failure categories and not this one | `activation_environment()` can fail (GEN-24's own test proves this reachable: malformed `conda-meta/state` JSON) after the environment already exists but before the pass-through program can be looked up — it fits neither FR-010's four categories (the environment was created successfully) nor FR-008's two (the program was never reached). FR-010's list is scoped to "environment-creation failure," a state this case never reaches, so this fills a gap the spec doesn't address rather than extending a closed enumeration — ordinary implementation detail, not a spec.md amendment. | Reusing `EphemeralEnvError::UnwritableLocation` (closest existing wording, same exit-code family) was rejected: it would misleadingly suggest the environment's own creation was at fault, when creation already succeeded. Treating it as a `pass_through_*` (FR-008) failure was also rejected: no attempt was made to locate or execute the program, so FR-008's category names would be factually wrong. `PassThroughFailure::ActivationFailed` carries no wrapped message (research.md's redaction decision) — `rattler_shell`'s raw activation-error text can include the environment's own path, which FR-015 forbids disclosing; both the caller-facing message and the tracing record use one fixed string instead. |
