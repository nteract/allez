# Implementation Plan: Private Channel Authentication

**Branch**: `GEN-29_private_channel_auth` | **Spec**: [spec.md](./spec.md)

## Summary

A channel needs `ALLEZ_CHANNEL_TOKEN` attached to its requests if and only if the operator's `.condarc` names it in `channel_settings` with an `auth` key present; a channel with no such entry is public, unconditionally, with no host- or URL-based fallback (research.md Decision 1). This targets `allez`'s actual usage: Anaconda-hosted public and private channels share the same domains, so host or URL shape alone can never distinguish them. Header injection is a custom `reqwest_middleware::Middleware` layered onto the one shared client `solve.rs` builds and `install.rs` reuses (Decision 2), matching the raw token value byte-for-byte with no scheme prefix, satisfying FR-002 in a way `rattler_networking`'s native, `Bearer`-prefixing auth storage cannot. A missing or unusable token when at least one private channel is configured is `EphemeralEnvError::MissingChannelToken`, raised before any network request (Decision 5). An HTTP 401/403 from a channel this feature authenticated is `EphemeralEnvError::ChannelAuthenticationFailed`, distinguished from `UnresolvablePackage`/`ResolutionFailed` at both the repodata-resolution and package-download HTTP call sites (Decision 4). See `research.md` for the full rationale and rejected alternatives behind each of these, and `data-model.md` for the concrete types/functions.

## Technical Context

**Language/Version**: Rust, edition 2024, unchanged from the rest of the workspace.

**Primary Dependencies**: existing `rattler` 0.48.0 stack (`rattler_repodata_gateway` 0.31.0, `rattler_solve` 8.0.0, `rattler_cache` 0.10.4, `rattler_conda_types` 0.49.0, `reqwest` 0.13, `tokio`, `condarc`), plus three new direct dependencies already present transitively: `astral-reqwest-middleware` 0.5.1 (imported as `reqwest_middleware`), `async-trait` 0.1.91, and `http` 1.4.2 (needed directly for `http::HeaderValue`, since a crate may only reference a transitive dependency's items through a direct dependency's own re-export, and `channel_auth.rs` builds an `http::HeaderValue` directly). See research.md Decision 2 for the exact `Cargo.toml` lines and the `cargo tree` evidence.

**Storage**: N/A — the token is read from the process environment once per invocation and never written anywhere (FR-007).

**Testing**: `cargo test --all` plus a new dev-dependency, `wiremock` 0.6, for the scenarios that require a real HTTP server (header transmission, zero-request assertions on missing token, real 401/403 propagation through both the solve and install paths). Pure classification and redaction logic is covered by plain unit tests with no HTTP server involved — see data-model.md's test matrix for the split.

**Target Platform**: Linux, macOS, Windows, unchanged.

**Project Type**: Single Rust workspace, library + CLI, unchanged.

**Performance Goals**: none new; the middleware adds one small linear scan over the configured private-channel list per outgoing request, negligible next to network I/O.

**Constraints**: FR-002's byte-for-byte/no-scheme-prefix requirement; FR-006's redaction requirement; FR-007's no-storage requirement; Constitution VII (No Hardcoded Values) — classification reads the operator's own `.condarc`, never a duplicate list embedded in `allez`. All three are enforced structurally, not just tested — see research.md Decisions 1, 5, and 6.

**Scale/Scope**: touches `src/ephemeral/{solve.rs,install.rs,error.rs,channels.rs}`, a new `src/ephemeral/channel_auth.rs`, `crates/condarc/src/expand_channels.rs` (one new `ResolvedChannels` field), and `crates/condarc/src/model.rs` (one derive addition on `ChannelSetting`). No CLI flag surface changes.

## Constitution Check

| Principle | Compliance |
|---|---|
| I. Code Quality | `channel_auth.rs` has one responsibility: classify, then build the authenticating client. No `unsafe`. |
| II. Testing Standards | Every FR and acceptance scenario maps to at least one planned test; see data-model.md's coverage table. |
| III. Dual-Primary Interface (Agent and Human) | No new CLI flags; both new `EphemeralEnvError` variants flow through the existing `CategorizedError`/`render_error` JSON and human paths unchanged. |
| IV. DRY | Reuses the existing shared-client wiring point and extends, rather than duplicates, `redact_channel_url`. |
| V. Explicit Over Implicit | Classification is a deterministic function of the operator's own `.condarc`, not a runtime auto-probe or a hardcoded guess. |
| VI. Documentation and Type Safety | New public `condarc` field and new error variants carry doc comments; the new variants make "silently proceeding without required auth" unrepresentable. |
| VII. No Hardcoded Values | Classification reads `channel_settings` directly; no duplicate host/channel list lives in `allez`. |
| VIII. Mandatory 100% Spec Test Coverage | See data-model.md's coverage table; every FR and acceptance scenario has a planned test. |
| IX. Determinism & Idempotency | Classification is a pure function of resolved configuration; the token is read once per invocation, never cached across calls. |
| X. Security & Supply-Chain Integrity | `astral-reqwest-middleware`/`async-trait` are both already compiled transitively at these exact versions; promoting them to direct dependencies changes no supply-chain exposure. |
| XI. Structured Observability | New failure categories flow through the existing `tracing`-based `EphemeralLifecycleEvent`/`CategorizedError` machinery. |

## Project Structure

### Documentation (this feature)

```text
specs/GEN-29_private_channel_auth/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   └── channel_auth_api.md
└── tasks.md              # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
crates/condarc/src/
├── expand_channels.rs      # ResolvedChannels gains channel_settings:
│                          # Vec<condarc::ChannelSetting>, a pass-through
│                          # of Config.channel_settings -- see data-model.md.
└── model.rs                 # ChannelSetting gains a derived Eq (needed
                             # because ResolvedChannels derives Eq) --
                             # see research.md Decision 3.

src/
├── channel_config/mod.rs    # unchanged -- still threads condarc::ResolvedChannels
│                            # straight through; classification interpretation
│                            # lives in allez, not here (research.md Decision 3).
└── ephemeral/
    ├── mod.rs                # gains one `mod channel_auth;` declaration;
    │                          # create_ephemeral_environment's own signature
    │                          # is unchanged.
    ├── channels.rs            # redact_channel_url extended to also strip a
    │                          # URL's query string and fragment.
    ├── channel_auth.rs         # NEW -- classification, token-header
    │                          # construction, ClientWithMiddleware assembly,
    │                          # the Middleware impl,
    │                          # reqwest_http_failure/matching_private_channel/
    │                          # origin_only, and the relocated
    │                          # HTTP_USER_AGENT constant (moved from
    │                          # solve.rs, its only remaining call site).
    ├── solve.rs                 # builds the client via
    │                            # channel_auth::build_channel_auth_client,
    │                            # maps gateway 401/403 to the new category
    │                            # only when the failing URL matches a private
    │                            # channel; SolvedPackages carries
    │                            # private_channels forward for install.rs.
    ├── install.rs                # maps InstallerError::FailedToFetch's 401/403
    │                            # to the new category under the same
    │                            # private-channel-URL check, alongside the
    │                            # existing hash-mismatch check.
    └── error.rs                  # two new EphemeralEnvError variants:
                                   # MissingChannelToken, ChannelAuthenticationFailed.

tests/
├── ephemeral_env.rs          # gains one new `#[path = ...] mod
│                             # private_channel_auth;` declaration,
│                             # matching its existing creation/defaults/
│                             # failures submodule pattern (this file
│                             # itself holds only `mod` declarations).
├── support/
│   ├── private_channel_auth.rs  # NEW -- the private-channel-auth spec
│   │                             # tests (uses wiremock for the scenarios
│   │                             # that need a real HTTP server).
│   └── ephemeral.rs              # extended with a wiremock-backed
│                                  # fixture helper.
```

**Structure Decision**: single project, additive within the existing `allez`/`condarc` workspace layout, no new crate. The only cross-crate change is `condarc`: one new `ResolvedChannels` field passing through data it already parses, plus a derive addition on `ChannelSetting` to satisfy `ResolvedChannels`'s own existing `Eq` bound.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**
