# Phase 0 Research: `.condarc` Channel Resolution

This ticket's work spans two codebase locations (see spec.md Operating
Context): a new, additive `resolve` capability inside the `condarc` crate
(`crates/condarc`), and a new file-handling/adaptation module inside
`allez` itself. Both are covered below. Neither location needed an
external research pass (no new third-party library, no unfamiliar
protocol) — every open question was a design decision resolvable directly
from the spec, GEN-36's already-delivered crate, GEN-24's already-delivered
`ChannelConfig`, and `docs/condarc_research.md`'s already-verified conda
default constants. There are no "NEEDS CLARIFICATION" markers left in
Technical Context.

## R1 — `resolve()`'s input/output shape and location

**Decision**: A new private module, `crates/condarc/src/resolve.rs`,
exposing one pure function, `resolve(config: &Config) -> ResolvedChannels`,
plus the two small supporting types `ResolvedChannels` and
`ChannelListRole`/`CredentialStrippingEvent`. Re-exported from `lib.rs`
alongside the crate's existing selective `pub use` list, following the
same `mod resolve; ... pub use resolve::{...}` pattern `model` and
`error` already use (verified against `crates/condarc/src/lib.rs`:
`model` and `error` are the crate's only two `pub use`-re-exported
modules; `parse`, `validate`, `catalog`, and `coerce` are all
`mod`-private and re-export nothing, with `parse`'s surface instead
reached through the hand-written `parse`/`parse_with_options` wrapper
functions `lib.rs` defines itself) — `resolve.rs` is not `pub mod`.

**Rationale**: `Config` already exists as the crate's parsed representation
(GEN-36, delivered) and is deliberately `#[non_exhaustive]`/`Option`-shaped
with no defaulting layer (FR-038); `resolve()` is the additive layer FR-002
requires on top of it, matching the crate's own `ParseOptions.
null_sequence_map_defaults`-style "additive opt-in, never alters `parse()`"
precedent the spec's Assumptions section calls out directly. Borrowing
`&Config` (not consuming it) lets a caller keep the original parsed
document after resolving, and costs nothing since `resolve()` never
mutates its input.

**Alternatives considered**:
- *A method on `Config` itself* (`config.resolve()`) — rejected: the spec
  frames `resolve` as a capability layered on top of, not merged into,
  `parse()`/`Config` ("never altering" — Operating Context #1), and a
  free function keeps `Config`'s own surface exactly as GEN-36 delivered
  it (no diff to `model.rs` at all).
- *A `ResolveOptions` parameter mirroring `ParseOptions`* — rejected: FR-001
  through FR-009 leave no caller-tunable behavior (no opt-in flag anywhere
  in the spec's Channel Resolution requirements); adding one speculatively
  would violate Constitution VII (no unused configurability) and IV (DRY —
  don't duplicate `ParseOptions`'s shape for a feature with nothing to
  configure).

## R2 — Channel-priority legacy-boolean handling is already done by `parse()`

**Decision**: `resolve()` does not re-implement any part of FR-003's legacy
boolean-spelling mapping (`true`→`flexible`, `false`→`disabled`). It only
applies the FR-002 default: `config.channel_priority.unwrap_or(ChannelPriority::Flexible)`.

**Rationale**: `model.rs`'s own doc comment on `ChannelPriority` already
states parse-time coercion "accepts the lowercase value, the SHOUTY-CASE
member name, or a JSON boolean / boolish string via the historical compat
shim (FR-016/017)" — GEN-36's `catalog.rs`/`coerce/boolish.rs` already
turn a `.condarc` boolean spelling into `Option<ChannelPriority>` before
`resolve()` ever sees it. Re-implementing that mapping in `resolve()` would
duplicate GEN-36's own coercion (Constitution IV) and risk drifting from
it. User Story 3's boolean-spelling acceptance scenarios (5/6) are
satisfied by GEN-36's existing crate-level coercion tests plus one
resolve-level test confirming the *already-coerced* `ChannelPriority` value
passes through `resolve()` unchanged.

**Alternatives considered**: Re-validating the boolean spelling inside
`resolve()` as defense-in-depth — rejected as needless duplication; `Config`
already only ever contains the resolved 3-variant enum or `None`, so there
is no boolean value for `resolve()` to ever observe.

## R3 — `channels`/`channel` and `allowlist_channels`/`whitelist_channels` alias collisions are already rejected by `parse()`

**Decision**: `resolve()` performs no alias-collision detection of its own
for FR-006. A document setting both spellings of either pair never reaches
`resolve()` — `parse()` already returns `Err(ValidationReport)` for it via
the existing, generic `validate::alias_collision_entries` pass (confirmed:
`catalog.rs` already declares `channels` canonical with alias `"channel"`,
and `allowlist_channels` canonical with alias `"whitelist_channels"`).

**Rationale**: Matches the spec's own FR-006 text verbatim ("This is a
`parse()`-level rejection... not a `resolve()`-level decision — a document
with such a collision never reaches `resolve()` at all"). No new code
needed in either the crate or `allez`; User Story 1 Scenario 6 / User
Story 3 Scenario 4 are already covered by GEN-36's existing
`alias_collision` test suite, plus one integration test here confirming
`allez`'s FR-012 fallback path is what actually runs when such a
`.condarc` is fed through the full `allez` pipeline (since the crate
`Err`s, `allez` treats it exactly like any other rejected document).

## R4 — Default constants (`channel_alias`, `default_channels`, `custom_channels`)

**Decision**: Three private constants in `resolve.rs`, sourced directly
from `docs/condarc_research.md`'s already-verified values (§5, line
493–497), not re-derived:

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
empty list/map for `default_channels`/`custom_channels` — is used exactly
as given, never merged with these built-ins.

**Rationale**: This is the one place `resolve()` becomes platform-sensitive
(`cfg(windows)` selecting `DEFAULT_CHANNELS`), a narrower and more
targeted use of `cfg` than GEN-24's own platform-specific code — a
compile-time constant selection, not a runtime OS-detection API call, so
it does not compromise `resolve()`'s status as a pure, hermetic function
of its `&Config` input (no filesystem/network/env access, matching
`parse()`'s own default hermeticism). This mirrors real conda's own
`on_win` check, which is exactly as narrow.

**"No merge" is a deliberate spec simplification, not a `resolve()`
invention**: real conda's `MapParameter`/`SequenceParameter` loader
performs a more nuanced multi-source search-path merge that this ticket's
single-document, `.condarc`-only scope was never asked to reproduce
(GEN-36's own Target Platform note already scopes the crate to
"single-document... no multi-source search-path merge"). FR-002's table is
the authoritative contract here, not real conda's internal merge
semantics; `resolve()` implements the table exactly as written.

**Recorded decision — these values are conda-upstream's, not Anaconda's
own current product recommendation**: Anaconda-internal signals (as of
2026-07-30) show `pkgs/r` and `pkgs/msys2` being end-of-serviced, and
Anaconda's own current recommendation (CLI-744) diverging from these
values specifically for the `main-x`/premium-tier variants — `main` itself
stays on the same `repo.anaconda.com/pkgs/main` URL `DEFAULT_CHANNELS`
already hardcodes above, so the divergence is partial, not a wholesale
replacement of these defaults. The decision made here, deliberately
and not by inheritance, is to keep the conda-upstream values from
`docs/condarc_research.md` for this ticket's scope, for two reasons: (a)
they are what real, unconfigured conda itself actually falls back to, which
is precisely this ticket's own stated goal — "conda's own documented
default channel configuration" (spec.md FR-010/FR-012), not "Anaconda's
recommended channel configuration"; and (b) revisiting Anaconda's own
product-specific channel recommendation is a separate, later product
decision, outside this ticket's scope, and not a defect in this ticket's
design. Recording it here makes it a decision this ticket owns and can be
revisited against, rather than a silently inherited assumption.

## R5 — Channel-entry resolution algorithm (FR-001) has two resolution modes, not one

**Decision**: Two private functions, not one:

- `resolve_entry(entry, ctx) -> Vec<String>` — full precedence (a) URL
  check, (b) `custom_multichannels` lookup (`"defaults"` checked here
  first, per FR-001), (c) `custom_channels` progressive-prefix match, (d)
  `channel_alias` join. Used for every top-level `channels`/`allowlist_channels`/
  `denylist_channels` entry. Returns a `Vec` because FR-001(b) expands a
  matched multichannel name to that multichannel's own members, so one
  input entry can yield zero, one, or many output entries — branches
  (a)/(c)/(d) each return a single-element vec, branch (b) returns the
  fully-expanded member list, each member independently resolved via
  `resolve_member`.
- `resolve_member(entry, channel_alias) -> String` — restricted precedence:
  (a) URL check, then straight to (d) `channel_alias` join, skipping (b)
  and (c) entirely. Used only for a `custom_multichannels` (including the
  built-in-or-user `defaults` multichannel) member's own value.

**Rationale**: User Story 1 Acceptance Scenario 5 is explicit that a
multichannel member "is resolved as an ordinary bare name via
`channel_alias` — it is not expanded further," even when that member's
text happens to collide with another multichannel or a `custom_channels`
entry name. A single, uniformly-recursive resolver would over-expand this
case and risk an unbounded cycle on the self-referencing sub-case (a
multichannel naming itself as its own member) the same scenario also
covers; the restricted `resolve_member` mode makes that cycle structurally
impossible (it never looks `custom_multichannels` up again), rather than
requiring a separate visited-set/recursion-depth guard. How
`resolve_entry`'s multi-valued return then composes with each list's own
iteration and with credential stripping — in particular why an output
entry's recorded `index` is unambiguous despite the one-to-many
expansion — is spelled out in data-model.md's own "Composition: how the
three lists, `resolve_entry`, and credential stripping fit together"
section.

**`"defaults"` substitution walkthrough** (ties R4/R5 together): resolving
the literal entry `"defaults"` first checks `custom_multichannels` for a
key literally named `"defaults"` (FR-001(b), "the name `defaults` is
always resolved here first"); if present, its member list is resolved via
`resolve_member` (restricted mode) member-by-member. If absent, the
*effective* `default_channels` list (the user's own value if `Some`, else
`DEFAULT_CHANNELS` from R4) stands in as that same member list, each of
its own entries likewise passed through `resolve_member` — so a
user-configured `default_channels` entry that happens to itself be a bare
name is still only URL-checked-or-alias-joined, never re-expanded through
`custom_channels`/`custom_multichannels`, exactly like any other
multichannel member. This is why the Concrete Channel Identifier key
entity can say every case "resolves to a URL... through `channel_alias`,
a multichannel's own members, or a `custom_channels` base-URL join" as an
exhaustive set — a `default_channels` entry is categorically a
multichannel member, not a fourth case. This same restricted-precedence
treatment (via `resolve_member`, never re-checking `custom_channels`/
`custom_multichannels`) applies identically to *any* other named
multichannel's own members, not only `defaults`'s: FR-001(b)'s
member-expansion applies uniformly to every `custom_multichannels` entry,
which is exactly why `resolve_entry` returns a `Vec<String>` rather than a
single `String` (see the Decision above and data-model.md's Composition
section) — so there is exactly one member-resolution rule, never a special
case for the literal name `"defaults"`.

**Alternatives considered**: Fully recursive resolution with a visited-set
guard against cycles — rejected: adds a data structure and a new failure
mode (what happens when the guard trips?) the spec never asks for, to
handle a case Acceptance Scenario 5 already resolves more simply by not
recursing into (b)/(c) for members at all.

## R6 — `custom_channels` progressive-prefix match

**Decision**: For entry `E`, try `E` itself, then each successive
`/`-delimited prefix of `E` (`"acme/label/dev"` → `"acme/label"` →
`"acme"`), against the effective `custom_channels` map's keys, longest
match wins (checking `E` itself first is already the longest possible
match, so no separate "longest" comparison is needed — the first hit in
that fixed iteration order is correct by construction). On a hit against
key `K` with base URL `U`, the resolved URL is
`U.trim_end_matches('/') + "/" + E` — the **original, full** entry text,
not `E` with the matched prefix stripped (per FR-001(c) and the Edge
Cases table's explicit example).

**Rationale**: Directly matches `docs/condarc_research.md`'s own worked
example: `DEFAULT_CUSTOM_CHANNELS = {"pkgs/pro": "https://repo.anaconda.com"}`
— resolving the bare entry `"pkgs/pro"` must yield
`"https://repo.anaconda.com/pkgs/pro"` (base URL, which does **not**
itself contain `pkgs/pro`, joined with the full matched name) not
`"https://repo.anaconda.com"` alone. Verified against this one already-
known-correct pairing rather than invented from scratch.

## R7 — Credential stripping is a new, independent implementation in the crate, not a reuse of `allez`'s existing `redact_channel_url`

**Decision**: A new, crate-local `strip_credentials(url: &str) -> (String, bool)`
in `resolve.rs`, structurally similar to `allez::ephemeral::channels::redact_channel_url`
(same two forms: URL userinfo, `/t/<token>/` segment) but written
independently, with its own unit tests.

**Rationale**: `crates/condarc` cannot depend on the `allez` binary crate
(wrong dependency direction — `allez` depends on `condarc`, never the
reverse; `condarc` is meant to be independently publishable per its own
`lib.rs` doc comment). FR-018 additionally forbids touching `allez`'s
existing `channels.rs` at all for this ticket — that function stays
exactly as GEN-24 delivered it, serving its own, separate, defense-in-depth
`Debug`-formatting purpose for *any* `ChannelSpec`, not only a
`.condarc`-sourced one. FR-005 requires the crate's own `resolve()` to
strip credentials *before they ever appear in `resolve()`'s output at
all* (not merely redact them from a debug string later) and to report
*that stripping occurred*, which `redact_channel_url` has no return value
for — a genuinely different contract, not the same function with a
different name.

**Alternatives considered**: Extracting a shared, dependency-free
credential-stripping helper into a *third*, tiny crate both `condarc` and
`allez` depend on — rejected as premature abstraction for two ~15-line
functions with two different signatures (`String` vs. `(String, bool)`)
and two different purposes (defense-in-depth debug redaction vs.
authoritative stripping-with-reporting); Constitution IV's DRY principle
already permits documented exceptions for intentional decoupling, and this
is one.

## R8 — Credential-stripping report shape (FR-005)

**Decision**: `ResolvedChannels.credential_stripping: Vec<CredentialStrippingEvent>`,
where `CredentialStrippingEvent { role: ChannelListRole, index: usize }`
identifies the affected entry by its list (`Channels`/`AllowlistChannels`/
`DenylistChannels`) and post-resolution position, never by the stripped
material or the resolved URL itself.

**Rationale**: FR-005 requires identifying "the affected entry by its
position/role rather than by the stripped material itself" — a plain,
`Copy`-able struct of an enum and a `usize` is the smallest type that
satisfies this literally, and keeps the crate itself opinion-free about
*how* a caller's own observability records the fact (FR-005's own text:
"the crate MUST NOT assume anything about any particular caller's own
observability conventions"). `allez`'s adapter (FR-013) is what turns this
into an actual `tracing` event.

## R9 — `allez` locates `~/.condarc` via the `dirs` crate, promoted to a direct dependency

**Decision**: Add `dirs = "6"` to `allez`'s own `[dependencies]`
(currently only present transitively at `6.0.0`, confirmed via
`Cargo.lock`) and use `dirs::home_dir()` to resolve the user's home
directory, then join `.condarc`.

**Rationale**: Constitution VII requires paths to "remain correct across
Linux, macOS, and Windows" and be "constructed with `Path`/`PathBuf` rather
than manual separator concatenation" — hand-rolling `$HOME`/`%USERPROFILE%`
env-var lookups (`allez` has no existing precedent for this; GEN-24's
`paths.rs` only resolves `$ALLEZ_EPHEMERAL_ROOT`, an `allez`-owned,
already-fully-specified path, not a user's home directory) would
re-implement exactly what `dirs::home_dir()` already does correctly on
all three platforms, and the dependency is already present in the
resolved graph today (pulled in transitively), so promoting it to direct
adds no new supply-chain surface to audit.

**Alternatives considered**: `std::env::home_dir()` — deprecated by the
standard library itself specifically for giving wrong answers in some
Windows configurations; explicitly not an option. Hand-rolling
`env::var("HOME").or_else(|_| env::var("USERPROFILE"))` — rejected per
Constitution VII's own rationale ("conda targets multiple platforms, so
path handling must not assume one") and because it would silently miss
the `%USERPROFILE%`-alternative lookup order Windows itself documents
(`dirs` already encodes this correctly via `known_folders`/`SHGetKnownFolderPath`).

## R10 — Testability: a path-injectable internal entry point, not real-`$HOME` manipulation, for the exhaustive scenario matrix

**Decision**: The public entry point is the zero-argument
`allez::channel_config::resolve_channel_config() -> ChannelConfigResolution`. It
delegates to a `pub(crate)` function,
`resolve_channel_config_from(path: Option<&Path>) -> ChannelConfigResolution`, that
takes an explicit, optional `.condarc` path. SC-002's
missing/rejected/unreadable/populated matrix is driven
through `resolve_channel_config_from` directly, via co-located
`#[cfg(test)]` unit tests using `tempfile::NamedTempFile`/a
missing-by-construction path/a file written with invalid UTF-8 bytes for
the unreadable case (see "Unreadable-file fixture, decided here" below —
deterministic on every target platform, no `cfg`-gating) — never by
mutating the real process-wide `$HOME`/`%USERPROFILE%` environment
variable. The two observability-capture test groups (SC-004's 2
fallback-path cases — rejected and unreadable — and SC-005/FR-013's 6
credential-location cases: 8 dedicated `#[test]` functions in total, per
spec.md's own SC-004/SC-005 text) live in that same
co-located `#[cfg(test)]` module, for a hard technical reason rather than
a stylistic preference: `resolve_channel_config_from` is `pub(crate)`,
and a Rust integration test under `tests/` compiles as a *separate crate*
linked against this library, so it cannot name a `pub(crate)` item at all
— and `#[cfg(test)]` does not help there either, since that attribute
only applies when the library crate itself is compiled in test mode for
its own unit tests, never when it is linked into a separate
integration-test binary. Every one of those tests needs a controlled,
non-real path to produce a rejected/unreadable/credential-bearing input
deterministically, which is exactly what only
`resolve_channel_config_from` offers; therefore they are unit tests,
despite covering cross-cutting observability behavior. A single, separate
integration test (`tests/channel_config_resolution.rs`) then holds
exactly two things: the SC-001 five-sample adaptation contract test, and
one test exercising the *public*, zero-argument entry point end-to-end,
confirming it actually resolves `dirs::home_dir()` and joins `.condarc`
correctly — the one case that must observe a real home directory. That
one test does so **out of process**, not by touching its own
environment: it re-executes **the current test binary itself** — the
already-compiled executable `std::env::current_exe()` points at, which is
guaranteed to exist because it is the very binary currently running — as
a child process, with `HOME` set on that child's own `Command`
environment to a `tempfile::tempdir()` containing a known `.condarc`,
and with CLI arguments that make the child's own libtest harness run
only this one test function, with its output uncaptured (see the
mechanism paragraph in this decision's Rationale below for the full
argument list and the sentinel-delimiting scheme). It then asserts the
child exited successfully and asserts on the sentinel-delimited region
of the child's stdout — the resolved channel list, the effective
`channel_priority`, the allow/deny list sizes, and the FR-019 fallback
line, if any — against a **hand-derived expected output** for that one
known `.condarc` fixture: the test author works out by hand what
`resolve_channel_config`'s own read → parse → resolve → adapt pipeline
should produce for that specific fixture and hard-codes that expectation
in the test. It does **not** call `resolve_channel_config_from` to
generate its own expectation dynamically — that function is `pub(crate)`
and unreachable from this separate-crate integration test, the same
visibility fact this decision already establishes for the
observability tests above. Hard-coding the expectation is also exactly
how the SC-001 five-sample adaptation contract test and the SC-003
named-scenario tests already work in this same plan.

**Rationale**: Constitution II requires tests to be "isolated,
deterministic, and fast." A child process is the strongest isolation
available for this one case: a process's environment is inherited at
process-creation time rather than shared as mutable memory, so setting
`HOME` on a `Command` cannot race with anything — not with other test
threads in this binary, not with `tracing`/logging internals, not with
any third-party crate that happens to read an environment variable
concurrently. That is a categorically stronger guarantee than
serializing tests within the parent process could offer, and it is why
this design needs no `#[serial]`, no `unsafe` block, and no in-process
environment mutation of any kind: there is no shared-process environment
race left to reason about at all.

**The process-spawning mechanism, decided here — self-re-execution, no
manifest-declared `[[bin]]` target and no new dependency**: the child
process this test spawns is *the test binary itself*, re-executed.
Nothing is added to the workspace root `Cargo.toml`:
`examples/channel_config_smoke.rs` remains exactly the plain,
Cargo-auto-discovered example already planned for this ticket from the
start (its own manual-smoke-test purpose, quickstart.md) — an existing
planned target, not a new one introduced by this test-mechanism decision
— nothing else is declared about it, and no separate binary is built for
this test to run.

The test function has two branches, selected by a dedicated marker
environment variable reserved for this one test —
`__CHANNEL_CONFIG_SMOKE_CHILD=1`, a name no production code ever reads:

- **Parent branch** (the marker is unset — the ordinary `cargo test`
  invocation): the test spawns a child process at
  `std::env::current_exe()`, the path of the already-compiled test
  binary currently executing, which is therefore guaranteed to exist and
  to need no build step of its own. On that child `Command`'s own
  environment — and nowhere else — it sets `HOME` to a
  `tempfile::tempdir()` containing a known `.condarc`, plus the marker
  variable. It additionally passes **three CLI arguments** to the
  re-executed binary, because a libtest harness selects which tests to
  run from its command line and not from the environment: (1) this smoke
  test function's own fully-qualified libtest name (e.g.
  `channel_config::smoke_test_name`), (2) `--exact`, and (3)
  `--nocapture`. The first two together restrict the child to running
  *only* this one test function — without them the child's default
  harness would re-run **every** test in this same integration binary,
  including the SC-001 five-sample contract test, in parallel, under the
  redirected `HOME`. The third is what makes the child's `println!`
  output actually reach the pipe the parent reads: libtest captures a
  test's stdout internally by default (`io::set_output_capture`) and
  releases it to the real stdout only when that test *fails*, so without
  `--nocapture` the child's printed result would be swallowed inside the
  child and never appear in the parent's `Command::output()`-captured
  stdout at all. The parent then captures the child's stdout via
  `Command::output()`, **asserts the child's exit status was successful
  first** (so a child-side failure surfaces as a child-side failure
  rather than as a confusing parse error), and only then extracts the
  content between the two unique sentinel markers described below —
  ignoring everything outside them, since libtest's own `running 1
  test`/`test result: ok.` preamble and summary lines are present in
  that same captured stdout — and compares that extracted content
  against the hand-derived expected output described above.
- **Child branch** (the marker is set — the spawned process is the same
  test binary going through the same startup, so it reaches this same
  test function again): instead of the parent's assertion logic, it calls
  `resolve_channel_config()` directly and prints a well-defined result to
  stdout **wrapped in two unique sentinel markers** (e.g.
  `===CHANNEL_CONFIG_SMOKE_BEGIN===` / `===CHANNEL_CONFIG_SMOKE_END===`),
  which is what makes that result unambiguously separable from libtest's
  own surrounding output. It then **returns normally**, as an ordinary
  passing `#[test]` function return — it does **not** call
  `std::process::exit()`. That distinction is deliberate:
  `std::process::exit` from inside one thread of a multi-threaded
  test-running process terminates the entire process immediately,
  including any other concurrently-running test threads, which can then
  report as killed/cancelled rather than completing cleanly. Returning
  normally instead lets the child's own libtest harness finish and exit
  with status 0 on its own — which is exactly the successful exit status
  the parent asserts on.

Because the spawned path is an already-built executable rather than a
fresh `cargo` invocation, no `cargo` process ever runs under the
redirected `HOME`, so Cargo's own `~/.cargo`/`~/.rustup` toolchain and
registry discovery is never disturbed by this test's environment
override. This adds **zero new Cargo dependencies** and **no
manifest-declared `[[bin]]` target** (the one thing a prior round's fix
specifically removed); `examples/channel_config_smoke.rs` remains
exactly the plain, Cargo-auto-discovered example already planned for
this ticket from the start (its own manual-smoke-test purpose,
quickstart.md) — an existing planned target, not a new one introduced by
this test-mechanism decision. There is no `assert_cmd` involvement
either (that dev-dependency stays in the manifest for the existing
`tests/cli_scaffold.rs` CLI tests, but nothing in this ticket's own test
plan needs it): `std::process::Command` plus `std::env::current_exe()`
from the standard library is the whole mechanism.

*What is still fine to leave to task-breakdown, and what is not*: the
exact literal strings remain task-breakdown's own detail — the precise
sentinel text and the precise fully-qualified libtest name of the smoke
test function both depend on the final test-module/function names, which
this plan does not fix. The **structure**, however, is now fully decided
here and is no longer an open question: the child is invoked with its
own test name plus `--exact` plus `--nocapture`; the child prints its
result wrapped in sentinel markers; the child returns normally rather
than calling `std::process::exit()`; and the parent asserts the child's
exit status before parsing anything out of its stdout. Those four points,
together with self-re-exec via `current_exe()`, marker-gated branch
selection, and `HOME` overridden only on the child `Command`, constitute
the complete mechanism and need no further resolution at plan level.

**Unreadable-file fixture, decided here — invalid UTF-8, not a
permission manipulation**: SC-002's "unreadable" file state is produced
by writing a file containing **invalid UTF-8 bytes**.
`std::fs::read_to_string` requires valid UTF-8 and therefore fails
deterministically with `io::ErrorKind::InvalidData` on every one of this
ticket's four target platforms, needing no `chmod`, no Windows ACL
manipulation, and no `cfg`-gating anywhere. That exercises exactly the
`ReadOutcome::Unreadable(io::Error)` branch a real permission-denied file
would (`ReadOutcome`'s own doc comment in data-model.md defines that
variant as "any other `io::Error`, most commonly `PermissionDenied`", and
FR-012's own text says "or other I/O error"), so the test proves what it
needs to prove: that the `Unreadable` branch is reached, recorded
(FR-012), and surfaced as `Some(FallbackReason::Unreadable)` (FR-019) for
*any* non-`NotFound` I/O error. A real permission denial — `chmod 0` on
Unix, an ACL denial on Windows — remains the illustrative production case
this branch exists for, and is worth a comment saying so, but it is
deliberately *not* the mechanism the automated test uses, since
reproducing it would fragment one test case across two platform-specific
APIs for no additional coverage.

**Why invalid UTF-8 rather than a literal permission denial, given
spec.md's own "OS permission error" wording — recorded as a deliberate
decision, not an unaddressed gap**: spec.md's User Story 2 (Independent
Test, Acceptance Scenario 3) names "an OS permission error" as its
exemplar unreadable-file case, so it is worth stating outright why this
ticket's automated test does not reproduce that literal mechanism.
Beyond the cross-platform/`cfg`-gating concern already documented above,
literal permission denial has a second, independent reliability problem:
many CI environments run their test suites as `root` (the common default
in container-based CI), and under `root` Unix permission bits are
bypassed entirely — `root` can read a `chmod 0o000` file regardless of
its mode. A real permission-denial fixture would therefore be flaky or
silently no-op into the *success* path in exactly the environments most
likely to run it, never reaching the `Unreadable` branch it exists to
exercise. Invalid-UTF-8 content has no such privilege-bypass failure
mode on any platform or under any user context: `read_to_string` rejects
it identically as `root` and as an unprivileged user. spec.md's "OS
permission error" language names ONE real-world instance of the broader
class FR-012 and `ReadOutcome::Unreadable` actually cover — "any other
`io::Error`", per `ReadOutcome`'s own doc comment in data-model.md — and
this ticket's automated test verifies that broader class through a
fixture that is deterministic in every environment this ticket's tests
can run in, rather than through the literal permission-denial mechanism,
which would not reliably exercise the code path it is meant to test in
root-run CI.

Every other `allez`-level scenario test stays on the
path-injectable internal function, where an injected path makes a
subprocess unnecessary (SC-002 × 4, plus the two SC-004/FR-013
observability-capture test groups — 8 dedicated `#[test]` functions), so
no test in this ticket's scope pays any process-spawn or serialization
cost beyond the single smoke test that genuinely needs the real
`dirs::home_dir()` path.
SC-003's own 20 scenarios are not part of this `allez`-level accounting
at all — they are crate-level tests of `condarc::resolve()` itself, in
`crates/condarc/tests/resolve_scenarios.rs`, and never touch
`resolve_channel_config_from`.

**Alternatives considered**: Accepting a `path: Option<&Path>` parameter
directly on the *public* API — rejected: FR-010/FR-014 describe this
capability's public contract as parameterless ("locate and read
`~/.condarc`"), and GEN-25 (the production caller) has no reason to ever
supply a different path; exposing one would be an unused, undocumented
configurability surface (Constitution VII's spirit, even though that
principle is phrased around hardcoded values rather than unnecessary
parameters) and would need its own doc-comment caveat explaining when a
caller should *not* use it.

**Cross-platform caveat for the one real-entry-point test**: on Windows,
`dirs::home_dir()` resolves via `SHGetKnownFolderPath`, which does not
consult the `USERPROFILE` environment variable at all — so a
`HOME`/`USERPROFILE` value handed to the child process would not redirect
the child's own `dirs::home_dir()` call there either. This is purely a
Windows-API-behavior limitation, not a thread-safety or isolation one:
the subprocess design above already removes every isolation concern, and
crossing a process boundary does nothing to change which API `dirs`
itself consults on Windows. That is the sole reason this one test is
`#[cfg(unix)]`-gated. Verifying `dirs::home_dir()`'s own correctness on
Windows is `dirs`'s own already-tested responsibility, not re-verified
here, matching this plan's existing pattern of trusting already-tested
third-party/crate behavior (e.g. GEN-36's crate, research.md R2/R3).
Every other test in this ticket's scope runs on all four target platforms
unaffected — the crate-level SC-003 and SC-005 suites in
`crates/condarc/tests/resolve_scenarios.rs`, and every `allez`-level
co-located unit test driving the path-injectable
`resolve_channel_config_from` (SC-002's four file states plus the two
SC-004/FR-013 observability-capture test groups — 8 dedicated `#[test]`
functions) — only this one smoke test is Unix-only.

## R11 — Structured observability: two new, narrowly-scoped event shapes, no reuse of `EphemeralLifecycleEvent`

**Decision**: Two new `tracing`-emitting event types in
`src/channel_config/events.rs`:

- `ChannelConfigFallbackEvent` (FR-012): `schema_version`, `reason`
  (`"rejected"` | `"unreadable"`), `detail` (the crate's own per-problem
  `ValidationReport::to_string()` for `"rejected"`; the `io::Error`'s own
  `Display` text for `"unreadable"`).
- `CredentialStripLogRecord` (FR-013): `schema_version`, `role`
  (`"channels"` | `"allowlist_channels"` | `"denylist_channels"`),
  `index`.

`ChannelConfigFallbackEvent` emitted via `tracing::warn!` (a fallback is
a recovered problem, warranting attention); `CredentialStripLogRecord`
emitted via `tracing::info!` (a routine security-hygiene fact, not a
problem) — both through the existing `src/observability.rs` subscriber
(no second logging pipeline), with their own `schema_version` sibling
constant (not the
identical value as `output::SCHEMA_VERSION` or GEN-24's own
`EPHEMERAL_EVENT_SCHEMA_VERSION`), matching GEN-24's own precedent for
its `EphemeralLifecycleEvent`.

**Rationale**: `EphemeralLifecycleEvent`'s fixed shape
(`operation`/`packages`/`duration_ms`/`outcome`/`failure_category`) is
purpose-built for a create/install/teardown *operation* with a
success/failure outcome and a duration — this ticket's two events are
neither: a fallback record has no "operation" in that sense (resolution
itself always "succeeds," per FR-012's own text), and a credential-strip
record is a per-entry fact, not a whole-call outcome. Forcing either into
`EphemeralLifecycleEvent`'s shape would be exactly the "category-string
lie" `ActivationError`'s own doc comment (GEN-24 data-model.md) already
rejected for a structurally similar reason.

**Alternatives considered**: Extending `EphemeralLifecycleEvent` with new,
optional fields — rejected: that type is GEN-24's own already-delivered,
already-tested contract (FR-013/SC-008 in that ticket); extending it here
risks exactly the kind of unrelated-feature coupling Constitution I's
"single, clear responsibility" principle warns against, for no shared
benefit (nothing about channel-config resolution needs an
`environment_id`).

## R12 — Surfacing FR-012's fallback condition in the resolution result itself, not only via observability (FR-019)

**Decision**: `resolve_channel_config`/`resolve_channel_config_from` return
a new, small wrapper type, `ChannelConfigResolution { config: ChannelConfig,
fallback: Option<FallbackReason> }`, instead of a bare `ChannelConfig`.
`FallbackReason` (already introduced above for `ChannelConfigFallbackEvent`,
R11) is promoted to a `pub`, `#[non_exhaustive]` type shared between the
observability event and this new field. `fallback` is `None` for a
fully-successful resolution and for the silent missing-file case
(FR-010); `Some(FallbackReason::Rejected)` or
`Some(FallbackReason::Unreadable)` for FR-012's two recorded fallback
cases. `config` itself remains exactly GEN-24's own, unmodified four-part
`ChannelConfig` (FR-014/FR-018) — the wrapper adds a sibling field, never
a fifth field inside `ChannelConfig`.

**Rationale**: FR-019 (added in the same spec revision that closes out
2026-07-29's spec-review feedback on this ticket's own PR — see spec.md
Assumptions) requires this condition be inspectable at the result level,
not only via a tracing event a caller might never read during an
unattended run. A caller with more context than this ticket's own
resolution step (GEN-25) can then decide whether to warn, proceed, or
abort on a rejected/unreadable file, while `resolve_channel_config`
itself stays total and never fails, preserving FR-010–FR-012's own
unattended-agent guarantee. Reusing `FallbackReason` (rather than
inventing a second, parallel reason enum) avoids the DRY violation a
fresh type would introduce for the exact same two-case distinction the
observability event already models.

**Alternatives considered**:
- *Making `resolve_channel_config` fallible (`Result<ChannelConfig, ChannelConfigError>`)*
  — rejected: this was literally the alternative spec-review feedback
  proposed, and spec.md's own Assumptions explain why it was not
  adopted — an unattended agent invocation has no interactive way to
  react to a hard failure mid-run, so `resolve_channel_config` itself
  must stay total; the caller-level signal this decision adds is what
  actually answers that feedback's underlying concern (visibility)
  without reopening FR-010's own no-fail guarantee.
- *Leaving `FallbackReason` private and inventing a separate public copy
  for the return type* — rejected as needless duplication of a type that
  already exists and already models exactly this two-case distinction
  (Constitution IV).

## Test strategy

- **Crate-level** (`crates/condarc/`): unit tests co-located in
  `resolve.rs` for each of R5/R6/R7's pure functions in isolation
  (`resolve_entry`, `resolve_member`, the progressive-prefix matcher,
  `strip_credentials`); a new integration test file,
  `crates/condarc/tests/resolve_scenarios.rs`, mapping every one of
  SC-003's 20 named scenarios and SC-005's 6 credential-bearing-location
  scenarios to one `#[test]` each, hand-authored `.condarc` YAML strings
  (no JSON conformance-corpus fixtures — GEN-36's own conformance harness
  has no oracle for channel *resolution*, only parse-shape coercion, so
  there is nothing for a `resolve()`-focused fixture to conform against).
- **`allez`-level**: unit tests co-located in `src/channel_config/*.rs`
  for the path-injectable `resolve_channel_config_from` (R10) covering
  SC-002's four file-state cases, *and* the two observability-capture test
  groups (SC-004's 2 fallback-path cases and SC-005/FR-013's 6
  credential-location cases — 8 dedicated `#[test]` functions in total,
  per spec.md's own SC-004/SC-005 text; each using a `tracing` test
  subscriber, the same technique GEN-24's own
  `quickstart.md` describes for `EphemeralLifecycleEvent`) confirming
  SC-004's fallback record and SC-005/FR-013's credential-stripping
  record are both actually emitted, and that neither ever carries raw
  credential material. Those observability tests are co-located unit
  tests rather than integration tests because they need
  `resolve_channel_config_from` to inject a controlled path (the only way
  to produce a rejected/unreadable/credential-bearing input
  deterministically), and that function is `pub(crate)` — unreachable
  from a `tests/` file, which Rust compiles as a separate crate (R10).
  Separately, a new integration test,
  `tests/channel_config_resolution.rs`, holds exactly two things: the
  SC-001 five-sample contract test (constructing
  `allez::ephemeral::ChannelConfig` directly) and the one
  subprocess-based, real-home-directory public-entry-point smoke test,
  which re-executes the test binary itself via
  `std::env::current_exe()`, with `HOME` and a marker environment
  variable set on the child's own `Command` environment, plus that one
  test function's own libtest name, `--exact`, and `--nocapture` as CLI
  arguments so the child runs only that test and lets its `println!`
  output through, and asserts the child exited successfully before
  comparing the sentinel-delimited region of the child's stdout against
  the hand-derived expected output; the child branch returns normally
  rather than calling `std::process::exit()` (R10). Each of
  SC-002's four file-state
  cases also
  asserts `ChannelConfigResolution.fallback`'s exact value
  (`None`/`Some(Rejected)`/`Some(Unreadable)`/`None`) per FR-019 (R12),
  not only the returned `config`.
- `cargo test --all` remains the single entry point; no new opt-in feature
  flag is needed (unlike `conformance-tests`/`network-tests`) since every
  test this ticket adds is hermetic and network-free by construction —
  `resolve()` never touches the network, and `allez`'s own file-handling
  layer only ever reads a local path.
