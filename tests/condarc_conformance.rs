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
//!   - **crate**: the not-yet-implemented `condarc` crate (GEN-36).
//!     Currently always [`CheckOutcome::Skipped`] -- see
//!     [`check_crate`].
//!   - **openapi**: validates against `docs/condarc_openapi.json` (also
//!     not yet implemented) using the `jsonschema` crate.
//!
//! Each checker is automatically skipped (not failed) when its backend
//! is unavailable (no `conda`-capable python found / crate not
//! implemented / schema file missing), and can additionally be
//! force-skipped via `ALLEZ_CONFORMANCE_SKIP_CONDA` /
//! `ALLEZ_CONFORMANCE_SKIP_CRATE` / `ALLEZ_CONFORMANCE_SKIP_OPENAPI`
//! (set to `1` or `true`). See the `Makefile`'s `conformance-*` targets
//! for running one checker at a time.

use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use rstest::rstest;
use serde_json::Value;

// ---------------------------------------------------------------------
// Checkers
// ---------------------------------------------------------------------

/// The three independent "is this .condarc valid" oracles under test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Checker {
    /// Real conda, driven via its own Python `conda.base.context` API.
    Conda,
    /// The not-yet-implemented `condarc` Rust crate (GEN-36).
    Crate,
    /// `docs/condarc_openapi.json`, validated via the `jsonschema` crate.
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

    fn check(self, value: &Value) -> CheckOutcome {
        if self.is_force_skipped() {
            return CheckOutcome::Skipped(format!("force-skipped via {}=1", self.skip_env_var()));
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
/// order): `$ALLEZ_CONFORMANCE_PYTHON`, a bare `python3` on `PATH`,
/// then the interpreter shipped alongside whatever `conda` executable
/// is on `PATH` (a bare `python3` on `PATH` is frequently a
/// *different*, conda-less interpreter than the one conda itself
/// runs under).
fn find_conda_python() -> Option<PathBuf> {
    if let Ok(p) = env::var("ALLEZ_CONFORMANCE_PYTHON") {
        let p = PathBuf::from(p);
        return python_has_conda(&p).then_some(p);
    }

    let candidate = PathBuf::from("python3");
    if python_has_conda(&candidate) {
        return Some(candidate);
    }

    let conda_path = which_conda()?;
    let dir = conda_path.parent()?;
    for name in ["python3", "python"] {
        let candidate = dir.join(name);
        if candidate.is_file() && python_has_conda(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn check_conda(value: &Value) -> CheckOutcome {
    let Some(python) = find_conda_python() else {
        return CheckOutcome::Skipped(
            "no python interpreter with `conda` importable found (checked \
             $ALLEZ_CONFORMANCE_PYTHON, `python3` on PATH, and the interpreter \
             shipped alongside `conda` on PATH)"
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
            return CheckOutcome::Skipped(format!("failed to create temp fixture file: {err}"));
        }
    };
    let text = serde_json::to_string(value).expect("fixture value should serialize to JSON");
    if let Err(err) = fixture.write_all(text.as_bytes()) {
        return CheckOutcome::Skipped(format!("failed to write temp fixture file: {err}"));
    }

    let output = conda_free_command(&python)
        .args(["-c", CONDA_CHECK_SCRIPT])
        .arg(fixture.path())
        .output();

    let output = match output {
        Ok(o) => o,
        Err(err) => {
            return CheckOutcome::Skipped(format!("failed to run conda oracle subprocess: {err}"));
        }
    };

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    match output.status.code() {
        Some(0) => CheckOutcome::Valid,
        Some(1) => CheckOutcome::Invalid(stderr),
        code => CheckOutcome::Skipped(format!(
            "conda oracle exited unexpectedly (status={code:?}): {stderr}"
        )),
    }
}

// ---------------------------------------------------------------------
// crate checker
// ---------------------------------------------------------------------

/// The `condarc` crate (GEN-36) doesn't exist yet. Kept as a single,
/// isolated function so wiring in the real implementation later --
/// parse `value` with the real crate and map its result/error onto
/// [`CheckOutcome`] -- is a small, self-contained diff with no other
/// changes required in this file.
fn check_crate(_value: &Value) -> CheckOutcome {
    CheckOutcome::Skipped("condarc crate not implemented yet (GEN-36)".to_string())
}

// ---------------------------------------------------------------------
// openapi checker
// ---------------------------------------------------------------------

fn openapi_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/condarc_openapi.json")
}

fn check_openapi(value: &Value) -> CheckOutcome {
    let schema_path = openapi_schema_path();
    let schema_text = match std::fs::read_to_string(&schema_path) {
        Ok(text) => text,
        Err(_) => {
            return CheckOutcome::Skipped(format!("{} does not exist yet", schema_path.display()));
        }
    };
    let schema: Value = match serde_json::from_str(&schema_text) {
        Ok(v) => v,
        Err(err) => {
            return CheckOutcome::Skipped(format!(
                "{} is not valid JSON: {err}",
                schema_path.display()
            ));
        }
    };
    let validator = match jsonschema::validator_for(&schema) {
        Ok(v) => v,
        Err(err) => {
            return CheckOutcome::Skipped(format!(
                "{} is not a valid JSON Schema: {err}",
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
    if !is_combined_fixture(path) {
        if let Value::Object(obj) = value {
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
    let outcome = checker.check(&value);
    assert_outcome(outcome, true, checker, &path);
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
        let outcome = checker.check(&case.value);
        assert_outcome_for_case(outcome, false, checker, &path, Some(&case.label));
    }
}
