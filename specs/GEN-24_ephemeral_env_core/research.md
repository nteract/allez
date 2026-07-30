# Phase 0 Research: Ephemeral Environment Core

This consolidates the technical unknowns from `spec.md` into decisions. Every
crate/version below was confirmed current as of 2026-07-27 via the librarian
research tasks referenced inline; see those citations for primary sources.

**Revision note**: this file was amended after a plan review surfaced several
gaps (see the review's findings). Two explicit product decisions bound this
revision: (1) GEN-29 (private-channel authentication) is deferred in full —
every auth-related design the first draft introduced is removed, not
patched; (2) checksum verification relies 100% on `rattler_cache`'s own
built-in behavior as-is — the review's "verifies after extraction, not
before" observation is accepted, not fixed, in this ticket.

**Second revision note**: a follow-up re-review found this file's own
fixes needed further correction in several places — a materially wrong
claim about hard-link isolation (corrected, not merely softened), two
stale external-state claims (PR #3 has since merged into `main`; `cargo
deny check` was independently re-run and found to pass, contradicting the
earlier "currently red" characterization), and several smaller gaps
(secure fallback-root reuse, additional Windows FFI checks, sandbox
footprint completeness, an `Unknown`-orphan escape hatch, explicit
`execute_link_scripts` configuration (this second revision cycle made it
explicitly `false`; a later remediation pass reversed this to explicitly
`true` — see the Decision below for the current, final value). Each is marked inline below at its
specific decision.

**Third revision note**: a third re-review found two further real gaps —
`await_ready()`'s terminal-state logic could resolve prematurely with a
cleanup failure still pending (fixed in `data-model.md`), and the
`Unknown`-orphan age-based escalation heuristic doesn't actually prove an
environment isn't still active, which the orphan-detection decision below
now replaces with a definitive OS-level advisory-lock check instead of a
heuristic. The fallback-root reuse check is also strengthened from an
"implementation-time refinement to consider" into a firm requirement.

**Fourth revision note**: a fourth re-review found one further real race —
directory creation, `.owner.lock` file creation, and acquiring the lock
on it are three separate OS operations, not literally one atomic step
despite the third revision's wording; a concurrent reclamation scan could
in principle observe a just-created environment directory whose lock
file doesn't exist yet, or exists but hasn't been locked yet, and either
misclassify it or (worse) acquire the lock itself and remove a directory
whose legitimate creator is still alive and about to lock it. The
orphan-detection decision below now adds a second, coarse-grained
root-level lock that serializes every creation's publication sequence
against every reclamation scan, closing this window.

**Fifth revision note (Explicit reap, no automatic reaping — supersedes
the entire "Decision: Exit cleanup + orphan detection" section below, and
every revision note above that describes it)**: a later, separate product
decision removed automatic teardown of every kind. There is no longer a
per-environment teardown signal, no `Drop`-based cleanup for a
successfully created environment, no `.owner.lock`/`.root.lock` liveness
machinery, and no `reclaim_orphaned_environments()` orphan-detection
scan — the entire mechanism the four revision notes above spent four
review cycles hardening is gone, not merely revised further. `fs4` is no
longer a dependency. In its place, this ticket adds one new, much
simpler decision: a standalone `reap_ephemeral_environments()` function
that removes every ephemeral environment it finds under the managed
root, unconditionally, with no liveness check of any kind — see
"Decision: Explicit reap, no automatic reaping" below, which replaces
"Decision: Exit cleanup + orphan detection" in full. This is a
deliberate, temporary simplification, not a permanent design point; the
four revision notes above are left in place as a historical record of
the automatic-teardown design this ticket originally shipped with, not
as a description of current behavior.

## Decision: Package resolution + install engine

**Decision**: Use the `rattler` ecosystem (native Rust libraries from
`conda/rattler`, the same libraries `pixi` uses) directly — no shelling out
to `conda`/`mamba`/`micromamba`.

```toml
rattler = "0.48.0"
rattler_conda_types = "0.49.0"
rattler_repodata_gateway = "0.31.0"
rattler_solve = "8.0.0"
rattler_cache = "0.10.4"
rattler_virtual_packages = "4.0.0"
rattler_shell = "0.27.11"
ulid = "1"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "fs", "sync"] }
reqwest = { version = "<pin at implementation time>", default-features = false, features = ["rustls-tls"] }
rustix = { version = "1", features = ["fs", "process"] }
```

**`reqwest`'s exact version is deliberately left unpinned here (corrected
in this revision — an earlier draft pinned `"0.12"` without checking
it)**: `rattler_networking` (a transitive dependency via `rattler`
itself) has changed its own `reqwest` major-version requirement at least
once in its released history, and `Installer::with_download_client`/
`Gateway::with_client` (`install.rs`/`solve.rs`) accept a client type built around
whichever `reqwest` version `rattler_networking` itself currently pins —
sharing one client across the solve/install boundary (this feature's own
design requirement, not `rattler`'s) only compiles if this feature's own
direct `reqwest` dependency resolves to that exact same major version.
Confirm the correct version via `cargo tree` against the exact `rattler`
release pinned above before writing a version number into `Cargo.toml`
at all — a mismatch here is a hard compile-time type error, not a
subtle runtime bug, so it fails loudly and immediately if guessed wrong,
but it should not be guessed at when it can simply be checked.

`rustix` is likewise promoted to an **explicit direct dependency** —
`permissions.rs` calls `rustix::fs::mkdirat` directly for
handle-anchored per-environment directory creation on Unix (see the
Anchoring-extends-to-per-environment-creation note above), which is the
same "a crate whose types/functions you name directly must be a direct
dependency" rule `reqwest` above follows.

`reqwest` is now an **explicit direct dependency, corrected in this
revision** — an earlier draft assumed it only mattered as a *transitive*
dependency (see the surrounding discussion below on the ISC/`aws-lc-rs`
license question), but `solve.rs`/`install.rs` actually
construct one `reqwest::Client` directly, with `.no_proxy()` explicitly
set, shared between the repodata `Gateway` and the `Installer` — a type
this feature's own code names and configures cannot be merely transitive;
Rust requires a crate whose types you name directly to be a direct
dependency, `Cargo.toml`-declared version included. Pinned to whichever
minor version is actually compatible with the pinned `rattler_repodata_gateway`/
`rattler`/`rattler_cache` versions above at implementation time (`0.12` is
this plan's best current estimate, not a hard requirement); `default-features
= false` plus `rustls-tls` matches this plan's own license-driven TLS-backend
preference (see Open Items below) rather than pulling in `native-tls`
by accident via `reqwest`'s own default feature set.
`rattler_networking` is **not added as a direct dependency** — see the
Scope note above; this ticket adds no authentication middleware, so there
is nothing for it to do. This does **not** mean HTTP/TLS-related crates
are absent from the dependency *tree* beyond the one now added directly:
`rattler_repodata_gateway` needs
HTTP to fetch repodata regardless of authentication, so
`rattler_networking` itself, as `rattler`'s standard HTTP
client layer, is very likely present as an *additional transitive*
dependency either
way — see the corrected Open Items note below; the earlier draft's "moot
for this ticket" framing for the ISC/`aws-lc-rs` license question was
wrong for exactly this reason. `rattler_shell` is new in this revision,
for `ReadyEnvironment::activation_environment()` (GEN-25's own PATH/env-var
requirement — see `data-model.md`). `ulid` is new in this revision — the
first draft specified `EnvironmentId` as "a newtype over `ulid::Ulid`" but
never actually added the crate to this dependency list. **`ctrlc` is
explicitly not a dependency**: `allez` is invoked by an AI agent inside an
externally-established sandbox, never directly by a human at a terminal,
so there is no human-initiated interrupt (a terminal Ctrl-C) for this
feature to ever need to handle. **`fs4` is likewise not a dependency**
(removed by the "Explicit reap, no automatic reaping" decision below,
which supersedes the per-environment/root-level advisory-locking design
an earlier revision of this file added it for) — a failed creation
attempt's own partial directory is still rolled back synchronously, as
part of the same call that discovered the failure, and a successfully
created environment is removed only by the explicit
`reap_ephemeral_environments()` call a caller makes on its own; neither
needs a signal handler or an advisory file lock. See "Decision: Explicit
reap, no automatic reaping" below.

**Rationale**:
- `rattler` is a library crate first; its CLI is feature-gated behind
  `cli-tools` and not something we depend on.
- `rattler_repodata_gateway::Gateway::query(channels, platforms, specs)`
  fetches/caches repodata from an ordered channel list and respects channel
  ordering, satisfying FR-002's "exactly as given" requirement.
- `rattler_solve::resolvo::Solver` + `SolverTask { channel_priority, .. }`
  exposes `rattler_solve::ChannelPriority::Strict`/`Disabled` — confirmed
  against `conda/rattler`'s current source (`crates/rattler_solve/src/lib.rs`):
  this enum has **exactly two variants**, `Strict` (default) and `Disabled`;
  there is no `Flexible`. Since `ChannelConfig::channel_priority` (see
  `data-model.md`) mirrors `condarc::ChannelPriority`'s real 3-variant shape
  (`Strict | Flexible | Disabled`), this feature must map `Flexible` onto
  one of rattler's two: **`Flexible → ChannelPriority::Disabled`**. Rationale
  for that specific direction: `Strict` hard-filters out every
  lower-priority channel once a higher one has a match for a given package
  — a behavior with no `Flexible` analog (conda's `flexible` mode allows a
  lower-priority channel's package to still win on version grounds).
  `Disabled` doesn't hard-filter by channel either, making it the closer
  (if imperfect) match of the two available options. This is a documented
  approximation, not an exact semantic match — revisit if `rattler_solve`
  ever adds a closer mode. **This mapping is a final, ratified decision
  (see spec.md's FR-002, which explicitly carves out this exact
  "underlying mechanism cannot represent one of the modes" case as an
  accepted approximation) — not an open question for future review.**
- `rattler::install::Installer::install(prefix, records)` creates the
  prefix, downloads via `PackageCache`, extracts, links, and writes
  `conda-meta` — this is FR-001/FR-002's create+install step in one call.
- **Checksum verification (FR-011) is relied upon exactly as `rattler_cache`
  implements it, as-is — this is a team-approved deviation from Principle
  X's literal text, signed off on during review, but not yet folded into
  `constitution.md` itself (amending the constitution is out of scope for
  a feature PR — see `plan.md`'s Constitution Check).** Per that decision,
  this ticket adds no additional verification layer. For the record (not a
  to-do for this ticket): the review noted that `PackageCache::get_or_fetch_from_url_with_retry`
  streams-extracts into a temporary destination and *then* compares the
  resulting SHA-256 (or MD5 if SHA-256 is absent) digest, deleting the
  destination on mismatch — i.e. verification happens before the archive is
  linked into any environment prefix, but after it's been extracted to a
  temp location, and a record with neither hash present is accepted
  unverified. FR-011's "before it is extracted, installed, or activated" is
  satisfied for the "installed, or activated" half; the "extracted" half is
  a known, accepted gap for this ticket, accepted by the same team-approved
  decision above. No package *signature* verification
  is performed by this path either — also accepted as-is. This has now
  been confirmed and documented multiple times across this
  document, `spec.md`'s Assumptions, and `plan.md`'s Constitution Check —
  but not in `constitution.md` itself, which still reads as originally
  written; there is no further ambiguity left to resolve about *this
  plan's* stance, and no future review pass should treat this as an open
  finding again, though a dedicated constitution-amendment change is still
  the right place to eventually fold it into Principle X's own text.
- `SolverTask.locked_packages`/`pinned_packages` stay empty and
  `rattler_lock` is not added — this keeps solving best-effort/non-pinned
  per FR-004's explicit non-determinism-across-runs allowance.
- Requires `tokio` (rt-multi-thread, since installs/downloads do concurrent
  I/O); `rattler_solve` itself is sync but `rattler`/`rattler_repodata_gateway`
  need the runtime.
- **Post-install script execution is explicitly enabled, by a team-approved deviation from Principle X's literal text — reversed from an earlier draft's opposite decision** (corrected in this revision): `Installer` is configured with `execute_link_scripts` set to `true` explicitly, overriding `rattler` 0.48's own current default (`false`) rather than leaving package post-link scripts disabled. This is a deliberate, documented decision, and the team's reading of Constitution X's post-install-script clause — *"allez MUST NOT execute arbitrary post-install/build scripts from untrusted sources without explicit, documented consent"* — is that this feature's Operating Context itself (spec.md) already supplies that consent, independently confirmed by GEN-19's epic body (updated 2026-07-27: *"Phantom secrets are securely injected into the environment such as the real secrets are never themselves present in it"*): `allez` always runs inside an externally-imposed sandbox that is the actual code-execution and damage boundary, invoked by an AI agent that needs the freedom to run arbitrary conda-packaged code (including post-link scripts) to be useful at all — restricting that inside allez itself, on top of the sandbox, would be redundant at best and would break legitimate use cases at worst. **This reading is signed off on by the team for this plan; do not re-open it without a new, explicit product decision to actually reverse it again — but note `constitution.md`'s own Principle X text hasn't been amended to say this, since amending the constitution is out of scope for a feature PR (see `plan.md`'s Constitution Check).** Separately, whether
  `rattler_shell::Activator` (used by `ReadyEnvironment::activation_environment()`)
  itself *executes* any package-supplied `activate.d`/`deactivate.d` shell
  script as part of computing the activation environment, versus only
  statically parsing environment-variable-setting commands from them
  without executing arbitrary code, remains unconfirmed as of this plan —
  worth confirming at implementation time purely for documentation
  accuracy, since either answer is acceptable under the same sandbox
  reasoning above (this is not a decision gate, just a fact to record once
  known).

**Alternatives considered**:
- Shelling out to `micromamba`/`conda` binaries: rejected — adds an external
  binary dependency per platform, defeats "written for agents" portability,
  and duplicates checksum-verification logic we'd have to trust blindly
  from stdout/stderr instead of a typed Rust `Result`.
- Hand-rolling a solver against raw repodata JSON: rejected — reimplements
  a SAT solver and checksum/extraction pipeline that `rattler_solve`/
  `rattler_cache` already provide, violating DRY (Constitution IV) and
  Security & Supply-Chain Integrity (Constitution X)'s "rely on the
  mechanism's own verification" framing.

*Source: librarian research task `bg_8f382419`, citing `conda/rattler` and
`prefix-dev/pixi` source at commit `e4ed482`/`48a1ecf4`; `ChannelPriority`
variant count independently confirmed against `conda/rattler`'s current
`main` branch (`crates/rattler_solve/src/lib.rs`, `py-rattler/src/channel/mod.rs`)
during plan review.*

## Decision: Align `ChannelConfig` with GEN-36's `condarc::Config`, not an invented shape

**Decision**: `ChannelConfig`'s field shapes (see `data-model.md`) mirror
`condarc::Config`'s relevant fields as closely as possible: a 3-variant
`ChannelPriorityMode` matching `condarc::ChannelPriority` exactly
(`Strict | Flexible | Disabled`, both `#[non_exhaustive]`), plus explicit
`allowed_channels`/`denied_channels` fields mirroring
`allowlist_channels`/`denylist_channels`.

**Rationale**: the first draft of this plan invented `ChannelConfig`/
`ChannelSpec`/`ChannelPriorityMode` from scratch, asserting a "1:1" mapping
to `rattler_solve::ChannelPriority` that doesn't hold for conda's own
default value (`Flexible`), and omitted allow/deny entirely despite
spec.md's own Key Entities section requiring it. Plan review discovered
that GEN-36 (a sub-task of GEN-23) has already landed the actual
parsed-`.condarc` type, `condarc::Config`, at `crates/condarc` in this
same repository — **PR #3 merged into `main` during this plan's review
cycle** (confirmed directly: `main` is now at the merge commit, includes
`crates/condarc`, and the source branch was deleted), so this is no
longer an in-flight dependency but an already-landed one. `condarc` was
added as a Cargo **workspace** member alongside the existing `allez`
package (`members = [".", "crates/condarc"]`, confirmed against `main`'s
actual `Cargo.toml` — `allez` itself stays exactly at the workspace root,
unmoved). `condarc::Config` is the *parsed* document, not a *resolved*
channel list (expanding `default_channels`/`custom_channels`/
`custom_multichannels`/`channel_alias`/`override_channels_enabled` into one
final ordered list remains GEN-23's own remaining scope, not GEN-36's or
this ticket's) — so `ChannelConfig` is deliberately *not* `condarc::Config`
itself, but is kept isomorphic to it wherever the two overlap, so that
future resolution step is a straightforward mapping instead of a lossy
one. This ticket does **not** add `condarc` as a dependency — the
resolution/adapter step is out of scope here either way, per spec
Assumptions ("does not read or parse `~/.condarc` itself"). **Practical
implication of the merge**: the `GEN-24_ephemeral_env_core` branch should
rebase onto the post-merge `main` before implementation starts, so
`condarc::Config` is actually present in the tree this ticket's own
sibling work (GEN-23's resolution step) will eventually need to build
against — this ticket's own code still never imports `condarc` directly.

**Alternatives considered**: Keep the original invented 2-variant
`ChannelPriorityMode` and treat the `Flexible → ???` mapping as an
implementation-time surprise — rejected, this is exactly the kind of gap a
plan should surface, not one an implementer should discover mid-coding.

*Source: GitHub PR #3 (`nteract/allez`, `crates/condarc/src/{model.rs,lib.rs}`),
merged into `main` during this plan's review cycle; Jira GEN-36; and
`docs/condarc_research.md` (confirms `channel_priority` defaults to
conda's own `flexible` when the key is absent from `.condarc`) — discovered
during plan review's context-mining pass, merge status independently
re-verified against the live repository during the second review cycle.*

## Decision: Owner-only directory permissions, applied atomically at creation (FR-014)

**Decision (corrected from the first draft)**: permissions are established
**as part of the directory-creation call itself**, not applied afterward.

- **Unix**: `std::fs::DirBuilder` with `DirBuilderExt::mode(0o700)`, called
  once per directory (`DirBuilder::new().mode(0o700).create(path)`) — the
  directory never exists with any broader permission, even momentarily.
- **Windows**: build a security descriptor granting the creating user's SID
  full control (via an SDDL string with that SID substituted in, e.g.
  `"D:PAI(A;OICI;FA;;;<SID>)"`, passed through
  `ConvertStringSecurityDescriptorToSecurityDescriptorW`), wrap it in a
  `SECURITY_ATTRIBUTES { lpSecurityDescriptor, bInheritHandle: FALSE }`, and
  pass that directly to `CreateDirectoryW`'s `lpSecurityAttributes`
  parameter — so the directory is created with the restrictive ACL already
  in place, never created-then-restricted. The security descriptor
  returned by `ConvertStringSecurityDescriptorToSecurityDescriptorW` MUST be
  freed via `LocalFree` after `CreateDirectoryW` returns (documented Win32
  ownership convention for that conversion function) — this is one of
  several explicit memory-ownership/error-handling rules the eventual
  `// SAFETY:` comment on this `unsafe` block must state:
  - `SECURITY_ATTRIBUTES.nLength` MUST be initialized to
    `size_of::<SECURITY_ATTRIBUTES>()` before the struct is passed to
    `CreateDirectoryW` — the Win32 API requires this and silently
    misbehaves if it's left zeroed.
  - `ConvertStringSecurityDescriptorToSecurityDescriptorW`'s own `BOOL`
    return value MUST be checked before using its output descriptor at
    all — a failed conversion must not be treated as "no descriptor,
    fall back to default permissions"; it must be a hard error.
  - The wide (UTF-16) path/SDDL string buffers must outlive the
    `CreateDirectoryW` call itself (no dangling pointers passed to the
    FFI boundary).
  - `CreateDirectoryW`'s own `BOOL` return value must be checked before
    assuming the directory (and its ACL) actually exists.

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.61.2", features = [
    "Win32_Foundation",
    "Win32_Security",
    "Win32_Security_Authorization",
    "Win32_Storage_FileSystem",
] }
```

**Rationale**: the first draft's "create, then `chmod`/`SetNamedSecurityInfoW`"
sequencing leaves a window — however brief — during which the directory
exists with whatever permissions its parent/OS default would otherwise
grant, before this feature's own restriction takes effect; plan review
flagged this as a real race (another local process could observe or, on
some OS/filesystem combinations, pre-populate the directory in that
window). Passing the restrictive descriptor directly into the creation
syscall itself (`DirBuilder::mode`/`CreateDirectoryW`'s own
`lpSecurityAttributes`) closes that window entirely rather than narrowing
it. `windows-sys` remains the choice over the two unmaintained
higher-level ACL wrapper crates (`windows-acl`, `windows-permissions`,
both last released 2021) for the same reason as the first draft: a small,
isolated, `// SAFETY:`-documented `unsafe` block against Microsoft's own
maintained bindings is the better trade-off (Constitution I explicitly
sanctions this exception path). This restricts access against other
non-privileged local accounts; it does not and cannot override a platform
administrator/root-equivalent account, matching the spec's Edge Cases note
verbatim.

**Alternatives considered**: `windows-acl`/`windows-permissions` crates —
rejected as unmaintained. `SetEntriesInAclW` + `SetNamedSecurityInfoW`
*after* `CreateDirectoryW` (the first draft's approach) — rejected in favor
of the atomic-at-creation approach above for the reason stated.

*Source: librarian research task `bg_e342f133`, citing `microsoft/windows-rs`
and a real-world ACL implementation (`openai/codex`'s `windows-sandbox-rs`);
atomicity correction and `LocalFree`/buffer-lifetime notes added during plan
review (security lane, finding on unspecified SID/ACL ownership).*

## Decision: Explicit reap, no automatic reaping (FR-007, FR-008)

**Supersedes "Decision: Exit cleanup + orphan detection" in full** — see
the Fifth revision note at the top of this file. A later, separate
product decision removed automatic teardown of every kind: there is no
longer a per-environment teardown signal, no `Drop`-triggered cleanup for
a successfully created environment, no per-environment `.owner.lock`
liveness marker, no root-level `.root.lock` publication/reclamation
serialization, and no `reclaim_orphaned_environments()` orphan-detection
scan. None of the machinery the superseded section below this one
describes — the four review cycles that hardened it, the `fs4`-based
advisory-lock liveness check, the diagnostics-only metadata file, the
`Unknown`/`StillActive`/`Removed` classification — exists any more.

**Decision**: A successfully created ephemeral environment is left
exactly as created, indefinitely, until a caller explicitly calls a new,
standalone function:

```rust
pub fn reap_ephemeral_environments() -> Result<Vec<ReapOutcome>, EphemeralEnvError>;

pub enum ReapOutcome {
    Removed { id: EnvironmentId },
    RemovalFailed { id: EnvironmentId, error: EphemeralEnvError },
}
```

`reap_ephemeral_environments()` resolves the managed root the same way
`create_ephemeral_environment` does (`$ALLEZ_EPHEMERAL_ROOT`, falling
back to the same per-user, per-installation temp-dir path — see the
"same caller" path-naming decision below, which is unaffected by this
revision), lists every entry directly under that root's `envs/`
directory whose name parses as an `EnvironmentId`, and calls
`cleanup.rs`'s `remove_prefix_dir()` — the same anchored, verified
removal primitive the creation-failure rollback path already uses — on
each one in turn. There is no liveness check of any kind: every entry
found is removed unconditionally, regardless of whether some other
process is still actively creating or using it. A removal failure for
one entry is reported as `ReapOutcome::RemovalFailed` and does not
prevent or affect any other entry's own outcome (FR-009); an empty
`envs/` directory yields `Ok(vec![])`, not an error (FR-008's
idempotency requirement). `Err(EphemeralEnvError::UnwritableLocation)` is
returned only if the root itself cannot be securely opened/verified — a
scan that cannot even start.

A *failed* creation attempt is unaffected by this decision: rolling back
whatever partial directory a failed attempt created still happens
synchronously, as part of the same `create_ephemeral_environment` call
that discovered the failure — see `mod.rs`'s `fail_and_roll_back()` — and
still reports a distinct `cleanup_error` if that rollback itself also
fails (FR-004/FR-010). Only a *successfully completed* creation's own
environment is now left alone rather than torn down automatically.

**Rationale**: this is an explicit, deliberate simplification, not an
oversight or an unconsidered regression. The automatic-teardown design
this decision replaces required a `Drop`-guard, an OS-level advisory
lock per environment, a second, coarse-grained root lock to serialize
creation publication against reclamation scans, a diagnostics-only
on-disk metadata file, and a multi-state internal lifecycle machine
(`LifecycleState`) just to make a caller-initiated signal, a background
reclamation scan, and an in-progress creation all correctly reconcile
with one another — real complexity, maintained across four separate
review cycles (see the revision notes at the top of this file) before
this decision replaced it outright. Removing all of it in favor of one
small, unconditional removal function trades away two guarantees the
prior design provided — an environment is never removed by this
feature while it's still genuinely in use, and leftover environments are
reclaimed automatically without a caller having to remember to do
anything — for a large reduction in surface area and failure modes. This
trade is accepted as **temporary**: a caller (or an agent driving
`allez`) that never invokes `reap_ephemeral_environments()` will
accumulate ephemeral environments on disk indefinitely, and a caller
that invokes it while another operation still needs an existing
environment can corrupt or remove that environment out from under it —
both are now the calling caller's own responsibility to avoid, not
something this feature enforces. Safer, more automatic reclamation
(e.g., reintroducing some form of liveness detection, or an exit-time
cleanup hook, without reintroducing the full state-machine complexity
above) is expected to be revisited in a future ticket.

**Alternatives considered**: Keeping the full `Drop`-guard/advisory-lock/
orphan-reclamation design and only fixing its remaining rough edges —
rejected by explicit product decision; the team judged the complexity
cost no longer worth it for this ticket's scope, preferring to ship a
much simpler, explicit-only mechanism now and revisit automatic
reclamation later once real usage patterns are better understood. A
reap operation that *does* attempt some liveness check (e.g., reusing the
removed design's advisory-lock idea, but only for reap rather than for a
background scan) — deferred, not rejected outright; nothing in this
decision precludes a future ticket adding one, but this ticket
deliberately ships the simpler, unconditional version first.

**"Same caller" path-naming (unaffected by this revision — carried
forward from the superseded design)**: FR-008 scopes `reap_ephemeral_environments()`
to "the same local user account and the same `allez` installation." The
managed root is not a bare `std::env::temp_dir()` call, since two
different OS users sharing one system temp directory (e.g. `/tmp` on
Linux/macOS) would otherwise collide on the same path. The fallback root
is therefore:

```text
std::env::temp_dir().join(format!("allez-{user}-{install_hash}"))
```

where `{user}` is `$USER`/`$LOGNAME` (Unix) or `%USERNAME%` (Windows) —
used here purely to avoid two accounts colliding on the same directory
*name*, not as the actual security boundary (an env var is not a trust
boundary; the owner-only permission from the previous decision is) — and
`{install_hash}` is a short hash of `std::env::current_exe()`'s
canonicalized path, so two different `allez` binaries/checkouts on the
same machine (e.g. during development) don't cross-scan each other's
environments. `{user}` is sanitized to a fixed safe character set before
it ever reaches this `format!` call — never interpolated verbatim, since
an attacker-influenced environment variable containing a path separator
or a `..` segment could otherwise turn this into more than one path
component. This is implemented in `paths.rs`'s `fallback_root_path()`.

**Explicit `$ALLEZ_EPHEMERAL_ROOT` and the same install-hash scoping**:
the `{install_hash}` component above exists only for the *fallback*
path — an explicitly-set `$ALLEZ_EPHEMERAL_ROOT` is honored verbatim,
with no install-hash suffix appended to it. If a caller deliberately
points two separate `allez` installations at the *same* explicit
`$ALLEZ_EPHEMERAL_ROOT` value, `reap_ephemeral_environments()` run from
either one will remove environments the other created too — this is
intentional, not a gap: an explicit override is, by definition, the
caller choosing the scope themselves.

**Secure create-or-verify on reuse (unaffected by this revision — carried
forward from the superseded design)**: because the fallback path name is
predictable (derivable by anyone who can read `$USER`/`current_exe()`'s
path), a malicious local process could attempt to pre-create it — as a
symlink to somewhere else, or with unexpected ownership/permissions —
before this feature's own first legitimate use. `paths.rs`'s
`verified_root()` never simply "creates if absent, else trusts what's
there": it opens the root (and each of its long-lived children —
`envs/`, `cache/packages/`, `cache/repodata/`) with no-follow semantics
(Unix `O_NOFOLLOW`; Windows a `CreateFileW` open with
`FILE_FLAG_OPEN_REPARSE_POINT`, which opens a reparse point/symlink as
itself rather than transparently following it), verifies ownership and
permissions on that already-open handle (never a second, separate
path-based `stat`), and anchors every subsequent operation to that
handle rather than re-resolving `$ALLEZ_EPHEMERAL_ROOT` as a fresh path
string each time. This applies identically whether the root came from an
explicitly-set `$ALLEZ_EPHEMERAL_ROOT` or the predictable fallback path —
there is no "trust it, the caller configured it explicitly" shortcut for
the former.

**Anchoring extends to per-environment directory creation and removal
too, not just the shared root's own subdirectories (unaffected by this
revision — carried forward from the superseded design)**: each
individual ephemeral environment's own directory (a new, ULID-named
subdirectory under `envs/`) is created fresh by `permissions.rs` on
every `create_ephemeral_environment` call, and removed — by the
creation-failure rollback path or by `reap_ephemeral_environments()` —
by `cleanup.rs`'s `remove_prefix_dir()`. On Unix, both operations are
anchored to the verified root's own already-open `envs/` directory
descriptor (`rustix::fs::mkdirat`/`openat`/`unlinkat`-style relative
operations), never a bare, independently-resolved path string.
`mkdirat`'s own mode argument (`0o700`) is passed directly at creation
time — the directory never exists with a broader-than-owner-only mode,
even transiently, and no `umask` manipulation is needed or used (`umask`
can only narrow a requested mode, never widen it, so `0o700` passed
directly already guarantees the result). `remove_prefix_dir()`
re-verifies its target immediately before removing it — a real
directory owned by the current user, not a symlink — via the same
anchored, no-follow discipline. On Windows, no ergonomic,
handle-relative equivalent of `openat`/`mkdirat` exists without a
materially larger `unsafe` surface than this ticket's already-scoped ACL
exception; both creation and removal instead re-verify via a fresh
no-follow `CreateFileW` open against the already-verified root's own
canonical path (captured once at verification time, never re-resolved as
a fresh string later) immediately before acting — narrowing, not fully
eliminating, the Windows-specific residual TOCTOU window, accepted for
the same reason the ACL exception itself is (see `plan.md`'s Complexity
Tracking).

*Source: librarian research task `bg_e342f133` (original "same caller"
path-naming and secure-open/verify research, largely unaffected by this
revision); this section's own "Explicit reap, no automatic reaping"
decision itself has no external citation — it is a straightforward
product/scope decision, not a technical unknown requiring research.*

## Decision: Ephemeral location + package/repodata cache (long-lived and shared, not run-scoped)

**Decision (corrected from the first draft)**: the environment prefix
lives under `$ALLEZ_EPHEMERAL_ROOT` (falling back to the per-user,
per-installation temp-dir path above) in a uniquely-named subdirectory
(`<root>/envs/<ULID>/`), created with owner-only permissions per the
decisions above — this part is unchanged. **(Revised — "Explicit reap, no
automatic reaping" above)**: this directory is no longer described as
"torn-down per environment" automatically; it persists until an explicit
`reap_ephemeral_environments()` call removes it (or a failed creation
attempt's own rollback removes it immediately, per that decision). The
package-download cache and repodata cache
(`<root>/cache/packages/`, `<root>/cache/repodata/`) are, however,
**deliberately long-lived and shared across every ephemeral environment
this installation ever creates** — created once, reused by every
subsequent `create_ephemeral_environment` call, **never removed by
`reap_ephemeral_environments()` or by a failed creation's own rollback.**
Both cache directories still get the same owner-only permission treatment
at creation.

**Rationale**: the first draft described this cache as "this-run-scoped,
non-shared," which both contradicted its own fixed, reused path
(`<root>/cache/...` doesn't change between runs) and — more importantly —
was a worse design than what spec.md's own Assumptions already permit:
"Whether the underlying package-resolution/install mechanism maintains
its own shared package-download cache across separate ephemeral
environments (for performance) is not defined by this feature and is
deferred to whichever ticket addresses performance (e.g. GEN-32); this
feature's own guarantee is only that the reap operation removes the
ephemeral environment's directory and the packages installed into it."
A shared, persistent cache is explicitly anticipated by that wording —
and GEN-32's own acceptance criteria need a real cold-vs-warm-cache
distinction to have anything to benchmark; a cache whose lifetime is tied
to a single environment's lifetime can never produce a "warm" case at
all. FR-008's/SC-002's "fully removed" guarantee is unaffected: it
describes the *environment prefix* (`<root>/envs/<id>/`), which was
never a package's own download-cache location to begin with (rattler's
install pipeline downloads into the cache, then links/copies from there
into the prefix — the cache and the prefix are already two different
directories in every draft of this plan).

**Residual risk, corrected in this revision**: a package's own executed code
(inside a running environment) could reach the shared cache directory if
`rattler::install::Installer` hard-links cached files into environment
prefixes rather than copying them. **Correction**: the first draft of this
note incorrectly claimed that even if hard-linking is used, an
already-installed file in a sibling environment's own prefix would be
unaffected by such a mutation "since each prefix's directory entry is
independent once linked" — this is wrong. A hard link shares the same
underlying inode/data blocks across every directory entry pointing to it;
an in-place content mutation through *any* one of those entries (e.g. a
running package overwriting its own installed file rather than replacing
it) is visible through *every* hard link to that inode, including the
cache's own copy and every other environment's already-installed copy.
Directory-entry independence only protects against *unlinking* (deleting
one entry doesn't delete the others), not against in-place content
mutation.

Given that correction, this ticket's decision is: **hard-linking from the
shared cache into environment prefixes MUST be disabled if
`rattler::install::Installer` exposes any option to do so** (verify the
exact API name/shape at implementation time — this plan does not assert
one, since it hasn't been confirmed against `rattler`'s actual source; a
link-mode/link-options builder method is the likely shape). If no such
override exists at all, this ticket falls back to **per-environment,
non-shared package caches** rather than accept silent cross-environment
mutation as an open residual risk — this reverses the "accept as a
residual risk" framing the first draft of this note used, since "prefer a
copy-only mode if available" is not an adequate mitigation for a risk this
concrete once the actual sharing mechanism is understood correctly. The
repodata cache (`<root>/cache/repodata/`) is unaffected by this concern —
it holds fetched metadata, not linked-into-prefix package files.

**Closing this out as an accepted, final design (not an open scope-creep
question)**: a shared, persistent package/repodata cache is deliberately
kept (not narrowed to per-environment-only caching) because GEN-32's own
acceptance criteria need a real cold-vs-warm-cache distinction to
benchmark, and because this feature's entire security model already rests
on the externally-imposed sandbox being the actual damage boundary (see
the Post-install script execution decision above and spec.md's Operating
Context) — a compromised package mutating allez's own internal package
cache is exactly the kind of "worst case, contained by the sandbox"
scenario this feature's design already accepts for arbitrary in-sandbox
code execution generally, not a new or different risk category requiring
its own separate mitigation. The hard-link-disable-or-fallback mitigation
above remains in place as good practice regardless (it costs nothing and
closes an easy, unforced mistake), but the underlying "could a running
package touch shared state" question is not this feature's to solve on
top of the sandbox.

**Cross-invocation addendum (ratified, 2026-07-27 — a final, maintainer-approved
product decision, not an open question)**: a security-review pass raised
a sharper version of the same question: since the shared package/repodata
cache is deliberately the one piece of state that survives *across*
separate `allez` invocations (not just across sibling environments within
one invocation — that's the whole point of keeping it, per the paragraph
above), a compromised package's post-link script (execution permitted per
the decision above) could in principle write to that cache during one
sandboxed invocation, and a **later, separate** sandboxed invocation
could then read the poisoned result — a scenario the "contained within
this invocation's own sandbox" framing above doesn't, on its own, fully
cover, since the cache is specifically designed to outlive that
boundary. **Explicit product decision: this is an accepted risk**,
consistent with — not an exception to — this feature's overall security
posture: every sandbox invocation of `allez` already trusts whatever
`$ALLEZ_EPHEMERAL_ROOT` currently contains (that trust is inherent to
having a shared root at all, independent of post-link scripts
specifically), and the alternative (per-invocation cache
re-verification, or abandoning the shared cache) would give up GEN-32's
real cold-vs-warm-cache benchmarking distinction to guard against a risk
this feature's design already accepts in spirit elsewhere. This decision
is final; do not re-raise it without a new, explicit decision to actually
change the approach.

**Rationale for the fallback path change**: see the previous decision's
"same caller" path-naming section — this applies identically to the cache
root, which lives under the same `$ALLEZ_EPHEMERAL_ROOT`/fallback base.

**Alternatives considered**: Keeping the cache run-scoped/non-shared (the
first draft's stated intent) — rejected per the rationale above. A fully
separate, GEN-32-owned shared-cache design — deferred; this ticket does
the minimum viable version (a persistent directory, not a run-scoped one)
without building GEN-32's eventual cross-installation or size-bounded
cache-eviction policy.

## Decision: Sandbox-visible filesystem/process footprint

**Decision**: Every filesystem path this feature (transitively, via
`rattler`) can touch is either (a) fully overridden with an explicit,
`$ALLEZ_EPHEMERAL_ROOT`-derived path (never a crate default), (b)
deliberately disabled/avoided because it's unnecessary for this feature's
scope, or (c) a small, enumerated, unavoidable OS-level touchpoint that
must be documented so a nono sandbox profile wrapping `allez` can be
authored to explicitly grant it. **Verified still current**: GEN-19's
epic body (checked directly against Jira during plan review, updated the
same day as this ticket's other Slack activity) still explicitly states
allez "is designed to always run inside a sandbox... [with] Phantom
secrets... securely injected into the environment" — the only sandbox-related
change made elsewhere in the tracker was closing GEN-28, an unrelated
`allez sandbox` *interactive-subshell CLI subcommand* feature, not this
Operating Context premise.

| Touchpoint | Default (if unconfigured) | This feature's choice |
|---|---|---|
| `rattler_cache::PackageCache` | none — `::new(path)` always requires an explicit path | Always `<root>/cache/packages/` (long-lived — see previous decision) — never `rattler_cache::default_cache_dir()`. |
| `rattler_repodata_gateway::Gateway` cache dir | `dirs::cache_dir()/rattler/cache` if `.with_cache_dir()` omitted | Always call `.with_cache_dir(<root>/cache/repodata/)` explicitly. |
| `rattler::install::Installer` package cache | `rattler_cache::default_cache_dir()/pkgs` if `.with_package_cache()` omitted | Always call `.with_package_cache(PackageCache::new(<root>/cache/packages/))` explicitly. |
| `rattler_conda_types::Prefix::create` | N/A — only ever touches the path it's given | Always `<root>/envs/<id>/`; nothing else to configure. |
| Authentication (`rattler_networking`) | N/A | **Not used in this ticket's scope at all** — no `AuthenticationMiddleware` layer, no credential-source path/env var/keyring access of any kind. Removed entirely from this revision (see Scope note); GEN-29 owns this section's eventual content. |
| `rattler_virtual_packages::VirtualPackages::detect()` — CUDA detection | Dynamically loads `libcuda.so[.1]` from several hardcoded paths (incl. `/usr/lib/wsl/lib/...`); on musl, instead executes `nvidia-smi --query -u -x` as a subprocess | Explicitly disabled via `VirtualPackageOverrides`. **Consequence, stated explicitly**: a requested package with a `__cuda`-constrained dependency will either fail to resolve (if it hard-requires `__cuda`) or fall back to a CPU-only variant if one exists in the configured channels — both are FR-004-compliant outcomes (clean failure, or a successful non-GPU-accelerated install), just never a GPU-accelerated one. Revisit only if a future ticket needs GPU-constrained package support. |
| `VirtualPackages::detect()` — OS/libc/arch detection | macOS: reads `/System/Library/CoreServices/SystemVersion.plist`. Linux: `uname` syscall + `dlopen("libc.so.6")`/`gnu_get_libc_version`, falling back to executing `ldd --version` only if that symbol load fails. Windows: `winver` crate's WinAPI/WMI-based version query. | Left enabled (needed for correct solving against platform-specific packages). |
| macOS install-time code signing | `rattler::install`'s `AppleCodeSignBehavior` defaults to `Fail` (not `DoNothing`) when a linked package needs (re-)signing, and signs via executing `/usr/bin/codesign` | Left at the default `Fail`/sign behavior — required for installed Apple Silicon binaries to actually execute. |
| TLS certificate trust (new row, this revision) | `reqwest`'s TLS backend either reads the OS certificate store (`native-tls`) or bundles its own root set (`rustls` + `webpki-roots`, no filesystem access) | Prefer `rustls`/`webpki-roots` explicitly (already this plan's stated license-driven preference — see Open Items) specifically *because* it avoids an additional OS-cert-store filesystem grant, not only for licensing reasons. If `native-tls` is ever selected instead, the OS certificate store becomes an additional required sandbox grant. |
| HTTP proxy environment variables (new row, this revision) | `reqwest` reads `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY` env vars by default unless disabled | Explicitly call `.no_proxy()` on the HTTP client builder — matching Constitution V's "magic behavior... MUST be opt-in, not default": this feature does not implicitly trust ambient proxy configuration any more than it implicitly trusts ambient credential stores. Revisit if a future ticket needs explicit, opt-in proxy support. |

**Unavoidable OS-level touchpoints a wrapping sandbox profile must grant**
(beyond `$ALLEZ_EPHEMERAL_ROOT` and outbound network access to the
configured channels):

- **macOS aarch64**: read `/System/Library/CoreServices/SystemVersion.plist`; execute `/usr/bin/codesign`.
- **Linux aarch64/amd64**: the `uname` syscall; read/`dlopen` the system `libc.so.6`; as a rare fallback only, execute `ldd --version`.
- **Windows amd64**: OS version APIs/WMI queried by the `winver` crate (no explicit file path to grant).

**Rationale**: Constitution VII and the Operating Context's sandboxing
premise both point the same direction — every path this feature's own
code chooses must be explicit and overridable, and every path/exec target
it cannot avoid must be enumerated up front.

**Alternatives considered**: Relying on `rattler`'s own defaults and only
fixing sandbox denials reactively — rejected, exactly the kind of
implicit-filesystem-dependency Constitution V warns against.

*Source: librarian research task `bg_ea65b6f3`, citing `conda/rattler` tag
`rattler-v0.48.0` (commit `e4ed4827`); GEN-19/GEN-28 verification performed
directly against Jira during plan review.*

## Decision: Test strategy for solve+install without live network dependency

**Decision**: Author a small local, `file://`-served conda channel fixture
(a handful of tiny `noarch` packages with known names/versions/hashes,
generated once and checked in, mirroring the existing
`conformance/condarc/*` fixture pattern) for the integration tests that
exercise the real solve → install → reap path. Tests specifically
needing live, real-world channels follow the same opt-in pattern as
`condarc_conformance`: gated behind a Cargo feature, off by default —
concretely, `network-tests` (no longer just an illustrative name: this is
the concrete feature this ticket's own test suite adds, for the FR-015
`defaults`-fallback end-to-end proof described above).

**Rationale**: Constitution II requires unit tests to be "isolated,
deterministic, and fast" — a live-network dependency in the default test
suite violates that and would make CI flaky. A local fixture channel keeps
full acceptance-scenario coverage deterministic and fast while still
exercising the real `rattler` solve/install/checksum code paths.

**Alternatives considered**: Mocking `rattler`'s types entirely — rejected;
it would test our own glue code only, not the actual checksum-verification
and channel-ordering behavior the spec cares about.

## Open items carried to Phase 2 (`/speckit.tasks`), not Phase 0/1 gate blockers

- **License/supply-chain verification is unverified, not just pending —
  and the specific "currently red" framing from the prior review cycle
  was itself corrected during the second review cycle**: `cargo deny
  check` was independently re-run directly against both this branch's
  HEAD and the post-merge `main` (with `crates/condarc` and its own
  dependencies included) during the second review cycle, using the
  locally-installed `cargo-deny` 0.20.2 — **it passes cleanly
  (advisories/bans/licenses/sources all green)** on both. The earlier
  "currently red at HEAD" claim came from a single contributor's PR #4
  comment using `cargo-deny` 0.18.5, which rejects `deny.toml`'s existing
  `allow-workspace` key as unrecognized — a tool-version-specific issue,
  not a current, environment-independent state of this repository. PR #4
  itself closed unmerged and never touched `deny.toml`. This does not
  change this ticket's own obligation to verify its *new* dependencies
  with a real `cargo deny check` run at implementation time (using
  whatever `cargo-deny` version CI actually pins), rather than asserting
  license compatibility from a manual reading — it just means that
  obligation isn't blocked on some pre-existing, unrelated red state.
  **Repo-wide gap, now closed by this ticket rather than left as FYI**: `.github/workflows/ci.yml`
  had no `cargo-deny` *or* `cargo-audit` job at all, despite Constitution X requiring
  both to run in CI and block merge on failure —
  a pre-existing gap independent of GEN-24's own feature scope, but one
  this ticket's own new dependencies make worth finally closing rather
  than inheriting silently; both jobs are required as part of this
  ticket's own CI setup work.
- **The ISC/`aws-lc-rs` license question is not moot for this ticket** —
  corrected during the second review cycle. The first draft of this note
  reasoned that since `rattler_networking`/`reqwest` are not added as
  *direct* dependencies (auth is fully deferred), the TLS-backend license
  question doesn't apply here. That reasoning conflates "not opting into
  rattler's authentication features" with "not depending on HTTP at
  all": `rattler_repodata_gateway` needs an HTTP client to fetch repodata
  regardless of authentication, so `reqwest` (and quite possibly
  `rattler_networking` itself, as rattler's own default HTTP-client
  layer) are very likely present as *transitive* dependencies of
  `rattler`/`rattler_repodata_gateway` either way. This must be verified
  with `cargo tree`/`cargo deny check` against the actual resolved
  dependency graph at implementation time, not assumed away — if
  `aws-lc-rs`/`ring` do appear (via `reqwest`'s `rustls` feature or
  otherwise), `ISC` needs adding to `deny.toml`'s allow-list.
- Windows ACL code path (this ticket's one `unsafe` exception) MUST be
  exercised in CI, with a dedicated Windows-only integration test that
  explicitly asserts the created directory's effective ACL (e.g. via
  `GetNamedSecurityInfo`) grants access only to the creating user's SID
  — not merely a happy-path `CreateDirectoryW`-succeeded check.
- **`GEN-24_ephemeral_env_core` should rebase onto post-merge `main`**
  before implementation starts (see the `ChannelConfig`/GEN-36 decision
  above) — `crates/condarc` and the workspace-member `Cargo.toml` change
  are now on `main`, not still in-flight.
- **Whether `rattler`'s own channel-name resolution recognizes the bare
  string `"defaults"` the same way conda's CLI does is unverified, not
  assumed** (new — see the FR-015 empty-`channels` fallback decision,
  added after team discussion on this ticket's own review thread):
  `channels.rs`'s fallback substitutes the literal string `"defaults"` as
  a `ChannelSpec.url_or_name` when the caller supplies no channels at
  all, on the assumption that whichever channel-resolution step
  `rattler_repodata_gateway::Gateway`/`rattler_conda_types::Channel`
  performs already understands that name the same way conda's own
  `channel_alias`/`default_channels` convention does (i.e., it expands to
  Anaconda's real default channel set, not treated as a literal,
  unresolvable relative path). This must be confirmed against `rattler`'s
  actual source/behavior at implementation time — do not guess a
  resolution mechanism and design around it; the opt-in, network-requiring
  test this ticket's own task breakdown adds is exactly the mechanism that
  confirms it, matching how the `reqwest` version and the
  `rattler_shell::Activator` script-execution question above are each
  left for implementation-time confirmation rather than asserted in
  advance. If `rattler` does *not* resolve the bare name `"defaults"` on
  its own, this feature's fallback needs a literal, hardcoded URL (or
  small set of URLs) instead of the bare name — a follow-up correction to
  this decision, not a reason to abandon the fallback itself.
</content>
