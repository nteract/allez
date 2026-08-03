---

description: "Task list template for feature implementation"
---

# Tasks: Resolve `.condarc` Channel Preferences for Package Selection

**Input**: Design documents from `/specs/GEN-23_condarc_package_selection/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/condarc_resolve_api.md, contracts/allez_channel_config_api.md, quickstart.md (all present)

**Tests**: NOT optional for this ticket. Constitution VIII mandates 100% spec test coverage (every acceptance scenario, FR, and SC below). Constitution II mandates tests are written before their implementation (Red-Green-Refactor) — every task with real branching logic has its test listed before it, except where noted otherwise for a documented technical reason; a test may reference a not-yet-existing function or assert a value the current stub can't yet produce, and is expected to fail until the corresponding implementation task lands.

**Organization**: Tasks are grouped by user story (spec.md priorities: US1=P1, US2=P1, US3=P2) to enable independent implementation and testing, with a Setup and Foundational phase first per plan.md's two-codebase-location structure (`crates/condarc` and `allez`'s own `src/channel_config/`).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: US1, US2, or US3 — maps to spec.md's three user stories
- File paths are exact, taken from plan.md's Project Structure

## Path Conventions

Single Cargo workspace, two members:

- `crates/condarc/src/expand_channels.rs` — the new, additive channel-resolution capability (crate-side)
- `crates/condarc/tests/expand_channels_scenarios.rs` — new crate-level integration test
- `src/channel_config/{mod.rs,locate.rs,events.rs}` — the new `allez`-side module tree
- `src/ephemeral/channels.rs` — MODIFIED (FR-016): loses `ChannelConfig`/`ChannelSpec`/`ChannelPriorityMode`/`filter_channels`/`channels_with_fallback`; `redact_channel_url` survives
- `examples/channel_config_smoke.rs` — new manual smoke example
- `Cargo.toml` (workspace root) — gains `dirs`, promotes `condarc`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Wire the two new module locations into the workspace so Foundational/User-Story work has somewhere to land.

- [x] T001 [P] Update root `Cargo.toml`: add `dirs = "6"` under `[dependencies]`, and move `condarc = { path = "crates/condarc" }` out of `[dev-dependencies]` into `[dependencies]` (research.md R7, plan.md Primary Dependencies)
- [x] T002 [P] Create `crates/condarc/src/expand_channels.rs` as an empty module (module-level doc comment only) and add `mod expand_channels;` to `crates/condarc/src/lib.rs` (plan.md Project Structure)
- [x] T003 [P] Create the `src/channel_config/` module tree — `mod.rs`, `locate.rs`, `events.rs`, each with a module-level doc comment and no public items yet — and add `pub mod channel_config;` to `src/lib.rs` (plan.md Project Structure)

**Checkpoint**: `cargo build --all` succeeds with the new, still-empty module tree wired in.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: The data-model types every user story's tests build on. No FR-001–FR-008/FR-019 resolution logic yet — that is User Story 1's and User Story 3's own deliverable. Every one of these tasks is a type/shape declaration with no branching logic, so Constitution II's Red-Green-Refactor cycle does not meaningfully apply to them individually.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [x] T004 Define the `ResolvedChannels` struct (`#[non_exhaustive]`, `channels: Vec<String>`, `channel_priority: ChannelPriority`, `Debug, Clone, PartialEq, Eq`) in `crates/condarc/src/expand_channels.rs` (data-model.md)
- [x] T005 Define the `ExpandChannelsError` enum (`#[non_exhaustive]`, one variant `EmptyChannelAlias { entry: String }`) plus its manual `Display`/`std::error::Error` impls in `crates/condarc/src/expand_channels.rs` (data-model.md, research.md R11) — depends on T004 (same file); its `Display` wording is asserted by T029's `.to_string()` check
- [x] T006 Define the three private default constants — `DEFAULT_CHANNEL_ALIAS`, `cfg`-gated `DEFAULT_CHANNELS` (non-Windows 2-entry / Windows 3-entry), `DEFAULT_CUSTOM_CHANNELS` — in `crates/condarc/src/expand_channels.rs` (research.md R4) — depends on T005 (same file)
- [x] T007 Define the private `ResolveContext<'a>` struct (`channel_alias: &'a str`, `custom_channels: HashMap<&'a str, &'a str>`, `custom_multichannels: &'a BTreeMap<String, Vec<String>>`, `default_channels: Vec<&'a str>`) in `crates/condarc/src/expand_channels.rs` (data-model.md) — depends on T006 (same file)
- [x] T008 Add a placeholder `pub fn expand_channels(config: &Config) -> Result<ResolvedChannels, ExpandChannelsError>` in `crates/condarc/src/expand_channels.rs` that returns `Ok(ResolvedChannels { channels: Vec::new(), channel_priority: ChannelPriority::Flexible })` unconditionally (a compiling stub — User Story 1 replaces the body), and add `pub use expand_channels::{expand_channels, ResolvedChannels, ExpandChannelsError};` to `crates/condarc/src/lib.rs` — depends on T004–T007
- [x] T009 [P] Define the `FallbackReason` enum (`#[non_exhaustive]`, `Rejected`, `Unreadable`, `Debug, Clone, Copy, PartialEq, Eq`) in `src/channel_config/events.rs` (data-model.md)
- [x] T010 Define the `ChannelConfigResolution` enum (`#[non_exhaustive]`, `Debug, Clone, PartialEq`, `Ready { config: condarc::ResolvedChannels, fallback: Option<FallbackReason> }`, `NoChannels`) in `src/channel_config/mod.rs`, plus `pub use events::FallbackReason;` (data-model.md, research.md R13) — depends on T009 and T008 (needs `ResolvedChannels` re-exported to name it)
- [x] T011 [P] Define the private `ReadOutcome` enum (`Missing`, `Unreadable(io::Error)`) in `src/channel_config/locate.rs` (data-model.md) — independent of T009/T010

**Checkpoint**: `cargo build --all` succeeds; both new public surfaces (`condarc::expand_channels`/`ResolvedChannels`/`ExpandChannelsError`, `allez::channel_config`'s types) exist and compile, with `expand_channels()` still a stub.

---

## Phase 3: User Story 1 - Resolve real channel preferences into one ready-to-use list (Priority: P1) 🎯 MVP

**Goal**: `condarc::expand_channels()` turns a parsed `Config`'s `channels`/`channel_alias`/`custom_channels`/`custom_multichannels`/`default_channels` into one flat, ordered list of concrete channel identifiers, with every bare name and `defaults` reference expanded per FR-001's precedence.

**Independent Test**: Feed `expand_channels()` a `Config` (via `condarc::parse`) exercising `channels`, `channel_alias`, `custom_channels`, `custom_multichannels`, and `default_channels` together; assert the resolved `channels` is one flat, ordered list preserving input order.

### Tests for User Story 1 (write first — Constitution II)

> Every test below targets a function or behavior that does not exist yet, or that the Foundational stub (T008) does not yet produce. Confirm each one fails before starting the Implementation subsection.

- [x] T013 [P] [US1] Unit tests for `match_custom_channel()` in `crates/condarc/src/expand_channels.rs`: exact match, progressive-prefix match (`"acme/label/dev"` against `{"acme": "https://internal.example.com"}"` -> `"https://internal.example.com/acme/label/dev"`), no match -> `None`
- [x] T014 [P] [US1] Unit tests for `resolve_member()` in `crates/condarc/src/expand_channels.rs`: scheme-match passthrough, `channel_alias` join with correct single-slash join, restricted precedence never consults `custom_channels`/`custom_multichannels`, `Err(EmptyChannelAlias)` on an empty `channel_alias`
- [x] T015 [P] [US1] Unit tests for `resolve_entry()` in `crates/condarc/src/expand_channels.rs`: branch (a) scheme match, branch (b) `custom_multichannels`/`"defaults"`-first lookup expanding to multiple members, branch (c) `custom_channels`, branch (d) `channel_alias`, and `Err(EmptyChannelAlias)` propagation from branch (d)
- [x] T016 [US1] Create `crates/condarc/tests/expand_channels_scenarios.rs` with a shared helper that parses a `.condarc` YAML string via `condarc::parse` and resolves it via `condarc::expand_channels` (already callable against the T008 stub)
- [x] T017 [P] [US1] SC-003 scenario 1 (bare-name/alias-only resolution) test in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016
- [x] T018 [P] [US1] SC-003 scenario 2 (`custom_channels` exact-match) test in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016
- [x] T019 [P] [US1] SC-003 scenario 3 (`custom_channels` progressive-prefix-match, `"acme/label/dev"`) test in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016
- [x] T020 [P] [US1] SC-003 scenarios 4–5 (`custom_multichannels` member naming another multichannel / a `custom_channels` entry, neither expanded further) tests in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016
- [x] T021 [P] [US1] SC-003 scenarios 6–9 (`defaults` substitution via explicit `[defaults]`, absent `channels`, explicit `null`, explicit `[]`, all four equivalent) tests in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016
- [x] T022 [P] [US1] SC-003 scenario 10 (a user-configured, non-built-in `default_channels` value is what actually substitutes for `defaults`) test in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016
- [x] T023 [P] [US1] SC-003 scenarios 19–20 (`channels`/`channel` and `allowlist_channels`/`whitelist_channels` alias-collision malformed input, asserting `condarc::parse` itself returns `Err`) tests in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016 (FR-005, research.md R3; these already pass against `parse()`'s existing behavior, unaffected by the `expand_channels()` stub)
- [x] T024 [P] [US1] SC-003 scenario 21 (`channel_alias` with a trailing slash joined against a bare name — exactly one slash in the result) test in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016
- [x] T025 [P] [US1] SC-003 scenario 22 (a dot-containing bare name that does not match the scheme pattern still resolves via `channel_alias`) test in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016
- [x] T026 [P] [US1] User Story 1 Acceptance Scenario 5 test (a `custom_multichannels` member naming another multichannel, a `custom_channels` entry, or the multichannel being defined itself resolves as an ordinary bare name via `channel_alias`) in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016
- [x] T027 [P] [US1] FR-006/FR-007/FR-008 tests (`override_channels_enabled` has zero effect; two different bare names expanding to the same URL both survive uncollapsed; `channel_settings` never appears in `ResolvedChannels.channels`) in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016
- [x] T028 [P] [US1] `Config::default()` empty-document test (resolves to the built-in `DEFAULT_CHANNELS` URLs with `channel_priority: Flexible`, and cannot reach `EmptyChannelAlias`) in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016
- [x] T029 [P] [US1] SC-005 crate-level test (a `.condarc` resolving a channel-list entry through an explicit empty-string `channel_alias` asserts `expand_channels()` returns `Err(ExpandChannelsError::EmptyChannelAlias { entry })` with the exact triggering entry, and that its `.to_string()` contains that entry) in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016
- [x] T071 [P] [US1] User Story 1 Acceptance Scenario 4 test: a `.condarc` of `channels: [conda-forge, https://example.com/x]` with `default_channels: [https://repo.example.org/main]` (a non-default value, so a bug that always appends `default_channels` is caught) asserts the resolved list is exactly `["https://conda.anaconda.org/conda-forge", "https://example.com/x"]` — no `https://repo.example.org/main` member present — in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016
- [x] T072 [P] [US1] FR-002 explicit-empty test: `default_channels: []` with `channels: [defaults]` and no `custom_multichannels.defaults` entry resolves to an empty `channels` list (distinct, valid value — not replaced by the platform-specific built-in default), asserted alongside a sibling case in the same test — `channels: [conda-forge]` with no `default_channels` override resolves to a non-empty `["https://conda.anaconda.org/conda-forge"]` — so the test as a whole fails against the T008 stub (which returns an empty list unconditionally) and only passes once T033 lands, in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016
- [x] T073 [P] [US1] FR-002 explicit-empty test: `custom_channels: {}` with `channels: [pkgs/pro]` (the one entry that would otherwise match conda's built-in `custom_channels` mapping) resolves via `channel_alias` instead, to exactly `["https://conda.anaconda.org/pkgs/pro"]` — the explicit empty map is not replaced by conda's built-in `custom_channels` mapping (which would instead resolve to `https://repo.anaconda.com/pkgs/pro`) — in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016

### Implementation for User Story 1

- [x] T030 [US1] Implement `match_custom_channel(entry: &str, custom_channels: &HashMap<&str, &str>) -> Option<String>` in `crates/condarc/src/expand_channels.rs`: try `entry` itself, then each successive `/`-delimited prefix, longest-match-first; on a hit, join as `base_url.trim_end_matches('/') + "/" + entry` using the **original, full** entry (research.md R6) — makes T013 pass
- [x] T031 [US1] Implement `resolve_member(entry: &str, channel_alias: &str) -> Result<String, ExpandChannelsError>` in `crates/condarc/src/expand_channels.rs`: scheme-pattern (`^[a-z][a-z0-9]{0,11}://`) match used as-is, otherwise `channel_alias.trim_end_matches('/') + "/" + entry`, propagating `Err(EmptyChannelAlias { entry })` when `channel_alias` is empty — deliberately skips custom_channels/custom_multichannels (research.md R5) — depends on T030 (same file); makes T014 pass
- [x] T032 [US1] Implement `resolve_entry(entry: &str, ctx: &ResolveContext) -> Result<Vec<String>, ExpandChannelsError>` in `crates/condarc/src/expand_channels.rs`: full FR-001 precedence (a) scheme match as-is, (b) `custom_multichannels` lookup by `entry`'s own name — but when `entry` is the literal `"defaults"`, check `ctx.custom_multichannels` for a `"defaults"` key first; if none exists there, fall back to `ctx.default_channels` for its members instead (empty `ctx.default_channels` yields zero members, per FR-002's explicit-empty row) — each member expanded via `resolve_member`, (c) `match_custom_channel`, (d) `channel_alias` join via `resolve_member`'s same join rule — depends on T030, T031; makes T015 pass
- [x] T033 [US1] Replace the `expand_channels()` stub body in `crates/condarc/src/expand_channels.rs`: build the effective `ResolveContext` from `config` per FR-002's defaulting table (`channels`/`channel` alias already unified by `parse()`, defaulting to `[defaults]` when absent/null/empty; `channel_alias` defaulting to `DEFAULT_CHANNEL_ALIAS`; `custom_channels` defaulting to `DEFAULT_CUSTOM_CHANNELS`; `custom_multichannels` defaulting to an empty `BTreeMap`; `default_channels` defaulting to `DEFAULT_CHANNELS`), resolve the `channels` role by calling `resolve_entry` per entry in order and flattening results, set `channel_priority` via `config.channel_priority.unwrap_or(ChannelPriority::Flexible)` (FR-003, research.md R2), and return `Ok(ResolvedChannels { channels, channel_priority })` — no allow/deny filtering yet (User Story 3's own scope) — depends on T032; makes T017–T029, T071–T073 pass

**Checkpoint**: `cargo test -p condarc` passes; `condarc::expand_channels()` correctly resolves bare names, `custom_channels`, `custom_multichannels`, and `defaults` per FR-001/FR-002, independently of `allez`.

---

## Phase 4: User Story 2 - Never block on a missing or broken preferences file (Priority: P1)

**Goal**: `allez::channel_config::resolve_channel_config()`/`resolve_channel_config_from()` always return a fully-populated `ChannelConfigResolution`, falling back to conda's own documented defaults on a missing, rejected, unreadable, or unexpandable `~/.condarc`, with the fallback condition recorded via observability and exposed in the result itself.

**Independent Test**: Call `resolve_channel_config_from` against (a) a nonexistent path, (b) a path whose contents `condarc::parse` rejects, and (c) a path containing invalid UTF-8 bytes; confirm all three return `Ready` with conda's documented default channel configuration, and that (b)/(c) additionally set `fallback` and emit exactly one `ChannelConfigFallbackEvent`.

### Tests for User Story 2 (write first — Constitution II)

> Every test below targets a function that does not exist yet in this phase. Confirm each one fails (or fails to compile) before starting the Implementation subsection.

- [x] T034 [P] [US2] Unit tests for `read_condarc()` in `src/channel_config/locate.rs`: nonexistent path -> `Missing`; a `tempfile` written with invalid UTF-8 bytes -> `Unreadable`; a valid populated file -> `Ok` (research.md R8)
- [x] T036 [P] [US2] Unit test for `emit_fallback()` in `src/channel_config/events.rs`: captured via a per-test-scoped `tracing::subscriber::with_default`, asserting the emitted event carries exactly `reason`, `schema_version` equal to `CHANNEL_CONFIG_EVENT_SCHEMA_VERSION`, and `detail` — no additional or omitted fields (SC-004, unit half)
- [x] T037 [P] [US2] SC-004 test: `resolve_channel_config_from` driven against a `.condarc` `condarc::parse` rejects emits exactly one `ChannelConfigFallbackEvent { reason: Rejected, .. }`, captured via a per-test-scoped `tracing::subscriber::with_default`, in `src/channel_config/mod.rs`
- [x] T038 [P] [US2] SC-004 test: `resolve_channel_config_from` driven against an invalid-UTF-8 fixture emits exactly one `ChannelConfigFallbackEvent { reason: Unreadable, .. }`, captured the same way, in `src/channel_config/mod.rs`
- [x] T039 [P] [US2] SC-004 test: `resolve_channel_config_from` driven against a `.condarc` triggering `condarc::expand_channels`'s `EmptyChannelAlias` `Err` emits exactly one `ChannelConfigFallbackEvent { reason: Rejected, .. }` carrying the `ExpandChannelsError`'s own detail, in `src/channel_config/mod.rs`
- [x] T040 [P] [US2] FR-009 test (grouped with SC-004's observability tests since it's the negative case of the same assertion): `resolve_channel_config_from` driven against a missing file emits **zero** `ChannelConfigFallbackEvent`, in `src/channel_config/mod.rs`
- [x] T042 [P] [US2] SC-002 test: `resolve_channel_config_from(Some(<nonexistent path>))` -> `Ready { fallback: None, .. }` with conda's documented defaults, in `src/channel_config/mod.rs`
- [x] T043 [P] [US2] SC-002 test: `resolve_channel_config_from` against a `.condarc` `condarc::parse` rejects -> `Ready { fallback: Some(FallbackReason::Rejected), .. }`, in `src/channel_config/mod.rs`
- [x] T044 [P] [US2] SC-002 test: `resolve_channel_config_from` against an invalid-UTF-8 fixture -> `Ready { fallback: Some(FallbackReason::Unreadable), .. }`, in `src/channel_config/mod.rs`
- [x] T045 [P] [US2] SC-002/SC-005 (allez-level) test: `resolve_channel_config_from` against a `.condarc` triggering `condarc::expand_channels`'s `EmptyChannelAlias` `Err` -> `Ready { fallback: Some(FallbackReason::Rejected), .. }`, in `src/channel_config/mod.rs`
- [x] T046 [P] [US2] SC-002 test: `resolve_channel_config_from` against a real, populated, non-empty `.condarc` -> `Ready { fallback: None, .. }` carrying that file's actual resolved channels, in `src/channel_config/mod.rs`
- [x] T047 [P] [US2] SC-002 argument-level test: `resolve_channel_config_from(None)` (undeterminable home directory) takes the same silent fallback path as a missing file, as its own distinct test case, in `src/channel_config/mod.rs`
- [x] T048 [US2] SC-001 pass-through test: 5 distinct, real-world-shaped `.condarc` samples, each parsed via `condarc::parse` and resolved via the real `condarc::expand_channels`, asserting `resolve_channel_config_from`'s `Ready.config` equals that same `ResolvedChannels` value exactly (FR-012, research.md R15) — no field-mapping to verify, since `Ready.config` is `condarc::ResolvedChannels` directly; this test guards against `mod.rs` ever silently wrapping or transforming it:
  1. `channels: [conda-forge, defaults]`
  2. `channels: [acme/label/dev, team]`, `custom_channels: {acme: "https://internal.example.com"}`, `custom_multichannels: {team: ["https://other.example.com/x", member2]}`
  3. `channels: [defaults, https://custom.example.com/chan]`
  4. `channels: [conda-forge]`, `channel_priority: true`
  5. `{}` (empty document, `Config::default()`)

  In `src/channel_config/mod.rs` — exercises T057 and Phase 3's `expand_channels()` (T033)
- [x] T049 [US2] Basic `NoChannels` test: a `.condarc` setting `custom_multichannels: {defaults: []}` (no allow/deny involved) asserts `resolve_channel_config_from` returns `NoChannels`, in `src/channel_config/mod.rs`
- [x] T050 [P] [US2] FR-013 test: after `resolve_channel_config_from` runs against each real file state (populated, rejected, unreadable, expansion-failing), the file's own modification time and contents are unchanged, in `src/channel_config/mod.rs`
- [x] T051 [P] [US2] FR-014 test: a `.condarc` containing an unrecognized, channel-unrelated top-level key resolves exactly as if that key were absent, in `src/channel_config/mod.rs`
- [x] T052 [P] [US2] FR-015 test: two consecutive `resolve_channel_config_from` calls against the same path, with the file's contents changed between calls, produce two different results, in `src/channel_config/mod.rs`

### Implementation for User Story 2

- [x] T053 [US2] Implement `read_condarc(path: &Path) -> Result<String, ReadOutcome>` in `src/channel_config/locate.rs`: `io::ErrorKind::NotFound` -> `Err(ReadOutcome::Missing)`, any other `io::Error` -> `Err(ReadOutcome::Unreadable(err))`, success -> `Ok(contents)` (FR-009/FR-011) — depends on T011; makes T034 pass
- [x] T054 [US2] Implement `default_condarc_path() -> Option<PathBuf>` in `src/channel_config/mod.rs` via `dirs::home_dir()` joined with `.condarc` (research.md R7); no branching logic, no dedicated test (research.md R8)
- [x] T056 [US2] Define `const CHANNEL_CONFIG_EVENT_SCHEMA_VERSION: &str = "1"` and implement `ChannelConfigFallbackEvent` (`schema_version: &'static str`, `reason: FallbackReason`, `detail: String`, constructed with `schema_version: CHANNEL_CONFIG_EVENT_SCHEMA_VERSION`) and `emit_fallback(reason: FallbackReason, detail: &str)` in `src/channel_config/events.rs`, emitting via `tracing::warn!` through the existing `src/observability.rs` pipeline (FR-011, research.md R9) — depends on T009 (same file, needs the `FallbackReason` enum already defined); makes T036 pass
- [x] T057 [US2] Implement `resolve_channel_config_from(path: Option<&Path>) -> ChannelConfigResolution` in `src/channel_config/mod.rs`: `path` of `None` or a `read_condarc` `Missing` result falls back silently (`fallback: None`, no event); a `read_condarc` `Unreadable` result, a `condarc::parse` rejection, or a `condarc::expand_channels` `Err` all fall back to `condarc::expand_channels(&Config::default())`, unwrapped via `.expect("expand_channels(&Config::default()) is documented to never fail; if it does, the crate's own built-in defaults changed incompatibly")` — this call is guaranteed to succeed per FR-002's defaulting table (data-model.md, research.md R14) and a deliberate panic on that invariant, not a silent `NoChannels`/fabricated reason, is the specified behavior if it ever didn't — then call `emit_fallback` with the matching `FallbackReason` (`Unreadable` vs `Rejected`, the latter also covering an `expand_channels` `Err` per research.md R11) and the per-problem detail text; on any successful `expand_channels()` result (real or fallback), an empty `resolved.channels` maps to `ChannelConfigResolution::NoChannels`, otherwise `Ready { config: resolved, fallback }` (FR-009/FR-010/FR-011/FR-012/FR-017/FR-018/FR-020) — depends on T053, T054, T056; makes T037–T040, T042–T047, T049–T052 pass
- [x] T058 [US2] Implement `pub fn resolve_channel_config() -> ChannelConfigResolution` in `src/channel_config/mod.rs` delegating to `resolve_channel_config_from(default_condarc_path().as_deref())` (FR-009) — depends on T057

**Checkpoint**: `cargo test --all` passes; `allez` never blocks on any `~/.condarc` state, with the fallback condition visible both via observability and in the returned `ChannelConfigResolution`.

---

## Phase 5: User Story 3 - Preserve priority and enforce access-restriction preferences exactly (Priority: P2)

**Goal**: `channel_priority` (including its legacy boolean spellings) survives resolution intact, and `allowlist_channels`/`denylist_channels` are actually applied (deny-first, then allow) to the resolved `channels` list, with a filtering-caused empty result distinguished from an ordinary fallback or error.

**Independent Test**: Resolve a `.condarc` setting each of `channel_priority`'s three string values plus its two legacy boolean spellings, confirming each maps correctly; separately, resolve a `.condarc` setting `allowlist_channels`/`denylist_channels` (including one channel in both) and confirm the deny-list wins and the survivors keep their relative order.

### Tests for User Story 3 (write first — Constitution II)

- [x] T059 [P] [US3] SC-003 scenarios 11–16 (`channel_priority` = `strict`/`flexible`/`disabled`, absent-defaults-to-`flexible`, legacy boolean `true`/`false`) tests in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016 (FR-003); already-correct passthrough from Phase 3/T033, so these pass immediately
- [x] T060 [P] [US3] SC-003 scenarios 17–18 (a `denylist_channels`/`allowlist_channels` entry given as a bare name, requiring FR-001 expansion before it matches and removes/retains the corresponding `channels` entry, with scenario 18 additionally asserting a sibling `channels` entry not in the allow-list is removed) tests in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016; fails until T064/T065 (Implementation subsection below) land
- [x] T061 [US3] User Story 3 Acceptance Scenario 3 test: a `.condarc` setting both `allowlist_channels` and `denylist_channels`, including one channel present in both, asserting the resulting `channels` has every denied entry removed (checked first) and every non-allowed entry removed too, with the both-lists entry specifically absent, in `crates/condarc/tests/expand_channels_scenarios.rs` — depends on T016; fails until T064/T065 land
- [x] T062 [US3] SC-006 test: a `.condarc` whose `channels` is non-empty before filtering but whose `allowlist_channels`/`denylist_channels` remove every entry asserts `resolve_channel_config_from` returns `ChannelConfigResolution::NoChannels` — not `Ready` with an empty channel configuration — in `src/channel_config/mod.rs` (FR-020, research.md R13) — targets behavior delivered by T057, T065; fails until T065 lands
- [x] T063 [US3] Add a sixth SC-001 sample to T048's pass-through test in `src/channel_config/mod.rs`: `channels: [alpha, beta, gamma]`, `channel_alias: https://example.com`, `allowlist_channels: [alpha, beta]`, `denylist_channels: [beta]` — deny-then-allow leaves only `alpha` — asserting `Ready.config` still equals `condarc::expand_channels(&config)`'s own output exactly, now with a non-trivial filtering result — targets behavior delivered by T048, T065; fails until T065 lands
- [x] T076 [US3] Unit tests for `apply_allow_deny()` in `crates/condarc/src/expand_channels.rs`: empty-allowlist passthrough (deny-only filtering), deny-and-allow together, a channel present in both lists removed (deny wins), and survivor order plus a coincidental duplicate (FR-007) both preserved — same file as T064/T065, so not `[P]`; fails until T064 lands

### Implementation for User Story 3

- [x] T064 [US3] Implement `apply_allow_deny(channels: Vec<String>, allowlist: &[String], denylist: &[String]) -> Vec<String>` in `crates/condarc/src/expand_channels.rs`: remove every entry present in `denylist` first, then, only if `allowlist` is non-empty, remove every remaining entry not present in it, preserving survivor order and any coincidental duplicate (FR-019, research.md R12) — makes T076 pass
- [x] T065 [US3] Wire `allowlist_channels`/`denylist_channels` role resolution into `expand_channels()` in `crates/condarc/src/expand_channels.rs`: resolve both roles via the same `resolve_entry` loop and FR-002 defaulting (empty when absent) used for `channels`, then call `apply_allow_deny(channels, &allowlist, &denylist)` before constructing the returned `ResolvedChannels` (FR-004/FR-019) — depends on T064, T033; makes T060, T061, T062, T063 pass

**Checkpoint**: `cargo test --all` passes; `channel_priority` and allow/deny restrictions are preserved exactly, and a filtering-caused empty result is never mistaken for "nothing was configured."

---

## Phase 6: Retire GEN-24's now-duplicate channel-configuration type (FR-016, research.md R15)

**Purpose**: `condarc::ResolvedChannels` (US1) plus `allez::channel_config`'s file-handling/fallback/`NoChannels` distinction (US2/US3) now fully cover what `allez::ephemeral`'s `ChannelConfig`/`ChannelSpec`/`ChannelPriorityMode` and their compensating behaviors existed to provide. Retire the duplicates and wire GEN-24's environment-creation capability to consume `condarc::ResolvedChannels`/`ChannelPriority` directly. Depends on both US2 (T057/T058) and US3 (T065) being complete — retiring GEN-24's own fallback/filtering is only safe once `expand_channels()` already performs both.

- [x] T077 [P] Add `ResolvedChannels::from_channels(channels: Vec<String>) -> Self` (strict priority) in `crates/condarc/src/expand_channels.rs`, mirroring the retired `ChannelConfig::from_urls`'s own behavior exactly (data-model.md, research.md R15) — needed so external callers (`allez`'s own tests, `examples/ephemeral_smoke.rs`) can construct a `ResolvedChannels` directly despite `#[non_exhaustive]`
- [x] T078 Remove `ChannelConfig`, `ChannelSpec`, `ChannelPriorityMode`, `filter_channels`, and `channels_with_fallback` from `src/ephemeral/channels.rs`; simplify `resolve_channel_source` to a passthrough (every identifier `expand_channels()` produces is already a fully-qualified URL, so the literal `"defaults"` name never reaches this function any more) and remove the now-dead `DEFAULTS_CHANNEL_NAME`/`DEFAULTS_CHANNEL_URL` constants along with it (both are used only by the functions this task removes); add `redact_channel_url` to `src/ephemeral/mod.rs`'s existing `pub use channels::{...};` re-export list, removing the three retired types from that same list — depends on T065, T077
- [x] T079 [P] Rewrite `channel_config_from_urls_defaults_policy` in `channels.rs`'s own test module to assert against `condarc::ResolvedChannels::from_channels` instead — depends on T078
- [x] T080 [P] Remove `channel_spec_debug_redacts_credentials` and `channel_config_debug_redacts_credentials_in_all_channel_fields` from `channels.rs`'s own test module (the types they test no longer exist); `redact_channel_url_removes_userinfo_and_conda_tokens_and_preserves_clean_values` is unaffected and stays — depends on T078
- [x] T081 [P] Remove `channel_fallback_substitutes_defaults_only_when_empty` from `channels.rs`'s own test module (`channels_with_fallback` no longer exists); its coverage is superseded by `NoChannels` (FR-020, already tested by T049/T062) — depends on T078
- [x] T082 [P] Remove `filter_channels_denies_before_applying_allowlist` and `filter_channels_without_allowlist_only_applies_denials` from `channels.rs`'s own test module (`filter_channels` no longer exists); their coverage is superseded by `apply_allow_deny`'s own crate-level tests (T076) — depends on T078
- [x] T083 Update `src/ephemeral/mod.rs`'s `create_ephemeral_environment(requested, channels: condarc::ResolvedChannels, default_override)` signature and `src/ephemeral/solve.rs`'s `solve_packages(root, config: &condarc::ResolvedChannels, packages)` to take `condarc::ResolvedChannels` directly; `solve_packages` uses `config.channels` as-is (already deny-then-allow filtered, FR-019) instead of calling `channels_with_fallback`/`filter_channels`, erroring `EphemeralEnvError::NoChannelsConfigured` only when it receives an empty `channels` list directly; also migrate `solve.rs`'s private `solver_priority(priority: ChannelPriorityMode) -> rattler_solve::ChannelPriority` helper to take `condarc::ChannelPriority` instead, preserving the existing `Strict -> Strict`/`{Flexible, Disabled} -> Disabled` mapping with a wildcard arm covering any future `#[non_exhaustive]` variant the same way `Flexible` is covered today — depends on T078
- [x] T084 [P] Update `solve.rs`'s own test module and `install.rs`'s/`examples/ephemeral_smoke.rs`'s test/example fixtures: replace every `ChannelConfig`/`ChannelPriorityMode`/`ChannelConfig::from_urls` reference with `condarc::ResolvedChannels`/`condarc::ChannelPriority`/`ResolvedChannels::from_channels` — depends on T083, T077
- [x] T086 [P] Update every `tests/support/*.rs` fixture that constructs or matches on `ChannelConfig`/`ChannelSpec`/`ChannelPriorityMode` (`ephemeral.rs`, `creation.rs`, `failures.rs`, `defaults.rs`) to build `condarc::ResolvedChannels` (via `ResolvedChannels::from_channels` or a struct literal) instead — these back the `tests/ephemeral_env.rs`/`tests/cli_scaffold.rs` integration suites, which do not compile once T078/T083 land without this migration — depends on T083, T077
- [x] T087 Run `cargo test --lib ephemeral::` and `cargo test --all` and confirm zero regressions beyond the deliberate rewrites above — depends on T077, T078, T079, T080, T081, T082, T083, T084, T086

**Checkpoint**: `cargo test --all` passes; `allez::ephemeral` no longer defines `ChannelConfig`/`ChannelSpec`/`ChannelPriorityMode`/`filter_channels`/`channels_with_fallback`; `create_ephemeral_environment` takes `condarc::ResolvedChannels` directly.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Manual/developer-facing artifacts and the quality gates every change in this workspace requires.

- [x] T066 [P] Add `examples/channel_config_smoke.rs` per quickstart.md: calls `allez::channel_config::resolve_channel_config()`, prints `fallback`/`channel_priority`/`channels` (each passed through `redact_channel_url` explicitly) for `Ready`, a distinct message for `NoChannels`, and a wildcard arm for `ChannelConfigResolution`'s `#[non_exhaustive]` future variants — depends on T078 (needs `redact_channel_url` re-exported from `src/ephemeral/mod.rs`)
- [x] T068 Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo audit`, `cargo deny check`, and `cargo doc --no-deps` against this ticket's changed files; fix any warning introduced by this ticket
- [x] T069 Run `cargo test --all` and confirm every test this ticket added passes, with zero pre-existing regressions
- [x] T070 Manually run `cargo run --example channel_config_smoke` against the current machine's real (or absent) `~/.condarc` and confirm the printed output matches quickstart.md's documented expected outcome

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately, all three tasks in parallel.
- **Foundational (Phase 2)**: Depends on Setup completion — BLOCKS all user stories.
- **User Story 1 (Phase 3)**: Depends on Foundational completion. No dependency on US2/US3.
- **User Story 2 (Phase 4)**: Depends on Foundational **and** User Story 1 (`resolve_channel_config_from` calls the real `condarc::expand_channels`, which must already resolve `Config::default()` correctly).
- **User Story 3 (Phase 5)**: Depends on Foundational, User Story 1 (extends `expand_channels()`), and User Story 2 (adds SC-006's test coverage against `resolve_channel_config_from`'s already-generic `NoChannels` check, for the allow/deny-caused case).
- **GEN-24 Retirement (Phase 6)**: Depends on User Story 2 **and** User Story 3 — retiring GEN-24's own fallback/filtering is only safe once `expand_channels()` already performs both.
- **Polish (Phase 7)**: Depends on the Retirement phase being complete (T066/T070 need `redact_channel_url` re-exported by T078).

### User Story Dependencies

- **User Story 1 (P1)**: No dependency on US2/US3 — independently testable via `condarc::expand_channels()` alone.
- **User Story 2 (P1)**: Structurally depends on US1's `expand_channels()` (it is the real function `resolve_channel_config_from` calls), but is independently *testable* once US1 is done — its own acceptance scenarios (missing/rejected/unreadable) do not require US3's allow/deny filtering.
- **User Story 3 (P2)**: Depends on both US1 (extends the same `expand_channels()` function) and US2 (exercises the same `resolve_channel_config_from` function for SC-006's `NoChannels` case, without modifying it).

### Within Each User Story

- Test tasks before the implementation tasks that make them pass (Constitution II Red-Green-Refactor) — each phase's Tests subsection precedes its Implementation subsection.
- Tasks touching the same file are sequential; tasks touching different files with no unmet dependency are marked `[P]`.

### Parallel Opportunities

- All Setup tasks (`[P]`) run in parallel.
- Within Foundational: T009 and T011 run in parallel with each other and with the T004→T008 chain.
- Within User Story 1: T013/T014/T015 (helper unit tests) run in parallel; T017–T029 and T071–T073 (scenario tests, all in `expand_channels_scenarios.rs`) run in parallel once T016 lands, but merge sequentially into that one file.
- Within User Story 2: T034/T036 run in parallel; T037–T040, T042–T047, and T050–T052 (all in `src/channel_config/mod.rs`) are independent test cases authored in parallel, merged sequentially into that one file.
- Within User Story 3: T059/T060 run in parallel; T076 is sequential with T064/T065, same file.
- Within GEN-24 Retirement: T077 runs in parallel with T078's own prerequisites; T079/T080/T081/T082 (test-module rewrites, all in `channels.rs`) run in parallel once T078 lands, but merge sequentially into that one file; T084 and T086 run in parallel with each other once T083 lands (different files); T087 runs last, after every other Phase 6 task.

---

## Parallel Example: User Story 1

```bash
# Write these test-authoring tasks together, before any of the helpers they target exist:
Task: "Unit tests for match_custom_channel() in crates/condarc/src/expand_channels.rs"
Task: "Unit tests for resolve_member() in crates/condarc/src/expand_channels.rs"
Task: "Unit tests for resolve_entry() in crates/condarc/src/expand_channels.rs"

# Once T016 (scenarios file scaffold) lands, launch these together:
Task: "SC-003 scenario 1 (bare-name/alias-only) in expand_channels_scenarios.rs"
Task: "SC-003 scenario 2 (custom_channels exact-match) in expand_channels_scenarios.rs"
Task: "SC-003 scenario 3 (custom_channels progressive-prefix) in expand_channels_scenarios.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories)
3. Complete Phase 3: User Story 1 (Tests subsection, then Implementation subsection)
4. **STOP and VALIDATE**: `cargo test -p condarc` — `expand_channels()` correctly resolves every FR-001/FR-002 case, independently of `allez`
5. This alone is not yet usable by GEN-25 (no file-handling layer exists) — proceed to US2 for that.

### Incremental Delivery

1. Setup + Foundational → foundation ready, both modules compile as stubs.
2. Add User Story 1 → `condarc::expand_channels()` fully correct and tested → crate-level value delivered.
3. Add User Story 2 → `allez::channel_config::resolve_channel_config()` never blocks, fallback visible → GEN-25 can now wire this ticket's output into `allez oneshot`.
4. Add User Story 3 → `channel_priority`/allow-deny preserved exactly, `NoChannels` never mistaken for "nothing configured" → full spec coverage complete.
5. Retire GEN-24's now-duplicate `ChannelConfig`/`ChannelSpec`/`ChannelPriorityMode` and their compensating behaviors (FR-016) → `create_ephemeral_environment` consumes `condarc::ResolvedChannels` directly.
6. Polish → manual smoke example, quality gates.

### Sequential Strategy (replaces Parallel Team Strategy)

Given US2 structurally depends on US1's `expand_channels()` being real (not a stub), US3 extends both US1's and US2's same functions, and the Retirement phase depends on both US2 and US3 being complete, this ticket is best executed **sequentially** (Setup → Foundational → US1 → US2 → US3 → Retirement → Polish) rather than with parallel-team story ownership.

---

## Notes

- `[P]` tasks = different files, no dependencies on incomplete tasks.
- `[Story]` label maps task to specific user story for traceability.
- Tests are mandatory in this ticket (Constitution VIII) and precede their implementation (Constitution II) except where a task's own text documents a technical reason otherwise — every SC-00n and FR-0nn above has at least one dedicated task.
- Commit after each task or logical group (per this project's own git conventions).
- Stop at any checkpoint to run `cargo test --all` and validate the story independently.
- Avoid: vague tasks, same-file conflicts marked `[P]`, cross-story dependencies not called out explicitly above.
