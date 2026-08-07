# Phase 0 Research: `allez oneshot` Command

This consolidates the technical unknowns from `spec.md` into decisions.
Every crate/API claim below was confirmed via a librarian research pass
against Tokio 1.53, `rustix` 1.1.4, and `windows-sys` 0.61 — the exact
versions already pinned in this repository's `Cargo.lock` — as of
2026-08-03; see the citations inline.

## Decision: Async runtime entry point

**Decision**: Promote `src/main.rs`'s `fn main()` to
`#[tokio::main(flavor = "multi_thread")] async fn main()`, and make
`dispatch` an `async fn`.

**Rationale**: `create_ephemeral_environment` (GEN-24) is already an
`async fn` that internally uses `tokio::task::spawn_blocking`, which
requires an active Tokio runtime `Handle` to exist when it's called — but
no code path in this repository today ever starts one; every existing CLI
handler (`create`, `list`, `run`, `sandbox`, `remove`, and `oneshot`'s own
stub) is plain synchronous code. `#[tokio::main]` is the standard,
lowest-friction way to give a `fn main()`-shaped binary a runtime without
manually constructing and holding a `tokio::runtime::Runtime` value only
to immediately call `.block_on()` on it once. `flavor = "multi_thread"`
matches the `rt-multi-thread` feature already enabled in `Cargo.toml`
(GEN-24 needs it for `rattler`'s own parallelism), so this is not a new
runtime-flavor decision, only wiring an already-required flavor up to an
entry point that previously had none.

**Alternatives considered**:
- Construct a `tokio::runtime::Runtime` manually inside only the `Oneshot`
  dispatch arm, `.block_on()`-ing just that arm's async work, leaving
  `fn main()` itself synchronous. Rejected: this is strictly more code
  than `#[tokio::main]` for an identical outcome, and creates a second,
  easily-overlooked place a future subcommand needing async (e.g. `run`/
  `sandbox`, once they gain real process-spawning logic too) would have to
  either duplicate or refactor away — `#[tokio::main]` pays that cost once,
  now, for every future subcommand's benefit.
- Leave `main` synchronous and give `create_ephemeral_environment` a
  synchronous wrapper. Rejected: that wrapper would just be `Runtime::new
  ().block_on(...)` moved one level down — the same cost, hidden inside
  `ephemeral::mod.rs` instead of `main.rs`, and GEN-24's own `mod.rs` is
  explicitly not this ticket's file to modify (Constraints).

## Decision: Pass-through package-spec parse failures map to `unresolvable_package`

**Decision**: `RequestedPackages::from_cli(args.packages)` (GEN-24) can
return `Err(InvalidPackageSpec { input, reason })` for a syntactically
invalid match-spec string (e.g. `"[[[not-a-spec"`). `spec.md`'s FR-010
closed category set does not separately name this case. This plan maps it
to `EphemeralEnvError::UnresolvablePackage { package: input }` — rendered,
exit code, and category exactly as any other unresolvable-package failure
— constructed directly in `oneshot.rs` *before* `create_ephemeral_
environment` is ever called (so FR-010's "before ever attempting to start
the pass-through command" and FR-001's package-before-command ordering
both still hold trivially).

**Rationale**: A syntactically invalid spec can never resolve against any
channel, so "could not resolve package `X`" is a true, not merely
convenient, description of the failure — no new category is needed, and
FR-010's closed four-category set stays exactly as specified.

**Alternatives considered**: A new `AllezError::InvalidPackageSpec` usage
error (exit `2`) was considered and rejected — FR-011 scopes usage errors
(exit `2`) specifically to "no `--` separator, or `--` with nothing after
it"; a bad package spec is a resolution-time problem, not a
missing-argument problem, and the caller's remedy (fix the package name)
is identical to any other unresolvable-package case.

## Decision: Zero usable channels maps to the existing `no_channels_configured` category

**Decision**: `channel_config::resolve_channel_config()`'s
`ChannelConfigResolution::NoChannels` variant (GEN-23) maps directly to
`EphemeralEnvError::NoChannelsConfigured` — constructed in `oneshot.rs`
before `create_ephemeral_environment` is called (that function requires a
non-trivial `condarc::ResolvedChannels` as an argument; there is nothing
useful to pass it when GEN-23 has already determined zero channels remain).

**Rationale**: FR-010 already names `no_channels_configured` as one of its
four closed categories, and `EphemeralEnvError::NoChannelsConfigured`'s own
existing `Display` text ("no channels remain to solve against") is
accurate for this case too — no new type or category needed; this is a
direct reuse, not an adaptation.

## Decision: Default package list — `["python"]` stopgap pending GEN-30

**Decision**: `src/ephemeral/defaults.rs`'s `DEFAULT_PACKAGES` constant
changes from GEN-24's test-fixture placeholder
(`["fixture-default-alpha", "fixture-default-beta"]`) to `["python"]`.
`create_ephemeral_environment`'s third parameter, `default_override:
Option<Vec<PackageSpec>>`, is passed as `None` from `oneshot.rs` — no
mechanism anywhere in this codebase surfaces a caller-configured override
today (`condarc::Config`'s `create_default_packages` field is parsed by
GEN-23 but never consumed past that point); building that mechanism is
GEN-30's own scope. Until GEN-30 lands, `allez oneshot` with zero packages
resolves and installs `python` against whatever channels the caller's
`.condarc` (or its own built-in fallback) configures — an explicit,
documented, temporary decision, not a claim that `python` is the final
product-decided default.

**Rationale**: `DEFAULT_PACKAGES`'s prior value only ever resolved against
GEN-24's own checked-in local fixture channel — against any real channel
(`conda-forge`, `defaults`), it fails with `unresolvable_package`,
directly contradicting spec.md AS-1.2/FR-001's "the environment still
comes into existence... rather than being empty or failing." `python` is
a real, near-universally-available package on real channels, closing that
production gap immediately without waiting on GEN-30's own
override-authoring mechanism.

**Alternatives considered**: Waiting for GEN-30 before touching this
constant at all was considered and rejected — GEN-30 is unscheduled, and
shipping `allez oneshot` with a default that provably cannot resolve
outside a test fixture is a worse outcome than a documented stopgap.
Adding a fixture-local package literally named `python` so the
zero-package path stays testable purely offline was considered and
rejected — see this file's own Test strategy decision below for why.

## Decision: Process spawning, stdio, and environment merge

**Decision**: Build a `tokio::process::Command` for
`args.pass_through.program()`/`.args()`. Leave stdin/stdout/stderr at their
default (`Stdio::inherit()`-equivalent — `tokio::process::Command`, like
`std::process::Command`, inherits the parent's file descriptors by
default; no explicit `.stdin(...)`/`.stdout(...)`/`.stderr(...)` call is
needed to satisfy FR-004/FR-005's streaming/passthrough requirements).
Apply `ReadyEnvironment::activation_environment()`'s overlay via one
`.env(key, value)` call per pair, on top of `tokio::process::Command`'s own
default (inherit the parent's full environment) — never `.env_clear()`.

**Rationale**: This is the literal, no-extra-code reading of FR-003's
"merge, not replace" requirement: `Command`'s documented default behavior
*is* "inherit everything," and `.env()` calls only ever add or override
individual keys on top of that default, so every variable `allez`'s own
process had (including anything the surrounding sandbox supplied) reaches
the child unless activation itself happens to define the same key — which
is activation's own explicit, intended override, not a silent drop.

**Alternatives considered**: Explicitly enumerating and re-setting every
inherited variable via `std::env::vars()` before adding the overlay was
considered and rejected as strictly more code for the identical outcome
`Command`'s own inherit-by-default already provides.

## Decision: Signal listening and forwarding (FR-014) — no new dependency

**Decision**:

- **Listening**: enable Tokio's `signal` feature. Register every listener
  below *before* spawning the pass-through program (not after) — a signal
  arriving in the gap between spawn and listener registration would
  otherwise have no forwarder alive to relay it, leaving the child running
  detached, exactly the outcome FR-014 exists to prevent. On Unix, listen
  with `tokio::signal::unix::signal(SignalKind::interrupt())`/
  `::terminate()`/`::hangup()`/`::quit()` for `SIGINT`/`SIGTERM`/`SIGHUP`/
  `SIGQUIT` — the fixed set of signals whose OS-level semantic is "please
  terminate" and that `tokio::signal::unix` can catch; signals with a
  different semantic (`SIGUSR1`/`SIGUSR2`, `SIGWINCH`, `SIGCHLD`,
  `SIGPIPE`, and so on) are excluded by definition, not by omission — they
  are not "a termination signal" in FR-014's own sense regardless of
  whether they're catchable. On Windows, listen with
  `tokio::signal::windows::ctrl_c()`/`::ctrl_break()`.
- **Forwarding on Unix**: send the *same* signal to the child's own PID
  only via `rustix::process::kill_process(pid, signal)` — `rustix` is
  already a direct dependency (`process` feature, already enabled for
  `mkdirat`/`geteuid` in `src/ephemeral/`), so this adds a new call site,
  not a new dependency. The child is spawned with no process-group change
  of its own (no `.process_group(0)`) — see Alternatives considered below
  for why forwarding is scoped to the direct child only, not its process
  group.
- **Forwarding on Windows**: best-effort `GenerateConsoleCtrlEvent(
  CTRL_BREAK_EVENT, child_pid)` (via `windows-sys`, new
  `Win32_System_Console` feature) against a child spawned with
  `#[cfg(windows)] .creation_flags(CREATE_NEW_PROCESS_GROUP)` (`windows-sys`'s
  existing `Win32_System_Threading` feature already covers this constant —
  required here purely because `GenerateConsoleCtrlEvent` itself only
  accepts a process-group ID as its target, an unrelated Win32 API
  constraint from the Unix process-group question above); if that call's
  `BOOL` return is falsy, fall back to `child.kill()` (Tokio's own
  cross-platform hard-kill, ultimately `TerminateProcess` on Windows).
- After forwarding (either platform), the `tokio::select!` loop
  **continues waiting** for the child's own `wait()` to resolve — `allez`
  itself never exits early on receiving the signal; it only relays it and
  keeps blocking on the child's real outcome, per FR-014's "forwards it
  ... rather than exiting and leaving that command running detached."
- **Listener-registration failure**: `tokio::signal::unix::signal(...)`/
  `tokio::signal::windows::ctrl_c()`/`::ctrl_break()` each return an
  `io::Result`, so registering a listener can itself fail (a rare,
  OS-resource-level failure, not a program- or environment-related one).
  This happens after the environment is already ready but strictly
  before `.spawn()` is called, so it is a pre-start failure distinct
  from every other `PassThroughFailure` variant — see the new
  `PassThroughFailure::SignalSetupFailed` variant (`data-model.md`),
  category `signal_setup_failed`, exit code `1` (the same family as
  `ActivationFailed`, for the same reason: nothing about the requested
  program or the environment was wrong).

**Rationale**: Every one of the four target platforms is covered without
adding a new dependency — `nix` and `signal-hook` already appear only
*transitively* in `Cargo.lock` (pulled in by unrelated dependencies), and
promoting either to a direct dependency would duplicate capability this
repository's own existing direct dependencies (`tokio`, `rustix`,
`windows-sys`) already provide, working against Constitution VII/the
project's stated "no hardcoded values"/minimal-dependency ethos GEN-24's
own `Cargo.toml` comments already model.

**Alternatives considered (Unix forwarding scope)**: forwarding to the
child's *process group* (`.process_group(0)` at spawn time +
`rustix::process::kill_process_group`), so a forwarded signal also
reaches any grandchildren the pass-through program itself spawns, was
this decision's first draft. Rejected: putting the child in its own
process group makes it a *background* process group under any attached
controlling terminal, which can raise `SIGTTIN`/`SIGTTOU` on ordinary
terminal I/O — a real conflict with FR-005's "stdin forwarded unchanged"
for any TTY-attached invocation. FR-014's own text only requires
forwarding "to the pass-through command," not to its descendants;
targeting the direct child PID satisfies that literal requirement with no
process-group side effect, at the cost of not reaching grandchildren —
an explicit, narrower scope than the first draft, not an oversight.

**Windows Ctrl-C limitation, accepted as-is**: `GenerateConsoleCtrlEvent`
cannot target `CTRL_C_EVENT` at a specific process group — only
`CTRL_BREAK_EVENT` supports group-scoped delivery on Windows; a
system-wide `CTRL_C_EVENT` would also strike `allez`'s own process (see
Microsoft's own documentation:
<https://learn.microsoft.com/en-us/windows/console/generateconsolectrlevent>).
This plan therefore only attempts graceful forwarding for `Ctrl-Break` on
Windows; an intercepted `Ctrl-C` always results in the child being
`.kill()`ed, since a group-scoped `CTRL_C_EVENT` is unsafe to attempt at
all. The `Ctrl-Break` attempt itself is bounded: after
`GenerateConsoleCtrlEvent` returns a truthy `BOOL`, this plan waits `100ms`
(a short, fixed, named constant — see plan.md's Complexity Tracking) and
checks once whether the child is still running, falling back to
`child.kill()` if so rather than waiting indefinitely or trusting the
`BOOL` return alone. This mirrors real-world precedent: `watchexec` and
`mise` both document Windows graceful-forwarding as unsupported/
force-kill-only, and `zellij` attempts console-control delivery with the
same kill-fallback this plan adopts. This is the concrete instance of
spec.md's Assumptions clause on platforms with no equivalent concept
still reporting a distinct failure category without the same numeric
convention everywhere — Windows's forwarding is best-effort for one
signal only, within that Assumption's already-granted latitude.

## Decision: Exit-code classification from `ExitStatus`

**Decision**:

```rust
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

match status.code() {
    Some(code) => code,                      // normal exit (FR-006)
    None => {
        #[cfg(unix)]
        {
            match status.signal() {
                Some(signal) => 128 + signal, // FR-007
                // `code()` returning `None` on Unix is documented to mean
                // the process was signal-terminated, so `signal()`
                // returning `None` here would mean that documented
                // contract was violated by the platform. Handled
                // explicitly with a fixed fallback rather than via
                // `.unwrap()`/`.expect()`/`.unwrap_or()` (Constitution V).
                None => 128,
            }
        }
        #[cfg(windows)]
        {
            unreachable!("Windows ExitStatus::code() is always Some")
        }
    }
}
```

**Rationale**: confirmed directly against `std::os::unix::process::
ExitStatusExt`: on Unix, `code()` returns `None` exactly when the process
was terminated by a signal, and `signal()` (also from `ExitStatusExt`)
then returns `Some(signal_number)` — the `128 + n` value is the same
convention POSIX shells already use, which is what FR-007 itself cites.
The inner `None => 128` arm is dead code under that documented contract,
not a value this plan expects to ever produce — it exists only so the
match is exhaustive without an `.unwrap()`/`.expect()`/`.unwrap_or()` call,
per Constitution V's "no `.unwrap()`/`.expect()` outside tests." An
earlier draft of this decision used `status.signal().unwrap_or(0)`
directly, which is exactly the kind of call that rule forbids — the
`match` above replaces it with an explicit, named fallback instead.
`std::os::windows::process::ExitStatusExt` exposes no `signal()`/
equivalent at all — on Windows, `code()` is documented to always return
`Some(_)`, even for a force-terminated process, so the outer `None` branch
is genuinely unreachable there; the `unreachable!()` is guarded by
`#[cfg(windows)]` specifically so it never compiles into, or executes on,
a Unix build.

## Decision: Spawn-failure classification (FR-008)

**Decision**: `tokio::process::Command::spawn()`'s `Err(io::Error)` maps
by `.kind()`:

| `io::ErrorKind` | Category | Exit code |
|---|---|---|
| `NotFound` | `pass_through_not_found` | `127` |
| `PermissionDenied` | `pass_through_not_executable` | `126` |
| any other kind | `pass_through_not_executable` | `126` |

**Rationale**: FR-008 names exactly these two categories/codes for "the
program cannot be found" vs. "found but cannot be executed." `io::Error`'s
own `ErrorKind` enum is the only portable signal available at this point
(the OS hasn't even created a process yet), and `NotFound` is the one kind
every platform reliably reports for "no such program on `PATH`" — every
other spawn-time `io::Error` kind (permission errors of a more specific
flavor, `ENOEXEC`-style "not a valid executable," and so on) is closer in
spirit to "the program exists but this attempt to execute it failed" than
to "it doesn't exist," so this plan folds all of them into
`pass_through_not_executable` rather than inventing a third category
FR-008 doesn't name.

## Decision: `ActivationError` mapping — new, additive `activation_failed` category

**Decision**: introduce one new error type, `PassThroughFailure`
(`#[non_exhaustive]`, implements `CategorizedError`, lives in
`src/cli/pass_through.rs`), with (at minimum) variants `NotFound`,
`NotExecutable`, `TerminatedBySignal { signal: i32 }`, and a unit variant
`ActivationFailed` (no wrapped message — see the redaction note below).
`ActivationFailed`'s category is `activation_failed`, rendered at exit
code `1` (the same code family as an environment-creation failure, since
— from the caller's point of view — nothing about their requested program
was wrong; retrying or investigating the environment is the correct next
step either way). This is treated as this plan's own implementation-level
judgment call, not a spec.md amendment: `activation_environment()`
failing happens *after* environment creation already succeeded, a state
FR-010's four-category closed list was never scoped to describe in the
first place (that list is explicitly about "environment-creation
failure"), so this fills a gap the spec doesn't speak to rather than
extending an enumeration it deliberately closed. The `category` field
remains the caller's disambiguator either way — it is never confusable
with a genuine FR-010 category.

**Redaction (FR-015)**: `ActivationError.message` (GEN-24,
`src/ephemeral/error.rs`) is `rattler_shell`'s own raw activation-error
text, sourced from `Activator::from_path`/`run_activation` — this plan
does not control its contents, and it can plausibly include the
environment's own filesystem path (e.g. when activation fails because
`conda-meta/state` is malformed at a specific prefix). Forwarding it
verbatim into `PassThroughFailure::ActivationFailed`'s caller-facing
message, or into the `OneshotOutcomeEvent` tracing record, would risk
violating FR-015's "MUST NOT disclose the created environment's location."
`ActivationFailed` therefore carries no field at all: both the
caller-facing error message and the tracing event's own message use one
fixed, hardcoded string (e.g. "failed to prepare the created environment
for use") — the underlying `ActivationError.message` detail is
deliberately dropped, not merely truncated or best-effort-redacted, since
there is no reliable, generic way to strip an arbitrary path substring
out of an upstream library's own free-text error without risking a
false negative.

## Decision: Test strategy

**Decision**: new end-to-end tests in `tests/oneshot_exec.rs`, spawning the
real compiled `allez` binary through `assert_cmd` (matching `tests/
cli_scaffold.rs`'s existing convention). Test isolation is via a new
environment variable, `ALLEZ_CONDARC_PATH`, checked in
`channel_config::default_condarc_path()` before falling back to
`dirs::home_dir()`'s own `~/.condarc` resolution: if set,
`channel_config::resolve_channel_config()` reads the `.condarc` at that
exact path instead. **The check exists only behind a new, non-default
Cargo feature, `test-config-override`** (`Cargo.toml`: `test-config-override
= []`, not part of any `default = [...]` list — there is none in this
`Cargo.toml` today). `tests/oneshot_exec.rs` declares `required-features =
["test-config-override"]` in its own `[[test]]` `Cargo.toml` entry. Unlike
`conformance-tests` (which gates a *slow, external-oracle-dependent* test
tier behind a *dedicated* Makefile target and CI job, deliberately kept
out of the default `cargo test --all`/CI `test` job), this ticket's own
tests are fully local, fixture-based, and fast — the *only* reason they
need a feature flag at all is the `ALLEZ_CONDARC_PATH` security concern
below, not test speed or external dependencies. So this ticket adds
`--features test-config-override` directly to the *existing* invocations
that already run the full suite by default, rather than creating a new,
separate opt-in tier: `Makefile`'s `test` target becomes `cargo test --all
--features test-config-override`, and `.github/workflows/ci.yml`'s `test`
job (all four platform legs) and `coverage` job both gain the same flag on
their existing `cargo test`/`cargo llvm-cov` invocations. A bare `cargo
test --all` with no explicit feature flags (e.g. run directly, bypassing
`make test`) still compiles a binary that skips `tests/oneshot_exec.rs`
specifically — same Cargo-level skip mechanism `conformance-tests` uses,
applied to a default-on invocation instead of a separate one. Each test
sets `ALLEZ_CONDARC_PATH` (plus `ALLEZ_EPHEMERAL_ROOT`) via that one
invocation's own `Command::env(...)` call rather than a process-wide
`std::env::set_var`.

**Rationale (feature-gating, not just documentation)**: an always-compiled
`ALLEZ_CONDARC_PATH`, documented as "test-only" but not actually blocked
from a real release build, would be a real security problem: `allez`'s
own stated purpose (spec.md's Operating Context; GEN-19) is to stand
between an AI agent and unrestricted channel/package access, and an
always-checked env var letting the agent redirect channel resolution
before any policy applies is a stronger version of the
`CONDA_ALLOWLIST_CHANNELS`-style bypass the team's own internal research
already identified. Feature-gating removes this structurally: a binary
compiled without `--features test-config-override` has no code path that
reads `ALLEZ_CONDARC_PATH` at all — it doesn't just go undocumented, it
doesn't exist. This repository's CI has no release/distribution-build job
today — every job runs tests, lints, or checks docs — so there is no
existing pipeline step this could leak into; if one is added later, it
MUST NOT pass `--features test-config-override`, the same discipline
`cargo install`/`cargo build --release` already provides by default
(Cargo never enables a non-default feature implicitly). This keeps
plan.md's Constitution VII claim ("this ticket adds no new environment
variable of its own") true for any real distribution build; only a
`cargo test`-time build with the feature explicitly requested gains the
check. This ticket keeps its own name (`ALLEZ_CONDARC_PATH`) rather than
conda's `CONDARC` — conda's `CONDARC` is additive (merges with
`$HOME/.condarc`), and `condarc` (GEN-36) is a normative, conda-compatible
parser; reusing that name for a differently-scoped, exclusive override
would confuse more than an unrelated name.

**Rationale (the seam itself)**: `dirs::home_dir()` on Windows resolves
via a real OS API call (`SHGetKnownFolderPath(FOLDERID_Profile)`), not by
reading `USERPROFILE` or any other environment variable, so setting
`USERPROFILE` on one `assert_cmd::Command` invocation would silently have
no effect there. `ALLEZ_CONDARC_PATH` is the override seam this needs for
deterministic, portable test isolation on every target platform.
`assert_cmd::Command::env()` also scopes the variable to exactly the one
child process it spawns; since every test here spawns its own fresh
`allez` subprocess (unlike GEN-24's own tests, which call `ephemeral::`
functions in-process and share one `cargo test` binary's process-wide
environment), no cross-test interference is possible and no
`#[serial]`/mutex discipline is needed — simpler and more isolated than
GEN-24's own story, because these tests operate one level higher, through
the CLI binary.

**Default-package testing, given `DEFAULT_PACKAGES = ["python"]`**: the
checked-in local fixture channel does not, and will not, contain a
package named `python` — inventing one purely to make the zero-package
path resolve offline would be the same "author production defaults to
match a test fixture" anti-pattern a GEN-24 reviewer already objected to,
now with a more misleading name. Coverage is therefore split: (1) an
offline test asserts that `oneshot -- <cmd>` with zero packages routes
through the *same* resolution code path an explicit, unresolvable package
would — failing with category `unresolvable_package` (not a usage error)
against the fixture channel, proving the "zero packages defers to
`UseDefaultOrOverride`" mechanism is wired correctly without needing
`python` to actually resolve; (2) full end-to-end proof that the
configured default value resolves and installs against a real channel is
an explicitly accepted gap in this ticket's own test suite, deferred
until GEN-30 provides a default (or override) this suite can point at
deterministically — not silently unverified, but not solved here either.
**Every other test in `tests/oneshot_exec.rs` that needs the pass-through
program to actually start** (i.e. every scenario except the one exercising
this specific zero-packages-fails-offline path) MUST supply an explicit,
fixture-resolvable package (e.g. `fixture-default-alpha`) before `--` —
bare `oneshot -- <cmd>` with zero packages would, under this same
default-package change, fail at package resolution before the pass-through
program is ever reached, for any test that isn't specifically testing that
failure.

**Signal-forwarding tests, platform scoping**: Unix signal-forwarding
tests (spawn a long-running pass-through command such as a small script
that traps `SIGTERM` and exits with a distinct code, send `allez` itself
`SIGTERM`, assert the direct child received and reacted to it — no
process-group assertion needed, since forwarding targets the direct child
PID only, see this file's own signal-forwarding decision above) run on
the Linux/macOS CI legs only (`#[cfg(unix)]`), mirroring GEN-24's own
`#[cfg(windows)]`-gated-ACL-tests-only-run-on-`windows-latest` precedent.
Windows `Ctrl-Break` forwarding is inherently harder to drive from a test
harness that isn't itself attached to the same console session as the
subject process — this plan defers exact Windows signal-forwarding test
mechanics to task-breakdown/implementation time rather than guessing at a
specific harness technique here, while still requiring *some* automated
coverage of the Windows fallback-to-`kill()` path. That coverage cannot
come from hard-terminating a spawned `allez` subprocess and checking
whether its own pass-through child is also gone: `child.kill()` only runs
inside an *intercepted* Ctrl-C/Ctrl-Break handler, so a hard-terminated
`allez` process never runs that handler at all and would leave the child
orphaned and still running, not gone. The fallback decision itself is
therefore covered by a unit test of that logic directly — constructing
both conditions it falls back on (a falsy `GenerateConsoleCtrlEvent`
return, and a truthy return followed by the bounded liveness recheck
still finding the child running) against a real child process the test
spawns and kills independently — which needs no console plumbing at all.
