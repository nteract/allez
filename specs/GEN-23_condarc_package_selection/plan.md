# Implementation Plan: Resolve `.condarc` Channel Preferences for Package Selection

**Branch**: `GEN-23_condarc_package_selection` | **Date**: 2026-07-30 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/GEN-23_condarc_package_selection/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command; its definition describes the execution workflow.

## Summary

This ticket spans two codebase locations, both planned, implemented, and
tested under this one ticket (spec.md Operating Context):

1. A new, additive `expand_channels` capability inside the `condarc`
   crate (`crates/condarc/src/expand_channels.rs`) that expands a parsed
   `Config`'s channel-related settings — bare names,
   `custom_channels`/`custom_multichannels`, the `defaults` placeholder,
   `channel_priority`, `allowlist_channels`/`denylist_channels` — into a
   single, ordered, already deny-then-allow filtered `ResolvedChannels`
   value (or a typed `ExpandChannelsError` for the one documented
   failure case, FR-018), layered on top of and never altering GEN-36's
   existing `parse()`/`Config`.
2. A new `allez`-internal module (`src/channel_config/`) that locates
   and reads `~/.condarc`, hands it to `parse()` then `expand_channels()`,
   falls back to conda's own documented defaults whenever the file is
   absent, rejected, unreadable, or unexpandable (recording the
   rejected/unreadable/unexpandable cases via structured observability,
   never the silent absent case), and distinguishes a resolution whose
   filtering legitimately left zero usable channels (FR-020) from a
   fully-populated one — plus retiring `src/ephemeral/`'s now-duplicate
   `ChannelConfig`/`ChannelSpec`/`ChannelPriorityMode` types and their
   compensating behaviors (FR-016), since GEN-24's environment-creation
   capability now consumes this ticket's `ResolvedChannels` output
   directly, with no adaptation step — so GEN-25's `allez oneshot` can
   wire the crate and GEN-24 together directly, deciding for itself
   whether to warn or proceed on either signal.

Full design rationale for both parts lives in `research.md` (R1–R15);
this plan covers scope, structure, and the constitution gate only.

**Cleanup boundary (reversed, per spec.md Assumptions and PR review)**:
this plan changes `src/ephemeral/channels.rs` directly (spec.md's
"Cleanup boundary" decision and FR-016): `ChannelConfig`, `ChannelSpec`,
`ChannelPriorityMode`, `filter_channels`, `channels_with_fallback`, and
`resolve_channel_source`'s literal-`"defaults"`-to-URL special case are
removed; `redact_channel_url` survives as a plain function, applied
explicitly at each channel-identifier format site instead of through
the retired `ChannelSpec`/`ChannelConfig` `Debug` impls. `mod.rs`'s
`create_ephemeral_environment`, `solve.rs`'s `solve_packages`, and
`install.rs`'s/`examples/ephemeral_smoke.rs`'s test/example fixtures are
updated to the new `condarc::ResolvedChannels`-shaped input; every
existing GEN-24 test exercising a retired item is rewritten, not merely
kept passing.

## Technical Context

**Language/Version**: Rust, `edition = "2024"` (matches the existing
workspace `Cargo.toml`; no change).

**Primary Dependencies**: No new dependency in either crate.
`expand_channels()` uses only `std` (a `HashMap` for the effective
`custom_channels` lookup — key lookups only, no ordering requirement,
research.md R6). `dirs` (already present transitively) is promoted to a
direct `allez` dependency for cross-platform home-directory resolution
(research.md R7); `condarc` is promoted from `[dev-dependencies]`-only
to `[dependencies]`, since `allez`'s new `src/channel_config/` module
depends on it at runtime. Reuses the existing `tracing` pipeline for the
one new observability event (research.md R9). No new dev-dependency
and no manifest-declared `[[bin]]` target — the manual, optional smoke
example calls `resolve_channel_config()` directly (research.md R8).

**Storage**: N/A. The one file this feature touches is `~/.condarc`
itself, read-only (FR-013), never cached across invocations (FR-015).

**Testing**: `cargo test --all`, no new opt-in feature flag needed —
every automated test this ticket adds is network-free and hermetic
against a controlled fixture; the one exception is the manual, optional
`examples/channel_config_smoke.rs`, which deliberately reads whichever
real `.condarc` exists on the machine running it (research.md R8). See
`research.md` § Test strategy and `quickstart.md` for the exact
SC-00n/FR-0nn → test-file mapping, summarized in Project Structure
below.

**Target Platform**: The same four targets GEN-24 already established
(Windows amd64, macOS aarch64, Linux aarch64/amd64). One `cfg`-gated
production line (`DEFAULT_CHANNELS`'s Windows variant, research.md R4);
every test this ticket adds runs on all four targets unconditionally.

**Project Type**: Library additions to two existing locations — a new
private module inside the already-published `crates/condarc` library,
and a new internal module tree (`src/channel_config/`) inside `allez`'s
existing library target. No new CLI surface (GEN-25's job).

**Performance Goals**: Not defined by this ticket. `expand_channels()`
operates on a single, already-parsed, in-memory `Config`, not a hot
path. The one `~/.condarc` read `allez`'s half performs is a single,
small, local-filesystem read per invocation.

**Constraints**: `expand_channels()` performs no I/O and never panics
(FR-001–FR-004, FR-005–FR-008, FR-019); its only failure case is
`ExpandChannelsError::EmptyChannelAlias` (FR-018) — every other input,
including one whose filtering empties `channels`, resolves
successfully. `allez`'s file-handling layer never blocks on a
missing/malformed/unreadable/unexpandable `~/.condarc` (FR-009/FR-011),
and never hands GEN-24 an intentionally-empty channel configuration
(FR-020). Embedded credential material in the resolved channel
identifiers themselves passes through unchanged — out of this ticket's
scope, deferred to GEN-29's own approach. `~/.condarc` is never written
to (FR-013); resolution is always fresh (FR-015); neither half alters
`parse()`'s existing behavior (Operating Context #1). FR-016 is the one
deliberate exception: it retires GEN-24's now-duplicate
channel-configuration type and its compensating behaviors.

**Scale/Scope**: Two small, single-responsibility additions plus one
targeted retirement — see Project Structure below for the exact file
list. No schema/migration/multi-service scope; GEN-36's `Config`/
`ChannelPriority` are consumed as-is, unchanged. GEN-24's `ChannelConfig`/
`ChannelSpec`/`ChannelPriorityMode` are the one exception (FR-016):
removed outright, along with `filter_channels`/`channels_with_fallback`/
`resolve_channel_source`'s defaults special case, once GEN-24's
environment-creation capability consumes `condarc::ResolvedChannels`/
`ChannelPriority` directly.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|---|---|---|
| I. Code Quality | PASS | Two new, single-responsibility module additions, each file mapping to one of `research.md`'s decisions. No `unsafe` code anywhere in this ticket's scope. |
| II. Testing Standards | PASS (one documented exception — see Complexity Tracking) | TDD throughout; see `research.md` § Test strategy for the full test-file breakdown. The one exception is the public, zero-argument `resolve_channel_config()` wrapper itself, which has no automated integration test (research.md R8). |
| III. Dual-Primary Interface | N/A (justified) | Neither half ships a CLI subcommand (GEN-25's job). Both public functions return fully-typed values for GEN-25 to consume later. |
| IV. DRY | PASS | Reuses `Config`'s already-coerced `channel_priority` (research.md R2) rather than re-implementing GEN-36's coercion. `expand_channels()`'s deny-then-allow filtering (FR-019) is now the sole implementation of that policy: GEN-24's own copy (`filter_channels()`) is retired (FR-016) rather than kept as a redundant second implementation, closing the DRY gap a PR reviewer flagged (research.md R12/R15, spec.md Assumptions). No undocumented exception remains. |
| V. Explicit Over Implicit | PASS | `resolve_channel_config`/`_from` are total for every caller-reachable `~/.condarc` state, one defended internal invariant excepted (research.md R14); `ChannelConfigResolution`'s `NoChannels` variant makes "legitimately zero channels" a distinct, matchable case rather than an ambiguous empty channel configuration (research.md R13). `expand_channels()` itself is fallible (research.md R11) with a named `Err` variant, not a sentinel. Fallback paths use explicit `ReadOutcome`/`FallbackReason` enums, never `io::Error::kind()` inspection at the call site. Default constants are named (research.md R4). |
| VI. Documentation and Type Safety | PASS | Every new public item gets a doc comment per `data-model.md`/`contracts/*.md`; `ReadOutcome` is a closed enum, `FallbackReason`/`ExpandChannelsError`/`ChannelConfigResolution` are all `#[non_exhaustive]`. `cargo doc --no-deps` must warn zero (implementation-time verification). |
| VII. No Hardcoded Values | PASS | `~/.condarc`'s location resolves via `dirs::home_dir()` (research.md R7), never a hand-rolled env-var lookup. Default constants are named, not inline literals. |
| VIII. Mandatory 100% Spec Test Coverage | PASS (planned) | Covers all 3 user stories and SC-001 through SC-006, and every FR. |
| IX. Determinism & Idempotency | PASS (one documented exception — see Complexity Tracking) | `expand_channels()` is pure. `resolve_channel_config` intentionally is not (fresh disk read every call, FR-015). |
| X. Security & Supply-Chain Integrity | PASS | `dirs` already present transitively, no new supply-chain surface; promoting it to a direct dependency still requires the standard `cargo audit`/`cargo deny check` gates like any other manifest change. Embedded credential material in the resolved channel identifiers themselves passes through unchanged — out of this ticket's scope, deferred to GEN-29's own approach. |
| XI. Structured Observability | PASS | One new `tracing`-emitted event shape (`ChannelConfigFallbackEvent`, research.md R9), through the existing subscriber, no second logging pipeline. |

No constitution violations requiring justification beyond the two,
explicitly justified exceptions (rows II and IX), recorded as this
ticket's two Complexity Tracking entries below.

## Project Structure

### Documentation (this feature)

```text
specs/GEN-23_condarc_package_selection/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md         # Phase 1 output (/speckit.plan command)
├── quickstart.md         # Phase 1 output (/speckit.plan command)
├── contracts/             # Phase 1 output (/speckit.plan command)
│   ├── condarc_resolve_api.md
│   └── allez_channel_config_api.md
├── checklists/            # existing — untouched by this command
├── context-files/         # existing — untouched by this command
├── context-summary.md    # existing — untouched by this command
├── spec.md                # existing — this command's input, untouched
└── tasks.md               # exists (later /speckit.tasks output)
```

### Source Code (repository root)

Extends the existing Cargo workspace (`members = [".", "crates/condarc"]`,
already established by GEN-36) — this ticket adds no new workspace
member. Within `crates/condarc`, this ticket adds one new module; within
the `allez` package, it adds one new module tree, sibling to the
existing `src/ephemeral/`.

```text
crates/condarc/
├── Cargo.toml            # unchanged — no new dependency
└── src/
    ├── lib.rs             # updated — `mod expand_channels;` + `pub use expand_channels::{expand_channels, ResolvedChannels, ExpandChannelsError};`
    ├── catalog.rs         # existing — untouched
    ├── coerce/            # existing — untouched
    ├── error.rs            # existing — untouched
    ├── model.rs            # existing — untouched (Config/ChannelPriority consumed as-is)
    ├── parse.rs            # existing — untouched
    ├── validate.rs         # existing — untouched
    └── expand_channels.rs  # NEW — expand_channels() -> Result<ResolvedChannels, ExpandChannelsError>,
                             #       ResolvedChannels, ExpandChannelsError, and the private
                             #       resolve_entry/resolve_member/match_custom_channel/
                             #       apply_allow_deny/ResolveContext helpers
                             #       (research.md R1/R5/R6/R11/R12; data-model.md)

crates/condarc/tests/
├── public_api_usage.rs    # existing — untouched
├── parse_invalid.rs       # existing — untouched
├── parse_valid.rs         # existing — untouched
├── multi_error_accumulation.rs  # existing — untouched
└── expand_channels_scenarios.rs   # NEW — SC-003's 22 named scenarios,
                             #       one #[test] per scenario, plus SC-005's
                             #       EmptyChannelAlias-`Err` scenario, the
                             #       `Config::default()` case, and the
                             #       US1/FR-level regression scenarios
                             #       (quickstart.md)

Cargo.toml (workspace root)
└── [dependencies]          # gains `dirs = "6"` and promotes `condarc` from
                             #       [dev-dependencies]; no other manifest change,
                             #       no manifest-declared [[bin]] target (research.md R8)

src/
├── lib.rs                 # updated — `pub mod channel_config;` alongside the
                             #       existing `pub mod cli; pub mod error; pub mod ephemeral;
                             #       pub mod observability; pub mod output;`
├── ephemeral/              # MODIFIED (FR-016): channels.rs loses
                             #       ChannelConfig/ChannelSpec/ChannelPriorityMode/
                             #       filter_channels/channels_with_fallback;
                             #       resolve_channel_source's defaults-name
                             #       special case removed; redact_channel_url
                             #       stays (already used directly by
                             #       defaults.rs/lifecycle.rs/install.rs/
                             #       error.rs/events.rs for package-spec
                             #       redaction, unaffected), now also
                             #       re-exported from mod.rs so external
                             #       callers can apply it explicitly.
                             #       mod.rs's create_ephemeral_environment
                             #       and solve.rs's solve_packages take
                             #       condarc::ResolvedChannels directly; every
                             #       existing test exercising a retired item
                             #       is rewritten (research.md R15)
├── cli/                    # existing — untouched (GEN-25's job to wire this
                             #       ticket's output into `oneshot`)
├── error.rs                 # existing — untouched
├── observability.rs         # existing — reused, not duplicated
├── output.rs                 # existing — untouched
├── main.rs                   # existing — untouched
└── channel_config/           # NEW — this ticket's entire allez-side scope
     ├── mod.rs                 # public API: resolve_channel_config() -> ChannelConfigResolution
                                  #       (FR-009–FR-011, FR-012–FR-020), pub(crate)
                                  #       resolve_channel_config_from(path) (research.md R8);
                                  #       default_condarc_path(); pub use events::FallbackReason.
                                  #       Ready.config is condarc::ResolvedChannels directly —
                                  #       no adaptation function (FR-012/FR-016, research.md R15).
                                  #       Also holds the co-located #[cfg(test)] module driving
                                  #       resolve_channel_config_from: SC-002's 5-file-state matrix
                                  #       plus the argument-level None case (6 tests total,
                                  #       one of which also covers SC-005's EmptyChannelAlias
                                  #       fallback), SC-004's observability-capture test group,
                                  #       SC-006's NoChannels test, the non-filtering-caused
                                  #       NoChannels case, FR-013/FR-014/FR-015's
                                  #       never-mutates/unknown-key/no-caching tests, and
                                  #       SC-001's pass-through assertion (Ready.config equals
                                  #       condarc::expand_channels(&config)'s own output
                                  #       byte-for-byte, for 5 distinct samples — no mapping
                                  #       logic left to test, research.md R15)
                                  #       — unit tests, not integration tests, since that
                                  #       function is pub(crate) (research.md R8)
     ├── locate.rs               # read_condarc()/ReadOutcome — distinguishes
                                  #       FR-009's silent "missing" from FR-011's
                                  #       recorded "unreadable" (data-model.md)
     └── events.rs                 # ChannelConfigFallbackEvent shape and tracing
                                  #       emission (FR-011). Also holds the co-located
                                  #       #[cfg(test)] module driving emit_fallback()
                                  #       directly: its own unit test asserting the
                                  #       emitted event's three fields exactly
                                  #       (research.md Test strategy)

tests/
├── cli_scaffold.rs          # existing — untouched
├── condarc_conformance.rs   # existing — untouched
├── ephemeral_env.rs          # existing — untouched (its own fixtures live in support/, below)
├── fixtures/                 # existing — untouched
└── support/                  # MODIFIED (FR-016, research.md R15): every fixture
                                     #       constructing/matching `ChannelConfig`/`ChannelSpec`/
                                     #       `ChannelPriorityMode` (`ephemeral.rs`,
                                     #       `user_story_1_creation.rs`, `user_story_1_failures.rs`,
                                     #       `user_story_2_reap.rs`, `user_story_3_defaults.rs`)
                                     #       migrated to `condarc::ResolvedChannels`/`ChannelPriority`
                                     #       — no new file here — SC-001, SC-002, SC-004,
                                     #       SC-005, and SC-006 are all co-located
                                     #       unit tests (research.md R8); there is no
                                     #       automated-test need this ticket adds that a
                                     #       `tests/` integration test could serve

examples/
├── ephemeral_smoke.rs        # MODIFIED (FR-016, research.md R15): `ChannelConfig::from_urls`
                                     #       call site replaced with `condarc::ResolvedChannels::from_channels`
└── channel_config_smoke.rs   # NEW — manual smoke test only (quickstart.md), a plain
                                     #       Cargo-auto-discovered example, the only
                                     #       coverage of the public, zero-argument
                                     #       `resolve_channel_config()` entry point
                                     #       (research.md R8)
```

**Structure Decision**: Within `crates/condarc`, one new private module
(`expand_channels.rs`) — the crate is already small and single-purpose, a
second workspace member would add indirection with no benefit. Within
`allez`, one new top-level module tree (`src/channel_config/`) as a
**sibling** to `src/ephemeral/`, not nested inside it — this ticket's
work *produces* the `condarc::ResolvedChannels` value a caller feeds
into `create_ephemeral_environment` directly, it is not itself part of
that creation. FR-016 now requires reaching into `ephemeral`'s own
internals to retire the now-duplicate type and its compensating
behaviors — the one deliberate exception to this ticket's otherwise
additive-only structure.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

Two entries:

**Testing Standards — the public, zero-argument `resolve_channel_config()`
wrapper has no automated integration test** (Constitution Check row II).
Its only branching-free body is `dirs::home_dir()` joined with
`.condarc`, delegating everything else to the already-fully-tested,
path-injectable `resolve_channel_config_from` (research.md R8). Reading
whichever `~/.condarc` happens to exist on the machine running the test
suite would be non-deterministic (Constitution II's own "isolated,
deterministic, and fast" requirement) and risks the crate's own
documented stack-depth-guard gap against an unknown real file. Its only
coverage is the manual, optional `examples/channel_config_smoke.rs`, run
deliberately by a developer, never by `cargo test`.

**Determinism & Idempotency — `resolve_channel_config` performs fresh
disk I/O on every call, with no caching** (Constitution Check row IX).
FR-015 explicitly requires it: two successive calls can legitimately
return different results because the file on disk changed underneath
them, and this ticket's guarantee is "resolve what is in the file now,"
not a memoized snapshot (a cache would make `allez` act on stale user
configuration).
