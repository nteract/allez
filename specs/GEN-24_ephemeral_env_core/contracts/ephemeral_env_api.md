# Interface Contract: Ephemeral Environment Core public API

This feature exposes a **Rust library API**, not an HTTP/CLI interface —
its Assumptions section is explicit that wiring into `allez oneshot`'s CLI
surface is a separate ticket (GEN-25). This contract is what GEN-25 (and
any other in-process caller, including tests) can rely on. It lives at a
new top-level module, `src/ephemeral/mod.rs`, re-exported as
`allez::ephemeral::*` via `src/lib.rs`.

Per Constitution III (Dual-Primary Interface), this contract does not
itself need a `--format json`/human split — that split is GEN-25's job,
rendering these typed `Result`s through `output::render_success`/
`render_error` the same way the existing CLI stubs already do.

**Scope note**: GEN-29 (private-channel authentication) is deferred in
full for this ticket. There is no authentication middleware, no
credential-source seam, and no `requires_auth` field anywhere in this
contract — a private channel simply fails like any other unreachable one
(`UnresolvablePackage`) until GEN-29 exists. Checksum verification relies
entirely on `rattler_cache`'s own built-in behavior, accepted as-is per
explicit product decision (see `research.md`).

**Explicit reap, no automatic reaping**: this contract's removal side is
a single, standalone function — `reap_ephemeral_environments()` — that
removes every ephemeral environment it finds unconditionally, with no
per-environment teardown signal, no handle type, and no liveness/orphan
detection of any kind. See `spec.md`'s User Story 2 and `research.md`'s
"Explicit reap, no automatic reaping" decision for the full rationale.

The function/method signatures below are shown **without bodies**,
matching how a trait interface is documented — they are the contract
this ticket's implementation must satisfy, not literal standalone
compiling code. Types referenced (`ChannelConfig`, `EphemeralEnvError`,
etc.) are defined in full, with bodies, either inline here or in
`data-model.md`.

## Public functions

```rust
/// Creates a new ephemeral environment: a system-managed temporary/cache
/// location (no name or path supplied by the caller), populated with the
/// requested (or default/overridden) packages, resolved and installed
/// against `channels`. Resolves once creation finishes, one way or the
/// other (FR-001) — there is no intermediate handle type; this is a
/// plain, directly-awaited `async fn`.
///
/// `requested` may be `RequestedPackages::Explicit(vec![])` — this is
/// treated identically to `UseDefaultOrOverride` (see `data-model.md`).
/// `RequestedPackages::from_cli(_)` remains a convenience for translating
/// a raw, possibly-empty caller-supplied list, but is not required for
/// correctness.
///
/// The returned `ReadyEnvironment` is **not** torn down when it (or its
/// last clone) is dropped, and there is no way to signal teardown for a
/// single environment — see [`reap_ephemeral_environments`] below. It
/// stays on disk, usable, until a caller later calls that function.
pub async fn create_ephemeral_environment(
    requested: RequestedPackages,
    channels: ChannelConfig,
    default_override: Option<Vec<PackageSpec>>,
) -> Result<ReadyEnvironment, CreationFailure>;
```

```rust
/// Removes every ephemeral environment found on disk for the current
/// local user account and `allez` installation — unconditionally,
/// without attempting to detect whether one is still in use elsewhere.
/// Callers are responsible for only invoking this when doing so is safe
/// (e.g. no other concurrent `allez` invocation still needs a live
/// environment). See this module's own doc comment and `spec.md`'s User
/// Story 2.
///
/// Processes each environment it finds independently (FR-009): a
/// removal failure for one is reported as `ReapOutcome::RemovalFailed`
/// without preventing or affecting any other environment's own outcome.
/// Returns `Ok(vec![])`, not an error, when no ephemeral environments
/// exist (FR-008's idempotency requirement) — including on a second,
/// immediately-repeated call after a first call already removed
/// everything.
///
/// # Errors
///
/// Returns [`EphemeralEnvError::UnwritableLocation`] if the root itself
/// cannot be securely opened or created — a scan that could not even
/// start, distinct from `Ok(vec![])` ("scanned, found nothing").
pub fn reap_ephemeral_environments() -> Result<Vec<ReapOutcome>, EphemeralEnvError>;

/// One environment's outcome from a [`reap_ephemeral_environments`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReapOutcome {
    /// The environment's directory was removed.
    Removed { id: EnvironmentId },
    /// The environment's directory could not be removed.
    RemovalFailed { id: EnvironmentId, error: EphemeralEnvError },
}
```

## Public types (full field definitions; see `data-model.md` for rationale)

```rust
#[derive(Debug, Clone)]
pub struct ReadyEnvironment {
    pub id: EnvironmentId,
    pub location: std::path::PathBuf,
    pub installed_packages: Vec<InstalledPackage>,
}

impl ReadyEnvironment {
    /// PATH/env-var overlay for running a command inside this environment
    /// (GEN-25's own requirement). Returns `ActivationError`, not
    /// `EphemeralEnvError` — activation isn't a create/install/reap
    /// operation, so it doesn't belong in FR-010's closed category set.
    /// See `data-model.md`.
    pub fn activation_environment(&self) -> Result<Vec<(String, String)>, ActivationError>;
}

pub struct ActivationError {
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub channel: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvironmentId(/* ulid::Ulid */);

pub struct ChannelConfig {
    pub channels: Vec<ChannelSpec>,
    pub channel_priority: ChannelPriorityMode,
    pub allowed_channels: Vec<String>,
    pub denied_channels: Vec<String>,
}

/// **Empty-`channels` fallback (FR-015)**: if `channels` is empty when
/// `create_ephemeral_environment` is called, this feature substitutes a
/// single built-in fallback entry (`ChannelSpec { url_or_name: "defaults".to_string() }`)
/// and proceeds as if the caller had supplied it — it does **not** fail
/// with `EphemeralEnvError::NoChannelsConfigured` for this case anymore.
/// `NoChannelsConfigured` is still returned if allow/deny filtering empties
/// the effective list afterward (including a list containing only the
/// substituted fallback channel). See `data-model.md`'s `ChannelConfig`
/// section for the exact ordering (fallback substitution happens before
/// allow/deny filtering, not after).

impl ChannelConfig {
    /// Convenience constructor for tests and the quickstart example:
    /// builds a `ChannelConfig` from a bare list of channel URLs/names
    /// with `ChannelPriorityMode::Strict` and no allow/deny restriction.
    /// Production callers (GEN-25, once GEN-23's `.condarc`-resolution
    /// step exists) should populate `ChannelConfig`'s fields directly
    /// instead, since real `.condarc` documents need `Flexible`/
    /// `Disabled` and allow/deny support this shortcut doesn't cover.
    pub fn from_urls(urls: Vec<String>) -> Self {
        Self {
            channels: urls.into_iter().map(|url_or_name| ChannelSpec { url_or_name }).collect(),
            channel_priority: ChannelPriorityMode::Strict,
            allowed_channels: Vec::new(),
            denied_channels: Vec::new(),
        }
    }
}

/// **Credential-validation boundary (per spec.md FR-003)**: `url_or_name`
/// is accepted as an opaque identifier. This feature does not parse,
/// canonicalize, or validate its contents, and in particular does not
/// detect or reject an accidentally credential-bearing value (e.g.
/// embedded `user:pass@` userinfo) at the point it's supplied — callers
/// MUST NOT supply one. `channels::redact_channel_url()` (see
/// `data-model.md`) exists purely as defense-in-depth for whatever this
/// feature itself formats into an error message or a tracing event; it
/// is not an input-validation gate, and its existence does not imply
/// this feature accepts credential-bearing input as a supported case.
pub struct ChannelSpec {
    pub url_or_name: String,
}

#[non_exhaustive]
pub enum ChannelPriorityMode { Strict, Flexible, Disabled }

pub enum RequestedPackages {
    Explicit(Vec<PackageSpec>),
    UseDefaultOrOverride,
}

impl RequestedPackages {
    /// Safe constructor for a raw, possibly-empty caller-supplied package
    /// list — see `data-model.md`.
    pub fn from_cli(packages: Vec<String>) -> Result<Self, InvalidPackageSpec>;
}

pub struct PackageSpec(/* opaque MatchSpec string */);

impl PackageSpec {
    /// `pub` — needed by any external caller constructing
    /// `default_override: Option<Vec<PackageSpec>>`, not only by
    /// `RequestedPackages::from_cli` internally.
    pub fn parse(input: &str) -> Result<Self, InvalidPackageSpec>;
}

pub struct InvalidPackageSpec { pub input: String, pub reason: String }

#[derive(Debug, Clone)]
pub struct CreationFailure {
    /// The environment identifier this failed attempt would have used —
    /// present so a caller/test can correlate this failure with the
    /// `EphemeralLifecycleEvent`s this attempt still emitted (FR-013),
    /// even though no `ReadyEnvironment` was ever produced.
    pub id: EnvironmentId,
    pub error: EphemeralEnvError,
    pub cleanup_error: Option<EphemeralEnvError>,
}

impl std::error::Error for CreationFailure {}
impl std::fmt::Display for CreationFailure { /* see data-model.md for the exact rendering */ }
```

- `EphemeralEnvError` (implements `std::error::Error`, `Display`, and a
  `category(&self) -> &'static str` method — same shape as the existing
  `crate::error::AllezError`; `#[non_exhaustive]` — see `data-model.md`)
- `EphemeralLifecycleEvent` — not constructed by callers; documented in
  `data-model.md` only as the `tracing` event shape.

## Cross-cutting contract: `CategorizedError` trait

To avoid FR-010's category set and the CLI scaffold's existing
`AllezError` category set drifting into two independently-maintained
"how do I render an error" code paths (Constitution IV: DRY), this plan
adds one shared trait in `src/error.rs`:

```rust
/// Implemented by every fixed error-category enum in this crate
/// (`AllezError`, `EphemeralEnvError`, ...) so `output::render_error` has
/// exactly one rendering path regardless of which subsystem raised the
/// error.
pub trait CategorizedError: std::error::Error {
    fn category(&self) -> &'static str;
}
```

`AllezError` and `EphemeralEnvError` both implement it; `output.rs`'s
existing `render_error(category: &str, message: &str, human: bool)`
signature is unchanged.

## Default package set (FR-005)

```rust
/// Fixed, non-empty, documented alongside this code so a test can assert
/// an ephemeral environment's installed top-level packages against it
/// exactly. Kept intentionally small so a from-scratch solve+install stays
/// fast in tests and in real one-shot usage.
///
/// **Illustrative shape, not a pinned literal**: the real values are
/// pinned by whatever fixture channel this feature's own test suite
/// provides — the fixture is authoritative for this constant's contents,
/// not the other way around. `["python", "pip"]` here is only a
/// representative example of the *kind* of small, useful default this
/// should be, not the literal contract.
pub const DEFAULT_PACKAGES: &[&str] = &["<fixture-defined>", "..."];
```

## Sandbox / filesystem-footprint contract

`allez` always runs inside an externally-imposed sandbox (spec Operating
Context — confirmed still current in GEN-19's epic body as of this
review), so this feature's entire filesystem/process-exec footprint is a
contract in its own right — see `research.md` § Sandbox-visible
filesystem/process footprint for the full rationale and per-platform
grant table.

- **`ALLEZ_EPHEMERAL_ROOT`** (env var, optional): if set, every path this
  feature touches is derived from it. If unset, falls back to a
  per-user, per-installation subdirectory of `std::env::temp_dir()` (see
  `research.md` for the exact naming scheme and why plain
  `std::env::temp_dir()` alone is insufficient for FR-008's "same caller"
  scoping).
- This feature never calls `rattler_cache::default_cache_dir()` or the
  gateway's own default cache dir — every `rattler`-facing path is passed
  explicitly.
- No authentication middleware is added to the HTTP client stack at all
  in this ticket's scope (see Scope note above) — there is nothing to
  enumerate here for credential-related filesystem access; GEN-29 owns
  that entirely, including updating this section when it lands.
- Required sandbox grants beyond `$ALLEZ_EPHEMERAL_ROOT` and outbound
  network access to the configured channels: macOS — read
  `/System/Library/CoreServices/SystemVersion.plist`, execute
  `/usr/bin/codesign`; Linux — the `uname` syscall and the system
  `libc.so.6` (rarely, `ldd --version` as a fallback); Windows — OS
  version/WMI queries via the `winver` crate (no explicit path).
- CUDA virtual-package detection is explicitly disabled (via
  `VirtualPackageOverrides`) — this feature never dynamically loads
  `libcuda.so*` or executes `nvidia-smi`.
- The package/repodata cache under `$ALLEZ_EPHEMERAL_ROOT` is
  long-lived and shared across every ephemeral environment this
  installation creates (see `research.md`) — it is not removed by any
  single environment's rollback or by `reap_ephemeral_environments()`.

## Removal contract: explicit reap only, no automatic teardown

**There is no automatic removal of a successfully created environment,
and no signal handler of any kind** — `allez` is invoked by an AI agent
operating inside an externally-established sandbox (spec Operating
Context), never directly by a human at a terminal, so there is no
human-initiated interrupt (a terminal Ctrl-C) for this feature to ever
need to catch, and there is no exit-time cleanup hook of any kind either.
This feature provides exactly two removal paths:

1. **Creation-failure rollback**: if `create_ephemeral_environment`
   itself fails (an unresolvable package, a failed integrity check, and
   so on), it removes whatever partial directory it created for that
   attempt synchronously, as part of the same call, before returning
   `Err(CreationFailure)` — reporting a distinct `cleanup_error` if that
   rollback itself also fails (FR-004/FR-010). This is the *only*
   automatic removal this feature performs, and it never applies to a
   *successfully* created environment.
2. **`reap_ephemeral_environments()`**: the sole way a caller removes one
   or more *successfully created* environments — see the function
   documentation above. It has no liveness/in-use detection of any kind.

**Caller obligation**: a successfully created `ReadyEnvironment` persists
on disk for as long as nothing removes it. Dropping every reference to
it, letting the owning process exit normally, or the owning process
being killed abruptly all have exactly the same effect on it: none.
Reclaiming the disk space an ephemeral environment occupies is entirely
the caller's own responsibility, taken only by explicitly calling
`reap_ephemeral_environments()` when the caller itself knows doing so is
safe. This is a deliberate, temporary simplification, not a permanent
design point — see `spec.md`'s User Story 2 and `research.md`'s
"Explicit reap, no automatic reaping" decision; safer, more automatic
reclamation is expected to be revisited in a future ticket.

## Open product-direction question (not blocking this plan)

Slack discussion (2026-07-23, `#ana-genai`) raised a possible future
product direction — `allez` enforcing a single "blessed" channel and
rejecting arbitrary `-c <channel>` selection — which would be in tension
with FR-002's current "honor channel configuration exactly as given,
never reinterpret." This is an open team discussion, not a ratified
requirement, and this ticket implements FR-002 as currently specified.
Flagged here so a future revision of `spec.md`/this plan can address it
explicitly if that direction is adopted, rather than this contract
silently becoming stale.

## Non-goals of this contract (explicitly out of scope, per spec Assumptions)

- No `--format`/JSON rendering here — that's GEN-25.
- No `.condarc` parsing, and no resolution of a parsed `condarc::Config`
  into this contract's `ChannelConfig` — `ChannelConfig` is accepted as a
  ready-to-use input; GEN-23 (using GEN-36's `condarc::Config` as its
  parsing sub-step) is what must learn to produce it.
- No default-override *authoring* mechanism — `default_override` is
  accepted as an already-resolved `Option<Vec<PackageSpec>>`; GEN-30 is
  what must learn to produce it.
- No private-channel authentication of any kind — GEN-29's entire scope,
  deferred without a placeholder mechanism (see Scope note above).
- No automatic teardown of any kind — safer, more automatic reclamation
  than the unconditional `reap_ephemeral_environments()` this ticket
  ships is deliberately deferred to a future ticket (see the Removal
  contract above).
