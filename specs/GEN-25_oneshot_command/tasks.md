---

description: "Task list template for feature implementation"
---

# Tasks: `allez oneshot` Command

**Input**: Design documents from `/specs/GEN-25_oneshot_command/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/oneshot_cli_contract.md, quickstart.md (all present)

**Tests**: Included. The project constitution (Principle II: TDD, Principle VIII: mandatory 100% spec test coverage) requires tests written before/alongside implementation; quickstart.md's acceptance-scenario → test mapping is the source of truth for which tests exist.

**Organization**: Tasks are grouped by user story (spec.md's User Story 1/2/3) to enable independent implementation and testing of each story, per plan.md's Project Structure and research.md's decisions.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: US1, US2, or US3 (maps to spec.md's three user stories)
- File paths are exact, taken from plan.md's Project Structure table and the current repository state.

## Path Conventions

Single Rust package (`allez`) within the existing `allez`/`crates/condarc` workspace — no new workspace member. All paths below are relative to the repository root.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Wire the new Cargo feature flag, dependency features, and CI/Makefile plumbing this ticket needs before any new code can compile or run under test.

- [ ] T001 In `Cargo.toml`: add the `process` and `signal` features to the existing `tokio` dependency entry; add `Win32_System_Console` to the `[target.'cfg(windows)'.dependencies]` `windows-sys` feature list (alongside the existing `Win32_System_Threading`, already present); add a new `[features]` entry `test-config-override = []` (not part of any `default` list — there is none today); add a new `[[test]]` block (mirroring the existing `condarc_conformance` entry's shape) — `name = "oneshot_exec"`, `path = "tests/oneshot_exec.rs"`, `required-features = ["test-config-override"]`.
- [ ] T002 [P] In `Makefile`: change the `test` target from `cargo test --all` to `cargo test --all --features test-config-override`.
- [ ] T003 [P] In `.github/workflows/ci.yml`: append `--features test-config-override` to the `test` job's `cargo test --all --locked` step (all four matrix legs) and to the `coverage` job's `cargo llvm-cov --all --locked --fail-under-lines 85` step.

**Checkpoint**: `cargo check`/`cargo build` still succeed with the new feature flag defined (no code reads it yet); CI/Makefile invocations already request it.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: The async runtime entry point, the new error/outcome types, and the default-package/test-isolation seam every user story's implementation builds on.

**⚠️ CRITICAL**: No user story implementation task can begin until this phase is complete.

- [ ] T004 In `src/main.rs`: promote `fn main()` to `#[tokio::main(flavor = "multi_thread")] async fn main()`; make `dispatch` an `async fn`; every non-`Oneshot` `dispatch` arm keeps calling its existing synchronous handler unchanged (legal from inside an `async fn`, no behavior change for those five arms).
- [ ] T005 [P] In `src/cli/mod.rs`: add `pub mod pass_through;` alongside the six existing sibling `pub mod` declarations (`create`, `list`, `oneshot`, `remove`, `run`, `sandbox`).
- [ ] T006 [P] In `src/ephemeral/defaults.rs`: change `DEFAULT_PACKAGES` from `&["fixture-default-alpha", "fixture-default-beta"]` to `&["python"]` (research.md's documented GEN-30 stopgap); this file's own `#[cfg(test)]` assertions derive from the `DEFAULT_PACKAGES` constant itself, not a hard-coded literal, and need no edit. Update this constant's own doc comment too — it currently describes the value as "matching `tests/fixtures/ephemeral_channel/`'s dependency-free noarch packages," which becomes false once the value is `["python"]`; replace it with a doc comment describing the `["python"]`/GEN-30-stopgap rationale instead.
- [ ] T006a In `tests/support/creation.rs` (`empty_package_list_installs_built_in_defaults`) and `tests/support/defaults.rs` (`an_override_resolving_to_empty_falls_back_to_default_packages`): both currently `.unwrap()` a `create_ephemeral_environment` call and assert the installed set equals `DEFAULT_PACKAGES`, resolved live against the offline fixture channel — under T006's `["python"]` change this now fails, since the fixture channel has no `python` package. Update both to `.unwrap_err()` instead, asserting the resulting `EphemeralEnvError::UnresolvablePackage` names `python` — proving each test's own code path (an empty explicit list; an override that itself resolves to empty) correctly reached and used `DEFAULT_PACKAGES` before failing, without requiring `python` to actually resolve offline. This is the same "prove the code path was reached via the resulting failure's identity" pattern quickstart.md's own scenario 1.2 already uses for the identical fixture-channel limitation. Do not substitute a fixture-resolvable package via `default_override` for either test: doing so for `an_override_resolving_to_empty_falls_back_to_default_packages` specifically would give it a non-empty override, eliminating the exact empty-override-falls-back-to-default code path it exists to cover, and duplicating the already-existing `no_packages_with_an_override_installs_the_override_instead_of_defaults`.
- [ ] T007 [P] In `src/channel_config/mod.rs`: add a new, `#[cfg(feature = "test-config-override")]`-gated check inside `default_condarc_path()` that reads the `ALLEZ_CONDARC_PATH` environment variable first, falling back to the existing `dirs::home_dir().map(|home| home.join(".condarc"))` when unset or when the feature is not compiled in; a release build compiled without `test-config-override` has no code path that reads `ALLEZ_CONDARC_PATH` at all.
- [ ] T008 Create `src/cli/pass_through.rs` (new file) with the `PassThroughFailure` enum from data-model.md: `#[non_exhaustive]` with variants `NotFound`, `NotExecutable`, `TerminatedBySignal { signal: i32 }`, `ActivationFailed`, `SignalSetupFailed`; implement `std::fmt::Display`/`std::error::Error`; implement `crate::error::CategorizedError::category()` returning `"pass_through_not_found"`/`"pass_through_not_executable"`/`"pass_through_terminated_by_signal"`/`"activation_failed"`/`"signal_setup_failed"` respectively; implement the inherent `exit_code(&self) -> i32` method (`127`/`126`/`128 + signal`/`1`/`1` per data-model.md's table, each its own match arm — do not fold `ActivationFailed`/`SignalSetupFailed` into `NotExecutable`'s arm).
- [ ] T008a In `src/cli/pass_through.rs`, add a `#[cfg(test)] mod tests` block unit-testing `PassThroughFailure::category()` and `exit_code()` for all five variants (`NotFound`→`"pass_through_not_found"`/`127`, `NotExecutable`→`"pass_through_not_executable"`/`126`, `TerminatedBySignal { signal: 15 }`→`"pass_through_terminated_by_signal"`/`143`, `ActivationFailed`→`"activation_failed"`/`1`, `SignalSetupFailed`→`"signal_setup_failed"`/`1`) — no real child process required (Constitution II: unit tests co-located with the code under test, per plan.md's own Constitution Check commitment).
- [ ] T009 In `src/cli/oneshot.rs` (overwrite the existing stub): define the `OneshotOutcome` enum from data-model.md — `EnvironmentCreationFailed { message: String }`, `PassThroughFailed { message: String, failure: PassThroughFailure }`, `PassThroughExited { exit_code: i32 }` — and its `exit_code(&self) -> i32` method (`1` / `failure.exit_code()` / `*exit_code` respectively; no stored `exit_code` field on the first two variants).
- [ ] T009a In `src/cli/oneshot.rs`, add a `#[cfg(test)] mod tests` block unit-testing `OneshotOutcome::exit_code()` for all three variants (`EnvironmentCreationFailed { .. }`→`1`, `PassThroughFailed { failure: PassThroughFailure::NotFound, .. }`→`127` and at least one other wrapped `PassThroughFailure` variant, `PassThroughExited { exit_code: 37 }`→`37`) — constructed directly, no real environment/process required (Constitution II).
- [ ] T010 In `src/main.rs`: add a `schema_version` field (the same value `OneshotOutcomeEvent`, T049, will use) to `exit_on_invalid_pass_through`'s existing `tracing::warn!(operation, category = err.category(), ...)` call — shared by `run`/`sandbox` too, since they call the same function.
- [ ] T011 [P] In `src/output.rs`: add `render_ephemeral_creation_failure(failure: &crate::ephemeral::CreationFailure, human: bool) -> String`, additive to the existing `{schema_version, category, message}` JSON shape with two optional fields (`cleanup_category`, `cleanup_message`) populated only when `failure.cleanup_error` is `Some`; human mode reuses `CreationFailure`'s own existing `Display` impl verbatim (already produces `"{error} (cleanup also failed: {cleanup})"`). Do not modify `render_error`/`render_success`/`render_pass_through`.

**Checkpoint**: Foundation ready — `cargo build` succeeds with the new types/module in place (even though nothing calls them from a real code path yet); user story implementation can now begin.

---

## Phase 3: User Story 1 - Run a one-off command against exactly the packages it needs (Priority: P1) 🎯 MVP

**Goal**: `allez oneshot [PACKAGES]... -- <COMMAND> [ARGS...]` creates a fresh ephemeral environment populated with the requested (or default) packages, activates it (PATH-first), and starts `<COMMAND>` with its own arguments unchanged — before the command is ever started, and independently of what the command itself does.

**Independent Test**: Invoke `allez oneshot pkg1 pkg2 -- some-command`; confirm a new ephemeral environment is created and populated with `pkg1`/`pkg2` (and dependencies) before `some-command` starts.

### Tests for User Story 1

- [ ] T012 [US1] Create `tests/oneshot_exec.rs` (new file) with the fixture-condarc test helper from quickstart.md: `#[path = "support/ephemeral.rs"] mod support;`, a helper that writes a temp `.condarc` file whose `channels:` entry is `support::fixture_channel("")`, and a temp `ALLEZ_EPHEMERAL_ROOT` directory, both set via `assert_cmd::Command::env(...)` per invocation (no process-wide `std::env::set_var`).
- [ ] T013 [US1] In `tests/oneshot_exec.rs`: scenario 1.1 — run `oneshot fixture-probe -- fixture-probe` (Unix) / `fixture-probe.cmd` (Windows) against the fixture channel; assert the bare-name program actually runs and exits `0`, proving the environment's own executable is installed and reachable via `PATH` before the pass-through program starts.
- [ ] T014 [US1] In `tests/oneshot_exec.rs`: scenario 1.2 — run `oneshot -- echo hi` (zero packages) against the fixture channel; assert exit `1`, category `unresolvable_package` (not `missing_pass_through_command`/exit `2`), proving `RequestedPackages::UseDefaultOrOverride` reached package resolution rather than failing as a usage error.
- [ ] T015 [US1] In `tests/oneshot_exec.rs`: scenario 1.3 — run `oneshot fixture-default-alpha -- <program printing its own environment's prefix path>` twice; assert the two printed prefix paths differ (FR-002, no cross-invocation sharing).
- [ ] T016 [US1] In `tests/oneshot_exec.rs`: scenario 1.4 — run `oneshot fixture-default-alpha -- echo "hello world" '$HOME'` on Unix / a platform-equivalent literal-argument-preserving program on Windows (`echo` is a `cmd.exe` builtin there, not an executable `allez` could spawn directly); assert the child's stdout contains those exact, unshell-expanded tokens (proving no intermediate shell interprets them).
- [ ] T017 [US1] In `tests/oneshot_exec.rs`: scenario 1.5 — plant a decoy `fixture-probe` executable (exiting with a distinct code) on the test's own `PATH`, then run `oneshot fixture-probe -- fixture-probe`; assert the environment's own copy ran (not the host decoy), proving FR-003's PATH-first activation preference.

### Implementation for User Story 1

- [ ] T018 [US1] In `src/cli/oneshot.rs`: implement `pub async fn run(args: &PackagesAndCommandArgs, human: bool, verbose: bool) -> OneshotOutcome` — `RequestedPackages::from_cli(args.packages.clone())`, mapping a returned `InvalidPackageSpec` directly to `EphemeralEnvError::UnresolvablePackage { package: input }` per research.md (never a usage error); `channel_config::resolve_channel_config()`, mapping `ChannelConfigResolution::NoChannels` to `EphemeralEnvError::NoChannelsConfigured`; then `create_ephemeral_environment(requested, channels, None)` (the `None` `default_override` per research.md — no override-authoring mechanism exists yet); on `Err(CreationFailure)`, render via `output::render_ephemeral_creation_failure` and return `OneshotOutcome::EnvironmentCreationFailed { message }`.
- [ ] T019 [US1] In `src/cli/pass_through.rs`: implement a function that builds a `tokio::process::Command` for the pass-through program/args, calling `ReadyEnvironment::activation_environment()` and applying its overlay via one `.env(key, value)` call per pair on top of `Command`'s own default full-environment inheritance (never `.env_clear()`), satisfying FR-003's "merge, not replace."
- [ ] T020 [US1] In `src/cli/pass_through.rs`: implement `pub(crate) async fn run_pass_through(environment: &ReadyEnvironment, pass_through: &PassThroughArgs) -> OneshotOutcome` covering only the happy path for this story — map an `ActivationError` from T019's activation call to `OneshotOutcome::PassThroughFailed { failure: PassThroughFailure::ActivationFailed, .. }` (fixed, non-path-bearing message per research.md's redaction decision); otherwise `.spawn()` the built `Command` (stdio left at `tokio::process::Command`'s inherited default — no explicit `.stdin`/`.stdout`/`.stderr` calls) and `.wait()` for it, mapping a normal exit (`status.code() == Some(code)`) to `OneshotOutcome::PassThroughExited { exit_code: code }`. Spawn-error classification (T046) and signal-based classification (T034/T036) are added in later stories — for now, a spawn `Err` may map through the same `NotExecutable` fallback arm.
- [ ] T021 [US1] In `src/cli/oneshot.rs`: wire T018's success path — on `Ok(environment)` from `create_ephemeral_environment`, call T020's `pass_through::run_pass_through(&environment, &args.pass_through)` and return its `OneshotOutcome` directly.
- [ ] T022 [US1] In `src/main.rs`: change the `Commands::Oneshot(args)` `dispatch` arm to `.await` `cli::oneshot::run(&args, cli.human, cli.verbose)`, match the returned `OneshotOutcome`, and call `std::process::exit(outcome.exit_code())` — printing `message` to stderr (via the existing `--human`-aware rendering already produced by T018/T020) for `EnvironmentCreationFailed`/`PassThroughFailed`, printing nothing for `PassThroughExited` (FR-013's "no envelope" rule) — this arm no longer calls the shared `emit_stub_success` helper the other five arms still use.

### Fix existing tests broken by the real (non-stub) implementation

- [ ] T023 [US1] In `tests/cli_scaffold.rs`: remove `t016_oneshot_with_packages_and_command_identifies_parsed_values` and `t016a_oneshot_zero_packages_is_valid_not_error` — both assert the retired stub `parsed` envelope (`v["parsed"]["packages"]`/`v["parsed"]["pass_through"]`), and FR-013/`contracts/oneshot_cli_contract.md` guarantee no `oneshot` outcome ever produces that envelope once real; there is no contract-legal replacement assertion to substitute here, since `run_allez` sets no `ALLEZ_CONDARC_PATH`/`ALLEZ_EPHEMERAL_ROOT` and `cli_scaffold.rs` is not gated behind `test-config-override` — a real `oneshot pkg1 pkg2 -- echo hello` invocation in this file would attempt an uncontrolled solve against the invoking machine's own `~/.condarc` rather than the fixture channel. This same isolation gap applies to T024/T025/T026/T027's own `oneshot` cases (`t023a`, `t023f`, `t023g`, `t023i`, `t038a`, `t038b`): none of them may assert a real, started environment/pass-through outcome either — only pre-start usage-error/parse-rejection behavior that never reaches `create_ephemeral_environment`. The packages-before-`--`/pass-through-after-`--` behavior these two tests exercised is covered instead by `tests/oneshot_exec.rs`'s own isolated scenarios (T013-T017).
- [ ] T024 [US1] In `tests/cli_scaffold.rs`: update `t023a_oneshot_default_json_matches_fixed_shape` — the literal `status: "stub"` assertion no longer applies to `oneshot` (FR-013: no envelope once the pass-through program starts); like T023's `t016`/`t016a`, this test's own `oneshot pkg1 -- echo hi` invocation has the same `run_allez` isolation gap (no `ALLEZ_CONDARC_PATH`/`ALLEZ_EPHEMERAL_ROOT`), so removal — not a replacement assertion requiring a real, isolated environment — is the likely correct outcome here too; replace with an assertion appropriate to the real invocation shape only if one exists that needs no isolation, or remove if none does.
- [ ] T025 [US1] In `tests/cli_scaffold.rs`: split the `oneshot` case out of the shared `#[rstest]` parametrizations in `t023f_pass_through_redacted_by_default_json_and_human` and `t023g_verbose_reveals_unredacted_pass_through_json_and_human` — `run`/`sandbox`'s own cases stay exactly as they are; `oneshot`'s case is rewritten (or removed, if no longer applicable given FR-013) as its own dedicated test.
- [ ] T026 [US1] In `tests/cli_scaffold.rs`: split the `oneshot` case out of the shared `#[rstest]` parametrizations in `t038a_empty_separator_and_flag_like_token_preservation_uniform_across_pass_through_subcommands` and `t038b_global_flag_spelled_tokens_after_separator_are_forwarded_verbatim_not_reinterpreted` the same way.
- [ ] T027 [US1] In `tests/cli_scaffold.rs`: update `t023i_human_flag_before_subcommand_matches_after_for_oneshot_with_args` (a plain, `oneshot`-only test, not part of any shared parametrization) to match the real implementation's output.

**Checkpoint**: `allez oneshot` creates a real ephemeral environment, activates it, and runs the pass-through command to completion with its real exit code on the happy path — independently testable per User Story 1's own acceptance scenarios. Signal handling and non-`NotFound`/`NotExecutable` failure classification land in the next two phases.

---

## Phase 4: User Story 2 - See the command's real output and get back its real exit code (Priority: P1)

**Goal**: The pass-through command's stdout/stderr stream live (kept separate), stdin forwards unchanged, an interceptable termination signal reaching `allez` is forwarded to the child rather than left running detached, and a signal-terminated child reports the documented `128 + N` exit code.

**Independent Test**: Run a pass-through command that produces output incrementally and exits with a specific non-zero code; confirm output is visible before the command finishes and `allez`'s own exit code matches exactly.

### Tests for User Story 2

- [ ] T028 [US2] In `tests/oneshot_exec.rs`: scenario 2.1 — run `oneshot fixture-default-alpha -- <script that writes stdout, sleeps, writes stderr, sleeps>` with a timeout shorter than the total sleep; assert the first write is observable before the process exits, on the correct stream.
- [ ] T029 [US2] In `tests/oneshot_exec.rs`: scenario 2.2 — run `oneshot fixture-default-alpha -- <a pass-through command that exits with a specific code>` (e.g. `sh -c 'exit 37'`, or platform equivalent); assert `allez`'s own exit code is exactly `37`.
- [ ] T030 [US2] In `tests/oneshot_exec.rs`: scenario 2.3 — pipe known bytes into `allez oneshot fixture-default-alpha -- cat` (or platform equivalent); assert the child's stdout echoes them.
- [ ] T031 [US2] In `tests/oneshot_exec.rs` (`#[cfg(unix)]` only): scenario 2.4 — run `oneshot fixture-default-alpha -- <a script that traps SIGTERM and exits 99>`; send the running `allez` process `SIGTERM`; assert `allez` doesn't exit until the child does, and the final exit code is `99` (not `128+15`).
- [ ] T032 [US2] In `tests/oneshot_exec.rs` (`#[cfg(unix)]` only): scenario 2.5 — run `oneshot fixture-default-alpha -- sh -c 'kill -TERM $$'` (self-signaling child); assert `allez`'s own exit code is `143` (`128+15`), stdout/stderr carry no `allez`-authored message or category (FR-013), and `pass_through_terminated_by_signal` appears only via the `RUST_LOG=debug` tracing channel.

### Implementation for User Story 2

- [ ] T033 [US2] In `src/cli/pass_through.rs`: before building/spawning the `Command`, register termination-signal listeners — Unix: `tokio::signal::unix::signal(SignalKind::interrupt())`/`::terminate()`/`::hangup()`/`::quit()` (`SIGINT`/`SIGTERM`/`SIGHUP`/`SIGQUIT`); Windows: `tokio::signal::windows::ctrl_c()`/`::ctrl_break()`; map any listener-registration `io::Result::Err` to `PassThroughFailure::SignalSetupFailed` before ever calling `.spawn()`.
- [ ] T034 [US2] In `src/cli/pass_through.rs`: replace T020's plain `.wait()` with a `tokio::select!` loop racing `Child::wait()` against T033's signal listeners; on Unix, forward via `rustix::process::kill_process(child_pid, signal)` targeting the direct child PID only (no process-group change, no `.process_group(0)` at spawn); after forwarding, continue waiting for the child's real outcome — never exit early.
- [ ] T035 [US2] In `src/cli/pass_through.rs` (`#[cfg(windows)]`): spawn the child with `.creation_flags(CREATE_NEW_PROCESS_GROUP)`; factor the fallback decision into its own function (not inline in the `tokio::select!` handler), so T035a can call it directly without a real console-control event: on an intercepted `Ctrl-Break`, attempt `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child_pid)` (new `unsafe` FFI call, `windows-sys`, with a `// SAFETY:` comment per Complexity Tracking) — if the returned `BOOL` is truthy, wait a fixed, named `100ms` constant then check once whether the child is still running, falling back to `child.kill()` if so; on an intercepted `Ctrl-C`, always call `child.kill()` directly (never attempt `CTRL_C_EVENT`, per research.md's Windows Ctrl-C limitation).
- [ ] T035a [US2] In `src/cli/pass_through.rs`, add a `#[cfg(windows)] #[cfg(test)]` unit test for T035's fallback-to-`child.kill()` decision function, exercised directly against a real child process this test spawns independently — not via a real console-control event sent to a running `allez` subprocess, since a hard-terminated `allez` process never runs the intercepted-signal handler `child.kill()` lives inside, and would leave that child orphaned and still running rather than gone. Cover all three paths T035 defines: the unconditional Ctrl-C direct-kill path, the Ctrl-Break-plus-falsy-`BOOL` fallback, and the Ctrl-Break-plus-truthy-`BOOL`-plus-still-running-after-the-`100ms`-recheck fallback — asserting the test's own spawned child is no longer running after each path runs. No attached console session or real signal delivery to `allez` itself is required.
- [ ] T036 [US2] In `src/cli/pass_through.rs`: implement the exit-code classification from `ExitStatus` per research.md's decision — Unix: `status.code()` `Some(code)` is a normal exit; `None` uses `ExitStatusExt::signal()` to compute `128 + signal` (an unreachable `None` fallback returns a fixed `128`, never `.unwrap()`); Windows: `unreachable!()` guarded by `#[cfg(windows)]`, since `ExitStatus::code()` is documented to always return `Some(_)` there. A signal-terminated outcome maps to `OneshotOutcome::PassThroughExited { exit_code: 128 + signal }` — **never** `OneshotOutcome::PassThroughFailed` (data-model.md's construction rule) — though `PassThroughFailure::TerminatedBySignal { signal }`'s `category()`/message are still produced for T049's tracing event only.
- [ ] T036a [US2] In `src/cli/pass_through.rs`, add a `#[cfg(unix)] #[cfg(test)]` unit test for T036's `ExitStatus`→exit-code classification: construct an `ExitStatus` via `std::os::unix::process::ExitStatusExt::from_raw` for a normal exit and for a few representative signals (e.g. `SIGTERM`=15, `SIGKILL`=9), asserting the `128 + signal` mapping — no real child process spawn required (Constitution II). Windows has nothing to unit-test here (`unreachable!()` per T036).

**Checkpoint**: Streaming, stdin forwarding, signal forwarding, and both normal- and signal-based exit-code propagation are all correct and independently testable per User Story 2's acceptance scenarios.

---

## Phase 5: User Story 3 - Get a clear, distinct failure when the environment itself can't be built (Priority: P2)

**Goal**: An environment-creation failure, a pass-through-program-could-not-be-started failure, and a started-then-terminated outcome are always distinguishable by category and exit code — never conflated.

**Independent Test**: Request a package that cannot be resolved; confirm the invocation fails with a clear message/category before the pass-through command is ever started.

### Tests for User Story 3

- [ ] T037 [US3] In `tests/oneshot_exec.rs`: scenario 3.1 — run `oneshot definitely-nonexistent-package-xyz -- echo hi`; assert exit `1`, category `unresolvable_package`, and that `echo`'s own output (`"hi"`) never appears in `allez`'s stdout.
- [ ] T038 [US3] In `tests/oneshot_exec.rs`: scenario 3.1a — point `ALLEZ_CONDARC_PATH` at a condarc whose `denylist_channels` denies its own `channels:` entry (mirroring GEN-24's `deny_filtered_channel_list_returns_no_channels` fixture shape); run `oneshot fixture-default-alpha -- echo hi`; assert exit `1`, category `no_channels_configured`.
- [ ] T039 [US3] In `tests/oneshot_exec.rs`: scenario 3.1b — run `oneshot fixture-corrupt-checksum -- echo hi` against the checked-in `fixture-corrupt-checksum` package (`tests/fixtures/ephemeral_channel/noarch/`); assert exit `1`, category `integrity_verification_failed`.
- [ ] T040 [US3] In `tests/oneshot_exec.rs` (`#[cfg(unix)]` only): scenario 3.1c — `chmod 000` the `ALLEZ_EPHEMERAL_ROOT` directory before invoking `oneshot fixture-default-alpha -- echo hi`; assert exit `1`, category `unwritable_location`; use an RAII guard (not a plain post-assertion statement) to restore the directory to a removable mode even on assertion failure/panic, so the temp-dir guard's own silent-on-error `Drop` never leaks a `000`-mode directory.
- [ ] T041 [US3] In `tests/oneshot_exec.rs`: scenario 3.2 — run `oneshot fixture-default-alpha -- definitely-nonexistent-binary-xyz`; assert exit `127`, category `pass_through_not_found`.
- [ ] T042 [US3] In `tests/oneshot_exec.rs` (`#[cfg(unix)]` only): scenario 3.2a — write a file with mode `0o644` (no execute bit) into a temp directory, run `oneshot fixture-default-alpha -- <that file's absolute path>`; assert exit `126`, category `pass_through_not_executable`.
- [ ] T043 [US3] In `tests/oneshot_exec.rs` (`#[cfg(unix)]` only): scenario 3.3 — reproduce a dual failure deterministically: first check `tests/support/failures.rs` for a GEN-24-owned helper that already provokes `EphemeralEnvError`+`TeardownFailed` together (mirroring the shape `deny_filtered_channel_list_returns_no_channels` and similar fixtures already use) and reuse it verbatim; only if none exists, construct one — e.g. let package resolution fail against an unresolvable package so `fail_and_roll_back` (`src/ephemeral/mod.rs`) runs its own cleanup, and `chmod 000` the environment's own prefix directory between directory creation and cleanup so that cleanup itself fails. Assert both `category` (the original creation failure) and `cleanup_category: "teardown_failed"` are present and distinct in the JSON body; restore directory permissions via an RAII guard before the test's temp-dir drops (same pattern as T040).
- [ ] T044 [US3] In `tests/cli_scaffold.rs`: confirm the existing `t034_oneshot_empty_separator_exits_2_missing_pass_through_command` test (scenario 3.4) still passes unchanged against the real implementation; re-run only, do not rewrite.
- [ ] T045 [US3] In `tests/cli_scaffold.rs`: confirm (or add, if missing) a test asserting `oneshot pkg1` with no `--` token anywhere in argv is rejected with `MissingPassThroughCommand`/exit `2`/`missing_pass_through_command` (scenario 3.5 — a distinct clap-parsed state from 3.4's empty-separator case, per `cli::validate_pass_through`'s existing behavior).

### Implementation for User Story 3

- [ ] T046 [US3] In `src/cli/pass_through.rs`: classify `tokio::process::Command::spawn()`'s `Err(io::Error)` by `.kind()` per research.md's table — `io::ErrorKind::NotFound` → `PassThroughFailure::NotFound` (exit `127`); every other kind (including `PermissionDenied`) → `PassThroughFailure::NotExecutable` (exit `126`).
- [ ] T046a [US3] In `src/cli/pass_through.rs`, add a `#[cfg(test)] mod tests` unit test for T046's `io::ErrorKind`→`PassThroughFailure` classification: construct `io::Error::from(io::ErrorKind::NotFound)` and assert it maps to `PassThroughFailure::NotFound`; construct `io::Error::from(io::ErrorKind::PermissionDenied)` and at least one other `ErrorKind`, asserting both map to `PassThroughFailure::NotExecutable` — no real `.spawn()` call required (Constitution II).
- [ ] T047 [US3] In `src/cli/oneshot.rs`: wire T018/T020's pre-start `PassThroughFailure` variants (`NotFound`, `NotExecutable`, `ActivationFailed`, `SignalSetupFailed` — the four pre-start-only variants per data-model.md's construction rule) into `OneshotOutcome::PassThroughFailed { message, failure }`, with `message` rendered via `output::render_error(failure.category(), &failure.to_string(), human)`.
- [ ] T048 [US3] In `src/cli/oneshot.rs`: wire T011's `render_ephemeral_creation_failure` into T018's `Err(CreationFailure)` branch so the dual-failure case (a failed creation attempt whose own rollback also failed) renders both `category`/`message` and the additive `cleanup_category`/`cleanup_message` fields, never masking either.
- [ ] T049 [US3] In `src/cli/pass_through.rs`: define the `OneshotOutcomeEvent` struct from data-model.md (`schema_version`, `invocation_id: EnvironmentId`, `pass_through_started: bool`, `exit_code: Option<i32>`, `failure_category: Option<&'static str>`, `message: Option<String>`, `cleanup_category: Option<&'static str>`, `cleanup_message: Option<String>`), and emit exactly one such event via `tracing` per invocation that reaches `oneshot::run` — immediately after `create_ephemeral_environment` resolves if it failed, or immediately after the pass-through outcome is known otherwise — never both, never while the pass-through program is running.
- [ ] T049a [US3] In `tests/oneshot_exec.rs`, with `RUST_LOG=debug`: for (a) an environment-creation failure (reuse scenario 3.1's invocation, T037) and (b) a normal successful pass-through exit (reuse scenario 1.1's invocation, T013), assert exactly one `OneshotOutcomeEvent`-shaped tracing record is emitted per invocation and that it carries a non-empty `schema_version` field — closing the gap T032 only covers for the signal-terminated case (SC-009's "100% of invocations ... produce a structured, schema-versioned record").

**Checkpoint**: All three user stories are independently functional and testable; every failure category from FR-007/FR-008/FR-010/FR-011 is distinct and correctly reported.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Cross-cutting acceptance scenarios (FR-003/FR-009/FR-015) not owned by any single user story, plus final documentation/coverage verification.

- [ ] T050 [P] In `tests/oneshot_exec.rs`: scenario C.1 — set an arbitrary marker variable (e.g. `MY_TEST_MARKER=xyz`) on the test's own `assert_cmd::Command`, run `oneshot fixture-default-alpha -- <program printing that variable>`, assert its value reaches the child unchanged (FR-003/SC-007).
- [ ] T051 [P] In `tests/oneshot_exec.rs`: scenario C.2 — after a successful run, a normal non-zero pass-through exit, and (`#[cfg(unix)]`) a signal-terminated pass-through, inspect the test's own known `ALLEZ_EPHEMERAL_ROOT` directory directly and assert the created environment's files are still present in each case (FR-009/SC-006).
- [ ] T052 [P] In `tests/oneshot_exec.rs`: scenario C.3 — for both a successful run and a pre-start environment-creation failure, assert `allez`'s own stdout/stderr never contains the `ALLEZ_EPHEMERAL_ROOT` path string or the environment's own ULID (FR-015/SC-010).
- [ ] T053 Run `make doc` (`RUSTDOCFLAGS='-D warnings' cargo doc -p condarc -p allez --lib --no-deps --locked`); add missing doc comments to any new public/`pub(crate)` item in `src/cli/pass_through.rs`/`src/cli/oneshot.rs`/`src/output.rs` until it builds clean (Constitution VI).
- [ ] T054 Run `make test` (`cargo test --all --features test-config-override`) end-to-end across every task above; fix any remaining failure before considering this feature complete. Manually walk through quickstart.md's own "Set up a fixture-pointing test condarc" section once to confirm the documented reproduction steps still match the shipped behavior.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately.
- **Foundational (Phase 2)**: Depends on Setup (needs the `test-config-override`/dependency-feature groundwork) — BLOCKS all user stories.
- **User Story 1 (Phase 3)**: Depends on Foundational completion. No dependency on US2/US3.
- **User Story 2 (Phase 4)**: Depends on Foundational completion **and** US1's `src/cli/pass_through.rs` skeleton (T019/T020) — extends the same file/functions rather than starting fresh. Not independently implementable before US1's spawn/wait scaffolding exists.
- **User Story 3 (Phase 5)**: Depends on Foundational completion **and** US1's `src/cli/oneshot.rs`/`pass_through.rs` scaffolding (same reason as US2). Independent of US2's signal-forwarding code (different match arms in the same files).
- **Polish (Phase 6)**: Depends on US1 + US2 + US3 all being complete.

### Within Each User Story

- Tests are written first per story (Constitution II); implementation tasks follow.
- `tests/oneshot_exec.rs` is a single file appended to across all three stories — later stories' test tasks are sequential with respect to that file, not parallel with each other.
- `src/cli/pass_through.rs` and `src/cli/oneshot.rs` are each edited across all three stories — same sequencing caveat applies to their own implementation tasks.

### Parallel Opportunities

- T002/T003 (Makefile/ci.yml) can run in parallel with each other, and with T001 once T001's `Cargo.toml` feature name (`test-config-override`) is decided.
- T005/T006/T007/T011 (Foundational) touch four independent files (`cli/mod.rs`, `ephemeral/defaults.rs`, `channel_config/mod.rs`, `output.rs`) and can run in parallel with each other; T004/T008/T009/T010 are sequential with respect to `main.rs`/the new `pass_through.rs`/`oneshot.rs` types they define or depend on. T006a touches two further-independent files (`tests/support/creation.rs`, `tests/support/defaults.rs`) but is sequential-after T006, not parallel with it, since it fixes tests T006's own `DEFAULT_PACKAGES` change breaks.
- T050/T051/T052 (Polish cross-cutting tests) can run in parallel with each other (each is an independent scenario, though all append to the same `tests/oneshot_exec.rs` file — coordinate merge order).

---

## Parallel Example: Foundational Phase

```bash
# Launch independent Foundational file edits together:
Task: "Add DEFAULT_PACKAGES = [\"python\"] in src/ephemeral/defaults.rs"
Task: "Add pub mod pass_through; in src/cli/mod.rs"
Task: "Add ALLEZ_CONDARC_PATH feature-gated check in src/channel_config/mod.rs"
Task: "Add render_ephemeral_creation_failure in src/output.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup.
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories).
3. Complete Phase 3: User Story 1 — real environment creation, activation, happy-path spawn/wait/exit-code, and the eight broken existing `cli_scaffold.rs` test functions (across five tasks, T023-T027) fixed.
4. **STOP and VALIDATE**: `cargo test --all --features test-config-override -- oneshot` passes User Story 1's own five scenarios independently.

### Incremental Delivery

1. Setup + Foundational → foundation ready.
2. Add User Story 1 → the happy path works end-to-end (a caller gets a real environment and a real exit code) → this is the MVP.
3. Add User Story 2 → signal forwarding and the full streaming/stdin/exit-code contract are provably correct, not just "probably fine because `Command` defaults to inherit."
4. Add User Story 3 → every pre-start failure category is distinct and correctly reported, including the dual-failure case.
5. Polish → cross-cutting FR-003/FR-009/FR-015 scenarios, `cargo doc`, full-suite validation.

---

## Notes

- [P] tasks touch different files with no dependency on an incomplete task; tasks sharing one file (`pass_through.rs`, `oneshot.rs`, `tests/oneshot_exec.rs`, `tests/cli_scaffold.rs`) are sequential even across story boundaries.
- Every test task that maps to a quickstart.md acceptance scenario references its exact scenario ID (e.g. "1.1", "2.4", "3.1a", "C.2") for direct traceability back to spec.md's acceptance scenarios (Constitution VIII); the unit tests (T008a, T009a, T035a, T036a, T046a) test pure logic already pinned down by data-model.md/research.md and have no scenario ID of their own to cite.
- `run`/`sandbox` are explicitly out of scope — `pass_through.rs`'s functions are written `pub(crate)`, but wiring them to `run`/`sandbox` is not part of this task list.
- No task modifies `create`/`list`/`remove`'s own exit-code/JSON surface (contracts/oneshot_cli_contract.md's Non-goals).
