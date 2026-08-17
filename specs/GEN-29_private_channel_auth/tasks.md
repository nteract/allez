---

description: "Task list template for feature implementation"
---

# Tasks: Private Channel Authentication

**Input**: Design documents from `/specs/GEN-29_private_channel_auth/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/channel_auth_api.md, quickstart.md

**Tests**: Included. `plan.md`'s Testing section and `data-model.md`'s spec-coverage matrix (Constitution VIII) require a `wiremock`-backed test for every FR/acceptance scenario, plus pure unit tests for every classification/redaction function — tests are not optional for this feature.

**Organization**: Tasks are grouped by user story (spec.md's US1/US2/US3) to enable independent implementation and testing of each story, with a Setup and Foundational phase first.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)
- File paths are exact, taken from plan.md's Project Structure and data-model.md's module surface

## Path Conventions

Single Rust workspace (`allez` crate + `crates/condarc`), unchanged. All paths below are relative to the repository root.

---

## Phase 1: Setup

**Purpose**: Add the three new direct dependencies and the one new dev-dependency this feature needs; confirm they resolve before any code changes.

- [X] T001 Add `reqwest-middleware = { package = "astral-reqwest-middleware", version = "0.5.1" }`, `async-trait = "0.1.91"`, and `http = "1.4.2"` to `[dependencies]`, and `wiremock = "0.6"` to `[dev-dependencies]`, in `Cargo.toml` (research.md Decision 2 / Summary of new dependencies — `reqwest-middleware`, `async-trait`, and `http` already resolve transitively at these exact versions, so promoting them to direct dependencies changes no supply-chain exposure; `wiremock` is a genuinely new dev-only addition, covered by T042's audit/deny check)
- [X] T002 Run `cargo build --workspace` and `cargo test --all --no-run` to confirm the new dependencies resolve cleanly against the existing `Cargo.lock` with no compile errors introduced

**Checkpoint**: Dependencies resolve; no code changed yet.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Thread `channel_settings` through `condarc`, extend redaction, add the two new error variants, and register the new module — every user story below depends on all of this.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [X] T003 [P] In `crates/condarc/src/model.rs`, add a derived `Eq` to `ChannelSetting` (alongside its existing `Debug, Clone, PartialEq, Default`) — needed because `ResolvedChannels` itself derives `Eq` and cannot hold a `Vec<ChannelSetting>` unless the element type also satisfies `Eq` (research.md Decision 3)
- [X] T004 In `crates/condarc/src/expand_channels.rs`, add a `channel_settings: Vec<condarc::ChannelSetting>` field to `ResolvedChannels`, with a `///` doc comment describing it as a direct pass-through of `Config.channel_settings` (Constitution VI), populated in `expand_channels()` as `config.channel_settings.clone().unwrap_or_default()` with no interpretation, and defaulted to an empty `Vec` in `ResolvedChannels::from_channels` (data-model.md §1; depends on T003 for the `Eq` bound to compile)
- [X] T004a In `crates/condarc/src/expand_channels.rs`, add a unit test constructing a `Config` with both `channels` and `channel_settings` populated and asserting `expand_channels()`'s returned `ResolvedChannels.channel_settings` equals the input unchanged — proving the pass-through FR-003 depends on actually holds at the `condarc`/`allez` integration seam, not only when `ChannelSetting` values are constructed directly in `allez`-side tests (depends on T004)
- [X] T005 In `src/ephemeral/channels.rs`, extend `redact_channel_url` to also strip a URL's query string and fragment, in addition to its existing userinfo and `/t/<token>/` path-segment stripping (research.md Decision 6)
- [X] T006 In `src/ephemeral/channels.rs`, add a unit test asserting `redact_channel_url` strips a credential placed in a URL's query string or fragment, alongside the existing userinfo/`/t/token/` cases (depends on T005)
- [X] T007 In `src/ephemeral/error.rs`, add two new `#[non_exhaustive]` `EphemeralEnvError` variants, each with a `///` doc comment describing its meaning (Constitution VI) — `MissingChannelToken` (no payload) and `ChannelAuthenticationFailed { channel: String }`, with matching arms added to `fmt::Debug`, `fmt::Display` (exact wording from data-model.md's table), and `CategorizedError::category()` (`"missing_channel_token"` / `"channel_authentication_failed"`)
- [X] T008 In `src/ephemeral/error.rs`, add unit tests asserting `CategorizedError::category()` returns `"missing_channel_token"` and `"channel_authentication_failed"` for the two new variants, and that their `Display` text matches data-model.md's table exactly (depends on T007)
- [X] T009 [P] In `src/ephemeral/mod.rs`, add a `mod channel_auth;` declaration alongside the existing module list
- [X] T010 [P] In `tests/support/ephemeral.rs`, add a `wiremock`-backed fixture helper: starts a mock server, builds a `condarc::ChannelSetting` entry (`channel` + `auth` keys) matching it, and a guard that sets/restores `ALLEZ_CHANNEL_TOKEN` under this workspace's existing `serial_test` discipline (mirrors data-model.md's test-coverage-matrix note on environment mutation safety)

**Checkpoint**: `condarc` threads `channel_settings` through; redaction is extended; both new error variants exist and categorize correctly; test infrastructure is ready. User story implementation can now begin.

---

## Phase 3: User Story 1 - Install from a private channel using an already-present token (Priority: P1) 🎯 MVP

**Goal**: A configured private channel with a valid token in the environment resolves and installs packages exactly as if it were public, carrying the token on every request to it; a public channel configured alongside it is never touched; a channel with no `channel_settings` entry is never treated as private regardless of its URL; and the token itself is never cached or persisted beyond what FR-007 permits.

**Independent Test**: Configure a private channel (via `wiremock` + a matching `channel_settings`/`auth` entry), set `ALLEZ_CHANNEL_TOKEN`, request a package that only exists there, and confirm it resolves/installs successfully with the token attached — independent of any failure-path behavior.

### Tests for User Story 1

- [X] T011 [US1] In `src/ephemeral/channel_auth.rs` (new file), add unit tests for `classify_private_channels`: an entry with `channel` exact-matching a configured channel and an `auth` key classifies private regardless of URL scheme; an entry with `channel` matching but no `auth` key classifies public; an entry whose `channel` ends `/*` matches `prefix/sub/path` but not `prefixed-differently`; a channel absent from `channel_settings` always classifies public (data-model.md's test-coverage matrix, FR-003). Also add unit tests for `matching_private_channel`: a private base URL matches its own repodata/package descendant paths (e.g. `/org/noarch/repodata.json`, `/org/pkg.tar.bz2`); it rejects a sibling prefix sharing only a string prefix without the path boundary (`/org` vs. `/organization/...`), a different scheme, a different host, and a different port (data-model.md's test-coverage matrix, FR-002/FR-005).
- [X] T012 [US1] Create `tests/support/private_channel_auth.rs` and register it in `tests/ephemeral_env.rs` as `#[path = "support/private_channel_auth.rs"] mod private_channel_auth;`, matching the existing `creation`/`defaults`/`failures` submodule pattern (`tests/ephemeral_env.rs` itself holds only `mod` declarations, no test bodies); in it, add an integration test for spec.md US1 Acceptance Scenario 1: a `wiremock` server configured private (via T010's helper) plus `ALLEZ_CHANNEL_TOKEN` set — assert the server received an `Authorization` header equal to the raw environment value, and the package resolves and installs
- [X] T013 [US1] In `tests/support/private_channel_auth.rs`, add an integration test for spec.md US1 Acceptance Scenario 2: one `wiremock` server configured private and a second with no `channel_settings` entry, both requested — assert only the first's captured requests carry `Authorization`; the second's carry none
- [X] T014 [US1] In `tests/support/private_channel_auth.rs`, add an integration test for the spec.md Edge Case / quickstart Scenario 5: a `wiremock` server with no `channel_settings` entry at all and `ALLEZ_CHANNEL_TOKEN` unset — assert the environment creates successfully, no request carries `Authorization`, and no `MissingChannelToken` error occurs
- [X] T015 [US1] In `src/ephemeral/channel_auth.rs`, add a unit test asserting `read_channel_token_header` reflects the current environment value on every call — set `ALLEZ_CHANNEL_TOKEN` to one value, call it, change the variable to a second value, call it again, and assert the second call's result reflects the new value — proving no internal caching across repeated calls in one process (FR-007, data-model.md's updated FR-007 coverage row). Guard with `#[serial_test::serial]` and restore the variable's prior state on completion, per data-model.md's test-coverage-matrix environment-mutation discipline.
- [X] T015a [US1] In `src/ephemeral/channel_auth.rs`, add a unit test asserting `read_channel_token_header` returns `None` when `ALLEZ_CHANNEL_TOKEN` is set to a value `http::HeaderValue::from_str` rejects (for example, one containing a bare `\r` or `\n` byte) — covering FR-004's "not representable as an HTTP header value" case, distinct from the unset/empty cases T023/T024 cover. Guard with `#[serial_test::serial]` and restore the variable's prior state on completion.
- [X] T016 [US1] In `tests/support/private_channel_auth.rs`, add an integration test that, after a successful private-channel install (reusing T012's fixture), recursively scans every file under the environment root (`ALLEZ_EPHEMERAL_ROOT`, including its cache and prefix subdirectories) for the literal token value and asserts it appears in none of them — proving the token is never persisted to disk (FR-007, data-model.md's updated FR-007 coverage row)

### Implementation for User Story 1

- [X] T017 [US1] In `src/ephemeral/channel_auth.rs`, define the `CHANNEL_TOKEN_ENV_VAR = "ALLEZ_CHANNEL_TOKEN"` constant and implement `classify_private_channels(channels: &[String], channel_settings: &[condarc::ChannelSetting]) -> Vec<String>` and `matching_private_channel(url: &reqwest::Url, private_channels: &[String]) -> Option<&str>` per the exact boundary rules in contracts/channel_auth_api.md's Classification contract (depends on T011 to define the target behavior)
- [X] T018 [US1] In `src/ephemeral/channel_auth.rs`, implement `read_channel_token_header() -> Option<http::HeaderValue>`, reading `CHANNEL_TOKEN_ENV_VAR`, treating unset/empty/non-header-safe values as `None`, and never retaining the intermediate `String` beyond this one call (depends on T017, same file; must satisfy T015's no-caching test)
- [X] T019 [US1] In `src/ephemeral/channel_auth.rs`, implement the private `PrivateChannelAuthMiddleware` struct (`private_channels: Vec<String>`, `token_header: http::HeaderValue` marked `.set_sensitive(true)`) and its `#[async_trait::async_trait] impl reqwest_middleware::Middleware`, whose `handle()` attaches `token_header` to the `Authorization` header only when `matching_private_channel` returns `Some`, forwarding every other request unmodified (depends on T018)
- [X] T020 [US1] In `src/ephemeral/channel_auth.rs`, relocate the `HTTP_USER_AGENT` constant from `src/ephemeral/solve.rs` (its only remaining call site once this task lands) and implement `build_channel_auth_client(private_channels: Vec<String>) -> Result<reqwest_middleware::ClientWithMiddleware, EphemeralEnvError>`: wraps the base `reqwest::Client` (`.no_proxy().user_agent(HTTP_USER_AGENT)`) with `PrivateChannelAuthMiddleware` only when `private_channels` is non-empty, returning `Err(EphemeralEnvError::MissingChannelToken)` when it is non-empty and `read_channel_token_header()` returns `None` (depends on T019; `HTTP_USER_AGENT` was private to `solve.rs` and a sibling module cannot reference it, so relocation, not cross-module visibility, is the fix)
- [X] T021 [US1] In `src/ephemeral/solve.rs`, change `SolvedPackages.client` from `reqwest::Client` to `reqwest_middleware::ClientWithMiddleware`, add a `private_channels: Vec<String>` field, and wire `channel_auth::classify_private_channels` + `channel_auth::build_channel_auth_client` into `solve_packages` in place of the current plain `reqwest::Client::builder()...build()` call and `Gateway::builder().with_client(...)` (depends on T020)
- [X] T022 [US1] In `src/ephemeral/install.rs`, update `install_packages`'s `Installer::with_download_client(solution.client)` call site for `SolvedPackages.client`'s new `ClientWithMiddleware` type (depends on T021)

**Checkpoint**: User Story 1 is fully functional and testable independently — happy-path private-channel install, public-channel non-interference, the unmarked-channel edge case, and the FR-007 no-caching/no-persistence guarantees all pass.

---

## Phase 4: User Story 2 - Get a clear, actionable error when the token is missing (Priority: P1)

**Goal**: A private channel configured with `ALLEZ_CHANNEL_TOKEN` unset or empty produces `EphemeralEnvError::MissingChannelToken` before any network request, in both the library API and the `allez oneshot --json` error body.

**Independent Test**: Configure a private channel, leave `ALLEZ_CHANNEL_TOKEN` unset (then empty), request a package from it, and confirm `MissingChannelToken` is returned with zero requests made — independent of User Story 1's happy path.

### Tests for User Story 2

- [X] T023 [US2] In `tests/support/private_channel_auth.rs`, add an integration test for spec.md US2 Acceptance Scenario 1 / quickstart Scenario 3: a private channel configured, `ALLEZ_CHANNEL_TOKEN` unset — assert `Err(CreationFailure { error: EphemeralEnvError::MissingChannelToken, .. })` and that the mock server recorded zero requests
- [X] T024 [US2] In `tests/support/private_channel_auth.rs`, add an integration test for spec.md US2 Acceptance Scenario 2: the same configuration with `ALLEZ_CHANNEL_TOKEN` set to an empty string — assert the identical `MissingChannelToken` error and zero requests
- [X] T025 [P] [US2] In `tests/oneshot_exec.rs`, add an end-to-end test running `allez oneshot --json <fixture-package>` against a private channel with `ALLEZ_CHANNEL_TOKEN` unset — assert the JSON error body's `category` is `"missing_channel_token"`, its `message` names `ALLEZ_CHANNEL_TOKEN`, and the exit code is `2` (quickstart Scenario 3, step 3)

### Implementation for User Story 2

- [X] T026 [US2] In `src/ephemeral/solve.rs`, confirm/adjust `solve_packages` so the `classify_private_channels` → `build_channel_auth_client` check (T021) runs synchronously at the top of the function, before the `Gateway` or any HTTP client is constructed, so `MissingChannelToken` is always returned before any network request per research.md Decision 5 (depends on T021; likely a no-op confirmation if T021 already places the check first, otherwise a reordering fix)

**Checkpoint**: User Story 2 is fully functional and testable independently, without depending on User Story 3's failure-categorization work.

---

## Phase 5: User Story 3 - Distinguish a rejected credential from a nonexistent package (Priority: P2)

**Goal**: An HTTP 401/403 from a channel this feature attached a credential to is reported as `EphemeralEnvError::ChannelAuthenticationFailed { channel }` (origin only, no path) — distinct from `UnresolvablePackage`/`ResolutionFailed` — at both the repodata-resolution and package-download HTTP call sites; a public channel's own 401/403 is never mislabeled this way.

**Independent Test**: Configure a private channel, script `wiremock` to reject a request with 401 (then 403), request a package from it, and confirm the failure is `ChannelAuthenticationFailed` rather than a generic resolution failure — independent of User Stories 1 and 2.

### Tests for User Story 3

- [X] T027 [US3] In `src/ephemeral/channel_auth.rs`, add unit tests for `reqwest_http_failure`: a `reqwest::Error` at the top level of `error` is found directly; one nested only in `error.source()` is found by walking the chain; an error with no `reqwest::Error` anywhere in its chain returns `None` (research.md Decision 4)
- [X] T028 [US3] In `src/ephemeral/channel_auth.rs`, add unit tests for `origin_only`: a channel URL with a path, query string, and fragment reduces to `scheme://host[:port]` only; an unparseable or opaque-origin input returns the fixed `"<unparseable channel>"` constant, never a substring of the input (research.md Decision 4, data-model.md's redaction test row)
- [X] T029 [US3] In `tests/support/private_channel_auth.rs`, add an integration test for spec.md US3 Acceptance Scenario 1 / quickstart Scenario 4.1: a private channel's repodata request rejected with 401 — assert `Err(EphemeralEnvError::ChannelAuthenticationFailed { channel })` with `channel` equal to the channel's origin; repeat with 403
- [X] T030 [US3] In `tests/support/private_channel_auth.rs`, add an integration test for quickstart Scenario 4.2: a private channel serving valid repodata but rejecting the package-archive download with 401 — assert the same `ChannelAuthenticationFailed` category surfaces through the install path
- [X] T031 [US3] In `tests/support/private_channel_auth.rs`, add an integration test for the FR-005 correlation negative case / quickstart Scenario 4.3: a **public** channel (no matching `channel_settings` entry) rejecting a request with 401 — assert the failure stays `ResolutionFailed`/`UnresolvablePackage`, never `ChannelAuthenticationFailed`

### Implementation for User Story 3

- [X] T032 [US3] In `src/ephemeral/channel_auth.rs`, implement the `HttpFailure { status: reqwest::StatusCode, url: Option<reqwest::Url> }` struct and `reqwest_http_failure(error: &(dyn std::error::Error + 'static)) -> Option<HttpFailure>`, checking `error` itself before walking `error.source()` (depends on T027 to define target behavior; T020 for module context)
- [X] T033 [US3] In `src/ephemeral/channel_auth.rs`, implement `origin_only(channel: &str) -> String`, parsing `channel` as a URL and returning `.origin().ascii_serialization()` only when that origin is a tuple origin (`Origin::is_tuple()`), reducing it to `scheme://host[:port]`; returning the fixed `"<unparseable channel>"` constant — never `Origin::Opaque`'s own `"null"` serialization — on any parse failure or opaque origin (depends on T032, same file)
- [X] T034 [US3] In `src/ephemeral/solve.rs`, map `gateway.query(...).execute().await`'s error through `channel_auth::reqwest_http_failure` + `channel_auth::matching_private_channel(url, &private_channels)`: on a 401/403 matching a private channel, return `EphemeralEnvError::ChannelAuthenticationFailed { channel: channel_auth::origin_only(matched) }`; every other outcome keeps today's `ResolutionFailed` (depends on T032, T033, T021)
- [X] T035 [US3] In `src/ephemeral/install.rs`, in the `InstallerError::FailedToFetch(identifier, source)` arm, try the same `reqwest_http_failure` + `matching_private_channel(&solution.private_channels)` check before falling through to the existing `"hash mismatch"` cause walk, mapping a match to `ChannelAuthenticationFailed { channel }` (depends on T032, T033, T022) — `PackageCacheError`/`PackageCacheLayerError`'s `#[error(transparent)]` variants make `Error::source()` skip past the boxed/`Arc`'d `ExtractError` value itself, so `reqwest_http_failure`'s generic chain walk cannot reach it; a dedicated `channel_auth::package_cache_http_failure` downcasts through `PackageCacheError → PackageCacheLayerError → rattler_package_streaming::ExtractError` directly instead, requiring `rattler_package_streaming` as a new direct dependency (already resolves transitively at this exact pinned version via `rattler_cache`/`rattler`, so this changes no supply-chain exposure — same precedent as T001's `http` addition)

**Checkpoint**: User Story 3 is fully functional and testable independently; all three user stories now work together without regressing one another.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Lock in FR-006's redaction guarantee end-to-end, validate the whole feature against quickstart.md, and pass every constitution-mandated quality gate before merge.

- [X] T036 [P] In `src/ephemeral/channel_auth.rs`, add a unit test constructing a real `reqwest::Request` carrying the middleware's injected header and asserting `format!("{request:?}")` does not contain the literal token value (data-model.md's redaction test row; confirms `set_sensitive` redacts `Request`'s own `Debug` impl for this exact `reqwest`/`http` version pairing)
- [X] T037 [P] In `tests/support/private_channel_auth.rs`, add an end-to-end test for quickstart Scenario 6: capture `RUST_LOG=allez=trace` stderr in both the JSON and human log-formatter modes during a 401 rejection, asserting neither capture contains the token value — implemented in `tests/oneshot_exec.rs` instead (the real-binary end-to-end test file, matching T025's own precedent), since exercising the JSON/`--human` formatter split requires the compiled `allez` binary rather than `create_ephemeral_environment`'s library-level API
- [X] T038 Run every scenario in `specs/GEN-29_private_channel_auth/quickstart.md` end to end (via the test suite added above, or manually per its steps) and confirm all six pass
- [X] T039 Run `cargo test --all --features test-config-override`; confirm the full suite is clean with no pre-existing failures newly introduced
- [X] T040 Run `cargo fmt --check`; confirm clean (constitution.md Quality Gates)
- [X] T041 Run `cargo clippy --workspace --all-targets -- -D warnings`; confirm clean (constitution.md Quality Gates — `--all-targets`, covering tests/benches/examples in addition to library code)
- [X] T042 Run `cargo audit` and `cargo deny check`; confirm both clean, with no newly introduced vulnerable, duplicate, or non-compliant-license dependencies from T001's additions (constitution.md Quality Gates / Principle X) — also covers `rattler_package_streaming`, promoted to a direct dependency during T035 (see that task's note)
- [X] T043 Run `cargo doc --workspace --no-deps`; confirm zero warnings, including on the new `channel_settings` field, `ChannelSetting`'s derive, and the two new `EphemeralEnvError` variants' doc comments (constitution.md Quality Gates / Principle VI)
- [X] T044 Run `cargo semver-checks check-release -p condarc`; confirm the new `ResolvedChannels.channel_settings` field and `ChannelSetting`'s derived `Eq` are correctly classified (additive/non-breaking, consistent with both types already being `#[non_exhaustive]`) per constitution.md Governance's SemVer-evaluation requirement for public crate API changes — 196 checks, 0 breaking changes, confirmed via `--baseline-rev HEAD` (the crate is unpublished, so no crates.io baseline exists)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately.
- **Foundational (Phase 2)**: Depends on Setup (T001's dependency additions) — BLOCKS all user stories.
- **User Story 1 (Phase 3)**: Depends on Foundational completion. No dependency on US2/US3.
- **User Story 2 (Phase 4)**: Depends on Foundational completion, and on US1's `build_channel_auth_client`/`solve_packages` wiring (T020–T021) already existing, since `MissingChannelToken` is raised by that same shared code path — but its own tests and checkpoint are independently verifiable without any US3 work.
- **User Story 3 (Phase 5)**: Depends on Foundational completion, and on US1's `SolvedPackages.client`/`private_channels` plumbing (T021–T022) — independently verifiable without any US2 work.
- **Polish (Phase 6)**: Depends on US1 (the middleware) and US3 (the 401 scenario) being complete. T040–T044 (quality gates) additionally depend on every prior phase's code changes being in place, since they run against the whole workspace.

### User Story Dependencies

- **User Story 1 (P1)**: Foundational only. This is the mechanism (`channel_auth` module, client wiring) every other story builds on, and the only story whose own tests cover FR-007 (no caching/no persistence) directly.
- **User Story 2 (P1)**: Foundational + reuses US1's `build_channel_auth_client`/`solve_packages` wiring (same shared function already returns `MissingChannelToken`); adds only its own tests plus a placement check.
- **User Story 3 (P2)**: Foundational + reuses US1's `SolvedPackages.private_channels` field; adds its own new functions (`reqwest_http_failure`, `origin_only`) and their two call sites.

### Within Each User Story

- Tests are written first per story (they will fail to compile/pass until the story's Implementation tasks land).
- Pure functions (`classify_private_channels`, `reqwest_http_failure`, `origin_only`) before the code that calls them.
- `channel_auth.rs` functions before `solve.rs`/`install.rs` wiring that calls them.
- `solve.rs` changes before the corresponding `install.rs` changes (install consumes `SolvedPackages`).

### Parallel Opportunities

- T001–T002 (Setup) are sequential (build must follow the dependency edit).
- T003, T009, T010 (Foundational) can run in parallel with each other; T004 depends on T003; T004a depends on T004; T005/T006 and T007/T008 are each sequential same-file pairs but independent of T003/T004/T009/T010.
- US1's seven test tasks (T011–T016, T015a) are independent test cases with no ordering dependency on each other, but all of T011/T015/T015a share `src/ephemeral/channel_auth.rs` and all of T012–T014/T016 share `tests/support/private_channel_auth.rs`; within each shared file, add the test functions sequentially rather than in parallel.
- T025 (US2, `tests/oneshot_exec.rs`) can run in parallel with T023/T024; T023/T024 themselves share `tests/support/private_channel_auth.rs` with each other and with US1's/US3's integration tests, so add them sequentially within that file.
- US3's five test tasks (T027–T031) are independent test cases, but T027/T028 share `src/ephemeral/channel_auth.rs` and T029/T030/T031 share `tests/support/private_channel_auth.rs`; same same-file coordination note as above applies.
- T036 (`src/ephemeral/channel_auth.rs`) and T037 (`tests/support/private_channel_auth.rs`) touch different files and can run in parallel with each other.
- T040–T044 (quality-gate commands) can all run in parallel with each other once T039 confirms the test suite itself is green.
- `channel_auth.rs` implementation tasks within one story (T017–T020, T032–T033) are additive to the same new file and are listed in the required sequential order.

---

## Sequencing Example: User Story 1 Tests

```bash
# These seven US1 test tasks are independent test cases; add them in this
# order within their two shared files (channel_auth.rs, then
# private_channel_auth.rs) rather than as unconditional [P] parallel work:
Task: "Unit tests for classify_private_channels and matching_private_channel in src/ephemeral/channel_auth.rs"
Task: "Unit test: read_channel_token_header has no internal caching in src/ephemeral/channel_auth.rs"
Task: "Unit test: read_channel_token_header rejects a non-header-safe value in src/ephemeral/channel_auth.rs"
Task: "Integration test: happy path, Authorization header equals raw token in tests/support/private_channel_auth.rs"
Task: "Integration test: private channel gets header, public channel doesn't in tests/support/private_channel_auth.rs"
Task: "Integration test: unmarked channel never becomes private in tests/support/private_channel_auth.rs"
Task: "Integration test: token never persisted to disk in tests/support/private_channel_auth.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup.
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories).
3. Complete Phase 3: User Story 1.
4. **STOP and VALIDATE**: run T012–T016's and T015a's integration/unit tests independently; confirm the happy path, public-channel non-interference, the unmarked-channel edge case, and the FR-007 no-caching/no-persistence guarantees all pass.
5. This alone satisfies SC-001 (and FR-007's coverage).

### Incremental Delivery

1. Setup + Foundational → foundation ready.
2. Add User Story 1 → validate independently (MVP — SC-001, FR-007).
3. Add User Story 2 → validate independently (SC-002).
4. Add User Story 3 → validate independently (SC-003).
5. Polish (redaction end-to-end, quickstart, full quality-gate suite) → SC-004 and final sign-off.

### Notes

- [P] tasks touch different files, or are independent test functions with no ordering dependency on each other.
- [Story] label maps each Phase 3–5 task to spec.md's US1/US2/US3 for traceability.
- Every test task cites the exact spec.md Acceptance Scenario, Edge Case, FR, or quickstart.md Scenario it locks in.
- No CLI flag surface changes anywhere in this feature (plan.md) — T025 is the only CLI-facing test, and it exercises the existing `--json` error-rendering path unchanged.
- Constitution VIII (Mandatory 100% Spec Test Coverage): every FR and acceptance scenario in spec.md has at least one task above that tests it directly, including FR-004's unusable-value case (T015a) and FR-007 (T015, T016) — cross-reference data-model.md's own coverage table to confirm none were dropped.
- Constitution's Quality Gates (fmt, clippy `--all-targets`, audit, deny, full test suite, `cargo doc`) are each their own task (T039–T043) rather than folded into a single catch-all, so a failure in one gate doesn't mask the others.
- Constitution Governance's SemVer-evaluation requirement for public crate API changes is covered by T044, since this feature adds a public field to `condarc::ResolvedChannels` and a derive to `condarc::ChannelSetting`.
- FR-006's redaction scope is bounded to the recognized credential-transport conventions (userinfo, query, fragment, `/t/<token>/` path segment) per spec.md's Edge Cases and research.md Decision 6; no task attempts to strip arbitrary/unrecognized path-segment content, since doing so would risk stripping legitimate channel/package-name content from `UnresolvablePackage`/`IntegrityVerificationFailed` messages.
