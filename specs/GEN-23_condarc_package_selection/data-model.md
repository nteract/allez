# Phase 1 Data Model: `.condarc` Channel Resolution

Types are grouped by codebase location, matching spec.md's own grouping
("Requirements below are grouped by which codebase location implements
them"). See `research.md` for the rationale behind each design choice
below.

## `crates/condarc` — the new `resolve` capability

### `ResolvedChannels`

The crate's own generic output type — Channel Resolution's Key Entity
"Resolved Channel Configuration," minus the adaptation into any one
particular downstream shape (that adaptation is `allez`'s own job, see
below). Deliberately **not** isomorphic to `allez::ephemeral::ChannelConfig`
in field type (`Vec<String>` here, `Vec<ChannelSpec>` there) — this crate
has no dependency on `allez` and no reason to know that type exists;
`allez`'s adapter (FR-014) is a direct, lossless field-by-field mapping
precisely because the *shapes* line up conceptually, not because the
*types* are literally shared.

Now that the crate has a resolved type of its own at all, it is worth
stating why that type still cannot simply *be*
`allez::ephemeral::ChannelConfig`: (a) `crates/condarc` cannot depend on
the `allez` crate in the first place — that is the wrong dependency
direction, since `allez` depends on `condarc` and never the reverse (the
same constraint research.md R7 already establishes for the
credential-stripping decision, and the reason `condarc` stays
independently publishable); and (b) a future non-`allez` consumer of this
crate — the ticket's own stated motivation for moving resolution into the
crate at all (spec.md Operating Context, "generic conda behavior a
consumer other than `allez`... may need independently") — would have no
use for `allez`'s own `ChannelSpec`/`ChannelPriorityMode` types. This
closes out the open PR #5 review thread requesting struct reuse: the
crate-side resolved type now exists, satisfying that request's spirit,
while a second, `allez`-shaped type remains structurally required,
satisfying the technical constraint the original rebuttal identified.

```rust
/// The complete result of resolving one parsed [`Config`]'s channel
/// settings (FR-001–FR-009): an ordered, fully-expanded channel list, the
/// effective channel-priority mode, the resolved allow/deny lists, and a
/// report of every credential-stripping event this resolution performed.
/// Never fails — every input `Config` a caller can construct (including
/// `Config::default()`, the same value `parse()` produces for an empty
/// document) resolves to some `ResolvedChannels` value.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedChannels {
    /// Ordered, concrete channel identifiers (see Concrete Channel
    /// Identifier in spec.md), preserving `channels`'/`default_channels`'
    /// own original relative ordering throughout every expansion.
    pub channels: Vec<String>,
    /// Mirrors `Config::channel_priority`, defaulted to `Flexible` when
    /// absent (FR-002/FR-003). Never itself re-derives the legacy
    /// boolean-spelling mapping — that already happened at `parse()` time
    /// (research.md R2).
    pub channel_priority: ChannelPriority,
    /// Resolved `allowlist_channels` (alias `whitelist_channels`), empty
    /// when the setting was absent (FR-004). Same concrete-identifier
    /// form as `channels`.
    pub allowlist_channels: Vec<String>,
    /// Resolved `denylist_channels`, empty when the setting was absent
    /// (FR-004). Same concrete-identifier form as `channels`.
    pub denylist_channels: Vec<String>,
    /// Every credential-stripping event this resolution performed, in
    /// the order encountered (FR-005). Empty when nothing needed
    /// stripping — the overwhelmingly common case.
    pub credential_stripping: Vec<CredentialStrippingEvent>,
}
```

`#[non_exhaustive]` matches every other public enum/struct GEN-36 already
established in this crate (Constitution IV's "matches existing precedent"
reading) and leaves room for a future, additive field without a breaking
change.

### `ChannelListRole` and `CredentialStrippingEvent`

```rust
/// Which of [`ResolvedChannels`]'s three lists a
/// [`CredentialStrippingEvent`] refers to.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelListRole {
    Channels,
    AllowlistChannels,
    DenylistChannels,
}

/// One credential-stripping event: userinfo or a conda access-token path
/// segment (spec.md's Credential Material key entity) was removed from
/// one entry before it became part of [`ResolvedChannels`]'s output
/// (FR-005). Identifies the affected entry by position/role only — never
/// by the stripped material or the resulting URL, so this type itself
/// can never leak what it describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialStrippingEvent {
    pub role: ChannelListRole,
    /// This entry's index within the list `role` names, in
    /// [`ResolvedChannels`]'s own post-resolution ordering.
    pub index: usize,
}
```

### `resolve`

```rust
/// Resolves one parsed `.condarc` [`Config`]'s channel settings
/// (`channels`/`channel`, `channel_alias`, `custom_channels`,
/// `custom_multichannels`, `default_channels`, `channel_priority`,
/// `allowlist_channels`/`whitelist_channels`, `denylist_channels`) into a
/// single, ordered, fully-expanded [`ResolvedChannels`] — see FR-001
/// through FR-009. Layered on top of, and never mutating,
/// [`crate::parse`]'s output.
///
/// Never fails and performs no I/O of any kind: a pure function of
/// `config`, applying conda's own documented defaults (research.md R4)
/// wherever a relevant setting is absent, exactly as FR-002's table
/// specifies.
pub fn resolve(config: &Config) -> ResolvedChannels;
```

### Internal (crate-private) helpers

Not part of the public contract, listed here because `research.md`
R5–R7 describe their exact behavior and `tasks.md` will need to break
them out individually for TDD:

```rust
/// Full FR-001 precedence: (a) URL check, (b) `custom_multichannels`
/// lookup (`"defaults"` checked here first), (c) `custom_channels`
/// progressive-prefix match, (d) `channel_alias` join. Used for every
/// top-level `channels`/`allowlist_channels`/`denylist_channels` entry.
///
/// Returns a `Vec` rather than a single `String` because FR-001(b)
/// expands *any* matched `custom_multichannels` name — not only the
/// literal name `defaults` — to that multichannel's own members, so one
/// input entry can legitimately produce zero, one, or many output
/// entries. Branches (a)/(c)/(d) each return a single-element vec;
/// branch (b) returns the fully-expanded member list, each member
/// independently resolved via `resolve_member` (research.md R5).
fn resolve_entry(entry: &str, ctx: &ResolveContext) -> Vec<String>;

/// Restricted precedence for a `custom_multichannels` (including
/// `defaults`) member: (a) URL check, then straight to (d)
/// `channel_alias` join — (b)/(c) are deliberately skipped (research.md
/// R5, spec.md Acceptance Scenario 5).
fn resolve_member(entry: &str, channel_alias: &str) -> String;

/// FR-001(c)'s progressive-prefix matcher against the effective
/// `custom_channels` map: tries `entry`, then each successive
/// `/`-delimited prefix, longest match wins by construction (checking
/// the full entry first). Returns the joined URL on a hit
/// (research.md R6).
fn match_custom_channel(entry: &str, custom_channels: &BTreeMap<String, String>) -> Option<String>;

/// Removes URL userinfo and a `/t/<token>/` conda-token path segment.
/// Returns the (possibly unchanged) string and whether stripping
/// occurred (research.md R7) — an independent implementation from
/// `allez::ephemeral::channels::redact_channel_url`, not a shared call.
fn strip_credentials(value: &str) -> (String, bool);

/// Bundles the effective (already-defaulted, per FR-002) `channel_alias`,
/// `custom_channels`, `custom_multichannels`, and `default_channels`
/// values `resolve_entry`/`resolve_member` both need, computed once per
/// `resolve()` call rather than re-derived per entry.
struct ResolveContext<'a> {
    channel_alias: &'a str,
    custom_channels: BTreeMap<&'a str, &'a str>,
    custom_multichannels: &'a BTreeMap<String, Vec<String>>,
    default_channels: Vec<&'a str>,
}
```

### Composition: how the three lists, `resolve_entry`, and credential stripping fit together (FR-005/R5/R12)

`resolve()` builds each of `channels`, `allowlist_channels`, and
`denylist_channels` independently, in that fixed role order, by iterating
that one setting's own (already-defaulted per FR-002) input entries in
their original order — the three lists never share state beyond the
per-call `ResolveContext` and the single, growing
`credential_stripping` vec they all append to. For each input entry,
`resolve_entry(entry, ctx)` is called, yielding a `Vec<String>` of zero,
one, or many resolved strings (FR-001(b)'s member expansion is what makes
"many" possible; see `resolve_entry`'s own doc comment above). Each string
in that returned vec is then, in order, passed through
`strip_credentials(s) -> (String, bool)`; the returned string is pushed
onto the growing output list for this role, and when the returned bool is
`true`, a `CredentialStrippingEvent { role: <this list's
ChannelListRole>, index: <the index this string just landed at in the
output list> }` is pushed onto `credential_stripping`.

This ordering is why index assignment is unambiguous even though one
input entry can expand to zero, one, or many output entries: the index
recorded is always "wherever this particular output string ended up,"
assigned at push time against the output list itself, and never
pre-computed from the input entry's own position in the source setting.
`CredentialStrippingEvent`'s own doc comment (above) already commits to
this reading — "this entry's index within the list `role` names, in
[`ResolvedChannels`]'s own post-resolution ordering" — and FR-005's
"position/role" identification is satisfied by exactly that pairing, with
no need for the caller to reconstruct any input-to-output entry
correspondence.

Sketched for one list (pseudocode — illustrative of the ordering, not a
literal transcription of `resolve.rs`):

```rust
// pseudocode
let mut out: Vec<String> = Vec::new();
for entry in effective_entries_for(role) {          // FR-002-defaulted, original order
    for resolved in resolve_entry(entry, ctx) {     // 0..n per input entry (FR-001(b))
        let (clean, stripped) = strip_credentials(&resolved);
        out.push(clean);                            // index is decided here, at push time
        if stripped {
            credential_stripping.push(CredentialStrippingEvent {
                role,
                index: out.len() - 1,               // never derived from `entry`'s position
            });
        }
    }
}
```

## `allez` — file handling and adaptation

New module tree, `src/channel_config/` (sibling to the existing
`src/ephemeral/`, `src/cli/`, `src/error.rs`, `src/observability.rs`,
`src/output.rs` — not nested inside `ephemeral`, since FR-018 requires
this ticket to add an adaptation layer *on top of* GEN-24's existing,
unmodified public types, not reach into that module's own internals).

### `resolve_channel_config` / `resolve_channel_config_from`

```rust
/// Locates and reads `~/.condarc` (via [`dirs::home_dir`]), parses and
/// resolves it through the `condarc` crate, and adapts the result into
/// the four-part [`crate::ephemeral::ChannelConfig`] shape GEN-24's
/// ephemeral-environment-creation capability requires (FR-010–FR-014),
/// paired with an explicit fallback signal (FR-019).
///
/// Always returns a fully-populated [`ChannelConfigResolution`] — never
/// fails. A missing `~/.condarc` falls back silently, with
/// `fallback: None` (FR-010); a file the crate rejects, or one that
/// cannot be read due to an OS permission/I-O error, falls back the same
/// way but records the condition both via this project's structured
/// observability (FR-012) and via
/// `fallback: Some(FallbackReason::{Rejected,Unreadable})` (FR-019), so a
/// caller can decide how to react without needing to separately consult
/// observability output. Resolves fresh from disk on every call — never
/// caches or reuses a previous result (FR-017).
pub fn resolve_channel_config() -> ChannelConfigResolution {
    resolve_channel_config_from(default_condarc_path().as_deref())
}

/// The path-injectable core of [`resolve_channel_config`] (research.md
/// R10) — `pub(crate)` only; every external caller (including GEN-25)
/// uses the zero-argument public function above. `path: None` is treated
/// identically to a `Some(path)` that does not exist (FR-010) — this
/// lets [`default_condarc_path`] itself return `None` (a home directory
/// [`dirs::home_dir`] could not determine) without a separate failure
/// path, since that case is conda-config-file-absence in every way that
/// matters to this capability.
pub(crate) fn resolve_channel_config_from(path: Option<&Path>) -> ChannelConfigResolution;

/// `dirs::home_dir()` joined with `.condarc`, or `None` if the home
/// directory itself could not be determined.
fn default_condarc_path() -> Option<PathBuf>;
```

### `ChannelConfigResolution` (FR-019)

The result of one [`resolve_channel_config`]/[`resolve_channel_config_from`]
call: GEN-24's own, unmodified four-part `ChannelConfig` (FR-014/FR-018),
paired with a sibling signal distinguishing a rejected/unreadable-file
fallback (FR-012) from the silent missing-file case (FR-010) — see
`FallbackReason`, defined below alongside the observability types that
already model this same two-case distinction (research.md R12).

```rust
/// See this section's own doc comment above.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelConfigResolution {
    /// GEN-24's own four-part shape, exactly as FR-014 constructs it —
    /// never itself carries the fallback signal (FR-019 keeps that a
    /// sibling field, not a fifth field on this type or on
    /// `ChannelConfig` itself).
    pub config: ChannelConfig,
    /// `None` for a fully-successful resolution and for the silent
    /// missing-file case (FR-010). `Some(FallbackReason::Rejected)` or
    /// `Some(FallbackReason::Unreadable)` for FR-012's two recorded
    /// fallback cases — the same two cases `ChannelConfigFallbackEvent`
    /// already records via observability; this field makes that same
    /// fact inspectable in the return value itself (FR-019).
    pub fallback: Option<FallbackReason>,
}
```

Because `fallback` is itself public, `FallbackReason` needs a publicly
nameable path of its own: it is re-exported from `src/channel_config/mod.rs`
via `pub use events::FallbackReason;`, making it reachable externally as
`allez::channel_config::FallbackReason` — the same two-step shape by which
GEN-24's own `ChannelPriorityMode` is reached as
`allez::ephemeral::ChannelPriorityMode` (a `pub use` inside
`ephemeral/mod.rs`, plus `pub mod ephemeral;` in `lib.rs`), so an external
caller (GEN-25) can import the type and match its variants.

### `read_condarc` and `ReadOutcome`

```rust
/// Distinguishes FR-010's silent "missing" case from FR-012's recorded
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

### Adaptation (FR-014)

```rust
/// Direct, lossless, field-by-field construction of GEN-24's
/// `ChannelConfig` from the crate's `ResolvedChannels` — no additional
/// resolution or transformation logic (SC-001). `ResolvedChannels`'s
/// fifth field, `credential_stripping`, is deliberately excluded from
/// this mapping rather than silently dropped by it: it is not channel
/// data GEN-24's four-part shape has a home for, and it is handled
/// separately, by the caller, as FR-013's observability records
/// (`emit_credential_strip_record`) — mirroring how spec.md's Resolved
/// Channel Configuration key entity already treats that report as a
/// sibling of the four-part artifact, never a fifth part of it
/// (FR-013/FR-019). `condarc::ChannelPriority`
/// is `#[non_exhaustive]`; an unrecognized future variant maps to
/// `ChannelPriorityMode::Flexible` (conda's own documented default,
/// FR-002) rather than panicking, satisfying `#[non_exhaustive]`'s
/// external-consumer wildcard-arm requirement without inventing a new,
/// undocumented fourth mode.
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
        allowed_channels: resolved.allowlist_channels,
        denied_channels: resolved.denylist_channels,
    }
}
```

`ChannelConfig`/`ChannelSpec`/`ChannelPriorityMode` themselves are GEN-24's
own, already-delivered, already-`pub`-exported types
(`allez::ephemeral::{ChannelConfig, ChannelPriorityMode, ChannelSpec}`) —
this ticket adds no new fields, variants, or methods to any of them
(FR-018).

### Structured observability (FR-012/FR-013)

```rust
/// FR-012's fallback record: a rejected or unreadable `~/.condarc` was
/// treated the same as a missing one, but the condition itself is
/// recorded, distinct from FR-010's silent missing-file case.
struct ChannelConfigFallbackEvent<'a> {
    schema_version: &'static str,
    reason: FallbackReason,
    /// The crate's own per-problem detail (`ValidationReport`'s
    /// `Display` text) for `FallbackReason::Rejected`; the `io::Error`'s
    /// own `Display` text for `FallbackReason::Unreadable`. Credential
    /// safety for this field is inherited entirely from the crate's own
    /// already-delivered error-reporting behavior, not guaranteed by
    /// this ticket's own construction — spec.md SC-005 itself accounts
    /// for this boundary as out of this ticket's own control to
    /// re-guarantee.
    detail: &'a str,
}

/// Distinguishes FR-012's two recorded fallback cases — shared between
/// [`ChannelConfigFallbackEvent`] (the observability record above) and
/// [`ChannelConfigResolution::fallback`] (the same fact, made inspectable
/// in the return value itself, FR-019/research.md R12). `#[non_exhaustive]`
/// for the same future-proofing reason every other public enum in this
/// ticket's scope already uses (e.g. `ChannelListRole`).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    Rejected,
    Unreadable,
}

/// FR-013's credential-stripping record, one per
/// `condarc::CredentialStrippingEvent` a resolution reported.
struct CredentialStripLogRecord {
    schema_version: &'static str,
    role: &'static str,
    index: usize,
}

fn emit_fallback(reason: FallbackReason, detail: &str);
fn emit_credential_strip_record(event: condarc::CredentialStrippingEvent);
```

Both emit through the existing `tracing`/`src/observability.rs` pipeline
— `tracing::warn!` for `ChannelConfigFallbackEvent`, `tracing::info!` for
`CredentialStripLogRecord` (research.md R11), both with structured
fields, consistent with Constitution XI — no second logging pipeline, no
new subscriber.

## Relationships

```text
resolve_channel_config -> ChannelConfigResolution (never fails; always returns a fully-populated value)
resolve_channel_config_from(path) -> ChannelConfigResolution
ChannelConfigResolution   1---1 ChannelConfig (the `config` field, GEN-24's own unmodified shape)
ChannelConfigResolution   1---1 Option<FallbackReason> (the `fallback` field, FR-019/R12)
read_condarc(path) -> Result<String, ReadOutcome>
condarc::parse(text) -> Result<condarc::Config, condarc::ValidationReport>
condarc::resolve(&Config) -> ResolvedChannels
ResolvedChannels          1---* CredentialStrippingEvent (0..n, in resolution order)
CredentialStrippingEvent  1---1 ChannelListRole
adapt(ResolvedChannels) -> ChannelConfig (FR-014, lossless field-by-field)
ChannelConfigFallbackEvent  0..1---1 resolve_channel_config_from call (emitted at most once per call, only on the rejected/unreadable path)
CredentialStripLogRecord    *---1 resolve_channel_config_from call (0..n per call, mirrors ResolvedChannels.credential_stripping)
```

No persistent storage anywhere in this feature: `~/.condarc` is read-only
input (FR-015 — never written, moved, or deleted), and nothing this
ticket adds is cached or reused across invocations (FR-017). The only
on-disk state this feature ever touches is the one file it reads.
