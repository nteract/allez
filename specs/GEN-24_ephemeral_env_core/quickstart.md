# Quickstart: Validating Ephemeral Environment Core

This is a validation/run guide, not an implementation walkthrough — see
`contracts/ephemeral_env_api.md` for the API and `data-model.md` for types.
Task-by-task implementation breakdown is a separate, later artifact.

## Prerequisites

- Rust toolchain matching `Cargo.toml`'s `edition = "2024"`.
- No network access required for the default test suite (see `research.md`
  § Test strategy) — the local `file://` fixture channel is checked into
  the repo. One test is the exception: the FR-015 defaults-fallback
  end-to-end proof requires real network access and lives behind its own
  opt-in `network-tests` Cargo feature, off by default, mirroring
  `condarc_conformance`'s existing `conformance-tests` pattern — see
  "Run the full test suite" below.
- Windows-specific ACL behavior (FR-014) can only be validated on an
  actual Windows host/runner: the `#[cfg(windows)]` unit tests against the
  `windows-sys` call wrappers only compile and run on the Windows leg of
  the CI matrix (`windows-latest`) — there is no equivalent code path to
  exercise on macOS/Linux CI at all, `#[cfg(windows)]` excludes it
  entirely on those platforms by construction. The other three target
  platforms' owner-only-permission behavior (Unix `DirBuilder::mode`) is
  validated by their own `#[cfg(unix)]` unit tests, which run on both the
  macOS and Linux CI legs.
- Each test process should set its own `ALLEZ_EPHEMERAL_ROOT` (e.g. a
  fresh `tempfile::tempdir()` per test binary invocation) — the package/
  repodata cache under this root is deliberately long-lived and shared
  (see `research.md`), so tests that don't isolate their root from each
  other, or from a developer's own manual runs, could observe each
  other's cached packages or orphan-scan state. **This alone is not
  sufficient isolation for tests within the same integration-test binary
  (corrected in this revision — an earlier draft implied "one root per
  binary" was the whole story)**: `cargo test` runs every `#[test]`
  function inside one compiled test binary (e.g. all of `tests/ephemeral_env.rs`'s
  dozens of test functions) concurrently, on multiple threads, by
  default — but `ALLEZ_EPHEMERAL_ROOT` is a single process-wide
  environment variable, so setting it once per binary still leaves every
  test function within that binary sharing the *same* root and its
  automatic orphan-reclamation scan. A test that deliberately creates an
  orphaned directory to assert against could have it consumed by a
  *different*, concurrently-running test's own `create_ephemeral_environment()`
  call before the first test's own assertion runs. Any test that touches
  the shared root directly (orphan-reclamation tests, root-lock-timeout
  tests, and any test relying on `ALLEZ_EPHEMERAL_ROOT` being exclusively
  its own) MUST serialize against every other such test — e.g. via the
  `serial_test` crate's `#[serial]` attribute, or an equivalent shared
  `Mutex` — and additionally give itself its own fresh, uniquely-named
  subdirectory even while serialized, so failures in one such test don't
  leave state a later one could misinterpret.
- The root lock's own acquisition wait is a configurable timeout
  (`ALLEZ_ROOT_LOCK_TIMEOUT_MS`, default 2000ms — see `research.md`);
  tests exercising this specifically (a deliberately-held root lock, or
  an invalid override value) should set an explicit, short override
  rather than relying on the 2-second default, to keep the suite fast.
- Each test process should also neutralize ambient conda-related
  environment variables it doesn't control (`CONDA_*`, `CONDARC`,
  `HTTP_PROXY`/`HTTPS_PROXY`) before exercising the real solve→install
  path, mirroring the convention `tests/condarc_conformance.rs` already
  establishes in this repo — real `rattler` solves/installs are far more
  sensitive to ambient configuration than that conformance suite's own
  oracle comparison is, so skipping this sanitization risks
  non-deterministic behavior on a developer's own machine (Constitution
  IX).

## Setup

```sh
cargo build
```

No environment variables are *required* to exercise this feature directly
(it is a library — see Assumptions in `spec.md`: no finished CLI
subcommand yet) — `ALLEZ_EPHEMERAL_ROOT` is optional and falls back to a
per-user, per-installation temp-dir path if unset (see `research.md`).
Setting it explicitly is recommended for tests (see above) and is exactly
the lever a wrapping sandbox profile uses in production. Tests construct
`ChannelConfig` values in-process pointing at the checked-in local fixture
channel.

## Run the full test suite (network-free, deterministic)

```sh
cargo test --all
```

Separately, once real network access is available, run the one
network-requiring test (the FR-015 defaults-fallback end-to-end proof)
via its own opt-in feature:

```sh
cargo test --all --features network-tests
```

This must include, at minimum, one test per acceptance scenario in
`spec.md` (Constitution VIII: 100% spec test coverage) — for example
(non-exhaustive; exact test IDs/names are decided during task breakdown):

- Create with a small resolvable package list against the fixture channel
  → environment location returned, packages present and usable
  (User Story 1, Scenario 1; SC-001).
- Create with an empty package list, no override → default packages
  installed (User Story 3, Scenario 1).
- Create with an empty package list, an override configured → override
  packages installed instead (User Story 3, Scenario 2).
- Create with explicit packages requested alongside a configured
  override → only the explicit packages (and their own dependencies) are
  installed; the override contributes nothing (User Story 3, Scenario 3).
- Create with an override that itself resolves to an empty list → treated
  identically to no override; the built-in default set is installed
  (FR-006 edge case).
- Create with an unresolvable package name → creation fails with
  `EphemeralEnvError::UnresolvablePackage`, no partial directory left on
  disk (User Story 1, Scenario 3; SC-004).
- Create with a package whose fixture-channel repodata record has a
  deliberately mismatched checksum (a corrupted-artifact fixture entry,
  distinct from the normal resolvable packages) → creation fails with
  `EphemeralEnvError::IntegrityVerificationFailed`, no partial directory
  left on disk (FR-011; SC-006 — previously untested).
- Create against two fixture channels offering conflicting versions of
  the same package under `channel_priority: ChannelPriorityMode::Strict`
  → the higher-priority (index 0) channel's version installs, proving
  channel *ordering* itself is honored, not only allow/deny filtering and
  the `Flexible` mapping (FR-002).
- Create with zero channels configured → the built-in `defaults` channel
  is substituted in before allow/deny filtering runs, proving the
  substitution logic itself (a pure, network-free function) resolves an
  empty `channels` list to `["defaults"]` rather than short-circuiting to
  `NoChannelsConfigured` (User Story 1, Scenario 4; FR-015). Actually
  resolving and installing against the real `defaults` channel requires
  live network access and is not part of this network-free suite — see
  "Run the full test suite" above for the separate, opt-in
  `network-tests`-gated test that proves that end-to-end.
- Create with zero channels configured, where the substituted `defaults`
  channel is itself present in `denied_channels` (or `allowed_channels` is
  non-empty and excludes it) → the fallback is filtered out same as any
  other channel, and creation fails with `EphemeralEnvError::NoChannelsConfigured`,
  no directory left on disk — the one path that still reaches this category
  for an originally-empty channel list (FR-015; FR-002 allow/deny honoring).
- Signal teardown after successful creation → directory fully removed
  (User Story 2, Scenario 1; SC-002).
- Signal teardown twice → second signal is a no-op, no error (User Story
  2, Scenario 4).
- Signal teardown before creation finishes → install completes
  undisturbed, then cleanup runs automatically (User Story 2, Scenario 5).
- Two concurrent environments; tear one down → the other is unaffected
  (US2 Scenario 3; FR-009).
- Every create/install/teardown step emits an `EphemeralLifecycleEvent`
  (capture via a `tracing` test subscriber) carrying one consistent
  `environment_id` per environment (FR-013/SC-008).
- Create with a channel present in both `channels` and `denied_channels`
  (or absent from a non-empty `allowed_channels`) → that channel is
  filtered out before solving; if filtering empties the effective list,
  creation fails with `NoChannelsConfigured` (FR-002 allow/deny honoring).
- Create against a fixture `ChannelConfig` with `channel_priority:
  ChannelPriorityMode::Flexible` → internally mapped to
  `rattler_solve::ChannelPriority::Disabled` and solves successfully
  (documented approximation — see `research.md`).
- Create with a package that fails to resolve, where cleanup of the
  partially-created directory also fails (e.g. `chmod` the prefix to
  read-only on Unix, or hold an open handle without `FILE_SHARE_DELETE`
  on Windows — a deterministic failure-injection technique, not a
  "locked file," which doesn't reliably block removal on Unix) →
  `await_ready()` does **not** resolve while cleanup is still running
  (`CreationFailed { cleanup: Running, .. }` is explicitly non-terminal —
  see `data-model.md`; assert `await_ready()`'s future has not completed
  yet while cleanup is in flight), then resolves only once cleanup
  reaches its own terminal state, returning `CreationFailure { error:
  UnresolvablePackage, cleanup_error: Some(TeardownFailed) }` — both
  failures surfaced, neither masking the other, and neither silently
  dropped by resolving too early (FR-010).
- Signal teardown while `LifecycleState` is `CreationFailed { cleanup:
  Running }` → no-op/folds into the already-running cleanup; no second
  removal attempt is started (FR-012, the branch the first draft's
  `LifecycleState` didn't handle).
- Dropping an `EphemeralEnvironmentHandle` without ever calling
  `signal_teardown()` (simulating the owning process's normal,
  no-explicit-signal exit from scope) → the environment directory is
  still removed via `CleanupGuard`'s `Drop`, distinct from both the
  explicit-signal path above and the crash-then-reclaim path below — this
  is FR-008/SC-003's "cleanup completing at exit time" half, previously
  exercised only implicitly (FR-008).
- A leftover environment directory whose `.owner.lock` can be acquired
  (no live process holds it) → reported as `OrphanReclamationOutcome::Removed`
  and removed, purely from the successful lock acquisition — never from
  the directory's age or a PID/metadata match (FR-008's "detected, not
  silently undetected" guarantee, now backed by a definitive OS-level
  check rather than a heuristic).
- A leftover environment directory whose `.owner.lock` is still held by a
  live process → classified `StillActive`, never touched, regardless of
  how old the directory is.
- A leftover environment directory whose lock file itself can't be
  opened (simulate via a permission error) → classified `Unknown`, never
  removed, but still present in the reported outcomes.
- `ReadyEnvironment::activation_environment()` returns a `PATH` entry
  that includes the environment's own `bin`/`Scripts` directory (GEN-25
  forward-compatibility check); a forced activation failure returns
  `ActivationError`, not an `EphemeralEnvError` variant.
- A channel URL containing embedded userinfo (`https://user:pass@host/...`)
  or a conda-token-style path (`https://host/t/<token>/...`) never appears
  verbatim in any `EphemeralEnvError` message or `EphemeralLifecycleEvent`
  field — assert the redacted form only (credential-redaction check,
  FR-013).
- Two ephemeral environments created concurrently, one signaled to tear
  down and failing (e.g. a read-only prefix on Unix / an open handle
  without `FILE_SHARE_DELETE` on Windows, the same deterministic
  failure-injection technique as above), the other torn down successfully
  → each environment's own `await_torn_down()` result reflects only its
  own outcome, never the sibling's (distinct from the dual-failure
  `CreationFailure` case above, which is about a single environment's
  creation-then-cleanup, not two independent environments).
- `reclamation_outcomes()` returns `ReclamationStatus::Scanning` before
  the automatic scan completes, then `ReclamationStatus::Complete(_)`
  afterward, and keeps returning the same `Complete(_)` value on repeated
  polls (FR-008's "distinct signal" is actually observable, not just
  eventually consistent by accident).
- A reclamation scan started while `envs/.root.lock` is held by a
  concurrent creation's own publication sequence waits for that lock
  rather than proceeding — no directory is ever reported `Removed`
  while its creator is still mid-publication (the race the fourth review
  cycle's root-lock fix specifically closes).

## Manual smoke test (optional, illustrative)

Once implemented, a throwaway `examples/ephemeral_smoke.rs` (or a `cargo
test -- --ignored` test) can exercise the real flow end-to-end against the
local fixture channel:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channels = allez::ephemeral::ChannelConfig::from_urls(vec![
        "file:///path/to/fixture-channel".to_string(),
    ]);
    let handle = allez::ephemeral::create_ephemeral_environment(
        allez::ephemeral::RequestedPackages::UseDefaultOrOverride,
        channels,
        None,
    );
    let ready = handle.await_ready().await?;
    println!("environment ready at {}", ready.location.display());
    for pkg in &ready.installed_packages {
        println!("  {} {}", pkg.name, pkg.version);
    }
    handle.signal_teardown();
    handle.await_torn_down().await?;
    assert!(!ready.location.exists());
    Ok(())
}
```

Expected outcome: prints the resolved prefix path and installed package
list, then the directory is gone by the time the process exits.

## Validating owner-only permissions manually (per-platform)

- **Unix**: after creation, `stat -f '%Sp' <location>` (macOS) or
  `stat -c '%A' <location>` (Linux) should show `drwx------`.
- **Windows**: `icacls <location>` should show access granted only to the
  creating user (and, implicitly, whatever administrator access Windows
  itself never lets you fully exclude — see `spec.md` Edge Cases).

## Validating orphan reclamation manually

1. Start creating an ephemeral environment, then kill the process with
   `SIGKILL`/`taskkill /F` before it finishes (bypassing the `Drop` guard
   entirely, on purpose — there is no other exit-cleanup mechanism to
   bypass, since this feature installs no signal handler of any kind;
   see `research.md`'s Exit cleanup decision) — this also
   forces the OS to release the `.owner.lock` file's advisory lock,
   exactly as it would for any other file handle the killed process held.
2. Confirm the leftover directory (and its `.owner.lock` file, and its
   `{pid, created_at, environment_id, packages}` metadata file — all
   diagnostic-only, not load-bearing) is still present on disk.
3. Invoke `create_ephemeral_environment` again for the same user/`allez`
   installation.
4. Confirm the leftover directory from step 1 is gone (or, if its removal
   itself fails, that a `RemovalFailed` outcome was reported), retrievable
   via the new handle's `reclamation_outcomes()` once it returns
   `ReclamationStatus::Complete(_)` (poll past any `Scanning` result first)
   — and that this new creation's own success/failure was unaffected by
   that reclamation attempt (FR-008's "MUST NOT block or fail the new
   creation itself").
5. To confirm the lock-based check is definitive, not age-based: create
   an environment, keep the owning process alive and running (don't kill
   it), and confirm a concurrent `create_ephemeral_environment` call's
   own reclamation scan reports it as `StillActive` and does not touch
   it — even if the test artificially backdates the directory's
   modification time to make it look old. Age must have zero influence
   on this outcome.
6. To exercise the `Unknown` path: simulate a permission error opening a
   candidate directory's `.owner.lock` file (e.g. via a restrictive
   parent-directory permission in a test harness); confirm the outcome
   is `Unknown`, not `Removed` or `StillActive`, and that the directory
   is left untouched.
7. To confirm the root-lock serialization actually closes the
   creation/reclamation race (not just documents it): hold
   `envs/.root.lock` externally (e.g. from a test harness thread) for a
   short window, then trigger `create_ephemeral_environment` and a
   concurrent `reclaim_orphaned_environments()` from two different
   simulated callers at once; confirm neither proceeds past its own
   `mkdir`/enumeration step until the externally-held lock is released,
   and confirm the reclamation scan never observes a directory this
   create call has started but not yet finished publishing (no
   directory should ever be reported `Removed` while its own creator
   is still inside that publication window).
