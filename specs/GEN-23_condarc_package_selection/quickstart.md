# Quickstart: Validating `.condarc` Channel Resolution

This is a validation/run guide, not an implementation walkthrough — see
`contracts/condarc_resolve_api.md` and `contracts/allez_channel_config_api.md`
for the two public APIs this feature adds, and `data-model.md` for every
type.

## Prerequisites

- Rust toolchain matching the workspace's `edition = "2024"`.
- No network access required for any test this feature adds —
  `expand_channels()` is a pure, hermetic function (research.md R1/R4)
  and `allez`'s file-handling layer only reads a local path when one is
  explicitly given. Unlike `condarc_conformance`/`network-tests`, there's
  no opt-in feature flag; `cargo test --all` runs everything automated.
- There is deliberately no automated test of the real, zero-argument
  `allez::channel_config::resolve_channel_config()` entry point: reading
  whatever `.condarc` exists on the test machine would be
  non-deterministic (Constitution II) and risks the crate's own
  documented stack-depth-guard gap against an unknown real file (spec.md
  Known Limitations). Its only coverage is the manual, optional
  `examples/channel_config_smoke.rs` (below), run deliberately by a
  developer, never by `cargo test` (research.md R8). Every automated
  `allez`-level test drives the path-injectable internal function
  directly, using `tempfile` fixtures; the crate-level
  `expand_channels()` tests (SC-003) call `expand_channels()`
  directly, a second path touching neither entry point's file handling.

## Setup

```sh
cargo build --all
```

No environment variables are required to exercise either public
function directly. `crates/condarc`'s `expand_channels()` needs nothing
beyond an in-memory `Config` (construct one via `condarc::parse(...)`,
or use `Config::default()` for the "nothing configured" case).
`allez::channel_config::resolve_channel_config()` handles the resolvable
and the undeterminable home-directory case identically (FR-009) — no
setup is needed either way.

## Run the full test suite

```sh
cargo test --all
```

This must include, at minimum, one test per acceptance scenario in
`spec.md` (Constitution VIII: 100% spec test coverage). Exact test
names are decided during task breakdown; the mapping below is
non-exhaustive but covers every `SC-00n` this ticket's spec defines.

## Quality gates

The same gates Constitution's own Quality Gates section requires for
every change, run against this ticket's one changed manifest
(root `Cargo.toml`, gaining `dirs` and promoting `condarc`) and new
source files:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo audit
cargo deny check
cargo doc --no-deps
```

`cargo audit`/`cargo deny check` apply even though `dirs` is only
promoted from transitive to direct (no new package enters the resolved
graph) — the gate runs on every manifest change, not only ones that add
a new dependency. `cargo test --all` above already covers all four
target platforms per the workspace's own CI matrix.

### `crates/condarc` (`expand_channels()`, User Stories 1 and 3)

- **SC-003's 22 named scenarios** — one test per row, each asserting the
  exact resolved value for that specific `.condarc` shape (bare-name
  resolution via each of the four FR-001 precedence branches; all four
  `defaults`-substitution triggers — explicit `[defaults]`, absent,
  explicit `null`, explicit `[]`; a user-configured, non-built-in
  `default_channels` value actually substituting; all three
  `channel_priority` modes, the absent-defaults-to-`flexible` case, and
  the two legacy boolean spellings; a `denylist_channels`/`allowlist_channels`
  entry requiring FR-001 expansion before it matches and
  removes/retains the corresponding `channels` entry, per FR-019; both
  alias-collision malformed-input cases — proven by asserting
  `condarc::parse` itself returns `Err`, since `expand_channels()` never
  runs on that input, research.md R3; a `channel_alias` with a trailing
  slash, asserting exactly one slash in the joined result; and a
  dot-containing bare name that doesn't match FR-001(a)'s scheme
  pattern, asserting it still resolves via `channel_alias`).
- **SC-005** — a `.condarc` resolving a channel-list entry through an
  effective, explicit empty-string `channel_alias` asserts
  `expand_channels()` returns `Err(ExpandChannelsError::EmptyChannelAlias { entry })`
  with the exact triggering entry (FR-018, research.md R11).
- **User Story 1 Acceptance Scenario 5** — a `custom_multichannels`
  member naming another multichannel, a `custom_channels` entry, or the
  multichannel being defined itself → resolved as an ordinary bare name
  via `channel_alias`, proving `resolve_member`'s restricted precedence
  (research.md R5) rather than full recursive expansion.
- **`custom_channels` progressive-prefix match** — `custom_channels:
  {acme: "https://internal.example.com"}`, entry `"acme/label/dev"` →
  `"https://internal.example.com/acme/label/dev"` (research.md R6's
  worked example, mirroring `docs/condarc_research.md`'s own `pkgs/pro`
  pairing).
- **FR-006** — a `.condarc` setting `override_channels_enabled` (either
  value) has zero effect on the resolved output.
- **FR-007** — two different bare names that happen to expand to the
  same concrete URL both survive in `channels`, uncollapsed. Since
  FR-019's filtering matches by identifier value, this pair either both
  survive or both get removed together — never just one.
- **FR-008** — a `.condarc` setting `channel_settings` never causes any
  entry to appear in `ResolvedChannels.channels`, and the setting itself
  is never read.
- **User Story 1 Acceptance Scenario 4** — a `.condarc` setting
  `channels` to a non-empty list that omits `defaults` resolves to only
  the channels derived from that list — `default_channels` is never
  appended alongside it.
- **FR-002 explicit-empty rows** — `default_channels: []` (with
  `channels: [defaults]` and no `custom_multichannels.defaults` entry)
  resolves to an empty `channels` list, and `custom_channels: {}`
  resolves a bare name that would otherwise hit conda's built-in
  `custom_channels` mapping via `channel_alias` instead — neither
  explicit empty value is replaced by its built-in default.
- **User Story 3 Acceptance Scenarios 1/2/5** — `channel_priority`
  already-coerced-by-`parse()` passthrough for all three modes, the
  absent-defaults-to-`Flexible` case, and the two legacy boolean
  spellings, confirming `expand_channels()` performs no re-coercion of
  its own (research.md R2).
- **User Story 3 Acceptance Scenario 3** — a `.condarc` setting both
  `allowlist_channels` and `denylist_channels`, including one channel
  present in both, asserting the resulting `channels` has every denied
  entry removed (checked first) and every entry not in a non-empty
  allow-list removed too, with the both-lists entry specifically absent
  (FR-004/FR-019 — deny wins on conflict, see spec.md's Edge Cases and
  Design Decisions).
- **`Config::default()` (the empty-document case)** — resolves to
  `channels: [<the three/two built-in DEFAULT_CHANNELS URLs>]`,
  `channel_priority: Flexible` — exactly what `allez`'s own
  FR-009/FR-011 fallback path relies on producing, and confirms
  `expand_channels(&Config::default())` cannot reach the
  `EmptyChannelAlias` `Err` branch (`channel_alias` defaults to the
  non-empty built-in alias, FR-002).

### `allez` (`resolve_channel_config`, User Story 2, SC-001/SC-002/SC-004/SC-005/SC-006)

- **FR-010** — satisfied by construction, not a dedicated test: every
  test in this section exercises the real `condarc::parse` and real
  `condarc::expand_channels`, never a private reimplementation of either
  — SC-002/SC-004/SC-005/SC-006 via `resolve_channel_config_from`,
  SC-001 via a direct `expand_channels()` call, asserted against
  `resolve_channel_config_from`'s own `Ready.config`
  (contracts/allez_channel_config_api.md's own file-state table names
  both calls explicitly). No separate assertion needed beyond what those
  already exercise end-to-end.
- **SC-002's five file-state cases, plus one argument-level case (six tests total)** — missing, crate-rejected,
  unreadable (a file written with invalid UTF-8 bytes: `read_to_string`
  fails deterministically with `io::ErrorKind::InvalidData` on every
  target platform, exercising the same `ReadOutcome::Unreadable(io::Error)`
  branch a real permission denial would, with no `chmod`, ACL
  manipulation, or `cfg`-gating — research.md R8; a real
  permission-denied file is the illustrative production case, not the
  test mechanism, since Unix permission bits are bypassed under `root`
  and root-run CI would silently no-op such a fixture into the success
  path), one triggering `condarc::expand_channels()`'s own `Err`
  (FR-018, see SC-005 below), and populated — each via
  `resolve_channel_config_from(Some(path))`/`None`, asserting
  `ChannelConfigResolution::Ready { config, fallback }` with a valid,
  fully-populated channel configuration in every case (User Story 2 Scenarios
  1–3, plus the populated/happy-path case — Scenario 4 is the separate
  never-mutates guarantee, covered by its own FR-013 bullet below), and
  asserting `fallback`'s exact value for each — `None`, `Some(Rejected)`,
  `Some(Unreadable)`, `Some(Rejected)`, `None` respectively (FR-017, the
  expansion-failure case sharing `Rejected` per research.md R11) — plus
  `resolve_channel_config_from(None)` (the home-directory-undeterminable
  case) exercised as its own distinct test case, since `data-model.md`'s
  own doc comment specifically distinguishes `path: None` from
  `Some(<nonexistent path>)` even though both take the same silent
  fallback path (FR-009).
- **SC-001** — a co-located `#[cfg(test)]` unit test inside
  `src/channel_config/mod.rs`, for at least 5 distinct, real-world-shaped
  `.condarc` samples (e.g. a plain `channels: [conda-forge, defaults]`;
  one exercising `custom_channels`+`custom_multichannels` together; one
  whose `channels` mixes the `defaults` placeholder with an
  already-fully-qualified URL entry; one with `channel_priority` set via
  a legacy boolean spelling; one setting `allowlist_channels`/
  `denylist_channels`), asserting `resolve_channel_config_from`'s
  `Ready.config` equals `condarc::expand_channels(&config)`'s own output
  exactly (FR-012, research.md R15) — there is no mapping logic left to
  verify now that `Ready.config` is `condarc::ResolvedChannels` directly,
  so this test guards against `mod.rs` ever silently wrapping or
  transforming that value, not against a mapping bug.
- **SC-004** — one test per fallback path (a `parse()` rejection, an
  `expand_channels()` failure per FR-018, and an unreadable file)
  asserting a `ChannelConfigFallbackEvent` is actually emitted (captured
  via a per-test-scoped `tracing::subscriber::with_default`, never a
  process-global subscriber, so this stays independent of every other
  test running in parallel — the same technique GEN-24's
  `quickstart.md` describes for `EphemeralLifecycleEvent`), carrying the
  crate's own per-problem detail for each rejected-equivalent case
  (`ValidationReport`'s or `ExpandChannelsError`'s own `Display` text)
  and a distinct signal for the unreadable case — and that **zero** such
  events are emitted for the silent missing-file case (FR-009). These
  run as co-located `#[cfg(test)]` unit tests in `src/channel_config/`,
  driven through `resolve_channel_config_from(Some(path))`, not as
  integration tests, since that function is `pub(crate)` (research.md
  R8).
- **SC-005** — an `allez`-level counterpart to the crate-level SC-005
  test above: a `.condarc` triggering `expand_channels()`'s
  `EmptyChannelAlias` `Err` asserts `resolve_channel_config_from` falls
  back exactly like a `parse()`-rejected file —
  `ChannelConfigResolution::Ready { fallback: Some(FallbackReason::Rejected), .. }`,
  with `ChannelConfigFallbackEvent.detail` carrying the
  `ExpandChannelsError`'s own `Display` text.
- **SC-006** — a `.condarc` whose `channels` is non-empty before
  filtering but whose `allowlist_channels`/`denylist_channels` remove
  every entry (FR-019) asserts `resolve_channel_config_from` returns
  `ChannelConfigResolution::NoChannels` — not `Ready` with an empty
  channel configuration. This distinction remains essential even though
  GEN-24's own `channels_with_fallback` is retired (research.md R15):
  GEN-24's environment-creation capability now receives
  `condarc::ResolvedChannels` directly, with no fallback net of its own
  left to catch an ambiguous empty list (FR-020, research.md R13). A
  separate, non-filtering-caused case — a `.condarc` setting
  `custom_multichannels: {defaults: []}` with no `allowlist_channels`/
  `denylist_channels` involved — asserts the same `NoChannels` result,
  covering FR-020's other legitimate cause (spec.md Design Decisions,
  "Empty resolved list, two legitimate causes").
- **FR-013** — after any `resolve_channel_config_from` call against a
  real file (each of SC-002's four file-exists states: populated,
  rejected, unreadable, expansion-failing), the file's own modification
  time and contents are unchanged.
- **FR-014** — a `.condarc` containing an unrecognized, unrelated
  top-level key resolves exactly as if that key were absent (inherited
  from the crate's own unknown-key tolerance; `Config::extra` is never
  consulted by `expand_channels()`).
- **FR-015** — two consecutive calls to `resolve_channel_config_from`
  against the same path, where the file's contents change between
  calls, produce two different results, proving no caching occurs.


## Manual smoke test (optional, illustrative)

`examples/channel_config_smoke.rs` exercises the full read → parse →
resolve pipeline against whatever `~/.condarc` (if any) exists on
the machine running it:

```rust
use allez::ephemeral::redact_channel_url;

fn main() {
    match allez::channel_config::resolve_channel_config() {
        allez::channel_config::ChannelConfigResolution::Ready { config, fallback, .. } => {
            if let Some(reason) = fallback {
                println!("fell back due to: {reason:?}"); // FR-017 — visible without reading logs
            }
            println!("channel_priority: {:?}", config.channel_priority);
            println!("channels:");
            for channel in &config.channels {
                // redact_channel_url applied explicitly at this format site —
                // ResolvedChannels no longer carries a redacting Debug impl of
                // its own (research.md R15); GEN-24's own credential-redaction
                // property is preserved as a plain function call instead.
                println!("  {:?}", redact_channel_url(channel));
            }
        }
        allez::channel_config::ChannelConfigResolution::NoChannels => {
            // FR-020 — a fully successful resolution whose allow/deny
            // filtering removed every channel; never printed as an empty list.
            println!("no usable channels after allow/deny filtering");
        }
        // `ChannelConfigResolution` is `#[non_exhaustive]` (data-model.md) —
        // this example is a separate crate (Cargo compiles `examples/` as
        // such), so a wildcard arm is required for a future, additive
        // variant, exactly like matching `condarc::ChannelPriority`
        // elsewhere in this ticket's own scope.
        _ => {}
    }
}
```

Run it with:

```sh
cargo run --example channel_config_smoke
```

Expected outcome: prints an ordered channel list — non-empty in the
common case (at minimum the built-in `defaults` URLs, if no `~/.condarc`
exists or configures none explicitly) — and the effective
channel-priority mode, with no raw credential material ever printed,
even if the running machine's own `~/.condarc` happens to contain any
(`redact_channel_url()`, GEN-24's own existing defense-in-depth,
applied explicitly at this print site — research.md R15). If the
machine's own
`~/.condarc`'s allow/deny filtering (FR-019) legitimately removes every
channel, this prints the distinct `NoChannels` message above instead of
an empty list (FR-020) — one of two legitimate causes of an
otherwise-empty resolution (spec.md Assumptions). If the machine's own
`~/.condarc` is rejected, unreadable, or fails to expand (FR-018), an
additional `fell back due to: ...` line prints first (FR-017); nothing
prints there for the ordinary missing-file case.

## Validating "never writes to `~/.condarc`" manually

1. Note `~/.condarc`'s modification time and a checksum of its contents
   (or create a throwaway one in a scratch home directory for this
   check, to avoid touching a real development machine's own file).
2. Run the smoke test above, or any of the SC-002 unit tests,
   against that file.
3. Confirm the modification time and checksum are unchanged.
