# Feature Specification: `allez oneshot` Command

**Feature Branch**: `GEN-25_oneshot_command`

**Created**: 2026-08-03

**Input**: User description: "GEN-25 `allez oneshot` command: build ephemeral environment, run command inside it, stream output and exit code back to caller"

**Jira**: [GEN-25](https://anaconda.atlassian.net/browse/GEN-25) — `allez oneshot` command (parent epic: [GEN-19](https://anaconda.atlassian.net/browse/GEN-19))

**Operating Context**: `allez` runs inside a sandbox established externally, before `allez` itself starts; that sandbox boundary — not this feature — isolates `allez` from the host system. This feature is invoked by an automated agent operating inside that sandbox, not directly by a human at a terminal. Requesting a package authorizes any code that package runs during its own installation; this feature adds no separate consent gate for that. Running the pass-through command is the action the caller explicitly directed `allez` to perform, the same as any command a shell runs on a caller's behalf. The pass-through program is executed directly, without an intermediate shell; a caller wanting shell features (chaining commands, pipes, redirection) supplies a shell itself as the program.

**No environment teardown**: `allez oneshot` performs no cleanup of any kind on a successful outcome — it does not modify or remove an environment it successfully creates (see FR-009).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Run a one-off command against exactly the packages it needs (Priority: P1)

An agent needs to run a specific command-line using a specific set of packages, without first creating, naming, or managing a persistent environment — it wants a working environment materialized on demand, used once, and then left alone.

**Why this priority**: This is this feature's entire reason to exist — every other behavior (correct activation, output streaming, exit-code propagation, failure reporting) only matters once a caller can reliably request "these packages, then this command" and have both actually happen. Nothing else in this feature is testable without this working first.

**Independent Test**: Can be fully tested by invoking `allez oneshot pkg1 pkg2 -- some-command`, and confirming a new ephemeral environment is created and populated with `pkg1` and `pkg2` (and their own dependencies) before `some-command` is ever started — independent of what `some-command` itself does.

**Acceptance Scenarios**:

1. **Given** a list of package names before `--` and a pass-through command after it, **When** `allez oneshot` is invoked, **Then** a new ephemeral environment is created and populated with the requested packages (and their own dependencies) before the pass-through command starts, using the configured package-source/channel preferences.
2. **Given** no package names before `--` (e.g. `allez oneshot -- some-command`), **When** `allez oneshot` is invoked, **Then** the environment still comes into existence, populated with the configured default (or override) package set, rather than being empty or failing.
3. **Given** two separate `allez oneshot` invocations, **When** each successfully creates its environment, **Then** each received its own freshly created environment — neither invocation's environment or installed packages is shared with, or affected by, the other's. Repeating an invocation with identical inputs creates its own separate new environment rather than reusing or being matched against a prior one.
4. **Given** a pass-through command supplied with multiple arguments, including ones containing spaces or shell-special characters, **When** `allez oneshot` runs it, **Then** the program receives exactly those arguments, unchanged and in the same order, exactly as if it had been invoked directly.
5. **Given** a requested package that installs an executable also present elsewhere on the host, **When** the pass-through command invokes that executable by name, **Then** the environment's own installed executable is found and run, not a same-named one that may exist elsewhere on the host.

---

### User Story 2 - See the command's real output and get back its real exit code (Priority: P1)

An agent needs to observe the pass-through command's own output as it is produced, and needs allez's own process exit code to be the pass-through command's exit code, so it can react to the command's outcome the same way it would if it had run that command directly.

**Why this priority**: Streaming output and propagating the exit code is the entire observable contract an automated caller depends on — without it, a caller cannot tell what the command did or whether it succeeded, no matter how correctly the environment itself was built.

**Independent Test**: Can be fully tested by running a pass-through command that produces output incrementally and exits with a specific non-zero code, and confirming that output is visible before the command finishes and that `allez`'s own process exit code matches the command's exit code exactly — independent of which packages were requested.

**Acceptance Scenarios**:

1. **Given** a pass-through command that writes output over time rather than all at once, **When** it is run via `allez oneshot`, **Then** that output is visible to the caller as it is produced, not withheld until the command finishes, and each stream keeps its own identity — the command's own standard output reaches the caller's standard output, and its standard error reaches the caller's standard error, without merging the two.
2. **Given** a pass-through command that starts successfully and exits with a specific exit code (zero or non-zero), **When** it finishes, **Then** `allez`'s own process exit code is exactly that code.
3. **Given** a pass-through command that reads from standard input, **When** it is run via `allez oneshot`, **Then** it receives the same standard input `allez` itself received, so interactive or piped input still reaches it.
4. **Given** a pass-through command that is still running when `allez` itself receives a termination signal it is able to intercept, on a platform where that concept exists, **When** that signal arrives, **Then** `allez` forwards it to the pass-through command rather than exiting and leaving that command running detached.
5. **Given** a pass-through command that is terminated by a signal rather than exiting normally, on a platform where that concept exists, **When** it terminates, **Then** `allez`'s own exit code is the documented 128-plus-signal-number fallback value, not a zero exit code.

---

### User Story 3 - Get a clear, distinct failure when the environment itself can't be built (Priority: P2)

An agent needs to be able to tell "the environment could not be built" apart from "the environment was built, but the command itself failed," so it can decide whether to retry with different packages versus treat the command's own result as authoritative.

**Why this priority**: Automated callers branch on outcomes; conflating an environment-setup failure with the pass-through command's own failure would make that branching unreliable. This matters once the happy path (User Story 1/2) works, but before this feature can be considered complete per its own acceptance criteria.

**Independent Test**: Can be fully tested by requesting a package that cannot be resolved, and confirming the invocation fails with a clear, actionable message before the pass-through command is ever started, using a failure category distinct from any category a started pass-through command's own result could carry.

**Acceptance Scenarios**:

1. **Given** one or more requested packages that cannot be resolved or installed, **When** `allez oneshot` is invoked, **Then** the pass-through command is never started, and the caller receives a human-readable message plus a stable failure category identifying that environment creation failed, following this project's existing error-reporting convention.
2. **Given** a pass-through command whose program name cannot be found or executed inside the newly created environment, **When** `allez oneshot` attempts to run it, **Then** the caller receives a distinct, actionable failure — not a misleading "success" exit code, and not indistinguishable from a resolvable package failure (Acceptance Scenario 1).
3. **Given** a creation failure whose own partial-state cleanup also fails, **When** `allez oneshot` reports the failure, **Then** the caller still receives the original creation-failure category and message, plus a further, distinct indication that cleanup could not fully complete — neither masking the other.
4. **Given** an invocation with no `--` separator, or with `--` and nothing after it, **When** `allez oneshot` is invoked, **Then** it is rejected as a usage error before any environment is created.

---

### Edge Cases

- What happens when no packages are requested before `--`? (The configured default/override package set is used — see User Story 1, Acceptance Scenario 2.)
- What happens when a requested package cannot be resolved or installed? (Creation fails cleanly, the pass-through command is never started, and a distinct failure category is reported — see User Story 3, Acceptance Scenario 1.)
- What happens when the pass-through command's program name does not exist, or exists but is not executable, inside the newly created environment? (Reported as a distinct, actionable failure using exit codes 127/126 respectively, so the caller can tell the two apart — see User Story 3, Acceptance Scenario 2, and FR-008.)
- What happens when the pass-through command is terminated by a signal (e.g. killed) rather than exiting normally? (Reported via the documented 128-plus-signal-number fallback exit code rather than silently mapping to success — see User Story 2, Acceptance Scenario 5, and FR-007.)
- What happens if `allez` itself receives a termination signal while the pass-through command is still running? (An interceptable signal is forwarded to the pass-through command rather than left running detached — see User Story 2, Acceptance Scenario 4, and FR-014.)
- What happens to the environment after the pass-through command finishes, however it finishes (success, failure, or abnormal termination)? (Nothing — it is not removed; see FR-009.)
- What happens to a partially populated environment if creation itself fails? (It undergoes atomic rollback per the environment-creation capability's own behavior; see FR-009 and FR-010.)
- What happens if that rollback itself also fails? (Both the original creation failure and the distinct cleanup failure are reported, neither masking the other — see User Story 3, Acceptance Scenario 3, and FR-010.)
- What happens when multiple `allez oneshot` invocations run at the same time? (Each gets its own independently created environment; see User Story 1, Acceptance Scenario 3.)
- What happens when the pass-through command needs to read piped or interactive input? (It receives `allez`'s own standard input unchanged; see User Story 2, Acceptance Scenario 3.)
- What happens if a caller wants to chain multiple commands (e.g. `cmd1 && cmd2`)? (Not interpreted by this feature: the pass-through program is executed directly, so a caller supplies a shell itself as the program if it wants chaining, pipes, or redirection.)
- What happens to the arguments after the pass-through program's own name? (They reach the program unchanged and in order — see User Story 1, Acceptance Scenario 4, and FR-001.)
- What happens if a package installs an executable whose name also exists elsewhere on the host? (The environment's own copy is found first via `PATH` — see User Story 1, Acceptance Scenario 5, and FR-003.)

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST create a new ephemeral environment populated with the packages named before `--` (or the built-in default/override package set when none are named), before starting the pass-through program named after `--` together with every argument that follows it, unchanged and in the order supplied.
- **FR-002**: System MUST NOT reuse, cache, or share an ephemeral environment across separate `allez oneshot` invocations; each invocation receives its own independently created environment. This requirement does not prohibit shared package-download or repodata caches.
- **FR-003**: System MUST run the pass-through command with the newly created environment activated: at minimum, its `PATH` (or platform-equivalent) adjusted so the environment's own installed executables are found first, plus any other environment variables activation defines. Activation MUST merge with, not replace, the pass-through command's environment: every environment variable `allez`'s own process had — including anything supplied to `allez` by its surrounding sandbox — MUST still reach the pass-through command, with activation's own variables added or overridden on top; no inherited variable is silently dropped.
- **FR-004**: System MUST stream the pass-through command's own standard output and standard error to the caller as they are produced, rather than buffering and emitting them only after the command finishes; standard output reaches the caller's own standard output and standard error reaches the caller's own standard error, without merging the two.
- **FR-005**: System MUST forward the caller's own standard input to the pass-through command unchanged, so a command that reads input (interactively or via a pipe) behaves the same as if it had been run directly.
- **FR-006**: System MUST propagate the pass-through command's own process exit code as `allez`'s own process exit code whenever that command starts successfully and terminates normally. `allez oneshot` uses exit code `1` for an environment-creation failure (FR-010) and exit codes `126`/`127`/`128`-plus-signal-number for pass-through-command outcomes that are not a normal exit (FR-007, FR-008), in addition to `0` (success) and `2` (usage error, FR-011); every other subcommand's own exit-code surface is unaffected. Once the pass-through command itself starts and terminates normally, its own exit code is the authoritative outcome signal for that invocation, even if it happens to equal one of the values above.
- **FR-007**: System MUST report a pass-through command that is terminated by a signal it does not survive, on a platform where that concept exists, using exit code `128` plus the signal number and failure category `pass_through_terminated_by_signal` — the same convention shell tooling already uses — rather than silently reporting a zero exit code, with a human-readable message recorded through the FR-012 observability channel. Like the shell tools it mirrors, this convention does not guarantee the resulting exit code never coincides with one a normally-exiting command could also choose to return.
- **FR-008**: System MUST report a pass-through command whose program cannot be found using exit code `127` and failure category `pass_through_not_found`, and one that is found but cannot be executed using exit code `126` and failure category `pass_through_not_executable` — additive, non-breaking values alongside this project's existing failure categories — each paired with a human-readable message. As with FR-007, this failure category and message — present only in this pre-start case — is what a caller relies on to disambiguate; the exit code alone is not guaranteed unique.
- **FR-009**: System MUST NOT remove, clean up, or otherwise tear down a successfully created ephemeral environment, whether the pass-through command succeeds, fails, or terminates abnormally. A failed creation attempt is unaffected by this requirement and uses the environment-creation capability's own partial-state cleanup.
- **FR-010**: System MUST report an environment-creation failure — using exit code `1`, a human-readable message, and one of this project's own stable failure categories (`unresolvable_package`, `integrity_verification_failed`, `unwritable_location`, or `no_channels_configured`) — before ever attempting to start the pass-through command. If that failed attempt's own cleanup also fails, the caller MUST still receive the original failure's category and message, plus the `teardown_failed` category and its own message — neither masking the other. As with FR-007/FR-008, the accompanying category and message is the reliable disambiguator, not the bare exit code in isolation.
- **FR-011**: System MUST reject an `allez oneshot` invocation that has no `--` separator, or has `--` with nothing after it, as a usage error — exit code `2` and this project's existing `missing_pass_through_command` failure category — before attempting to create an environment at all. As with FR-008/FR-010, this failure's own message and category — present only because no environment was created and no pass-through command started — is what a caller relies on to disambiguate from a pass-through command that itself happens to exit `2`.
- **FR-012**: System MUST emit, through this project's existing structured-observability channel, a record of whether environment creation succeeded, failed (and with which category, or categories, if a failed creation's own cleanup also failed), or was never attempted (e.g. because the invocation itself was rejected as a usage error); whether the pass-through command started; and its termination outcome (exit code and, for an abnormal-termination or could-not-start outcome, the failure category and a human-readable message) when available. These records MUST be emitted only before the pass-through command starts or after it terminates, never while it is running, so they never interleave with or contaminate the streams FR-004 requires. These records extend this project's existing observability schema with the fields above, as an additive extension carrying its own documented schema version and a value that ties every record for one invocation together.
- **FR-013**: Once the pass-through command has successfully started, system MUST NOT wrap, buffer, or otherwise transform its standard output/error content into a JSON/human-readable success envelope — that command's own raw output, plus the propagated exit code (FR-006), constitute this invocation's entire observable result. This is the one outcome this project's JSON/human dual-format convention does not cover: every failure that occurs before the pass-through command starts (FR-008, FR-010, FR-011) still renders as a JSON or human-readable payload depending on format selection, exactly like every other subcommand's own failure output; a successfully started pass-through command has no separate structured payload to render two ways, since its own propagated exit code is already the stable, machine-actionable signal that convention exists to provide.
- **FR-014**: If `allez` itself receives a termination signal it is able to intercept, while the pass-through command is still running, on a platform where that concept exists, system MUST forward that signal to the pass-through command rather than exiting and leaving it running detached.
- **FR-015**: System MUST NOT disclose the created environment's location or identifier in the caller-facing result payload `allez` itself generates on standard output or standard error. This does not extend to the pass-through command's own raw output, which FR-004/FR-013 already require forwarding unchanged regardless of its content, nor to the FR-012 observability channel, which may carry the environment identifier as its correlation value for any record tied to an environment that was actually attempted.

### Key Entities *(include if feature involves data)*

- **One-Shot Run Request**: The set of package names supplied before `--`, and the program name plus arguments supplied after it.
- **Ephemeral Environment**: An unnamed, caller-unpathed environment populated with the requested (or default/overridden) packages, created fresh for this one invocation; not reused or cached across invocations (FR-002), and not torn down by this feature once successfully created (FR-009).
- **Pass-Through Command Outcome**: The result of attempting to start and run the pass-through command inside the created environment — either a propagated exit code (normal termination), a documented fallback code (abnormal termination), or a distinct "could not be started" failure (program not found/not executable) — mutually exclusive with an environment-creation failure, since the two can never both apply to the same invocation.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: For 100% of invocations in which environment creation succeeds, every requested package (and its own dependencies) is installed and usable by the pass-through command before that command starts.
- **SC-002**: For a pass-through command that writes output, pauses, then writes more output before exiting, the caller observes the first output before the pause ends — not only after the command exits.
- **SC-003**: 100% of pass-through commands that start successfully and terminate normally result in `allez`'s own exit code exactly matching that command's exit code.
- **SC-004**: 100% of environment-creation failures — across every category FR-010 covers, including the dual-failure case where a failed attempt's own cleanup also fails — are reported with exit code `1` and a distinct message/category before the pass-through command is ever started, and in 0% of those cases is the pass-through command invoked.
- **SC-005**: 100% of "pass-through command could not be started" cases use exit code `127` (not found) or `126` (not executable), each carrying its own distinct message and failure category that never accompanies a normal command exit or an environment-creation failure — so a caller can always tell the three apart without inspecting the pass-through command's own output.
- **SC-006**: 100% of successfully created ephemeral environments remain present on disk immediately after the invocation finishes, regardless of how it finished (pass-through success, pass-through failure, or abnormal termination) — confirming no teardown occurs. This is separate from a failed creation attempt's own partial-state cleanup, which is unaffected by this feature.
- **SC-007**: 100% of pass-through command invocations receive every environment variable `allez`'s own process had (e.g. `HOME`, locale settings), with only activation's own variables added or overridden on top — 0% of inherited variables are silently dropped.
- **SC-008**: 100% of termination signals `allez` itself is able to intercept while the pass-through command is running, on a platform where that concept exists, are forwarded to that command.
- **SC-009**: 100% of `allez oneshot` invocations — whether environment creation is never attempted, fails before the pass-through command starts, or the command runs through to completion — produce a structured, schema-versioned record via this project's existing observability channel that lets an operator reconstruct the environment-creation outcome, whether the pass-through command started, and its termination outcome when available, without re-running the invocation.
- **SC-010**: 0% of caller-facing result payloads `allez` itself generates on standard output or standard error disclose the created environment's location or identifier (FR-015).
- **SC-011**: On a platform where signal-based termination exists, 100% of pass-through commands terminated by a signal result in `allez`'s own exit code being `128` plus that signal number, with failure category `pass_through_terminated_by_signal` and its message recorded via the observability channel.

## Assumptions

- `allez oneshot` uses the environment-creation capability's package-resolution, default-package, channel-selection, and artifact-integrity behavior without modification.
- No environment-removal mechanism is available to `allez oneshot`; it does not remove a successfully created environment.
- The exit codes and conventions above (FR-007, FR-008, FR-010, FR-011), and FR-014's signal forwarding, apply on this product's supported platforms: Windows amd64, macOS aarch64, Linux aarch64, and Linux amd64. Where a platform has no equivalent concept (e.g. Windows has no POSIX signal-number convention), this feature still reports its own documented, distinct failure category for the equivalent situation, without requiring the same numeric convention on every platform.
- Concrete latency targets for how fast `allez oneshot` should complete are out of this feature's scope; SC-002 requires observably streamed (not buffered) output, not a specific speed.
- Activation's merge behavior (FR-003) applies uniformly to every inherited environment variable, including any the surrounding sandbox may have supplied to `allez` itself. The pass-through command is an untrusted, caller-directed command like any other; this feature does not add its own additional trust boundary around which inherited variables reach it.
- The environment-creation capability treats each invocation, including one with inputs identical to an earlier invocation, as a new operation that creates an independent environment, not as a retry deduplicated against a prior result.
