# Implementation Plan: Ephemeral Environment Core (Create, Populate, Teardown)

**Branch**: `GEN-24_ephemeral_env_core` | **Date**: 2026-07-27 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/GEN-24_ephemeral_env_core/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command; its definition describes the execution workflow.

## Summary

Implement, as a Rust library module (no CLI wiring — that's GEN-25), the
create → solve/install lifecycle for an unnamed, caller-unpathed conda
environment: creation always succeeds or fails atomically, installs
either the caller's explicit package list or the built-in/overridden
default set, relies on `rattler`'s own built-in checksum verification
before install, and restricts the environment's location to the owning
user. **Removal is explicit, not automatic (revised — see the
"Explicit reap, no automatic reaping" revision note immediately
below)**: a successfully created environment is never torn down by this
feature on its own — not on a signal, not on process exit, not via any
crash/orphan-detection mechanism — it simply persists on disk until a
caller invokes a separate, standalone reap operation that removes every
ephemeral environment it finds for the current local user account and
`allez` installation, unconditionally, without checking whether any of
them is still in use. Technical approach: build directly on the native
Rust `rattler` ecosystem (the same libraries `pixi` uses) for repodata
fetching, solving, and checksum-verified install/link, rather than
shelling out to `conda`/`mamba`; layer owner-only permissions (applied
atomically at directory-creation time) on top, since `rattler` itself
provides neither. **No signal handler of any kind is installed** —
`allez` is invoked by an AI agent inside an externally-established
sandbox, never directly by a human at a terminal, so there is no
Ctrl-C/SIGINT for this feature to ever catch; there is also no RAII
`Drop`-based cleanup guard any more (see the revision note below) — a
failed creation attempt is still rolled back synchronously, as part of
the same call that discovered the failure, but a successful one is left
alone entirely.

**Revision note**: this plan was revised after a review pass, and again
after a later, separate product decision. Two explicit product decisions
bound the review-pass revision: GEN-29 (private-channel authentication)
is deferred in full — every auth-related design element from the first
draft is removed, not patched — and checksum verification relies 100% on
`rattler_cache`'s own behavior as-is, with the review's "verifies after
extraction" observation accepted rather than fixed for this ticket. See
`research.md`'s revision note for the full list of corrections
(channel-config alignment with GEN-36, atomic permission application,
shared package cache, error/lifecycle model gaps, a new `src/lib.rs`, and
more).

**Explicit reap, no automatic reaping (later revision, supersedes every
automatic-teardown/orphan-reclamation design element the review-pass
revision above added)**: a subsequent, separate product decision removed
automatic teardown entirely. There is no longer a per-environment
teardown signal, no `EphemeralEnvironmentHandle` returned before creation
completes, no RAII `CleanupGuard`/`Drop`-based cleanup for a successfully
created environment, no per-environment `.owner.lock` liveness marker,
no root-level `.root.lock` publication/reclamation serialization, and no
`reclaim_orphaned_environments()` orphan-detection scan. `fs4` is no
longer a dependency of this feature at all. `create_ephemeral_environment`
is now a plain `async fn` that resolves directly to
`Result<ReadyEnvironment, CreationFailure>` — there is no intermediate
handle type. In its place, this ticket adds one new, standalone function,
`reap_ephemeral_environments()`, that removes every ephemeral environment
directory it finds under the managed root, one at a time, reporting each
one's own outcome independently and without any liveness check at all —
see `research.md`'s "Explicit reap, no automatic reaping" decision for
the full rationale and `contracts/ephemeral_env_api.md` for the resulting
public API. A creation attempt that itself fails is unaffected by this
change: rolling back whatever partial directory a failed attempt created
still happens automatically, synchronously, as part of the same call that
discovered the failure (FR-004/FR-010) — only a *successfully completed*
creation's own environment is now left alone rather than torn down
automatically. This is a deliberate, temporary simplification, not a
permanent design point; more automatic, safety-checked reclamation is
expected to be revisited in a future ticket.

## Technical Context

**Language/Version**: Rust, `edition = "2024"` (matches existing
`Cargo.toml`).

**Primary Dependencies**: `rattler`, `rattler_conda_types`,
`rattler_repodata_gateway`, `rattler_solve`, `rattler_cache`,
`rattler_virtual_packages`, `rattler_shell`, `ulid` (package
resolution/install, plus activation-environment computation for GEN-25's
benefit, plus environment-identifier generation); `tokio` (async runtime
these require); `reqwest` (direct dependency — `solve.rs`/`install.rs`
construct and configure the shared HTTP client themselves, so this cannot
be left merely transitive via `rattler_repodata_gateway`); `rustix`
(direct dependency, `fs` + `process` features — handle-anchored directory
creation/removal and current-user-ownership checks in `paths.rs`/
`permissions.rs`/`cleanup.rs`); `tempfile` (already a
dev-dependency, promoted to a normal dependency); `windows-sys`
(Windows-only, owner-only ACLs, applied atomically at directory creation).
See `research.md` for versions and the rationale behind each. **`fs4` is
not a dependency of this feature** — an earlier revision of this plan
added it for per-environment/root-level advisory-file-locking used by
automatic orphan detection; that whole mechanism was removed by the
"Explicit reap, no automatic reaping" decision (see the Summary's own
revision note above and `research.md`), so there is no liveness lock of
any kind left for it to back. **`ctrlc` is
not a dependency of this feature** — `allez` is invoked by an AI agent
inside an externally-established sandbox, never directly by a human at a
terminal, so there is no human-initiated interrupt (e.g. a terminal
Ctrl-C) for this feature to handle; a failed creation attempt is rolled
back synchronously as part of the same call that discovered the failure,
and a successful one is left alone entirely — removed only by the
explicit `reap_ephemeral_environments()` call a caller makes on its own
— so there is no exit-time cleanup mechanism of any kind that a signal
handler could usefully hook into.
**`rattler_networking` is not added as a direct dependency** — this
ticket adds no authentication middleware; GEN-29 owns that entirely when
it lands. This does not mean HTTP/TLS crates are absent from the
dependency *tree* — `reqwest` (and possibly `rattler_networking` itself)
is very likely a transitive dependency of `rattler_repodata_gateway`
regardless, since fetching repodata needs HTTP either way; see
`research.md`'s corrected Open Items note on why the ISC/`aws-lc-rs`
license question is not moot for this ticket. Reuses existing
`tracing`/`tracing-subscriber` for structured observability — no second
logging pipeline.

**Storage**: N/A (no database/config file produced by this feature). Files
on disk are: (a) each ephemeral environment's own prefix directory —
persists after a successful creation until an explicit
`reap_ephemeral_environments()` call removes it (see the Summary's
"Explicit reap, no automatic reaping" revision note above; a *failed*
creation attempt's own partial directory is still rolled back
immediately, unaffected by that revision) — and (b) a **long-lived,
shared** package-download cache and repodata cache, reused across every
ephemeral environment this installation creates (see `research.md` §
Ephemeral location + package/repodata cache for how this gives GEN-32 a
real cold-vs-warm-cache distinction to benchmark). Both live under a
single configurable root (`$ALLEZ_EPHEMERAL_ROOT`, falling back to a
per-user, per-installation subdirectory of `std::env::temp_dir()`) —
chosen specifically so a sandbox wrapping `allez` can redirect this
feature's entire filesystem footprint to one explicitly-granted,
dedicated location without any code change; see `research.md` §
Sandbox-visible filesystem/process footprint.

**Testing**: `cargo test --all` (unchanged entry point). New unit tests
alongside the code they test (`#[cfg(test)]`, per Constitution II); a new
integration test file (`tests/ephemeral_env.rs`) exercising the real
solve→install→reap path against a checked-in local `file://` fixture
channel — network-free and deterministic by construction, following the
same "opt-in feature flag for anything that needs a live oracle" pattern
`condarc_conformance`/`conformance-tests` already established, reused here
only for any *additional* live-network smoke test, not for the core
fixture-backed suite. See `research.md` § Test strategy.

**Target Platform**: Windows amd64, macOS aarch64, Linux aarch64, Linux
amd64 — the parent epic's (GEN-19) own target list, explicitly named in
FR-014/SC-009 for this ticket's permission/lifecycle guarantees. macOS
means Apple Silicon (`aarch64`) exclusively — Intel/`amd64` macOS is not
a supported target for this feature or the parent epic, and this ticket
does not need to build for or test it. CI validation for these four
targets is bounded by what GitHub Actions actually offers on its
standard/free runner tiers: `macos-latest` is already an Apple Silicon
(`aarch64`) runner, so the existing `test` job's `macos-latest` leg
already covers the one supported macOS target with a full `cargo test
--all` run; **Linux `aarch64` now also has a real, free, GitHub-hosted
native runner — `ubuntu-24.04-arm` — generally available for public
repositories since August 2025 (corrected in this revision: an earlier
draft's "no equivalent free-tier native runner, cross-compile-check-only"
framing was accurate when first written but is now stale)**. `allez`
(`nteract/allez`) is a public repository, so this ticket's own CI matrix
SHOULD add a real `ubuntu-24.04-arm` leg running the full `cargo test
--all` suite natively, not merely `cargo check --target
aarch64-unknown-linux-gnu` — closing what was previously an accepted
limitation rather than continuing to accept it now that a fix is free
and available. This does not require this plan itself to guess at that
job's exact YAML; the concrete CI change is implementation-time work.

**Project Type**: Library (a new internal module within the `allez`
package, gaining a library target of its own — see Project Structure; no
new public CLI surface in this ticket — see spec Assumptions). "Single
Rust crate" was the first draft's phrasing for this line; corrected here
since the `allez` *package* itself remains singular, but the *repository*
is no longer a single crate as of GEN-36/PR #3 merging a `crates/condarc`
workspace member (see Structure Decision below) — this ticket's own scope
is unaffected by that either way.

**Performance Goals**: Not defined by this ticket (tracked separately by
GEN-32 per spec Assumptions); this plan does not introduce performance
targets.

**Constraints**: Cross-run resolution is best-effort, not deterministic
(FR-004); no bespoke retry layer for transient failures (FR-010); no
credential material in inputs, error messages, or structured events
(FR-013); owner-only environment-location access on all four target
platforms (FR-014); a reap call MUST process each environment it finds
independently, so a removal failure for one never prevents or affects
another (FR-009); reap performs no liveness/in-use detection of any kind
and removes every environment it finds unconditionally — the calling
caller is solely responsible for only invoking it when doing so is safe
(FR-008; see the Summary's "Explicit reap, no automatic reaping"
revision note above); this
feature always runs inside an externally-imposed sandbox with only
explicitly-granted filesystem/exec visibility (spec Operating Context) —
every path this feature's own code touches must be explicit and
redirectable via `$ALLEZ_EPHEMERAL_ROOT`, never an implicit crate default,
and the small set of unavoidable OS-detection touchpoints (see
`research.md`) must be enumerated for whoever authors that sandbox
profile, not discovered later as denials. **Private-channel authentication
(GEN-29) is out of scope for this ticket by explicit product decision** —
no credential-source seam, env var, or placeholder mechanism is designed
here; a private channel simply fails like any other unreachable one until
GEN-29 exists. **Checksum verification relies entirely on `rattler_cache`'s
own built-in behavior, accepted as-is** — this ticket adds no additional
pre-extraction verification layer, by explicit product decision (see
`research.md`).

**Scale/Scope**: One `allez`-internal library module plus its own unit and
integration tests; no schema/migration/multi-service scope. Concurrency
scope is "multiple ephemeral environments coexisting within one or more
`allez` process invocations on one machine" (FR-009), not a distributed or
multi-host scenario.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|---|---|---|
| I. Code Quality | PASS | Module split (`channels`, `defaults`, `error`, `solve`, `install`, `permissions`, `cleanup`, `reap`, `lifecycle`, `events`, `paths`) keeps each file single-responsibility — `reap.rs` replaces the earlier `orphan.rs`/`handle.rs`/`state.rs` trio the "Explicit reap, no automatic reaping" revision removed (see Summary). **One documented `unsafe` FFI exception *category* (raw Windows `CreateFileW`/security-descriptor FFI, sharing one `// SAFETY:`-discipline, used at three call sites** — `paths.rs`'s no-follow root secure-open/verify, `permissions.rs`'s atomically-ACL'd directory creation, and `cleanup.rs`'s pre-removal no-follow re-verify — see Complexity Tracking for the SID/ACL-lifetime and buffer/return-value obligations each site's own `// SAFETY:` comment must state; these three sites are unaffected by the reap revision, since `remove_prefix_dir()` is still the one anchored removal primitive both the creation-failure rollback path and `reap.rs` call. |
| II. Testing Standards | PASS | TDD; unit tests co-located; integration tests under `tests/ephemeral_env.rs` against a local fixture channel — isolated, deterministic, fast. Coverage targets now include: the atomic-permission-creation behavior, the creation-failure rollback's own dual-failure path, and `reap_ephemeral_environments()`'s own behavior (removes every environment it finds; reports a per-environment removal failure without affecting any other environment in the same call; is a no-op when nothing remains) — the `LifecycleState` transition-table coverage an earlier revision described here no longer applies, since that type no longer exists. |
| III. Dual-Primary Interface | N/A (justified) | This ticket ships no CLI subcommand (spec Assumptions: CLI wiring is GEN-25's job). The public API returns fully-typed `Result`s (`EphemeralEnvError` with a fixed `category()`, wrapped in `CreationFailure` where FR-010's dual-failure case applies) specifically so GEN-25 can satisfy this principle later without re-deriving categories from message text. (Wording corrected: `CreationFailure` itself does not have a `category()` — only the `EphemeralEnvError` fields inside it do; see `data-model.md`.) |
| IV. DRY | PASS | `CategorizedError` trait (see `contracts/ephemeral_env_api.md`) lets `EphemeralEnvError` share `AllezError`'s existing category-rendering pattern. `DEFAULT_PACKAGES` defined once, in code, per FR-005. `EphemeralEnvError` is now `#[non_exhaustive]` (corrected from the first draft), resolving a direct contradiction with FR-010's "MAY add further category values as an additive, non-breaking extension" — the attribute costs nothing internally since every current match lives inside this crate; it only constrains a future external consumer. Also now matches the `#[non_exhaustive]` precedent GEN-36's `condarc::Config`/`ChannelPriority` already established in this repo. **Reconciling with GEN-22's prior review decision** ("one error type, captured in one place at the top of the CLI"): that decision governs `AllezError`'s role as the single error type the *CLI* (`src/cli/`, `main.rs`) surfaces — this ticket ships no CLI code at all (row III), so it does not yet decide how `EphemeralEnvError` and `AllezError` interoperate once a CLI consumer exists. The shared `CategorizedError` trait is deliberately the seam that lets GEN-25 keep rendering through one code path (`output::render_error`) regardless of whether it chooses to keep two enums or eventually fold `EphemeralEnvError` into `AllezError` — that choice is explicitly left to GEN-25, not decided here. |
| V. Explicit Over Implicit | PASS | Newtypes (`EnvironmentId`, `PackageSpec`, `ChannelSpec`) instead of bare `String`s at the public boundary; `rattler`-internal types never cross that boundary. No `.unwrap()`/`.expect()` outside tests. Every `rattler`-facing path (package cache, repodata cache) is passed explicitly — never `rattler_cache::default_cache_dir()` or the gateway's own default. No implicit credential source of any kind (auth is entirely out of scope this ticket, not implicitly deferred to a crate default). |
| VI. Documentation and Type Safety | PASS | Every public type/function gets a doc comment; `create_ephemeral_environment` is a plain, fully-typed `async fn` (no internal state machine to keep exhaustive any more — the "Explicit reap, no automatic reaping" revision removed `LifecycleState` entirely, see Summary); `ReapOutcome`'s two variants (`Removed`/`RemovalFailed`) are likewise a small, exhaustive, documented enum. `cargo doc` now has real public API to document once `src/lib.rs` exists (see Project Structure) — existing doc comments on `error.rs`/`observability.rs`/`output.rs` already look sufficient; confirm with a `cargo doc` run at implementation time. |
| VII. No Hardcoded Values | PASS | `DEFAULT_PACKAGES` is a named, documented constant; the FR-015 empty-`channels` fallback (`"defaults"`) is likewise a named, documented constant in `channels.rs`, not an inline literal; every path this feature owns is derived from one configurable root (`$ALLEZ_EPHEMERAL_ROOT`, falling back to a per-user, per-installation temp-dir path — not a bare, collision-prone `std::env::temp_dir()`); all paths built with `PathBuf`. |
| VIII. Mandatory 100% Spec Test Coverage | PASS (planned) | `quickstart.md` enumerates the acceptance-scenario → test mapping at a plan level; exact test IDs are the task-breakdown phase's own job. Covers: allow/deny channel filtering, the `Flexible` channel-priority mapping, `Strict` channel-priority ordering, the integrity-verification failure path (FR-011/SC-006), the creation-failure rollback's own dual-failure (`CreationFailure.cleanup_error`) path, and — per the "Explicit reap, no automatic reaping" revision (see Summary) — `reap_ephemeral_environments()` removing every environment it finds, reporting a per-environment removal failure independently, and being a no-op when nothing remains (User Story 2, rewritten). None of the prior revision's lock-based `StillActive`/`Removed`/`Unknown` orphan-classification coverage, or the Drop-without-signal "cleanup at exit time" coverage, applies any more — both described a mechanism this revision removed. |
| IX. Determinism & Idempotency | PASS | FR-004 explicitly scopes non-determinism to cross-run channel-availability drift. Reap's own idempotency (invoking it with nothing left to remove is a no-op, not an error) is a plain, directly-testable property of `reap_ephemeral_environments()` — it no longer needs a state-machine transition table to reason about, since the "Explicit reap, no automatic reaping" revision removed `LifecycleState`/per-environment teardown signals entirely (see Summary). `Cargo.lock` stays committed once dependencies are added (implementation-phase task). A retried creation request intentionally producing a *new*, independent environment rather than being deduplicated against a possible prior success is a **team-approved deviation from Principle IX's literal text — validated in review, but not yet reflected in `constitution.md` itself** (amending the constitution is out of scope for a feature PR; see Governance below): each `create_ephemeral_environment` call is a new one-shot operation by product design, not a retry of a previously-completed one, so Principle IX's "re-running a completed operation MUST be idempotent" does not apply to it — the team has signed off on this reading for this plan; folding it into Principle IX's own text is a separate, dedicated constitution-amendment change, not something this table can do on its own. |
| X. Security & Supply-Chain Integrity | PASS | Checksum verification (FR-011) relies entirely on `rattler_cache`'s existing built-in SHA-256/MD5 check, accepted as-is by explicit product decision — no redundant verification layer, and no claim that this fully satisfies "before extraction" (see `research.md`'s honest accounting of that gap). This reliance is a **team-approved deviation from Principle X's literal text — validated in review, but likewise not yet reflected in `constitution.md` itself**; do not re-raise it as an open item in future review, but also don't cite it as an already-amended constitutional fact until a dedicated amendment actually lands. No authentication middleware/credential storage of any kind is added in this ticket (GEN-29 deferred in full). `execute_link_scripts` is explicitly set to `true`, **overriding `rattler`'s own current default of `false`** (not "left at the default" — this is a deliberate override, not an accident of upstream defaults): this feature always runs inside an externally-imposed sandbox (spec Operating Context; confirmed directly by GEN-19's epic body as updated 2026-07-27, "Phantom secrets are securely injected into the environment such as the real secrets are never themselves present in it") that is the actual code-execution/damage boundary, not this feature — giving the invoking AI agent full freedom to run a package's own install-time code inside that sandbox, with no additional consent gate from allez, **is** the "explicit, documented consent" Principle X's post-install-script clause requires; this is the same team-approved-but-not-yet-constitutionally-amended deviation as above, not a second, independent one. `cargo deny check` was independently re-run and found **green** at both this branch's HEAD and post-merge `main` (correcting the prior "currently red" characterization, which was specific to a reviewer's older `cargo-deny` version — see `research.md`'s Open Items); this ticket's own new dependencies, and their *transitive* HTTP/TLS dependencies (not just direct ones — see Primary Dependencies above), must still be verified with a real `cargo deny check` run at implementation time, and CI now also gains a blocking `cargo audit` job alongside `cargo deny`, both previously entirely absent from this repo's CI. Shared package-cache hard-linking, if used by `Installer`, must be disabled rather than merely preferred against — see `research.md`'s corrected residual-risk note. |
| XI. Structured Observability | PASS | `EphemeralLifecycleEvent` emitted via the existing `tracing`/`observability.rs` pipeline; carries `environment_id`, `operation`, `packages`, `duration_ms`, `outcome`, `failure_category`, `schema_version` per FR-013. Any future URL-bearing field addition must route through `channels::redact_channel_url()` first (new decision — see `data-model.md`), keeping "MUST NOT contain credential material" true by construction. |

No constitution violations requiring *this plan's own* justification beyond
the one documented `unsafe` exception category (see Complexity Tracking's
three call sites) — no further gate failures to resolve before Phase 0. Two
further deviations exist, both discussed and signed off on by the team
during review, but **not yet folded into `constitution.md` itself** —
amending the constitution is out of scope for a feature PR, so this table
is where they're documented and approved for now, not `constitution.md`:
Principle IX's idempotency clarification and Principle X's
checksum-verification clarification are the maintainer-approved deviations
Governance's "Deviations MUST be documented and approved by maintainers"
clause requires, approved here in this plan, pending a separate, dedicated
constitution-amendment change to actually carry them into
`constitution.md`'s own text. Until that lands, treat both as this plan's
own documented exceptions, not as already-settled constitutional text.

## Project Structure

### Documentation (this feature)

```text
specs/GEN-24_ephemeral_env_core/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output (/speckit.plan command)
│   └── ephemeral_env_api.md
└── tasks.md             # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)

Single Rust *package* (the repo **is now** a Cargo **workspace** — GEN-36's
PR #3 merged `crates/condarc` into `main` as a separate member during this
plan's review cycle, confirmed directly against `main`'s current
`Cargo.toml`: `members = [".", "crates/condarc"]`. That addition does not
move or otherwise affect the `allez` package itself, which stays exactly
where it is at the workspace root). Within the `allez` package, this
feature adds one new internal module tree plus its integration test file,
and — new in this revision — a library target; it does not touch
`src/cli/` (that's GEN-25's job per spec Assumptions). **Implication for
this branch**: `GEN-24_ephemeral_env_core` was cut before that merge and
should be rebased onto post-merge `main` before implementation starts.

**New in this revision**: `src/lib.rs` is added. `contracts/ephemeral_env_api.md`
promises `allez::ephemeral::*` as a real, importable path, and
`tests/ephemeral_env.rs` (an external integration test) can only link
against a library target — neither is possible against a binary-only
crate, which is what `allez` is today (`src/main.rs` only, no `[lib]`
target). `src/main.rs` becomes a thin binary shim over the new library
target; no `[lib]`/`[[bin]]` *target-declaration* changes are needed in
`Cargo.toml` (Cargo infers both targets, both named `allez`, once
`src/lib.rs` exists alongside `src/main.rs`) — narrower than "no
`Cargo.toml` changes at all": every dependency under Primary Dependencies
above (`rattler`, `ulid`, etc.) still needs its own
`[dependencies]` entry added, same as any new dependency would.

```text
src/
├── lib.rs                   # NEW — pub mod cli; pub mod error; pub mod ephemeral; pub mod observability; pub mod output;
├── cli/                    # existing — untouched by this feature; now declared via lib.rs instead of main.rs
├── error.rs                # existing AllezError — gains the new shared CategorizedError trait
├── observability.rs         # existing tracing/tracing-subscriber init — reused, not duplicated
├── output.rs                # existing JSON/human rendering — untouched (GEN-25 will call it)
├── main.rs                  # updated — `use allez::{cli, error, observability, output};` instead of `mod` declarations; fn main() logic otherwise unchanged
└── ephemeral/                # NEW — this feature's entire scope
    ├── mod.rs                 # public API: create_ephemeral_environment, reap_ephemeral_environments, re-exports
    ├── paths.rs                # $ALLEZ_EPHEMERAL_ROOT resolution + per-user/per-installation temp-dir fallback naming + <root>/{envs,cache/packages,cache/repodata} layout (sandbox-footprint decision)
    ├── channels.rs            # ChannelConfig/ChannelSpec/ChannelPriorityMode (isomorphic to condarc::ChannelPriority — see research.md) + redact_channel_url()
    ├── defaults.rs            # DEFAULT_PACKAGES const + effective_packages() resolution (FR-005/FR-006)
    ├── error.rs                # EphemeralEnvError (#[non_exhaustive], implements CategorizedError) + CreationFailure (impl Error+Display, carries the failed attempt's own EnvironmentId) + ActivationError
    ├── solve.rs                # rattler_repodata_gateway::Gateway + rattler_solve wiring; explicit cache_dir, Flexible→Disabled channel-priority mapping (documented, ratified approximation — see research.md), CUDA virtual-package detection disabled via VirtualPackageOverrides, no_proxy() set
    ├── install.rs              # rattler::install::Installer wiring; explicit (shared, long-lived) package cache path; hard-linking disabled/non-shared cache fallback (see research.md); execute_link_scripts explicitly set to true, overriding rattler's own current default of false (the sandbox is the code-execution boundary, not this feature; see research.md and this plan's own Constitution Check, Principle X row — a team-approved deviation, not yet folded into constitution.md itself); activation_environment() via rattler_shell (returns ActivationError)
    ├── permissions.rs         # owner-only directory creation, applied atomically at creation: #[cfg(unix)] DirBuilder::mode(0o700)/mkdirat, #[cfg(windows)] SECURITY_ATTRIBUTES passed into CreateDirectoryW; secure create-or-verify on reuse of the fallback root
    ├── cleanup.rs              # remove_prefix_dir() — the one anchored, verified removal primitive both the creation-failure rollback path (mod.rs) and reap.rs call; no RAII guard, no registry, no signal handler of any kind (see the "Explicit reap, no automatic reaping" revision note in the Summary above)
    ├── reap.rs                 # NEW — reap_ephemeral_environments()'s implementation: lists every environment directory under the verified root's envs directory and calls cleanup.rs's remove_prefix_dir() on each, reporting a per-environment ReapOutcome (Removed/RemovalFailed); no liveness/in-use detection of any kind — replaces the removed orphan.rs/handle.rs/state.rs trio in full
    ├── lifecycle.rs            # EnvironmentId, InstalledPackage, ReadyEnvironment (plain data — no LifecycleState/handle machinery any more, see the "Explicit reap, no automatic reaping" revision note in the Summary above)
    └── events.rs               # EphemeralLifecycleEvent shape + tracing emission (FR-013/SC-008)

tests/
├── cli_scaffold.rs          # existing — untouched
├── condarc_conformance.rs   # existing — untouched
├── ephemeral_env.rs          # NEW — integration tests against the local fixture channel (see below)
└── fixtures/
    └── ephemeral_channel/     # NEW — checked-in tiny local conda channel (file:// source), generated once
```

**Removed from the first draft**: `src/ephemeral/auth.rs` — there is no
authentication design in this ticket's scope at all (see Constraints
above), so there is nothing for that module to contain. **Removed by the
"Explicit reap, no automatic reaping" revision** (see Summary):
`src/ephemeral/orphan.rs`, `src/ephemeral/orphan_files.rs`,
`src/ephemeral/handle.rs`, and `src/ephemeral/state.rs` — the
per-environment `.owner.lock`/root-level `.root.lock` liveness machinery,
the `EphemeralEnvironmentHandle`/`ReclamationStatus` public types, and the
`LifecycleState` state machine they all backed no longer exist; `reap.rs`
above replaces their entire responsibility with one small, unconditional
removal loop.

**Structure Decision**: Within the `allez` package, extend the existing
flat `src/` layout with one new cohesive module directory
(`src/ephemeral/`) rather than a new crate/workspace member — this
feature is small enough, and tightly-coupled-enough to `error.rs`/
`observability.rs`, that a separate crate would only add indirection
without a corresponding benefit (no other consumer exists yet; GEN-25
will `use allez::ephemeral::*` in-process). This choice is about this
feature's own internal organization, not about whether the *repository*
as a whole is a single crate or a workspace — it already isn't purely
the latter (GEN-36's `crates/condarc` is now a merged sibling workspace
member, not a still-in-flight one), and this feature's own layout is
unaffected either way. Each file above maps to exactly one of `research.md`'s technical
decisions, keeping Constitution I's "single, clear responsibility" per
module.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| `unsafe` FFI calls in `src/ephemeral/permissions.rs` (Windows: building a security descriptor via `ConvertStringSecurityDescriptorToSecurityDescriptorW` and passing it into `CreateDirectoryW`'s `lpSecurityAttributes`, so the directory is created with its owner-only ACL already in place — see `research.md`'s atomic-creation correction) | FR-014 requires owner-only access to the environment's location on Windows amd64, and the Win32 security-descriptor/ACL APIs are only exposed as raw, `unsafe` FFI — there is no safe abstraction in `std`. | The two candidate safe-wrapper crates (`windows-acl`, `windows-permissions`) are both unmaintained since 2021, making them a worse security/maintenance trade-off than a small, isolated, `// SAFETY:`-documented `unsafe` block directly against Microsoft's own maintained `windows-sys` bindings — exactly the exception path Constitution I already sanctions. The eventual `// SAFETY:` comment MUST document: (1) the security descriptor returned by `ConvertStringSecurityDescriptorToSecurityDescriptorW` is freed via `LocalFree` after `CreateDirectoryW` returns; (2) the wide (UTF-16) path/SDDL buffers outlive the FFI call; (3) `CreateDirectoryW`'s `BOOL` return is checked before assuming the directory/ACL exists. |
| `unsafe` FFI calls in `src/ephemeral/paths.rs` (Windows: a no-follow directory-handle open via `CreateFileW` with `FILE_FLAG_OPEN_REPARSE_POINT`/`FILE_FLAG_BACKUP_SEMANTICS`, then inspecting the returned handle's `dwFileAttributes` — **added in this revision, correcting an earlier undercount that listed only the `permissions.rs` site above**) | The root's (explicit or fallback) secure-open/verify sequence (`research.md`) requires a handle-relative, non-reparse-following open on Windows before this feature can safely trust anything about that root — `std` has no safe wrapper for `FILE_FLAG_OPEN_REPARSE_POINT`. | Same rejection as above — no safe wrapper exists; isolating this to one small, `// SAFETY:`-documented block against `windows-sys` is the same accepted trade-off, applied at a second call site rather than a second, independent exception category. The `// SAFETY:` comment MUST document: (1) the returned handle is checked for `FILE_ATTRIBUTE_REPARSE_POINT` before any further trust is placed in it; (2) the handle is closed on every return path, including error paths; (3) the wide path buffer outlives the call. |
| `unsafe` FFI calls in `src/ephemeral/cleanup.rs` (Windows: the same no-follow `CreateFileW` re-verify as `paths.rs` above, run immediately before `remove_prefix_dir()` removes its target — **added in this revision**) | `research.md`'s "anchoring extends to removal too" correction requires the same no-follow re-verify discipline immediately before removal on Windows, where no safe handle-relative recursive-delete primitive exists either. | Same rejection and same accepted trade-off as the `paths.rs` site above — this is the third call site sharing one `unsafe` FFI exception category, not a third independent exception. Same `// SAFETY:` obligations as the `paths.rs` site. |
