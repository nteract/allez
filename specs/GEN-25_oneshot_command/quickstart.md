# Quickstart: Validating `allez oneshot`

This is a validation/run guide, not an implementation walkthrough — see
`contracts/oneshot_cli_contract.md` for exit codes/output shapes and
`data-model.md` for types. Task-by-task implementation breakdown is a
separate, later artifact (`/speckit.tasks`).

## Prerequisites

- Rust toolchain matching `Cargo.toml`'s `edition = "2024"`.
- GEN-24's checked-in local `file://` fixture channel
  (`tests/fixtures/ephemeral_channel/`) — reused unmodified; no new
  fixture is added by this ticket.
- No real network access required for the default test suite: every new
  test in `tests/oneshot_exec.rs` points `ALLEZ_CONDARC_PATH` at a
  temporary condarc-format file whose `channels:` entry is the checked-in
  fixture channel, never a real remote channel.
- `tests/oneshot_exec.rs` requires the `test-config-override` Cargo
  feature. Unlike this repository's `conformance-tests`/`network-tests`
  (genuinely slow, external-oracle/network-dependent tiers deliberately
  kept out of the default test run), this feature exists solely for the
  `ALLEZ_CONDARC_PATH` security reason (see `research.md` § Test
  strategy) — so it is wired into the *default* test invocation itself,
  not a separate opt-in tier: `Makefile`'s `test` target and
  `.github/workflows/ci.yml`'s `test` (all four platform legs) and
  `coverage` jobs all gain `--features test-config-override` on their
  existing `cargo test`/`cargo llvm-cov` invocations (see `plan.md`'s
  Project Structure). Running `make test`, or CI, therefore includes
  `tests/oneshot_exec.rs` by default; a bare `cargo test --all` invoked
  directly, with no explicit `--features` flag, still compiles and runs
  everything else, only skipping this one `[[test]]` target (the same
  Cargo-level mechanism `conformance-tests` uses, applied to a
  default-on invocation instead of a separate one).
- Each test spawns its own `allez` subprocess via `assert_cmd` and sets
  `ALLEZ_CONDARC_PATH`/`ALLEZ_EPHEMERAL_ROOT` through that one
  invocation's own `Command::env(...)` — not a process-wide
  `std::env::set_var` — so tests run safely in parallel within one
  `cargo test` binary, unlike GEN-24's own in-process integration suite.
  `ALLEZ_CONDARC_PATH` exists specifically because `dirs::home_dir()`
  resolves via a real OS API call on Windows (not by reading `USERPROFILE`
  or any other env var), so a per-invocation `HOME`/`USERPROFILE` override
  would silently have no effect there — `ALLEZ_CONDARC_PATH` sidesteps
  home-directory resolution entirely and behaves identically on every
  target platform.
- `DEFAULT_PACKAGES` will be `["python"]` once this ticket's own change to
  `src/ephemeral/defaults.rs` lands (a documented stopgap pending GEN-30
  — see `research.md`), which does not exist in the local fixture channel.
  Scenario 1.2 below tests the zero-packages *code path*, not that
  `python` itself resolves; full end-to-end proof that the real
  configured default resolves against a live channel is an accepted,
  documented gap in this ticket's own test suite until GEN-30 lands.
- Unix signal-forwarding scenarios require a real signal-capable
  process/OS; they run on the Linux/macOS CI legs only. Windows
  `Ctrl-Break` forwarding needs console-session plumbing this guide does
  not prescribe in detail (see `research.md`'s test-strategy note) — at
  minimum, the Windows-only kill-fallback path (no console session
  needed) must be covered.

## Set up a fixture-pointing test condarc

Reuses the existing `fixture_channel` helper (GEN-24,
`tests/support/ephemeral.rs`, `pub(crate) fn fixture_channel(relative: &str)
-> String`) to get the fixture's own canonical channel string, rather than
hand-building a `file://` URL — no new dependency (e.g. the `url` crate,
which is not in `Cargo.toml`) needed. `tests/support/mod.rs` only exports
`adapter` via a plain `mod support;` — the real, established way to reach
`ephemeral.rs`'s helpers from a sibling integration test file is the same
`#[path = ...]` include `tests/ephemeral_env.rs` already uses:

```rust
#[path = "support/ephemeral.rs"]
mod support;

let condarc = tempfile::NamedTempFile::new().unwrap();
std::fs::write(
    condarc.path(),
    format!("channels: [{}]\n", support::fixture_channel("")),
)
.unwrap();
let root = tempfile::tempdir().unwrap();

let output = assert_cmd::Command::cargo_bin("allez")
    .unwrap()
    .env("ALLEZ_CONDARC_PATH", condarc.path())
    .env("ALLEZ_EPHEMERAL_ROOT", root.path())
    .args(["oneshot", "fixture-default-alpha", "--", "echo", "hello"])
    .output()
    .unwrap();
```

This works identically on every target platform, since
`ALLEZ_CONDARC_PATH` bypasses `dirs::home_dir()`'s own per-platform
resolution entirely rather than trying to override it. The temp file's
own name doesn't need to be `.condarc`/`condarc`-shaped the way conda's
own `CONDARC` env var requires (that convention only applies when
scanning a directory for a recognizably-named file) — `ALLEZ_CONDARC_PATH`
points directly at one file, so its name is irrelevant to how it's read.

## Acceptance-scenario → test mapping (spec.md)

### User Story 1 — build the environment before running the command

| Scenario | Validation |
|---|---|
| 1.1 Packages before `--` are installed before the command starts | Run `oneshot fixture-probe -- fixture-probe` on Unix / `oneshot fixture-probe -- fixture-probe.cmd` on Windows (bare name; `fixture-probe`'s own per-platform build is GEN-24's fixture purpose-built for exactly this activation/PATH-usability assertion — `fixture-default-alpha`/`beta` are dependency-free `noarch` packages with no payload files, so they cannot be used here); assert the program actually runs and exits `0`. |
| 1.2 Zero packages routes through the same code path as an explicit package, without a usage error | Run `oneshot -- echo hi` against the fixture channel; assert exit `1`, category `unresolvable_package` (not `missing_pass_through_command`/exit `2`) — proving `RequestedPackages::UseDefaultOrOverride` was accepted and reached package resolution, not that `python` itself resolves (see Prerequisites' `DEFAULT_PACKAGES` note). |
| 1.3 Two invocations never share an environment | Run `oneshot fixture-default-alpha -- <program printing its own environment's prefix path>` twice; assert the two printed prefix paths differ. |
| 1.4 Multi-arg pass-through, spaces/shell-special chars preserved | Run `oneshot fixture-default-alpha -- echo "hello world" '$HOME'`; assert the child's own stdout contains those exact, unshell-expanded tokens (proving no intermediate shell interprets them). |
| 1.5 Environment's own executable found first via `PATH` | Run `oneshot fixture-probe -- fixture-probe` on Unix / `oneshot fixture-probe -- fixture-probe.cmd` on Windows (bare name, no absolute path given); assert it runs and exits `0`. This proves the environment's own binary is *findable* via `PATH` — proving it's found in *preference* to a same-named host binary (FR-003's literal "not a same-named one that may exist elsewhere on the host") additionally requires a decoy `fixture-probe` planted on the host's own `PATH` that exits with a distinct code, so the test can tell which copy ran; that decoy setup is implementation-time task-breakdown scope, not fully specified here. |

### User Story 2 — real output and real exit code

Every scenario below supplies an explicit, fixture-resolvable package
(`fixture-default-alpha`) before `--`: under `DEFAULT_PACKAGES =
["python"]` (see Prerequisites), a bare `oneshot -- <cmd>` with zero
packages would fail at package resolution before the pass-through program
is ever reached, for any scenario that isn't specifically testing that
failure (that's 1.2's own job, above).

| Scenario | Validation |
|---|---|
| 2.1 Streamed (not buffered), stdout/stderr kept separate | Run `oneshot fixture-default-alpha -- <a small script that writes to stdout, sleeps, writes to stderr, sleeps>` with a timeout shorter than the total sleep; assert the *first* write is already observable before the process exits (proves streaming), and that it landed on the correct stream. |
| 2.2 Exit code propagated exactly | Run `oneshot fixture-default-alpha -- <program using `sh -c 'exit 37'` or platform-equivalent>`; assert `allez`'s own exit code is exactly `37`. |
| 2.3 stdin forwarded unchanged | Pipe known bytes into `allez oneshot fixture-default-alpha -- cat` (or platform equivalent); assert the child's stdout echoes them. |
| 2.4 Interceptable signal forwarded, `allez` doesn't exit early | `#[cfg(unix)]` only: run `oneshot fixture-default-alpha -- <a script that traps SIGTERM and exits 99>`, send `allez`'s own process `SIGTERM`, assert `allez` itself doesn't exit until the child does, and that the final exit code is `99` (not `128+15`, since the child exited normally after its own trap ran). Forwarding targets the direct child PID only (no process-group assertion needed — see `research.md`'s signal-forwarding decision). |
| 2.5 Signal-terminated child reports `128+N` | `#[cfg(unix)]` only: run `oneshot fixture-default-alpha -- <a script that immediately self-signals or is killed externally, e.g. `sh -c 'kill -TERM $$'`>`; assert `allez`'s own exit code is `143` (`128+15`), that stdout/stderr carry no `allez`-authored message or category (FR-013), and that category `pass_through_terminated_by_signal` appears only in the FR-012 tracing event (`RUST_LOG=debug`). |

### User Story 3 — distinct environment-vs-command failure

| Scenario | Validation |
|---|---|
| 3.1 Unresolvable package fails before the command starts | Run `oneshot definitely-nonexistent-package-xyz -- echo hi`; assert exit `1`, category `unresolvable_package`, and that `echo`'s own output (`"hi"`) never appears anywhere in `allez`'s stdout — proving the pass-through command was never started. |
| 3.1a Zero usable channels | Point `ALLEZ_CONDARC_PATH` at a condarc whose `denylist_channels` denies the one channel its own `channels:` list names (verbatim shape of GEN-24's own `deny_filtered_channel_list_returns_no_channels` test, `tests/support/failures.rs`), then run `oneshot fixture-default-alpha -- echo hi`; assert exit `1`, category `no_channels_configured`. |
| 3.1b Integrity verification failure | Run `oneshot fixture-corrupt-checksum -- echo hi` (GEN-24's checked-in fixture package with a deliberately wrong checksum, `tests/fixtures/ephemeral_channel/`); assert exit `1`, category `integrity_verification_failed`. |
| 3.1c Unwritable location | `#[cfg(unix)]` only: `chmod 000` the `ALLEZ_EPHEMERAL_ROOT` directory before invoking `oneshot fixture-default-alpha -- echo hi`; assert exit `1`, category `unwritable_location`. The test itself must restore the directory to a removable mode (e.g. `chmod 700`) before its own temp-directory guard drops, in a way that also runs on assertion failure/panic (e.g. an RAII guard, not a plain post-assertion statement) — `tempfile::TempDir`'s own `Drop` silently ignores removal errors, so a `000`-mode directory left behind by a failing assertion would otherwise leak on disk. Windows coverage of this specific category is an accepted gap in this ticket's own test suite (no portable, deterministic way to make a directory unwritable to its own owner across this project's four target platforms) — `EphemeralEnvError::UnwritableLocation` itself is already GEN-24's own scope and GEN-24's own test suite covers its Windows path; this ticket only needs to prove `oneshot` propagates whichever category GEN-24 already produces correctly, which 3.1/3.1a/3.1b/3.2 already establish for the other three. |
| 3.2 Program not found inside the new environment | Run `oneshot fixture-default-alpha -- definitely-nonexistent-binary-xyz`; assert exit `127`, category `pass_through_not_found` — distinct from 3.1's `1`/`unresolvable_package`. |
| 3.2a Program found but not executable | `#[cfg(unix)]` only: write a file with no execute permission (`fs::Permissions::from_mode(0o644)`) into a temp directory, run `oneshot fixture-default-alpha -- <that file's absolute path>`; assert exit `126`, category `pass_through_not_executable`. Windows has no equivalent "found but not executable" permission-bit concept this suite can portably construct — accepted as a Unix-only scenario, consistent with 2.4/2.5/3.1c's own platform scoping. |
| 3.3 Dual failure (creation *and* its own cleanup both fail) | Requires provoking both a creation failure and a rollback failure in the same attempt (e.g. via a filesystem permission fixture) — exact mechanics are implementation-time task-breakdown scope; assert both `category`/`cleanup_category` are present and distinct in the JSON body. |
| 3.4 Empty `--` separator | Already covered by existing `tests/cli_scaffold.rs` (`t034`); unchanged by this ticket — re-run, don't re-write. |
| 3.5 No `--` separator at all, distinct code path from 3.4 | `cli::validate_pass_through` rejects both "no `--` at all" and "`--` present with nothing after it" with the same `MissingPassThroughCommand`/exit `2`/`missing_pass_through_command` outcome, but clap's own parsed state differs between the two (confirmed empirically against clap 4.6.x per `src/cli/mod.rs`'s own doc comments) — task-breakdown scope should confirm `tests/cli_scaffold.rs` already asserts the literal-no-`--`-at-all case for `oneshot` specifically (e.g. `oneshot pkg1` with no `--` token anywhere in argv) and add it if it doesn't, rather than assuming 3.4's empty-separator case implies this one. |

### Cross-cutting scenarios (FR-003, FR-009, FR-015)

| Scenario | Validation |
|---|---|
| C.1 Inherited environment variables reach the pass-through command (FR-003/SC-007) | Set an arbitrary, `allez`-unrelated marker variable (e.g. `MY_TEST_MARKER=xyz`) on the test's own `assert_cmd::Command` alongside `ALLEZ_CONDARC_PATH`/`ALLEZ_EPHEMERAL_ROOT`, run `oneshot fixture-default-alpha -- <program that prints that variable>`, assert its value reaches the child unchanged, proving activation's own overlay merges with (never replaces) the inherited environment. |
| C.2 Environment persists after every invocation, regardless of outcome (FR-009/SC-006) | After each of: a successful run (1.1), a normal non-zero pass-through exit (2.2), and a signal-terminated pass-through (2.5, `#[cfg(unix)]`) — inspect the test's own known `ALLEZ_EPHEMERAL_ROOT` directory directly from the test process (not via `allez`'s own output, which FR-015 forbids disclosing this through) and assert the created environment's files are still present in each case. |
| C.3 No caller-facing disclosure of the environment's location/identifier (FR-015/SC-010) | For both a successful run where the environment was actually created (1.1, or 3.2's "program not found" — creation succeeds, only the pass-through fails to start) and a pre-start environment-creation failure (3.1, where rollback removes the partial directory but the attempt still had an `EnvironmentId`), assert `allez`'s own stdout/stderr never contains the `ALLEZ_EPHEMERAL_ROOT` path string or any substring recognizable as the environment's own ULID (obtained independently from the test's own `ALLEZ_EPHEMERAL_ROOT` directory listing for the success case, never parsed out of `allez`'s own output). |

## Run the full test suite

```sh
make test
```

Equivalent to `cargo test --all --features test-config-override` (see
`plan.md`'s `Makefile` entry). `test-config-override` is a new, non-default
Cargo feature that this ticket wires into `Makefile`'s `test` target and
`.github/workflows/ci.yml`'s `test`/`coverage` jobs directly, rather than
creating a separate opt-in tier the way `conformance-tests`/`network-tests`
are — see Prerequisites and `research.md` § Test strategy for why. Running
`cargo test --all` directly, with no explicit `--features` flag, still
compiles and runs everything except `tests/oneshot_exec.rs`. The fixture
channel itself is local/`file://`, matching GEN-24's default-suite
convention — no real network access is needed either way.
