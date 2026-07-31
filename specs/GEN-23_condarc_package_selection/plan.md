# Implementation Plan: Resolve `.condarc` Channel Preferences for Package Selection

**Branch**: `GEN-23_condarc_package_selection` | **Date**: 2026-07-30 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/GEN-23_condarc_package_selection/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command; its definition describes the execution workflow.

## Summary

This ticket's own deliverable spans two codebase locations, both planned,
implemented, and tested entirely under this one ticket (spec.md Operating
Context): (1) a new, additive `resolve` capability inside the `condarc`
crate (`crates/condarc/src/resolve.rs`) that expands a parsed `Config`'s
channel-related settings — bare names, `custom_channels`/
`custom_multichannels`, the `defaults` placeholder, `channel_priority`,
`allowlist_channels`/`denylist_channels` — into a single, ordered,
credential-stripped `ResolvedChannels` value, layered on top of and never
altering GEN-36's existing `parse()`/`Config`; and (2) a new
`allez`-internal module (`src/channel_config/`) that locates and reads
`~/.condarc`, hands it to the crate's `parse()` then `resolve()`, falls
back to conda's own documented default channel configuration whenever the
file is absent, rejected, or unreadable (recording the rejected/unreadable
cases via structured observability, never the silent absent case), and
adapts the result into the exact four-part `ChannelConfig` shape GEN-24's
already-delivered ephemeral-environment-creation capability requires —
so GEN-25's `allez oneshot` can wire the crate and GEN-24 together
directly, deciding for itself whether to warn or proceed on that signal,
with no further translation needed for either the channel data or the
signal itself. Technical approach for both parts:
pure, hermetic, in-memory transformations (no new I/O beyond the one
`~/.condarc` read `allez`'s own half performs) — no new third-party
dependency in the crate; one dependency promotion in `allez` itself
(`dirs`, already present transitively, promoted to direct, for
cross-platform home-directory resolution).

**Cleanup boundary (per spec.md Assumptions)**: this plan makes no change
to `src/ephemeral/`'s existing, already-delivered code (`channels.rs`'s
`redact_channel_url`, the empty-`channels` fallback, or the allow/deny
filter) — none of it is a genuine duplicate of this ticket's own work
(see spec.md's own "Cleanup boundary" design decision and FR-018); this
plan's own module additions sit strictly *on top of* that module's
existing public surface.

## Technical Context

**Language/Version**: Rust, `edition = "2024"` (matches the existing
workspace `Cargo.toml`; no change).

**Primary Dependencies**: No new dependency in `crates/condarc` —
`resolve()` is pure string/collection logic over types the crate already
exposes (`Config`, `ChannelPriority`), using only `std` (`BTreeMap`
already imported in `model.rs`). In `allez`: `dirs` (currently present
only transitively at `6.0.0`, confirmed via `Cargo.lock`; research.md R9)
is promoted to a direct `[dependencies]` entry for `~/.condarc`'s
cross-platform home-directory resolution; `condarc` itself is promoted
from `[dev-dependencies]`-only to `[dependencies]`, since `allez`'s new
`src/channel_config/` module depends on it at runtime, not only from
tests. Reuses the existing `tracing`/`tracing-subscriber` pipeline
(`src/observability.rs`) for the two new structured-observability event
shapes this ticket adds (research.md R11) — no second logging pipeline.
No new dev-dependency either, and no manifest-declared `[[bin]]` target
(the one thing a prior round's fix specifically removed);
`examples/channel_config_smoke.rs` remains exactly the plain,
Cargo-auto-discovered example already planned for this ticket from the
start (its own manual-smoke-test purpose, quickstart.md) — an existing
planned target, not a new one introduced by this test-mechanism decision.
The one subprocess-based smoke test (research.md R10) re-executes **the
test binary itself** via `std::env::current_exe()`, with `HOME` and a
dedicated marker environment variable (`__CHANNEL_CONFIG_SMOKE_CHILD=1`,
never read by production code) set only on that child `Command`'s own
environment, plus three CLI arguments on the re-executed binary — that
one test function's own libtest name, `--exact`, and `--nocapture` — so
the child's own harness runs only that one test and lets its `println!`
output reach the parent's captured stdout instead of swallowing it. The
child prints its result wrapped in unique sentinel markers and returns
normally rather than calling `std::process::exit()`; the parent asserts
the child's exit status was successful before extracting and comparing
the sentinel-delimited region. `std::process::Command` plus
`std::env::current_exe()` from the standard library is the whole
mechanism, so nothing is added to the workspace root `Cargo.toml` for it.
`assert_cmd` remains an existing `[dev-dependencies]` entry serving the
existing `tests/cli_scaffold.rs` CLI tests, but no test in this ticket's
own plan needs it (research.md R10).

**Storage**: N/A (no database/config file produced by this feature).
The one file this feature ever touches is `~/.condarc` itself, read-only
(FR-015) — never written, moved, or deleted, and never cached or reused
across separate invocations (FR-017: fresh resolution every call).

**Testing**: `cargo test --all` (unchanged entry point; no new opt-in
feature flag is needed, unlike `conformance-tests`/`network-tests` —
every test this ticket adds is hermetic and network-free by
construction). New crate-level unit tests co-located in
`crates/condarc/src/resolve.rs` (Constitution II) plus a new integration
test file, `crates/condarc/tests/resolve_scenarios.rs`, mapping SC-003's
20 named scenarios and SC-005's 6 credential-bearing-location scenarios
one-to-one. New `allez`-level unit tests co-located in
`src/channel_config/*.rs` (the path-injectable internal function, per
research.md R10) — this is where SC-002's four-file-state matrix actually
runs, via `resolve_channel_config_from` directly, needing no test-ordering
or process isolation of any kind, and also where both
observability-capture test groups live (SC-004's 2
fallback-path cases and SC-005/FR-013's 6 credential-location cases — 8
dedicated `#[test]` functions in total, per spec.md's own SC-004/SC-005
text), since `resolve_channel_config_from` is `pub(crate)` and therefore
unreachable from a separate-crate integration test (research.md R10) —
plus a new integration test, `tests/channel_config_resolution.rs`, which
holds exactly two things: the SC-001 five-sample adaptation contract
test, and the one real-public-entry-point smoke test, which is
subprocess-based: it re-executes the test binary itself via
`std::env::current_exe()` with `HOME` set on that child process's own
environment to a `tempfile::tempdir()` (plus a marker environment
variable selecting the child branch, and that one test function's own
libtest name, `--exact`, and `--nocapture` as CLI arguments, so the child
runs only that test and its `println!` output actually reaches the
parent), asserting the child exited successfully and then comparing the
sentinel-delimited region of the child's stdout against a hand-derived
expectation — rather than mutating this test binary's own environment, per
research.md R10. See `research.md` § Test strategy and `quickstart.md`
for the full mapping.

**Target Platform**: Windows amd64, macOS aarch64 (Apple Silicon only),
Linux aarch64, Linux amd64 — the same four targets GEN-24 already
established for the parent epic (GEN-19). This ticket's own
platform-sensitivity is narrower than GEN-24's, but it is not zero: there
are exactly two `cfg`-gated additions, one in production code and one in
test-only code. (1) Production: a single, compile-time constant selection
(`DEFAULT_CHANNELS`'s two-vs-three-URL `cfg(windows)`/non-Windows
variant, research.md R4) — no `unsafe` FFI, no ACL/permission code. (2)
Test-only: the one real-public-entry-point smoke test is
`#[cfg(unix)]`-gated, for one reason only — Windows's `dirs::home_dir()`
resolves via `SHGetKnownFolderPath` and does not consult the
`USERPROFILE` environment variable at all, so no environment override can
redirect it there (research.md R10). That gate has nothing to do with
test isolation: this test achieves its isolation by re-executing **the
test binary itself** (`std::env::current_exe()`) as a **child process**
with `HOME` set on that child's own environment, never by mutating the
test binary's own environment. That second gate does change
CI-matrix behavior, and the resulting asymmetry is accepted deliberately:
this one smoke test does not run on the Windows target at all, while
every other test in this ticket's scope (the crate-level
`resolve_scenarios.rs` suite, and every `allez`-level co-located unit
test driving `resolve_channel_config_from`) still runs on all four
targets. No new CI *job* or matrix dimension is added — `cargo test
--all` remains the single entry point on all four targets exactly as
today; only this one test's per-target coverage is narrower.

**Project Type**: Library additions to two existing locations — a new
private module (`resolve.rs`, selectively re-exported) inside the
already-published `crates/condarc` library, and a new internal module
tree (`src/channel_config/`) inside the `allez` package's own existing
library target (`src/lib.rs`, added by GEN-24). No new CLI surface in
this ticket (spec.md Assumptions: "No CLI or human-facing surface of its
own" — GEN-25's job).

**Performance Goals**: Not defined by this ticket. `resolve()` operates
on a single, already-parsed, in-memory `Config` (at most ~100 keys per
GEN-36's own Scale/Scope note) — not a hot path; no specific throughput
SLO. The one `~/.condarc` file read `allez`'s own half performs is a
single, small (typically <10 KB) local-filesystem read per invocation.

**Constraints**: `resolve()` never fails and performs no I/O of any kind
(FR-001–FR-009 are all pure transformations of an already-in-memory
`Config`); `allez`'s own file-handling layer never blocks on, or fails
because of, a missing/malformed/unreadable `~/.condarc` (FR-010/FR-012 —
"never block" is this ticket's own explicit design goal, since `allez`
runs unattended on behalf of an AI agent); no credential material may
ever appear in `resolve()`'s output or in `allez`'s adapted output
(FR-005); the credential-strip observability record structurally cannot
carry credential material either (only `role`/`index`), but the fallback
event's own `detail` field inherits its credential safety from the
crate's own already-audited error-reporting behavior rather than being
guaranteed by this ticket's own construction, consistent with SC-005's
own accounting of that boundary (FR-013/SC-005);
`~/.condarc` itself is never written to, moved, or deleted under any
circumstance (FR-015); resolution is always fresh, never cached across
separate invocations (FR-017); this ticket's own crate-side work MUST NOT
alter `parse()`'s existing behavior in any way (Operating Context #1),
and its `allez`-side work MUST NOT weaken, remove, or reimplement any of
GEN-24's existing, already-delivered channel-handling behaviors it
happens to overlap in subject matter with (FR-018).

**Scale/Scope**: Two small, single-responsibility additions — one new
crate-private module (`resolve.rs`, ≈150–250 LOC including its own
`#[cfg(test)]` unit tests, per Constitution I's single-clear-responsibility
ceiling) plus its own dedicated integration test file; one new
`allez`-internal module tree (`src/channel_config/`, 2–4 small files:
orchestration, path/read handling, adaptation, observability events) plus
its own dedicated integration test file. No schema/migration/multi-service
scope; no change to any already-delivered public type's own fields or
methods (GEN-24's `ChannelConfig`/`ChannelSpec`/`ChannelPriorityMode`,
GEN-36's `Config`/`ChannelPriority`/`ValidationReport`, are all consumed
as-is, never modified). One caveat on "already-delivered": GEN-24 merged
with a small number of reviewer threads left explicitly "revisit later"
at merge time (API return-type bikeshedding, a question about splitting
one API call into two, and some discomfort with reclamation appearing in
the API's own structure), none of which names
`ChannelConfig`/`ChannelSpec`/`ChannelPriorityMode` directly — so this
ticket's reliance on those three types specifically remains sound, but
"already-delivered" should not be read as "this API surface is
permanently frozen."

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|---|---|---|
| I. Code Quality | PASS | Two new, single-responsibility module additions (`crates/condarc/src/resolve.rs`; `allez`'s `src/channel_config/{mod,locate,adapt,events}.rs`) — each file maps to exactly one of `research.md`'s decisions. No `unsafe` code anywhere in this ticket's own additions, production or test (contrast GEN-24's documented FFI exception category, which this ticket does not touch or extend): the one test that must observe a real home directory does so by spawning a child process with its own environment (research.md R10), so no `std::env` mutation — and therefore no `unsafe` block — appears anywhere in this ticket's scope. |
| II. Testing Standards | PASS | TDD: every test this ticket adds is written *before* the implementation exists, per Constitution II's Red-Green-Refactor cycle — `crates/condarc/tests/resolve_scenarios.rs` against SC-003 and SC-005's crate-side scenarios; `tests/channel_config_resolution.rs` against SC-001 (plus the one real-public-entry-point smoke test); and the co-located `#[cfg(test)]` unit tests in `src/channel_config/` against SC-002 and against SC-004/FR-013's observability-capture assertions, which have to be unit tests rather than integration tests because `resolve_channel_config_from` is `pub(crate)` and a separate-crate integration test cannot call it (research.md R10). Unit tests co-located (`#[cfg(test)]`) for every pure, path-injectable function (research.md R10); integration tests cover the two public, cross-module-boundary entry points (`condarc::resolve`, `allez::channel_config::resolve_channel_config`). |
| III. Dual-Primary Interface | N/A (justified) | Neither half of this ticket ships a CLI subcommand of its own (spec.md Assumptions: GEN-25's job). Both public functions return fully-typed values (`ResolvedChannels`, `ChannelConfigResolution`) specifically so GEN-25 can satisfy this principle later without re-deriving anything from string output — the same posture GEN-24's own plan documented for its own public API. |
| IV. DRY | PASS | `resolve()` reuses `Config`'s already-coerced `channel_priority` rather than re-implementing GEN-36's own legacy-boolean-spelling mapping (research.md R2); `allez`'s adapter constructs GEN-24's existing `ChannelConfig`/`ChannelSpec` types directly rather than inventing a parallel shape. The one deliberate, documented DRY exception: credential stripping is implemented independently in the crate rather than shared with `allez::ephemeral::channels::redact_channel_url` — justified in research.md R7 (wrong dependency direction, different contract: stripping-with-reporting vs. debug-only redaction), matching Constitution IV's own "exceptions to DRY MUST be documented with rationale" clause. |
| V. Explicit Over Implicit | PASS | `resolve()` is a plain, fully-typed, non-`Result` pure function (never fails, so there is no error type to swallow implicitly); `resolve_channel_config` is likewise total, with its two internal fallback paths distinguished by an explicit `ReadOutcome`/`FallbackReason` enum, never inferred from a bare `io::Error`'s kind at the call site. No `.unwrap()`/`.expect()` outside tests in either new module. Default values (`DEFAULT_CHANNELS`/`DEFAULT_CHANNEL_ALIAS`/`DEFAULT_CUSTOM_CHANNELS`) are named constants sourced directly from `docs/condarc_research.md`'s already-verified values, never re-derived ad hoc. |
| VI. Documentation and Type Safety | PASS | Every new public item (`condarc::resolve`, `ResolvedChannels`, `ChannelListRole`, `CredentialStrippingEvent`, `allez::channel_config::resolve_channel_config`, `ChannelConfigResolution`, `FallbackReason`) gets a `///` doc comment describing purpose/parameters/return/errors, per `data-model.md`/`contracts/*.md`. Invalid states made unrepresentable: `ChannelListRole` is a closed, `#[non_exhaustive]` enum rather than a string; `ReadOutcome`/`FallbackReason` are closed enums rather than inspecting `io::Error::kind()` ad hoc at each call site. `cargo doc` must build without warnings on both new modules (implementation-time verification). |
| VII. No Hardcoded Values | PASS | `~/.condarc`'s location is resolved via `dirs::home_dir()` + `PathBuf::join(".condarc")` (research.md R9), never a hand-rolled `$HOME`/`%USERPROFILE%` string concatenation — directly satisfying this principle's own path-handling rationale ("conda targets multiple platforms, so path handling must not assume one"). `DEFAULT_CHANNELS`/`DEFAULT_CHANNEL_ALIAS`/`DEFAULT_CUSTOM_CHANNELS` are named, documented constants, not inline literals scattered through `resolve.rs`. |
| VIII. Mandatory 100% Spec Test Coverage | PASS (planned) | `quickstart.md` enumerates the acceptance-scenario/SC → test mapping at a plan level (exact test IDs are the task-breakdown phase's own job, per this command's own scope boundary). Covers all 3 user stories' acceptance scenarios, all 5 success criteria (SC-001 through SC-005), and every numbered FR this ticket introduces (FR-001–FR-019). |
| IX. Determinism & Idempotency | PASS (with one documented, scoped exception — see Complexity Tracking) | `resolve()` is a pure function of its `&Config` input — identical input always produces identical `ResolvedChannels` (no randomness, no I/O, no ambient state). `resolve_channel_config`, by contrast, is a non-pure, I/O-performing wrapper: it reads `~/.condarc` fresh from disk on every call by explicit design (FR-017 — "MUST NOT cache or reuse a previous resolution result"), so two calls can legitimately return different values. That is a documented, justified deviation from this principle's literal idempotency clause — the same treatment Constitution IV's own DRY exception already receives in row IV of this table — not an argument that the clause does not govern this function; it is documented as this ticket's one Complexity Tracking entry below. `Cargo.lock` stays committed once the `dirs` dependency promotion lands (implementation-time task). |
| X. Security & Supply-Chain Integrity | PASS | `dirs` is already present in the resolved dependency graph today (pulled in transitively at `6.0.0`); promoting it to a direct dependency adds no new supply-chain surface to `cargo audit`/`cargo deny` beyond what is already being audited. No new crate is added to `crates/condarc`. Neither half of this ticket downloads, executes, or verifies any package artifact — this ticket's entire scope is reading and interpreting one local, already-trusted config file, not installing untrusted code (that boundary belongs to GEN-24, unaffected by this ticket per FR-018). Credential material (FR-005/FR-013) is this ticket's own supply-chain-adjacent concern and is treated with the same rigor the constitution's rationale implies, even though this principle's own text is framed around package artifacts rather than config-file secrets. |
| XI. Structured Observability | PASS | Two new, narrowly-scoped `tracing`-emitted event shapes (`ChannelConfigFallbackEvent`, `CredentialStripLogRecord`, research.md R11), each with its own `schema_version` constant, emitted through the existing `tracing`/`observability.rs` subscriber — no ad hoc `println!`/`eprintln!`, no second logging pipeline. The two event shapes' credential-safety guarantees are of different strengths and are worth stating separately: the credential-strip record *structurally* cannot carry credential material — it never carries the stripped value or the resolved URL at all, only `role`/`index` — a genuine by-construction guarantee of this ticket's own making; the fallback event's `detail` field, by contrast, inherits its credential safety from the crate's own already-audited error-reporting behavior, and is *not* guaranteed by this ticket's own construction, consistent with SC-005's own accounting of that boundary as out of this ticket's control to re-guarantee. |

No constitution violations requiring justification — every principle
gates PASS with no documented exception beyond the one, explicitly
justified DRY deviation (credential-stripping independence, row IV) and
the one explicitly justified, scoped Determinism exception
(`resolve_channel_config`'s fresh-read-every-call I/O, row IX), both
already resolved by the spec itself rather than requiring a new team
decision. The Determinism exception is recorded as this ticket's one
Complexity Tracking entry below; the DRY exception is fully justified in
row IV itself and needs no separate entry.

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
└── tasks.md               # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
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
    ├── lib.rs             # updated — `mod resolve;` + `pub use resolve::{resolve, ResolvedChannels, ChannelListRole, CredentialStrippingEvent};`
    ├── catalog.rs         # existing — untouched
    ├── coerce/            # existing — untouched
    ├── error.rs            # existing — untouched
    ├── model.rs            # existing — untouched (Config/ChannelPriority consumed as-is)
    ├── parse.rs            # existing — untouched
    ├── validate.rs         # existing — untouched
    └── resolve.rs         # NEW — resolve(), ResolvedChannels, ChannelListRole,
                             #       CredentialStrippingEvent, and the private
                             #       resolve_entry/resolve_member/match_custom_channel/
                             #       strip_credentials/ResolveContext helpers
                             #       (research.md R1/R5/R6/R7/R8; data-model.md)

crates/condarc/tests/
├── public_api_usage.rs    # existing — untouched
├── parse_invalid.rs       # existing — untouched
├── parse_valid.rs         # existing — untouched
├── multi_error_accumulation.rs  # existing — untouched
└── resolve_scenarios.rs   # NEW — SC-003's 20 named scenarios + SC-005's 6
                             #       credential-bearing-location scenarios,
                             #       one #[test] per scenario (quickstart.md)

Cargo.toml (workspace root)
└── [dependencies]          # gains `dirs = "6"` (promoted from transitive);
                             #       gains `condarc = { path = "crates/condarc" }`
                             #       (promoted from [dev-dependencies]-only).
                             #       No other manifest change: this ticket adds no
                             #       new dependency and no manifest-declared [[bin]]
                             #       target (the one thing a prior round's fix
                             #       specifically removed) — the subprocess smoke test
                             #       re-executes the test binary itself via
                             #       std::env::current_exe(), and
                             #       examples/channel_config_smoke.rs remains exactly
                             #       the plain, Cargo-auto-discovered example already
                             #       planned for this ticket from the start (its own
                             #       manual-smoke-test purpose, quickstart.md) — an
                             #       existing planned target, not a new one introduced
                             #       by this test-mechanism decision (research.md R10)

src/
├── lib.rs                 # updated — `pub mod channel_config;` alongside the
                             #       existing `pub mod cli; pub mod error; pub mod ephemeral;
                             #       pub mod observability; pub mod output;`
├── ephemeral/              # existing — untouched (FR-018; this ticket only
                             #       consumes its existing pub ChannelConfig/
                             #       ChannelSpec/ChannelPriorityMode, never edits them)
├── cli/                    # existing — untouched (GEN-25's job to wire this
                             #       ticket's output into `oneshot`)
├── error.rs                 # existing — untouched
├── observability.rs         # existing — reused, not duplicated
├── output.rs                 # existing — untouched
├── main.rs                   # existing — untouched
└── channel_config/           # NEW — this ticket's entire allez-side scope
    ├── mod.rs                 # public API: resolve_channel_config() (FR-010–FR-019),
                                 #       returning ChannelConfigResolution{config, fallback}
                                 #       (research.md R12); pub(crate)
                                 #       resolve_channel_config_from(path) (research.md R10);
                                 #       default_condarc_path();
                                 #       pub use events::FallbackReason; — re-exported so
                                 #       external callers (GEN-25) can match its variants.
                                 #       Also holds the co-located #[cfg(test)] module
                                 #       driving resolve_channel_config_from: SC-002's
                                 #       4-file-state matrix AND both observability-capture
                                 #       test groups (SC-004's 2 fallback-path cases,
                                 #       SC-005/FR-013's 6 credential-location cases — 8
                                 #       dedicated #[test] functions) — these must be unit
                                 #       tests, not integration tests, because
                                 #       resolve_channel_config_from is pub(crate) and a
                                 #       separate-crate integration test cannot call it
                                 #       (research.md R10)
    ├── locate.rs               # read_condarc()/ReadOutcome — distinguishes
                                #       FR-010's silent "missing" from FR-012's
                                #       recorded "unreadable" (data-model.md)
    ├── adapt.rs                 # adapt(condarc::ResolvedChannels) -> ChannelConfig
                                #       (FR-014, lossless field-by-field mapping)
    └── events.rs                 # ChannelConfigFallbackEvent/CredentialStripLogRecord
                                #       shapes + tracing emission (FR-012/FR-013); the
                                #       tests asserting those emissions are co-located in
                                #       mod.rs's #[cfg(test)] module above, alongside the
                                #       other resolve_channel_config_from-driven tests

tests/
├── cli_scaffold.rs          # existing — untouched
├── condarc_conformance.rs   # existing — untouched
├── ephemeral_env.rs          # existing — untouched
└── channel_config_resolution.rs  # NEW — exactly two things: SC-001's 5-sample
                                     #       adaptation contract test, and the one
                                     #       subprocess-based public-entry-point smoke
                                     #       test — re-executes this same test binary via
                                     #       std::env::current_exe(), with HOME and a
                                     #       marker env var set on the child's own
                                     #       environment, plus this test's own libtest
                                     #       name, --exact, and --nocapture as CLI args so
                                     #       the child runs only this test and its
                                     #       println! output reaches the parent; the child
                                     #       prints sentinel-wrapped output and returns
                                     #       normally (no std::process::exit()), and the
                                     #       parent asserts the child's exit status before
                                     #       parsing (#[cfg(unix)], for the Windows
                                     #       SHGetKnownFolderPath reason only —
                                     #       research.md R10), itself asserting
                                     #       ChannelConfigResolution.fallback (FR-019)
                                     #       (quickstart.md). SC-002's own 4-file-state
                                     #       matrix and both SC-004/FR-013
                                     #       observability-capture test groups (8 #[test]
                                     #       functions) run as unit tests
                                     #       co-located in src/channel_config/mod.rs
                                     #       instead — resolve_channel_config_from is
                                     #       pub(crate) and unreachable from here
                                     #       (research.md R10)

examples/
└── channel_config_smoke.rs   # NEW — manual smoke test only (quickstart.md), a plain
                                    #       Cargo-auto-discovered example in the same
                                    #       category as the existing `ephemeral_smoke.rs`,
                                    #       planned for this ticket from the start — an
                                    #       existing planned target, not one introduced by
                                    #       the automated smoke test's own mechanism, and
                                    #       specifically not a manifest-declared [[bin]];
                                    #       nothing else is declared about it in
                                    #       Cargo.toml, and the automated smoke test does
                                    #       not run it — that test re-executes its own
                                    #       test binary instead (research.md R10)
```

**Structure Decision**: Within `crates/condarc`, one new private module
(`resolve.rs`) — the crate is already small and single-purpose; a second
crate/workspace member for a ~200-line additive capability layered on
top of the crate's own existing `Config` would add indirection with no
corresponding benefit (no consumer other than `allez` exists today, and
`resolve()`'s own dependency on `Config`/`ChannelPriority` is intrinsic,
not incidental). Within the `allez` package, one new top-level module
tree (`src/channel_config/`) as a **sibling** to `src/ephemeral/`, not
nested inside it — this ticket's own work is a distinct concern (locate,
read, parse, resolve, adapt) that *produces* a `ChannelConfig` for a
caller to feed into `create_ephemeral_environment`; it is not itself part
of ephemeral-environment creation, and FR-018 specifically requires this
ticket to sit on top of `ephemeral`'s existing public surface without
reaching into or editing its internals. This mirrors GEN-24's own
Structure Decision reasoning (a cohesive module directory over a new
crate/workspace member) applied one level up, to sibling-module placement
within the same package.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

One entry, from Constitution Check row IX:

**Determinism & Idempotency — `resolve_channel_config` performs fresh
disk I/O on every call, with no caching.** *What it is*: the public
`allez`-side entry point is not a pure function; it re-locates and
re-reads `~/.condarc` on every invocation, so two successive calls with
no intervening code change can return two different
`ChannelConfigResolution` values (different `config`, and potentially a
different `fallback`) purely because the file on disk changed underneath
them. *Why it is necessary*: FR-017 explicitly requires it ("MUST resolve
fresh from `~/.condarc` on every invocation; MUST NOT cache or reuse a
previous resolution result across separate invocations"). The file's
contents can legitimately change between invocations, and this ticket's
guarantee to its caller is "resolve what is in the file now," not
"resolve a memoized snapshot from an earlier call" — a cache would make
`allez` act on stale user configuration, which is a worse failure than
non-repeatability, and `quickstart.md`'s own FR-017 test asserts the
two-different-results behavior directly. *Simpler alternative rejected*:
caching the first resolution per process would make the function
trivially repeatable but would directly violate FR-017 and defeat the
requirement's own purpose. This is this ticket's one Complexity Tracking
entry; the other documented deviation (the DRY exception for
credential-stripping independence) is fully justified in Constitution
Check row IV and research.md R7 and needs no entry here.

Otherwise, this ticket adds no `unsafe` code at all — not in
`resolve.rs`/`channel_config/*.rs`, and not in any of its tests either,
since the one test that must observe a real home directory spawns a child
process with its own environment rather than mutating this process's
(research.md R10, Constitution Check row I). It adds no new workspace
member and no
platform-specific FFI; its one platform-sensitive production line
(`DEFAULT_CHANNELS`'s `cfg(windows)` constant selection, research.md R4)
is a compile-time constant choice, not a runtime OS-detection API call,
and needs no justification beyond research.md's own rationale for it (the
second `cfg` gate, on that one test-only smoke test, is accounted for in
Technical Context's Target Platform section above).
