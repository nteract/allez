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

## `create_ephemeral_environment` (plain async function — no handle type)

**Rewritten in full (see the "Explicit reap, no automatic reaping"
decision) — supersedes every `EphemeralEnvironmentHandle`/`LifecycleState`/
`CreationOutcomeCell`/`ReclamationStatus` design this section previously
described.** Automatic teardown is removed entirely: there is no longer a
reference returned before creation completes, no way to signal teardown
for a single environment, no internal state machine, and no orphan
detection. Representing the spec's Ephemeral Environment key entity no
longer needs a handle type at all — creation is a plain, directly-awaited
async function that resolves once, to one of two outcomes:

```rust
pub async fn create_ephemeral_environment(
    requested: RequestedPackages,
    channels: ChannelConfig,
    default_override: Option<Vec<PackageSpec>>,
) -> Result<ReadyEnvironment, CreationFailure>;
```

Calling this function performs the entire create → solve → install
sequence and resolves directly to `Ok(ReadyEnvironment)` on success or
`Err(CreationFailure)` on failure — there is nothing further for the
caller to await, poll, or signal. `EnvironmentId::new()` is generated once
at the start of the call and threaded through every step (including a
failed one), so it is the identifying value every `EphemeralLifecycleEvent`
this call emits carries (FR-013/SC-008), and the value a `CreationFailure`
reports even though no `ReadyEnvironment` was ever produced (see
`CreationFailure` below).

If creation fails partway through (an unresolvable package, a failed
integrity check, and so on), this function still rolls back whatever
partial directory it created for that attempt before returning
`Err(CreationFailure)` — synchronously, as part of the same call, not via
a background task or a `Drop` guard — reporting a distinct
`cleanup_error` if that rollback itself also fails (FR-004/FR-010). This
rollback behavior is unaffected by the "Explicit reap, no automatic
reaping" decision: it is unrelated to tearing down a *successfully
created* environment, which this function never does under any
circumstance. A successful `ReadyEnvironment` this function returns is
never removed by this function, by anything it spawns, or by anything
that runs when the caller's own reference to it is dropped — see
`ReadyEnvironment` below and [`reap_ephemeral_environments`](#reapoutcome-and-reap_ephemeral_environments)
for the only way one is ever removed.

## `ReadyEnvironment`

What a *successful* creation resolves to — "everything the caller needs...
at minimum, its resolved location" (FR-001).

```rust
#[derive(Debug, Clone)]
pub struct ReadyEnvironment {
    pub id: EnvironmentId,
    pub location: std::path::PathBuf,
    pub installed_packages: Vec<InstalledPackage>,
}

#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub channel: String,
}
```

All three `ReadyEnvironment` fields and all three `InstalledPackage`
fields are `pub` — `quickstart.md`'s example reads them directly
(`ready.location`, `pkg.name`, `pkg.version`), which requires this
explicitly, not just a prose description. `InstalledPackage` is a thin
projection of `rattler_conda_types::PackageRecord`, not the full record,
so `rattler` types never leak across the public API boundary.

**No cleanup guard, no `keep_alive` field (Explicit reap, no automatic
reaping decision — supersedes the use-after-drop fix an earlier revision
of this section described in detail)**: an earlier revision had
`ReadyEnvironment` hold a private `Arc<CleanupGuard>` clone specifically
so dropping a temporary `EphemeralEnvironmentHandle` (in the
`handle.await_ready().await?`-on-a-temporary usage pattern that contract
once documented) couldn't trigger a premature `Drop`-based removal of the
value the caller hadn't used yet. Both the hazard and the fix it
motivated no longer apply: there is no handle type any more (see
`create_ephemeral_environment` above), no `CleanupGuard`, and no
`Drop`-triggered removal of a successfully created environment at all.
Dropping every clone of a `ReadyEnvironment` — or never holding one in
the first place — has no effect whatsoever on the environment's
directory; it persists exactly as created until a caller explicitly
invokes [`reap_ephemeral_environments`](#reapoutcome-and-reap_ephemeral_environments).

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
    /// Returns `ActivationError`, **not** `EphemeralEnvError`: computing
    /// the activation environment for an already-successfully-`Ready`
    /// environment is not a create/install/reap operation, so an
    /// activation failure doesn't fit any of FR-010's closed categories,
    /// and forcing it into one would be a category-string lie.
    /// `ActivationError` is its own small, single-purpose error type,
    /// outside FR-010's scope entirely.
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

Represents a failed creation attempt — `create_ephemeral_environment`'s
`Err` variant. FR-010 requires the caller to receive *both* the original
creation failure *and* a distinct cleanup-failure indication when
rolling back a partially-installed environment itself also fails; a bare
`EphemeralEnvError` cannot represent "two things went wrong," so this
type carries both.

```rust
#[derive(Debug, Clone)]
pub struct CreationFailure {
    /// The environment identifier this failed attempt would have used —
    /// present so a caller/test can correlate this failure with the
    /// `EphemeralLifecycleEvent`s this attempt still emitted (FR-013),
    /// even though no `ReadyEnvironment` was ever produced.
    pub id: EnvironmentId,
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

`CreationFailure` implementing `Display`/`Error` is required for
`quickstart.md`'s manual smoke test to compile as written:
`create_ephemeral_environment(...).await?` inside a function returning
`Result<(), Box<dyn std::error::Error>>` needs the `?` operator's error
type to implement `std::error::Error`. Note `CreationFailure` itself does
**not** implement `CategorizedError`/`category()` — only its
`error`/`cleanup_error` fields (each a plain `EphemeralEnvError`) do; a
caller rendering a `CreationFailure` for display picks whichever of its
one or two categories it needs.

**No `LifecycleState`, no `EphemeralEnvironmentHandle`, no per-environment
teardown signal, and no orphan-reclamation reporting (Explicit reap, no
automatic reaping decision — supersedes this section's own prior content
in full)**: earlier revisions of this data model described a substantial
internal state machine here (`LifecycleState`'s `Creating`/
`CreatingTeardownQueued`/`Ready`/`TearingDown`/`TornDown`/
`CreationFailed` variants, a `signal_teardown()` transition table, and
`EphemeralEnvironmentHandle::reclamation_outcomes() -> ReclamationStatus`
backed by an automatic `reclaim_orphaned_environments()` scan run at the
start of every creation call) needed to reconcile a caller-initiated
teardown signal, a background orphan-reclamation scan, and an in-progress
creation all racing against one another. None of that exists any more.
`create_ephemeral_environment` (see above) is a single, directly-awaited
`async fn` with no intermediate state to expose: it resolves exactly
once, to `Ok(ReadyEnvironment)` or `Err(CreationFailure)`, and that is
the entire lifecycle this data model needs to represent for creation.
Removing a *successfully created* environment is handled by a completely
separate, unrelated function — see `ReapOutcome` below — that has no
notion of "in progress," "queued," or "still active" at all.

## `ReapOutcome` and `reap_ephemeral_environments`

**New (Explicit reap, no automatic reaping decision)** — the entire
removal side of this feature's public API, replacing every
`EphemeralEnvironmentHandle`/`LifecycleState`/orphan-reclamation type
this section previously described.

```rust
pub fn reap_ephemeral_environments() -> Result<Vec<ReapOutcome>, EphemeralEnvError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReapOutcome {
    /// The environment's directory was removed.
    Removed { id: EnvironmentId },
    /// The environment's directory could not be removed.
    RemovalFailed { id: EnvironmentId, error: EphemeralEnvError },
}
```

`reap_ephemeral_environments()` is synchronous (no `async`/`await` — it
performs its own blocking filesystem work directly, unlike
`create_ephemeral_environment`, which spawns its filesystem work onto
Tokio's blocking pool internally): it resolves the same
`$ALLEZ_EPHEMERAL_ROOT` (or fallback) root `create_ephemeral_environment`
uses, lists every entry under that root's `envs/` directory whose name
parses as an `EnvironmentId`, and calls the same anchored
`remove_prefix_dir()` primitive (`cleanup.rs`) the creation-failure
rollback path uses on each one in turn, collecting one `ReapOutcome` per
entry. There is no liveness check, no `.owner.lock`/`.root.lock` of any
kind, and no distinction between an environment that finished installing
long ago and one a concurrent `create_ephemeral_environment` call might
still be writing to — every entry found is removed unconditionally. A
removal failure for one entry (reported as `RemovalFailed`) does not stop
the loop or affect any other entry's own outcome (FR-009). An empty
`envs/` directory — whether because nothing was ever created, or because
a prior `reap_ephemeral_environments()` call already removed everything
— yields `Ok(vec![])`, not an error (FR-008's idempotency guarantee).
`Err(EphemeralEnvError::UnwritableLocation)` is returned only if the root
itself cannot be securely opened/verified — a scan that cannot even
start, distinct from `Ok(vec![])` ("scanned, found nothing").

Every `ReapOutcome` — success or failure — emits an
`EphemeralLifecycleEvent` with `operation: "teardown"` (see
`EphemeralLifecycleEvent` below; the operation name is unchanged from
when this feature had a signal-based teardown path, since it still
accurately describes "removing an environment," now performed only by
this function). Unlike a create/install event, a reap event has no way to
know what packages a given environment was originally installed with —
there is no metadata file recording that any more (that bookkeeping was
part of the orphan-reclamation machinery this decision removes) — so its
`packages` field is always an empty list; this is a deliberate
simplification, not an oversight.

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
create_ephemeral_environment -> Result<ReadyEnvironment, CreationFailure> (resolves once; no intermediate handle type)
ReadyEnvironment            *---1 EnvironmentId
ReadyEnvironment            1---0..1 ActivationError (on-demand, not stored — see activation_environment())
CreationFailure             1---1 EnvironmentId (the failed attempt's own identifier)
CreationFailure             1---1 EphemeralEnvError (the original failure)
CreationFailure             0..1---1 EphemeralEnvError (the distinct cleanup failure, if any)
ChannelConfig               1---* ChannelSpec (ordered)
RequestedPackages::Explicit *---* PackageSpec
EphemeralLifecycleEvent     *---1 EnvironmentId (correlates events to one environment)
EphemeralEnvError           1---1 category (via `category()`, 1:1, no drift)
ReapOutcome                 1---1 EnvironmentId (the removed, or failed-to-remove, environment)
reap_ephemeral_environments -> Vec<ReapOutcome> (one call, every environment currently on disk, processed independently)
```

No persistent storage: every type above is in-memory/on-disk-as-a-side-effect
only (the prefix directory itself); nothing is written to a database or
config file by this feature (matches spec Assumptions: "not cached, reused,
or referenced by name across separate operations" — this does **not**
extend to the shared package/repodata cache, which is deliberately
long-lived; see `research.md` § Ephemeral location + package-download cache).
There is no on-disk metadata file of any kind associated with an
environment any more (no `{pid, created_at, environment_id, packages}`
file, no `.owner.lock`, no `.root.lock`) — that bookkeeping belonged
entirely to the orphan-reclamation machinery the "Explicit reap, no
automatic reaping" decision removed; an environment's directory and its
installed packages are the only on-disk state this feature tracks.
</content>
