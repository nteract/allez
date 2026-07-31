# Quickstart: Validating `.condarc` Channel Resolution

This is a validation/run guide, not an implementation walkthrough — see
`contracts/condarc_resolve_api.md` and `contracts/allez_channel_config_api.md`
for the two public APIs this feature adds, and `data-model.md` for every
type. Task-by-task implementation breakdown is a separate, later
artifact (`tasks.md`, produced by `/speckit.tasks`).

## Prerequisites

- Rust toolchain matching the workspace's `edition = "2024"`.
- No network access required for any test this feature adds — `resolve()`
  is a pure, hermetic function (research.md R1/R4) and `allez`'s own
  file-handling layer only ever reads a local path. Unlike
  `condarc_conformance`/`network-tests`, there is no opt-in feature flag
  to enable for this feature's own tests; `cargo test --all` runs
  everything.
- The one test that exercises the real, zero-argument
  `allez::channel_config::resolve_channel_config()` public entry point
  does so **out of process**: it re-executes **the test binary itself**
  — the already-compiled executable `std::env::current_exe()` points at,
  guaranteed to exist because it is the binary currently running — as a
  child process, with `HOME` set on that child's own `Command`
  environment, pointing at a `tempfile::tempdir()` that contains a known
  `.condarc`, and asserts on the child's stdout. Because a child process
  inherits its environment at creation time rather than sharing it, this
  test needs no test-ordering attribute, no `unsafe` block, and no
  mutation of the test binary's own environment — there is no
  process-wide environment race to isolate against in the first place
  (research.md R10). The spawning mechanism is settled and needs no
  manifest change at all: which of the two branches the process takes is
  selected by a dedicated marker environment variable set only on the
  child (`__CHANNEL_CONFIG_SMOKE_CHILD=1`, a name no production code ever
  reads). Unset (the ordinary `cargo test` invocation), the test spawns
  the child and asserts; set (the spawned process, which is the same test
  binary reaching the same test function again), it instead calls
  `resolve_channel_config()` directly and prints a well-defined result to
  stdout wrapped in two unique sentinel markers, then **returns
  normally** as an ordinary passing test — it does not call
  `std::process::exit()`, which would terminate the whole
  multi-threaded test process and could leave other concurrently-running
  test threads reported as killed rather than completing. The parent also
  passes three CLI arguments to the re-executed binary: that test
  function's own libtest name, `--exact`, and `--nocapture`. The first
  two restrict the child to running only this one test (a libtest harness
  selects tests from its command line, not from the environment, so
  without them the child would re-run every test in this binary under the
  redirected `HOME`); `--nocapture` is what lets the child's `println!`
  output actually reach the parent's captured stdout, since libtest
  otherwise captures a test's stdout internally and releases it only when
  that test fails. The parent asserts the child's exit status was
  successful first, then extracts and compares only the
  sentinel-delimited region, ignoring libtest's own `running 1
  test`/`test result: ok.` lines in that same stdout. Since the spawned
  path is an already-built executable rather than a fresh `cargo`
  invocation, no `cargo` process ever runs under a redirected `HOME`, so
  Cargo's own toolchain/registry discovery is unaffected — and this adds
  zero new Cargo dependencies and no manifest-declared `[[bin]]` target
  (the one thing a prior round's fix specifically removed);
  `examples/channel_config_smoke.rs` remains exactly the plain,
  Cargo-auto-discovered example already planned for this ticket from the
  start (its own manual-smoke-test purpose, below) — an existing planned
  target, not a new one introduced by this test-mechanism decision.
  `std::process::Command` plus `std::env::current_exe()` is the whole
  mechanism (research.md R10).
  That same test is `#[cfg(unix)]`-gated
  for one unrelated reason only: Windows's `dirs::home_dir()` resolves via
  `SHGetKnownFolderPath` and does not consult `USERPROFILE` at all, so no
  environment value — in this process or a child's — would redirect it
  (research.md R10). Every other test in this
  feature drives the path-injectable internal function instead and needs
  no isolation of any kind.

## Setup

```sh
cargo build --all
```

No environment variables are required to exercise either public function
directly. `crates/condarc`'s `resolve()` needs nothing beyond an
in-memory `Config` (construct one via `condarc::parse(...)`, or use
`Config::default()` for the "nothing configured" case).
`allez::channel_config::resolve_channel_config()` needs nothing beyond a
resolvable home directory — which every development/CI machine already
has.

## Run the full test suite

```sh
cargo test --all
```

This must include, at minimum, one test per acceptance scenario in
`spec.md` (Constitution VIII: 100% spec test coverage). Exact test
names are decided during task breakdown; the mapping below is
non-exhaustive but covers every `SC-00n` this ticket's spec defines.

### `crates/condarc` (`resolve()`, User Stories 1 and 3)

- **SC-003's 20 named scenarios** — one test per row, each asserting the
  exact resolved value for that specific `.condarc` shape (bare-name
  resolution via each of the four FR-001 precedence branches; all four
  `defaults`-substitution triggers — explicit `[defaults]`, absent,
  explicit `null`, explicit `[]`; a user-configured, non-built-in
  `default_channels` value actually substituting; all four
  `channel_priority` modes including the two legacy boolean spellings and
  the absent-defaults-to-`flexible` case; allow/deny entries requiring
  FR-001 expansion; both alias-collision malformed-input cases — proven
  by asserting `condarc::parse` itself returns `Err`, since `resolve()`
  never runs on that input at all, research.md R3).
- **User Story 1 Acceptance Scenario 5** — a `custom_multichannels`
  member naming another multichannel, a `custom_channels` entry, or the
  multichannel being defined itself → resolved as an ordinary bare name
  via `channel_alias`, proving `resolve_member`'s restricted precedence
  (research.md R5) rather than a full recursive expansion.
- **`custom_channels` progressive-prefix match** — `custom_channels:
  {acme: "https://internal.example.com"}`, entry `"acme/label/dev"` →
  `"https://internal.example.com/acme/label/dev"` (research.md R6's
  worked example, mirroring `docs/condarc_research.md`'s own
  `pkgs/pro` pairing).
- **SC-005's 6 credential-bearing-location scenarios** — a
  credential-bearing entry in `channel_alias`, `custom_channels`,
  `default_channels`, a `custom_multichannels` member, directly in a
  fully-qualified `channels` URL, and in an `allowlist_channels`/
  `denylist_channels` entry → each resolves with credentials stripped
  from the output *and* produces exactly one matching
  `CredentialStrippingEvent` (correct `role`/`index`).
- **FR-007** — a `.condarc` setting `override_channels_enabled` (either
  value) has zero effect on the resolved output (assert `resolve()`'s
  output is identical with and without that key set).
- **FR-008** — two different bare names that happen to expand to the
  same concrete URL both survive in `channels`, uncollapsed.
- **FR-009** — a `.condarc` setting `channel_settings` never causes any
  entry to appear in any of `ResolvedChannels`'s three lists, and the
  setting itself is never read.
- **User Story 3 Acceptance Scenarios 1/2/5/6** — `channel_priority`
  already-coerced-by-`parse()` passthrough for all three modes, the
  absent-defaults-to-`Flexible` case, and the two legacy boolean
  spellings, confirming `resolve()` performs no re-coercion of its own
  (research.md R2).
- **`Config::default()` (the empty-document case)** — resolves to
  `channels: [<the three/two built-in DEFAULT_CHANNELS URLs>]`,
  `channel_priority: Flexible`, both allow/deny lists empty, no
  credential-stripping events — this is exactly what `allez`'s own
  FR-010/FR-012 fallback path relies on producing.

### `allez` (`resolve_channel_config`, User Story 2, SC-001/SC-002/SC-004)

- **SC-002's four file-state cases** — missing, crate-rejected,
  unreadable (a file written with **invalid UTF-8 bytes**:
  `std::fs::read_to_string` requires valid UTF-8 and so fails
  deterministically with `io::ErrorKind::InvalidData` on every target
  platform, exercising the same `ReadOutcome::Unreadable(io::Error)`
  branch a real permission denial would, with no `chmod`, no Windows ACL
  manipulation, and no `cfg`-gating — research.md R10; a real
  permission-denied file is the illustrative production case that branch
  exists for, not the mechanism this test uses, deliberately, since Unix
  permission bits are bypassed entirely under `root` and root-run CI
  would silently no-op such a fixture into the success path —
  research.md R10), and populated — each via
  `resolve_channel_config_from(Some(path))`/`None`, asserting a valid,
  fully-populated `ChannelConfig` in every case (User Story 2 Scenarios
  1–4), and asserting `ChannelConfigResolution.fallback`'s exact value
  for each — `None`, `Some(Rejected)`, `Some(Unreadable)`, `None`
  respectively (FR-019) — plus `resolve_channel_config_from(None)` (the
  home-directory-undeterminable case) exercised as its own distinct test
  case rather than folded into the general "missing" description, since
  `data-model.md`'s own doc comment on `resolve_channel_config_from`
  specifically distinguishes `path: None` from
  `Some(<nonexistent path>)` even though both take the same silent
  fallback path (FR-010).
- **SC-001** — a dedicated contract test constructing
  `allez::ephemeral::ChannelConfig` directly from `condarc::resolve`'s
  output for at least 5 distinct, real-world-shaped `.condarc` samples
  (e.g. a plain `channels: [conda-forge, defaults]`; one exercising
  `custom_channels`+`custom_multichannels` together; one setting
  `allowlist_channels`/`denylist_channels`; one with credential-bearing
  entries; one relying purely on defaults), confirming the field-by-field
  mapping (FR-014) produces the exact same `ChannelConfig` a hand-written
  expected value would.
- **SC-004** — one test per fallback path (rejected, unreadable)
  asserting a `ChannelConfigFallbackEvent` is actually emitted (captured
  via a `tracing` test subscriber, the same technique GEN-24's own
  `quickstart.md` describes for `EphemeralLifecycleEvent`), carrying the
  crate's own per-problem detail for the rejected case and a distinct
  signal for the unreadable case — and that **zero** such events are
  emitted for the silent missing-file case (FR-010). These run as
  co-located `#[cfg(test)]` unit tests in `src/channel_config/`, driven
  through `resolve_channel_config_from(Some(path))`, not as integration
  tests — that function is `pub(crate)` and a `tests/` file compiles as a
  separate crate that cannot call it (research.md R10).
- **FR-013** — one test per credential-bearing location, mirroring the
  crate-level SC-005 bullet's own 6 cases but now asserted at the `allez`
  boundary too: a credential-bearing entry in `channel_alias`, in
  `custom_channels`, in `default_channels`, in a `custom_multichannels`
  member, directly in a fully-qualified `channels` URL, and in an
  `allowlist_channels`/`denylist_channels` entry — each exercised through
  the full `allez` pipeline (read → parse → resolve → adapt), asserting
  both that the returned `ChannelConfig` contains no raw credential
  material *and* that exactly one matching `CredentialStripLogRecord`
  (correct `role`/`index`) was emitted. Plus: never emits one for the
  rejected/unreadable fallback path (since `condarc::resolve` never runs
  there). Like the SC-004 bullet above, these run as co-located
  `#[cfg(test)]` unit tests in `src/channel_config/` via
  `resolve_channel_config_from`, for the same `pub(crate)`-visibility
  reason (research.md R10).
- **FR-015** — after any `resolve_channel_config_from` call against a
  real file (any of the four SC-002 states), the file's own modification
  time and contents are unchanged.
- **FR-016** — a `.condarc` containing an unrecognized, unrelated
  top-level key resolves exactly as if that key were absent (inherited
  from the crate's own unknown-key tolerance; `Config::extra` is simply
  never consulted by `resolve()`).
- **FR-017** — two consecutive calls to `resolve_channel_config_from`
  against the *same* path, where the file's contents change between
  calls, produce two *different* results, proving no caching occurs.
- **The real, zero-argument public entry point** — one subprocess-based
  test (see Prerequisites above) that re-executes the test binary itself
  via `std::env::current_exe()`, with `HOME` set on the child's own
  environment to a fresh `tempfile::tempdir()` containing a known
  `.condarc` plus the marker environment variable that makes the child
  call `resolve_channel_config()` with no arguments and print its result
  wrapped in unique sentinel markers, and with that test function's own
  libtest name, `--exact`, and `--nocapture` passed as CLI arguments so
  the child runs only this one test and its printed result actually
  reaches the parent's captured stdout. The child returns normally rather
  than calling `std::process::exit()`, and the parent asserts the child's
  exit status was successful *before* extracting the sentinel-delimited
  region — then asserts that extracted
  output (channel list, `channel_priority`, allow/deny sizes, and
  the FR-019 fallback line if any) matches a **hand-derived expected
  output** for that one known fixture, hard-coded in the test the same way
  the SC-001 and SC-003 bullets above hard-code theirs. The test does not
  call `resolve_channel_config_from` to compute that expectation — that
  function is `pub(crate)` and unreachable from a `tests/` file, which
  Rust compiles as a separate crate (research.md R10). This is the one
  proof that `dirs::home_dir()` resolution itself is wired correctly.

## Manual smoke test (optional, illustrative)

`examples/channel_config_smoke.rs` exercises the full read → parse →
resolve → adapt pipeline against whatever `~/.condarc` (if any) exists on
the machine running it:

```rust
fn main() {
    let resolution = allez::channel_config::resolve_channel_config();
    if let Some(reason) = resolution.fallback {
        println!("fell back due to: {reason:?}"); // FR-019 — visible without reading logs
    }
    let config = resolution.config;
    println!("channel_priority: {:?}", config.channel_priority);
    println!("channels:");
    for channel in &config.channels {
        println!("  {channel:?}"); // ChannelSpec's own Debug already redacts credentials
    }
    println!("allowed: {} entries, denied: {} entries", config.allowed_channels.len(), config.denied_channels.len());
}
```

Run it with:

```sh
cargo run --example channel_config_smoke
```

Expected outcome: prints an ordered channel list — non-empty in the
common case (at minimum the built-in `defaults` URLs, if no `~/.condarc`
exists or configures none explicitly), though spec.md's own Assumptions
note the list can legitimately be empty if the machine's own `~/.condarc`
explicitly configures an empty `custom_multichannels.defaults` or
`default_channels` — the effective channel-priority mode, and the
allow/deny list sizes — with no raw credential material ever printed,
even if the running machine's own `~/.condarc` happens to contain any
(its `ChannelSpec::Debug` impl, GEN-24's own existing defense-in-depth,
redacts it regardless of this ticket's own upstream stripping). If the
machine's own `~/.condarc` is rejected or unreadable, an additional
`fell back due to: ...` line prints first (FR-019); nothing prints there
for the ordinary missing-file case.

## Validating "never writes to `~/.condarc`" manually

1. Note `~/.condarc`'s modification time and a checksum of its contents
   (or create a throwaway one in a scratch home directory for this
   check, to avoid touching a real development machine's own file).
2. Run the smoke test above, or any of the SC-002 integration tests,
   against that file.
3. Confirm the modification time and checksum are unchanged.
