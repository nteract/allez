---

description: "Task list template for feature implementation"
---

# Tasks: CLI Scaffold and Subcommand Routing

**Input**: Design documents from `/specs/GEN-22_cli_scaffold_routing/`

**Prerequisites**: [plan.md](./plan.md), [spec.md](./spec.md), [research.md](./research.md), [data-model.md](./data-model.md), [contracts/cli-schema.md](./contracts/cli-schema.md), [quickstart.md](./quickstart.md)

**Tests**: Included and REQUIRED — the project constitution mandates TDD (Principle II: tests written before implementation) and 100% spec test coverage (Principle VIII). Every acceptance scenario in spec.md maps to at least one black-box integration test below (T011-T038B). Principle II additionally requires unit tests for internal/non-public-API logic, living alongside the code under test in `#[cfg(test)]` modules, distinct from that black-box coverage — see T005B, T007B, T007C, T008A, T040B (added during `/speckit.analyze` remediation — closes finding D1: an earlier draft committed to this in plan.md's Testing section with no covering tasks).

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

Single project (per [plan.md](./plan.md) Project Structure): `src/`, `tests/` at repository root.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project initialization and basic structure

- [X] T001 Create Cargo project skeleton (`cargo init --name allez`) and the directory layout from plan.md's Project Structure (`src/cli/`, `tests/`) at the repository root; explicitly set `edition = "2024"` in the generated `Cargo.toml` — `cargo init`'s own default edition depends on the installed toolchain and is NOT assumed to already match plan.md's Technical Context ("Rust, stable toolchain, 2024 edition")
- [X] T002 Add primary dependencies to `Cargo.toml`: `clap` (`derive` feature, v4), `serde` (`derive` feature), `serde_json`, `tracing`, `tracing-subscriber` (`fmt`, `json`, `env-filter` features) — per research.md §1-3
- [X] T003 [P] Add dev-dependencies to `Cargo.toml`: `assert_cmd`, `predicates`, `rstest` — per research.md §4
- [X] T004 Verify `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` run clean on the empty skeleton; add `rustfmt.toml` only if project defaults need overriding; add `#![warn(missing_docs)]` at the crate root in `src/main.rs` so `cargo clippy`/`cargo doc` (T044/T045) mechanically enforce constitution Principle VI's doc-comment requirement across every module added later
- [X] T004B Add `#![deny(clippy::unwrap_used, clippy::expect_used)]` at the crate root in `src/main.rs`, immediately below T004's `#![warn(missing_docs)]` attribute (or, if preferred, as an equivalent `[lints.clippy]` table in `Cargo.toml` instead — a genuinely different file, restoring true `[P]`-parallelism with T004), scoped to non-test code only, so `cargo clippy` (T044) mechanically enforces constitution Principle V's "no `.unwrap()`/`.expect()` in library code outside tests" rather than relying on manual review. **Not marked `[P]`**: both the crate-root-attribute option above and T004 target the same file (`src/main.rs`); run T004 first, then T004B, to avoid a same-file edit collision — this corrects an earlier draft that marked both `[P]` while both edited `src/main.rs`

**Checkpoint**: `cargo build` succeeds on an empty `main.rs`; toolchain gates are clean.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core CLI skeleton, output/error/observability plumbing that every user story's tests and implementation depend on

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T005 Define a single shared `PassThroughArgs` struct (`#[derive(Args)]`, fields `program: String`, `args: Vec<String>` via `#[arg(last = true)]`, **no `required` attribute on the field** — see research.md §1) in `src/cli/mod.rs`; define `PackagesAndCommandArgs` (composes `packages: Vec<String>` + a flattened `PassThroughArgs`) for `oneshot` only, per data-model.md's Pass-Through Command / Package Reference entities and research.md §1's flatten pattern. `CreateArgs` (path + packages, no pass-through command at all — FR-004) is defined independently in T006 and MUST NOT reuse `PassThroughArgs`/`PackagesAndCommandArgs`
- [X] T005A [P] Implement `parse_nonempty_path(s: &str) -> Result<String, String>` in `src/cli/mod.rs`: returns `Err` with an actionable message when `s` is empty, `Ok(s.to_string())` otherwise (FR-015, research.md §7). This is a plain function with no dependency on T005/T006's types, so it can be written before or alongside them; T006's `CreateArgs.env_path`/`RunArgs.env_path`/`RemoveArgs.env_path` fields wire it in via `#[arg(value_parser = parse_nonempty_path)]`
- [X] T005B [P] Write unit tests for `parse_nonempty_path` in a `#[cfg(test)] mod tests` block in `src/cli/mod.rs` (constitution Principle II — unit tests for internal/non-public-API logic live alongside the code under test, distinct from T031A's black-box `assert_cmd` coverage of the same rule): cases for empty string → `Err`, non-empty string → `Ok` with the value unchanged, and a whitespace-only string (e.g. `" "`) to confirm it's treated as non-empty (not additionally trimmed) unless FR-015 is read to require trimming — write this FIRST, confirm it fails against a stub/`todo!()` body, before T005A's real implementation lands (depends on T005A's signature existing; no dependency on T006)
- [X] T006 Define the top-level `Cli` struct and `Commands` enum (6 tuple variants: `Oneshot(PackagesAndCommandArgs)`, `Create(CreateArgs)`, `Run(RunArgs)`, `Sandbox(PassThroughArgs)`, `List`, `Remove(RemoveArgs)`) with the exact argument shapes from contracts/cli-schema.md, in `src/cli/mod.rs` (depends on T005, T005A); `CreateArgs` is its own independent struct (`env_path: String` via `#[arg(value_parser = parse_nonempty_path)]` — T005A, `packages: Vec<String>`), and `RunArgs` composes `env_path: String` (same `value_parser`) + a flattened `PassThroughArgs` (reusing the same shared type `Sandbox` uses — see T007A for why this is safe); `RemoveArgs.env_path` uses the same `value_parser` too. `Cli` also carries two `global = true` flags spanning all six subcommands (FR-013, FR-016): `human: bool` (`--human`, selects human-readable output; JSON is the unconditional default when absent — there is no `--format`/`--json` enum-plus-shorthand, per research.md Decision 2) and `verbose: bool` (`-v`/`--verbose`, reveals redacted pass-through content) — `global = true` is what makes each valid before *or* after the subcommand name (per FR-013's Placement rule) while still never crossing the `--` separator, since clap stops treating tokens as flags once `--` is consumed
- [X] T007 [P] Implement `src/error.rs`: an `AllezError` enum representing usage errors, consistent with the two-tier exit-code convention (FR-011: `0` success / `2` usage error); variants MUST map 1:1 to the fixed `category` enum FR-017 requires — `AllezError::MissingArgument`, `AllezError::UnknownSubcommand`, `AllezError::UnknownFlag`, `AllezError::MissingPassThroughCommand` (the last for use by T007A) — with a method (or `#[serde(rename_all = "snake_case")]` derive) producing each variant's `category` string directly from the enum, so there is no second, hand-maintained list of category strings to drift out of sync with the type
- [X] T007B [P] Write unit tests for `AllezError`'s category-string derivation (T007) in a `#[cfg(test)] mod tests` block in `src/error.rs` (constitution Principle II): one case per variant asserting its `category()` method (or serialized form) matches exactly one of the fixed FR-017 strings — `missing_argument`, `unknown_subcommand`, `unknown_flag`, `missing_pass_through_command` — write this FIRST, confirm it fails before `AllezError`'s categorization logic is implemented for real (depends on T007's enum shape existing)
- [X] T007A Implement `validate_pass_through(pt: &PassThroughArgs) -> Result<(), AllezError>` in `src/cli/mod.rs`: returns `Err(AllezError::MissingPassThroughCommand)` if `pt.program` is empty, `Ok(())` otherwise (depends on T005, T007). This is the enforcement mechanism for FR-003/FR-005's "pass-through command required" rule — called from **`src/main.rs`'s dispatch** for the `Oneshot`/`Run` match arms only, immediately after `Cli::try_parse()` succeeds and *before* invoking either handler's function at all (wired in T039; see FR-010's "before invoking any stub handler's core logic" requirement — this is why the call site is the dispatch match, not the handler body); the `Sandbox` match arm does NOT call it unmodified — `sandbox`'s required-ness is conditional on whether a literal `--` token was present at all (FR-006, revised during PR #1 review), a distinction `validate_pass_through()`'s `pt.program.is_empty()` check alone cannot make, since clap's `#[arg(last = true)]` match state is identical for "no `--`" and "`--` with nothing after" (see research.md §1/Decision 1, empirically confirmed against `clap` 4.6.x); `sandbox` instead uses T040A's raw-argv check
- [X] T007C [P] Write unit tests for `validate_pass_through()` (T007A) in a `#[cfg(test)] mod tests` block in `src/cli/mod.rs` (constitution Principle II — distinct from T034/T035/T035A's black-box `assert_cmd` coverage of the same rule at the process level): a `PassThroughArgs` with empty `program` → `Err(AllezError::MissingPassThroughCommand)`; a `PassThroughArgs` with a non-empty `program` (with and without `args`) → `Ok(())` — write this FIRST, confirm it fails before T007A's real implementation lands (depends on T005, T007's types existing)
- [X] T008 [P] Implement `src/output.rs`: the JSON/human rendering behind `Cli`'s `human`/`verbose` fields (T006, FR-013, FR-016) via `render_success`/`render_error` functions. `render_success` MUST emit the normatively fixed minimal shape `{schema_version, subcommand, status: "stub", parsed: {...}}` for JSON (the default, when `human` is `false`) and an equivalent one-line human rendering carrying the same facts (when `human` is `true`) — FR-014; for `oneshot`/`run`/`sandbox`, `parsed`'s pass-through field is `{"program": "<redacted>", "arg_count": N}` unless `verbose` is set, in which case it is the full `{"program": "...", "args": [...]}` — FR-016. `packages`/`path` fields are never redacted. `render_error` MUST emit `{schema_version, category, message}` for JSON, with `category` taken from `AllezError`'s own categorization (T007) — FR-017
- [X] T008A [P] Write unit tests for `render_success`/`render_error` (T008) in a `#[cfg(test)] mod tests` block in `src/output.rs` (constitution Principle II — distinct from T023-T023I/T023F/T023G's black-box `assert_cmd` coverage of the same behavior at the process level): assert the exact minimal JSON shape/keys for `render_success` (`human = false`) via `serde_json::from_str`; assert the pass-through field is `{"program": "<redacted>", "arg_count": N}` when `verbose = false` and the full unredacted value when `verbose = true`, for a representative `PassThroughArgs`; assert `render_error`'s JSON shape (`{schema_version, category, message}`) for at least one `AllezError` variant — write this FIRST, confirm it fails before T008's real implementation lands (depends on T006, T007's types existing)
- [X] T009 [P] Implement `src/observability.rs`: `tracing-subscriber` initialization selecting a human (`.pretty()`) or JSON (`.json()`, the default) formatter based on the parsed `--human` flag, writer = stderr, called once before subcommand dispatch — per research.md §3
- [X] T010 Wire `src/main.rs`: call `Cli::try_parse()` (NOT `Cli::parse()`, so parse errors can be routed through `error.rs`'s consistent rendering instead of clap's default print-and-exit — see T042); on `Err(e)`, render via `error.rs`/`output.rs` to stderr and `std::process::exit(2)`; on `Ok(cli)`, call `observability::init(...)`, dispatch on `Commands` to placeholder stub bodies — the `Oneshot`/`Run` arms of this dispatch match are where T039 will later insert the `validate_pass_through()` call, and the `Sandbox` arm is where T040A will later insert its raw-argv `--`-presence check, both *before* invoking the corresponding handler function, so structure the match arms now in a shape that supports inserting a pre-handler validation step per arm (depends on T006, T008, T009)

**Checkpoint**: `cargo build` succeeds; `allez` runs without panicking (help/content polish happens in US1; real stub bodies happen in US2) — foundation ready for user story work.

---

## Phase 3: User Story 1 - Discover available operations via help (Priority: P1) 🎯 MVP

**Goal**: `allez --help`, `allez` (no args), and `allez <subcommand> --help` all surface accurate, complete usage information for a first-time operator.

**Independent Test**: Run `allez --help` and confirm all six subcommands are listed with usage; run `allez` with no arguments and confirm usage + exit code `2`; run `allez <subcommand> --help` for each of the six and confirm subcommand-specific argument descriptions — independent of whether stub handlers do anything real yet.

### Tests for User Story 1 ⚠️

> **Write these tests FIRST in `tests/cli_scaffold.rs`; confirm they FAIL before the implementation tasks below.**

- [X] T011 [US1] Integration test: `allez --help` exits `0` and lists all six subcommand names with a usage line each, in `tests/cli_scaffold.rs` (spec.md User Story 1, Acceptance Scenario 1)
- [X] T012 [US1] Integration test: `allez` with no arguments exits with status code `2` and prints usage/help (not a silent no-op), in `tests/cli_scaffold.rs` (Acceptance Scenario 2)
- [X] T013 [US1] Integration test (table-driven via `rstest`, one case per subcommand): `allez <subcommand> --help` exits `0` and describes that subcommand's own required/optional arguments, for all six subcommands, in `tests/cli_scaffold.rs` (Acceptance Scenario 3)
- [X] T013A [US1] Integration test: `allez --version` exits `0` and prints the crate's version string on stdout, in `tests/cli_scaffold.rs` (FR-018, added during `/speckit.analyze` remediation — closes finding I2: the flag was documented in contracts/cli-schema.md's synopsis with no backing FR or test)

### Implementation for User Story 1

- [X] T014 [US1] Add `about`/help text (doc comments and/or `#[command(about = "...")]`) to the top-level `Cli` and each of the six `Commands` variants in `src/cli/mod.rs`, satisfying FR-001/FR-002 (depends on T006)
- [X] T015 [US1] Configure `Cli` so a missing subcommand shows usage and exits `2` (e.g. `#[command(subcommand_required = true, arg_required_else_help = true)]` or equivalent) in `src/cli/mod.rs`, satisfying FR-001's no-args behavior (depends on T006)
- [X] T015A [US1] Add `#[command(version)]` to the top-level `Cli` struct in `src/cli/mod.rs` so `--version`/`-V` prints the crate version and exits `0` (FR-018, depends on T006; verify against T013A)

**Checkpoint**: T011-T013A pass. `allez --help` / no-args / per-subcommand `--help` / `--version` all behave exactly per spec.md User Story 1.

---

## Phase 4: User Story 2 - Invoke any subcommand with its correct arguments (Priority: P1)

**Goal**: Each of the six subcommands parses its documented argument shape (packages, path, pass-through command via `--`) correctly and routes to a distinct stub handler that acknowledges what it parsed, in the format selected by `--human` (JSON by default).

**Independent Test**: Invoke each of the six subcommands with a representative valid example from contracts/cli-schema.md and confirm the tool routes to that subcommand's stub with the arguments parsed as expected, both in the default JSON output and with `--human` — no environment backend required.

### Tests for User Story 2 ⚠️

> **Write these tests FIRST in `tests/cli_scaffold.rs`; confirm they FAIL before the implementation tasks below.**

- [X] T016 [US2] Integration test: `allez oneshot pkg1 pkg2 -- echo hello` exits `0`; stub output identifies packages=[pkg1,pkg2] and pass-through command=echo,args=[hello], in `tests/cli_scaffold.rs` (spec.md User Story 2, Acceptance Scenario 1)
- [X] T016A [US2] Integration test: `allez oneshot -- echo hi` (zero packages) exits `0`; stub output identifies packages=[] and pass-through command=echo,args=[hi], in `tests/cli_scaffold.rs` (spec.md Edge Cases: zero-package invocation is valid, not an error — FR-003)
- [X] T017 [US2] Integration test: `allez create ./my-env pkg1 pkg2` exits `0`; stub output identifies path=./my-env and packages=[pkg1,pkg2], in `tests/cli_scaffold.rs` (Acceptance Scenario 2)
- [X] T017A [US2] Integration test: `allez create ./my-env` (zero packages) exits `0`; stub output identifies path=./my-env and packages=[], in `tests/cli_scaffold.rs` (spec.md Edge Cases: zero-package invocation is valid, not an error — FR-004)
- [X] T018 [US2] Integration test: `allez run ./my-env -- echo hello` exits `0`; stub output identifies path=./my-env and pass-through command=echo,args=[hello], in `tests/cli_scaffold.rs` (Acceptance Scenario 3)
- [X] T019 [US2] Integration test: `allez sandbox -- python -c "print(1)"` exits `0`; stub output identifies pass-through command=python,args=[-c, print(1)], in `tests/cli_scaffold.rs` (Acceptance Scenario 4)
- [X] T020 [US2] Integration test: `allez sandbox` (no pass-through command) exits `0`; stub output identifies the distinct interactive-subshell path, in `tests/cli_scaffold.rs` (Acceptance Scenario 5)
- [X] T021 [US2] Integration test: `allez list` exits `0` with no positional arguments required, in `tests/cli_scaffold.rs` (Acceptance Scenario 6)
- [X] T022 [US2] Integration test: `allez remove ./my-env` exits `0`; stub output identifies path=./my-env as the sole argument, in `tests/cli_scaffold.rs` (Acceptance Scenario 7)
- [X] T023 [US2] Integration test: `allez list` (no flag — default) exits `0` with stdout that parses as valid JSON via `serde_json::from_str` and matches `{schema_version, subcommand: "list", status: "stub", parsed: {}}` (FR-013, FR-014)
- [X] T023A [US2] Integration test: `allez oneshot pkg1 -- echo hi` (no flag — default) exits `0`, stdout is valid JSON matching `{schema_version, subcommand: "oneshot", status: "stub", parsed: {packages: ["pkg1"], pass_through: {program: "<redacted>", arg_count: 1}}}` (FR-013 — closes the gap that only `list` was tested against the strict-schema JSON path)
- [X] T023B [US2] Integration test: `allez create ./my-env` (no flag — default; valid, non-error invocation) exits `0`, stdout is valid JSON matching `{schema_version, subcommand: "create", status: "stub", parsed: {path: "./my-env", packages: []}}` (FR-013, FR-014)
- [X] T023C [US2] Integration test: `allez run ./my-env -- echo hi` (no flag — default) exits `0`, stdout is valid JSON matching `{schema_version, subcommand: "run", status: "stub", parsed: {path: "./my-env", pass_through: {program: "<redacted>", arg_count: 1}}}` (FR-013)
- [X] T023D [US2] Integration test: `allez sandbox` (no flag — default; no pass-through command) exits `0`, stdout is valid JSON matching `{schema_version, subcommand: "sandbox", status: "stub", parsed: {interactive_subshell: true}}` (FR-013, FR-014)
- [X] T023E [US2] Integration test: `allez remove ./my-env` (no flag — default) exits `0`, stdout is valid JSON matching `{schema_version, subcommand: "remove", status: "stub", parsed: {path: "./my-env"}}` (FR-013, FR-014)
- [X] T023F [US2] Integration test (table-driven via `rstest` across `oneshot`/`run`/`sandbox`, both the default JSON output and `--human`): the default (non-`--verbose`) acknowledgment's pass-through field is redacted — `arg_count` matches the actual token count after `--`, but the literal program name and argument values (e.g. `echo`, `hello`) never appear anywhere in stdout — in `tests/cli_scaffold.rs` (FR-016)
- [X] T023G [US2] Integration test (table-driven via `rstest` across `oneshot`/`run`/`sandbox`, both output formats): `--verbose`/`-v` reveals the full unredacted `program`/`args` in the acknowledgment, matching what was actually passed after `--` (including a flag-like token, e.g. `-c`, to reconfirm FR-012's verbatim-passthrough guarantee is independently visible once unredacted) — in `tests/cli_scaffold.rs` (FR-016)
- [X] T023H [US2] Integration test: `allez --human list` (`--human` placed *before* the subcommand name) exits `0` and produces stdout byte-for-byte identical to `allez list --human`'s human-readable output — in `tests/cli_scaffold.rs` (FR-013's Placement rule; confirms the before-placement window works, mirroring the original finding-E1 remediation now applied to `--human` instead of `--format`)
- [X] T023I [US2] Integration test: `allez --human oneshot pkg1 -- echo hi` (`--human` placed *before* the subcommand name, on a subcommand with its own packages + pass-through command) exits `0` and produces stdout identical to `allez oneshot pkg1 --human -- echo hi`'s human-readable output — in `tests/cli_scaffold.rs` (FR-013's Placement rule; confirms the before-placement window works identically even when combined with packages/pass-through parsing)

### Implementation for User Story 2

- [X] T024 [P] [US2] Implement the `oneshot` stub handler in `src/cli/oneshot.rs`: call `output::render_success` with parsed packages + pass-through command, in the fixed shape from T008 (redacted by default, FR-016) (FR-009) (depends on T006, T008). This handler does NOT call `validate_pass_through()` itself — that happens at dispatch time in `main.rs` before this handler is ever invoked (T007A, T039)
- [X] T025 [P] [US2] Implement the `create` stub handler in `src/cli/create.rs`: call `output::render_success` with parsed path + packages (depends on T006, T008)
- [X] T026 [P] [US2] Implement the `run` stub handler in `src/cli/run.rs`: call `output::render_success` with parsed path + pass-through command, redacted by default (FR-016) (depends on T006, T008). Like `oneshot` (T024), this handler does NOT call `validate_pass_through()` itself — see T039
- [X] T027 [P] [US2] Implement the `sandbox` stub handler in `src/cli/sandbox.rs`: call `output::render_success` with the parsed pass-through command (redacted by default, FR-016), or a distinct "interactive subshell" acknowledgment when none was supplied (FR-006) (depends on T006, T008)
- [X] T028 [P] [US2] Implement the `list` stub handler in `src/cli/list.rs`: call `output::render_success` acknowledging the invocation with an empty `parsed` object, satisfying FR-007's "no required positional arguments" (depends on T006, T008)
- [X] T029 [P] [US2] Implement the `remove` stub handler in `src/cli/remove.rs`: call `output::render_success` with the parsed path (depends on T006, T008)
- [X] T030 [US2] Wire all six stub handlers into `src/main.rs`'s dispatch `match`, replacing the Phase 2 placeholders (depends on T024, T025, T026, T027, T028, T029)

**Checkpoint**: T016-T023I pass. All six subcommands parse and route correctly, in both output formats and both `--human` placement windows (including zero-package invocations, redacted-by-default and `--verbose`-revealed pass-through content), per spec.md User Story 2.

---

## Phase 5: User Story 3 - Get consistent, actionable errors on invalid input (Priority: P2)

**Goal**: Every subcommand rejects invalid/incomplete invocations the same way — exit code `2`, an actionable message, JSON by default or human-readable with `--human` — without special-casing.

**Independent Test**: Feed each subcommand a representative invalid invocation (missing path, missing pass-through command, unknown subcommand, unknown flag) and confirm exit code `2` plus a message identifying what was wrong, using the same convention across all of them; separately confirm `sandbox --` with nothing after is now exit `2` (`missing_pass_through_command`, same as `oneshot`/`run`'s equivalent case), distinct from `sandbox` with no `--` at all, which stays exit `0`, not an error.

### Tests for User Story 3 ⚠️

> **Write these tests FIRST in `tests/cli_scaffold.rs`; confirm they FAIL before the implementation tasks below.**

- [X] T031 [US3] Integration test: `allez create` (no path) exits `2` with a message identifying the missing required argument, in `tests/cli_scaffold.rs` (spec.md User Story 3, Acceptance Scenario 1)
- [X] T031A [US3] Integration test (table-driven via `rstest`): `allez create ""`, `allez run "" -- echo hi`, and `allez remove ""` (empty-string path, distinct from a missing path) each exit `2` with a message identifying an invalid/missing required argument, rejected by the shared `parse_nonempty_path` value_parser (T005A) — in `tests/cli_scaffold.rs` (spec.md Edge Cases: "empty-string path argument... rejected as a missing/invalid required argument at parse time" — FR-004, FR-005, FR-008, FR-015)
- [X] T032 [US3] Integration test: `allez remove` (no path) exits `2` with a message identifying the missing required argument, in `tests/cli_scaffold.rs` (Acceptance Scenario 2)
- [X] T033 [US3] Integration test: `allez frobnicate` (unrecognized subcommand) exits `2` with a message indicating the subcommand is unknown, in `tests/cli_scaffold.rs` (Acceptance Scenario 3)
- [X] T033A [US3] Integration test: `allez List` (case-mismatched, otherwise-valid subcommand name) exits `2` with a message indicating the subcommand is unknown — distinct from T033's wholly-unknown-name case — in `tests/cli_scaffold.rs` (spec.md Edge Cases: "subcommand names are case-sensitive; a mismatched case is treated as unrecognized")
- [X] T034 [US3] Integration test: `allez oneshot pkg1 --` (nothing after separator) exits `2` with a message indicating a pass-through command is required, in `tests/cli_scaffold.rs` (Acceptance Scenario 4)
- [X] T035 [US3] Integration test: `allez run ./my-env` (no `--` at all) exits `2` with a message indicating a pass-through command is required, in `tests/cli_scaffold.rs` (contracts/cli-schema.md `run` table)
- [X] T035A [US3] Integration test: `allez run ./my-env --` (separator present, nothing after — distinct from T035's separator-absent case) exits `2` with a message indicating a pass-through command is required, in `tests/cli_scaffold.rs` (spec.md Edge Cases: "oneshot or run... given `--` with nothing after it" — mirrors T034's oneshot case for run)
- [X] T036 [US3] Integration test: `allez sandbox --` (nothing after separator) exits `2` with a message indicating a pass-through command is required (`category: missing_pass_through_command`, same as `oneshot`/`run`'s equivalent case) — distinct from `allez sandbox` (T020, no `--` at all), which stays exit `0`, not an error — in `tests/cli_scaffold.rs` (spec.md Edge Cases and FR-006, revised during PR #1 review; supersedes an earlier draft that treated both as equivalent, per research.md §1)
- [X] T037 [US3] Integration test: `allez list --bogus-flag` (unknown flag) exits `2` with an error message, in `tests/cli_scaffold.rs` (spec.md Edge Cases)
- [X] T038 [US3] Integration test (table-driven via `rstest`): all usage-error cases from T031-T035A, T036, and T037 share exit code `2`, the same message-format convention, and (in the default JSON mode) a `category` field drawn from the fixed enum — `missing_argument`, `unknown_subcommand`, `unknown_flag`, `missing_pass_through_command` (FR-017) — separate from the message text, delivered on **stderr** in both the default JSON and `--human` modes, in `tests/cli_scaffold.rs` (Acceptance Scenario 5, FR-011, FR-017)
- [X] T038A [US3] Integration test (table-driven via `rstest`, parameterized across `oneshot`/`run`/`sandbox`): the `--` separator invariant (FR-012) behaves identically across all three subcommands for the *empty-separator* case (`--` present, nothing after → usage error, `missing_pass_through_command`, for all three as of T036's fix) and for verbatim passthrough of flag-like tokens after `--` (e.g. a token like `-c` is preserved, not reinterpreted as an allez flag) — as one explicit cross-cutting property, not three independently-passing per-subcommand behaviors. `sandbox`'s *missing-separator* case (no `--` at all) is intentionally excluded from this cross-cutting property — it alone routes to the interactive-subshell path (T020), which `oneshot`/`run` have no equivalent of, since a pass-through command is unconditionally required for those two (FR-003, FR-005) — in `tests/cli_scaffold.rs` (FR-012)
- [X] T038B [US3] Integration test (table-driven via `rstest`, parameterized across `oneshot`/`run`/`sandbox`): a token identical in spelling to an allez global flag — `allez oneshot pkg1 -- echo hi --human`, `allez run ./my-env -- echo --verbose`, `allez sandbox -- printf -- human` — placed *after* `--` is forwarded to the pass-through command verbatim (visible via `--verbose`'s unredacted output, T023G) and does NOT change allez's own output format/verbosity or produce a usage error; exit `0` in every case, in `tests/cli_scaffold.rs` (FR-012, FR-013's Placement rule, spec.md Edge Cases)

### Implementation for User Story 3

- [X] T039 [US3] In `src/main.rs`'s dispatch `match` (T010), call `validate_pass_through()` (T007A) in the `Commands::Oneshot` and `Commands::Run` arms — immediately after the match, *before* calling into `oneshot::run(args)`/`run::run(args)` respectively — propagating its `Err(AllezError::MissingPassThroughCommand)` to `main.rs`'s error-rendering path (T042) so a missing/empty pass-through command exits `2` (FR-003, FR-005, FR-010) without ever invoking the handler's own body. This is a dispatch-layer call, NOT a call from inside `src/cli/oneshot.rs`/`run.rs` (an earlier draft called it from within the handlers, which technically means the invocation had already "reached a stub handler," in tension with FR-010's wording — moving the call to dispatch removes that ambiguity). `validate_pass_through()` itself is NOT reused unmodified for `sandbox` (T040A), since `sandbox`'s required-ness is conditional on `--`-presence rather than unconditional like `oneshot`/`run`'s (depends on T007A, T010, T024, T026; verify against T034, T035, T035A)
- [X] T040 [US3] Confirm `src/main.rs`'s dispatch arm for `Commands::Sandbox` does NOT call `validate_pass_through()` unmodified — when `pt.program` is non-empty, or when it's empty and T040A's raw-argv check finds no literal `--` token, dispatch proceeds straight to `sandbox::run(args)` (T027), which routes to the distinct interactive-subshell acknowledgment path, exit `0` (FR-006) (depends on T007A, T010, T027, T040A; verify against T020, T036)
- [X] T040A [US3] Implement `sandbox_missing_command(pt: &PassThroughArgs, raw_args: &[OsString]) -> Result<(), AllezError>` in `src/cli/mod.rs` (matching data-model.md's Pass-Through Command entity exactly — name and signature MUST match, not just intent): only when `pt.program` is empty, scans `raw_args` for a literal `OsStr::new("--")` token and returns `Err(AllezError::MissingPassThroughCommand)` if found, `Ok(())` otherwise — this is the mechanism that distinguishes `allez sandbox` (no `--` at all, T020) from `allez sandbox --` (T036), a distinction clap's own `ArgMatches` state cannot make for a `#[arg(last = true)]` field (empirically confirmed against `clap` 4.6.x, research.md §1/Decision 1). `raw_args` is taken as a parameter — NOT read internally via `std::env::args_os()` inside the function itself — specifically so this function stays unit-testable with fake argv slices in a `#[cfg(test)]` module (see T040B); `src/main.rs`'s `Sandbox` dispatch arm is the sole call site that supplies the real `std::env::args_os()` collected into a `Vec<OsString>`. Use `OsString`/`args_os()` at that call site, not `String`/`args()`, so non-UTF-8 argv doesn't panic; passing the full, un-scoped argv is safe only because this call site is the already-dispatched `Sandbox` match arm, where exactly one subcommand is known to be active per process invocation (depends on T007, T010; verify against T020, T036)
- [X] T040B [US3] Write unit tests for `sandbox_missing_command()` (T040A) in a `#[cfg(test)] mod tests` block in `src/cli/mod.rs` (constitution Principle II — distinct from T020/T036's black-box `assert_cmd` coverage of the same rule at the process level; this is exactly the test T040A's parameter-injecting signature exists to make possible, per the `/speckit.analyze` remediation that reconciled T040A with data-model.md): with `pt.program` empty — a fake `raw_args` slice containing a literal `"--"` token → `Err(AllezError::MissingPassThroughCommand)`; a fake `raw_args` slice with no `"--"` token at all → `Ok(())`; separately, with `pt.program` non-empty, any `raw_args` content → `Ok(())` (the function must not even need to inspect `raw_args` in this case) — write this FIRST, confirm it fails before T040A's real implementation lands (depends on T007, T040A's signature existing)
- [X] T041 [US3] Implement consistent human-readable and JSON error rendering for usage errors in `src/error.rs`/`src/output.rs` (FR-010, FR-011); both modes render to **stderr** (fixed, per contracts/cli-schema.md's Output Format Contract — not an implementation choice); the JSON error body MUST be `{schema_version, category, message}`, with `category` taken directly from the `AllezError` variant matched (T007's fixed enum — `missing_argument`, `unknown_subcommand`, `unknown_flag`, `missing_pass_through_command`) rather than a separately-typed string literal (FR-011, FR-017) (depends on T007, T008)
- [X] T042 [US3] In `src/main.rs`, handle `Cli::try_parse()`'s `Err(clap::Error)` variant (from T010) by mapping it to the matching `AllezError` variant (`UnknownSubcommand`/`UnknownFlag`/`MissingArgument`, per clap's own `ErrorKind`) and rendering it through `src/error.rs`'s consistent stderr rendering (T041) before calling `std::process::exit(2)`, instead of letting clap auto-print-and-exit; this is what makes FR-011's single consistent message/JSON convention apply to clap's own parse errors (unknown subcommand, unknown flag, missing required arg — including `parse_nonempty_path`'s empty-path rejection, T005A) as well as the app-level errors from T039/T040A (depends on T010, T039, T040, T040A, T041)

**Checkpoint**: T031-T038B pass. All error scenarios across all six subcommands share one exit-code/message convention, delivered consistently on stderr, per spec.md User Story 3.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Constitution-mandated quality gates and final validation across all three user stories

- [X] T043 [P] Add `///` doc comments to all public items across `src/**/*.rs` — `Cli`/`Commands`/`Args` types in `src/cli/mod.rs`, all six stub handlers (`src/cli/oneshot.rs`, `create.rs`, `run.rs`, `sandbox.rs`, `list.rs`, `remove.rs`), and `src/error.rs`/`src/output.rs`/`src/observability.rs` — satisfying constitution Principle VI; `#![warn(missing_docs)]` (T004) surfaces any gaps via T044/T045
- [X] T044 Run `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` across the whole crate; fix any violations (constitution Quality Gates)
- [X] T045 [P] Run `cargo doc --no-deps` and fix any warnings (constitution Principle VI)
- [X] T046 Execute every scenario in [quickstart.md](./quickstart.md) manually against the built binary and confirm the observed behavior matches; this manual pass MUST also time the `allez --help` scenario (spec.md SC-001: a first-time user can identify all six subcommands and their basic usage within 10 seconds) and record a pass/fail note, since SC-001's timing claim has no automated equivalent
- [X] T047 Commit `Cargo.lock` (constitution Principle IX: allez is an application, lockfile MUST be committed) — verified `Cargo.lock` exists at repo root and is NOT gitignored (`git check-ignore` confirms); the actual `git add`/`git commit` is left to the orchestrator per this session's constraints
- [X] T048 [P] Cross-check every FR-001–FR-018 and every acceptance scenario in spec.md against the tests in T011-T038B; confirm 100% spec test coverage (constitution Principle VIII). Separately confirm every internal/non-public-API helper identified for unit testing (`parse_nonempty_path`, `AllezError`'s category derivation, `validate_pass_through()`, `render_success`/`render_error`, `sandbox_missing_command()`) has a corresponding `#[cfg(test)]` unit test (T005B, T007B, T007C, T008A, T040B) — this second check is distinct from the first: it verifies constitution Principle II's unit-test requirement, which black-box `assert_cmd` coverage of T011-T038B does not, by itself, satisfy — one genuine gap found (spec.md Edge Cases: "`--help`/`--version` never renders as JSON" had no positive test) and closed with a new `t048_help_and_version_output_is_never_json` test. Also confirms SC-001–SC-004 coverage explicitly (added during `/speckit.analyze` remediation): SC-001 → T046 (manual timing pass, no automated equivalent per spec.md's own note); SC-002 → T016-T022 (all six subcommands parse a valid invocation); SC-003 → T031-T038B (100% invalid invocations exit `2` with a distinguishable message); SC-004 (exit status alone distinguishes success from usage error, for all six subcommands) → the union of T016-T023I's exit-`0` happy paths and T031-T038B's exit-`2` error paths across all six subcommands — no single dedicated test is tagged to SC-004 by name, but the property is fully exercised by that union
- [X] T049 Run `cargo audit`; fix or explicitly document (with justification) any reported advisory (constitution Principle X, top-level Quality Gates — closes the gap where this ticket introduces `clap`/`serde`/`tracing`/`assert_cmd` et al. without ever running a supply-chain check against them) — installed via `cargo install cargo-audit --locked`; ran clean, 0 advisories across 84 crate dependencies
- [X] T050 Run `cargo deny check` (licenses, bans, duplicate/banned crates); fix any violation (constitution Principle X, top-level Quality Gates) — installed via `cargo install cargo-deny --locked`; generated `deny.toml` via `cargo deny init`, added `license = "BSD-3-Clause"` to `Cargo.toml` and allow-listed MIT/Apache-2.0/BSD-3-Clause/Unicode-3.0 in `deny.toml`; `advisories ok, bans ok, licenses ok, sources ok` (one benign `syn` v2/v3 duplicate-version warning remains, expected per cargo-deny's default policy, not a failure)
- [X] T051 [P] Run `cargo check --target <triple>` for each of the four target triples from plan.md's Technical Context (`x86_64-pc-windows-msvc`, `aarch64-apple-darwin`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu`; install via `rustup target add` as needed); confirm the crate compiles cleanly on all four, matching this ticket's "no platform-specific code introduced" claim — full `cargo test` execution across the same matrix on real/emulated hardware is GEN-31's CI-matrix scope, not re-established here — Unblocked after `rustup` (Homebrew formula) was installed and `rustup target add` ran for all four triples. Homebrew's `rustup` formula does not install the usual `~/.cargo/bin/{cargo,rustc}` PATH shims, so `cargo`/`rustc` on `$PATH` kept resolving to Homebrew's plain `rust` formula (no rustup awareness) even via `rustup run stable`; invoking `$(rustup which cargo)`'s toolchain dir directly (prepending `~/.rustup/toolchains/stable-aarch64-apple-darwin/bin` to `PATH`) fixed it. `cargo check --target <triple>` passed cleanly (exit 0, no errors/warnings) for all four: `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu`.
- [X] T052 [P] Run `cargo test --all` on the native development/CI platform and confirm every test passes; this satisfies the Quality Gates' "full test suite" requirement for this PR on at least one platform (constitution top-level Quality Gates), with the remaining three target triples covered by T051's compile-only check — see plan.md's Complexity Tracking for the documented deviation from running the full suite on all four triples, which is GEN-31's CI-matrix scope — 82/82 passed (17 unit + 65 integration)
- [X] T053 [P] Generate a test coverage report via `cargo llvm-cov --workspace --summary-only` (installing `cargo-llvm-cov` first if not already present); review the report and confirm no unexpectedly-uncovered public code paths, satisfying constitution Quality Gates' "with coverage report" requirement — installed via `cargo install cargo-llvm-cov --locked`; no rustup `llvm-tools-preview` component available, but Homebrew's standalone LLVM 22.1.8 exactly matches rustc's bundled LLVM version, so `LLVM_COV`/`LLVM_PROFDATA` env vars pointing at Homebrew's `llvm-cov`/`llvm-profdata` worked as a substitute. Result: 99.35% region / 100% function / 99.28% line coverage; the two uncovered lines (`main.rs:93`'s `ErrorKind` non-exhaustive catch-all, `observability.rs:23`'s double-init error branch) are both genuinely unreachable defensive paths, not real gaps
- [X] T054 Add a `CHANGELOG.md` entry at the repository root (creating the file if it does not yet exist) documenting the new user-visible `allez` CLI surface introduced by this ticket — the six subcommands (`oneshot`, `create`, `run`, `sandbox`, `list`, `remove`), the `--human`/`--verbose` output flags (JSON is the default; noting pass-through content is redacted by default, revealed under `--verbose`), the fixed `category` error enum, and the `0`/`2` exit-code convention — satisfying constitution's "All PRs MUST include ... Changelog entry for user-visible changes" requirement (depends on T046, so the entry reflects manually-verified behavior)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup completion — BLOCKS all user stories (the `Cli`/`Commands` skeleton, error/output/observability plumbing, the `parse_nonempty_path` value_parser (T005A), and the `validate_pass_through()` helper (T007A) are shared by every story)
- **User Stories (Phase 3-5)**: All depend on Foundational phase completion
  - US1 and US2 are both P1 and can proceed in parallel once Foundational is done (different files: US1 touches help text/`subcommand_required` in `src/cli/mod.rs`; US2 touches the six `src/cli/*.rs` stub files) — coordinate if both touch `src/cli/mod.rs` in the same window
  - US3 (P2)'s *tests* (T031-T038B) can be written once Foundational is done, in parallel with US1/US2, per TDD (constitution Principle II). Its *implementation* is more constrained than "can also start" implies: T039 explicitly depends on T024/T026 (US2's `oneshot`/`run` stub handlers) existing before `validate_pass_through()` can be wired ahead of them in `src/main.rs`'s dispatch match, and T040/T040A similarly depend on T027 (US2's `sandbox` stub handler) — so T039/T040/T040A cannot complete until those US2 tasks land — not merely "sequence if working solo," but a real cross-story dependency (added during `/speckit.analyze` remediation — closes finding U1). T039/T040/T040A also touch the same file (`src/main.rs`) as T010 (Foundational) and T030 (US2) — sequence these edits if working solo
- **Polish (Phase 6)**: Depends on all three user stories being complete

### User Story Dependencies

- **User Story 1 (P1)**: No dependencies on US2/US3; independently testable via `--help`/no-args/per-subcommand-help alone
- **User Story 2 (P1)**: No dependencies on US1/US3; independently testable via valid invocations of each subcommand alone
- **User Story 3 (P2)**: Builds on the same `Cli`/`Commands` shapes as US1/US2 but is independently testable via invalid invocations alone; layers `validate_pass_through()` calls (T007A) and the `sandbox`-specific raw-argv check (T040A) into `main.rs`'s dispatch (T039, T040), at the same site US2's T030 wires the six handlers into

### Within Each User Story

- Tests MUST be written and FAIL before implementation (constitution Principle II)
- Foundational struct/type definitions before per-story implementation
- Story complete (all its tests passing) before moving to the next priority

### Parallel Opportunities

- T003 and T004 (Setup) can run in parallel — different files
- T004B (Setup) runs after T004, not in parallel with it — both edit `src/main.rs`'s crate-root attributes (an earlier draft incorrectly marked both `[P]`)
- T005A (Foundational: `parse_nonempty_path`) can run in parallel with T005 — no shared state, both land in `src/cli/mod.rs` but as independent, non-overlapping functions/types; sequence the actual file edit if working solo, same as T005/T006. T005B (its unit tests, same file) is NOT marked `[P]` relative to T005A for the same same-file reason, even though it must be *written* before T005A's real logic per TDD
- T007, T008, T009 (Foundational: `error.rs`, `output.rs`, `observability.rs`) can run in parallel — different files, no interdependencies; T007A depends on T005+T007 and runs after. T007B (unit tests for T007, in `error.rs`) and T008A (unit tests for T008, in `output.rs`) can run in parallel with each other and with T009, for the same different-file reasoning; T007C (unit tests for T007A, in `cli/mod.rs`) sequences after T007B for the same same-file reason as T005A/T005B. T040B (unit tests for T040A, added in US3) sequences after T040A for the same reason
- T024-T029 (US2 implementation: six stub handlers) can all run in parallel — six different files
- T043 and T045 (Polish) can run in parallel with each other; T049, T050, T051, T052, T053 (Polish) can all run in parallel with each other and with T043/T045 — independent checks, no shared files. T054 (Changelog) depends on T046 completing first

---

## Parallel Example: User Story 2 Implementation

```bash
# Once Foundational (T005-T010, including T007A) is complete, launch all six stub handlers together:
Task: "Implement the oneshot stub handler in src/cli/oneshot.rs"
Task: "Implement the create stub handler in src/cli/create.rs"
Task: "Implement the run stub handler in src/cli/run.rs"
Task: "Implement the sandbox stub handler in src/cli/sandbox.rs"
Task: "Implement the list stub handler in src/cli/list.rs"
Task: "Implement the remove stub handler in src/cli/remove.rs"
```

---

## Implementation Strategy

### MVP First (User Stories 1 + 2 — both Priority P1)

This spec assigns **two** co-primary (P1) stories: discoverability (US1) alone is not a meaningful scaffold without correct parsing/routing (US2), and vice versa. The MVP for this ticket is both together.

**⚠️ This MVP does NOT satisfy the Jira ticket's full acceptance criteria.** GEN-22's 4th acceptance criterion ("exit codes and error messages follow a consistent convention across subcommands") is entirely US3's scope (P2, Phase 5) and is deliberately excluded from this MVP slice — do not report GEN-22 as done after US1+US2 alone.

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories)
3. Complete Phase 3: User Story 1
4. Complete Phase 4: User Story 2
5. **STOP and VALIDATE**: Run T011-T023I; confirm `allez --help`/`--version` and all six subcommands' happy paths (including zero-package invocations, each subcommand's default JSON mode and `--human` in both placement windows, default redaction, and `--verbose` reveal) work
6. This is the deliverable MVP for GEN-22 — matches the ticket's first three acceptance criteria

### Incremental Delivery

1. Complete Setup + Foundational → Foundation ready
2. Add User Story 1 + User Story 2 together → Test independently → MVP for this ticket
3. Add User Story 3 → Test independently → Full ticket acceptance criteria satisfied (including "exit codes and error messages follow a consistent convention")
4. Add Polish → Ship

### Parallel Team Strategy

With multiple developers, after Foundational is done:

- Developer A: User Story 1 (help text, `src/cli/mod.rs` help attributes)
- Developer B: User Story 2 (six independent stub handler files)
- Developer C: User Story 3 (error consistency, coordinate on `src/cli/mod.rs` with Developer A)

---

## Notes

- [P] tasks = different files, no dependencies — same-file tasks (e.g., all tests in `tests/cli_scaffold.rs`, or successive edits to `src/cli/mod.rs`/`src/main.rs`) are intentionally left unmarked even when logically independent, to avoid merge conflicts from concurrent edits to one file
- [Story] label maps task to specific user story for traceability
- Every acceptance scenario in spec.md has exactly one corresponding test task above (T011-T038B); T048 verifies this cross-check explicitly. Additional edge-case/regression tests beyond the spec's literal acceptance scenarios (T013A, T016A, T017A, T023A-T023I, T031A, T033A, T035A, T038A, T038B) exist to close gaps identified during plan review and `/speckit.analyze` remediation — they map to spec.md's Edge Cases/Assumptions or FR-013–FR-018 text even where no separate numbered acceptance scenario exists
- T005B, T007B, T007C, T008A, T040B are `#[cfg(test)]` unit tests for internal/non-public-API helper functions, distinct from and additional to the T011-T038B black-box `assert_cmd` integration tests above — they satisfy constitution Principle II's separate "unit tests... live alongside the code under test" requirement (added during `/speckit.analyze` remediation — closes finding D1: plan.md's Testing section committed to this with no covering tasks in an earlier draft) and are not counted against the T011-T038B acceptance-scenario range
- Verify tests fail before implementing (constitution Principle II)
- Commit after each task or logical group
- Stop at any checkpoint to validate story independently

---

## Phase 7: Convergence

**Purpose**: Close gaps found by `/speckit.converge` between spec.md/plan.md/the constitution and the implementation completed in Phases 1-6. Constitution-violation tasks are listed first (CRITICAL).

- [X] T055 Emit structured `tracing::*!` events (with consistent fields — operation/subcommand, result) from `src/main.rs`'s `dispatch()` and/or each `src/cli/<subcommand>.rs` stub handler, so the `tracing-subscriber` initialized in `observability::init()` (T009) actually produces stderr output; currently no `tracing::*!`/`#[instrument]` call exists anywhere in `src/`, leaving the subscriber wired but silent, per Constitution XI (missing) — closed during post-review remediation: `dispatch()` now emits one `tracing::info!`/`tracing::warn!` event per subcommand arm (`operation`/`result`/`category` fields, counts only, never redacted pass-through content); `observability::init()` moved to run unconditionally before `Cli::try_parse()` so `RUST_LOG` also covers parse-time errors. Closing this surfaced a second, previously-latent bug fixed in the same pass: the pre-existing default log filter (`"info"`, since T009) had never actually been exercised until these call sites existed, and firing on every invocation by default would have polluted the fixed single-JSON-object stderr contract T031-T038 depend on — the default filter is now `"off"` (opt-in via `RUST_LOG`, standard Rust-CLI convention), verified via new tests t063/t063a/t063b
- [X] T056 Commit `Cargo.lock` to version control (git add + git commit, requires explicit user authorization per this project's git workflow) — `git status` currently shows it untracked, leaving Constitution IX's "`Cargo.lock` MUST be committed to version control" unsatisfied per Constitution IX (missing) — **this note was already stale by the time it was written**: `git ls-files Cargo.lock` confirms it has been tracked and committed since the initial `Implement` commit; independently re-verified during post-review remediation (`git status --short` shows no pending changes to it). No action was ever needed; correcting the record here rather than deleting it so the discrepancy stays visible
- [X] T057 Give the `oneshot`/`run`/`sandbox` pass-through argument (`PassThroughArgs` in `src/cli/mod.rs`) a `value_name`/doc comment that names and describes the required `COMMAND` token distinctly from its own `[ARGS...]`, so `allez oneshot --help`/`run --help`/`sandbox --help` match contracts/cli-schema.md's documented `-- <COMMAND> [COMMAND ARGS...]` synopsis instead of the current `[-- <ARGS>...]` (which never mentions `COMMAND`); extend T013's test to assert the help text names the command argument per FR-002 (partial) — closed during post-review remediation: `override_usage` on each per-subcommand wrapper (`PackagesAndCommandArgs`, `RunArgs`, and a new `SandboxArgs` wrapper introduced specifically so `sandbox` doesn't share `PassThroughArgs` directly as its command payload) now renders the exact literal `-- <COMMAND> [ARGS...]`/`[-- <COMMAND> [ARGS...]]` synopsis per subcommand; `override_usage` deliberately does NOT live on the shared `PassThroughArgs` type itself — confirmed empirically against clap 4.6.x that a `#[command(override_usage=...)]` container attribute on a `#[command(flatten)]`-ed type leaks into every flattening site, which would have shown `sandbox`'s usage line under `oneshot --help`. New test t060 locks this in
- [X] T058 Add `# Examples` runnable-example doc sections, where practical, to the non-trivial public functions currently missing them — `parse_nonempty_path`, `PassThroughArgs::split_program`, `validate_pass_through`, `sandbox_missing_command`, `render_success`, `render_error`, `render_pass_through` (`src/cli/mod.rs`, `src/output.rs`) per Constitution VI (partial) — closed during post-review remediation: all seven now carry a fenced example. Marked ```rust,ignore rather than a real doctest since `allez` is a binary-only crate (no `[lib]` target, so `cargo test --doc` has nothing to run against) — the example still renders correctly under `cargo doc` and documents intended usage; the equivalent assertions already exist as executable `#[cfg(test)]` unit tests in the same files

## Phase 8: Post-Review Remediation (this same PR)

**Purpose**: Fixes for issues a 5-lane parallel review (goal/constraint, code quality, security, hands-on QA, context-mining) surfaced beyond the Phase 7 self-identified gaps above. All items below are new findings, not re-statements of T055-T058.

- [X] T059 Fix `map_parse_error()`/`render_error()` mislabeling extra, unconsumable positional arguments (`allez list foo`, `remove ./env extra`, `sandbox foo`) as `message: "unrecognized flag"` when no flag was involved — clap's `UnknownArgument` `ErrorKind` covers both "unknown flag" and "unexpected extra argument" cases, but the old fixed per-`AllezError`-variant `Display` string only had flag-specific wording. `category` stays `unknown_flag` (FR-017's enum is frozen; adding a 5th category is a contract change out of scope here) — only `message` changes, now sourced from clap's own rendered text (`clap_parse_error_message()` in `src/main.rs`) instead of a hardcoded string. `output::render_error()`'s signature changed from `(&AllezError, bool)` to `(&str, &str, bool)` accordingly. New test t059
- [X] T060 Repeated global boolean flags (`allez --human --human list`, `list -vv`, `--human list --human`) previously failed with exit `2`/`ArgumentConflict` — clap's `global = true` propagation treats a second occurrence of the same global flag as conflicting with itself by default. Fixed via `overrides_with = "human"`/`overrides_with = "verbose"` self-references on `Cli`'s `human`/`verbose` fields (confirmed empirically against clap 4.6.x in an isolated scratch project before applying to this repo). New test t061
- [X] T061 `allez --human` (or any lone global flag) with no subcommand previously rendered a terse one-line JSON/human error (`missing_argument`) instead of the full multi-line usage/help dump bare `allez` (zero args) produces — both are "no subcommand given," but diverged in presentation purely because `arg_required_else_help`'s special-cased `ErrorKind`s didn't include plain `MissingSubcommand`. Fixed by adding `ErrorKind::MissingSubcommand` to `main.rs`'s help-dump `matches!` arm. New test t062
- [X] T062 Added `.github/workflows/ci.yml`: `cargo test --all` on `ubuntu-latest`/`macos-latest`/`windows-latest` (closes the Quality Gates' "run on all supported target platforms (Linux, macOS, Windows)" clause automatically, superseding the previous manual-macOS-only + compile-check-elsewhere approach the Constitution Check/Complexity Tracking sections of plan.md had documented as a temporary deviation) plus a `cargo check --target aarch64-unknown-linux-gnu` job for the 4th target triple. See plan.md's updated Constitution Check (Principle II) and Complexity Tracking sections
