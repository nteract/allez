//! Conformance harness for `.condarc` validity (GEN-36 / GEN-23).
//!
//! For every fixture under `conformance/condarc/valid/*.json` and
//! `conformance/condarc/invalid/*.json`, this asks three independent
//! "checkers" whether they consider the fixture's JSON value a valid
//! `.condarc`, and asserts the answer matches the fixture's directory
//! (`valid/` vs `invalid/` is the sole pass/fail oracle -- fixtures are
//! bare JSON values with no wrapper metadata). See
//! `docs/condarc_research.md` for the research backing the fixtures and
//! the eventual `docs/condarc_openapi.json` schema.
//!
//! ## Invalid-fixture explosion
//!
//! A checker that rejects a *whole* multi-key document only proves that
//! *some* key in it is invalid, not that *every* key in it is -- and
//! for checkers that stop at the first bad key (real conda does, for
//! several of its own bugs; see `docs/condarc_research.md` §8 items
//! 8/9/11), a shared-battery fixture can pass conformance while never
//! even evaluating most of its own keys. To close that gap,
//! `invalid_condarc_is_rejected` below explodes every *object-shaped*
//! fixture under `conformance/condarc/invalid/` into one single-key
//! JSON value per top-level key at test time (see [`invalid_cases`]),
//! and checks + asserts on each key independently, instead of sending
//! the fixture's document through as a single whole-document check.
//!
//! Two kinds of invalid fixture are exempt from this explosion, and
//! are still tested as a single whole-document case:
//!
//!   - Fixtures whose JSON root isn't an object (e.g. `array_root.json`)
//!     -- there are no keys to explode.
//!   - Fixtures named with a `_combined` suffix (before `.json`), e.g.
//!     `always_copy_and_softlink_combined.json`. Use this naming
//!     convention when a fixture's invalidity comes from the
//!     *combination* of two or more keys (each of which is
//!     independently valid on its own) rather than from any single key
//!     -- exploding such a fixture per-key would silently turn each
//!     resulting case into a spuriously *valid* check.
//!
//! The three checkers:
//!
//!   - **conda**: shells out to a Python interpreter with `conda`
//!     importable and drives conda's own `conda.base.context` machinery
//!     directly (`reset_context(search_path=(path,))` +
//!     `context.validate_all()`), so this is genuinely "the python
//!     innards", not a re-implementation. `search_path=(path,)` fully
//!     *replaces* conda's default search path (which otherwise includes
//!     `/etc/conda/.condarc`, `~/.condarc`, `$CONDA_PREFIX/.condarc`,
//!     etc.) -- only the fixture file is read, matching a real
//!     single-file `.condarc` conformance check rather than a merge of
//!     every config source on the host running the tests. All
//!     `CONDA*`-prefixed environment variables (including `CONDARC`,
//!     which would otherwise inject one more merged source) are also
//!     stripped from the subprocess environment for the same reason.
//!   - **crate**: the `condarc` crate (GEN-36), driven via its own public
//!     `parse_with_options` entry point (`ssl_verify_fs_check: true`, per
//!     spec A3) and rendered through the test-only adapter at
//!     `tests/support/adapter.rs` for the exact `expected/*.json`
//!     comparison -- see [`check_crate`] and
//!     [`assert_crate_expected_representation`]. Four `valid/` bignum
//!     fixtures are declared A1 divergences (fixed-width `i64`/`f64`
//!     cannot represent them) in
//!     `support::adapter::CRATE_A1_DIVERGENCES` and are asserted
//!     *rejected* by this checker specifically, even though conda/openapi
//!     still accept them -- see `valid_condarc_is_accepted`.
//!   - **openapi**: validates against the `Condarc` schema nested under
//!     `components.schemas.Condarc` in `docs/condarc_openapi.json`
//!     (an OpenAPI 3.1 document; only that one subschema is used as the
//!     actual JSON Schema fed to the `jsonschema` crate -- see
//!     `check_openapi`). Currently a deliberate stub covering only
//!     `default_threads`; still under active, iterative construction
//!     (GEN-36-adjacent) -- see that file's own `info.description` for
//!     scope.
//!
//! Each checker is automatically skipped (not failed) when its backend
//! is unavailable (no `conda`-capable python found / schema file
//! missing -- the crate checker's backend, the compiled-in `condarc`
//! crate, is always available), and can additionally be
//! force-skipped via `ALLEZ_CONFORMANCE_SKIP_CONDA` /
//! `ALLEZ_CONFORMANCE_SKIP_CRATE` / `ALLEZ_CONFORMANCE_SKIP_OPENAPI`
//! (set to `1` or `true`). See the `Makefile`'s `conformance-*` targets
//! for running one checker at a time.
//!
//! This whole test binary only builds/runs with `--features
//! conformance-tests` (see the `[[test]]` entry in `Cargo.toml`), so a
//! plain `cargo test --all` -- the normal local dev workflow -- never
//! even compiles it; it's treated as a separate, slower integration-test
//! tier rather than a unit test. The one exception to "unavailable
//! backend is a skip, not a failure" is the conda oracle specifically:
//! when `CI=true` (set automatically by GitHub Actions and most other CI
//! providers), [`find_conda_python`] panics instead of returning `None`,
//! so the dedicated conformance CI job (which installs miniconda) fails
//! loudly on a broken/missing conda install instead of silently skipping
//! every conda-oracle case.
//!
//! That same "unavailable backend is a skip, but broken-once-it-started
//! is a hard failure under `CI=true`" split also applies *after* a
//! backend has been located: once `find_conda_python` has already
//! succeeded, a subsequent tempfile-creation failure, subprocess-spawn
//! failure, or unexpected (neither `0` nor `1`) exit code from the
//! conda oracle (in [`check_conda`] and
//! [`assert_conda_expected_representation`]) no longer indicates "conda
//! isn't set up" -- it indicates the oracle broke mid-run, which is just
//! as much a CI-worthy hard failure as a missing interpreter. The same
//! goes for [`check_openapi`]'s schema *parsing* (as opposed to the
//! schema file simply not existing yet, which is still a legitimate
//! permanent skip -- see that function's own comments): a checked-in
//! `docs/condarc_openapi.json` that fails to parse, or whose
//! `Condarc` subschema fails to compile, is corrupt, not absent. See
//! [`skip_or_ci_panic`] for the shared helper backing this.
//!
//! ## Always invoke via `make conformance*` (or copy its `touch`)
//!
//! `valid_condarc_is_accepted` and `invalid_condarc_is_rejected` below
//! glob `conformance/condarc/{valid,invalid}/*.json` via rstest's
//! `#[files(...)]`, which expands *at proc-macro time during
//! compilation* -- not at runtime. Cargo's rebuild decision is based
//! solely on its own tracked inputs (`.rs`/`Cargo.toml` mtimes,
//! `Cargo.lock`, etc.); it has no visibility into files a proc macro
//! happened to read while expanding, so adding/removing/editing a
//! fixture *without touching any `.rs` file* will not, by itself,
//! trigger a rebuild. Running `cargo test --test condarc_conformance
//! --features conformance-tests` directly after only changing fixtures
//! will silently keep executing the previously-compiled test binary
//! against the previously-compiled (stale) fixture set.
//!
//! Both places that matter -- the `make conformance*` targets and the
//! `conformance` job in `.github/workflows/ci.yml` -- work around this
//! by running `touch tests/condarc_conformance.rs` immediately before
//! the `cargo test` invocation, which is enough to force cargo to
//! recompile (and thus re-expand the `#[files(...)]` glob) every time.
//! This is a deliberate, low-cost fix for a real rstest limitation
//! (rstest doesn't use `tracked_path`/`include_str!` to register the
//! glob's fixture files as compiler inputs), not a stopgap awaiting a
//! `build.rs`: a `build.rs` would need to duplicate this same
//! directory-scan-and-invalidate logic (and get directory-mtime
//! semantics right across filesystems) just to force the same
//! recompile `touch` already forces for a few bytes of Makefile/CI
//! YAML. If you need to run this suite by hand, run it through one of
//! the `make conformance*` targets, or `touch` this file yourself
//! first -- don't invoke `cargo test --test condarc_conformance`
//! directly and expect newly-added fixtures to be picked up.
//!
//! ## Expected internal representation (conda only, `valid/` only)
//!
//! Accept/reject alone doesn't confirm a checker parsed a fixture's
//! values *correctly* -- e.g. that `"yes"` really becomes the internal
//! `true`, not just "some truthy-ish thing". For every fixture under
//! `conformance/condarc/valid/`, once the conda checker accepts it,
//! `valid_condarc_is_accepted` also calls
//! [`assert_conda_expected_representation`], which re-derives conda's
//! live internal representation (via
//! `scripts/generate_zzz_condarc_expected_fixtures.py --fixture`, the
//! exact same logic used to generate the checked-in
//! `conformance/condarc/expected/<name>.json` files) and asserts it
//! still matches what's checked in. There's no equivalent for
//! `invalid/` fixtures (nothing parses, so there's no representation to
//! record) or for the crate/openapi checkers (neither produces a value
//! yet -- see `check_crate`/`check_openapi`).

use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use rstest::rstest;
use serde_json::Value;

mod support;

// ---------------------------------------------------------------------
// Checkers
// ---------------------------------------------------------------------

/// The three independent "is this .condarc valid" oracles under test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Checker {
    /// Real conda, driven via its own Python `conda.base.context` API.
    Conda,
    /// The `condarc` Rust crate (GEN-36).
    Crate,
    /// The `Condarc` subschema of `docs/condarc_openapi.json`,
    /// validated via the `jsonschema` crate.
    OpenApi,
}

/// Result of asking a [`Checker`] about one fixture.
enum CheckOutcome {
    Valid,
    Invalid(String),
    /// The checker's backend isn't available (or was force-skipped) --
    /// this fixture makes no assertion for this checker.
    Skipped(String),
}

// ---------------------------------------------------------------------
// Hardcoded per-fixture checker applicability (docs/condarc_research.md item 21)
// ---------------------------------------------------------------------
//
// Two fixture-name lists, checked *before* any checker runs (`Checker::check` below) rather than
// by running every checker and then inspecting/asserting on the outcome afterward. Each list
// says outright which checker(s) a fixture simply doesn't apply to; there is no live behavior to
// probe for those checkers on that fixture, so none is probed.
//
// Both lists exist for exactly one reason: `yaml-rust2` (this crate's YAML dependency) enforces
// no equivalent to the YAML 1.1 "simple key" 1024-character scanner limit that both real conda's
// YAML library (`ruamel.yaml`) and `docs/condarc_openapi.json`'s schema (which deliberately
// copies conda's limit as `propertyNames.maxLength: 1022`) enforce.

/// `invalid/` fixture *file stems* for which the `Crate` checker is skipped entirely: these use
/// a 1023-character raw key, one character past conda's/the schema's 1024-character limit, so
/// real conda and the openapi schema both correctly reject them, but the crate's own YAML
/// dependency has no equivalent limit and would accept them -- there's nothing to compare a
/// non-existent crate rejection against, so the `Crate` checker simply doesn't run here. See
/// [`RUST_ONLY_FIXTURES`] for this same gap's mirror image.
const CRATE_SKIPPED_FIXTURES: &[&str] = &[
    "custom_multichannels_values_reject_key_exceeds_yaml_simple_key_length_limit",
    "dict_of_strings_values_reject_key_exceeds_yaml_simple_key_length_limit",
];

/// `valid/` fixture *file stems* for which the `Conda` and `OpenApi` checkers are both skipped
/// entirely: these fixtures exist purely to pin the crate's own (lack of a) YAML simple-key
/// length limit (see [`CRATE_SKIPPED_FIXTURES`]'s doc comment) using the identical
/// 1023-character key. Real conda would reject the document outright (no live value to
/// compare), and the openapi schema copies conda's same limit, so neither checker has a
/// meaningful verdict to produce -- only the `Crate` checker runs normally on these, including
/// its usual exact adapter-output comparison against a hand-authored `expected/*.json` (see
/// `scripts/generate_zzz_condarc_expected_fixtures.py`'s `CRATE_ONLY_YAML_KEY_LENGTH_FIXTURES`).
const RUST_ONLY_FIXTURES: &[&str] = &[
    "custom_multichannels_values_accept_key_exceeds_yaml_simple_key_length_limit",
    "dict_of_strings_values_accept_key_exceeds_yaml_simple_key_length_limit",
];

fn fixture_stem(path: &Path) -> &str {
    path.file_stem().and_then(|s| s.to_str()).unwrap_or("")
}

/// Whether `checker` is inapplicable to `path` per the hardcoded lists above -- checked by
/// [`Checker::check`] *before* doing anything else, so an inapplicable checker never actually
/// runs against that fixture at all (no conda subprocess spawned, no `condarc::parse` call, no
/// schema validation attempted).
fn is_fixture_skipped_for_checker(checker: Checker, path: &Path) -> bool {
    let stem = fixture_stem(path);
    match checker {
        Checker::Crate => CRATE_SKIPPED_FIXTURES.contains(&stem),
        Checker::Conda | Checker::OpenApi => RUST_ONLY_FIXTURES.contains(&stem),
    }
}

impl Checker {
    fn skip_env_var(self) -> &'static str {
        match self {
            Checker::Conda => "ALLEZ_CONFORMANCE_SKIP_CONDA",
            Checker::Crate => "ALLEZ_CONFORMANCE_SKIP_CRATE",
            Checker::OpenApi => "ALLEZ_CONFORMANCE_SKIP_OPENAPI",
        }
    }

    fn is_force_skipped(self) -> bool {
        env::var(self.skip_env_var())
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    }

    fn check(self, value: &Value, path: &Path) -> CheckOutcome {
        if self.is_force_skipped() {
            return CheckOutcome::Skipped(format!("force-skipped via {}=1", self.skip_env_var()));
        }
        if is_fixture_skipped_for_checker(self, path) {
            return CheckOutcome::Skipped(
                "not applicable to this fixture (see CRATE_SKIPPED_FIXTURES/RUST_ONLY_FIXTURES)"
                    .to_string(),
            );
        }
        match self {
            Checker::Conda => check_conda(value),
            Checker::Crate => check_crate(value),
            Checker::OpenApi => check_openapi(value),
        }
    }
}

// ---------------------------------------------------------------------
// conda checker
// ---------------------------------------------------------------------

/// Python snippet run against a fixture written out to a real file.
///
/// `reset_context(search_path=(path,))` replaces conda's *entire*
/// default search path with just this one file (see module docs), then
/// `validate_all()` runs both per-parameter type coercion/validation
/// and the two cross-field rules in `Context.post_build_validation()`
/// (`client_ssl_cert`/`client_ssl_cert_key`,
/// `always_copy`/`always_softlink`) -- see `docs/condarc_research.md`
/// Section 1.4. Exit code 0 = valid, 1 = rejected (stderr has the
/// exception), 2 = `import conda` itself failed inside this
/// interpreter (shouldn't happen since callers already probed for
/// this, but handled defensively).
const CONDA_CHECK_SCRIPT: &str = r#"
import sys

try:
    from conda.base.context import reset_context, context
except Exception as e:
    print(f"IMPORT_ERROR: {type(e).__name__}: {e}", file=sys.stderr)
    sys.exit(2)

path = sys.argv[1]

try:
    reset_context(search_path=(path,))
    context.validate_all()
except Exception as e:
    print(f"{type(e).__name__}: {e}", file=sys.stderr)
    sys.exit(1)

sys.exit(0)
"#;

/// Builds a [`Command`] for `program` with every `CONDA*`-prefixed
/// environment variable (including `CONDARC`) removed, so the conda
/// oracle only ever sees the single fixture file passed via
/// `search_path`, never the host machine's real `~/.condarc` or any
/// `CONDA_*` setting merged in from the ambient shell environment.
fn conda_free_command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    for (key, _) in env::vars() {
        if key.starts_with("CONDA") {
            cmd.env_remove(key);
        }
    }
    cmd
}

fn python_has_conda(python: &Path) -> bool {
    conda_free_command(python)
        .args(["-c", "import conda"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Candidate python interpreter paths under a conda installation root
/// (e.g. `$CONDA`, or the parent of a `condabin/` directory found on
/// `PATH`). Unix layouts put the interpreter at `<root>/bin/python3`;
/// Windows layouts put `python.exe` directly under `<root>`.
fn python_candidates_under_root(root: &Path) -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![root.join("python.exe")]
    } else {
        vec![root.join("bin/python3"), root.join("bin/python")]
    }
}

/// Locates the first `conda`-capable `conda` executable on `PATH`
/// (without invoking it), so we can find the Python interpreter that
/// ships alongside it.
fn which_conda() -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join("conda");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Finds a Python interpreter with `conda` importable, trying (in
/// order):
///
///   1. `$ALLEZ_CONFORMANCE_PYTHON`.
///   2. A bare `python3` on `PATH`.
///   3. `$CONDA/bin/python3` (or `python`) -- `$CONDA` is set by common
///      installers/CI actions (e.g. `conda-incubator/setup-miniconda`)
///      to the conda installation *root*. That root's `condabin/`
///      subdirectory (containing only the `conda` launcher script, not
///      python) is typically what such actions put on `PATH`, not
///      `bin/` (where the real interpreter lives) -- so this step
///      exists specifically to not miss that layout.
///   4. The interpreter shipped in the same directory as whatever
///      `conda` executable is on `PATH` (covers installs that
///      co-locate `conda` and `python` in one `bin/`), or, if that
///      `conda` turned out to live in a `condabin/` directory, its
///      sibling `bin/` under the same install root (covers the same
///      `condabin`-on-`PATH` layout as step 3, but without requiring
///      `$CONDA` to be set).
///
/// A bare `python3` on `PATH` is frequently a *different*, conda-less
/// interpreter than the one conda itself runs under, hence not
/// stopping at step 2 alone.
fn probe_conda_python() -> Option<PathBuf> {
    if let Ok(p) = env::var("ALLEZ_CONFORMANCE_PYTHON") {
        let p = PathBuf::from(p);
        return python_has_conda(&p).then_some(p);
    }

    let candidate = PathBuf::from("python3");
    if python_has_conda(&candidate) {
        return Some(candidate);
    }

    if let Ok(root) = env::var("CONDA") {
        for candidate in python_candidates_under_root(Path::new(&root)) {
            if candidate.is_file() && python_has_conda(&candidate) {
                return Some(candidate);
            }
        }
    }

    let conda_path = which_conda()?;
    let dir = conda_path.parent()?;
    for name in ["python3", "python"] {
        let candidate = dir.join(name);
        if candidate.is_file() && python_has_conda(&candidate) {
            return Some(candidate);
        }
    }
    if dir.file_name().and_then(|n| n.to_str()) == Some("condabin")
        && let Some(root) = dir.parent()
    {
        for candidate in python_candidates_under_root(root) {
            if candidate.is_file() && python_has_conda(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Cached result of [`probe_conda_python`]. Every fixture x checker
/// test case calls [`find_conda_python`], and the probe itself spawns
/// a `python3 -c "import conda"` subprocess (sometimes two, if the
/// bare `python3` on `PATH` doesn't have conda) -- caching means that
/// subprocess spawn happens once per test binary invocation instead
/// of once per case, regardless of how many test threads call it
/// concurrently.
static CONDA_PYTHON: OnceLock<Option<PathBuf>> = OnceLock::new();

/// True when running under a CI runner (GitHub Actions, and most other
/// CI providers, export `CI=true` into every job's environment
/// automatically -- no workflow-specific config needed on our end).
fn is_ci() -> bool {
    matches!(env::var("CI"), Ok(v) if v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Wraps the cached [`probe_conda_python`] result, additionally
/// panicking (instead of quietly returning `None`) when no
/// conda-capable python was found *and* `CI=true` -- so a CI run that's
/// supposed to have a real conda oracle (see the `conformance` job in
/// `.github/workflows/ci.yml`, which installs miniconda via
/// `conda-incubator/setup-miniconda`) fails loudly on a broken/missing
/// install instead of every conda-oracle case silently reporting
/// `CheckOutcome::Skipped`. Outside CI (plain local dev use, e.g. `make
/// conformance-conda` without conda installed), a missing conda oracle
/// is still just a skip, same as before.
fn find_conda_python() -> Option<PathBuf> {
    let found = CONDA_PYTHON.get_or_init(probe_conda_python).clone();
    if found.is_none() && is_ci() {
        panic!(
            "no python interpreter with `conda` importable was found, but CI=true -- the \
             conda oracle is required in CI, not an optional skip (checked \
             $ALLEZ_CONFORMANCE_PYTHON, `python3` on PATH, `$CONDA/bin/python3`, and the \
             interpreter shipped alongside `conda` on PATH -- see probe_conda_python's docs \
             for the full lookup order). If this is the conformance CI job, check that the \
             miniconda setup step ran and actually set up `conda`/`python3`/`$CONDA` before \
             this step."
        );
    }
    found
}

/// Turns an unexpected-failure `reason` into a hard panic when `CI=true`,
/// and into an ordinary [`CheckOutcome::Skipped`] otherwise.
///
/// Only meant for failures that happen *after* a backend has already
/// been confirmed available (e.g. `find_conda_python` already
/// succeeded) -- at that point, a tempfile-creation failure,
/// subprocess-spawn failure, or unexpected exit code means the oracle
/// broke mid-run, not that it's simply unavailable, so it deserves the
/// same "fail loudly in CI" treatment as [`find_conda_python`]'s own
/// missing-interpreter panic rather than a silent skip.
fn skip_or_ci_panic(reason: String) -> CheckOutcome {
    if is_ci() {
        panic!(
            "conda oracle broke mid-run, but CI=true -- treating this as a hard failure \
             instead of a silent skip: {reason}"
        );
    }
    CheckOutcome::Skipped(reason)
}

fn check_conda(value: &Value) -> CheckOutcome {
    let Some(python) = find_conda_python() else {
        return CheckOutcome::Skipped(
            "no python interpreter with `conda` importable found (checked \
             $ALLEZ_CONFORMANCE_PYTHON, `python3` on PATH, `$CONDA/bin/python3`, and the \
             interpreter shipped alongside `conda` on PATH)"
                .to_string(),
        );
    };

    // conda's `_expand_search_path` only recognizes files named exactly
    // `.condarc`/`condarc`, or with a `.yml`/`.yaml` suffix -- anything
    // else is silently dropped from the search path *before* it's ever
    // parsed, which would make every fixture look "valid" by omission
    // rather than by actually being loaded. The fixture's JSON text is
    // valid YAML for every shape this suite cares about, so writing it
    // out with a `.yml` suffix lets conda's real YAML loader parse it
    // unmodified.
    let mut fixture = match tempfile::Builder::new().suffix(".yml").tempfile() {
        Ok(f) => f,
        Err(err) => {
            return skip_or_ci_panic(format!("failed to create temp fixture file: {err}"));
        }
    };
    let text = serde_json::to_string(value).expect("fixture value should serialize to JSON");
    if let Err(err) = fixture.write_all(text.as_bytes()) {
        return skip_or_ci_panic(format!("failed to write temp fixture file: {err}"));
    }

    let output = conda_free_command(&python)
        .args(["-c", CONDA_CHECK_SCRIPT])
        .arg(fixture.path())
        .output();

    let output = match output {
        Ok(o) => o,
        Err(err) => {
            return skip_or_ci_panic(format!("failed to run conda oracle subprocess: {err}"));
        }
    };

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    match output.status.code() {
        Some(0) => CheckOutcome::Valid,
        Some(1) => CheckOutcome::Invalid(stderr),
        code => skip_or_ci_panic(format!(
            "conda oracle exited unexpectedly (status={code:?}): {stderr}"
        )),
    }
}

// ---------------------------------------------------------------------
// conda "expected internal representation" check
// ---------------------------------------------------------------------
//
// In addition to accept/reject, the conda oracle is also the only
// checker today that can report *what* it parsed a valid fixture into
// (GEN-36's crate doesn't exist yet, and `docs/condarc_openapi.json`
// is schema-only -- neither produces a value, just accept/reject). This
// reuses `scripts/generate_zzz_condarc_expected_fixtures.py` --
// specifically its `--fixture PATH` mode, which computes and prints one
// fixture's expected representation to stdout without touching
// `conformance/condarc/expected/` at all -- so the exact same
// alias-resolution/shadowed-attribute/canonicalization logic backs both
// the checked-in fixtures and this live re-check, instead of drifting
// apart as two separate implementations.

/// Path to the generator script reused for computing conda's internal
/// representation of a fixture -- see its module docs for the
/// alias-resolution/shadowed-attribute logic this relies on.
fn condarc_expected_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts/generate_zzz_condarc_expected_fixtures.py")
}

/// The checked-in expected-representation fixture for `fixture_path`
/// (e.g. `conformance/condarc/valid/foo.json` ->
/// `conformance/condarc/expected/foo.json`), if one exists. `None`
/// either because the generator itself intentionally skips this fixture
/// (JSON root isn't an object, e.g. `null_root.json` -- see
/// `has_no_keys_to_resolve` in that script) or because `expected/`
/// simply hasn't been (re)generated for it yet -- callers must tell
/// those two cases apart themselves (see
/// `assert_conda_expected_representation`).
fn condarc_expected_fixture_path(fixture_path: &Path) -> Option<PathBuf> {
    let name = fixture_path.file_name()?;
    let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("conformance/condarc/expected")
        .join(name);
    candidate.is_file().then_some(candidate)
}

/// Asserts that conda's *live* internal representation of `fixture_path`
/// -- recomputed right now, via the same oracle `check_conda` just used
/// to accept it -- still matches the checked-in
/// `conformance/condarc/expected/<name>.json`. Only meaningful once
/// `check_conda` has already returned [`CheckOutcome::Valid`] for this
/// fixture; callers must gate on that themselves (see
/// `valid_condarc_is_accepted`), since there's no expected value to
/// compare against a rejection.
/// Asserts that conda's *live* internal representation of `fixture_path`
/// -- recomputed right now, via the same oracle `check_conda` just used
/// to accept it -- still matches the checked-in
/// `conformance/condarc/expected/<name>.json`. Only meaningful once
/// `check_conda` has already returned [`CheckOutcome::Valid`] for this
/// fixture; callers must gate on that themselves (see
/// `valid_condarc_is_accepted`), since there's no expected value to
/// compare against a rejection.
///
/// `value` is this same fixture's already-loaded JSON, used solely to
/// tell apart the two reasons `condarc_expected_fixture_path` can come
/// back empty: a non-object root (e.g. `null_root.json`) is a
/// legitimate, permanent skip -- `generate_zzz_condarc_expected_fixtures.py`
/// has no keys to resolve for those and will never produce a file for
/// them. An object root with no `expected/` file, on the other hand,
/// means someone added (or renamed) a `valid/` fixture without running
/// `make regenerate-condarc-fixtures` -- that's a real gap, so it fails
/// the test instead of silently skipping it.
fn assert_conda_expected_representation(fixture_path: &Path, value: &Value) {
    let Some(expected_path) = condarc_expected_fixture_path(fixture_path) else {
        assert!(
            !value.is_object(),
            "[Conda expected] {} -- no conformance/condarc/expected/{} fixture exists, but \
             this fixture's JSON root is an object, so one should. This usually means a \
             `valid/` fixture was added or renamed without running `make \
             regenerate-condarc-fixtures` -- run that and commit the resulting expected/ file.",
            fixture_path.display(),
            fixture_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        println!(
            "SKIPPED [Conda expected] {}: fixture's JSON root isn't an object, so \
             scripts/generate_zzz_condarc_expected_fixtures.py intentionally has no \
             conformance/condarc/expected/ file for it (nothing to resolve keys for)",
            fixture_path.display(),
        );
        return;
    };

    let Some(python) = find_conda_python() else {
        println!(
            "SKIPPED [Conda expected] {}: no conda-capable python found",
            fixture_path.display()
        );
        return;
    };

    let output = conda_free_command(&python)
        .arg(condarc_expected_script_path())
        .arg("--fixture")
        .arg(fixture_path)
        .output();

    let output = match output {
        Ok(o) => o,
        Err(err) => {
            let reason = format!("failed to run generate_zzz_condarc_expected_fixtures.py: {err}");
            if is_ci() {
                panic!(
                    "[Conda expected] {} -- {reason}, but CI=true -- the conda oracle broke \
                     mid-run, which is a hard failure, not a case for silently skipping",
                    fixture_path.display(),
                );
            }
            println!(
                "SKIPPED [Conda expected] {}: {reason}",
                fixture_path.display()
            );
            return;
        }
    };

    assert!(
        output.status.success(),
        "[Conda expected] {} -- `generate_zzz_condarc_expected_fixtures.py --fixture` \
         failed (exit={:?}), even though `check_conda` just accepted this same fixture: {}",
        fixture_path.display(),
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).trim(),
    );

    let actual: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "[Conda expected] {} -- `generate_zzz_condarc_expected_fixtures.py --fixture` \
             did not print valid JSON: {err}\nstdout: {}",
            fixture_path.display(),
            String::from_utf8_lossy(&output.stdout),
        )
    });

    let expected_text = std::fs::read_to_string(&expected_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", expected_path.display()));
    let expected: Value = serde_json::from_str(&expected_text)
        .unwrap_or_else(|err| panic!("{} is not valid JSON: {err}", expected_path.display()));

    assert_eq!(
        actual,
        expected,
        "[Conda expected] {} -- conda's live internal representation no longer matches {}. \
         If this is an intentional behavior change (e.g. a conda upgrade changed coercion \
         behavior), regenerate it with `make regenerate-condarc-fixtures` and review the \
         diff.",
        fixture_path.display(),
        expected_path.display(),
    );
}

// ---------------------------------------------------------------------
// crate checker
// ---------------------------------------------------------------------

/// Feeds `value` (re-serialized as YAML/JSON text -- JSON is valid YAML for every fixture shape
/// this suite exercises) to the real `condarc` crate (GEN-36) via
/// `parse_with_options(.., ssl_verify_fs_check: true, null_sequence_map_defaults: true)` (spec
/// A3 for the former -- the corpus was generated from real conda, which always performs the
/// `ssl_verify` filesystem check; docs/condarc_research.md item 22 for the latter -- real conda
/// unconditionally resolves an explicit `null` on a `SequenceParameter`/`MapParameter`-typed
/// setting to its own class-level default, so the corpus's `expected/*.json` records that
/// default, not "absent"), mapping `Ok`/`Err` onto [`CheckOutcome`].
fn check_crate(value: &Value) -> CheckOutcome {
    let yaml =
        serde_json::to_string(value).expect("fixture value should serialize to JSON (valid YAML)");
    match condarc::parse_with_options(
        &yaml,
        condarc::ParseOptions::default()
            .with_ssl_verify_fs_check(true)
            .with_null_sequence_map_defaults(true),
    ) {
        Ok(_) => CheckOutcome::Valid,
        Err(report) => CheckOutcome::Invalid(report.to_string()),
    }
}

// ---------------------------------------------------------------------
// crate "expected internal representation" check (adapter exact comparison)
// ---------------------------------------------------------------------
//
// Mirrors `assert_conda_expected_representation` above, but for the crate: re-parses the
// fixture with `condarc::parse_with_options`, renders it via `support::adapter::to_expected_json`,
// and asserts exact equality against the same checked-in `conformance/condarc/expected/*.json`
// conda's own check compares against (contracts/adapter-output.md's comparison semantics).

/// Asserts that the `condarc` crate's rendered representation of `fixture_path` -- computed via
/// `support::adapter::to_expected_json` -- matches the checked-in
/// `conformance/condarc/expected/<name>.json` exactly. Only meaningful once `check_crate` has
/// already returned [`CheckOutcome::Valid`] for this fixture; callers must gate on that
/// themselves (see `valid_condarc_is_accepted`).
fn assert_crate_expected_representation(fixture_path: &Path, value: &Value) {
    let Some(expected_path) = condarc_expected_fixture_path(fixture_path) else {
        assert!(
            !value.is_object(),
            "[Crate expected] {} -- no conformance/condarc/expected/{} fixture exists, but \
             this fixture's JSON root is an object, so one should. This usually means a \
             `valid/` fixture was added or renamed without running `make \
             regenerate-condarc-fixtures` -- run that and commit the resulting expected/ file.",
            fixture_path.display(),
            fixture_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        println!(
            "SKIPPED [Crate expected] {}: fixture's JSON root isn't an object, so \
             scripts/generate_zzz_condarc_expected_fixtures.py intentionally has no \
             conformance/condarc/expected/ file for it (nothing to resolve keys for)",
            fixture_path.display(),
        );
        return;
    };

    let yaml =
        serde_json::to_string(value).expect("fixture value should serialize to JSON (valid YAML)");
    let cfg = condarc::parse_with_options(
        &yaml,
        condarc::ParseOptions::default()
            .with_ssl_verify_fs_check(true)
            .with_null_sequence_map_defaults(true),
    )
    .unwrap_or_else(|err| {
        panic!(
            "[Crate expected] {} -- parse_with_options failed even though check_crate just \
             accepted this same fixture: {err}",
            fixture_path.display(),
        )
    });
    let actual = support::adapter::to_expected_json(&cfg);

    let expected_text = std::fs::read_to_string(&expected_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", expected_path.display()));
    let expected: Value = serde_json::from_str(&expected_text)
        .unwrap_or_else(|err| panic!("{} is not valid JSON: {err}", expected_path.display()));

    assert_eq!(
        actual,
        expected,
        "[Crate expected] {} -- the condarc crate's adapted representation does not match {} \
         exactly (contracts/adapter-output.md's comparison semantics: missing key, extra key, \
         renamed key, or wrong value all fail this check).",
        fixture_path.display(),
        expected_path.display(),
    );
}

// ---------------------------------------------------------------------
// openapi checker
// ---------------------------------------------------------------------

fn openapi_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/condarc_openapi.json")
}

/// `docs/condarc_openapi.json` is a (mostly vacuous -- no real
/// `paths`) OpenAPI 3.1 document, not a bare JSON Schema document, so
/// the actual `.condarc` schema the `jsonschema` crate needs to
/// validate against lives nested at `components.schemas.Condarc`
/// inside it, per the standard OpenAPI convention for where reusable
/// schemas are defined. This pulls that one subschema back out --
/// while re-attaching the document's `components` object as a
/// *sibling* key on the returned value, not just discarding it.
///
/// That re-attachment matters as soon as `Condarc`'s own properties
/// start using `"$ref": "#/components/schemas/SomeReusableType"` (the
/// proper-type-definition style this schema is meant to use instead
/// of ad hoc inline shapes -- see docs/condarc_research.md and the
/// per-bucket schema work it backs). `jsonschema::validator_for`
/// resolves a `#/...` JSON Pointer `$ref` against the root of
/// whatever document it was actually given -- so handing it just the
/// bare `Condarc` node in isolation (this function's original,
/// simpler behavior) would make every such `$ref` dangle, since
/// `#/components/schemas/...` doesn't exist starting from `Condarc`
/// itself. Splicing `components` back in as a sibling of `Condarc`'s
/// own `type`/`properties`/etc. keys makes the returned value a
/// valid, self-contained resolution root for those refs, without
/// requiring any external resolver/retrieval configuration on the
/// `jsonschema` side -- unknown sibling keywords (`components` isn't
/// itself a JSON Schema keyword) are simply ignored by the validator.
fn extract_condarc_schema(document: &Value) -> Result<Value, String> {
    let condarc = document
        .pointer("/components/schemas/Condarc")
        .ok_or_else(|| {
            "missing components.schemas.Condarc (expected an OpenAPI 3.1 document with the \
         .condarc schema nested there)"
                .to_string()
        })?;
    let Value::Object(mut merged) = condarc.clone() else {
        return Err("components.schemas.Condarc is not a JSON object".to_string());
    };
    if let Some(components) = document.get("components") {
        merged.insert("components".to_string(), components.clone());
    }
    Ok(Value::Object(merged))
}

fn check_openapi(value: &Value) -> CheckOutcome {
    let schema_path = openapi_schema_path();
    // Unlike the branches below, a missing schema file is a legitimate,
    // permanent skip -- not gated on `is_ci()` via `skip_or_ci_panic` --
    // since `docs/condarc_openapi.json` is still under active,
    // iterative construction (see the module docs). Once the file
    // exists, though, it's checked in and expected to always parse and
    // compile; failures past this point are corruption, not absence.
    let schema_text = match std::fs::read_to_string(&schema_path) {
        Ok(text) => text,
        Err(_) => {
            return CheckOutcome::Skipped(format!("{} does not exist yet", schema_path.display()));
        }
    };
    let document: Value = match serde_json::from_str(&schema_text) {
        Ok(v) => v,
        Err(err) => {
            return skip_or_ci_panic(format!(
                "{} is not valid JSON: {err}",
                schema_path.display()
            ));
        }
    };
    let schema = match extract_condarc_schema(&document) {
        Ok(v) => v,
        Err(err) => {
            return skip_or_ci_panic(format!("{}: {err}", schema_path.display()));
        }
    };
    let validator = match jsonschema::validator_for(&schema) {
        Ok(v) => v,
        Err(err) => {
            return skip_or_ci_panic(format!(
                "{} components.schemas.Condarc is not a valid JSON Schema: {err}",
                schema_path.display()
            ));
        }
    };
    match validator.validate(value) {
        Ok(()) => CheckOutcome::Valid,
        Err(err) => CheckOutcome::Invalid(err.to_string()),
    }
}

// ---------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------

fn load_fixture(path: &Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read fixture {}: {err}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("fixture {} is not valid JSON: {err}", path.display()))
}

/// Filename suffix (before the `.json` extension) marking a fixture
/// whose keys must be checked *together*, not split apart one key at a
/// time -- see [`invalid_cases`] and the module docs above.
const COMBINED_SUFFIX: &str = "_combined";

fn is_combined_fixture(path: &Path) -> bool {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.ends_with(COMBINED_SUFFIX))
}

/// One independent thing to feed a [`Checker`]: either a fixture's
/// whole JSON value, or a single-key JSON object isolating one
/// top-level key from a larger fixture. `label` identifies which, for
/// assertion messages.
struct InvalidCase {
    label: String,
    value: Value,
}

/// Splits an invalid fixture's JSON `value` into the individual cases
/// that should each, independently, cause rejection.
///
/// A checker that rejects a *whole* multi-key document only proves
/// that *some* key in it is invalid, not that *every* key in it is --
/// so for an ordinary JSON object, this yields one case per top-level
/// key, each a fresh single-key object (`{"only_this_key": ...}`).
///
/// Two kinds of fixture are exempt, and yield a single case built from
/// the entire, unmodified `value` instead: fixtures whose JSON root
/// isn't an object (nothing to split), and fixtures named with the
/// `_combined` suffix (multiple keys are invalid only in combination,
/// so splitting them apart would make each half spuriously valid --
/// see the module docs above).
fn invalid_cases(path: &Path, value: Value) -> Vec<InvalidCase> {
    if !is_combined_fixture(path)
        && let Value::Object(obj) = value
    {
        assert!(
            !obj.is_empty(),
            "{} is an empty JSON object -- there are no keys to test individually. \
             If this is intentional, rename the fixture with a `{COMBINED_SUFFIX}` \
             suffix so its whole (empty) document is tested as a single case instead.",
            path.display()
        );
        return obj
            .into_iter()
            .map(|(key, val)| {
                let mut single = serde_json::Map::new();
                single.insert(key.clone(), val);
                InvalidCase {
                    label: key,
                    value: Value::Object(single),
                }
            })
            .collect();
    }
    vec![InvalidCase {
        label: "<whole file>".to_string(),
        value,
    }]
}

fn assert_outcome(outcome: CheckOutcome, expect_valid: bool, checker: Checker, path: &Path) {
    assert_outcome_for_case(outcome, expect_valid, checker, path, None);
}

fn assert_outcome_for_case(
    outcome: CheckOutcome,
    expect_valid: bool,
    checker: Checker,
    path: &Path,
    key: Option<&str>,
) {
    let subject = match key {
        Some(key) => format!("{} (key: {key:?})", path.display()),
        None => path.display().to_string(),
    };
    match outcome {
        CheckOutcome::Skipped(reason) => {
            println!("SKIPPED [{checker:?}] {subject}: {reason}");
        }
        CheckOutcome::Valid => {
            assert!(
                expect_valid,
                "[{checker:?}] {subject} was accepted, but lives under invalid/ (expected \
                 rejection)"
            );
        }
        CheckOutcome::Invalid(reason) => {
            assert!(
                !expect_valid,
                "[{checker:?}] {subject} was rejected, but lives under valid/ (expected \
                 acceptance). reason: {reason}"
            );
        }
    }
}

#[rstest]
fn valid_condarc_is_accepted(
    #[files("conformance/condarc/valid/*.json")] path: PathBuf,
    #[values(Checker::Conda, Checker::Crate, Checker::OpenApi)] checker: Checker,
) {
    let value = load_fixture(&path);
    let outcome = checker.check(&value, &path);

    // The `Crate` checker has exactly four declared A1 divergences (spec Assumptions A1):
    // fixed-width i64/f64 cannot represent these fixtures' arbitrary-precision numerals, so the
    // crate deliberately *rejects* them even though they stay in `valid/` (real conda, and the
    // openapi schema, still accept them -- see `support::adapter::CRATE_A1_DIVERGENCES`). This
    // is an assertion, not a suppression: a listed fixture that stops diverging fails loudly
    // below, and no adapter comparison ever runs for a fixture the crate rejects.
    if checker == Checker::Crate && support::adapter::is_crate_a1_divergence(&path) {
        match outcome {
            CheckOutcome::Invalid(_) => {}
            CheckOutcome::Valid => panic!(
                "[Crate] {} is a declared A1 divergence (tests/support/adapter.rs's \
                 CRATE_A1_DIVERGENCES) -- the crate was expected to reject it, but now accepts \
                 it. If this is an intentional, reviewed behavior change, remove it from the \
                 divergence list and confirm the adapter comparison passes for it instead.",
                path.display()
            ),
            CheckOutcome::Skipped(reason) => {
                println!("SKIPPED [Crate] {}: {reason}", path.display());
            }
        }
        return;
    }

    let was_valid = matches!(outcome, CheckOutcome::Valid);
    assert_outcome(outcome, true, checker, &path);

    // Beyond accept/reject, also check *what* each producing checker parsed this fixture into
    // against the checked-in conformance/condarc/expected/*.json -- see the "expected internal
    // representation" sections above. Only conda and the crate produce a value today (openapi
    // is schema-only, accept/reject), and only once each has actually accepted the fixture
    // (nothing to compare a rejection against).
    if was_valid {
        match checker {
            Checker::Conda => assert_conda_expected_representation(&path, &value),
            Checker::Crate => assert_crate_expected_representation(&path, &value),
            Checker::OpenApi => {}
        }
    }
}

#[rstest]
fn invalid_condarc_is_rejected(
    #[files("conformance/condarc/invalid/*.json")] path: PathBuf,
    #[values(Checker::Conda, Checker::Crate, Checker::OpenApi)] checker: Checker,
) {
    let value = load_fixture(&path);

    // Explode multi-key fixtures into one independent check per key
    // (see `invalid_cases`'s docs) so a checker that stops at the
    // first bad field can't hide a bug in one of the others.
    for case in invalid_cases(&path, value) {
        let outcome = checker.check(&case.value, &path);
        assert_outcome_for_case(outcome, false, checker, &path, Some(&case.label));
    }
}
