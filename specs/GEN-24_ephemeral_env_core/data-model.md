# Phase 1 Data Model: Ephemeral Environment Core

This feature is a library, not a data-store-backed service — "entities"
here are the Rust types that make the spec's Key Entities
(`spec.md` § Key Entities) and failure/observability contracts
representable in code, per Constitution VI's "make invalid states
unrepresentable" principle.

**Scope note (per explicit product decision during plan review)**: GEN-29
(private-channel authentication) is deferred in full — this revision
removes every placeholder auth mechanism the first draft introduced
(there is no `auth.rs` module, no `ChannelSpec.requires_auth` field, no
`AuthenticationStorage`/`MemoryStorage` construction). This ticket's HTTP
client adds no authentication middleware at all and supports public
channels only; a private channel simply fails to resolve/install like any
other unreachable channel, surfaced as the existing
`EphemeralEnvError::UnresolvablePackage` category until GEN-29 adds a
dedicated one. Checksum verification is relied upon exactly as
`rattler_cache` implements it today, as an explicit product decision — no
additional pre-extraction quarantine step is added in this ticket (see
`research.md` for the accepted-as-is note).

## `ChannelConfig` (consumed, never produced — see spec Assumptions)

**Corrected from the first draft**: GEN-36 (sub-task of GEN-23, already in
review as of this writing) has already landed the canonical parsed-`.condarc`
shape, `condarc::Config`, with `channels: Option<Vec<String>>`,
`allowlist_channels`/`denylist_channels`, and a **3-variant**
`channel_priority: Option<condarc::ChannelPriority>` (`Strict | Flexible |
Disabled`, matching real conda semantics, `Flexible` being conda's own
documented default). The first draft of this data model invented an
incompatible, 2-variant `ChannelPriorityMode` that couldn't represent
`Flexible` at all. `condarc::Config` is the *parsed* document, not a
*resolved, ready-to-solve-against* channel list — turning `default_channels`/
`custom_channels`/`custom_multichannels`/`channel_alias`/
`override_channels_enabled` into one flat, final, ordered channel list
remains GEN-23's own remaining scope (not GEN-36's, which only parses; not
this ticket's, which only consumes). `ChannelConfig` below is that
resolution step's target shape, deliberately kept isomorphic to
`condarc::Config`'s relevant fields so that adapter is a straightforward,
lossless mapping rather than a lossy one.

| Field | Type | Notes |
|---|---|---|
| `channels` | `Vec<ChannelSpec>` | Fully resolved and ordered (index 0 = highest priority): bare names already expanded via `channel_alias`/`custom_channels`/`custom_multichannels`, the `defaults` name already substituted via `default_channels`, `override_channels_enabled` already applied. This feature does none of that expansion itself (see spec Assumptions: "does not read or parse `~/.condarc` itself"). |
| `channel_priority` | `ChannelPriorityMode` (`Strict` \| `Flexible` \| `Disabled`) | Mirrors `condarc::ChannelPriority` exactly (same 3 variants, both `#[non_exhaustive]`) — not `rattler_solve::ChannelPriority`'s own 2-variant enum, so a real `.condarc` value is representable at this feature's boundary without lossy translation. The `Flexible → rattler_solve::ChannelPriority` mapping is this feature's own internal problem to solve (see `research.md`), not something `ChannelConfig`'s shape should paper over. |
| `allowed_channels` | `Vec<String>` | Mirrors `allowlist_channels` (alias `whitelist_channels`). Empty ⇒ no allowlist restriction (matches `condarc::Config`'s `None`-is-absent convention: an empty `Vec` here means "the caller resolved no allowlist," not "allow nothing"). |
| `denied_channels` | `Vec<String>` | Mirrors `denylist_channels`. Checked before `allowed_channels`; a channel present in both is denied. |

**Defaults-channel fallback (FR-015, new — see `spec.md`'s Session
2026-07-28 clarification)** happens first, before allow/deny filtering: if
`channels` is empty, substitute a single fallback entry, `ChannelSpec {
url_or_name: "defaults".to_string() }`, and proceed as if the caller had
supplied it. This is `channels.rs`'s own fixed, built-in constant — the
same category of decision as `DEFAULT_PACKAGES` (FR-005) — not a
`.condarc`-derived value; it never touches `channel_priority`/
`allowed_channels`/`denied_channels`, which stay exactly as supplied.

**Allow/deny enforcement** happens as a pure pre-processing step, after
that fallback substitution, before solving (`solve.rs`): filter the
(possibly-substituted) `channels` to the *effective* list (drop any
channel in `denied_channels`, or — if `allowed_channels` is non-empty —
not in `allowed_channels`), then proceed with that effective list. This
reuses the existing `NoChannelsConfigured` category rather than adding a
new one: if the effective list is empty — which, now that an
originally-empty `channels` is substituted with `defaults` first, can
only happen because allow/deny filtering removed every entry (including,
potentially, the substituted `defaults` entry itself) — creation fails
with `NoChannelsConfigured`. This slightly broadens that category's
documented meaning ("no channels remain to solve against, once the
defaults fallback and allow/deny filtering have both run," not "the
caller configured literally zero") — intentional, to stay inside FR-010's
closed category set for this ticket rather than adding a
`ChannelNotPermitted` variant now.

**Known limitation, accepted for this ticket**: this comparison is a
plain string match against `denied_channels`/`allowed_channels`, with no
URL canonicalization (scheme/host/port/trailing-slash/case normalization).
Two spellings of the same channel that a caller's allow/deny list doesn't
write identically will not be recognized as the same channel. `ChannelConfig`
is accepted as an already-resolved input from a separate capability (per
spec Assumptions), not raw user text this feature itself must defend
against adversarially — if that capability's own resolution step someday
needs stronger equivalence guarantees, that's its job to add, not a gap
this ticket's own comparison logic needs to close now.

`ChannelSpec { pub url_or_name: String }` — a single resolved channel
reference. No `requires_auth` field (removed from the first draft — see
Scope note above).

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPriorityMode {
    Strict,
    Flexible,
    Disabled,
}
```

## `RequestedPackages`

Represents the spec's **Requested Package Set** key entity and the
explicit-vs-default-vs-override precedence rule (User Story 3 Acceptance
Scenario 3).

```rust
#[derive(Debug, Clone)]
pub enum RequestedPackages {
    Explicit(Vec<PackageSpec>),        // non-empty: wins outright, never merged with defaults/override. Empty: treated as UseDefaultOrOverride (see below) — not a separate "explicit zero" case.
    UseDefaultOrOverride,               // caller supplied none; resolves at call time
}

impl RequestedPackages {
    /// Convenience constructor for callers translating a raw,
    /// possibly-empty CLI/user-supplied package list: maps an empty list
    /// to `UseDefaultOrOverride` and a non-empty list to `Explicit(_)`.
    /// Not required for correctness (`effective_packages()` treats
    /// `Explicit(vec![])` identically to `UseDefaultOrOverride` either
    /// way — see below), but saves callers from writing that `is_empty()`
    /// check themselves. GEN-25's CLI layer should still prefer this over
    /// constructing `RequestedPackages` variants directly, purely for
    /// convenience.
    pub fn from_cli(packages: Vec<String>) -> Result<Self, InvalidPackageSpec> {
        if packages.is_empty() {
            return Ok(Self::UseDefaultOrOverride);
        }
        packages
            .into_iter()
            .map(|package| PackageSpec::parse(&package))
            .collect::<Result<Vec<_>, _>>()
            .map(Self::Explicit)
    }
}
```

Resolution (`fn effective_packages(requested, override_config) ->
Vec<PackageSpec>`) is a pure function: `UseDefaultOrOverride` resolves to
the configured override if non-empty, else the built-in `DEFAULT_PACKAGES`
constant (FR-005/FR-006, including "an override that itself resolves to
an empty set is treated the same as no override"). `Explicit(packages)`
wins outright **when `packages` is non-empty**; `Explicit(vec![])` is
treated identically to `UseDefaultOrOverride` (corrected in the third
review cycle — see below), so the default/override resolution above
applies to it too.

**`Explicit(vec![])` corrected to match spec.md, not defended as a
separate intentional path (reversing the second review cycle's stance)**:
the second fix pass argued `from_cli([])` mapping to `UseDefaultOrOverride`
while a directly-constructed `Explicit(vec![])` produced a genuinely
empty environment were "two different, individually-consistent entry
points, not a contradiction." On closer reading of spec.md, that
distinction is **not one the spec itself draws**: FR-005 and the
Requested Package Set key entity both describe behavior purely in terms
of outcome — "the caller requests no explicit packages" — with no
carve-out for *how* that caller-supplied emptiness was constructed.
Allowing `Explicit(vec![])` to bypass defaults was therefore a real,
spec-violating landmine, not a legitimately distinct code path. The fix
is at the point of consumption, not the constructor: `effective_packages()`
itself treats an empty `Explicit` list exactly like
`UseDefaultOrOverride`, so the outcome is identical regardless of which
`RequestedPackages` variant — or which constructor — a caller used to get
there. `RequestedPackages::from_cli` remains useful as a convenience (it
still saves a caller from writing `if packages.is_empty() { ... }`
themselves, and it's still the right choice for translating a raw,
possibly-empty CLI list), but it's no longer *load-bearing* for
correctness — bypassing it and constructing `Explicit(vec![])` directly
now produces the same, spec-compliant result.

`PackageSpec` is a thin newtype wrapper over
`rattler_conda_types::MatchSpec`'s string form (e.g. `"numpy>=1.20"`), kept
as an opaque `String` newtype at this feature's own public-API boundary
(parsed into a real `MatchSpec` only internally, in `solve.rs`) — again to
avoid leaking `rattler` types across the public boundary.

```rust
impl PackageSpec {
    /// Parses a raw match-spec string (e.g. `"numpy>=1.20"`) into an
    /// opaque `PackageSpec`. `pub` — needed by any external caller
    /// constructing `default_override: Option<Vec<PackageSpec>>` for
    /// `create_ephemeral_environment`, not only by `RequestedPackages::from_cli`
    /// internally (the first draft of this contract defined this method
    /// but never marked it `pub` or listed it in the public API surface).
    ///
    /// # Errors
    /// Returns `InvalidPackageSpec` if the input isn't a syntactically
    /// valid match spec — a constructor-time input-shape check, not a
    /// runtime creation/install/teardown failure, so this is not part of
    /// `EphemeralEnvError`'s FR-010 category set.
    pub fn parse(input: &str) -> Result<Self, InvalidPackageSpec>;
}
```

## `EphemeralEnvironmentHandle`

Represents the spec's Ephemeral Environment key entity. Returned the
moment creation is *requested* (FR-001), not only on successful completion,
so it can carry a teardown signal even mid-creation.

| Field | Type | Notes |
|---|---|---|
| `id` | `EnvironmentId` (newtype over `ulid::Ulid`) | Distinct per environment; the identifying value FR-013/SC-008 requires on every structured event for this environment's lifecycle. |
| (internal) lifecycle state | `Arc<Mutex<LifecycleState>>` | Not a public field — shared so a `signal_teardown()` call and the in-flight creation task see the same state. Governs *teardown* progress only (see the correction below); it is not `await_ready()`'s source of truth. |
| (internal) creation outcome | `Arc<CreationOutcomeCell>` — a small wrapper type this feature defines itself, **not** a bare `tokio::sync::OnceCell` | Populated exactly once, the instant creation resolves one way or the other; `await_ready()` reads it, never `LifecycleState` directly. See `CreationOutcomeCell` immediately below for the full definition and why a bare `OnceCell` doesn't work. |
| (internal) cleanup guard | `Arc<CleanupGuard>` (`cleanup.rs`) | **New in this revision, closes a real gap a review found**: owns this environment's `OwnerLock` (from `orphan.rs`'s `publish_environment()`), its `VerifiedRoot` anchor, and its `EnvironmentId`, for as long as *either* this handle *or* the `ReadyEnvironment` it eventually produces (see `ReadyEnvironment`'s own `keep_alive` field below — the same `Arc`, cloned) is alive. Constructed by `create_ephemeral_environment()`'s own wiring immediately after `publish_environment()` returns, *before* any async solve/install work begins — never left as a bare local variable in that function's own call frame, which the compiler would be free to drop (releasing the `OwnerLock` early) the instant that frame ends, making a live, still-installing environment look orphaned to a concurrent reclamation scan. This is the type whose `Drop` best-effort-removes the prefix directory (see `CleanupGuard`'s own description below) — firing only once every clone of this `Arc` (both the handle's own and every `ReadyEnvironment` clone's) has gone out of scope, never while either one is still held — and, as of this revision, the type responsible for the `OwnerLock`'s own lifetime — the lock is only ever released as a side effect of this guard's own removal/drop path, never earlier. |
| (internal) effective packages | `Vec<PackageSpec>` | **New in this revision, closes a real observability gap a review found**: the effective top-level package set (FR-005/FR-006's resolved list) is resolved once, before publication, and stored here for the handle's own lifetime — every `EphemeralLifecycleEvent` this environment's *live-process* paths emit (create, install, and the explicit-signal/`Drop`/creation-failure teardown paths) reads this field for its own `packages` value rather than re-deriving or duplicating it. (An orphan-reclaimed environment's teardown event, emitted by a different, later process that never resolved this value itself, instead reads the same list back from the on-disk metadata file `orphan.rs`'s `publish_environment()` wrote — see `research.md`.) |

**`CreationOutcomeCell` — new in this revision, corrects a factual error
from an earlier draft**: that draft asserted `tokio::sync::OnceCell` has
a `wait()` method. It does not — confirmed against `tokio`'s own source
and issue tracker; the maintainers explicitly declined to add one (see
[tokio-rs/tokio#4788](https://github.com/tokio-rs/tokio/issues/4788)).
The correct, standard pattern for "wait until some other task populates
this value" — which a bare `OnceCell` cannot do on its own — pairs it
with a `tokio::sync::Notify` (both gated behind tokio's `sync` feature,
which must be added to `Cargo.toml` — see `research.md`):

```rust
/// Internal to this crate; not part of the public API. Populated exactly
/// once, the instant creation resolves one way or the other — a
/// successful `Ready`, or a `CreationFailed` whose `cleanup` has itself
/// reached `Succeeded`/`Failed(_)` — regardless of which `LifecycleState`
/// transition follows immediately afterward (see the correction below:
/// this must NOT be described as "populated when LifecycleState reaches
/// Ready", since the `CreatingTeardownQueued`+success case populates this
/// cell and then transitions `LifecycleState` straight to `TearingDown`,
/// skipping `Ready` entirely — see the new Creation-completion
/// transitions table further down).
struct CreationOutcomeCell {
    cell: tokio::sync::OnceCell<Result<ReadyEnvironment, CreationFailure>>,
    notify: tokio::sync::Notify,
}

impl CreationOutcomeCell {
    /// Called exactly once, by whichever code path first determines the
    /// creation outcome (see the population rule above).
    fn set(&self, outcome: Result<ReadyEnvironment, CreationFailure>) {
        let _ = self.cell.set(outcome);
        self.notify.notify_waiters();
    }

    /// `await_ready()`'s sole read path. Race-safe: subscribes to
    /// `notify` *before* the second `get()` check, so a `set()` that
    /// races between the first check and the subscription is never
    /// missed (the classic check-subscribe-check pattern `Notify`'s own
    /// docs recommend for exactly this "wait for a one-shot value" case).
    async fn wait(&self) -> Result<ReadyEnvironment, CreationFailure> {
        loop {
            if let Some(outcome) = self.cell.get() {
                return outcome.clone();
            }
            let notified = self.notify.notified();
            if let Some(outcome) = self.cell.get() {
                return outcome.clone();
            }
            notified.await;
        }
    }
}
```

`await_ready()` reads/awaits this cell via the `wait()` method above,
never `LifecycleState` directly — see the correction in the
`LifecycleState` section below for why that distinction matters.
Cloning: `ReadyEnvironment`/`CreationFailure` both already derive `Clone`
(see their own sections below), so `wait()` returns an owned, cloned
value each call — the cell itself is never consumed, so repeated
`await_ready()` calls on the same handle all resolve to the same
(cloned) value with no additional synchronization needed.

Methods (see `contracts/ephemeral_env_api.md` for full signatures):
- `signal_teardown(&self)` — never blocks the caller past enqueueing the signal.
- `await_ready(&self) -> Result<ReadyEnvironment, CreationFailure>` — resolves once creation completes (success) or fails; see `CreationFailure` below for why this is not a bare `EphemeralEnvError`. Resolves from the decoupled creation-outcome cell above, so it always reports the *original* creation outcome even if a teardown signal has since moved the handle on to `TearingDown`/`TornDown`.
- `await_torn_down(&self) -> Result<(), EphemeralEnvError>` — resolves once a signaled teardown completes.
- `reclamation_outcomes(&self) -> ReclamationStatus` — see `Orphan reclamation reporting` below.

## `ReadyEnvironment`

What a *successful* creation resolves to — "everything the caller needs...
at minimum, its resolved location" (FR-001).

```rust
#[derive(Debug, Clone)]
pub struct ReadyEnvironment {
    pub id: EnvironmentId,
    pub location: std::path::PathBuf,
    pub installed_packages: Vec<InstalledPackage>,
    // (internal, not `pub` — see the note immediately below)
    keep_alive: std::sync::Arc<CleanupGuard>,
}

#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub channel: String,
}
```

All three **public** `ReadyEnvironment` fields and all three
`InstalledPackage` fields are `pub` — `quickstart.md`'s example reads
them directly (`ready.location`, `pkg.name`, `pkg.version`), which
requires this explicitly, not just a prose description. `InstalledPackage`
is a thin projection of `rattler_conda_types::PackageRecord`, not the
full record, so `rattler` types never leak across the public API
boundary.

**`keep_alive` — new in this revision, closes a real use-after-drop
hazard a review found**: an earlier draft had only
`EphemeralEnvironmentHandle` hold the `Arc<CleanupGuard>` responsible for
this environment's eventual removal. But the fully-typed, documented
usage pattern this feature's own contract shows —
`handle.await_ready().await?` called directly on a temporary, with no
intermediate `let handle = ...;` binding kept around afterward — is
exactly the shape in which Rust drops that temporary `handle` value
(and, with it, the only `Arc<CleanupGuard>` reference that existed) the
moment the enclosing statement finishes, *before* the caller's next
statement ever gets to use the resulting `ReadyEnvironment` at all. That
would trigger `CleanupGuard`'s `Drop`-based best-effort removal
immediately, out from under a value the caller hasn't even used yet —
a real bug, not a hypothetical misuse. `ReadyEnvironment` now holds its
own clone of the same `Arc<CleanupGuard>`, so the guard's `Drop` impl
(which only fires once the `Arc`'s last strong reference disappears)
cannot run until *both* the handle (if still held) *and* every live
`ReadyEnvironment` clone have gone out of scope — which is also the
*correct* description of "cleanup completing at exit time" (FR-008)
regardless of which of the two the caller happens to still be holding
when their own scope ends. This does not weaken `signal_teardown()`'s
own explicit path at all: that path never relies on `Drop` — it spawns
removal directly and sets `CleanupGuard`'s "claimed" flag immediately
(see `CleanupGuard`'s own description below), so an explicit signal
still removes the environment right away even while a `ReadyEnvironment`
clone is still alive elsewhere; the later `Drop` of that clone simply
becomes the no-op "someone already claimed removal" case the guard's
own design already handles.

**GEN-25 forward-compatibility**: `ReadyEnvironment` also exposes a
method (not a stored field, to avoid computing it eagerly during
creation when most callers won't need it):

```rust
impl ReadyEnvironment {
    /// The environment-variable overlay (at minimum `PATH`, prepended
    /// with this environment's own `bin`/`Scripts` directory) a child
    /// process needs to actually run installed packages' executables —
    /// computed via `rattler_shell::activation::Activator` against
    /// `self.location`. GEN-25 needs this to run the pass-through command
    /// with correct `PATH`/env vars; without it, GEN-25 would have to
    /// re-derive activation itself, duplicating this feature's own
    /// knowledge of the prefix layout.
    ///
    /// Returns `ActivationError`, **not** `EphemeralEnvError` (corrected
    /// in this revision — the first draft incorrectly reused
    /// `EphemeralEnvError` here): computing the activation environment
    /// for an already-successfully-`Ready` environment is not a
    /// create/install/teardown operation, so an activation failure
    /// doesn't fit any of FR-010's five closed categories, and forcing
    /// it into one would be a category-string lie. `ActivationError` is
    /// its own small, single-purpose error type, outside FR-010's scope
    /// entirely.
    pub fn activation_environment(&self) -> Result<Vec<(String, String)>, ActivationError>;
}

/// Failure computing `ReadyEnvironment::activation_environment()` — not
/// part of `EphemeralEnvError`'s FR-010 category set (see that method's
/// doc comment for why).
pub struct ActivationError {
    pub message: String,
}
```

Adds `rattler_shell` to this feature's dependency list (see `research.md`).

## `CreationFailure`

**New in this revision** — the first draft's `await_ready() ->
Result<ReadyEnvironment, EphemeralEnvError>` could only carry one error,
but FR-010 requires the caller to receive *both* the original creation
failure *and* a distinct cleanup-failure indication when cleanup of a
partially-installed environment itself also fails. A bare
`EphemeralEnvError` cannot represent "two things went wrong"; this type
can.

```rust
#[derive(Debug, Clone)]
pub struct CreationFailure {
    /// Why creation itself failed (channel/package/verification/location).
    pub error: EphemeralEnvError,
    /// `None` if cleaning up the partially-installed environment
    /// succeeded (or there was nothing to clean up — e.g. the failure
    /// happened before any directory was created); `Some(_)` if that
    /// cleanup itself also failed. Always the `TeardownFailed` category
    /// when present.
    pub cleanup_error: Option<EphemeralEnvError>,
}

impl std::fmt::Display for CreationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.cleanup_error {
            None => write!(f, "{}", self.error),
            Some(cleanup) => write!(f, "{} (cleanup also failed: {cleanup})", self.error),
        }
    }
}

impl std::error::Error for CreationFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
```

`CreationFailure` implementing `Display`/`Error` (new in this revision —
the first draft omitted this) is required for `quickstart.md`'s manual
smoke test to compile as written: `handle.await_ready().await?` inside a
function returning `Result<(), Box<dyn std::error::Error>>` needs the `?`
operator's error type to implement `std::error::Error`. Note
`CreationFailure` itself does **not** implement `CategorizedError`/
`category()` — only its `error`/`cleanup_error` fields (each a plain
`EphemeralEnvError`) do; a caller rendering a `CreationFailure` for
display picks whichever of its one or two categories it needs (plan.md's
Constitution Check row III previously worded this ambiguously as
"`EphemeralEnvError`/`CreationFailure` with a fixed `category()`," which
read as if `CreationFailure` itself had one — corrected there too).

`await_ready(&self) -> Result<ReadyEnvironment, CreationFailure>` (updated
from the first draft's bare `EphemeralEnvError`). `await_torn_down`'s
signature is unchanged (`Result<(), EphemeralEnvError>`) — tearing down an
already-`Ready` environment has no parallel "creation error" to pair with,
so the dual-failure case doesn't apply there.

## `LifecycleState` (internal, not part of the public API surface)

**Corrected from the first draft, and further corrected in this
revision**: the original enum had no defined `signal_teardown()`
transition while in a creation-failed state, and conflated "cleanup
running" with "cleanup complete" in one `Failed` variant, and conflated
"teardown succeeded" with "teardown failed" in one `TornDown` variant.
The first fix pass addressed the *transition* gaps but missed that
`Ready`/`CreationFailed` still didn't actually *carry* the data
`await_ready()` needs to return — there was nowhere for the successful
`ReadyEnvironment`, or the original `EphemeralEnvError` behind a creation
failure, to actually live. This revision carries both:

```rust
enum LifecycleState {
    Creating,
    CreatingTeardownQueued,
    Ready(ReadyEnvironment),
    TearingDown,
    TornDown(TeardownOutcome),
    CreationFailed { error: EphemeralEnvError, cleanup: CleanupOutcome },
}

enum TeardownOutcome { Succeeded, Failed(EphemeralEnvError) }
enum CleanupOutcome { Running, Succeeded, Failed(EphemeralEnvError) }
```

**`await_ready()` never reads `LifecycleState` directly — it reads the
`CreationOutcomeCell` above.** The rest of this section describes the
*conditions under which that cell gets populated*, expressed in terms of
`LifecycleState`'s own transitions (since that's what the creation task
and the cleanup task actually observe) — not a description of what
`await_ready()` itself inspects. `CreationFailed { cleanup: Running, .. }`
is explicitly **not yet a populate-the-cell condition (corrected in the
third review cycle)** — the first fix pass's wording ("populate once
it's no longer `Creating`/`CreatingTeardownQueued`") was still wrong: it
would have populated the cell the instant creation failed, even while
cleanup of the partial environment was still in flight, recording
`cleanup_error: None` purely because cleanup hadn't finished yet — not
because it had actually succeeded. That's a real violation of FR-010: a
cleanup failure discovered *after* the cell was already populated would
have nowhere to go, since a `OnceCell` can only be set once. The cell is
populated only once `cleanup` reaches `Succeeded` or `Failed(_)`:

- `Ready(env)` → cell populated with `Ok(env.clone())` (this remains
  genuinely terminal — once installation succeeds, nothing further
  changes this outcome).
- `CreationFailed { error, cleanup: Succeeded }` → cell populated with
  `Err(CreationFailure { error: error.clone(), cleanup_error: None })`.
- `CreationFailed { error, cleanup: Failed(e) }` → cell populated with
  `Err(CreationFailure { error: error.clone(), cleanup_error: Some(e.clone()) })`.
- `CreationFailed { cleanup: Running, .. }` → **cell not populated yet**;
  whichever task is running cleanup populates it once `cleanup`
  transitions `Running` → `Succeeded`/`Failed(_)`.
- `CreatingTeardownQueued`, on a **successful** creation outcome → cell
  populated with `Ok(env.clone())` **at the same moment**, even though
  `LifecycleState` itself does not pass through `Ready` on this path — it
  transitions directly to `TearingDown` instead (see the
  Creation-completion transitions table further down). Populating the
  cell is tied to *creation resolving*, never to *`LifecycleState` literally
  equalling `Ready`* — that distinction is exactly what the third
  correction below exists to fix.

(`Creating`/`TearingDown`/`TornDown(_)` are the other
non-populate-conditions/already-resolved-once states; documented here for
completeness, not as new caller-visible behavior.)

**Third correction (renumbered from "Fourth" for narrative clarity —
same substance)**: an earlier draft only handled the plain
`Ready(_) → TearingDown` case above and missed that `Ready(_)`'s own
`signal_teardown()` transition (see the transition table below) moves
`LifecycleState` on to `TearingDown` — which carries no `ReadyEnvironment`
payload — the instant `signal_teardown()` is called on an already-`Ready`
handle. A caller that calls `await_ready()` *after* that transition has
already happened (a legitimate ordering — nothing prevents a caller from
signaling teardown before ever awaiting readiness) would, if
`await_ready()` read `LifecycleState` directly, find no terminal-state
case left to resolve against. The fix: the moment the creation task
itself determines the outcome
(`Ready(env)`, or `CreationFailed` with `cleanup` at `Succeeded`/`Failed(_)`),
it writes that outcome into `EphemeralEnvironmentHandle`'s separate
`creation outcome` cell (see that struct's own field table above) as part
of the *same* critical section that updates `LifecycleState` — so no
`signal_teardown()` call can observe `Ready(_)` and start a
`Ready → TearingDown` transition before the outcome cell is already
populated. `await_ready()` reads/awaits only that cell from then on,
never `LifecycleState`. This makes the outcome permanently available
regardless of how many teardown signals arrive afterward, and regardless
of how many states `LifecycleState` itself moves through subsequently.

`signal_teardown()`'s complete transition table (every state handled):

| Current state | On `signal_teardown()` | SC-007 branch |
|---|---|---|
| `Creating` | → `CreatingTeardownQueued` | starts the (queued) attempt |
| `CreatingTeardownQueued` | unchanged | folds in (already queued) |
| `Ready(_)` | → `TearingDown` (the `ReadyEnvironment`'s `location` is captured for the removal step before the state transitions away from it) | starts the attempt |
| `TearingDown` | unchanged | folds in (already in progress) |
| `TornDown(_)` | unchanged | no-op (already completed) |
| `CreationFailed { cleanup: Running, .. }` | unchanged | folds in — cleanup from the creation failure *is* this environment's sole removal attempt, already in progress via another path |
| `CreationFailed { cleanup: Succeeded \| Failed(_), .. }` | unchanged | no-op — already completed via another path |

`await_torn_down()` resolves once `TornDown(_)` is reached (mapping
`Succeeded → Ok(())`, `Failed(e) → Err(e)`), or immediately if the
handle is already in `CreationFailed { cleanup: Succeeded, .. }` (→
`Ok(())`) or `CreationFailed { cleanup: Failed(e), .. }` (→ `Err(e.clone())`)
— that cleanup attempt *was* this environment's teardown in either case;
no second attempt is ever started, matching FR-012.

**Creation-completion transitions (new in this revision — closes a real gap
the table above doesn't cover: it only describes `signal_teardown()`'s own
transitions, not what the creation task itself does when it finishes)**:

| Current state when creation finishes | On success | On failure |
|---|---|---|
| `Creating` | → `Ready(env)` (populate the creation-outcome cell with `Ok(env)` first, in the same critical section) | → `CreationFailed { error, cleanup: Running }`, cleanup starts immediately (unconditional — a creation failure always cleans up regardless of any teardown signal) |
| `CreatingTeardownQueued` | Populate the creation-outcome cell with `Ok(env)` (so `await_ready()` still reports the real, successful outcome), then transition **directly to `TearingDown`** — skipping the `Ready` resting state entirely — and start the already-queued removal immediately, per FR-012's "cleanup runs automatically without requiring the caller to re-signal" | Identical to the `Creating` failure row above — a queued teardown signal changes nothing about the failure path, since cleanup already runs unconditionally on any creation failure |

This is the mechanism that actually implements FR-012's Acceptance Scenario 5 ("the teardown request is queued and the environment is cleaned up automatically as soon as creation completes ... the in-progress install is not interrupted") — the `signal_teardown()` table above only records *that* a teardown was requested (`Creating → CreatingTeardownQueued`); this table is what actually *acts* on that record once creation itself finishes.

## Orphan reclamation reporting

`reclaim_orphaned_environments()` (free function — see
`contracts/ephemeral_env_api.md` for its `Result`-wrapped signature,
changed in this revision to represent a scan that couldn't even start,
distinct from a scan that started and found nothing) runs automatically as the first step
of `create_ephemeral_environment`. **Corrected from the first draft**: its
results were previously discarded by that automatic call, with no way for
the caller to learn what it found — violating FR-008's "a failed
reclamation attempt MUST be reported as a distinct signal... separate
from that new creation's own outcome." `EphemeralEnvironmentHandle` now
exposes:

```rust
impl EphemeralEnvironmentHandle {
    /// Non-blocking. The automatic reclamation scan this handle's own
    /// creation request triggered (FR-008) — never blocks or fails this
    /// handle's own creation outcome (`await_ready`), which succeeds or
    /// fails entirely independently.
    ///
    /// Returns `ReclamationStatus::Scanning` while the scan is still
    /// running (corrected in this revision — the first draft's plain
    /// `Vec<OrphanReclamationOutcome>` return type couldn't distinguish
    /// "still scanning" from "scanned, found nothing," so a caller
    /// polling too early could wrongly conclude reclamation found no
    /// orphans when it simply hadn't finished yet). Once the scan
    /// completes, returns `ReclamationStatus::Complete(outcomes)` — from
    /// then on, repeated calls keep returning the same `Complete(_)`
    /// value. Returns `ReclamationStatus::Failed(error)` — **new in this
    /// revision, closes a real gap a security review found**: if the
    /// scan itself cannot even *start* (e.g. `envs/.root.lock` or the
    /// root directory itself fails the same secure-open/verify sequence
    /// `paths.rs` requires — a symlinked or wrong-owner root, or a
    /// genuine I/O error), that is categorically different from "scanned
    /// zero leftover directories": the former is this feature's own
    /// `UnwritableLocation`, the latter is `Complete(vec![])`. Silently
    /// returning `Complete(vec![])` for a root-level failure would let a
    /// caller wrongly conclude "no orphans exist" when in fact reclamation
    /// never actually ran at all — exactly the kind of undetected-orphan
    /// risk FR-008 exists to prevent. `Failed(_)` is terminal, same as
    /// `Complete(_)`: once reached, repeated calls return the same value.
    pub fn reclamation_outcomes(&self) -> ReclamationStatus;
}

pub enum ReclamationStatus {
    Scanning,
    Complete(Vec<OrphanReclamationOutcome>),
    Failed(EphemeralEnvError),
}
```

## `EphemeralEnvError`

The FR-010 fixed, exhaustive category set — implemented the same way the
existing `AllezError` already is (`src/error.rs`): one enum, one
`category()` match, `Display` for the human-readable message.

```rust
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EphemeralEnvError {
    NoChannelsConfigured,
    UnresolvablePackage { package: String },
    IntegrityVerificationFailed { package: String },
    UnwritableLocation,
    TeardownFailed,
}
```

**Corrected from the first draft**: now `#[non_exhaustive]`. The original
rationale for omitting it — "so every current match doesn't silently
compile through an unknown future variant" — doesn't actually cost
anything here: `#[non_exhaustive]` only forces a wildcard arm on matches
*outside* the defining crate. Every match against this type today lives
inside `allez`'s own crate, so internal exhaustiveness checking is
completely unaffected; only a future external consumer (were one ever to
exist) would need a wildcard arm. This also resolves a direct
contradiction the first draft had: spec.md's own FR-010 says a later
ticket "MAY add further category values as an additive, non-breaking
extension" — which a non-`#[non_exhaustive]` public enum cannot honor
(adding a variant would break every external `match`). It also now
matches the precedent GEN-36 already established in this repo
(`condarc::Config`/`ChannelPriority`/etc. are all `#[non_exhaustive]`).

Every URL-bearing value that could end up embedded in this type's fields
(currently none do directly, but see `EphemeralLifecycleEvent` below) is
passed through `channels::redact_channel_url()` first — see that section.

## `EphemeralLifecycleEvent`

The FR-013 structured-observability payload, emitted via this feature's
own `tracing` events (through the existing `src/observability.rs`
subscriber — no second logging pipeline).

| Field | Type | Notes |
|---|---|---|
| `schema_version` | `&'static str` | Reuses the same versioning convention as `output::SCHEMA_VERSION` (a sibling constant, not the identical value). |
| `environment_id` | `EnvironmentId` | Same identifier across every event for one environment's lifecycle (FR-013/SC-008). |
| `operation` | `"create"` \| `"install"` \| `"teardown"` | Fixed set. |
| `packages` | `Vec<String>` | The *effective* top-level package(s), per `effective_packages()` above — not the raw request. |
| `duration_ms` | `u64` | |
| `outcome` | `"success"` \| `"failure"` | |
| `failure_category` | `Option<&'static str>` | `Some(EphemeralEnvError::category())` when `outcome == "failure"`, else `None`. |

**Credential-leak guard (new in this revision; broadened in the second
review cycle)**: nothing above carries a raw channel URL today (channels
aren't part of this event shape), but as a defensive measure against a
future field addition or an upstream mistake in whatever produces
`ChannelConfig`, any function in this feature that ever formats a
`ChannelSpec`/URL into a `String` destined for an error message or a
tracing field MUST route it through `channels::redact_channel_url(url:
&str) -> String` first (strips userinfo — `user:pass@` — and any
`/t/<token>/`-style conda-token path segment). **This guard now also
covers wrapping any upstream `rattler`-originated error's own `Display`
text** before it's embedded in an `EphemeralEnvError`'s message or an
`EphemeralLifecycleEvent` field, not only this feature's own
channel-formatting code — `rattler`'s own error types may themselves
embed a full request URL (e.g. in a network-failure message), and this
feature has no control over what that text contains. Since this ticket
adds no authentication of any kind, no URL this feature constructs
should ever carry real credentials by construction — but a caller-supplied
`ChannelSpec.url_or_name` could still already contain embedded
credentials before this feature ever sees it (e.g. a mistake in a future
GEN-23 resolution step), so this remains defense-in-depth, not a
currently-exercised code path. This keeps FR-013's "MUST NOT contain
credential material" true by construction rather than by the current
accident of no field carrying a URL yet. **This guard's scope also
explicitly covers `InstalledPackage.channel`** (see `ReadyEnvironment`
above): it is populated from `PackageRecord`'s own channel string, which
ultimately traces back to the same caller-supplied `ChannelSpec.url_or_name`
— so it MUST be routed through `redact_channel_url()` in `install.rs`
before being projected into `InstalledPackage`, not left as a
separate, unguarded public output that happens to carry the same
underlying value.

Emitted as a single `tracing::info!`/`tracing::error!` call per step with
these as structured fields (not string-interpolated), consistent with
Constitution XI.

## Relationships

```text
EphemeralEnvironmentHandle 1---1 LifecycleState (internal, shared/interior-mutable)
EphemeralEnvironmentHandle 1---1 EnvironmentId
EphemeralEnvironmentHandle 1---1 ReclamationStatus (from its own creation's automatic scan)
EphemeralEnvironmentHandle 1---0..1 Result<ReadyEnvironment, CreationFailure> (the decoupled creation-outcome cell `await_ready()` reads — populated once, independent of later LifecycleState transitions)
ReadyEnvironment            *---1 EnvironmentId (same value as its handle)
ReadyEnvironment            1---0..1 ActivationError (on-demand, not stored — see activation_environment())
CreationFailure             1---1 EphemeralEnvError (the original failure)
CreationFailure             0..1---1 EphemeralEnvError (the distinct cleanup failure, if any)
ChannelConfig               1---* ChannelSpec (ordered)
RequestedPackages::Explicit *---* PackageSpec
EphemeralLifecycleEvent     *---1 EnvironmentId (correlates events to one environment)
EphemeralEnvError           1---1 category (via `category()`, 1:1, no drift)
```

No persistent storage: every type above is in-memory/on-disk-as-a-side-effect
only (the prefix directory itself); nothing is written to a database or
config file by this feature (matches spec Assumptions: "not cached, reused,
or referenced by name across separate operations" — this does **not**
extend to the shared package/repodata cache, which is deliberately
long-lived; see `research.md` § Ephemeral location + package-download cache).
</content>
