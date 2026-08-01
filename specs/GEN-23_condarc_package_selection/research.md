# Phase 0 Research: `.condarc` Channel Resolution

This ticket spans two codebase locations (spec.md Operating Context): a
new, additive `expand_channels` capability inside the `condarc` crate
(`crates/condarc`), and a new file-handling/adaptation module inside
`allez` itself. Neither needed external research — no new third-party
library, no unfamiliar protocol. Every open question was resolvable
directly from the spec, GEN-36's delivered crate, GEN-24's delivered
`ChannelConfig`, and `docs/condarc_research.md`'s verified conda default
constants. No "NEEDS CLARIFICATION" markers remain in Technical Context.

## R1 — `expand_channels()`'s input/output shape and location

**Decision**: A new private module, `crates/condarc/src/expand_channels.rs`,
exposing one pure function,
`expand_channels(config: &Config) -> Result<ResolvedChannels, ExpandChannelsError>`
(the `Result` is a later addition, R11), plus the supporting type
`ResolvedChannels`. Re-exported from `lib.rs` alongside the crate's
existing `pub use` list, following the same pattern `model` and `error`
already use.

**Rationale**: `Config` is already the crate's parsed representation
(GEN-36), deliberately `#[non_exhaustive]`/`Option`-shaped with no
defaulting layer (FR-038). `expand_channels()` is the additive layer
FR-002 requires on top of it, matching the "additive opt-in, never
alters `parse()`" precedent `ParseOptions.null_sequence_map_defaults`
already sets. Borrowing `&Config` lets a caller keep the original parsed
document after resolving. Named `expand_channels` rather than the
earlier, more generic `resolve` — a reviewer flagged `resolve` as giving
no clue what it covers; `expand_channels` names the function's core
identity (bare-name/`defaults` expansion) even after R12/R2 folded
allow/deny filtering and `channel_priority` passthrough into the same
function's scope.

**Alternatives considered**:
- *A method on `Config` itself* — rejected: the spec frames this as a
  layer on top of `parse()`/`Config`, not merged into it; a free
  function keeps `Config`'s surface exactly as GEN-36 delivered it.
- *A `ResolveOptions` parameter mirroring `ParseOptions`* — rejected:
  FR-001–FR-004/FR-005–FR-008 leave no caller-tunable behavior; adding
  one speculatively would violate Constitution VII (no unused
  configurability).

## R2 — Channel-priority legacy-boolean handling is already done by `parse()`

**Decision**: `expand_channels()` does not re-implement FR-003's legacy
boolean mapping (`true`→`flexible`, `false`→`disabled`). It only applies
the FR-002 default: `config.channel_priority.unwrap_or(ChannelPriority::Flexible)`.

**Rationale**: GEN-36's `catalog.rs`/`coerce/boolish.rs` already turn a
`.condarc` boolean into `Option<ChannelPriority>` before `expand_channels()`
sees it (`model.rs`'s own doc comment confirms this). Re-implementing
would duplicate GEN-36's coercion (Constitution IV) and risk drift. User
Story 3 Scenario 5 is covered by GEN-36's existing coercion tests plus one
resolution-level passthrough test.

**Alternatives considered**: Re-validating the boolean spelling inside
`expand_channels()` as defense-in-depth — rejected as needless
duplication; `Config` only ever contains the resolved 3-variant enum or
`None` by the time `expand_channels()` sees it.

## R3 — `channels`/`channel` and `allowlist_channels`/`whitelist_channels` alias collisions are already rejected by `parse()`

**Decision**: `expand_channels()` performs no alias-collision detection
of its own for FR-005. A document setting both spellings of either pair
never reaches `expand_channels()` — `parse()` already returns
`Err(ValidationReport)` via the existing `validate::alias_collision_entries`
pass (`catalog.rs` already declares `channels`/`"channel"` and
`allowlist_channels`/`"whitelist_channels"` as canonical/alias pairs).

**Rationale**: Matches FR-005 verbatim ("a `parse()`-level rejection...
a document with such a collision never reaches `expand_channels()` at
all"). Covered by GEN-36's existing `alias_collision` test suite, plus
one co-located unit test confirming `allez`'s FR-011 fallback path runs
when such a `.condarc` is fed through the full pipeline.

## R4 — Default constants (`channel_alias`, `default_channels`, `custom_channels`)

**Decision**: Three private constants in `expand_channels.rs`, sourced
directly from `docs/condarc_research.md` §5 (lines 493–497), not
re-derived:

```rust
const DEFAULT_CHANNEL_ALIAS: &str = "https://conda.anaconda.org";
#[cfg(not(windows))]
const DEFAULT_CHANNELS: &[&str] = &[
    "https://repo.anaconda.com/pkgs/main",
    "https://repo.anaconda.com/pkgs/r",
];
#[cfg(windows)]
const DEFAULT_CHANNELS: &[&str] = &[
    "https://repo.anaconda.com/pkgs/main",
    "https://repo.anaconda.com/pkgs/r",
    "https://repo.anaconda.com/pkgs/msys2",
];
const DEFAULT_CUSTOM_CHANNELS: &[(&str, &str)] = &[("pkgs/pro", "https://repo.anaconda.com")];
```

Each is used only when the corresponding `Config` field is `None`
(FR-002's table); an explicit, non-`None` value — including an explicit
empty list/map — is used exactly as given, never merged with these
built-ins.

**Rationale**: This is the one place `expand_channels()` is
platform-sensitive (`cfg(windows)`), a compile-time constant selection
rather than a runtime OS-detection call, so it doesn't compromise the
function's status as pure/hermetic (mirrors real conda's own narrow
`on_win` check).

**"No merge" is a deliberate spec simplification, not an invention**:
real conda's `MapParameter`/`SequenceParameter` loader does a more
nuanced multi-source search-path merge that this ticket's
single-document, `.condarc`-only scope was never asked to reproduce
(GEN-36's Target Platform note already scopes the crate this way).
FR-002's table is the authoritative contract here.

**Recorded decision — these values are conda-upstream's, not Anaconda's
own current recommendation**: Anaconda-internal signals (as of
2026-07-30) show `pkgs/r` and `pkgs/msys2` being end-of-serviced, and
Anaconda's own recommendation (CLI-744) diverges for the
`main-x`/premium-tier variants — `main` itself stays on the same URL
hardcoded above, so the divergence is partial. This ticket deliberately
keeps the conda-upstream values because (a) they are what real,
unconfigured conda actually falls back to, which is this ticket's stated
goal (spec.md FR-009/FR-011: "conda's own documented default channel
configuration," not Anaconda's product recommendation), and (b)
revisiting Anaconda's own recommendation is a separate, later product
decision. Recording it here makes it a decision this ticket owns, not a
silently inherited assumption.

**Reconciliation with GEN-24's own `DEFAULTS_CHANNEL_URL`**: GEN-24's
existing `channels_with_fallback` (`src/ephemeral/channels.rs`) already
defines its own single-URL `DEFAULTS_CHANNEL_URL` constant, used purely
as that function's own last-resort substitute for an *initially-supplied*
empty `ChannelConfig.channels` (GEN-24 FR-015) — a narrower, code-level
safety net with no `.condarc` awareness of its own. `DEFAULT_CHANNELS`
above is a different constant for a different purpose: it is what this
ticket's `expand_channels()` substitutes specifically for the `defaults`
placeholder when resolving real `.condarc` content, matching conda's own
documented multi-URL default. The two constants intentionally serve
different call sites and are never required to agree — GEN-24's own
function and constant are unmodified by this ticket (FR-016).

## R5 — Channel-entry resolution algorithm (FR-001) has two resolution modes, not one

**Decision**: Two private functions:

- `resolve_entry(entry, ctx) -> Result<Vec<String>, ExpandChannelsError>`
  — full precedence: (a) match against conda's own scheme pattern
  (`^[a-z][a-z0-9]{0,11}://`), used as-is on a match; (b) `custom_multichannels`
  lookup by `entry`'s own name (for the literal entry `"defaults"`, its
  own `custom_multichannels` key is checked first, per the walkthrough
  below); (c) `custom_channels`
  progressive-prefix match, joined as `base_url.trim_end_matches('/') + "/" + entry`
  (R6); (d) `channel_alias` join, `channel_alias.trim_end_matches('/') + "/" + entry`.
  Used for every top-level `channels`/`allowlist_channels`/`denylist_channels`
  entry. Returns a `Vec` because FR-001(b) expands a matched multichannel
  name to its members, so one entry can yield zero, one, or many outputs —
  (a)/(c)/(d) return a single-element vec, (b) returns the
  fully-expanded member list, each member resolved via `resolve_member`;
  branch (d) propagates `ExpandChannelsError::EmptyChannelAlias` (R11)
  when the effective `channel_alias` is an explicit empty string.
- `resolve_member(entry, channel_alias) -> Result<String, ExpandChannelsError>`
  — restricted precedence: (a) same scheme-pattern check, then straight
  to (d) the same `channel_alias` join, skipping (b) and (c). Used only
  for a `custom_multichannels` (including `defaults`) member's own
  value; propagates the same `EmptyChannelAlias` error under the same
  condition as `resolve_entry`'s own branch (d).

The scheme pattern matches `docs/condarc_research.md` §8 item 3's own
derivation of conda's `has_scheme()` regex, not re-derived. The
`channel_alias` join formula has no equivalent in that document — it is
this ticket's own construction, chosen to match R6's `custom_channels`
join exactly, so both branches share one join rule.

**Rationale**: Acceptance Scenario 5 is explicit that a multichannel
member "is resolved as an ordinary bare name via `channel_alias` — it is
not expanded further," even when it collides with another multichannel
or `custom_channels` entry name. A uniformly-recursive resolver would
over-expand this and risk an unbounded cycle on self-reference (a
multichannel naming itself as a member, also covered by Scenario 5); the
restricted `resolve_member` mode makes that cycle structurally
impossible instead of requiring a visited-set guard. How `resolve_entry`'s
multi-valued return composes with each list's iteration is spelled out
in data-model.md's "Composition" section.

**`"defaults"` substitution walkthrough** (ties R4/R5 together):
resolving `"defaults"` first checks `custom_multichannels` for a key
literally named `"defaults"` (FR-001(b)); if present, its members are
resolved via `resolve_member`. If absent, the effective `default_channels`
list (the user's own value if `Some`, else `DEFAULT_CHANNELS` from R4)
stands in, each entry likewise passed through `resolve_member` — so a
user-configured `default_channels` entry that is itself a bare name is
only URL-checked-or-alias-joined, never re-expanded through
`custom_channels`/`custom_multichannels`, exactly like any other
multichannel member. This is why the Concrete Channel Identifier key
entity can list its cases as exhaustive: a `default_channels` entry is
categorically a multichannel member, not a fourth case. The same
restricted-precedence treatment applies uniformly to every
`custom_multichannels` entry's members, not only `defaults`'s — there is
exactly one member-resolution rule, never a special case for the literal
name `"defaults"`.

**Alternatives considered**: Fully recursive resolution with a
visited-set guard against cycles — rejected: adds a data structure and a
new failure mode (what happens when the guard trips?) the spec never
asks for, to handle a case Scenario 5 already resolves more simply by
not recursing into (b)/(c) for members at all.

## R6 — `custom_channels` progressive-prefix match

**Decision**: For entry `entry`, try `entry` itself, then each successive
`/`-delimited prefix (`"acme/label/dev"` → `"acme/label"` → `"acme"`),
against the effective `custom_channels` map's keys — longest match wins
(checking `entry` first is already the longest possible match, so no
separate comparison is needed). On a hit against a key with base URL
`base_url`, the resolved URL is `base_url.trim_end_matches('/') + "/" + entry`
— the **original, full** entry text, not `entry` with the matched prefix
stripped (FR-001(c), Edge Cases table).

**Rationale**: Matches `docs/condarc_research.md`'s own worked example:
`DEFAULT_CUSTOM_CHANNELS = {"pkgs/pro": "https://repo.anaconda.com"}` —
resolving `"pkgs/pro"` must yield `"https://repo.anaconda.com/pkgs/pro"`
(base URL, which does not itself contain `pkgs/pro`, joined with the
full matched name), not the base URL alone.

## R7 — `allez` locates `~/.condarc` via the `dirs` crate, promoted to a direct dependency

**Decision**: Add `dirs = "6"` to `allez`'s own `[dependencies]`
(currently present only transitively, at `6.0.0` per `Cargo.lock`) and
use `dirs::home_dir()` to resolve the home directory, then join
`.condarc`.

**Rationale**: Constitution VII requires paths to work correctly across
Linux/macOS/Windows via `Path`/`PathBuf`, not manual env-var lookups.
`allez` has no precedent for *home-directory* resolution specifically —
GEN-24's `paths.rs` resolves `$ALLEZ_EPHEMERAL_ROOT` (an
already-fully-specified, `allez`-owned path, not the home directory),
falling back to a synthesized root under the system temp directory when
that variable is absent; it does not locate a user's home directory.
Hand-rolling that lookup would re-implement what `dirs::home_dir()`
already does correctly on all three platforms; promoting an
already-transitively-present dependency adds no new supply-chain
surface.

**Dependency direction, stated once, cited throughout**: `crates/condarc`
has, and will have, no dependency on `allez` — `allez` depends on the
crate, never the reverse, matching the crate's own status as an
independently publishable library with a consumer other than `allez` in
mind (spec.md Operating Context). This is the same directional
constraint this promotion respects (a dependency flows from `allez`
toward the crate, never the other way), and it is what makes R12's
crate-local filtering a necessarily independent implementation rather
than a call into `allez`'s own `filter_channels()`.

**Alternatives considered**: `std::env::home_dir()` — deprecated by std
itself for giving wrong answers on some Windows configurations, not an
option. Hand-rolling `env::var("HOME").or_else(|_| env::var("USERPROFILE"))`
— rejected per Constitution VII, and it would miss Windows's documented
`%USERPROFILE%`-alternative lookup order that `dirs` already encodes
correctly.

## R8 — Testability: a path-injectable internal entry point, plain fixtures throughout

**Decision**: The public entry point,
`allez::channel_config::resolve_channel_config() -> ChannelConfigResolution`,
delegates to a `pub(crate)` function,
`resolve_channel_config_from(path: Option<&Path>) -> ChannelConfigResolution`,
that takes an explicit, optional `.condarc` path. SC-002's
missing/rejected/unreadable/expansion-failing/populated matrix is driven through
`resolve_channel_config_from` directly via co-located `#[cfg(test)]` unit
tests (`tempfile::NamedTempFile`, a missing-by-construction path, or a
file written with invalid UTF-8 bytes for the unreadable case — see
below) — never by mutating the real `$HOME`/`%USERPROFILE%` environment
variable.

These tests must be unit tests rather than `tests/`-directory
integration tests for a hard technical reason: `resolve_channel_config_from`
is `pub(crate)`, and a Rust integration test compiles as a separate
crate, which cannot name a `pub(crate)` item at all. The same applies to
SC-004's observability-capture tests (3 fallback-path cases — rejected,
unreadable, and an `expand_channels()` failure sharing `Rejected`'s
treatment per R11) and to SC-001's six-sample adaptation contract test,
which calls the private `adapt()` function directly inside
`src/channel_config/adapt.rs`. Each of SC-001's 6 samples parses a
hand-authored `.condarc` string, resolves it through
`condarc::expand_channels`, calls the real `adapt()`, and asserts it
equals a hand-typed literal `ChannelConfig` — never re-deriving the
expected value by re-executing `adapt()`'s own logic, which would let a
mapping bug go undetected. No `tests/channel_config_resolution.rs`
integration file is added; there's no capability-level need for one.

There is deliberately no automated test of the public, zero-argument
`resolve_channel_config()` entry point against a real, uncontrolled
`.condarc`: spec.md names no acceptance scenario for it specifically, and
reading whatever `.condarc` happens to exist on the test machine would be
non-deterministic (Constitution II), including risking the crate's
documented stack-depth-guard gap (spec.md Known Limitations: "can abort
the whole process rather than return a typed rejection"). The manual,
optional `examples/channel_config_smoke.rs` (quickstart.md) is the only
coverage of this wrapper's real-`$HOME` behavior, run deliberately by a
developer, not by `cargo test`.

**Rationale**: Constitution II requires tests to be "isolated,
deterministic, and fast." Every scenario needing a specific file state
already has a deterministic path through `resolve_channel_config_from`'s
explicit path parameter, so there's no environment race to isolate
against. The only thing that function's tests can't cover is the public
wrapper's own two-line `dirs::home_dir()` + `.join(".condarc")` step —
but that step has no branching logic of its own, so leaving it to the
manual example (rather than mocking `dirs::home_dir()` or standing up a
self-re-executing child process) is proportionate.

**Unreadable-file fixture: invalid UTF-8, not a permission
manipulation**: SC-002's "unreadable" state is produced by writing a file
containing invalid UTF-8 bytes. `std::fs::read_to_string` requires valid
UTF-8 and fails deterministically with `io::ErrorKind::InvalidData` on
every target platform, needing no `chmod`, no Windows ACL manipulation,
and no `cfg`-gating. This exercises the same
`ReadOutcome::Unreadable(io::Error)` branch a real permission-denied file
would (that variant's doc comment defines it as "any other `io::Error`,
most commonly `PermissionDenied`"), proving the branch is reached,
recorded (FR-011), and surfaced as `Some(FallbackReason::Unreadable)`
(FR-017) for any non-`NotFound` I/O error.

**Why invalid UTF-8 rather than a literal permission denial**: beyond
the cross-platform/`cfg`-gating concern, literal permission denial has a
reliability problem: many CI environments run as `root`, under which
Unix permission bits are bypassed entirely (`root` can read a
`chmod 0o000` file regardless of mode). A real permission-denial fixture
would be flaky or silently no-op into the success path in exactly the
environments most likely to run it. Invalid-UTF-8 content has no such
privilege-bypass failure mode on any platform or user context. spec.md's
"OS permission error" wording (User Story 2, Scenario 3) names one
real-world instance of the broader `io::Error` class FR-011/`ReadOutcome::Unreadable`
actually cover; this fixture verifies that broader class deterministically
in every environment this ticket's tests run in.

Every other `allez`-level scenario test stays on the path-injectable
internal function (SC-002's five file states plus the argument-level
`None` case, six tests total, one of which also covers SC-005's own
`allez`-level case; the SC-004 observability-capture group — 3
fallback-path cases plus the FR-009 zero-events negative case, 4
dedicated tests — plus SC-006's own dedicated case, SC-007's
end-to-end redacted-detail assertion; and FR-013's
never-mutates, FR-014's unknown-key-tolerance, FR-015's no-caching, and
the non-filtering-caused `NoChannels` case, one dedicated test each).
SC-003's
22 scenarios are not part of this `allez`-level accounting at all — they
are crate-level tests of `condarc::expand_channels()` itself, in
`crates/condarc/tests/expand_channels_scenarios.rs`, and never touch
`resolve_channel_config_from`.

**Alternatives considered**: Accepting a `path: Option<&Path>` parameter
directly on the public API — rejected: FR-009/FR-012 describe this
capability's public contract as parameterless, and GEN-25 (the
production caller) never needs a different path; exposing one would be
unused configurability needing its own doc-comment caveat. Re-executing
the test binary as a child process with `HOME` redirected — this was the
original design; dropped because the process isolation it bought was
solving an environment race the explicit path parameter already avoids,
and the one remaining case (the wrapper's `dirs::home_dir()` call) has no
branching logic worth an automated test.

## R9 — Structured observability: one new, narrowly-scoped event shape, no reuse of `EphemeralLifecycleEvent`

**Decision**: One new `tracing`-emitting event type in
`src/channel_config/events.rs`:

- `ChannelConfigFallbackEvent` (FR-011): `schema_version`, `reason`
  (`FallbackReason::Rejected` | `FallbackReason::Unreadable`, promoted to
  `pub` by R10), `detail` — the crate's own per-problem
  `ValidationReport::to_string()` for `FallbackReason::Rejected`, or the `io::Error`'s
  own `Display` text for `FallbackReason::Unreadable` — passed through
  `redact_and_bound(&str) -> String` before construction (FR-021).
  `redact_and_bound` delegates the actual redaction to this module's own
  private `redact_channel_credentials` (R14), then bounds the result:
  every substring matching the FR-001(a) scheme pattern has its userinfo
  and `/t/<segment>/` path component stripped (the same two patterns
  `redact_channel_url()`, `src/ephemeral/channels.rs`, GEN-24, already
  applies to a single URL, here applied independently to every matched
  substring in free-form text, not just the first), then the result is
  truncated to `MAX_FALLBACK_DETAIL_LEN` (2048 bytes, data-model.md) at
  the nearest UTF-8 character boundary. Not a call into
  `redact_channel_url()` itself — that function's own signature takes
  one already-known-to-be-a-URL value, not a free-form string that may
  embed zero, one, or more of them — so this module's `redact_channel_credentials`
  is a second, independent implementation for a different input shape,
  the same pattern R12 already establishes for `filter_channels()`'s
  policy (and, per R14, the same pattern this ticket's `crates/condarc`
  side also uses independently, for a different consumer).

Emitted via `tracing::warn!` (a fallback is a recovered problem,
warranting attention) through the existing `src/observability.rs`
subscriber — no second logging pipeline — with its own `schema_version`
constant (not shared with `output::SCHEMA_VERSION` or GEN-24's
`EPHEMERAL_EVENT_SCHEMA_VERSION`), matching GEN-24's own precedent for
`EphemeralLifecycleEvent`.

**Rationale**: `EphemeralLifecycleEvent`'s fixed shape
(`operation`/`packages`/`duration_ms`/`outcome`/`failure_category`) is
purpose-built for a create/install/teardown operation with a
success/failure outcome and duration — a fallback record has no
"operation" in that sense (resolution itself always "succeeds," per
FR-011). Forcing it into that shape would be the same "category-string
lie" `ActivationError`'s doc comment (GEN-24 data-model.md) already
rejected for a structurally similar reason.

**Alternatives considered**: Extending `EphemeralLifecycleEvent` with new
optional fields — rejected: that type is GEN-24's own already-tested
contract; extending it here risks unrelated-feature coupling
(Constitution I) for no shared benefit.

## R10 — Surfacing FR-011's fallback condition in the resolution result itself, not only via observability (FR-017)

**Decision**: `resolve_channel_config`/`resolve_channel_config_from`
return a new wrapper type, `ChannelConfigResolution` (later changed from
a struct to a `#[non_exhaustive]` enum by R13, for an unrelated reason),
instead of a bare `ChannelConfig`. In this section's original struct
shape: `ChannelConfigResolution { config: ChannelConfig, fallback: Option<FallbackReason> }`.
`FallbackReason` (already introduced for `ChannelConfigFallbackEvent`,
R9) is promoted to a `pub`, `#[non_exhaustive]` type shared between the
observability event and this field. `fallback` is `None` for a
fully-successful resolution and for the silent missing-file case
(FR-009); `Some(Rejected)`/`Some(Unreadable)` for FR-011's recorded
fallback cases (broadened to a third trigger by R11, without a third
variant). `config` remains exactly GEN-24's unmodified four-part
`ChannelConfig` — the wrapper adds a sibling field, never a fifth field
inside `ChannelConfig`.

**Rationale**: FR-017 requires this condition be inspectable at the
result level, not only via a tracing event a caller might never read
during an unattended run. A caller with more context than this ticket's
own resolution step (GEN-25) can then decide whether to warn, proceed, or
abort, while `resolve_channel_config` itself stays total and never
fails. Reusing `FallbackReason` (rather than a second, parallel reason
enum) avoids duplicating the exact same two-case distinction the
observability event already models.

**Alternatives considered**:
- *Making `resolve_channel_config` fallible* (`Result<ChannelConfig, ChannelConfigError>`)
  — rejected: this was the alternative spec-review feedback proposed; an
  unattended agent invocation has no interactive way to react to a hard
  failure mid-run, so the function must stay total. The caller-level
  signal this decision adds answers the same underlying concern
  (visibility) without reopening FR-009's no-fail guarantee. The
  *crate*-level `expand_channels()` did become fallible, for an unrelated
  reason — see R11.
- *Leaving `FallbackReason` private and inventing a separate public
  copy* — rejected as needless duplication of a type that already models
  this distinction.

## R11 — `expand_channels()` becomes fallible: `Result<ResolvedChannels, ExpandChannelsError>`

**Decision**: `expand_channels(config: &Config) -> Result<ResolvedChannels, ExpandChannelsError>`,
where `ExpandChannelsError` is a new `#[non_exhaustive]` enum with
exactly one inhabited variant: `EmptyChannelAlias { entry: String }`,
produced when resolving `entry` requires joining it to an effective
`channel_alias` that is an explicit empty string (FR-018) — never for any
other input, including one that legitimately resolves to an empty
`channels` list (R13). `allez`'s `resolve_channel_config_from` treats
this `Err` exactly like a `parse()`-rejected document: FR-011's fallback
path runs, recording the `ExpandChannelsError`'s `Display` text through
`ChannelConfigFallbackEvent` with `reason: FallbackReason::Rejected` —
the same variant a `parse()` rejection uses, broadened rather than given
a third sibling.

**Rationale**: A PR reviewer argued a `Result` return type belongs at
the crate level rather than (or in addition to) the `allez` wrapper R10
already settled. Introducing `Result` needs an actual failure mode;
`expand_channels()` had none until this decision, since every other
FR-001 precedence branch always succeeds. The one existing gap — an
explicit empty-string `channel_alias` joined against a bare name — was
previously documented in Known Limitations as an unspecified,
out-of-scope corner. Turning it into a typed `Err` gives `Result` real
meaning rather than an uninhabited error type for future-proofing (which
`#[non_exhaustive]` already provides). No other input configuration
gains a new failure mode: R12's filtering can legitimately produce an
empty `channels` list, which stays `Ok` — filtering everything out is a
correctly applied policy, not a failure.

**Alternatives considered**:
- *`Result<ResolvedChannels, Infallible>`* — rejected once a real,
  already-documented failure candidate was identified; using it is more
  honest than a permanently-empty error type.
- *Reproducing real conda's own non-meaningful-URL construction for the
  empty-`channel_alias` case instead of rejecting it* — rejected: no
  evidence any real `.condarc` sets `channel_alias` to an empty string,
  so matching real conda's behavior here isn't worth the added
  `Channel.from_value()`-style logic.

## R12 — `expand_channels()` applies allow/deny filtering itself, as a fresh, crate-local implementation

**Decision**: FR-019's deny-then-allow filtering (deny-list checked
first; if the resolved allow-list is non-empty, only entries present in
it survive) is implemented as a small, private helper inside
`expand_channels.rs`, operating on the already-expanded `Vec<String>`
main list and allow/deny lists, all produced by the same
`resolve_entry`/`resolve_member` machinery (R5/R6). `ResolvedChannels`
shrinks to two fields — `channels` (final, filtered) and
`channel_priority` — the separate `allowlist_channels`/`denylist_channels`
fields are removed; nothing outside `expand_channels()` sees the
unfiltered intermediate lists.

**Rationale**: A reviewer asked why `ResolvedChannels` returned three raw
lists instead of one usable one; the ticket owner agreed. The filtering
algorithm is trivial (two `Vec::contains` checks per entry, no state, no
I/O) and applies the same deny-then-allow *precedence* `allez`'s own
`filter_channels()` (`src/ephemeral/channels.rs`, GEN-24) applies — real
conda's own documented enforcement order too (spec.md Known
Limitations) — not byte-identical semantics: `filter_channels()` matches
a `ChannelSpec.url_or_name` verbatim, including the literal bare name
`"defaults"`, while this crate-local copy matches already-expanded,
fully-qualified identifiers on both sides. The crate cannot call
`filter_channels()` directly — `crates/condarc` has no dependency on
`allez` and never will (R7) — so satisfying the reviewer's ask requires
an independent implementation. FR-016's "MUST NOT reimplement" language
is scoped to `allez`'s own codebase reimplementing its own logic; a new
implementation inside a different crate for a different consumer is
exactly the independently-useful generic behavior spec.md's Operating
Context already gives as this ticket's reason for moving resolution into
the crate. See spec.md Assumptions, "Two independent implementations of
the same filtering policy," for the full argument, and "Allow/deny
value-level conflicts are now reconciled by construction" for the
resulting behavior change (a channel in both lists is now removed, not
passed through unreconciled).

`allez`'s own `adapt()` (data-model.md) now constructs `ChannelConfig`
with `allowed_channels: Vec::new()` and `denied_channels: Vec::new()` —
GEN-24's own `filter_channels()` remains unmodified and still runs from
`solve_packages()`; it simply has nothing left to remove for a
`ChannelConfig` this ticket's adaptation layer produces, and remains
load-bearing for every other caller that constructs a `ChannelConfig`
directly (e.g. `ChannelConfig::from_urls`).

**Alternatives considered**:
- *Combine only at the `allez` level* (leave `ResolvedChannels` with
  three raw lists, have `adapt()` filter) — rejected: the reviewer's ask
  was specifically about the crate-level `ResolvedChannels` type; a
  future non-`allez` consumer would still get three raw lists to filter
  itself.
- *Have `expand_channels()` depend on `allez` to call `filter_channels()`
  directly* — rejected outright: wrong dependency direction (R7).

## R13 — `allez`'s resolution result must distinguish "legitimately zero channels" from "nothing was configured"

**Decision**: `ChannelConfigResolution` becomes a `#[non_exhaustive]` enum
instead of a struct:

```rust
#[non_exhaustive]
pub enum ChannelConfigResolution {
    Ready { config: ChannelConfig, fallback: Option<FallbackReason> },
    NoChannels,
}
```

`resolve_channel_config_from` returns `NoChannels` whenever
`expand_channels()`'s `Ok(ResolvedChannels)` has an empty `channels` list
(R12's filtering removed every entry, or the configuration otherwise
resolves to nothing) — and `Ready { .. }` for every other case, exactly
as `ChannelConfigResolution` already worked before this decision (R10).

**Rationale**: GEN-24's own, unmodified `channels_with_fallback`
(`src/ephemeral/channels.rs`) treats *any* empty `ChannelConfig.channels`
as "nothing was configured" and silently substitutes the built-in
`defaults` channel — correct for GEN-24's original callers, who never had
a reason to construct a deliberately empty `ChannelConfig`. R12 changes
that: once `expand_channels()` filters the main list down to nothing,
`allez`'s adaptation layer would otherwise hand GEN-24 exactly the empty
`ChannelConfig` `channels_with_fallback` is designed to override —
silently replacing a user's own explicit deny-everything restriction
with `defaults`, the opposite of what the user configured. Before R12,
`ChannelConfig.channels` was never empty as a result of filtering at this
ticket's layer — the raw list was handed to GEN-24, and
`filter_channels()`'s own later call inside `solve_packages()` correctly
turned a filtered-to-empty result into `EphemeralEnvError::NoChannelsConfigured`
(an explicit error, not a silent default) precisely because
`channels_with_fallback` had already run before that filtering. Catching
this one call earlier is not optional once R12 moves filtering into
`expand_channels()` itself.

**Alternatives considered**:
- *Leave `ChannelConfigResolution` as a struct and add a boolean field*
  (`empty_after_filtering: bool`) — rejected: a caller could ignore the
  boolean and pass `config` through to GEN-24 by mistake; an enum makes
  the two cases structurally impossible to conflate.
- *Fix `channels_with_fallback` itself* — rejected outright by FR-016:
  that's GEN-24's own delivered behavior, out of this ticket's scope to
  change.
- *Have `expand_channels()` itself return `Err` for an empty-after-filtering
  result* — rejected: filtering everything out is a correctly applied
  success, not a failure to compute one; conflating the two would make
  `ExpandChannelsError` (R11) mean two unrelated things.

## R14 — `ResolvedChannels` does not derive `Debug`; its own private `redact_channel_credentials` is a second, independent implementation of R9's redaction patterns

**Decision**: `ResolvedChannels` (`crates/condarc`) derives
`Clone, PartialEq, Eq` but not `Debug`. Its manual `Debug` impl
(data-model.md) redacts each `channels` entry through a private
`redact_channel_credentials` function declared in the same file,
applying the same two patterns R9's `redact_and_bound` applies (URL
userinfo, `/t/<segment>/`), before rendering the ordinary derived-style
output.

**Rationale**: `channels` entries come directly from `~/.condarc` and
can embed the same credential shapes FR-021 already redacts out of
observability text. A derived `Debug` would print them verbatim in any
test failure, panic message, or incidental `{:?}` logging call.
`redact_and_bound`'s own `redact_channel_credentials` (`allez`,
`src/channel_config/events.rs`) cannot be called from here: `crates/condarc`
has no dependency on `allez` (R7). The two functions are therefore a
second, independent implementation of the same redaction patterns, not
a shared one — the same pattern R12 already establishes for
`filter_channels()`'s policy.

**Alternatives considered**:
- *Keep the derived `Debug` and rely on callers not to print `channels`
  carelessly* — rejected: nothing enforces that, and the type is public.
- *Make the redaction function `pub` in `condarc` and have `allez` call
  it* — rejected: it would make `redact_and_bound` depend on `condarc`
  for a two-line pattern match, coupling an `allez`-internal
  observability detail to the crate's public surface for no benefit.

## R15 — `resolve_channel_config_from`'s internal `Config::default()` fallback call is defended by an explicit invariant, not left as an unstated `Err` arm

**Decision**: The internal call to `condarc::expand_channels(&Config::default())`
(the path every non-`Ready`-from-real-file case routes through) unwraps
via `.expect(...)` with a diagnostic message, rather than propagating or
silently reinterpreting a hypothetical `Err`.

**Rationale**: `Config::default()`'s `channel_alias` is `None`, which
FR-002 defaults to the non-empty built-in alias — never the empty
string `ExpandChannelsError::EmptyChannelAlias` requires — so this call
is guaranteed to succeed. The crate's own signature does not encode that
guarantee at the type level, so the call site must either leave the
`Err` arm's behavior unstated (the defect this decision closes) or
handle it explicitly. Mapping a hypothetical `Err` to `NoChannels` or a
fabricated `FallbackReason` would misrepresent a broken built-in
invariant as an ordinary user-input outcome; a deliberate panic with a
diagnostic message keeps the failure mode honest. The crate-level test
asserting `Config::default()` resolves and cannot reach
`EmptyChannelAlias` (User Story 1) is what would catch a future
violation of this invariant, before it ever reached this call site.

**Alternatives considered**:
- *Silently map the hypothetical `Err` to `NoChannels`* — rejected: a
  caller would read `NoChannels` as "the user's own configuration
  legitimately has zero channels" (FR-020), not "a built-in default
  broke."
- *Expose an infallible, crate-level default-resolution function* —
  rejected: it would add a second public entry point to `condarc` for a
  guarantee `expand_channels()`'s existing signature already documents
  by convention.

## Test strategy

- **Crate-level** (`crates/condarc/`): unit tests co-located in
  `expand_channels.rs` for each of R5/R6/R12's crate-private helpers
  (`resolve_entry`, `resolve_member`, the progressive-prefix matcher,
  `apply_allow_deny`),
  plus R14's `redact_channel_credentials`/`Debug`-impl test; a
  new integration test file, `crates/condarc/tests/expand_channels_scenarios.rs`,
  mapping every one of SC-003's 22 named scenarios to one `#[test]` each
  with hand-authored `.condarc` YAML strings, plus SC-005's own
  crate-level case (an empty-string-alias entry asserting
  `Err(ExpandChannelsError::EmptyChannelAlias)`, R11), the `Config::default()`
  regression case, and the FR-006/FR-007/FR-008/US1-AS4/US1-AS5/US3-AS3/FR-002-explicit-empty
  regression scenarios. No JSON
  conformance-corpus fixtures — GEN-36's own conformance harness has no
  oracle for channel *resolution*, only parse-shape coercion (its Python
  drivers invoke real conda's `Context.validate_all()`, never its
  channel-resolution code path). Extending it to cover resolution is a
  new subprocess driver, out of this ticket's scope; PR #6 review agreed
  a follow-up ticket should track it — none filed yet as of this
  writing.
- **`allez`-level**: unit tests co-located in `src/channel_config/*.rs`
  for the path-injectable `resolve_channel_config_from` (R8), covering
  SC-002's five file-state cases, one additional test for the
  argument-level `None`-vs-`Some(<nonexistent path>)` distinction (six
  tests total), and the SC-004 observability-capture
  group (using a per-test-scoped `tracing::subscriber::with_default` with
  its own capture buffer — never a process-global subscriber — the same
  technique GEN-24's `quickstart.md` describes for `EphemeralLifecycleEvent`,
  so tests needing this capture stay independent of every other test
  running in parallel) — both
  co-located unit tests, not integration tests, since that function is
  `pub(crate)`. SC-001's six-sample contract test is likewise a
  co-located unit test inside `src/channel_config/adapt.rs`, calling
  `adapt()` directly. Each of SC-002's five cases also asserts
  `ChannelConfigResolution::Ready { fallback, .. }`'s exact value
  (`None`/`Some(Rejected)`/`Some(Unreadable)`/`Some(Rejected)`/`None`,
  FR-017/R10/R11), not only `config` — plus SC-006's own case asserting
  `NoChannels` (never a `Ready` carrying an empty `ChannelConfig`) for a
  `.condarc` whose FR-019 filtering empties `channels` (R13), plus a
  second, non-filtering-caused `NoChannels` case (a `.condarc` setting
  `custom_multichannels: {defaults: []}` alone), and FR-013's
  never-mutates, FR-014's unknown-key-tolerance, and FR-015's
  no-caching tests (one dedicated test each — full accounting above).
  SC-005's
  `allez`-level half (the crate's `Err` triggering the rejected-equivalent
  fallback) lives alongside the other `resolve_channel_config_from`-driven
  tests. SC-007 is a co-located unit test in `src/channel_config/events.rs`
  itself (not `mod.rs`), calling `redact_and_bound` directly for the
  redaction and truncation cases, plus one `resolve_channel_config_from`-driven
  end-to-end assertion in `mod.rs` confirming the emitted event's
  `detail` is already redacted. `events.rs` also holds its own
  co-located `emit_fallback()` unit test, asserting the captured event
  carries exactly `reason`, `schema_version`, and the already-redacted
  `detail` — no additional or omitted fields. There is deliberately no automated test
  of the public,
  zero-argument `resolve_channel_config()` wrapper against a real
  `.condarc`; `examples/channel_config_smoke.rs` (quickstart.md) is its
  only, manually-run coverage (R8).
- `cargo test --all` remains the single entry point; no new opt-in
  feature flag is needed since every automated test this ticket adds is
  network-free and hermetic by construction — `expand_channels()` never
  touches the network, and `allez`'s file-handling layer only ever reads
  a local path when one is explicitly given. The one exception is not an
  automated test at all: the manual, optional
  `examples/channel_config_smoke.rs`, which deliberately reads whichever
  real `.condarc` exists on the machine running it (R8).
