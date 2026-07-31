# Interface Contract: `allez`'s channel-config file handling and adaptation

This is a Rust in-process library API, not an HTTP/CLI interface — the
same posture GEN-24's own `ephemeral_env_api.md` documents for that
ticket. `allez oneshot` (GEN-25) is this contract's intended production
caller; it is not itself part of this ticket's scope (spec.md's "No CLI
or human-facing surface of its own" Assumption). Lives at a new
top-level module, `src/channel_config/mod.rs`, re-exported as
`allez::channel_config::resolve_channel_config` via `src/lib.rs`.

Per Constitution III (Dual-Primary Interface), this contract itself does
not need a `--format json`/human split — GEN-25 inspects the returned
`ChannelConfigResolution`'s `fallback` field to decide how to react, then
renders its `config` through the existing `output::render_*` path, the
same way GEN-24's own public API does.

## Public function

```rust
/// Locates and reads `~/.condarc`, parses and resolves it through the
/// `condarc` crate, and adapts the result into GEN-24's
/// `ChannelConfig` shape, paired with an explicit fallback signal
/// (FR-019).
///
/// Always returns a fully-populated `ChannelConfigResolution` — never
/// fails, and never panics. A missing `~/.condarc` falls back to conda's
/// own documented default channel configuration silently, with no
/// observability record and `fallback: None` (FR-010). A `~/.condarc`
/// the `condarc` crate rejects, or one that exists but cannot be read
/// due to an OS permission/I-O error, falls back the same way but
/// records the specific condition through this project's structured
/// observability AND via `fallback: Some(FallbackReason::Rejected)` /
/// `Some(FallbackReason::Unreadable)`, distinct from the silent
/// missing-file case (FR-012/FR-019) — so a caller (e.g. GEN-25) can
/// decide how to react without separately consulting observability
/// output. Every credential-stripping event the crate's `resolve()`
/// reports for this call is also recorded (FR-013).
///
/// Resolves fresh from `~/.condarc` on every call. Never caches or
/// reuses a previous result across separate calls (FR-017). Never
/// writes to, modifies, or otherwise manages `~/.condarc` (FR-015).
pub fn resolve_channel_config() -> ChannelConfigResolution;

/// The result of one `resolve_channel_config` call — see `data-model.md`
/// for the full type definition. `config` is GEN-24's own, unmodified
/// `allez::ephemeral::ChannelConfig`; `fallback` is `None` unless this
/// call's fallback was caused by a rejected or unreadable `~/.condarc`
/// (FR-019).
pub struct ChannelConfigResolution {
    pub config: allez::ephemeral::ChannelConfig,
    pub fallback: Option<FallbackReason>,
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
  crate, or unreadable due to an OS permission/I-O error), this function
  returns a value — it has no `Result`/`Option` return type and no panic
  path (SC-002).
- **Fallback is inspectable in the return value, not only via
  observability** (FR-019): `fallback` distinguishes the
  rejected/unreadable cases from a fully-successful resolution and from
  the silent missing-file case, so a caller can decide whether to warn,
  proceed, or abort on that condition without separately consulting
  observability output. `fallback` is `None` in exactly the two cases
  where FR-010/observability are also silent-or-successful (absent, or
  parses successfully).
- **File-state → outcome mapping**:

  | `~/.condarc` state | `config` | Observability | `fallback` (FR-019) |
  |---|---|---|---|
  | Absent | Conda's documented defaults (`condarc::resolve(&Config::default())`) | None (FR-010) | `None` |
  | Present, `condarc::parse` rejects it | Same as absent | `ChannelConfigFallbackEvent { reason: Rejected, detail: <ValidationReport> }` (FR-012) | `Some(FallbackReason::Rejected)` |
  | Present, unreadable (OS permission/I-O error) | Same as absent | `ChannelConfigFallbackEvent { reason: Unreadable, detail: <io::Error> }` (FR-012) | `Some(FallbackReason::Unreadable)` |
  | Present, parses successfully | `adapt(condarc::resolve(&config))` | `CredentialStripLogRecord` per stripping event, if any (FR-013) | `None` |

- **Never mutates `~/.condarc`** (FR-015) — this function only ever
  calls `std::fs::read_to_string` (or an equivalent read-only primitive)
  against the resolved path; no write, rename, or delete of any kind.
- **Every credential-stripping event the crate reports is recorded**
  (FR-013), and only those events — a malformed-file fallback (FR-012)
  never itself produces a `CredentialStripLogRecord`, since `condarc::resolve`
  is never invoked on that path (`condarc::parse` already returned
  `Err` before resolution could run).
- **Adaptation is lossless and field-by-field** (FR-014/SC-001): the
  returned `ChannelConfig`'s four fields are populated directly from
  `condarc::ResolvedChannels`'s four corresponding fields, with no
  additional resolution or transformation logic — verifiable by
  constructing both independently from the same `.condarc` sample and
  comparing.
- **No effect on GEN-24's own existing behavior** (FR-018): this
  function never edits, and its own logic never re-implements,
  `allez::ephemeral`'s existing empty-channel-list fallback, allow/deny
  filtering, or defense-in-depth credential redaction — it only
  constructs a `ChannelConfig` value for a caller (e.g. GEN-25) to pass
  into `create_ephemeral_environment` unchanged, exactly as any other
  caller of that already-published function would.

## Non-goals (explicitly out of this contract)

- Any CLI flag, subcommand, or `--format` surface of its own (GEN-25's
  job).
- Any policy for what a caller does with a `Some(FallbackReason)`
  fallback signal (warn, abort, ignore) — GEN-25's own job; this
  contract only guarantees the signal is present and accurate (FR-019).
- Applying `ChannelConfig` to an actual environment creation — this
  function's return value is an *input* to `create_ephemeral_environment`
  (GEN-24), not a call to it.
- Any notion of a `CONDARC` environment-variable override or a
  multi-source conda search path — this ticket's scope is `~/.condarc`
  only, matching GEN-36's own already-documented single-document,
  no-merge boundary.
- Retrying, watching, or invalidating a previous result — every call is
  independent and reads fresh (FR-017); there is no caching layer to
  invalidate.
