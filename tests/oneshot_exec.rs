//! End-to-end `allez oneshot` tests against the real compiled binary,
//! pointed at GEN-24's checked-in local fixture channel
//! (`tests/fixtures/ephemeral_channel/`) via the feature-gated
//! `ALLEZ_CONDARC_PATH`/`ALLEZ_EPHEMERAL_ROOT` environment variables.
//! Requires `--features test-config-override` (`make test` sets this by
//! default; see `Cargo.toml`).
//!
//! Test IDs below reference their exact quickstart.md acceptance-scenario
//! ID (e.g. "1.1", "2.4", "3.1a", "C.2") for traceability.

// `support/ephemeral.rs` is a shared module also included whole by
// `tests/ephemeral_env.rs` (which uses every item in it); this binary
// only needs `fixture_channel`, so the rest is expected dead code here
// rather than a real problem to fix in either binary.
#[allow(dead_code)]
#[path = "support/ephemeral.rs"]
mod support;

/// The parent directory of the fixture channel, as a channel base URL, so a
/// test can declare that channel through `custom_channels`, whose own
/// semantics are `{name: BASE_URL}` yielding `BASE_URL/name`.
fn fixture_channel_base() -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    rattler_conda_types::Channel::try_from_directory(&path)
        .unwrap()
        .canonical_name()
}

use std::io::BufRead;
use std::time::Duration;

use assert_cmd::Command;

/// Fixture-pointing test condarc plus an isolated ephemeral root, both set
/// via per-invocation `Command::env(...)` (quickstart.md's "Set up a
/// fixture-pointing test condarc") — never a process-wide
/// `std::env::set_var`, so tests run safely in parallel.
///
/// `root` is a real, already-created directory (not merely a path under
/// its own enclosing temp directory): GEN-24's own root-reuse
/// verification (`ephemeral::paths::verified_root`) rejects a *reused*
/// root directory unless it is already owner-only (`0700`) —
/// `tempfile::tempdir()`'s own directory is created under this system's
/// umask (`0755` here), so it must be narrowed explicitly rather than
/// passed to `allez` as-is.
struct OneshotHarness {
    condarc: tempfile::NamedTempFile,
    _outer_dir: tempfile::TempDir,
    root: std::path::PathBuf,
    working_directory: Option<std::path::PathBuf>,
}

impl OneshotHarness {
    fn new() -> Self {
        Self::with_condarc_contents(&format!(
            "channels: [\"{}\"]\n",
            support::fixture_channel("")
        ))
    }

    fn with_condarc_contents(contents: &str) -> Self {
        let condarc = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(condarc.path(), contents).unwrap();
        let _outer_dir = tempfile::tempdir().unwrap();
        let root = _outer_dir.path().join("root");
        #[cfg(unix)]
        {
            std::fs::create_dir(&root).unwrap();
            std::fs::set_permissions(&root, std::os::unix::fs::PermissionsExt::from_mode(0o700))
                .unwrap();
        }
        // `std::fs::create_dir` here would leave `root` with its parent
        // temp directory's default (non-owner-only) ACL, which
        // `ephemeral::paths::verified_root`'s own Windows root-reuse check
        // then rejects as unwritable -- so this creates it via the same
        // owner-only-ACL routine `verified_root` itself uses instead (see
        // `Cargo.toml`'s `test-config-override` feature comment).
        #[cfg(windows)]
        allez::ephemeral::test_create_owner_only_directory(&root).unwrap();
        Self {
            condarc,
            _outer_dir,
            root,
            working_directory: None,
        }
    }

    /// Runs every subsequent invocation with `directory` as its own
    /// working directory, so a project-local `.condarc` planted there is
    /// discoverable by the subprocess (and must still be ignored).
    fn in_directory(mut self, directory: std::path::PathBuf) -> Self {
        self.working_directory = Some(directory);
        self
    }

    fn root_path(&self) -> &std::path::Path {
        &self.root
    }

    /// The name of every package installed into the one environment this
    /// harness's root holds, read from that environment's own
    /// `conda-meta` records.
    fn installed_package_names(&self) -> std::collections::BTreeSet<String> {
        let mut environments = std::fs::read_dir(self.root.join("envs"))
            .expect("the ephemeral root should hold an envs directory")
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(
            environments.len(),
            1,
            "expected exactly one created environment, found {environments:?}"
        );
        let conda_meta = environments.remove(0).join("conda-meta");
        let records = std::fs::read_dir(&conda_meta).unwrap_or_else(|error| {
            panic!(
                "a created environment must hold a readable conda-meta directory at \
                 {conda_meta:?}, found {error}"
            )
        });
        records
            .map(|entry| {
                entry
                    .expect("every conda-meta entry should be readable")
                    .path()
            })
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .map(|path| {
                let contents = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("reading {path:?} failed: {error}"));
                let record: serde_json::Value = serde_json::from_str(&contents)
                    .unwrap_or_else(|error| panic!("parsing {path:?} failed: {error}"));
                record["name"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{path:?} carries no `name` field"))
                    .to_string()
            })
            .collect()
    }

    fn command(&self) -> Command {
        let mut command = Command::cargo_bin("allez").unwrap();
        command
            .env("ALLEZ_CONDARC_PATH", self.condarc.path())
            .env("ALLEZ_EPHEMERAL_ROOT", &self.root);
        if let Some(directory) = &self.working_directory {
            command.current_dir(directory);
        }
        command
    }

    fn raw_command(&self) -> std::process::Command {
        let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin("allez"));
        command
            .env("ALLEZ_CONDARC_PATH", self.condarc.path())
            .env("ALLEZ_EPHEMERAL_ROOT", &self.root);
        if let Some(directory) = &self.working_directory {
            command.current_dir(directory);
        }
        command
    }

    /// Runs `oneshot` with `args` (everything after `oneshot`, including
    /// `--` and the pass-through command); returns `(exit_code, stdout,
    /// stderr)`.
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let mut command = self.command();
        command.arg("oneshot").args(args);
        self.capture(command)
    }

    /// Runs with `--human`, selecting human-readable rendering instead of
    /// the default JSON envelope.
    fn run_human(&self, args: &[&str]) -> (i32, String, String) {
        let mut command = self.command();
        command.arg("--human").arg("oneshot").args(args);
        self.capture(command)
    }

    fn capture(&self, mut command: Command) -> (i32, String, String) {
        let output = command.output().unwrap();
        let code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        (code, stdout, stderr)
    }
}

fn parse_stderr_json(stderr: &str) -> serde_json::Value {
    serde_json::from_str(stderr).unwrap_or_else(|error| {
        panic!("stderr should be exactly one JSON object: {error}\nstderr={stderr}")
    })
}

// ---------------------------------------------------------------------
// User Story 1 — build the environment before running the command
// ---------------------------------------------------------------------

/// Scenario 1.1 + 1.5: bare-name lookup finds the environment's own
/// `fixture-probe` via `PATH` before the pass-through program starts.
#[test]
fn scenario_1_1_packages_installed_before_command_starts() {
    let harness = OneshotHarness::new();
    #[cfg(unix)]
    let probe = "fixture-probe";
    #[cfg(windows)]
    let probe = "fixture-probe.cmd";

    let (code, _stdout, stderr) = harness.run(&["fixture-probe", "--", probe]);
    assert_eq!(code, 0, "stderr={stderr}");
}

/// T049a (SC-009): exactly one `OneshotOutcomeEvent`-shaped tracing
/// record is emitted per invocation, carrying a non-empty
/// `schema_version` field, for (a) an environment-creation failure and
/// (b) a normal successful pass-through exit.
#[test]
fn scenario_c_4_exactly_one_schema_versioned_tracing_record_on_creation_failure() {
    let harness = OneshotHarness::new();
    let output = harness
        .command()
        .env("RUST_LOG", "debug")
        .args([
            "oneshot",
            "definitely-nonexistent-package-xyz",
            "--",
            "echo",
            "hi",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let record_count = stderr.matches("oneshot pass-through outcome").count();
    assert_eq!(record_count, 1, "stderr={stderr}");
    assert!(stderr.contains("schema_version"), "stderr={stderr}");
    assert!(
        !stderr.contains("\"schema_version\":\"\""),
        "stderr={stderr}"
    );
}

#[test]
fn scenario_c_4_exactly_one_schema_versioned_tracing_record_on_success() {
    let harness = OneshotHarness::new();
    let output = harness
        .command()
        .env("RUST_LOG", "debug")
        .args(["oneshot", "fixture-default-alpha", "--", "echo", "hi"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let record_count = stderr.matches("oneshot pass-through outcome").count();
    assert_eq!(record_count, 1, "stderr={stderr}");
    assert!(stderr.contains("schema_version"), "stderr={stderr}");
    assert!(
        !stderr.contains("\"schema_version\":\"\""),
        "stderr={stderr}"
    );
}

/// Scenario GEN-30 1.1 (US1-AS1/FR-001): with no per-invocation
/// packages, the environment gets exactly the caller's own `.condarc`
/// `create_default_packages` list.
#[test]
fn scenario_gen30_1_1_default_packages_from_condarc_with_no_overrides() {
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "channels: [\"{}\"]\ncreate_default_packages: [fixture-default-alpha]\n",
        support::fixture_channel("")
    ));

    let (code, _stdout, stderr) = harness.run(&["--", "echo", "hi"]);

    assert_eq!(code, 0, "stderr={stderr}");
    assert_eq!(
        harness.installed_package_names(),
        std::collections::BTreeSet::from(["fixture-default-alpha".to_string()])
    );
}

/// Scenario GEN-30 1.2 (US1-AS2/FR-002): no `create_default_packages`
/// key at all plus no per-invocation packages succeeds with zero
/// installed packages — there is no `allez`-authored fallback list.
#[test]
fn scenario_gen30_1_2_no_default_packages_configured_creates_empty_environment() {
    let harness = OneshotHarness::new();

    let (code, _stdout, stderr) = harness.run(&["--", "echo", "hi"]);

    assert_eq!(code, 0, "stderr={stderr}");
    let installed = harness.installed_package_names();
    assert!(
        installed.is_empty(),
        "expected an empty installed set, found {installed:?}"
    );
}

/// Scenario GEN-30 project-local config (spec.md Edge Case): a `.condarc`
/// in the invocation's own working directory is never consulted — only
/// the invoking user's own file is.
#[test]
fn scenario_gen30_ignores_project_local_condarc() {
    let project_directory = tempfile::tempdir().unwrap();
    std::fs::write(
        project_directory.path().join(".condarc"),
        format!(
            "channels: [\"{}\"]\ncreate_default_packages: [fixture-default-beta]\n",
            support::fixture_channel("")
        ),
    )
    .unwrap();
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "channels: [\"{}\"]\ncreate_default_packages: [fixture-default-alpha]\n",
        support::fixture_channel("")
    ))
    .in_directory(project_directory.path().to_path_buf());

    let (code, _stdout, stderr) = harness.run(&["--", "echo", "hi"]);

    assert_eq!(code, 0, "stderr={stderr}");
    assert_eq!(
        harness.installed_package_names(),
        std::collections::BTreeSet::from(["fixture-default-alpha".to_string()])
    );
}

/// Scenario GEN-30 2.1 (US2-AS1/FR-003): a per-invocation package
/// sharing no bare name with the resolved default set is added to it.
#[test]
fn scenario_gen30_2_1_per_invocation_package_adds_to_default_set() {
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "channels: [\"{}\"]\ncreate_default_packages: [fixture-default-alpha]\n",
        support::fixture_channel("")
    ));
    #[cfg(unix)]
    let probe = "fixture-probe";
    #[cfg(windows)]
    let probe = "fixture-probe.cmd";

    let (code, _stdout, stderr) = harness.run(&["fixture-probe", "--", probe]);

    assert_eq!(code, 0, "stderr={stderr}");
    assert_eq!(
        harness.installed_package_names(),
        std::collections::BTreeSet::from([
            "fixture-default-alpha".to_string(),
            "fixture-probe".to_string(),
        ])
    );
}

/// Scenario GEN-30 2.2 (US2-AS2/FR-004): a per-invocation package whose
/// bare name matches a default entry supersedes that entry. The default
/// entry pins a version absent from the fixture channel, so the solve
/// could only succeed if supersede actually dropped it.
#[test]
fn scenario_gen30_2_2_per_invocation_package_supersedes_matching_default_entry() {
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "channels: [\"{}\"]\ncreate_default_packages: [fixture-default-alpha=9.9.9]\n",
        support::fixture_channel("")
    ));

    let (code, _stdout, stderr) = harness.run(&["fixture-default-alpha", "--", "echo", "hi"]);

    assert_eq!(code, 0, "stderr={stderr}");
    assert_eq!(
        harness.installed_package_names(),
        std::collections::BTreeSet::from(["fixture-default-alpha".to_string()])
    );
}

/// Scenario GEN-30 malformed default entry: `create_default_packages` is
/// never validated on the way in (FR-002), so a syntactically invalid
/// entry surfaces at solve time, and must name `create_default_packages`
/// as its source, since nothing on the command line produced it.
#[test]
fn scenario_gen30_malformed_default_entry_reports_its_condarc_source() {
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "channels: [\"{}\"]\ncreate_default_packages: [\"[[[not a spec\"]\n",
        support::fixture_channel("")
    ));

    let (code, stdout, stderr) = harness.run(&["--", "echo", "hi"]);

    assert_eq!(code, 1, "stderr={stderr}");
    let body = parse_stderr_json(&stderr);
    assert_eq!(body["category"], "unresolvable_package");
    assert_eq!(
        body["message"],
        "could not resolve package `<create_default_packages entry 1>` from `.condarc` \
         `create_default_packages`"
    );
    assert!(
        !stdout.contains("hi"),
        "pass-through must never have started: {stdout}"
    );
}

/// Scenario GEN-30 channel-qualified default (FR-002): a
/// `create_default_packages` entry qualified by a channel the caller
/// declared by name resolves against that channel, matching conda's own
/// `<channel>::<package>` semantics.
#[test]
fn scenario_gen30_channel_qualified_default_resolves_against_its_configured_channel() {
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "custom_channels:\n  ephemeral_channel: {}\nchannels: [ephemeral_channel]\n\
         create_default_packages: [ephemeral_channel::fixture-default-alpha]\n",
        fixture_channel_base()
    ));

    let (code, _stdout, stderr) = harness.run(&["--", "echo", "hi"]);

    assert_eq!(code, 0, "stderr={stderr}");
    assert_eq!(
        harness.installed_package_names(),
        std::collections::BTreeSet::from(["fixture-default-alpha".to_string()])
    );
}

/// Scenario GEN-30 multi-entry default failure: with more than one
/// unresolvable entry there is no single culprit to name, but the failure
/// must still point at `create_default_packages` rather than the command
/// line, which named nothing.
#[test]
fn scenario_gen30_multiple_default_entries_still_report_their_condarc_source() {
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "channels: [\"{}\"]\ncreate_default_packages: [fixture-default-alpha, fixture-nope]\n",
        support::fixture_channel("")
    ));

    let (code, stdout, stderr) = harness.run(&["--", "echo", "hi"]);

    assert_eq!(code, 1, "stderr={stderr}");
    let body = parse_stderr_json(&stderr);
    assert_eq!(body["category"], "unresolvable_package");
    assert_eq!(
        body["message"],
        "could not resolve packages from `.condarc` `create_default_packages`"
    );
    assert!(
        !stdout.contains("hi"),
        "pass-through must never have started: {stdout}"
    );
}

/// Scenario GEN-30 human rendering: `--human` carries the same
/// `.condarc` attribution as the JSON envelope, without a JSON body.
#[test]
fn scenario_gen30_human_rendering_reports_the_condarc_source() {
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "channels: [\"{}\"]\ncreate_default_packages: [\"[[[not a spec\"]\n",
        support::fixture_channel("")
    ));

    let (code, _stdout, stderr) = harness.run_human(&["--", "echo", "hi"]);

    assert_eq!(code, 1, "stderr={stderr}");
    assert_eq!(
        stderr.trim(),
        "error: could not resolve package `<create_default_packages entry 1>` from `.condarc` \
         `create_default_packages`"
    );
    assert!(
        !stderr.contains('{'),
        "human output must carry no JSON envelope: {stderr}"
    );
}

/// Scenario GEN-30 multichannel qualifier: a name designating several
/// channels cannot be expressed as one match spec's channel, so it fails
/// loudly rather than silently binding to one arbitrary member. This is a
/// known, deliberate divergence from conda, which resolves it.
#[test]
fn scenario_gen30_multichannel_qualified_default_fails_rather_than_guessing() {
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "custom_multichannels:\n  bundle:\n    - {}\n    - {}\nchannels: [bundle]\n\
         create_default_packages: [bundle::fixture-priority]\n",
        support::fixture_channel("priority-a"),
        support::fixture_channel("priority-b")
    ));

    let (code, _stdout, stderr) = harness.run(&["--", "echo", "hi"]);

    assert_eq!(code, 1, "stderr={stderr}");
    let body = parse_stderr_json(&stderr);
    assert_eq!(body["category"], "unresolvable_package");
}

/// Scenario GEN-30 qualified per-invocation package: the same qualifier
/// resolution applies to a package named on the command line, not only to
/// a `create_default_packages` entry.
#[test]
fn scenario_gen30_channel_qualified_explicit_package_resolves() {
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "custom_channels:\n  ephemeral_channel: {}\nchannels: [ephemeral_channel]\n",
        fixture_channel_base()
    ));

    let (code, _stdout, stderr) = harness.run(&[
        "ephemeral_channel::fixture-default-alpha",
        "--",
        "echo",
        "hi",
    ]);

    assert_eq!(code, 0, "stderr={stderr}");
    assert_eq!(
        harness.installed_package_names(),
        std::collections::BTreeSet::from(["fixture-default-alpha".to_string()])
    );
}

/// Scenario GEN-30 human rendering of the request-level default failure:
/// the plural message carries the same `.condarc` attribution.
#[test]
fn scenario_gen30_human_rendering_reports_the_plural_condarc_source() {
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "channels: [\"{}\"]\ncreate_default_packages: [fixture-default-alpha, fixture-nope]\n",
        support::fixture_channel("")
    ));

    let (code, _stdout, stderr) = harness.run_human(&["--", "echo", "hi"]);

    assert_eq!(code, 1, "stderr={stderr}");
    assert_eq!(
        stderr.trim(),
        "error: could not resolve packages from `.condarc` `create_default_packages`"
    );
}

/// Scenario GEN-30 denylisted qualifier: a channel removed by policy leaves
/// no name behind, so its name cannot be used to reach it again.
#[test]
fn scenario_gen30_a_denylisted_channels_name_cannot_be_used_as_a_qualifier() {
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "custom_channels:\n  ephemeral_channel: {base}\nchannels: [ephemeral_channel]\n\
         denylist_channels: [ephemeral_channel]\n\
         create_default_packages: [ephemeral_channel::fixture-default-alpha]\n",
        base = fixture_channel_base()
    ));

    let (code, _stdout, stderr) = harness.run(&["--", "echo", "hi"]);

    assert_eq!(code, 1, "stderr={stderr}");
    let body = parse_stderr_json(&stderr);
    assert_eq!(body["category"], "no_channels_configured");
}

/// Scenario GEN-30 entry position: a failing `create_default_packages` entry
/// is identified by its own one-based position, not the first entry's.
#[test]
fn scenario_gen30_reports_the_position_of_the_failing_default_entry() {
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "channels: [\"{}\"]\ncreate_default_packages: [fixture-default-alpha, \"[[[not a spec\"]\n",
        support::fixture_channel("")
    ));

    let (code, _stdout, stderr) = harness.run(&["--", "echo", "hi"]);

    assert_eq!(code, 1, "stderr={stderr}");
    let body = parse_stderr_json(&stderr);
    assert_eq!(
        body["message"],
        "could not resolve package `<create_default_packages entry 2>` from `.condarc` \
         `create_default_packages`"
    );
}

/// Scenario GEN-30 command-line position: an invalid package argument is
/// identified by its own one-based position, and its text is not echoed.
#[test]
fn scenario_gen30_reports_the_position_of_an_invalid_command_line_package() {
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "channels: [\"{}\"]\n",
        support::fixture_channel("")
    ));

    let (code, _stdout, stderr) =
        harness.run(&["fixture-probe", "//user:hunter2@host/x", "--", "echo", "hi"]);

    assert_eq!(code, 1, "stderr={stderr}");
    let body = parse_stderr_json(&stderr);
    assert_eq!(
        body["message"],
        "could not resolve package `<command-line package 2>`"
    );
    assert!(!stderr.contains("hunter2"), "stderr={stderr}");
}

/// Scenario GEN-30 tracing: the lifecycle event's `packages` field carries
/// safe labels, so a credential-bearing default cannot reach a trace.
#[test]
fn scenario_gen30_lifecycle_event_packages_carry_safe_labels() {
    // Given: a `create_default_packages` entry that both embeds a
    // credential and fails to resolve, so a lifecycle failure event is
    // guaranteed to fire with this package in its `packages` field --
    // an earlier version of this test used a *resolvable* entry with no
    // embedded credential, which could pass vacuously without proving any
    // event carrying that field was ever actually emitted.
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "channels: [\"{}\"]\ncreate_default_packages: [\"fixture-nope?access_token=TOPSECRET123\"]\n",
        support::fixture_channel("")
    ));

    let output = harness
        .command()
        .env("RUST_LOG", "debug")
        .args(["oneshot", "--", "echo", "hi"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1), "output={output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The final rendered error message also names the safe label, so
    // checking for that substring anywhere in `stderr` would pass even if
    // no lifecycle event ever carried it -- require a line that is
    // specifically a lifecycle event (identified by its own `message`
    // field) to also carry the safe label.
    assert!(
        stderr
            .lines()
            .any(|line| line.contains("ephemeral environment lifecycle")
                && line.contains("create_default_packages entry 1")),
        "a lifecycle event's own line must carry the safe label: {stderr}"
    );
    assert!(
        !stderr.contains("TOPSECRET123"),
        "the embedded credential must never reach a trace: {stderr}"
    );
}

/// Scenario 1.3: two invocations never share an environment.
#[test]
fn scenario_1_3_two_invocations_never_share_an_environment() {
    let harness = OneshotHarness::new();
    #[cfg(unix)]
    let printer: &[&str] = &["sh", "-c", "echo $CONDA_PREFIX"];
    #[cfg(windows)]
    let printer: &[&str] = &["cmd", "/C", "echo %CONDA_PREFIX%"];

    let mut args = vec!["fixture-default-alpha", "--"];
    args.extend_from_slice(printer);
    let (code1, stdout1, stderr1) = harness.run(&args);
    let (code2, stdout2, stderr2) = harness.run(&args);
    assert_eq!(code1, 0, "stderr={stderr1}");
    assert_eq!(code2, 0, "stderr={stderr2}");
    assert!(!stdout1.trim().is_empty());
    assert_ne!(stdout1.trim(), stdout2.trim());
}

/// Scenario 1.4: multi-argument pass-through preserves spaces/shell
/// special characters unshell-expanded (no intermediate shell).
#[test]
#[cfg(unix)]
fn scenario_1_4_pass_through_arguments_preserved_unshell_expanded() {
    let harness = OneshotHarness::new();
    let (code, stdout, stderr) = harness.run(&[
        "fixture-default-alpha",
        "--",
        "echo",
        "hello world",
        "$HOME",
    ]);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("hello world"));
    assert!(stdout.contains("$HOME"));
}

/// Scenario 1.5: the environment's own `fixture-probe` runs in preference
/// to a same-named decoy planted earlier on the host's own `PATH`.
#[test]
#[cfg(unix)]
fn scenario_1_5_environment_executable_preferred_over_host_path_decoy() {
    let harness = OneshotHarness::new();
    let decoy_dir = tempfile::tempdir().unwrap();
    let decoy_path = decoy_dir.path().join("fixture-probe");
    std::fs::write(&decoy_path, "#!/bin/sh\nexit 7\n").unwrap();
    let mut permissions = std::fs::metadata(&decoy_path).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&decoy_path, permissions).unwrap();

    let existing_path = std::env::var("PATH").unwrap_or_default();
    let combined_path = format!("{}:{existing_path}", decoy_dir.path().display());

    let output = harness
        .command()
        .env("PATH", combined_path)
        .args(["oneshot", "fixture-probe", "--", "fixture-probe"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "the environment's own fixture-probe should run, not the host decoy"
    );
}

// ---------------------------------------------------------------------
// User Story 2 — real output and real exit code
// ---------------------------------------------------------------------

/// Scenario 2.1: stdout/stderr are streamed (not buffered) and kept
/// separate.
#[test]
#[cfg(unix)]
fn scenario_2_1_streamed_output_visible_before_process_exits() {
    let harness = OneshotHarness::new();
    let script = "printf 'first\\n'; sleep 2; printf 'second\\n' 1>&2; sleep 2";
    let mut command = harness.raw_command();
    command
        .args(["oneshot", "fixture-default-alpha", "--", "sh", "-c", script])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = command.spawn().unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let start = std::time::Instant::now();
    let mut first_line = String::new();
    stdout.read_line(&mut first_line).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(first_line.trim(), "first");
    assert!(
        elapsed < Duration::from_secs(2),
        "first stdout line took {elapsed:?} to appear; output does not look streamed"
    );
    let _ = child.wait();
}

/// Scenario 2.2: `allez`'s own exit code matches the pass-through
/// program's exactly.
#[test]
#[cfg(unix)]
fn scenario_2_2_exit_code_propagated_exactly() {
    let harness = OneshotHarness::new();
    let (code, _stdout, stderr) =
        harness.run(&["fixture-default-alpha", "--", "sh", "-c", "exit 37"]);
    assert_eq!(code, 37, "stderr={stderr}");
}

/// Scenario 2.3: stdin is forwarded to the pass-through program unchanged.
#[test]
#[cfg(unix)]
fn scenario_2_3_stdin_forwarded_unchanged() {
    let harness = OneshotHarness::new();
    let output = harness
        .command()
        .args(["oneshot", "fixture-default-alpha", "--", "cat"])
        .write_stdin("hello stdin\n")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello stdin"
    );
}

/// Scenario 2.4 (`#[cfg(unix)]` only): an intercepted `SIGTERM` is
/// forwarded to the direct child; `allez` doesn't exit until the child
/// does, and the final exit code is the child's own (`99`), not `128+15`.
#[test]
#[cfg(unix)]
fn scenario_2_4_interceptable_signal_forwarded_allez_waits_for_child() {
    let harness = OneshotHarness::new();
    let script = "trap 'exit 99' TERM; sleep 5 & wait";
    let mut command = harness.raw_command();
    command.args(["oneshot", "fixture-default-alpha", "--", "sh", "-c", script]);
    let mut child = command.spawn().unwrap();

    // A generous delay: environment creation (package resolve + install)
    // must fully finish and `run_pass_through` must register its signal
    // listeners *before* this sends `SIGTERM` — sending it any earlier
    // would hit `allez`'s own default (uncaught) signal disposition
    // instead of the intercepted one this scenario means to exercise.
    std::thread::sleep(Duration::from_secs(2));
    let status = std::process::Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success(), "failed to send SIGTERM to allez");

    let start = std::time::Instant::now();
    let status = child.wait().unwrap();
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "allez should exit soon after the child's own trap runs, not wait for the full sleep"
    );
    assert_eq!(status.code(), Some(99));
}

/// Scenario 2.5 (`#[cfg(unix)]` only): a signal-terminated child reports
/// `128+N`, with no `allez`-authored message/category on stdout/stderr.
#[test]
#[cfg(unix)]
fn scenario_2_5_signal_terminated_child_reports_128_plus_signal_with_no_envelope() {
    let harness = OneshotHarness::new();
    let (code, stdout, stderr) =
        harness.run(&["fixture-default-alpha", "--", "sh", "-c", "kill -TERM $$"]);
    assert_eq!(code, 143);
    assert!(stdout.is_empty(), "stdout={stdout}");
    assert!(stderr.is_empty(), "stderr={stderr}");
}

/// Scenario 2.5's own FR-012 half: `pass_through_terminated_by_signal`
/// appears only via the `RUST_LOG=debug` tracing channel, never stdout.
#[test]
#[cfg(unix)]
fn scenario_2_5_terminated_by_signal_category_appears_only_via_rust_log() {
    let harness = OneshotHarness::new();
    let output = harness
        .command()
        .env("RUST_LOG", "debug")
        .args([
            "oneshot",
            "fixture-default-alpha",
            "--",
            "sh",
            "-c",
            "kill -TERM $$",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(143));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pass_through_terminated_by_signal"),
        "stderr={stderr}"
    );
}

// ---------------------------------------------------------------------
// User Story 3 — distinct environment-vs-command failure
// ---------------------------------------------------------------------

/// Scenario 3.1: an unresolvable package fails before the command starts.
#[test]
fn scenario_3_1_unresolvable_package_fails_before_command_starts() {
    let harness = OneshotHarness::new();
    let (code, stdout, stderr) =
        harness.run(&["definitely-nonexistent-package-xyz", "--", "echo", "hi"]);
    assert_eq!(code, 1, "stderr={stderr}");
    let body = parse_stderr_json(&stderr);
    assert_eq!(body["category"], "unresolvable_package");
    assert!(!stdout.contains("hi"));
}

/// Scenario 3.1a: a denylist-filtered channel list resolving to zero
/// usable channels.
#[test]
fn scenario_3_1a_zero_usable_channels() {
    let channel = support::fixture_channel("");
    let harness = OneshotHarness::with_condarc_contents(&format!(
        "channels: [\"{channel}\"]\ndenylist_channels: [\"{channel}\"]\n"
    ));
    let (code, _stdout, stderr) = harness.run(&["fixture-default-alpha", "--", "echo", "hi"]);
    assert_eq!(code, 1, "stderr={stderr}");
    let body = parse_stderr_json(&stderr);
    assert_eq!(body["category"], "no_channels_configured");
}

/// Scenario 3.1b: an integrity-verification failure against the checked-in
/// deliberately-corrupt fixture package.
#[test]
fn scenario_3_1b_integrity_verification_failure() {
    let harness = OneshotHarness::new();
    let (code, _stdout, stderr) = harness.run(&["fixture-corrupt-checksum", "--", "echo", "hi"]);
    assert_eq!(code, 1, "stderr={stderr}");
    let body = parse_stderr_json(&stderr);
    assert_eq!(body["category"], "integrity_verification_failed");
}

/// RAII guard restoring a chmod-000 directory to a removable mode even on
/// assertion failure/panic — `tempfile::TempDir`'s own `Drop` silently
/// ignores removal errors, so a `000`-mode directory left behind by a
/// failing assertion would otherwise leak on disk.
#[cfg(unix)]
struct RestorePermissionsGuard<'a> {
    path: &'a std::path::Path,
}

#[cfg(unix)]
impl Drop for RestorePermissionsGuard<'_> {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(
            self.path,
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        );
    }
}

/// Scenario 3.1c (`#[cfg(unix)]` only): an unwritable `ALLEZ_EPHEMERAL_ROOT`.
#[test]
#[cfg(unix)]
fn scenario_3_1c_unwritable_location() {
    let harness = OneshotHarness::new();
    std::fs::set_permissions(
        harness.root_path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o000),
    )
    .unwrap();
    let _guard = RestorePermissionsGuard {
        path: harness.root_path(),
    };

    let (code, _stdout, stderr) = harness.run(&["fixture-default-alpha", "--", "echo", "hi"]);
    assert_eq!(code, 1, "stderr={stderr}");
    let body = parse_stderr_json(&stderr);
    assert_eq!(body["category"], "unwritable_location");
}

/// Scenario 3.2: the pass-through program's name could not be found
/// inside the new environment — distinct from 3.1's `1`/`unresolvable_package`.
#[test]
fn scenario_3_2_pass_through_program_not_found() {
    let harness = OneshotHarness::new();
    let (code, stdout, stderr) = harness.run(&[
        "fixture-default-alpha",
        "--",
        "definitely-nonexistent-binary-xyz",
    ]);
    assert_eq!(code, 127, "stderr={stderr}");
    let body = parse_stderr_json(&stderr);
    assert_eq!(body["category"], "pass_through_not_found");
    assert!(stdout.is_empty());
}

/// Scenario 3.2a (`#[cfg(unix)]` only): the pass-through program is found
/// but has no execute permission bit.
#[test]
#[cfg(unix)]
fn scenario_3_2a_pass_through_program_found_but_not_executable() {
    let harness = OneshotHarness::new();
    let directory = tempfile::tempdir().unwrap();
    let file_path = directory.path().join("not-executable");
    std::fs::write(&file_path, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(
        &file_path,
        std::os::unix::fs::PermissionsExt::from_mode(0o644),
    )
    .unwrap();

    let (code, _stdout, stderr) =
        harness.run(&["fixture-default-alpha", "--", file_path.to_str().unwrap()]);
    assert_eq!(code, 126, "stderr={stderr}");
    let body = parse_stderr_json(&stderr);
    assert_eq!(body["category"], "pass_through_not_executable");
}

/// One attempt at the scenario 3.3 race: spawns `oneshot fixture-corrupt-checksum`
/// against a fresh harness, polls for the environment's own prefix
/// directory to appear under `envs/`, and immediately `chmod 000`s it so
/// `fail_and_roll_back`'s own cleanup fails too. Returns the parsed
/// stderr body if the created directory was actually found and locked
/// before the process exited (i.e. the race was won), `None` otherwise —
/// the caller retries on `None` rather than treating a lost race as a
/// pass.
///
/// Polls every [`POLL_INTERVAL`] rather than a coarser interval: measured
/// locally, the real window between the prefix directory's creation and
/// `fail_and_roll_back`'s own removal of it is on the order of tens of
/// milliseconds — comfortably caught by this interval on an idle
/// machine, but a coarser one (an earlier version of this loop used 5ms)
/// let a single scheduler-contention stall on a loaded CI runner blot out
/// the entire window, losing the race on every one of `ATTEMPTS` retries
/// in a row rather than the occasional single attempt this race is
/// otherwise expected to lose.
#[cfg(unix)]
const POLL_INTERVAL: Duration = Duration::from_millis(1);

#[cfg(unix)]
fn attempt_dual_failure_race() -> Option<serde_json::Value> {
    let harness = OneshotHarness::new();
    let envs_dir = harness.root_path().join("envs");
    let mut command = harness.raw_command();
    command
        .args(["oneshot", "fixture-corrupt-checksum", "--", "echo", "hi"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = command.spawn().unwrap();

    let mut locked_directory: Option<std::path::PathBuf> = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if let Ok(entries) = std::fs::read_dir(&envs_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir()
                    && std::fs::set_permissions(
                        &path,
                        std::os::unix::fs::PermissionsExt::from_mode(0o000),
                    )
                    .is_ok()
                {
                    locked_directory = Some(path);
                    break;
                }
            }
        }
        if locked_directory.is_some() {
            break;
        }
        std::thread::sleep(POLL_INTERVAL);
    }

    let output = child.wait_with_output().unwrap();
    if let Some(path) = &locked_directory {
        let _ = std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    }

    let _locked_directory = locked_directory?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let body = parse_stderr_json(&stderr);
    assert_eq!(body["category"], "integrity_verification_failed");
    body.get("cleanup_category")?;
    Some(body)
}

/// Scenario 3.3 (`#[cfg(unix)]` only): a dual failure — the primary
/// creation failure (`integrity_verification_failed`, via the checked-in
/// corrupt-checksum fixture) and its own rollback also fails, because the
/// environment's own prefix directory was made unwritable in the narrow
/// window between its creation and `fail_and_roll_back`'s cleanup. Races
/// that window from outside the process (see `attempt_dual_failure_race`);
/// retried up to 5 times with a fresh harness each attempt to absorb
/// inherent scheduling jitter — a lost race on every attempt is a genuine
/// test failure, not a silent pass, since the dual-failure JSON shape
/// (`render_ephemeral_creation_failure`'s own unit tests, `output.rs`)
/// only proves the *rendering* contract, not that this integration path
/// actually reaches it.
#[test]
#[cfg(unix)]
fn scenario_3_3_dual_failure_creation_and_cleanup_both_fail() {
    const ATTEMPTS: u32 = 5;
    for attempt in 1..=ATTEMPTS {
        if let Some(body) = attempt_dual_failure_race() {
            assert_eq!(body["cleanup_category"], "teardown_failed");
            assert_ne!(body["category"], body["cleanup_category"]);
            return;
        }
        eprintln!("scenario_3_3: lost the dual-failure race on attempt {attempt}/{ATTEMPTS}");
    }
    panic!(
        "scenario_3_3: never won the dual-failure race in {ATTEMPTS} attempts \
         (the environment's own prefix directory was never observed before \
         create_ephemeral_environment's own cleanup completed)"
    );
}

// ---------------------------------------------------------------------
// Cross-cutting scenarios (FR-003, FR-009, FR-015)
// ---------------------------------------------------------------------

/// Scenario C.1: an arbitrary, `allez`-unrelated environment variable
/// reaches the pass-through command unchanged.
#[test]
#[cfg(unix)]
fn scenario_c_1_inherited_environment_variables_reach_pass_through() {
    let harness = OneshotHarness::new();
    let output = harness
        .command()
        .env("MY_TEST_MARKER", "xyz")
        .args([
            "oneshot",
            "fixture-default-alpha",
            "--",
            "sh",
            "-c",
            "echo $MY_TEST_MARKER",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "xyz");
}

/// Scenario C.2: the created environment persists after every invocation,
/// regardless of outcome.
#[test]
fn scenario_c_2_environment_persists_after_successful_run() {
    let harness = OneshotHarness::new();
    let (code, _stdout, stderr) = harness.run(&["fixture-default-alpha", "--", "echo", "hi"]);
    assert_eq!(code, 0, "stderr={stderr}");
    let envs_dir = harness.root_path().join("envs");
    let entries: Vec<_> = std::fs::read_dir(&envs_dir).unwrap().collect();
    assert!(
        !entries.is_empty(),
        "the created environment should still be on disk"
    );
}

/// Scenario C.2, signal-terminated case (`#[cfg(unix)]` only).
#[test]
#[cfg(unix)]
fn scenario_c_2_environment_persists_after_signal_terminated_pass_through() {
    let harness = OneshotHarness::new();
    let (code, _stdout, _stderr) =
        harness.run(&["fixture-default-alpha", "--", "sh", "-c", "kill -TERM $$"]);
    assert_eq!(code, 143);
    let envs_dir = harness.root_path().join("envs");
    let entries: Vec<_> = std::fs::read_dir(&envs_dir).unwrap().collect();
    assert!(
        !entries.is_empty(),
        "the created environment should still be on disk"
    );
}

/// Scenario C.3: no caller-facing disclosure of the environment's own
/// location/identifier, for both a success and a pre-start failure.
#[test]
fn scenario_c_3_no_disclosure_of_environment_location_on_success() {
    let harness = OneshotHarness::new();
    let (code, stdout, stderr) = harness.run(&["fixture-default-alpha", "--", "echo", "hi"]);
    assert_eq!(code, 0, "stderr={stderr}");
    let root_str = harness.root_path().to_string_lossy();
    assert!(!stdout.contains(root_str.as_ref()));
    assert!(!stderr.contains(root_str.as_ref()));

    let envs_dir = harness.root_path().join("envs");
    let ulid = std::fs::read_dir(&envs_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .file_name()
        .to_string_lossy()
        .to_string();
    assert!(!stdout.contains(&ulid));
    assert!(!stderr.contains(&ulid));
}

#[test]
fn scenario_c_3_no_disclosure_of_environment_location_on_creation_failure() {
    let harness = OneshotHarness::new();
    let (code, stdout, stderr) =
        harness.run(&["definitely-nonexistent-package-xyz", "--", "echo", "hi"]);
    assert_eq!(code, 1, "stderr={stderr}");
    let root_str = harness.root_path().to_string_lossy();
    assert!(!stdout.contains(root_str.as_ref()));
    assert!(!stderr.contains(root_str.as_ref()));
}
