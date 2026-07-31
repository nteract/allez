# Phase 1 Data Model: `.condarc` Channel Resolution

Types are grouped by codebase location, matching spec.md's own grouping.
See `research.md` for the rationale behind each design choice below.

## `crates/condarc` — the new `expand_channels` capability

### `ResolvedChannels`

The crate's own generic output type — Channel Resolution's Key Entity
"Resolved Channel Configuration," minus the adaptation into any one
downstream shape (that's `allez`'s job, below). Deliberately **not**
isomorphic to `allez::ephemeral::ChannelConfig` in field type
(`Vec<String>` here, `Vec<ChannelSpec>` there) — this crate has no
dependency on `allez`; `allez`'s adapter (FR-012) is a direct, lossless
field-by-field mapping because the *shapes* line up conceptually, not
because the *types* are shared.

This type still can't simply *be* `allez::ephemeral::ChannelConfig`:
(a) `crates/condarc` cannot depend on `allez` — the wrong dependency
direction, since `allez` depends on `condarc` and never the reverse
(research.md R7), and the reason `condarc` stays independently
publishable; (b) a future non-`allez` consumer of this crate (the
ticket's own motivation for moving resolution into the crate, spec.md
Operating Context) would have no use for `allez`'s own
`ChannelSpec`/`ChannelPriorityMode` types. This closes out the PR #5
review thread requesting struct reuse: the crate-side resolved type now
exists, satisfying that request's spirit, while a second, `allez`-shaped
type remains structurally required.

```rust
/// The complete result of successfully resolving one parsed [`Config`]'s
/// channel settings (FR-001–FR-004, FR-005–FR-008, FR-019): an ordered,
/// fully-expanded, already deny-then-allow filtered channel list, and
/// the effective channel-priority mode. The separate allow-list/deny-list
/// entries used to compute `channels` are not themselves part of this
/// type (research.md R12) — they exist only inside `expand_channels()`'s
/// own implementation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedChannels {
    /// Ordered, concrete channel identifiers (see Concrete Channel
    /// Identifier in spec.md), preserving `channels`'/`default_channels`'
    /// own original relative ordering throughout every expansion, with
    /// FR-019's deny-then-allow filtering already applied. Can
    /// legitimately be empty — see spec.md Design Decisions, "Empty
    /// resolved list, two legitimate causes."
    pub channels: Vec<String>,
    /// Mirrors `Config::channel_priority`, defaulted to `Flexible` when
    /// absent (FR-002/FR-003). Never itself re-derives the legacy
    /// boolean-spelling mapping — that already happened at `parse()` time
    /// (research.md R2).
    pub channel_priority: ChannelPriority,
}
```

`#[non_exhaustive]` matches every other public enum/struct GEN-36
established in this crate, and leaves room for a future additive field
without a breaking change.

### `ExpandChannelsError`

```rust
/// Why `expand_channels()` could not produce a `ResolvedChannels` at all
/// (research.md R11). `#[non_exhaustive]` for the same future-proofing
/// reason every other public enum in this ticket's scope already uses —
/// today this has exactly one inhabited variant.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandChannelsError {
    /// Resolving `entry` required joining it to `channel_alias`
    /// (FR-001(d), or the restricted `resolve_member` precedence), but
    /// the effective `channel_alias` is an explicit empty string
    /// (FR-018). `entry` is the literal channel-list entry (or
    /// multichannel member) that triggered this, for the caller's own
    /// error message/observability detail.
    EmptyChannelAlias {
        /// The literal channel-list entry or multichannel member that
        /// required the empty alias.
        entry: String,
    },
}

impl std::fmt::Display for ExpandChannelsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyChannelAlias { entry } => write!(
                f,
                "cannot resolve {entry:?}: channel_alias is an explicit empty string"
            ),
        }
    }
}

impl std::error::Error for ExpandChannelsError {}
```

`expand_channels()` is a newly fallible function, not the crate's first
one — `parse`/`parse_with_options` already had `ValidationReport` before
this ticket. `ExpandChannelsError` follows the same manual
`Display`/`Error` pattern (`error.rs`'s own impls, no `thiserror` derive)
rather than introducing a new convention. FR-011's
`ChannelConfigFallbackEvent.detail` needs this `Display` impl to render
an `ExpandChannelsError`'s text.

### `expand_channels`

```rust
/// Resolves one parsed `.condarc` [`Config`]'s channel settings
/// (`channels`/`channel`, `channel_alias`, `custom_channels`,
/// `custom_multichannels`, `default_channels`, `channel_priority`,
/// `allowlist_channels`/`whitelist_channels`, `denylist_channels`) into a
/// single, ordered, fully-expanded, already-filtered [`ResolvedChannels`]
/// — see FR-001 through FR-004 and FR-005 through FR-008, FR-018, FR-019.
/// Layered on top of, and never mutating, [`crate::parse`]'s output.
///
/// Performs no I/O of any kind: a pure function of `config`, applying
/// conda's own documented defaults (research.md R4) wherever a relevant
/// setting is absent, exactly as FR-002's table specifies. Returns
/// `Err(ExpandChannelsError::EmptyChannelAlias)` only when resolving some
/// entry actually requires an empty-`channel_alias` join (FR-018,
/// research.md R11); every other input, including one whose allow/deny
/// filtering (FR-019) legitimately empties `channels`, resolves
/// successfully.
pub fn expand_channels(config: &Config) -> Result<ResolvedChannels, ExpandChannelsError>;
```

### Internal (crate-private) helpers

Not part of the public contract, listed here because `research.md`
R5–R6/R11–R12 describe their exact behavior and `tasks.md` will need to
break them out individually for TDD:

```rust
/// Full FR-001 precedence: (a) scheme-pattern match
/// (`^[a-z][a-z0-9]{0,11}://`), used as-is; (b) `custom_multichannels`
/// lookup (`"defaults"` checked here first); (c) `custom_channels`
/// progressive-prefix match, joined as
/// `base_url.trim_end_matches('/') + "/" + entry` (research.md R6); (d)
/// `channel_alias` join, `channel_alias.trim_end_matches('/') + "/" + entry`.
/// Used for every top-level `channels`/`allowlist_channels`/`denylist_channels`
/// entry.
///
/// Returns a `Vec` rather than a single `String` because FR-001(b)
/// expands *any* matched `custom_multichannels` name — not only the
/// literal name `defaults` — to that multichannel's own members, so one
/// input entry can legitimately produce zero, one, or many output
/// entries. Branches (a)/(c) each return a single-element vec; branch
/// (b) returns the fully-expanded member list, each member independently
/// resolved via `resolve_member` (research.md R5); branch (d) returns a
/// single-element vec on success, or propagates
/// `ExpandChannelsError::EmptyChannelAlias` (FR-018) when
/// `ctx.channel_alias` is empty.
fn resolve_entry(entry: &str, ctx: &ResolveContext) -> Result<Vec<String>, ExpandChannelsError>;

/// Restricted precedence for a `custom_multichannels` (including
/// `defaults`) member: (a) same scheme-pattern check, then straight to
/// (d) the same `channel_alias` join — (b)/(c) are deliberately skipped
/// (research.md R5, spec.md Acceptance Scenario 5). Propagates
/// `ExpandChannelsError::EmptyChannelAlias` (FR-018) under the same
/// condition as `resolve_entry`'s own branch (d).
fn resolve_member(entry: &str, channel_alias: &str) -> Result<String, ExpandChannelsError>;

/// FR-001(c)'s progressive-prefix matcher against the effective
/// `custom_channels` map: tries `entry`, then each successive
/// `/`-delimited prefix, longest match wins by construction (checking
/// the full entry first). Returns the joined URL on a hit
/// (research.md R6). Infallible — no `channel_alias` involved.
fn match_custom_channel(entry: &str, custom_channels: &HashMap<&str, &str>) -> Option<String>;

/// FR-019's deny-then-allow filtering (research.md R12): removes every
/// entry present in `denylist`, then, only if `allowlist` is non-empty,
/// removes every remaining entry not present in it. Preserves the
/// relative order of survivors and any coincidental duplicate (FR-007).
/// Pure, infallible — a purely structural `Vec` operation with no
/// `channel_alias`/URL logic of its own; `channels`/`allowlist`/`denylist`
/// are all already-resolved concrete identifiers by the time this runs.
/// This is a fresh, `condarc`-local implementation of the same policy
/// `allez`'s own `filter_channels()` applies — see research.md R12 for
/// why this is not the reimplementation FR-016 forbids.
fn apply_allow_deny(channels: Vec<String>, allowlist: &[String], denylist: &[String]) -> Vec<String>;

/// Bundles the effective (already-defaulted, per FR-002) `channel_alias`,
/// `custom_channels`, `custom_multichannels`, and `default_channels`
/// values `resolve_entry`/`resolve_member` both need, computed once per
/// `expand_channels()` call rather than re-derived per entry.
///
/// `custom_channels` is a `HashMap`: it is only ever looked up by key
/// (progressive-prefix matching, research.md R6), never iterated in
/// order, so there is no ordering requirement to justify `BTreeMap`.
/// `custom_multichannels` stays a `BTreeMap` for a different reason: it
/// borrows the inner `BTreeMap<String, Vec<String>>` already inside
/// `Config::custom_multichannels`'s own `Option` field (GEN-36) when that
/// field is `Some`, not a fresh type this ticket introduces — the `None`
/// case (absent setting) is handled by falling back to a `static` empty
/// `BTreeMap` rather than by choosing a different collection type.
/// `default_channels` is a plain `Vec`, not borrowed directly from
/// `Config`: it must unify one of two sources of different underlying
/// shape — the user's own `Some(Vec<String>)` value, or the borrowed
/// `&'static [&'static str]` built-in `DEFAULT_CHANNELS` (R4) — into one
/// common type `resolve_member` can iterate identically regardless of
/// which source supplied it; ordering is preserved by construction
/// (pushed in the source's own order), not by the `Vec` choice itself.
struct ResolveContext<'a> {
    channel_alias: &'a str,
    custom_channels: HashMap<&'a str, &'a str>,
    custom_multichannels: &'a BTreeMap<String, Vec<String>>,
    default_channels: Vec<&'a str>,
}
```

### Composition: how the three lists, `resolve_entry`, and filtering fit together (R5/R12)

`expand_channels()` builds each of `channels`, `allowlist_channels`, and
`denylist_channels` independently, in that fixed role order, by iterating
that setting's own (already-defaulted per FR-002) input entries in their
original order — the three lists never share state beyond the per-call
`ResolveContext`. For each input entry, `resolve_entry(entry, ctx)` is
called, yielding a `Vec<String>` of zero, one, or many resolved strings
on success (FR-001(b)'s member expansion is what makes "many" possible),
each pushed onto that role's own growing output list in order — or
propagating `Err` immediately, short-circuiting the whole call (the
first `EmptyChannelAlias` encountered, in role order `channels` →
`allowlist_channels` → `denylist_channels`, stops resolution; there is no
partial `ResolvedChannels` to salvage). Once all three raw lists are
resolved, `apply_allow_deny(channels, &allowlist, &denylist)` produces
the final, filtered `channels` — the allowlist/denylist `Vec`s are then
dropped; nothing outside `expand_channels()` ever sees them.

Sketched (pseudocode — illustrative of the ordering, not a literal
transcription of `expand_channels.rs`):

```rust
// pseudocode
fn resolve_role(role_entries: &[&str], ctx: &ResolveContext) -> Result<Vec<String>, ExpandChannelsError> {
    let mut out: Vec<String> = Vec::new();
    for entry in role_entries {                        // FR-002-defaulted, original order
        for resolved in resolve_entry(entry, ctx)? {    // 0..n per input entry (FR-001(b))
            out.push(resolved);
        }
    }
    Ok(out)
}

let channels = resolve_role(effective_entries_for(Channels), &ctx)?;
let allowlist = resolve_role(effective_entries_for(Allowlist), &ctx)?;
let denylist = resolve_role(effective_entries_for(Denylist), &ctx)?;
let channels = apply_allow_deny(channels, &allowlist, &denylist);   // FR-019
Ok(ResolvedChannels { channels, channel_priority })
```

## `allez` — file handling and adaptation

New module tree, `src/channel_config/` (sibling to the existing
`src/ephemeral/`, `src/cli/`, `src/error.rs`, `src/observability.rs`,
`src/output.rs` — not nested inside `ephemeral`, since FR-016 requires
this ticket to add an adaptation layer *on top of* GEN-24's existing,
unmodified public types, not reach into that module's own internals).

### `resolve_channel_config` / `resolve_channel_config_from`

```rust
/// Locates and reads `~/.condarc` (via [`dirs::home_dir`]), parses and
/// resolves it through the `condarc` crate, and adapts the result into
/// the four-part [`crate::ephemeral::ChannelConfig`] shape GEN-24's
/// ephemeral-environment-creation capability requires (FR-009–FR-011,
/// FR-012), paired with an explicit fallback signal (FR-017), or reports
/// FR-020's zero-usable-channels case instead of ever constructing an
/// intentionally-empty `ChannelConfig`.
///
/// Always returns a fully-populated [`ChannelConfigResolution`] — never
/// fails, never panics. A missing `~/.condarc` falls back silently, with
/// `Ready { fallback: None, .. }` (FR-009); a file the crate rejects, one
/// that cannot be read due to an OS permission/I-O error, or one whose
/// `condarc::expand_channels()` call itself returns `Err` (FR-018), falls
/// back the same way but records the condition both via this project's
/// structured observability (FR-011) and via
/// `Ready { fallback: Some(FallbackReason::Rejected), .. }` /
/// `Ready { fallback: Some(FallbackReason::Unreadable), .. }`
/// (FR-017) — an `expand_channels()` failure is folded into `Rejected`,
/// never a third variant (research.md R11). A `.condarc` that resolves
/// successfully but whose allow/deny filtering (FR-019) leaves `channels`
/// empty returns `NoChannels` instead of `Ready` (FR-020, research.md
/// R13) — never handed to GEN-24 as an empty `ChannelConfig`. Resolves
/// fresh from disk on every call — never caches or reuses a previous
/// result (FR-015).
pub fn resolve_channel_config() -> ChannelConfigResolution {
    resolve_channel_config_from(default_condarc_path().as_deref())
}

/// The path-injectable core of [`resolve_channel_config`] (research.md
/// R8) — `pub(crate)` only; every external caller (including GEN-25)
/// uses the zero-argument public function above. `path: None` is treated
/// identically to a `Some(path)` that does not exist (FR-009) — this
/// lets [`default_condarc_path`] itself return `None` (a home directory
/// [`dirs::home_dir`] could not determine) without a separate failure
/// path, since that case is conda-config-file-absence in every way that
/// matters to this capability.
///
/// Internally: `read_condarc` failure (`Missing`/`Unreadable`) or a
/// `condarc::parse`/`condarc::expand_channels` `Err` all fall back to
/// `condarc::expand_channels(&Config::default())` for `config` — a call
/// that cannot itself produce `Err`, since `Config::default()`'s
/// `channel_alias` is `None`, which FR-002 defaults to the non-empty
/// built-in alias, never the empty string `ExpandChannelsError::EmptyChannelAlias`
/// requires; callers still match on the `Result` rather than unwrapping
/// unchecked, since the crate's own signature does not encode that
/// guarantee at the type level. On a successful `expand_channels()` call
/// (real file or default fallback alike), an empty `.channels` produces
/// `NoChannels`; otherwise `Ready { config: adapt(resolved), fallback }`.
pub(crate) fn resolve_channel_config_from(path: Option<&Path>) -> ChannelConfigResolution;

/// `dirs::home_dir()` joined with `.condarc`, or `None` if the home
/// directory itself could not be determined.
fn default_condarc_path() -> Option<PathBuf>;
```

### `ChannelConfigResolution` (FR-017/FR-020)

The result of one [`resolve_channel_config`]/[`resolve_channel_config_from`]
call. A `#[non_exhaustive]` enum, not a struct (research.md R13): the
ordinary case, `Ready`, pairs GEN-24's own, unmodified four-part
`ChannelConfig` (FR-012/FR-016) with a sibling signal distinguishing a
rejected/unreadable/unexpandable-file fallback (FR-011/FR-018) from the
silent missing-file case (FR-009) — see `FallbackReason`, defined below
alongside the observability types that already model this distinction
(research.md R10). The other case, `NoChannels`, exists so a resolution
that legitimately produced zero usable channels (FR-019's filtering
removed every entry, or the underlying `.condarc` configuration otherwise
resolves to an empty list — spec.md Design Decisions, "Empty resolved
list, two legitimate causes") is never mistaken by GEN-24's own
`channels_with_fallback` for "nothing was configured" — see FR-020 and
research.md R13 for why this can't be caught one layer later, inside
GEN-24 itself.

```rust
/// See this section's own doc comment above.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ChannelConfigResolution {
    /// A resolution that produced at least one usable channel, whether
    /// from a real, populated `~/.condarc` or from conda's own
    /// documented defaults (missing/rejected/unreadable/unexpandable
    /// fallback).
    Ready {
        /// GEN-24's own four-part shape, exactly as FR-012 constructs
        /// it — never itself carries the fallback signal (FR-017 keeps
        /// that a sibling field, not a fifth field on this type or on
        /// `ChannelConfig` itself). `allowed_channels`/`denied_channels`
        /// are always empty here (FR-019 already filtered `channels`
        /// upstream) — see the Adaptation section below.
        config: ChannelConfig,
        /// `None` for a fully-successful resolution and for the silent
        /// missing-file case (FR-009). `Some(FallbackReason::Rejected)`
        /// or `Some(FallbackReason::Unreadable)` for FR-011's recorded
        /// fallback cases (`Rejected` broadened to also cover an
        /// `ExpandChannelsError`, FR-018/research.md R11) — the same
        /// cases `ChannelConfigFallbackEvent` already records via
        /// observability; this field makes that same fact inspectable
        /// in the return value itself (FR-017).
        fallback: Option<FallbackReason>,
    },
    /// The resolved configuration legitimately has zero usable channels
    /// (FR-019's allow/deny filtering removed every entry, or the
    /// underlying `.condarc` configuration otherwise resolves to an
    /// empty list) — a fully
    /// successful resolution of the user's own real preferences, not a
    /// fallback and not an error (FR-020). Carries no payload: there is
    /// no `ChannelConfig` for a caller to mistakenly pass to GEN-24.
    NoChannels,
}
```

Because `fallback` is itself public, `FallbackReason` needs a publicly
nameable path of its own: it is re-exported from `src/channel_config/mod.rs`
via `pub use events::FallbackReason;`, making it reachable externally as
`allez::channel_config::FallbackReason` — the same two-step shape by which
GEN-24's own `ChannelPriorityMode` is reached as
`allez::ephemeral::ChannelPriorityMode`, so an external caller (GEN-25)
can import the type and match its variants.

### `read_condarc` and `ReadOutcome`

```rust
/// Distinguishes FR-009's silent "missing" case from FR-011's recorded
/// "unreadable" case — both `std::fs::read_to_string` failure modes, but
/// with different observability requirements, so a single
/// `Result<String, io::Error>` is not expressive enough to route on
/// without re-inspecting `io::Error::kind()` at every call site.
enum ReadOutcome {
    /// The file does not exist (`io::ErrorKind::NotFound`).
    Missing,
    /// The file exists but could not be read (any other `io::Error`,
    /// most commonly `PermissionDenied`).
    Unreadable(io::Error),
}

fn read_condarc(path: &Path) -> Result<String, ReadOutcome>;
```

### Adaptation (FR-012)

```rust
/// Direct, lossless, field-by-field construction of GEN-24's
/// `ChannelConfig` from the crate's `ResolvedChannels` — no additional
/// resolution or transformation logic (SC-001). Only ever called with a
/// non-empty `resolved.channels`; the empty case produces
/// `ChannelConfigResolution::NoChannels` instead, one layer up, before
/// this function is reached at all (FR-020/research.md R13).
/// `condarc::ChannelPriority` is `#[non_exhaustive]`; both its current
/// `Flexible` variant and any unrecognized future variant map to
/// `ChannelPriorityMode::Flexible` (conda's own documented default,
/// FR-002) via the same wildcard arm — a deliberate policy this ticket
/// owns for exactly that reason, not an accident of satisfying
/// `#[non_exhaustive]`'s external-consumer wildcard-arm requirement:
/// `Strict` and `Disabled` are matched explicitly, and every other value
/// (today's `Flexible`, or a future addition) resolves to the same,
/// already-conda-documented default rather than panicking or inventing a
/// new, undocumented fourth mode.
/// `allowed_channels`/`denied_channels` are always empty: FR-019 already
/// applied that filtering upstream, inside `expand_channels()` itself —
/// `ResolvedChannels` no longer carries separate allow/deny fields to
/// map from (research.md R12). GEN-24's own `filter_channels()` remains
/// unmodified and still runs, unaffected, at `solve_packages()` time; it
/// simply has nothing left to remove for a `ChannelConfig` this function
/// produced.
fn adapt(resolved: condarc::ResolvedChannels) -> ChannelConfig {
    ChannelConfig {
        channels: resolved
            .channels
            .into_iter()
            .map(|url_or_name| ChannelSpec { url_or_name })
            .collect(),
        channel_priority: match resolved.channel_priority {
            condarc::ChannelPriority::Strict => ChannelPriorityMode::Strict,
            condarc::ChannelPriority::Disabled => ChannelPriorityMode::Disabled,
            _ => ChannelPriorityMode::Flexible,
        },
        allowed_channels: Vec::new(),
        denied_channels: Vec::new(),
    }
}
```

`ChannelConfig`/`ChannelSpec`/`ChannelPriorityMode` themselves are GEN-24's
own, already-delivered, already-`pub`-exported types
(`allez::ephemeral::{ChannelConfig, ChannelPriorityMode, ChannelSpec}`) —
this ticket adds no new fields, variants, or methods to any of them
(FR-016).

### Structured observability (FR-011)

```rust
/// FR-011's fallback record: a rejected, unreadable, or unexpandable
/// `~/.condarc` was treated the same as a missing one, but the condition
/// itself is recorded, distinct from FR-009's silent missing-file case.
struct ChannelConfigFallbackEvent {
    schema_version: &'static str,
    reason: FallbackReason,
    /// A redacted, length-bounded rendering (FR-021, `redact_and_bound`
    /// below) of the crate's own per-problem detail: `ValidationReport`'s
    /// `Display` text for `FallbackReason::Rejected` from a `parse()`
    /// rejection, or `ExpandChannelsError`'s own `Display` text for
    /// `FallbackReason::Rejected` from an `expand_channels()` failure
    /// (FR-018/research.md R11 — both share the one variant); the
    /// `io::Error`'s own `Display` text for `FallbackReason::Unreadable`.
    /// Owned rather than borrowed, since redaction/bounding produces a
    /// new string rather than a view into the source text.
    detail: String,
}

/// Distinguishes FR-011's two recorded fallback cases — shared between
/// [`ChannelConfigFallbackEvent`] (the observability record above) and
/// [`ChannelConfigResolution::Ready`]'s `fallback` field (the same fact, made
/// inspectable in the return value itself, FR-017/research.md R10).
/// `#[non_exhaustive]` for the same future-proofing reason every other
/// public enum in this ticket's scope already uses.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    /// A `~/.condarc` `parse()` rejected, or whose `expand_channels()`
    /// call failed (FR-011/FR-018, the latter broadened into this same
    /// variant per research.md R11).
    Rejected,
    /// A `~/.condarc` that exists but could not be read due to an OS
    /// permission or other I/O error (FR-011).
    Unreadable,
}

/// Redacts credential-shaped material from `raw`, then bounds its
/// length (FR-021). For every substring matching the scheme pattern
/// `[a-z][a-z0-9]{0,11}://` (same as FR-001(a)) up to the next
/// whitespace or quote character, removes (a) any `user[:pass]@`
/// userinfo immediately before the host, and (b) any `/t/<segment>/`
/// path component — the same two patterns `redact_channel_url()`
/// (`src/ephemeral/channels.rs`, GEN-24) applies to a single URL, here
/// applied independently to every matched URL-shaped substring, not
/// just the first. Text outside a matched substring is left untouched.
/// Truncates the result to `MAX_FALLBACK_DETAIL_LEN` at the nearest
/// UTF-8 character boundary at or before that length. Pure, infallible.
const MAX_FALLBACK_DETAIL_LEN: usize = 2048;
fn redact_and_bound(raw: &str) -> String;

/// Constructs and emits one [`ChannelConfigFallbackEvent`], passing
/// `detail` through [`redact_and_bound`] first (FR-021).
fn emit_fallback(reason: FallbackReason, detail: &str);
```

This emits through the existing `tracing`/`src/observability.rs` pipeline
— `tracing::warn!` for `ChannelConfigFallbackEvent` (research.md R9), with structured
fields, consistent with Constitution XI — no second logging pipeline, no
new subscriber.

## Relationships

```text
resolve_channel_config -> ChannelConfigResolution (never fails; always returns a fully-populated variant)
resolve_channel_config_from(path) -> ChannelConfigResolution
ChannelConfigResolution::Ready   1---1 ChannelConfig (the `config` field, GEN-24's own unmodified shape, always-empty allow/deny fields)
ChannelConfigResolution::Ready   1---1 Option<FallbackReason> (the `fallback` field, FR-017/R10/R11)
ChannelConfigResolution::NoChannels (no payload; FR-020/R13)
read_condarc(path) -> Result<String, ReadOutcome>
condarc::parse(text) -> Result<condarc::Config, condarc::ValidationReport>
condarc::expand_channels(&Config) -> Result<ResolvedChannels, ExpandChannelsError> (FR-018/R11)
adapt(ResolvedChannels) -> ChannelConfig (FR-012, lossless field-by-field; only called when resolved.channels is non-empty)
ChannelConfigFallbackEvent  0..1---1 resolve_channel_config_from call (emitted at most once per call, only on the rejected/unreadable/unexpandable path)
```

No persistent storage anywhere in this feature: `~/.condarc` is read-only
input (FR-013 — never written, moved, or deleted), and nothing this
ticket adds is cached or reused across invocations (FR-015). The only
on-disk state this feature ever touches is the one file it reads.
