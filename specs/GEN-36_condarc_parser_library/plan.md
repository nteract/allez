# Implementation Plan: `.condarc` Parser Library

**Branch**: `GEN-36_condarc_parser_library` | **Date**: 2026-07-24 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/GEN-36_condarc_parser_library/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command; its definition describes the execution workflow.

## Summary

Build a standalone, publishable Rust library crate (`condarc`) that turns the text of a
`.condarc` YAML document into a validated, strongly-typed `Config`, or into a `ValidationReport`
that accumulates *every* independent problem in the document (Pydantic-style), never panicking.
The authoritative oracle is the committed conformance corpus
(`conformance/condarc/{valid,invalid,expected}/*.json`) plus `docs/condarc_openapi.json`, not
conda's source. The crate is wired in as the `Crate` checker in `tests/condarc_conformance.rs`
and MUST accept every `valid/`, reject every exploded `invalid/`, and match every `expected/`
fixture under subset comparison via a documented, test-only adapter. Technical approach: a small
pipeline — `yaml-rust2` parse → lower to a private `RawValue` tree → root-shape check → per-key
alias resolution + typed coercion (mirroring conda's `typify`/`boolify`/`numberify` semantics,
driven by a single declarative catalog) → per-field semantic validation → cross-field rules →
error accumulation. Fixed-width numeric types (`i64`/`f64`) replace Python's arbitrary precision
(documented simplification A1), moving the enumerated bignum fixtures from `valid/` to `invalid/`.
Unknown top-level keys are retained in `Config::extra: HashMap<String, serde_json::Value>` with a
caller-facing `extra_as::<T>()` convenience method (research R2); the `ssl_verify` filesystem
check is an opt-in runtime `ParseOptions` flag, not a Cargo feature (research R6).

## Technical Context

**Language/Version**: Rust 1.96 (workspace edition 2024)

**Primary Dependencies**:
- YAML parser: **`yaml-rust2`** (research R1) — stable API, no `unsafe`, MSRV 1.65, same
  precedent as `config-rs`. `yaml_rust2::Yaml` resolves scalars into `Boolean`/`Integer`/`Real`/
  `String`/`Null`, which is sufficient to reproduce conda's type-vs-string coercion distinction
  (bare `true`/`7` vs. quoted `"true"`/`"7"`). `saphyr` and the `serde_yaml` family were evaluated
  and rejected (unstable API / archived / no converged serde-native successor — R1).
- `serde` + `serde_json` (already in the workspace) — used **only** for output serialization: the
  `ValidationReport` JSON contract (`Serialize`), the `Config::extra` escape-hatch value type and
  its `extra_as::<T: DeserializeOwned>()` convenience method (research R2), and test-fixture
  reading. **Not** used to parse YAML into `Config`: `yaml-rust2` is not a serde data format and
  `Config` has no `Deserialize` impl — the YAML→`Config` path is a hand-written pipeline (research
  R1 "Does yaml-rust2 work with serde?").
- `thiserror` for the internal error/kind enums, contingent on clearing `cargo deny`; otherwise
  hand-rolled `std::error::Error` (research R5).

**Storage**: N/A (pure in-memory parsing library; no filesystem/network access except the single
documented, opt-in `ssl_verify` path-existence branch, FR-024, gated behind a runtime
`ParseOptions.ssl_verify_fs_check` flag that defaults to `false` — research R6).

**Testing**: `cargo test` — unit tests in `#[cfg(test)]` modules alongside code per module
(Constitution II TDD: write the failing test first against the already-committed conformance
corpus); the crate's public API exercised via the repo-level conformance harness
(`tests/condarc_conformance.rs`, `Crate` checker) and crate-level integration tests under
`crates/condarc/tests/`.

**Target Platform**: Linux, macOS, Windows (library is platform-agnostic; only the opt-in
`ssl_verify` FS check, off by default, is platform-sensitive).

**Project Type**: Rust library crate inside a new Cargo workspace (the repo currently has a
single-binary `allez` crate at root; this plan converts it to a workspace with `crates/allez` +
`crates/condarc` — research R10).

**Performance Goals**: Parsing a single `.condarc` (typically < 100 keys) is not on a hot path;
target sub-millisecond parse for realistic files. No specific throughput SLO. Determinism
(Constitution IX) is the hard requirement: identical input → identical `Config` and identical
error-report ordering (research R7).

**Constraints**:
- No panics on any input (FR-004, FR-035); no `.unwrap()`/`.expect()` in library code
  (Constitution V) — enforced via `#![cfg_attr(not(test), deny(clippy::unwrap_used,
  clippy::expect_used))]` as in `src/main.rs`.
- No filesystem/network/env access during parse except the opt-in FR-024 `ssl_verify` branch
  (research R6), which defaults off.
- Must accumulate all independent errors, not stop at the first (FR-030/FR-031), except the two
  non-accumulable classes (FR-032: YAML syntax, wrong root shape).
- Fixed-width numerics `i64`/`f64`; out-of-range → typed error, never clamp/wrap (A1).
- Public API stays fully monomorphic — no generic type parameter on `Config`/`parse` for the
  unknown-key tail (research R2's rejected alternative).

**Scale/Scope**: 99 recognized settings across 9 documented groups (`docs/condarc_research.md`
§4.1–4.11), 20 documented alias pairs, 4 enum types (`ChannelPriority`, `PathConflict`,
`SafetyChecks`, `SatSolver`), 1 closed-vocabulary sequence type (`ListField`, 25 members), 2
cross-field rules, ~648 conformance fixtures (376 valid − 4 bignum-moved + 273 invalid + 4
bignum-moved). Single-document parse only (no multi-source search-path merge).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Assessment | Gate |
|-----------|-----------|------|
| I. Code Quality | Crate is small, single-responsibility modules (`parse`, `catalog`, `coerce/*`, `validate`, `model`, `error`). `cargo fmt`/`clippy -D warnings` enforced. No `unsafe` — `yaml-rust2` is pure safe Rust (R1). | PASS |
| II. Testing Standards | TDD: the conformance corpus already exists and fails (crate unimplemented). Per-module unit tests written before implementation; public API exercised via `tests/`. Red→Green→Refactor against the fixtures. | PASS |
| III. Dual-Primary Interface | `ValidationReport` is renderable both human-readable (`Display`, one entry per problem) and machine-readable (`Serialize` → JSON list of entries, FR-034); the JSON entry shape is a versioned, documented contract (`contracts/error-report.schema.json`). The crate is a library so it has no CLI, but its error/output types carry stable machine codes (FR-033/`ErrorKind`). | PASS |
| IV. DRY | Single coercion engine (`coerce/`) reused across all keys of the same shape; a single `catalog.rs` table defines each setting's name/aliases/type/validator once (no per-key duplication). | PASS |
| V. Explicit Over Implicit | Typed `Result`/error, no panics, no `.unwrap()`. Out-of-range numbers error rather than silently wrap (A1). No surprising blanket `From`. The `ssl_verify` FS check is an explicit, opt-in `ParseOptions` argument, not implicit/default behavior (R6). | PASS |
| VI. Documentation & Type Safety | Every public item gets `///` docs; invalid states made unrepresentable via enums (`ChannelPriority`, `PathConflict`, `SafetyChecks`, `SatSolver`, `ListField`) and typed value variants (`BoolOrInt`, `SslVerify`) (FR-010). `cargo doc` clean. Runnable doc example on `parse()`. | PASS |
| VII. No Hardcoded Values | Named constants for closed vocabularies (`CONDA_LIST_FIELDS`, boolish tokens, enum members, scheme-regex bound 11, `i64`/`f64` numeric range). No absolute paths; the crate reads no paths itself (the opt-in `ssl_verify` check receives the path value from the parsed document, not a hardcoded location). | PASS |
| VIII. 100% Spec Test Coverage | Every acceptance scenario + FR maps to a test: the conformance corpus covers coercion/accept/reject/adapter; hand-built multi-error tests cover FR-031/SC-005; unit tests cover each coercion rule. Traceability table lives in `quickstart.md`. | PASS |
| IX. Determinism & Idempotency | Pure function of input; error-entry ordering is deterministic (fixed catalog declaration order — research R7). `Cargo.lock` committed. | PASS |
| X. Security & Supply-Chain | New dep (`yaml-rust2`) must pass `cargo deny`/`cargo audit` (MIT/Apache-2.0, already on the allow list per R1); this is a checklist item for implementation. Crate itself downloads/executes nothing. | PASS (conditional on confirming `cargo deny`/`audit` clean during implementation) |
| XI. Structured Observability | Library-level: errors carry a stable machine code/category (`ErrorKind`, FR-033). No ad-hoc `println!`. No I/O to observe in the default (hermetic) path; the opt-in `ssl_verify_fs_check` touch is a single, documented filesystem read, not logged (pure library, no `tracing` dependency needed). | PASS |

**Initial gate: PASS.** The only open items were dependency choice (R1), the error-crate approach
(R5), and the FR-024 gating mechanism (R6) — all resolved in Phase 0 `research.md`. No principle
violations requiring Complexity Tracking.

**Post-Design re-check (after Phase 1): PASS.** The design artifacts introduce no new violations:
- The public API (`contracts/public-api.md`) is total/panic-free, typed, `#[non_exhaustive]`, and
  fully documented → Principles IV/V/VI intact.
- The error report has both `Display` (human) and `Serialize` (machine, versioned schema in
  `contracts/error-report.schema.json`) → Principle III intact.
- Deterministic entry ordering (research R7) → Principle IX intact.
- The `Config::extra`/`extra_as` design (research R2) keeps the public API fully monomorphic — no
  generic parameter tax on `Config`/`parse` → Principle VI (type safety without over-genericizing)
  and Principle IV (single, reusable escape hatch instead of N per-key mechanisms) intact.
- Only remaining supply-chain action is confirming `yaml-rust2` clears `cargo deny`/`cargo audit`
  during implementation (research R1) → Principle X gated but not violated.

## Project Structure

### Documentation (this feature)

```text
specs/GEN-36_condarc_parser_library/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output — complete, 10 resolved decisions (R1-R10)
├── data-model.md        # Phase 1 output — full Config struct + supporting types
├── quickstart.md        # Phase 1 output — validation guide
├── contracts/           # Phase 1 output
│   ├── public-api.md            # Public Rust API contract (parse/parse_with_options/Config/errors)
│   ├── error-report.schema.json # Versioned JSON schema for ValidationReport
│   └── adapter-output.md        # Test-only Config → expected/*.json adapter contract
├── checklists/
│   └── requirements.md  # Spec-quality checklist (complete)
└── tasks.md              # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
# Cargo workspace root (Cargo.toml gains a [workspace] table — research R10)
Cargo.toml                       # [workspace] members = ["crates/allez", "crates/condarc"]
Cargo.lock                       # committed (Constitution IX)

crates/
├── allez/                       # existing binary, moved from src/ (unchanged behavior)
│   ├── Cargo.toml
│   ├── src/                     # (current src/ contents relocated here verbatim)
│   └── tests/
└── condarc/                     # NEW — this feature's library crate
    ├── Cargo.toml                # name = "condarc"; deps: yaml-rust2, serde, serde_json,
    │                             # thiserror (or hand-rolled); dev-deps: workspace test helpers
    ├── src/
    │   ├── lib.rs                # crate root: re-exports Config, ValidationReport, ErrorEntry,
    │   │                         # ErrorKind, ParseOptions, parse(), parse_with_options()
    │   ├── parse.rs               # yaml-rust2 entry, Yaml -> RawValue lowering, root-shape gate,
    │   │                         # known/unknown key split, RawValue -> serde_json::Value (extra)
    │   ├── catalog.rs             # static CATALOG: &[Setting] (99 entries) + ValueKind + alias table
    │   ├── model.rs               # Config struct (full field list), ChannelPriority, PathConflict,
    │   │                         # SafetyChecks, SatSolver, ListField, BoolOrInt, SslVerify,
    │   │                         # ChannelSetting, ParseOptions
    │   ├── coerce/
    │   │   ├── mod.rs
    │   │   ├── boolish.rs         # boolify variants (Bool/NullableBool/SslVerifyKind)
    │   │   ├── numeric.rs         # Int/Float + A1 range check + BoolOrIntKind narrow vocab
    │   │   ├── enums.rs           # value-or-name enum lookup + channel_priority bool shim
    │   │   ├── strings.rs         # PlainString/NullableString str() coercion
    │   │   └── sequences.rs       # StringSeq/ListFieldsSeq/StringMap/NullableStringMap/
    │   │                         # StringSeqMap/ChannelSettingsSeq raw-shape + element typify
    │   ├── validate.rs            # channel_alias scheme, default_python range, alias collisions
    │   │                         # (20 pairs), the 2 cross-field rules
    │   └── error.rs               # ValidationReport, ErrorEntry, ErrorKind, Location, InputRepr
    └── tests/
        └── conformance_support.rs # to_expected_json() adapter (test-only — research R9)

tests/
└── condarc_conformance.rs        # existing repo-level harness; wires `condarc::parse` as the
                                   # `Crate` checker; unaffected by the workspace move (relative
                                   # `conformance/**` globs still resolve from the workspace root)

conformance/condarc/{valid,invalid,expected}/*.json   # unchanged; 4 bignum fixtures move
                                                        # valid/ -> invalid/ per A1 (SC-006)
docs/condarc_openapi.json          # additionalProperties relaxed true (already correct per
                                    # docs/condarc_research.md §1.2 / spec Assumptions)
docs/condarc_research.md           # unchanged; §4 catalog is catalog.rs's source of truth
```

**Structure Decision**: Single-project Rust workspace (Option 1 shape, adapted). The existing
`allez` binary moves into `crates/allez/` unchanged in behavior; a new library crate
`crates/condarc/` is added as an independent, publishable workspace member with no dependency on
`allez` (FR-003). Root-level `tests/`, `conformance/`, `docs/`, `Makefile`, and `deny.toml` stay in
place so the existing conformance harness and CI tooling require no path rewrites beyond the
workspace `Cargo.toml` change (research R10).

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

No violations. Table intentionally omitted.
