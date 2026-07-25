---

description: "Task list for `.condarc` Parser Library (GEN-36)"
---

# Tasks: `.condarc` Parser Library

**Input**: Design documents from `/specs/GEN-36_condarc_parser_library/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/public-api.md, contracts/adapter-output.md, contracts/error-report.schema.json, quickstart.md

**Tests**: Included. The plan's Constitution Check (II. Testing Standards) mandates TDD against the already-committed conformance corpus ("unit tests written before implementation... Red→Green→Refactor"), so unit/integration test tasks are generated alongside implementation tasks throughout, not treated as optional.

**Organization**: Tasks are grouped by user story (spec.md priorities: US1 = P1, US2 = P1, US3 = P2) so each can be implemented and (as much as the shared coercion pipeline allows) tested independently.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1/US2/US3)
- Every task includes an exact file path

## Path Conventions

Per plan.md's "Project Structure": a Cargo workspace. The existing `allez` binary package stays at
the repo root; the new library is `crates/condarc/`. The conformance harness and its test-only
adapter stay at the repo root's `tests/` (unchanged path, per research R10).

- Library source: `crates/condarc/src/`
- Library's own integration tests: `crates/condarc/tests/`
- Root harness + test-only adapter: `tests/condarc_conformance.rs`, `tests/support/`
- Conformance corpus (read-only, already committed): `conformance/condarc/{valid,invalid,expected}/*.json`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace restructure and crate scaffolding so `cargo build --workspace` succeeds before any behavior is implemented.

- [ ] T001 Convert the repo-root manifest into a Cargo workspace: add a `[workspace]` table with `members = [".", "crates/condarc"]` to `Cargo.toml`, and add `condarc = { path = "crates/condarc" }` as a dev-dependency of the `allez` package (research R10)
- [ ] T002 [P] Create `crates/condarc/Cargo.toml` (`name = "condarc"`, `edition = "2024"`, deps: `yaml-rust2`, `serde` with `derive`, `serde_json`, optionally `thiserror`) per plan.md's Project Structure
- [ ] T003 Create crate skeleton so the workspace compiles: `crates/condarc/src/lib.rs`, `crates/condarc/src/parse.rs`, `crates/condarc/src/catalog.rs`, `crates/condarc/src/model.rs`, `crates/condarc/src/validate.rs`, `crates/condarc/src/error.rs`, `crates/condarc/src/coerce/mod.rs`, `crates/condarc/src/coerce/boolish.rs`, `crates/condarc/src/coerce/numeric.rs`, `crates/condarc/src/coerce/enums.rs`, `crates/condarc/src/coerce/strings.rs`, `crates/condarc/src/coerce/sequences.rs` (each an empty stub with a `mod` declaration wired from `lib.rs`)
- [ ] T004 Add `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]` and any other crate-level lint attributes to `crates/condarc/src/lib.rs` (Constitution V, plan.md Constraints)
- [ ] T005 Run `cargo deny check` and `cargo audit` against the new `yaml-rust2` dependency to confirm it clears license/advisory checks (research R1); only touch `deny.toml` if an unexpected exception surfaces
- [ ] T006 Run `cargo build --workspace` and commit the regenerated `Cargo.lock` (Constitution IX)

**Checkpoint**: `cargo build --workspace` succeeds with an empty, un-implemented `condarc` crate.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: The shared types and tables every user story's coercion/validation code is written against. Both US1 (accept) and US2 (reject) dispatch off the same `CATALOG`/`ValueKind`/`Config`/error types, so these MUST exist first.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [ ] T007 [P] Define the private `RawValue` enum (`Null`, `Bool(bool)`, `Int(i64)`, `Float(f64)`, `Str(String)`, `Seq(Vec<RawValue>)`, `Map(IndexMap<String, RawValue>)`) in `crates/condarc/src/parse.rs` (data-model.md §1)
- [ ] T008 [P] Define `ValueKind`/`EnumKind`, the `Setting` struct, and the full 99-entry `CATALOG` static table (canonical names, aliases, `ValueKind`, validators, in declaration order per data-model.md §5) in `crates/condarc/src/catalog.rs`
- [ ] T009 [P] Define the `Config` struct with all 99 recognized-setting fields (`Option<_>`/`Option<Option<_>>` per data-model.md §2.1) plus `extra: HashMap<String, serde_json::Value>` in `crates/condarc/src/model.rs`
- [ ] T010 [P] Define the error model — `ValidationReport`, `ErrorEntry`, `Location`, `PathSegment`, `ErrorKind`, `InputRepr`, and the `SchemaVersion`/`SCHEMA_VERSION` constant — with `serde::Serialize` attrs matching `contracts/error-report.schema.json` exactly, in `crates/condarc/src/error.rs` (data-model.md §7)
- [ ] T011 Implement `lower(&yaml_rust2::Yaml) -> RawValue`, the hand-written recursive match from `yaml_rust2::Yaml` into `RawValue`, including the shape needed to detect a non-string mapping key and a multi-document stream later, in `crates/condarc/src/parse.rs` (depends on T007)
- [ ] T012 Define the supporting value types (`ChannelPriority`, `PathConflict`, `SafetyChecks`, `SatSolver`, `BoolOrInt`, `SslVerify`, `ListField`, `ChannelSetting`) and `ParseOptions` (`ssl_verify_fs_check: bool`, default `false`) in `crates/condarc/src/model.rs` (data-model.md §3, §6; depends on T009, same file)
- [ ] T013 Implement `ValidationReport::entries()`/`schema_version()`, `Display` (one line per entry), and `std::error::Error` for `ValidationReport` in `crates/condarc/src/error.rs` (FR-034/FR-037; depends on T010, same file)
- [ ] T014 Re-export the public API surface (`Config`, `ParseOptions`, `ValidationReport`, `ErrorEntry`, `ErrorKind`, `Location`, `PathSegment`, `InputRepr`, `ChannelPriority`, `PathConflict`, `SafetyChecks`, `SatSolver`, `ListField`, `BoolOrInt`, `SslVerify`, `ChannelSetting`) from `crates/condarc/src/lib.rs` per `contracts/public-api.md`'s type table (depends on T008, T009, T010, T012, T013)
- [ ] T015 [P] Unit test: `Config::default()` has every field `None`/empty (FR-038) in `crates/condarc/src/model.rs`
- [ ] T016 [P] Unit test: `lower()` correctly maps every `yaml_rust2::Yaml` scalar/collection kind (incl. an over-range integer numeral falling back to `Yaml::String`) in `crates/condarc/src/parse.rs`

**Checkpoint**: Foundation ready — `Config`, `CATALOG`, `ParseOptions`, and the error types compile and are unit-tested; no coercion/validation/error-accumulation behavior exists yet.

---

## Phase 3: User Story 1 - Parse a valid `.condarc` into typed settings (Priority: P1) 🎯 MVP (part 1 of 2)

**Goal**: Feed a valid `.condarc` YAML string to `parse()`/`parse_with_options()` and get back a `Config` whose fields are coerced exactly as conda would coerce them (booleans, numbers, enums, lists, maps, strings; aliases resolved to canonical names).

**Independent Test**: Exercise each coercion rule via unit tests, then hand-written documents covering spec.md's US1 acceptance scenarios 1–4 (alias spellings, boolish `"yes"`→`true`, typed `channels`/`channel_priority`/`always_yes` reads, empty/null root). Full byte-for-byte agreement with `conformance/condarc/expected/*.json` is proven once the adapter exists (US3); this phase proves the underlying coercion is correct at the unit level.

### Tests for User Story 1

- [ ] T017 [P] [US1] Unit tests for boolish accept-path coercion (`Bool`, `NullableBool` truth tables, whitespace-trimmed `BOOLISH_TRUE`/`BOOLISH_FALSE`) in `crates/condarc/src/coerce/boolish.rs`
- [ ] T018 [P] [US1] Unit tests for numeric accept-path coercion (`Int`/`Float`, PEP-515 single-underscore digit groups, leading zeros, float truncation toward zero) in `crates/condarc/src/coerce/numeric.rs`
- [ ] T019 [P] [US1] Unit tests for enum accept-path coercion (value-or-name lookup for all four enums + `channel_priority`'s bool/boolish shim) in `crates/condarc/src/coerce/enums.rs`
- [ ] T020 [P] [US1] Unit tests for string accept-path coercion (`PlainString` `str()` conversion, `NullableString` pass-through) in `crates/condarc/src/coerce/strings.rs`
- [ ] T021 [P] [US1] Unit tests for sequence/map accept-path coercion (all six `ValueKind` shapes: `StringSeq`, `ListFieldsSeq`, `StringMap`, `NullableStringMap`, `StringSeqMap`, `ChannelSettingsSeq`) in `crates/condarc/src/coerce/sequences.rs`

### Implementation for User Story 1

- [ ] T022 [P] [US1] Implement boolish accept-path coercion (`Bool`, `NullableBool`) in `crates/condarc/src/coerce/boolish.rs` (FR-012/FR-013)
- [ ] T023 [P] [US1] Implement numeric accept-path coercion (`Int`, `Float`) in `crates/condarc/src/coerce/numeric.rs` (FR-018/FR-019)
- [ ] T024 [US1] Implement `local_repodata_ttl`'s `BoolOrInt` narrow-vocabulary accept-path coercion in `crates/condarc/src/coerce/numeric.rs` (FR-020; depends on T023, same file)
- [ ] T025 [P] [US1] Implement enum accept-path coercion (`ChannelPriority`/`PathConflict`/`SafetyChecks`/`SatSolver` value-or-name lookup + `channel_priority` bool/boolish shim) in `crates/condarc/src/coerce/enums.rs` (FR-016/FR-017)
- [ ] T026 [P] [US1] Implement string accept-path coercion (`PlainString`, `NullableString`) in `crates/condarc/src/coerce/strings.rs` (FR-014/FR-015)
- [ ] T027 [P] [US1] Implement sequence/map accept-path coercion (raw-shape gate + element typify for all six shapes) in `crates/condarc/src/coerce/sequences.rs` (FR-021/FR-022/FR-023)
- [ ] T028 [US1] Implement `ssl_verify`'s default, side-effect-free accept-path coercion (bool/boolish/`truststore`/unverified path) in `crates/condarc/src/coerce/boolish.rs` (FR-024 default branch; depends on T022, same file)
- [ ] T029 [US1] Implement the per-key coercion dispatch loop: for a root `Map`, look up each key in `CATALOG` (canonical or alias), call the matching coercer by `ValueKind`, and set the corresponding `Config` field on success, in `crates/condarc/src/parse.rs` (FR-009/FR-010/FR-011; depends on T008, T022–T028)
- [ ] T030 [US1] Implement unknown top-level key retention: lower an unmatched key's `RawValue` to `serde_json::Value` and insert it into `Config::extra` in `crates/condarc/src/parse.rs` (FR-036; depends on T029)
- [ ] T031 [US1] Implement `Config::extra_as<T: DeserializeOwned>() -> Result<T, serde_json::Error>` in `crates/condarc/src/model.rs` (research R2; depends on T009)
- [ ] T032 [US1] Implement root-shape handling for the accepting cases (`Null` root → `Ok(Config::default())`; `Map` root → the full per-key loop) in `crates/condarc/src/parse.rs` (FR-005/FR-006; depends on T030)
- [ ] T033 [US1] Implement the public `parse(yaml: &str)` / `parse_with_options(yaml: &str, options: ParseOptions)` entry points wiring YAML load → `lower()` → root dispatch, returning `Ok(Config)` for every accepted document, in `crates/condarc/src/lib.rs` (FR-001/FR-002; depends on T032)
- [ ] T034 [US1] Integration test covering spec.md's US1 acceptance scenarios 1–4 (alias spellings, `"yes"`→`true`, typed `channels`/`channel_priority`/`always_yes` reads, empty/null root) in `crates/condarc/tests/parse_valid.rs`

**Checkpoint**: User Story 1 is functional — the crate accepts valid documents and produces correctly-typed `Config` values, independently testable via unit tests and `parse_valid.rs` (the full conformance-corpus proof against `expected/*.json` lands in US3).

---

## Phase 4: User Story 2 - Reject an invalid `.condarc` with a complete, structured error report (Priority: P1) 🎯 MVP (part 2 of 2)

**Goal**: Feed malformed/type-invalid `.condarc` text to the crate and get back a `ValidationReport` that accumulates every independent problem in one pass (never just the first), each entry carrying a typed location, kind, message, and offending input.

**Independent Test**: Unit tests per reject-path coercion rule and per semantic/cross-field validator, plus a hand-built multi-error document (bad `channel_alias` + out-of-range `remote_max_retries` + non-boolish `always_copy`) asserting exactly 3 accumulated entries (SC-005).

### Tests for User Story 2

- [ ] T035 [P] [US2] Unit tests for boolish reject-path coercion (non-boolish, non-numeric-parseable strings rejected) in `crates/condarc/src/coerce/boolish.rs`
- [ ] T036 [P] [US2] Unit tests for numeric reject-path coercion, incl. the A1 `i64`/`f64` range check and A4 ASCII-only-digit rule, in `crates/condarc/src/coerce/numeric.rs`
- [ ] T037 [P] [US2] Unit tests for enum reject-path coercion (wrong casing rejected) in `crates/condarc/src/coerce/enums.rs`
- [ ] T038 [P] [US2] Unit tests for sequence/map reject-path coercion (bare-scalar rejection, `list_fields` closed-vocabulary violations) in `crates/condarc/src/coerce/sequences.rs`
- [ ] T039 [P] [US2] Unit tests for `validate.rs`'s semantic validators (`channel_alias`, `default_python`, `ssl_verify` path existence), alias-collision detection, and the two cross-field rules in `crates/condarc/src/validate.rs`

### Implementation for User Story 2

- [ ] T040 [P] [US2] Implement boolish reject-path errors (`type_coercion` entries for non-boolish/unparseable strings) in `crates/condarc/src/coerce/boolish.rs` (FR-012)
- [ ] T041 [P] [US2] Implement numeric reject-path errors, incl. the A1 range check and A4 ASCII-only-digit enforcement, in `crates/condarc/src/coerce/numeric.rs` (FR-018/FR-019, A1, A4)
- [ ] T042 [P] [US2] Implement enum reject-path errors (bad casing rejected) in `crates/condarc/src/coerce/enums.rs` (FR-016)
- [ ] T043 [P] [US2] Implement sequence/map reject-path errors (bare-scalar rejection, `list_fields` vocabulary violation) in `crates/condarc/src/coerce/sequences.rs` (FR-021/FR-022/FR-023)
- [ ] T044 [US2] Implement the `channel_alias` scheme semantic validator (`^$|^[a-z][a-z0-9]{0,11}://`) in `crates/condarc/src/validate.rs` (FR-025)
- [ ] T045 [US2] Implement the `default_python` semantic validator (empty/null-or-falsy accepted; else `len>=3` ∧ `value[1]=='.'` ∧ whole string parses as an ASCII float in `[2.0, 4.0)`) in `crates/condarc/src/validate.rs` (FR-026, A4; depends on T044, same file)
- [ ] T046 [US2] Implement the opt-in `ssl_verify` filesystem-existence validator in `crates/condarc/src/validate.rs` (FR-024, A3, research R6; depends on T028, T045)
- [ ] T047 [US2] Implement alias-collision detection for all 20 documented alias pairs (`MultipleKeysError`) in `crates/condarc/src/validate.rs` (FR-029; depends on T046, same file)
- [ ] T048 [US2] Implement the two cross-field rules — `client_ssl_cert_key` requires `client_ssl_cert`; `always_copy`/`always_softlink` mutual exclusion — in `crates/condarc/src/validate.rs` (FR-027/FR-028; depends on T047, same file)
- [ ] T049 [US2] Implement YAML syntax error handling: a `yaml_rust2::ScanError` produces a single-entry `YamlSyntax` report with no per-field evaluation, in `crates/condarc/src/parse.rs` (FR-008, FR-032a; depends on T011)
- [ ] T050 [US2] Implement root-shape rejection: a `Seq`/scalar root, or a multi-document stream, produces a single-entry `RootShape` report, in `crates/condarc/src/parse.rs` (FR-007, FR-007a, FR-032b; depends on T049, same file)
- [ ] T051 [US2] Implement non-string mapping key handling: a per-key `type_coercion` entry naming the enclosing location, key dropped, evaluation continues, in `crates/condarc/src/parse.rs` (FR-007b; depends on T029)
- [ ] T052 [US2] Wire error accumulation: the per-key loop collects `Err` entries instead of short-circuiting, then runs `validate.rs`'s semantic/alias-collision/cross-field passes, returning `Err(ValidationReport)` (non-accumulable classes first, then catalog order, then alias-collision, then cross-field per research R7) iff any entries exist, in `crates/condarc/src/parse.rs` (FR-030/FR-031; depends on T040–T048, T050, T051)
- [ ] T053 [US2] Update `parse()`/`parse_with_options()` in `crates/condarc/src/lib.rs` to surface the accumulated `ValidationReport` as `Err(_)` (depends on T052, T033)
- [ ] T054 [US2] Integration test `crates/condarc/tests/multi_error_accumulation.rs`: a hand-built document with a bad `channel_alias`, an out-of-range `remote_max_retries`, and a non-boolish `always_copy` yields exactly 3 entries in `report.entries()` (SC-005)
- [ ] T055 [US2] Integration test `crates/condarc/tests/parse_invalid.rs`: single-entry `YamlSyntax`/`RootShape` reports, an alias-collision entry naming both keys, and cross-field entries (spec.md's US2 acceptance scenarios 1–5)

**Checkpoint**: User Stories 1 AND 2 both work independently — the crate accepts valid documents with correct values and rejects invalid documents with complete structured reports, all provable without the conformance harness.

---

## Phase 5: User Story 3 - Adapter to a portable representation for conformance (Priority: P2)

**Goal**: A maintainer runs the conformance suite; a test-only adapter renders `Config` into the same JSON shape as `conformance/condarc/expected/*.json`, and the harness's previously-always-skipped `Crate` checker now runs and passes for every fixture.

**Independent Test**: For every `valid/` fixture, parse it, run the adapter, and assert the adapted JSON equals the corresponding `expected/*.json` exactly; for every `invalid/` fixture (exploded per-key), assert rejection.

### Implementation for User Story 3

- [ ] T056 [US3] Implement `to_expected_json(&condarc::Config) -> serde_json::Value` — present-only, canonical loader names, FR-041 value encoding incl. non-finite float strings (`"Infinity"`/`"-Infinity"`/`"NaN"`) — in `tests/support/adapter.rs` (FR-040/FR-041, research R9)
- [ ] T057 [US3] Create `tests/support/mod.rs` with `pub mod adapter;`
- [ ] T058 [US3] Declare the `Crate`-checker A1 divergence list (the 4 bignum fixture names → expected crate-rejects verdict) alongside the adapter, in `tests/support/adapter.rs` (spec A1; depends on T056)
- [ ] T059 [US3] Wire `mod support;` into `tests/condarc_conformance.rs` and replace `check_crate`'s always-`Skipped` stub with a real call to `condarc::parse_with_options(&yaml, condarc::ParseOptions { ssl_verify_fs_check: true, ..Default::default() })`, mapping `Ok`/`Err` onto `CheckOutcome`, in `tests/condarc_conformance.rs` (depends on T057, T033, T053)
- [ ] T060 [US3] Add the adapter exact-comparison assertion for the `Crate` checker inside `valid_condarc_is_accepted` (parallel to the existing `assert_conda_expected_representation` conda check), applying the A1 divergence list, in `tests/condarc_conformance.rs` (depends on T056, T058, T059)
- [ ] T061 [US3] Run `make conformance-crate` against the full corpus and fix any remaining coercion/validation discrepancies until every `valid/`, `invalid/`, and `expected/` fixture passes (SC-001/SC-002/SC-003; depends on T060)
- [ ] T062 [US3] Integration test `crates/condarc/tests/public_api_usage.rs` exercising all 5 numbered usage patterns in `contracts/public-api.md`'s "End-to-end usage" section (file read + missing-file fallback, `ValidationReport::entries()` iteration/branching, typed `Config` field reads, `parse_with_options` with `ssl_verify_fs_check: true` against a real existing path, `Config::extra_as::<T>()` over conda-build's 4 out-of-scope keys)

**Checkpoint**: All three user stories are complete and independently verified; the `Crate` checker in the conformance harness passes for every fixture alongside the pre-existing `conda`/`openapi` checkers (SC-003).

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Final quality-gate pass required by the plan's Constitution Check before the feature is mergeable.

- [ ] T063 [P] Add `///` docs plus a runnable doc example on every public item (esp. `parse`/`parse_with_options`) across `crates/condarc/src/lib.rs`, `crates/condarc/src/model.rs`, `crates/condarc/src/error.rs` (Constitution VI)
- [ ] T064 [P] Run `cargo fmt --check` and fix formatting across `crates/condarc/` and `tests/`
- [ ] T065 [P] Run `cargo clippy --all-targets -- -D warnings` and fix all warnings
- [ ] T066 Run `cargo deny check` and `cargo audit` once more as a final post-implementation confirmation
- [ ] T067 [P] Run `cargo doc --workspace --no-deps` and fix any doc errors/broken intra-doc links
- [ ] T068 Execute every scenario in `quickstart.md` (Scenarios 1–4 plus the full quality gate) end-to-end and confirm SC-001 through SC-006 all hold

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately.
- **Foundational (Phase 2)**: Depends on Setup completion — BLOCKS all user stories (both US1 and US2 dispatch off `CATALOG`/`Config`/the error types defined here).
- **User Story 1 (Phase 3)**: Depends on Foundational completion. No dependency on US2 or US3.
- **User Story 2 (Phase 4)**: Depends on Foundational completion. Reuses (extends) the same coercion functions US1 implements the accept-path of (T022/T023/T025/T027/T028), so in practice proceeds after or alongside US1's implementation tasks on those specific files — but is scoped, tested, and checkpointed independently.
- **User Story 3 (Phase 5)**: Depends on US1's `parse`/`Config` (T033) and US2's error surfacing (T053) both existing, since the adapter and the `Crate` checker exercise the whole `parse_with_options` pipeline end to end.
- **Polish (Phase 6)**: Depends on all three user stories being complete.

### User Story Dependencies

- **User Story 1 (P1)**: Can start after Foundational. Independently testable via unit tests + `parse_valid.rs` without US2 or US3 existing.
- **User Story 2 (P1)**: Can start after Foundational. Shares files with US1 (each coercion module gets both an accept-path task and a reject-path task), but is independently testable via unit tests + `multi_error_accumulation.rs`/`parse_invalid.rs`.
- **User Story 3 (P2)**: Needs both US1 and US2 finished (it exercises full accept-and-reject behavior through the conformance harness); not independently implementable before them, but independently *testable* once implemented (`make conformance-crate` alone).

### Within Each User Story

- Tests are written before the corresponding implementation task (TDD, Constitution II) — e.g. T017 before T022, T035 before T040.
- Per-`ValueKind`-family coercers before the dispatch loop that calls them.
- The dispatch loop before unknown-key handling before root-shape wiring before the public `parse`/`parse_with_options` functions.
- Story complete (checkpoint) before moving to the next priority.

### Parallel Opportunities

- Setup: T002 (crate manifest) in parallel with the rest of T001 (root manifest) proceeding.
- Foundational: T007 (parse.rs), T008 (catalog.rs), T009 (model.rs), T010 (error.rs) are four different files with no cross-dependency — run in parallel.
- US1 tests T017–T021 (five different `coerce/*.rs` files) — run in parallel.
- US1 implementation T022/T023/T025/T026/T027 (boolish/numeric/enums/strings/sequences — different files) — run in parallel.
- US2 tests T035–T039 (five different files) — run in parallel.
- US2 implementation T040/T041/T042/T043 (different `coerce/*.rs` files) — run in parallel.
- Polish: T063–T065, T067 — run in parallel (different concerns, though `cargo fmt`/`clippy`/`doc` touch overlapping files, so treat file-level conflicts as sequential if run by literal parallel processes rather than parallel review).

---

## Parallel Example: User Story 1

```bash
# Launch all five coercion-module test tasks for US1 together:
Task: "Unit tests for boolish accept-path coercion in crates/condarc/src/coerce/boolish.rs"
Task: "Unit tests for numeric accept-path coercion in crates/condarc/src/coerce/numeric.rs"
Task: "Unit tests for enum accept-path coercion in crates/condarc/src/coerce/enums.rs"
Task: "Unit tests for string accept-path coercion in crates/condarc/src/coerce/strings.rs"
Task: "Unit tests for sequence/map accept-path coercion in crates/condarc/src/coerce/sequences.rs"

# Then launch the five matching implementation tasks together:
Task: "Implement boolish accept-path coercion in crates/condarc/src/coerce/boolish.rs"
Task: "Implement numeric accept-path coercion in crates/condarc/src/coerce/numeric.rs"
Task: "Implement enum accept-path coercion in crates/condarc/src/coerce/enums.rs"
Task: "Implement string accept-path coercion in crates/condarc/src/coerce/strings.rs"
Task: "Implement sequence/map accept-path coercion in crates/condarc/src/coerce/sequences.rs"
```

---

## Implementation Strategy

### MVP First (User Stories 1 + 2 — both P1)

spec.md assigns **P1 to both** US1 (accept valid) and US2 (reject invalid with a full report): GEN-23's
parent acceptance criteria need both "extract settings from a populated file" and "return an error
[with appropriate messages] if the `.condarc` is not valid", so neither alone is a usable MVP.

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL — blocks everything)
3. Complete Phase 3: User Story 1
4. Complete Phase 4: User Story 2
5. **STOP and VALIDATE**: run all unit tests + `parse_valid.rs` + `multi_error_accumulation.rs` + `parse_invalid.rs` — the crate is now usable by GEN-23 without the conformance harness
6. Deploy/demo if ready

### Incremental Delivery

1. Setup + Foundational → foundation ready
2. Add US1 → unit-test independently → the crate can parse a valid document
3. Add US2 → unit-test independently → the crate can also reject an invalid one with a full report (MVP complete)
4. Add US3 → run the full conformance suite (`make conformance-crate`) → SC-001/SC-002/SC-003 proven
5. Polish → quality gates (`fmt`/`clippy`/`deny`/`audit`/`doc`) + `quickstart.md` end-to-end

### Parallel Team Strategy

With multiple developers, once Foundational is done:

- Developer A: User Story 1's accept-path coercion + dispatch loop + `parse_valid.rs`
- Developer B: User Story 2's reject-path coercion + `validate.rs` + error accumulation + `parse_invalid.rs`/`multi_error_accumulation.rs`

Because US1 and US2 tasks land in the *same* `coerce/*.rs` files (accept-path vs. reject-path
branches of the same functions), coordinate at the file level even though the stories are tracked
independently — the two developers should pair or serialize on each shared coercion module rather
than editing it simultaneously. User Story 3 (the adapter + harness wiring) is a natural third
workstream but can only start once both US1 and US2 land.

---

## Notes

- [P] tasks touch different files with no unfinished dependency between them.
- [Story] labels (US1/US2/US3) map every user-story-phase task back to spec.md's prioritized stories.
- Coercion modules (`coerce/boolish.rs`, `coerce/numeric.rs`, `coerce/enums.rs`, `coerce/sequences.rs`) each receive one task from US1 (accept-path) and one from US2 (reject-path) — expect them to be the same functions gaining their `Err` arm, not two separate implementations.
- Every setting's canonical name/alias/`ValueKind`/validator is declared exactly once, in `CATALOG` (T008) — no per-key duplication anywhere else (Constitution IV).
- The conformance corpus (`conformance/condarc/{valid,invalid,expected}/*.json`) is the acceptance oracle; do not edit it as part of implementing this feature (Assumptions A1's divergence-list mechanism is the only sanctioned way a crate verdict may diverge from a `valid/` fixture).
- Commit after each task or logical group; stop at any checkpoint to validate a story independently.
