# Interface Contract: `allez`'s channel-config file handling

This is a Rust in-process library API, not an HTTP/CLI interface — the
same posture GEN-24's own `ephemeral_env_api.md` documents for that
ticket. `allez oneshot` (GEN-25) is this contract's intended production
caller; it is not itself part of this ticket's scope (spec.md's "No CLI
or human-facing surface of its own" Assumption). Lives at a new
top-level module, `src/channel_config/mod.rs`, re-exported as
`allez::channel_config::resolve_channel_config` via `src/lib.rs`.

Per Constitution III (Dual-Primary Interface), this contract itself does
not need a `--format json`/human split — GEN-25 matches on the returned
`ChannelConfigResolution`: for `Ready { config, fallback }`, it inspects
`fallback` to decide how to react, then
renders `config` through the existing `output::render_*` path, the
same way GEN-24's own public API does; for `NoChannels`, there is no
`config` to render at all (FR-020).

## Public function

```rust
/// Locates and reads `~/.condarc`, parses and resolves it through the
/// `condarc` crate, and hands the result directly to GEN-24's
/// environment-creation capability as its own channel-configuration
/// input, paired with an explicit fallback signal
/// (FR-017) — or reports FR-020's zero-usable-channels case instead of
/// ever constructing an intentionally-empty channel configuration.
///
/// Always returns a fully-populated `ChannelConfigResolution` for every
/// `~/.condarc` state a caller can present — never fails, and never
/// panics on any such input. (One defended internal invariant, not a
/// caller-reachable input state, is documented under "Total function"
/// below.) A missing `~/.condarc` falls back to conda's
/// own documented default channel configuration silently, with no
/// observability record and `Ready { fallback: None, .. }` (FR-009). A
/// `~/.condarc` the `condarc` crate rejects, one that exists but cannot
/// be read due to an OS permission/I-O error, or one whose
/// `condarc::expand_channels()` call itself fails (FR-018), falls back
/// the same way but records the specific condition through this
/// project's structured observability AND via
/// `Ready { fallback: Some(FallbackReason::Rejected), .. }` /
/// `Ready { fallback: Some(FallbackReason::Unreadable), .. }`, distinct
/// from the silent missing-file case (FR-011/FR-017) — so a caller (e.g.
/// GEN-25) can decide how to react without separately consulting
/// observability output. A `~/.condarc` that resolves successfully but
/// whose allow/deny filtering (FR-019) leaves zero channels returns
/// `NoChannels` instead (FR-020) — never a `Ready` carrying an empty
/// channel configuration.
///
/// Resolves fresh from `~/.condarc` on every call. Never caches or
/// reuses a previous result across separate calls (FR-015). Never
/// writes to, modifies, or otherwise manages `~/.condarc` (FR-013).
pub fn resolve_channel_config() -> ChannelConfigResolution;

/// The result of one `resolve_channel_config` call — see `data-model.md`
/// for the full type definition and research.md R13 for why this is an
/// enum rather than a struct.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ChannelConfigResolution {
    /// A resolution that produced at least one usable channel, whether
    /// from a real, populated `~/.condarc` or from conda's own
    /// documented defaults (missing/rejected/unreadable/unexpandable
    /// fallback).
    Ready {
        /// The crate's own `condarc::ResolvedChannels`, exactly as
        /// `condarc::expand_channels()` produced it — GEN-24's own
        /// required channel-configuration input directly (FR-012/FR-016,
        /// research.md R15). `channels` is already deny/allow-filtered —
        /// FR-019 applied that upstream, inside `expand_channels()`
        /// itself.
        config: condarc::ResolvedChannels,
        /// `None` unless this call's fallback was caused by a rejected,
        /// unreadable, or unexpandable `~/.condarc` (FR-017/FR-018).
        fallback: Option<FallbackReason>,
    },
    /// A fully successful resolution whose allow/deny filtering left
    /// zero usable channels (FR-019/FR-020) — not a fallback, not an
    /// error, and never paired with a channel configuration a caller
    /// could mistakenly pass to GEN-24.
    NoChannels,
}

/// Re-exported from `src/channel_config/mod.rs` so an external caller
/// (e.g. GEN-25) can name `fallback`'s type and match its variants as
/// `allez::channel_config::FallbackReason` — see `data-model.md` for the
/// enum's own definition.
pub use events::FallbackReason;
```

## Behavioral guarantees

- **Total function**: for every possible state of `~/.condarc` within
  this ticket's supported scope (populated, absent, rejected by the
  crate, unreadable due to an OS permission/I-O error, or unexpandable
  per FR-018), this function returns a value — no `Result`/`Option`
  return type, and no panic path reachable from any such state
  (SC-002). It never constructs an
  intentionally-empty channel configuration either — `NoChannels` exists
  specifically so a caller cannot receive one (FR-020). This guarantee
  depends on one internal invariant: resolving `Config::default()`
  itself (the default-fallback path every non-`Ready`-from-real-file
  case routes through) is guaranteed to succeed, never to hit FR-018's
  error case, because the crate's own built-in default `channel_alias`
  is never the empty string that error requires. If a future change to
  the crate's own built-in defaults ever violated that invariant, this
  function would deliberately panic with a diagnostic message rather
  than silently misreport the failure as `NoChannels` or a fabricated
  fallback reason (data-model.md) — a defended, documented failure of an
  internal invariant, not a possible outcome of any `~/.condarc` content
  a caller controls.
- **Fallback is inspectable in the return value, not only via
  observability** (FR-017): `Ready.fallback` distinguishes the
  rejected/unreadable/unexpandable cases from a fully-successful
  resolution and from the silent missing-file case, so a caller can
  decide whether to warn, proceed, or abort without separately
  consulting observability output. `fallback` is `None` in exactly the
  two cases where FR-009/observability are also silent-or-successful
  (absent, or parses and expands successfully).
- **File-state → outcome mapping**:

  | `~/.condarc` state | Result | `config` | Observability | `fallback` (FR-017) |
  |---|---|---|---|---|
  | Absent | `Ready` | Conda's documented defaults (`condarc::expand_channels(&Config::default())`, always non-empty) | None (FR-009) | `None` |
  | Present, `condarc::parse` rejects it | `Ready` | Same as absent | `ChannelConfigFallbackEvent { reason: Rejected, detail: <ValidationReport text> }` (FR-011) | `Some(FallbackReason::Rejected)` |
  | Present, unreadable (OS permission/I-O error) | `Ready` | Same as absent | `ChannelConfigFallbackEvent { reason: Unreadable, detail: <io::Error text> }` (FR-011) | `Some(FallbackReason::Unreadable)` |
  | Present, parses, but `condarc::expand_channels` returns `Err` (FR-018) | `Ready` | Same as absent | `ChannelConfigFallbackEvent { reason: Rejected, detail: <ExpandChannelsError text> }` (FR-011, `Rejected` broadened per research.md R11) | `Some(FallbackReason::Rejected)` |
  | Present, parses and expands successfully, `channels` non-empty | `Ready` | `condarc::expand_channels(&config)?` | None (FR-009, ordinary success) | `None` |
  | Present, parses and expands successfully, `channels` empty (FR-019 filtering removed every entry, or the configuration otherwise resolves to an empty list) | `NoChannels` | — no channel configuration constructed (FR-020) | None (a successful resolution, not a fallback) | — no `fallback` field on this variant |

- **Never mutates `~/.condarc`** (FR-013) — this function only ever
  calls `std::fs::read_to_string` (or an equivalent read-only primitive)
  against the resolved path; no write, rename, or delete of any kind.
- **Embedded credential material passes through unchanged** in the
  resolved channel identifiers themselves, and in the fallback
  observability event's own `detail` field — this ticket's own scope
  does not strip or transform any of it.
- **No adaptation step** (FR-012/SC-001): `Ready.config` is
  `condarc::expand_channels()`'s own `ResolvedChannels` output,
  unchanged — GEN-24's environment-creation capability now takes that
  type directly as its own channel-configuration input, so there is no
  intermediate type or field-by-field mapping left to verify.
- **Retires GEN-24's now-duplicate channel-configuration behavior**
  (FR-016, research.md R15): `allez::ephemeral`'s previously-separate
  `ChannelConfig`/`ChannelSpec`/`ChannelPriorityMode` types, its
  empty-channel-list fallback (`channels_with_fallback()`), and its own
  allow/deny filtering (`filter_channels()`) are removed — each existed
  only to compensate for GEN-24 never having a fully-resolved,
  already-filtered channel list of its own to consume. GEN-24's
  defense-in-depth credential redaction (`redact_channel_url()`)
  survives, applied explicitly at each channel-identifier format site
  rather than through a retired wrapper type's `Debug` impl.

## Non-goals (explicitly out of this contract)

- Any CLI flag, subcommand, or `--format` surface of its own (GEN-25's
  job).
- Any policy for what a caller does with a `Some(FallbackReason)`
  fallback signal (warn, abort, ignore) — GEN-25's own job; this
  contract only guarantees the signal is present and accurate (FR-017).
- Any policy for what a caller does with a `NoChannels` result (warn,
  abort, proceed with no environment) — GEN-25's own job; this contract
  only guarantees a caller can never mistake it for a `Ready` carrying an
  empty channel configuration (FR-020).
- Applying the resolved channel configuration to an actual environment
  creation — this function's return value is an *input* to
  `create_ephemeral_environment` (GEN-24), not a call to it.
- Any notion of a `CONDARC` environment-variable override or a
  multi-source conda search path — this ticket's scope is `~/.condarc`
  only, matching GEN-36's own already-documented single-document,
  no-merge boundary.
- Retrying, watching, or invalidating a previous result — every call is
  independent and reads fresh (FR-015); there is no caching layer to
  invalidate.
