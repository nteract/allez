# Quickstart: Validating Ephemeral Environment Core

This is a validation/run guide, not an implementation walkthrough — see
`contracts/ephemeral_env_api.md` for the API and `data-model.md` for types.
Task-by-task implementation breakdown is a separate, later artifact.

**Explicit reap, no automatic reaping**: this feature does not tear down a
successfully created ephemeral environment on its own — not on a signal,
not on process exit, not via any orphan-detection mechanism. A successfully
created environment persists on disk, usable, until a caller explicitly
calls [`reap_ephemeral_environments`], which removes every ephemeral
environment it finds for the current local user account and `allez`
installation, unconditionally, without checking whether any of them is
still in use. See `spec.md`'s User Story 2 and `research.md`'s "Explicit
reap, no automatic reaping" decision for the full rationale; this guide's
validation steps reflect that behavior throughout.

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
  platforms' owner-only-permission behavior (Unix `mkdirat`/mode) is
  validated by their own `#[cfg(unix)]` unit tests, which run on both the
  macOS and Linux CI legs.
- Each test process should set its own `ALLEZ_EPHEMERAL_ROOT` (e.g. a
  fresh `tempfile::tempdir()` per test binary invocation) — the package/
  repodata cache under this root is deliberately long-lived and shared
  (see `research.md`), so tests that don't isolate their root from each
  other, or from a developer's own manual runs, could observe each
  other's cached packages or (now that `reap_ephemeral_environments()`
  removes *every* environment it finds under a root, unconditionally)
  each other's environments. `cargo test` runs every `#[test]` function
  inside one compiled test binary (e.g. all of `tests/ephemeral_env.rs`'s
  dozens of test functions) concurrently, on multiple threads, by
  default — but `ALLEZ_EPHEMERAL_ROOT` is a single process-wide
  environment variable, so setting it once per binary still leaves every
  test function within that binary sharing the *same* root. Any test
  that calls `reap_ephemeral_environments()`, or that otherwise relies on
  `ALLEZ_EPHEMERAL_ROOT` being exclusively its own, MUST serialize against
  every other such test — e.g. via the `serial_test` crate's `#[serial]`
  attribute, or an equivalent shared `Mutex` — since a reap call has no
  way to distinguish "this test's own environments" from a concurrently
  running sibling test's.
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
  left on disk (FR-011; SC-006).
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
- Create with a channel present in both `channels` and `denied_channels`
  (or absent from a non-empty `allowed_channels`) → that channel is
  filtered out before solving; if filtering empties the effective list,
  creation fails with `NoChannelsConfigured` (FR-002 allow/deny honoring).
- Create against a fixture `ChannelConfig` with `channel_priority:
  ChannelPriorityMode::Flexible` → internally mapped to
  `rattler_solve::ChannelPriority::Disabled` and solves successfully
  (documented approximation — see `research.md`).
- Every create/install step emits an `EphemeralLifecycleEvent` (capture
  via a `tracing` test subscriber) carrying one consistent
  `environment_id`, a `packages` field matching the effective top-level
  package set, and a well-formed `duration_ms` (FR-013/SC-008).
- A channel URL containing embedded userinfo (`https://user:pass@host/...`)
  or a conda-token-style path (`https://host/t/<token>/...`) never appears
  verbatim in any `EphemeralEnvError` message or `EphemeralLifecycleEvent`
  field — assert the redacted form only (credential-redaction check,
  FR-013).
- `ReadyEnvironment::activation_environment()` returns a `PATH` entry
  that includes the environment's own `bin`/`Scripts` directory (GEN-25
  forward-compatibility check); a forced activation failure returns
  `ActivationError`, not an `EphemeralEnvError` variant.
- Create with a package that fails to resolve, where rolling back the
  partially-created directory also fails (e.g. `chmod` the prefix to
  read-only on Unix, or hold an open handle without `FILE_SHARE_DELETE`
  on Windows — a deterministic failure-injection technique, not a
  "locked file," which doesn't reliably block removal on Unix) →
  `create_ephemeral_environment` returns `CreationFailure { error:
  UnresolvablePackage, cleanup_error: Some(TeardownFailed), .. }` — both
  failures surfaced, neither masking the other (FR-010).
- **(Explicit reap, no automatic reaping — see `spec.md`'s rewritten
  User Story 2)** After a successful creation, drop every reference to
  the returned `ReadyEnvironment` → the environment's directory is left
  exactly as it was; nothing removes it (User Story 2, Scenario 1).
- Create one or more ephemeral environments, then call
  `reap_ephemeral_environments()` → every one of them is removed from
  disk, each reported as `ReapOutcome::Removed { id }` (User Story 2,
  Scenario 2; SC-003).
- Create two environments, and force one's removal to fail during a reap
  call (the same failure-injection technique above, applied to one of the
  two environment directories) → that one is reported
  `ReapOutcome::RemovalFailed { id, error }`, distinct from the other's
  `ReapOutcome::Removed { id }`, which is unaffected (User Story 2,
  Scenario 3; FR-009).
- Call `reap_ephemeral_environments()` when no ephemeral environments
  exist → returns `Ok(vec![])`, not an error (User Story 2, Scenario 4;
  SC-007).
- Call `reap_ephemeral_environments()` twice in a row after creating one
  environment → the first call removes it and reports `Removed`; the
  second call, immediately afterward, returns `Ok(vec![])` since nothing
  remains (User Story 2, Scenario 5; SC-007).
- Every `ReapOutcome` — success or failure — emits a `"teardown"`-operation
  `EphemeralLifecycleEvent` carrying that environment's own
  `environment_id` (FR-013/SC-008).

## Manual smoke test (optional, illustrative)

`examples/ephemeral_smoke.rs` exercises the real create → reap flow
end-to-end against the local fixture channel:

```rust
use allez::ephemeral::{
    ChannelConfig, RequestedPackages, create_ephemeral_environment, reap_ephemeral_environments,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channels = ChannelConfig::from_urls(vec!["file:///path/to/fixture-channel".to_string()]);
    let ready = create_ephemeral_environment(
        RequestedPackages::UseDefaultOrOverride,
        channels,
        None,
    )
    .await?;
    println!("environment ready at {}", ready.location.display());
    for pkg in &ready.installed_packages {
        println!("  {} {}", pkg.name, pkg.version);
    }

    // Nothing above tore the environment down automatically -- it stays on
    // disk, usable, until reaped explicitly.
    assert!(ready.location.exists());

    let outcomes = reap_ephemeral_environments()?;
    println!("reaped {} environment(s)", outcomes.len());
    assert!(!ready.location.exists());
    Ok(())
}
```

Run it with:

```sh
ALLEZ_EPHEMERAL_ROOT=/tmp/allez-smoke cargo run --example ephemeral_smoke
```

Expected outcome: prints the resolved prefix path and installed package
list, confirms the directory is still present immediately after creation
(nothing tore it down on its own), then confirms it is gone only after the
explicit `reap_ephemeral_environments()` call.

## Validating owner-only permissions manually (per-platform)

- **Unix**: after creation, `stat -f '%Sp' <location>` (macOS) or
  `stat -c '%A' <location>` (Linux) should show `drwx------`.
- **Windows**: `icacls <location>` should show access granted only to the
  creating user (and, implicitly, whatever administrator access Windows
  itself never lets you fully exclude — see `spec.md` Edge Cases).

## Validating explicit reap manually

1. Create an ephemeral environment (e.g. via `examples/ephemeral_smoke.rs`,
   commenting out or stopping short of its own `reap_ephemeral_environments()`
   call), or let a process creating one exit — normally, or via
   `SIGKILL`/`taskkill /F` mid-install. Confirm the environment's directory
   is still present on disk in either case: nothing about how the owning
   process exited removes it (User Story 2, Scenario 1).
2. Confirm that starting a *new* `create_ephemeral_environment` call for
   the same user/`allez` installation does not touch the leftover
   directory from step 1 either — it is left exactly as it was; this
   feature performs no automatic detection or removal of it at any point.
3. Call `reap_ephemeral_environments()` and confirm the leftover directory
   from step 1 (and any other ephemeral environment present under the
   same root) is now gone, reported as `ReapOutcome::Removed`.
4. Repeat step 3 immediately afterward and confirm it returns an empty
   list — there is nothing left to remove, and this is not an error
   (User Story 2, Scenario 5).
5. To confirm reap performs no liveness check at all: create an
   environment, keep the owning process alive and actively using it
   (e.g. holding a file open inside it), and call
   `reap_ephemeral_environments()` from a separate process or thread —
   confirm it removes the environment anyway, since this feature does not
   attempt to determine whether an environment is still in use before
   removing it. Avoiding this outcome in practice is the reaping caller's
   own responsibility, not a guarantee this feature provides.
