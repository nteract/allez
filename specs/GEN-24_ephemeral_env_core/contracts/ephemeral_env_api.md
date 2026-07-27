# Interface Contract: Ephemeral Environment Core public API

This feature exposes a **Rust library API**, not an HTTP/CLI interface —
its Assumptions section is explicit that wiring into `allez oneshot`'s CLI
surface is a separate ticket (GEN-25). This contract is what GEN-25 (and
any other in-process caller, including tests) can rely on. It lives at a
new top-level module, `src/ephemeral/mod.rs`, re-exported as
`allez::ephemeral::*` via a new `src/lib.rs` (see `plan.md` § Project
Structure — the crate is binary-only today; this ticket adds a library
target).

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

The function/method signatures below are shown **without bodies**,
matching how a trait interface is documented — they are the contract
this ticket's implementation must satisfy, not literal standalone
compiling code. Types referenced (`ChannelConfig`, `EphemeralEnvError`,
etc.) are defined in full, with bodies, either inline here or in
`data-model.md`.

## Public functions

```rust
/// Requests creation of a new ephemeral environment. Returns immediately
/// with a handle usable to signal teardown right away (FR-001) — creation
/// itself proceeds asynchronously (spawned onto the Tokio runtime this
/// function requires to already be running, matching the rest of the
/// crate's async boundary).
///
/// `requested` may be `RequestedPackages::Explicit(vec![])` — this is
/// treated identically to `UseDefaultOrOverride` (corrected in the third
/// review cycle: an earlier revision of this doc comment claimed the
/// opposite — that an empty explicit list was NOT the same as
/// `UseDefaultOrOverride` — which turned out to contradict FR-005 itself,
/// not merely this contract's own earlier design; see `data-model.md`).
/// `RequestedPackages::from_cli(_)` remains a convenience for translating
/// a raw, possibly-empty caller-supplied list, but is not required for
/// correctness.
pub fn create_ephemeral_environment(
    requested: RequestedPackages,
    channels: ChannelConfig,
    default_override: Option<Vec<PackageSpec>>,
) -> EphemeralEnvironmentHandle;
```

```rust
impl EphemeralEnvironmentHandle {
    /// This handle's identifier — stable for its whole lifecycle, and the
    /// value every `EphemeralLifecycleEvent` for this environment carries
    /// (FR-013/SC-008).
    pub fn id(&self) -> EnvironmentId;

    /// Signals teardown. Never blocks waiting for removal to finish —
    /// idempotent and non-blocking per FR-012. See `data-model.md`'s
    /// `LifecycleState` transition table for the exhaustive no-op/
    /// folds-in/starts-the-attempt behavior for every possible current
    /// state, including a creation-failure's own in-flight cleanup.
    pub fn signal_teardown(&self);

    /// Resolves once creation finishes, one way or the other —
    /// including waiting through an in-progress cleanup after a creation
    /// failure (`CreationFailed { cleanup: Running, .. }` is explicitly
    /// NOT terminal; corrected in the third review cycle — see
    /// `data-model.md` for why an earlier revision could return
    /// prematurely with a since-then-discovered cleanup failure silently
    /// dropped). `CreationFailure` (not a bare `EphemeralEnvError`) so a
    /// cleanup failure following a creation failure can be reported
    /// alongside the original error, per FR-010 — see `data-model.md`.
    /// `CreationFailure` implements `std::error::Error`/`Display`, so
    /// `handle.await_ready().await?` works directly with `?` (needed by
    /// the quickstart's manual smoke test). Always reports the *original*
    /// creation outcome even if `signal_teardown()` was already called
    /// (before or after this method) and has since moved this handle on
    /// to `TearingDown`/`TornDown` — a fourth correction, see
    /// `data-model.md`'s `EphemeralEnvironmentHandle`/`LifecycleState`
    /// sections for the decoupled internal cell this relies on.
    pub async fn await_ready(&self) -> Result<ReadyEnvironment, CreationFailure>;

    /// Resolves once a signaled teardown actually completes (success), or
    /// the distinct teardown-failure category if removal itself failed.
    /// Awaiting this without ever calling `signal_teardown()` first waits
    /// indefinitely, **except one case (see `data-model.md`'s
    /// `LifecycleState` section): if creation itself failed and that
    /// failure's own cleanup has already resolved
    /// (`CreationFailed { cleanup: Succeeded | Failed(_), .. }`), this
    /// resolves immediately — that cleanup attempt *was* this
    /// environment's teardown, so there is nothing further to wait for**.
    /// Callers needing "torn down or still active" polling for a
    /// successfully-created environment should await this only after
    /// calling `signal_teardown()`.
    pub async fn await_torn_down(&self) -> Result<(), EphemeralEnvError>;

    /// Non-blocking. `ReclamationStatus::Scanning` while the automatic
    /// scan this handle's own creation request triggered (FR-008) is
    /// still running; `ReclamationStatus::Complete(outcomes)` once it
    /// finishes — this is the distinct signal FR-008 requires, decoupled
    /// from this handle's own creation outcome. (Corrected in this
    /// revision from a bare `Vec<OrphanReclamationOutcome>`, which
    /// couldn't distinguish "still scanning" from "found nothing.")
    /// `ReclamationStatus::Failed(error)` if the scan itself could not
    /// even start (e.g. the root's own secure-open/verify check failed) —
    /// **new in this revision**: distinct from `Complete(vec![])`
    /// ("scanned, found zero leftover directories"), since collapsing
    /// the two would let a caller wrongly conclude no orphans exist when
    /// reclamation never actually ran. See `data-model.md`.
    pub fn reclamation_outcomes(&self) -> ReclamationStatus;
}

pub enum ReclamationStatus {
    Scanning,
    Complete(Vec<OrphanReclamationOutcome>),
    Failed(EphemeralEnvError),
}
```

```rust
/// Scans for and removes/reports any ephemeral environment left behind by
/// a prior, no-longer-running process for the same local user account and
/// `allez` installation (FR-008/SC-003). Called automatically as the first
/// step of `create_ephemeral_environment` (its results surfaced via
/// `EphemeralEnvironmentHandle::reclamation_outcomes`), and also exposed
/// standalone so tests can exercise orphan reclamation deterministically.
/// Returns `Err(EphemeralEnvError::UnwritableLocation)` — **new in this
/// revision** — if the scan itself cannot even start (the root or its
/// `.root.lock` fails the same secure-open/verify check `paths.rs`
/// requires elsewhere), so this failure is never silently indistinguishable
/// from `Ok(vec![])` ("scanned, found nothing"). See `data-model.md`'s
/// `ReclamationStatus::Failed` for how this surfaces through the
/// automatic, handle-driven path.
/// Liveness is determined definitively via a non-blocking attempt to
/// acquire each candidate directory's own OS-level advisory lock (`fs4`;
/// `flock`/`LockFileEx` under the hood), not by inspecting a PID — an
/// acquired lock means the owning process is verifiably gone (the OS
/// itself releases the lock on process exit, including `SIGKILL`); a
/// held lock means it's still `StillActive`, regardless of the
/// directory's age. `Unknown` (corrected in the third review cycle) is
/// now reserved for a genuine I/O error opening/locking the lock file
/// itself (e.g. permission denied) — not for "metadata hasn't been
/// written yet," which the lock-based design no longer has as a race —
/// and is never removed, matching FR-008's conservative "never remove
/// something still actively owned" guarantee.
pub fn reclaim_orphaned_environments() -> Result<Vec<OrphanReclamationOutcome>, EphemeralEnvError>;

pub enum OrphanReclamationOutcome {
    Removed { id: EnvironmentId },
    RemovalFailed { id: EnvironmentId, error: EphemeralEnvError }, // category: TeardownFailed
    StillActive { id: EnvironmentId },       // lock is held; not touched
    Unknown { id: EnvironmentId },           // lock file access itself failed (e.g. permission denied); not touched, conservative
}
```

## Public types (full field definitions; see `data-model.md` for rationale)

```rust
pub struct EphemeralEnvironmentHandle { /* opaque; see methods above */ }

#[derive(Debug, Clone)]
pub struct ReadyEnvironment {
    pub id: EnvironmentId,
    pub location: std::path::PathBuf,
    pub installed_packages: Vec<InstalledPackage>,
    // (private, not part of this contract's own public surface — but
    // its PRESENCE is a correctness requirement, not optional: closes a
    // real use-after-drop hazard a review found. `handle.await_ready().await?`,
    // chained on a temporary with no `let handle = ...;` binding kept
    // around (this contract's own documented usage pattern above), drops
    // that temporary the instant the statement ends; without this field
    // independently keeping the same cleanup guard alive, that drop
    // would remove the environment before the very next line ever uses
    // the `ReadyEnvironment` just returned. See `data-model.md` for the
    // exact type (`Arc<CleanupGuard>` — the same one
    // `EphemeralEnvironmentHandle` itself holds) and full rationale.
    keep_alive: std::sync::Arc<()>, // (opaque placeholder here; real type is crate-internal `CleanupGuard`)
}

impl ReadyEnvironment {
    /// PATH/env-var overlay for running a command inside this environment
    /// (GEN-25's own requirement). Returns `ActivationError`, not
    /// `EphemeralEnvError` — activation isn't a create/install/teardown
    /// operation, so it doesn't belong in FR-010's closed category set
    /// (corrected in this revision). See `data-model.md`.
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
    /// `RequestedPackages::from_cli` internally (added to the public
    /// surface in this revision — the first draft defined this method in
    /// `data-model.md` but never listed it here or marked it `pub`).
    pub fn parse(input: &str) -> Result<Self, InvalidPackageSpec>;
}

pub struct InvalidPackageSpec { pub input: String, pub reason: String }

#[derive(Debug, Clone)]
pub struct CreationFailure {
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
/// **Illustrative shape, not a pinned literal (corrected in this
/// revision — an earlier draft stated this exact `&["python", "pip"]`
/// value as settled, then separately said package names were still
/// implementation-time-confirmed, which contradicted itself)**: the real
/// values are pinned by whatever fixture channel this feature's own test
/// suite provides — the fixture is authoritative for this constant's
/// contents, not the other way around. `["python", "pip"]` here is only a
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
  guarantee).
- **`ALLEZ_ROOT_LOCK_TIMEOUT_MS`** (env var, optional, default `2000`):
  overrides the bounded wait for the brief root-level lock creation and
  reclamation both serialize against (see `research.md`'s "named,
  configurable timeout" decision) — an invalid/unparseable value falls
  back to the default rather than erroring. Exhausting this timeout is
  surfaced as `EphemeralEnvError::UnwritableLocation`.
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
  installation creates (see `research.md`) — it is not removed on any
  single environment's teardown, and is not scanned by
  `reclaim_orphaned_environments()`.

## Exit-cleanup contract: `Drop`-only, no signal handler (corrected in this revision)

**No signal handler of any kind is installed by this feature, and it has
no `ctrlc` dependency** — an earlier revision of this contract proposed a
process-wide `ctrlc::set_handler`-based Ctrl-C handler; that proposal is
removed entirely, not merely softened, because it never applied in the
first place: `allez` is invoked by an AI agent operating inside an
externally-established sandbox (spec Operating Context), never directly
by a human at a terminal, so there is no human-initiated interrupt (a
terminal Ctrl-C) for this feature to ever need to catch. The only two
exit-cleanup mechanisms this feature provides are:

1. An RAII `CleanupGuard`'s `Drop` implementation, which best-effort-removes
   the prefix directory on ordinary Rust scope-unwinding (normal process
   exit, or an explicit early drop of a live handle).
2. `reclaim_orphaned_environments()`, run automatically as the first step
   of every `create_ephemeral_environment` call, which catches anything
   `Drop` can't (a crash, `SIGKILL`, power loss — none of which run Rust's
   normal unwind path).

**Caller obligation: never call `std::process::exit()` with live handles
outstanding.** `std::process::exit()` (and `abort()`) skip Rust's normal
unwind/drop sequence entirely, so any live `EphemeralEnvironmentHandle`
whose teardown hasn't already been awaited at that point will **not** be
cleaned up by `Drop`; it will only ever be recovered later via
`reclaim_orphaned_environments()` on a subsequent `create()` call (still
correct per FR-008's reclamation-deadline fallback, but not "at exit
time"). Any caller of this API (GEN-25 included) that needs the
immediate-cleanup guarantee, not just the eventual-reclamation one, MUST
either await every live handle's teardown before exiting, or avoid
`std::process::exit()`/`abort()` in favor of returning normally from
`main()`.

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
</content>
