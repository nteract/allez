# Interface Contract: `condarc::resolve` (the crate's new public API surface)

This ticket adds exactly one new public function and three new public
types to the already-published `condarc` crate's surface
(`crates/condarc/src/lib.rs`). Every other existing public item
(`parse`, `parse_with_options`, `Config`, `ValidationReport`, ...) is
unchanged — this contract is purely additive (spec.md Operating Context
#1).

Signatures are shown without bodies, matching how GEN-24's own
`ephemeral_env_api.md` documents its contract — full bodies for the small
supporting types live in `data-model.md`.

## Public function

```rust
/// Resolves one parsed `.condarc` `Config`'s channel settings into a
/// single, ordered, fully-expanded `ResolvedChannels`. See FR-001
/// through FR-009 for the full precedence/defaulting rules this
/// implements.
///
/// Never fails and performs no I/O — a pure function of `config`.
pub fn resolve(config: &Config) -> ResolvedChannels;
```

## Public types

```rust
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedChannels {
    pub channels: Vec<String>,
    pub channel_priority: ChannelPriority,
    pub allowlist_channels: Vec<String>,
    pub denylist_channels: Vec<String>,
    pub credential_stripping: Vec<CredentialStrippingEvent>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelListRole {
    Channels,
    AllowlistChannels,
    DenylistChannels,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialStrippingEvent {
    pub role: ChannelListRole,
    pub index: usize,
}
```

`ChannelPriority` itself is not new — it is the same, already-published,
`#[non_exhaustive]` 3-variant enum `Config::channel_priority` already
uses (GEN-36); `ResolvedChannels.channel_priority` reuses it directly
rather than introducing a parallel type.

## Behavioral guarantees (callable contract, not implementation detail)

- **Never panics, never returns an error.** Every `Config` value a caller
  can construct — including `Config::default()`, produced by `parse("")`
  — resolves to some `ResolvedChannels`.
- **Ordering is preserved.** `channels[i]`'s relative order matches the
  order its corresponding source entry (or, for a `defaults`
  substitution, the corresponding `custom_multichannels`/`default_channels`
  member) appeared in `config`.
- **Every entry in every one of the three lists is a concrete channel
  identifier** (spec.md's Concrete Channel Identifier key entity) — never
  a bare name, except the one documented out-of-scope corner (an
  explicit, empty-string `channel_alias` — see spec.md's Known
  Limitations).
- **No credential material ever appears in any of the three lists.**
  Every occurrence of URL userinfo or a `/t/<token>/` segment is stripped
  before the entry is placed into `channels`/`allowlist_channels`/
  `denylist_channels`, and reported via `credential_stripping` instead
  (FR-005).
- **`override_channels_enabled` has no effect on the output** (FR-007) —
  `resolve()` does not read that field at all.
- **No deduplication pass of its own** (FR-008) — a coincidental repeat
  across two different source entries that both resolve to the same
  concrete identifier is preserved, not collapsed.
- **`channel_settings` never appears in the output** (FR-009) —
  `resolve()` does not read that field at all.

## Non-goals (explicitly out of this contract)

- Locating, reading, or otherwise touching any file — `resolve()` takes an
  already-parsed `Config`, exactly as `parse()` takes an already-read
  `&str` (crate-wide convention, `lib.rs`'s own doc comment: "This crate
  never opens a file itself").
- Any notion of a caller's own observability conventions — `resolve()`
  only *reports* `credential_stripping`; turning that into a log record
  is the caller's job (`allez`'s contract, see
  `allez_channel_config_api.md`).
- Per-channel authentication/proxy resolution (`channel_settings`) — out
  of scope for this ticket (FR-009), deferred to GEN-29.
