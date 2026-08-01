# Interface Contract: `condarc::expand_channels` (the crate's new public API surface)

This ticket adds exactly three new public items to the
already-published `condarc` crate's surface (`crates/condarc/src/lib.rs`):
`expand_channels()` itself, plus its `ResolvedChannels`/`ExpandChannelsError`
return types. Every other existing public item (`parse`,
`parse_with_options`, `Config`, `ValidationReport`, ...) is unchanged —
this contract is purely additive (spec.md Operating Context #1).

Signatures are shown without bodies, matching how GEN-24's own
`ephemeral_env_api.md` documents its contract — full bodies for the small
supporting type live in `data-model.md`.

## Public function

```rust
/// Resolves one parsed `.condarc` `Config`'s channel settings into a
/// single, ordered, fully-expanded, already-filtered `ResolvedChannels`.
/// See FR-001 through FR-004, FR-005 through FR-008, FR-018, and FR-019
/// for the full precedence/defaulting/filtering rules this implements.
///
/// Performs no I/O — a pure function of `config`. Returns
/// `Err(ExpandChannelsError::EmptyChannelAlias)` only when resolving some
/// entry actually requires joining it to an empty-string `channel_alias`
/// (FR-018); every other input resolves successfully, including one
/// whose allow/deny filtering (FR-019) empties `channels`.
pub fn expand_channels(config: &Config) -> Result<ResolvedChannels, ExpandChannelsError>;
```

## Public types

```rust
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq)]
pub struct ResolvedChannels {
    pub channels: Vec<String>,
    pub channel_priority: ChannelPriority,
}

impl std::fmt::Debug for ResolvedChannels { /* redacts URL userinfo/access-token
    material in `channels` before printing — see data-model.md */ }

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandChannelsError {
    EmptyChannelAlias { entry: String },
}

impl std::fmt::Display for ExpandChannelsError { /* ... */ }
impl std::error::Error for ExpandChannelsError {}
```

`ChannelPriority` itself is not new — it is the same, already-published,
`#[non_exhaustive]` 3-variant enum `Config::channel_priority` already
uses (GEN-36); `ResolvedChannels.channel_priority` reuses it directly
rather than introducing a parallel type. `ResolvedChannels` no longer
carries separate `allowlist_channels`/`denylist_channels` fields — see
"The single `channels` list is already filtered," below.

## Behavioral guarantees (callable contract, not implementation detail)

- **Never panics.** Every `Config` value a caller can construct —
  including `Config::default()`, produced by `parse("")` — either
  resolves to some `ResolvedChannels` or returns `Err`; neither path ever
  panics.
- **The only `Err` case is `EmptyChannelAlias`** (FR-018). Every other
  input resolves successfully, including one whose allow/deny filtering
  (FR-019) legitimately empties `channels` — that is a successful `Ok`
  with an empty list, never an `Err` (see spec.md Design Decisions,
  "Empty resolved list, two legitimate causes").
- **`Config::default()` specifically is guaranteed to resolve
  successfully, never `Err`.** `Config::default()`'s `channel_alias`
  field is `None`, which this contract's own defaulting resolves to the
  non-empty built-in alias, never the empty string `EmptyChannelAlias`
  requires — so this particular input can never reach the one `Err`
  case above. Callers relying on this for a default-fallback path
  (e.g. `allez`'s own file-handling layer) still match on the `Result`
  rather than unwrapping unchecked, since this signature does not
  encode the guarantee at the type level.
- **Ordering is preserved.** `channels[i]`'s relative order matches the
  order its corresponding source entry (or, for a `defaults`
  substitution, the corresponding `custom_multichannels`/`default_channels`
  member) appeared in `config`, after FR-019's filtering removes any
  non-surviving entries (survivors keep their original relative order).
- **Every entry in `channels` is a concrete channel identifier**
  (spec.md's Concrete Channel Identifier key entity) — always a
  fully-qualified URL, never a bare name, for every input within this
  ticket's supported scope. The empty-string `channel_alias` case that
  used to be the one documented exception (a non-meaningful
  bare-name-through-alias result) is now `Err` instead (FR-018); the
  separate schemeless-`custom_channels`-value corner remains unspecified
  (see spec.md's Known Limitations — no evidence any real `.condarc`
  triggers it).
- **Embedded credential material is passed through unchanged in the
  data itself.** This function does not strip or otherwise touch URL
  userinfo or a `/t/<token>/` segment in `channels`' values — that kind
  of stripping is out of this ticket's scope, deferred to GEN-29's own
  approach. `ResolvedChannels`'s own `Debug` impl redacts both patterns
  before printing (data-model.md) so a test failure, panic message, or
  incidental `{:?}` logging call doesn't leak them — that redaction is a
  presentation-layer safeguard only, not a claim that the returned
  `channels` values themselves are credential-free.
- **`override_channels_enabled` has no effect on the output** (FR-006) —
  `expand_channels()` does not read that field at all.
- **No deduplication pass of its own** (FR-007) — a coincidental repeat
  across two different source entries that both resolve to the same
  concrete identifier is preserved, not collapsed. FR-019's filtering
  matches by identifier value, so a duplicate pair either both survive
  or both get removed together — filtering can never distinguish
  between them to remove only one.
- **`channel_settings` never appears in the output** (FR-008) —
  `expand_channels()` does not read that field at all.
- **The single `channels` list is already filtered.** `allowlist_channels`/
  `denylist_channels` are resolved internally (through the same
  precedence as `channels`) and applied directly to `channels` —
  denied entries removed first, then, if the resolved allow-list is
  non-empty, every entry not in it removed too (FR-004/FR-019) — but
  never exposed as their own fields. This is a fresh, `condarc`-local
  implementation of the same deny-then-allow policy `allez`'s own
  already-delivered, crate-internal `filter_channels()`
  (`src/ephemeral/channels.rs`, GEN-24) applies at environment-creation
  time; `expand_channels()` does not call, depend on, or alter that
  function in any way — see spec.md Assumptions, "Two independent
  implementations of the same filtering policy."

## Non-goals (explicitly out of this contract)

- Locating, reading, or otherwise touching any file — `expand_channels()` takes an
  already-parsed `Config`, exactly as `parse()` takes an already-read
  `&str` (crate-wide convention, `lib.rs`'s own doc comment: "This crate
  never opens a file itself").
- Raising an error, or aborting an operation, when a channel is absent
  from a non-empty allowlist or present in the denylist — this function
  silently removes such entries from `channels` (FR-019); it does not
  reproduce real conda's own `ChannelNotAllowed`/`ChannelDenied`
  exception-raising behavior (see spec.md's Known Limitations).
- Per-channel authentication/proxy resolution (`channel_settings`) — out
  of scope for this ticket (FR-008), deferred to GEN-29.
- Reproducing real conda's own non-meaningful-URL construction for an
  empty-string `channel_alias` — this function returns `Err` for that
  input instead (FR-018).
- `migrated_channel_aliases`/`migrated_custom_channels` (legacy
  channel-migration settings) — not resolved at all, out of scope (see
  spec.md's Known Limitations).
- `allow_non_channel_urls` — not read or acted on; it only matters once a
  channel's repodata is actually fetched over the network, a step this
  function's pure, no-I/O scope never reaches (see spec.md's Known
  Limitations).
