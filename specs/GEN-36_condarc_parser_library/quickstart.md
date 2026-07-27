# Quickstart: `.condarc` Parser Library (GEN-36)

Validation guide for proving the crate works end-to-end. This is a *run guide* — it references
`data-model.md` and `contracts/` for exact shapes rather than duplicating them, and contains no
implementation code (module bodies belong to the implementation phase / `tasks.md`).

## Prerequisites

- Rust toolchain matching `Cargo.toml`'s `rust-version` (workspace edition 2024).
- Repo checked out at this feature branch (`GEN-36_condarc_parser_library`), with the workspace
  restructure applied (root `allez` package retained, new `crates/condarc/` member — plan.md
  "Project Structure").
- No external services, network, or VPN required — the crate is hermetic by default
  (`ParseOptions::default()`, `data-model.md` §6).

## Setup

```sh
# From the workspace root:
cargo build --workspace
```

Confirms the workspace restructure compiles and the new `crates/condarc` member resolves.

## Scenario 1 — Parse a valid `.condarc` (User Story 1, SC-001/SC-002)

**Goal**: every `conformance/condarc/valid/*.json` fixture is accepted and its adapted output
equals `conformance/condarc/expected/*.json` exactly.

```sh
cargo test --workspace --test condarc_conformance -- --nocapture
```

**Expected outcome**: the `Crate` checker in `tests/condarc_conformance.rs` (previously skipped, per
the ticket's starting state) now runs and passes for every fixture in `conformance/condarc/valid/` and
`conformance/condarc/invalid/`, alongside the pre-existing `conda` and `openapi` checkers on the same
fixture set (spec SC-003). The checker calls `parse_with_options(.., ssl_verify_fs_check: true,
null_sequence_map_defaults: true)` (spec A3, A7) and compares adapted output to `expected/` for
**exact** equality. See `contracts/public-api.md`
§"End-to-end usage" #1–3 for the shape of a successful `parse()` call and how to read the resulting
typed `Config`, and `contracts/adapter-output.md` for exactly how the conformance comparison is
performed.

**Ad hoc check** (a single fixture, useful while implementing):

```sh
cargo test --workspace -p condarc -- --nocapture channels_and_priority
```

(substitute any unit test name added under `crates/condarc/src/**/#[cfg(test)]` for a specific
coercion rule — Constitution II requires these to exist *before* the corresponding production
code, per TDD).

## Scenario 2 — Reject an invalid `.condarc` with a complete error report (User Story 2, SC-005)

**Goal**: a document with several independently-invalid settings produces one report entry per
invalid setting, not just the first.

```sh
cargo test --workspace -p condarc multi_error_accumulation -- --nocapture
```

**Expected outcome**: a hand-built fixture (a bad `channel_alias`, an out-of-range
`remote_max_retries`, and a non-boolish `always_copy` in one document) yields a
`ValidationReport` whose `entries()` has exactly 3 members, each independently locatable — see
`contracts/public-api.md` §"End-to-end usage" #2 for the exact iteration/branching pattern a
caller (and this test) uses, and `contracts/error-report.schema.json` for the JSON shape asserted
against.

## Scenario 3 — Adapter exact comparison (User Story 3, SC-002)

Already exercised by Scenario 1's `cargo test --test condarc_conformance` run (the `Crate` checker
*is* the adapter comparison). To inspect the adapter's raw output for a single fixture while
debugging a mismatch:

```sh
cargo test --test condarc_conformance --features conformance-tests -- --nocapture dump_adapted_json
```

(a small diagnostic test, added during implementation, that prints `to_expected_json(&cfg)` for a
named fixture — see `contracts/adapter-output.md` for the exact encoding rules it must satisfy.)

## Scenario 4 — Caller integration smoke test (contracts/public-api.md end-to-end usage)

**Goal**: confirm the public API contract (not just the conformance corpus) — reading a file,
handling a missing file, handling a rejected document, using typed values, and interpreting an
unmodeled key via `extra_as`.

```sh
cargo test --workspace -p condarc --test public_api_usage -- --nocapture
```

**Expected outcome**: an integration test under `crates/condarc/tests/public_api_usage.rs`
exercises, at minimum, each of the six numbered usage patterns in `contracts/public-api.md`
§"End-to-end usage": (1) `parse()` from file text with a missing-file fallback, (2) iterating
`ValidationReport::entries()` and branching on `ErrorKind`, (3) reading a handful of typed `Config`
fields (`channels`, `channel_priority`, `always_yes`'s tri-state), (4) `parse_with_options` with
`ssl_verify_fs_check: true` against a fixture that points at a real, existing test-temp-dir path,
(4b) `parse_with_options` with `null_sequence_map_defaults: true` resolving an explicit `null` on a
sequence-/map-typed setting to conda's own default (Assumption A7), and (5) `Config::extra_as::<T>()`
against a document containing conda-build's four out-of-scope keys (`croot`, `bld_path`,
`anaconda_upload`, `conda_build`).

## Full quality gate (pre-merge — Constitution "Quality Gates")

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo deny check          # confirms yaml-rust2 clears license/advisory checks (research R1)
cargo audit
cargo test --workspace
cargo doc --workspace --no-deps
```

All six MUST pass with zero warnings/failures before this feature is mergeable.

## Traceability (spec coverage — Constitution VIII)

Every acceptance scenario in `spec.md` and every FR maps to at least one test:

| Spec section | Test location |
|---|---|
| User Story 1 (valid parse) | `tests/condarc_conformance.rs` `Crate` checker + `crates/condarc/src/**/#[cfg(test)]` unit tests per coercion rule |
| User Story 2 (structured errors) | `tests/condarc_conformance.rs` (`invalid/` fixtures) + `crates/condarc/tests/multi_error_accumulation.rs` |
| User Story 3 (adapter) | `tests/condarc_conformance.rs` (exact comparison) + its `tests/support/adapter.rs` module |
| FR-001..008 (API surface, root shape, single-document + string-key limits) | `crates/condarc/src/parse.rs` unit tests |
| FR-009..026 (catalog, typing, coercion) | `crates/condarc/src/coerce/*.rs` unit tests, one module per `ValueKind` family (`data-model.md` §4) |
| FR-027..029 (cross-field, alias collision) | `crates/condarc/src/validate.rs` unit tests |
| FR-030..037 (error accumulation & shape) | `crates/condarc/tests/multi_error_accumulation.rs` + `contracts/error-report.schema.json` conformance |
| FR-038 (absent settings, no defaulting) | `crates/condarc/src/model.rs` unit tests (`Config::default()` all-`None`) |
| FR-040..042 (adapter, harness wiring) | `tests/condarc_conformance.rs` + `contracts/adapter-output.md` |
| Assumptions A1–A7 (numeric bound + divergence list, crash-to-error, ssl_verify FS opt-in, ASCII-only digits, YAML simple-key limit + divergence lists, null-sequence-map-defaults opt-in) | `crates/condarc/src/coerce/numeric.rs`, `crates/condarc/src/parse.rs` (`conda_sequence_map_default`), `crates/condarc/tests/public_api_usage.rs` (Scenario 4 above), `tests/support/adapter.rs`'s divergence lists |

A `cargo llvm-cov` (or equivalent) coverage report generated in CI is the mechanical enforcement of
"no decrease in test coverage percentage" (Constitution Quality Gates); this table is the
human-readable cross-check that every *documented* behavior specifically has a home, not merely
that some line of code executed.
