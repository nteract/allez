# Implementation Plan: CLI Scaffold and Subcommand Routing

**Branch**: `GEN-22_cli_scaffold_routing` | **Date**: 2026-07-21 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/GEN-22_cli_scaffold_routing/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command; its definition describes the execution workflow.

## Summary

Stand up the `allez` Rust binary's CLI surface: a `clap`-derive-based `Cli`/`Commands` type covering all six subcommands (`oneshot`, `create`, `run`, `sandbox`, `list`, `remove`), each routed to a stub handler that acknowledges its parsed arguments without performing real environment work.

- **Parser & Routing**: `clap` v4 derive API (`Cli`/`Commands` enum) with a shared `#[derive(Args)]` `PassThroughArgs` struct capturing the `--` pass-through-command shape (FR-003–008, FR-012); `oneshot`/`run` enforce the pass-through command's required-ness via a shared post-parse validation function called from `main.rs`'s dispatch — before either handler's core logic is invoked at all, not from inside the handler bodies (FR-010) — rather than a clap-level `required` attribute, since `PassThroughArgs` is reused unconditionally-optional by `sandbox` in its no-`--`-at-all case (see research.md Decision 1 and data-model.md's Pass-Through Command entity for the rationale). `sandbox`'s dispatch arm additionally runs its own raw-argv (`std::env::args_os()`) check — not the shared validation function unmodified — to distinguish "no `--` at all" (non-error, interactive subshell) from "`--` present, nothing after it" (usage error, `missing_pass_through_command`, FR-006), a distinction clap's own match state cannot make for a `#[arg(last = true)]` field (research.md Decision 1, empirically confirmed). `Environment Path` fields (`create`/`run`/`remove`) are non-empty-validated via a shared clap `value_parser`, `parse_nonempty_path` (FR-015, research.md Decision 7) — enforced during `try_parse()` itself, not a separate post-parse step.
- **Handler & Stub Behavior**: each of the six subcommands routes to a distinct stub handler (FR-009) that acknowledges its parsed arguments — in the normatively fixed minimal shape `{schema_version, subcommand, status, parsed}` (FR-014), with pass-through `program`/`args` redacted to `{program: "<redacted>", arg_count: N}` by default and revealed in full only under `--verbose`/`-v` (FR-016, research.md Decision 5) — without performing real environment work; help text is established for the top level and every subcommand (FR-001/002).
- **Output & Error Contract**: `--human`/`--verbose` output-mode plumbing (FR-013, FR-016), both declared `global = true` on `Cli` so they may appear before or after the subcommand name but never after `--` (FR-012 is absolute — a token after `--` is always forwarded to the pass-through command, never reinterpreted as an allez flag); JSON is the unconditional default (no `--format`/`--json` flag, no TTY-based default — see research.md Decision 2 for why); a versioned (`schema_version`) but content-minimal stub payload on stdout; a two-tier exit-code convention of `0`/`2` that clap's `try_parse()` path renders consistently for both success and usage errors (FR-011), with a `category` field drawn from one fixed, closed enum (FR-017, research.md Decision 6) on JSON error bodies, all written to stderr alongside the human-readable message.
- **Testing Approach**: `assert_cmd`+`predicates`+`rstest` black-box integration tests covering 100% of the spec's acceptance scenarios, `tracing`+`tracing-subscriber` for structured logs to stderr (separate from the stdout result payload), `serde`/`serde_json` for the primary JSON output mode.

## Technical Context

**Language/Version**: Rust, stable toolchain, 2024 edition — set explicitly (`edition = "2024"` in `Cargo.toml`, T001) rather than left to `cargo init`'s toolchain-dependent default, which may differ from this Technical Context's stated edition on some installs

**Primary Dependencies**: `clap` (v4, `derive` feature) for argument parsing/subcommand routing/help generation; `serde` + `serde_json` for the machine-readable output mode; `tracing` + `tracing-subscriber` for structured, dual-format (human/JSON) observability

**Storage**: N/A — this ticket only parses and routes; no persistence, no filesystem or network side effects (deferred to GEN-23/24/26/27)

**Testing**: `cargo test`; unit tests in `#[cfg(test)]` modules alongside code under test (constitution Principle II); black-box integration tests in `tests/` using `assert_cmd` + `predicates` (+ `rstest` for table-driven cases across the six subcommands), one test per spec acceptance scenario (constitution Principle VIII)

**Target Platform**: Windows amd64, macOS aarch64, Linux aarch64, Linux amd64 (per epic GEN-19's installation targets; this ticket introduces no platform-specific code, so no special handling is required here — cross-platform *release* automation itself is GEN-31's scope). This ticket verifies only that the crate *compiles* for all four target triples (`cargo check --target <triple>`, tasks.md T051); full cross-platform `cargo test` execution on real hardware/emulation for every PR is GEN-31's CI-matrix scope, not re-established here.

**Project Type**: Single-project CLI binary

**Performance Goals**: N/A for this ticket — argument parsing/routing is a negligible, one-time cost per invocation; no target is meaningful at this scope

**Constraints**: `Cargo.lock` MUST be committed (constitution Principle IX, allez is an application); MUST build and pass tests on all four target platforms; no `.unwrap()`/`.expect()` outside tests (constitution Principle V); `--human`/`--verbose` are `global = true` clap flags valid before or after the subcommand name but never after `--` (FR-013); JSON is the unconditional default when `--human` is omitted, not a TTY-detected one (FR-013, research.md Decision 2); pass-through argv redacted by default (FR-016); `Environment Path` non-emptiness and error `category` values are each enforced through one shared function/enum, not per-subcommand duplication (FR-015, FR-017)

**Scale/Scope**: Single-process CLI; six subcommands; no concurrency, no multi-user concerns at this scope

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|---|---|---|
| I. Code Quality | PASS | `clap` derive keeps each subcommand's arg definition single-purpose; shared `Args` structs avoid ad hoc complexity; `cargo fmt`/`clippy -D warnings` are the CI gate, unaffected by this ticket's design |
| II. Testing Standards | PASS (scope-limited) | Plan requires integration tests (`assert_cmd`) written before stub handlers, one per acceptance scenario, per TDD; full `cargo test` execution is gated on the native CI platform for this PR (T052), with the remaining three target triples covered by compile-only checks (T051) — full N-platform `cargo test` execution is GEN-31's CI-matrix scope; see Complexity Tracking for the documented deviation from the Quality Gates' "run on all supported target platforms" clause |
| III. Dual-Primary Interface | PASS | FR-013 establishes a single `--human` flag (JSON is the unconditional default) plumbing on all six subcommands starting with this ticket, with a mandatory `schema_version` field and a normatively fixed minimal `{schema_version, subcommand, status, parsed}` shape on the success payload (FR-014) — satisfies "documented, versioned schema" by the letter from day one, and is precise enough that two independent implementations produce test-comparable output, not just syntactically-valid-but-divergent JSON. Format selection is explicit (`--human`), not TTY-context-aware — the constitution permits either mechanism ("Format selection MUST be explicit... or context-aware... either way, documented"); TTY detection was evaluated and rejected specifically because it cannot reliably distinguish an agent caller from a human one (research.md Decision 2), so the unconditional-default form of "explicit" is used instead, and is documented here and in research.md/spec.md's Clarifications. The "verbose/trace output suitable for interactive debugging" half of this principle is satisfied by two distinct, complementary mechanisms, not one: `--verbose`/`-v` (FR-016) reveals the redacted-by-default pass-through `program`/`args` in the primary result payload (stdout), while `tracing`/`tracing-subscriber`'s startup-selected `.pretty()` formatter (research.md §3) provides human-readable structured trace/log output on stderr, independent of `--verbose`. Non-version/non-verbose payload fields stay minimal until each subcommand's real behavior lands (resolved via `/speckit.analyze` finding D1, tightened during post-review remediation) |
| IV. DRY | PASS | Shared `PassThroughArgs`/`PackagesAndCommandArgs` structs (tuple variants) eliminate duplicating the `--` capture field across `oneshot`/`run`/`sandbox` |
| V. Explicit Over Implicit | PASS | `Commands` enum makes invalid subcommand combinations unrepresentable; clap's typed `Result`-based error path (via `try_parse()`, see main.rs) replaces manual panics; no `.unwrap()`/`.expect()` in library code, mechanically enforced via `clippy::unwrap_used`/`clippy::expect_used` denied at the crate root (T004B) rather than left to manual review |
| VI. Documentation and Type Safety | PASS | Public `Cli`/`Commands`/`Args` types and every other new public item (error/output/observability modules, all six stub handlers) get doc comments, enforced via `#![warn(missing_docs)]` (resolved via `/speckit.analyze` finding E1); `cargo doc` gate applies unchanged |
| VII. No Hardcoded Values | PASS | No config values introduced this ticket; subcommand list and help text are generated once from the `clap` derive macros (single source of truth), not duplicated string literals scattered across modules. This claim is verified, not merely asserted: `cargo clippy` (T044) reviews confirm no ad hoc literals appear outside `clap` attribute macros |
| VIII. Mandatory 100% Spec Test Coverage | PASS | Every FR/acceptance scenario in spec.md maps to at least one `assert_cmd` integration test (tracked explicitly in tasks.md) |
| IX. Determinism & Idempotency | PASS | Parsing/routing is a pure, stateless function of argv; `Cargo.lock` committed; no environment side effects exist yet to reason about idempotency for (deferred) |
| X. Security & Supply-Chain | PASS (scope-limited) | `cargo audit`/`cargo deny` CI gates apply unchanged; no package download/signature verification is in scope for this ticket (that's GEN-29's auth work and later resolution tickets) |
| XI. Structured Observability | PASS | `tracing`+`tracing-subscriber` wired at scaffold time with a startup-selected human/JSON formatter (per research.md §3), stderr for logs, stdout reserved for FR-013's primary output; error bodies carry a `category` field drawn from a fixed, closed enum (FR-017, resolved via `/speckit.analyze` finding F1, tightened during post-review remediation from an open string to an enum) alongside the human-readable message |

**One documented deviation exists** (Principle II, above): full `cargo test --all` execution is gated on the native CI platform only for this PR (T052), with the remaining three target triples covered by compile-only `cargo check --target <triple>` checks (T051) rather than full test execution, because no CI matrix for the four target triples exists yet (that's GEN-31's dedicated scope). See Complexity Tracking below for the full rationale and rejected alternatives. This is the only Constitution Check violation this ticket carries; every other principle above is a clean, unqualified PASS. Per the constitution's Governance section ("Deviations MUST be documented and approved by maintainers"), this deviation is documented and justified here, but requires explicit maintainer sign-off at PR review before merge — it is not yet approved as of this planning artifact (added during `/speckit.analyze` remediation — closes finding K1). Update: now approved.

## Project Structure

### Documentation (this feature)

```text
specs/GEN-22_cli_scaffold_routing/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/
│   └── cli-schema.md    # Phase 1 output (/speckit.plan command)
└── tasks.md              # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
Cargo.toml
Cargo.lock

src/
├── main.rs               # entry point: Cli::try_parse(), route Err via error.rs's human/JSON renderer (exit 2); on Ok(cli), dispatch on Commands — calling validate_pass_through() for Oneshot/Run, and a distinct raw-argv `--`-presence check for Sandbox, *before* invoking the matched handler's core logic (FR-010), never inside the handler bodies — then invoke the handler (exit 0)
├── cli/
│   ├── mod.rs             # Cli struct (with global --human/--verbose flags, JSON default), Commands enum, top-level parse+dispatch, shared Args structs, validate_pass_through() helper, parse_nonempty_path() shared value_parser (FR-015)
│   ├── oneshot.rs         # OneshotArgs (via PackagesAndCommandArgs) + stub handler (validate_pass_through() is called by main.rs's dispatch before this handler is invoked, not from within it)
│   ├── create.rs          # CreateArgs (path via parse_nonempty_path + packages; independent type, no PassThroughArgs/PackagesAndCommandArgs — create has no pass-through command) + stub handler
│   ├── run.rs              # RunArgs (path via parse_nonempty_path + PassThroughArgs) + stub handler (validate_pass_through() called by main.rs's dispatch, as with oneshot)
│   ├── sandbox.rs         # SandboxArgs (PassThroughArgs, optional) + stub handler — main.rs's dispatch does not call validate_pass_through() unmodified for this arm; it instead runs a distinct raw-argv `--`-presence check (see research.md Decision 1) to reject `--` present-with-nothing-after while still accepting `--` absent entirely
│   ├── list.rs              # ListArgs (empty) + stub handler
│   └── remove.rs          # RemoveArgs (path via parse_nonempty_path) + stub handler
├── output.rs               # --human/--verbose selection: json (default) vs. human rendering of stub acknowledgments in the fixed {schema_version, subcommand, status, parsed} shape (FR-014), with pass-through program/args redacted by default and revealed under --verbose (FR-016)
├── error.rs                 # AllezError enum; maps to clap's exit-code convention (0/2); each variant's category name (missing_argument/unknown_subcommand/unknown_flag/missing_pass_through_command, FR-017) is the single source of truth for the JSON error body's `category` field — no separate hand-maintained string list
└── observability.rs      # tracing-subscriber init (human/json formatter, stderr writer)

tests/
└── cli_scaffold.rs        # assert_cmd + rstest integration tests, one case per spec acceptance scenario
```

**Structure Decision**: Single-project CLI binary (Option 1 from the template, specialized for this feature). No `models/`/`services/`/`lib/` split is warranted yet — this ticket has no domain logic beyond parsing and routing, so `src/cli/` (one file per subcommand, sharing `Args` structs from `cli/mod.rs`) plus three small cross-cutting modules (`output.rs`, `error.rs`, `observability.rs`) is the minimal structure that satisfies DRY and single-responsibility without speculative layering. Later tickets (GEN-23–GEN-30) that add real environment/package logic will introduce `src/env/`, `src/package/`, etc. as needed — not created here to avoid unused scaffolding (constitution Principle I: complexity must be justified).

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|---------------------------------------|
| Quality Gates' "full test suite ... run on all supported target platforms (Linux, macOS, Windows)" is satisfied on only the native CI platform for this PR (T052), not all four target triples | No CI matrix for the four target triples (`x86_64-pc-windows-msvc`, `aarch64-apple-darwin`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu`) exists in this repo yet; standing one up is GEN-31's dedicated, explicit scope. Gating this scaffolding PR on infrastructure that doesn't exist yet would block an otherwise-correct, verified change indefinitely | Skipping compile verification entirely on the non-native triples (leaving only native `cargo test`) would silently regress platform coverage with no signal at all; skipping the native-platform full-suite run entirely (the ticket's original scope) would leave zero automated test-execution gate before merge, which the Quality Gates do not permit as a silent gap. Combining native `cargo test` (T052) + all-triple `cargo check` (T051) is the closest approximation available without GEN-31's infrastructure |
