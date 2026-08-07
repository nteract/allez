use assert_cmd::Command;
use predicates::prelude::*;
use rstest::rstest;
use serde_json::Value;

/// Runs the compiled `allez` binary; returns `(exit_code, stdout, stderr)`.
fn run_allez(args: &[&str]) -> (i32, String, String) {
    let output = Command::cargo_bin("allez")
        .expect("allez binary should build")
        .args(args)
        .output()
        .expect("allez should run");
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (code, stdout, stderr)
}

const SUBCOMMANDS: [&str; 6] = ["oneshot", "create", "run", "sandbox", "list", "remove"];

#[test]
fn t011_help_lists_all_six_subcommands_with_usage() {
    let mut cmd = Command::cargo_bin("allez").expect("allez binary should build");
    let mut assert = cmd
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
    for name in SUBCOMMANDS {
        assert = assert.stdout(predicate::str::contains(name));
    }
}

#[test]
fn t012_no_args_exits_2_with_usage_on_stderr() {
    Command::cargo_bin("allez")
        .expect("allez binary should build")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("Usage").or(predicate::str::contains("usage")));
}

#[rstest]
#[case("oneshot")]
#[case("create")]
#[case("run")]
#[case("sandbox")]
#[case("list")]
#[case("remove")]
fn t013_subcommand_help_exits_0_with_usage(#[case] subcommand: &str) {
    Command::cargo_bin("allez")
        .expect("allez binary should build")
        .args([subcommand, "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}

#[test]
fn t013a_version_exits_0_and_prints_version_string() {
    Command::cargo_bin("allez")
        .expect("allez binary should build")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

/// Confirms `--help`/`--version` output is never JSON: the existing
/// discoverability tests only assert plain-text substrings are present,
/// none of them positively rule out JSON.
#[rstest]
#[case(&["--help"])]
#[case(&["--version"])]
#[case(&["list", "--help"])]
fn t048_help_and_version_output_is_never_json(#[case] args: &[&str]) {
    let (code, stdout, _stderr) = run_allez(args);
    assert_eq!(code, 0, "args={args:?}");
    assert!(
        serde_json::from_str::<Value>(&stdout).is_err(),
        "args={args:?} stdout should be plain text, not JSON: {stdout}"
    );
}

#[test]
fn t017_create_with_path_and_packages() {
    let (code, stdout, _stderr) = run_allez(&["create", "./my-env", "pkg1", "pkg2"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(v["parsed"]["path"], "./my-env");
    assert_eq!(v["parsed"]["packages"], serde_json::json!(["pkg1", "pkg2"]));
}

#[test]
fn t017a_create_zero_packages_is_valid_not_error() {
    let (code, stdout, _stderr) = run_allez(&["create", "./my-env"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(v["parsed"]["path"], "./my-env");
    assert_eq!(v["parsed"]["packages"], serde_json::json!([]));
}

#[test]
fn t018_run_with_path_and_command_identifies_parsed_values() {
    let (code, stdout, _stderr) =
        run_allez(&["run", "./my-env", "--verbose", "--", "echo", "hello"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(v["parsed"]["path"], "./my-env");
    assert_eq!(v["parsed"]["pass_through"]["program"], "echo");
    assert_eq!(
        v["parsed"]["pass_through"]["args"],
        serde_json::json!(["hello"])
    );
}

#[test]
fn t019_sandbox_with_command_identifies_parsed_values() {
    let (code, stdout, _stderr) =
        run_allez(&["sandbox", "--verbose", "--", "python", "-c", "print(1)"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(v["parsed"]["pass_through"]["program"], "python");
    assert_eq!(
        v["parsed"]["pass_through"]["args"],
        serde_json::json!(["-c", "print(1)"])
    );
}

#[test]
fn t020_sandbox_without_command_routes_to_interactive_subshell() {
    let (code, stdout, _stderr) = run_allez(&["sandbox"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(v["parsed"]["interactive_subshell"], true);
}

#[test]
fn t021_list_requires_no_positional_arguments() {
    let (code, stdout, _stderr) = run_allez(&["list"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(v["subcommand"], "list");
}

#[test]
fn t022_remove_with_path() {
    let (code, stdout, _stderr) = run_allez(&["remove", "./my-env"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(v["parsed"]["path"], "./my-env");
}

#[test]
fn t023_list_default_json_matches_fixed_minimal_shape() {
    let (code, stdout, _stderr) = run_allez(&["list"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v["schema_version"].is_string());
    assert_eq!(v["subcommand"], "list");
    assert_eq!(v["status"], "stub");
    assert_eq!(v["parsed"], serde_json::json!({}));
}

#[test]
fn t023b_create_default_json_matches_fixed_shape() {
    let (code, stdout, _stderr) = run_allez(&["create", "./my-env"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v["schema_version"].is_string());
    assert_eq!(v["subcommand"], "create");
    assert_eq!(v["status"], "stub");
    assert_eq!(
        v["parsed"],
        serde_json::json!({"path": "./my-env", "packages": []})
    );
}

#[test]
fn t023c_run_default_json_matches_fixed_shape() {
    let (code, stdout, _stderr) = run_allez(&["run", "./my-env", "--", "echo", "hi"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v["schema_version"].is_string());
    assert_eq!(v["subcommand"], "run");
    assert_eq!(v["status"], "stub");
    assert_eq!(
        v["parsed"],
        serde_json::json!({
            "path": "./my-env",
            "pass_through": {"program": "<redacted>", "arg_count": 1}
        })
    );
}

#[test]
fn t023d_sandbox_default_json_matches_fixed_shape_when_no_command() {
    let (code, stdout, _stderr) = run_allez(&["sandbox"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v["schema_version"].is_string());
    assert_eq!(v["subcommand"], "sandbox");
    assert_eq!(v["status"], "stub");
    assert_eq!(
        v["parsed"],
        serde_json::json!({"interactive_subshell": true})
    );
}

#[test]
fn t023e_remove_default_json_matches_fixed_shape() {
    let (code, stdout, _stderr) = run_allez(&["remove", "./my-env"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v["schema_version"].is_string());
    assert_eq!(v["subcommand"], "remove");
    assert_eq!(v["status"], "stub");
    assert_eq!(v["parsed"], serde_json::json!({"path": "./my-env"}));
}

#[rstest]
#[case(&["run", "./my-env"], &["echo", "hello"])]
#[case(&["sandbox"], &["echo", "hello"])]
fn t023f_pass_through_redacted_by_default_json_and_human(
    #[case] prefix: &[&str],
    #[case] command_args: &[&str],
) {
    for human in [false, true] {
        let mut args: Vec<&str> = prefix.to_vec();
        if human {
            args.push("--human");
        }
        args.push("--");
        args.extend_from_slice(command_args);
        let (code, stdout, _stderr) = run_allez(&args);
        assert_eq!(code, 0, "args={args:?}");
        assert!(
            !stdout.contains("echo"),
            "program leaked into redacted output: {stdout}"
        );
        assert!(
            !stdout.contains("hello"),
            "arg leaked into redacted output: {stdout}"
        );
        assert!(
            stdout.contains("<redacted>") || stdout.contains("arg_count"),
            "expected redaction marker in output: {stdout}"
        );
    }
}

#[rstest]
#[case(&["run", "./my-env"], &["python", "-c", "print(1)"])]
#[case(&["sandbox"], &["python", "-c", "print(1)"])]
fn t023g_verbose_reveals_unredacted_pass_through_json_and_human(
    #[case] prefix: &[&str],
    #[case] command_args: &[&str],
) {
    for human in [false, true] {
        let mut args: Vec<&str> = prefix.to_vec();
        args.push("--verbose");
        if human {
            args.push("--human");
        }
        args.push("--");
        args.extend_from_slice(command_args);
        let (code, stdout, _stderr) = run_allez(&args);
        assert_eq!(code, 0, "args={args:?}");
        assert!(stdout.contains("python"), "missing program: {stdout}");
        assert!(stdout.contains("-c"), "missing flag-like arg: {stdout}");
        assert!(stdout.contains("print(1)"), "missing arg: {stdout}");
        assert!(
            !stdout.contains("<redacted>"),
            "unexpected redaction marker: {stdout}"
        );
    }
}

#[test]
fn t023h_human_flag_before_subcommand_matches_after_for_list() {
    let (code_before, stdout_before, _) = run_allez(&["--human", "list"]);
    let (code_after, stdout_after, _) = run_allez(&["list", "--human"]);
    assert_eq!(code_before, 0);
    assert_eq!(code_after, 0);
    assert_eq!(stdout_before, stdout_after);
}

#[test]
fn t023i_human_flag_before_subcommand_matches_after_for_oneshot_with_args() {
    // Uses the empty-separator usage-error path (never reaches
    // `create_ephemeral_environment`, since `run_allez` has no
    // `ALLEZ_CONDARC_PATH`/`ALLEZ_EPHEMERAL_ROOT` isolation) rather than a
    // real pass-through invocation: FR-013 forbids any envelope once the
    // pass-through program starts, so there is no unredacted payload left
    // to compare positions of `--human` against.
    let (code_before, stdout_before, stderr_before) =
        run_allez(&["--human", "oneshot", "pkg1", "--"]);
    let (code_after, stdout_after, stderr_after) = run_allez(&["oneshot", "pkg1", "--human", "--"]);
    assert_eq!(code_before, 2);
    assert_eq!(code_after, 2);
    assert_eq!(stdout_before, stdout_after);
    assert_eq!(stderr_before, stderr_after);
}

/// Runs `allez` with `args`, asserts exit code `2`, an empty stdout, and a
/// JSON error body on stderr matching `expected_category`. Returns the
/// parsed error body for callers that need to inspect it further.
fn assert_usage_error(args: &[&str], expected_category: &str) -> Value {
    let (code, stdout, stderr) = run_allez(args);
    assert_eq!(code, 2, "args={args:?}");
    assert!(stdout.is_empty(), "args={args:?} stdout={stdout}");
    let v: Value = serde_json::from_str(&stderr).expect("valid JSON error body on stderr");
    assert_eq!(v["category"], expected_category, "args={args:?}");
    assert!(v["schema_version"].is_string());
    assert!(v["message"].is_string());
    v
}

#[test]
fn t031_create_no_path_exits_2_missing_argument() {
    assert_usage_error(&["create"], "missing_argument");
}

#[rstest]
#[case(&["create", ""])]
#[case(&["run", "", "--", "echo", "hi"])]
#[case(&["remove", ""])]
fn t031a_empty_string_path_exits_2_missing_argument(#[case] args: &[&str]) {
    assert_usage_error(args, "missing_argument");
}

#[test]
fn t032_remove_no_path_exits_2_missing_argument() {
    assert_usage_error(&["remove"], "missing_argument");
}

#[test]
fn t033_unrecognized_subcommand_exits_2_unknown_subcommand() {
    let v = assert_usage_error(&["frobnicate"], "unknown_subcommand");
    let message = v["message"].as_str().expect("message is a string");
    assert!(
        message.to_lowercase().contains("subcommand"),
        "message should indicate the subcommand is unknown: {message}"
    );
}

#[test]
fn t033a_case_mismatched_subcommand_exits_2_unknown_subcommand() {
    assert_usage_error(&["List"], "unknown_subcommand");
}

#[test]
fn t034_oneshot_empty_separator_exits_2_missing_pass_through_command() {
    assert_usage_error(&["oneshot", "pkg1", "--"], "missing_pass_through_command");
}

#[test]
fn t034a_oneshot_without_separator_exits_2_missing_pass_through_command() {
    assert_usage_error(&["oneshot", "pkg1"], "missing_pass_through_command");
}

#[test]
fn t035_run_without_separator_exits_2_missing_pass_through_command() {
    assert_usage_error(&["run", "./my-env"], "missing_pass_through_command");
}

#[test]
fn t035a_run_empty_separator_exits_2_missing_pass_through_command() {
    assert_usage_error(&["run", "./my-env", "--"], "missing_pass_through_command");
}

#[test]
fn t036_sandbox_empty_separator_exits_2_distinct_from_no_separator_exit_0() {
    assert_usage_error(&["sandbox", "--"], "missing_pass_through_command");

    let (code, stdout, _stderr) = run_allez(&["sandbox"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(v["parsed"]["interactive_subshell"], true);
}

#[test]
fn t037_unknown_flag_exits_2_unknown_flag() {
    assert_usage_error(&["list", "--bogus-flag"], "unknown_flag");
}

#[rstest]
#[case(&["create"], "missing_argument")]
#[case(&["create", ""], "missing_argument")]
#[case(&["run", "", "--", "echo", "hi"], "missing_argument")]
#[case(&["remove", ""], "missing_argument")]
#[case(&["remove"], "missing_argument")]
#[case(&["frobnicate"], "unknown_subcommand")]
#[case(&["List"], "unknown_subcommand")]
#[case(&["oneshot", "pkg1", "--"], "missing_pass_through_command")]
#[case(&["run", "./my-env"], "missing_pass_through_command")]
#[case(&["run", "./my-env", "--"], "missing_pass_through_command")]
#[case(&["sandbox", "--"], "missing_pass_through_command")]
#[case(&["list", "--bogus-flag"], "unknown_flag")]
fn t038_all_usage_errors_share_exit_code_category_and_stderr_convention(
    #[case] args: &[&str],
    #[case] expected_category: &str,
) {
    assert_usage_error(args, expected_category);

    // `--human` mode: still exit 2 and empty stdout, but the stderr
    // message is no longer JSON, so it isn't re-parsed as such.
    let mut human_args: Vec<&str> = vec!["--human"];
    human_args.extend_from_slice(args);
    let (code, stdout, stderr) = run_allez(&human_args);
    assert_eq!(code, 2, "human args={human_args:?}");
    assert!(
        stdout.is_empty(),
        "human args={human_args:?} stdout={stdout}"
    );
    assert!(
        !stderr.is_empty(),
        "human args={human_args:?} should still emit a message on stderr"
    );
}

#[test]
fn t038a_oneshot_empty_separator_rejected_as_usage_error() {
    // Only the empty-separator usage-error half of the shared
    // `run`/`sandbox` parametrization below applies to `oneshot`: the
    // real-invocation, `--verbose`-pass-through half would require a
    // controlled channel/root this file's own `run_allez` (no
    // `ALLEZ_CONDARC_PATH`/`ALLEZ_EPHEMERAL_ROOT` isolation) cannot
    // provide safely.
    assert_usage_error(&["oneshot", "pkg1", "--"], "missing_pass_through_command");
}

#[rstest]
#[case(&["run", "./my-env"])]
#[case(&["sandbox"])]
fn t038a_empty_separator_and_flag_like_token_preservation_uniform_across_pass_through_subcommands(
    #[case] prefix: &[&str],
) {
    let mut empty_sep_args: Vec<&str> = prefix.to_vec();
    empty_sep_args.push("--");
    assert_usage_error(&empty_sep_args, "missing_pass_through_command");

    let mut verbose_args: Vec<&str> = prefix.to_vec();
    verbose_args.push("--verbose");
    verbose_args.push("--");
    verbose_args.extend_from_slice(&["python", "-c", "print(1)"]);
    let (code, stdout, _stderr) = run_allez(&verbose_args);
    assert_eq!(code, 0, "args={verbose_args:?}");
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let pass_through = &v["parsed"]["pass_through"];
    assert_eq!(pass_through["program"], "python");
    assert_eq!(pass_through["args"], serde_json::json!(["-c", "print(1)"]));
}

#[rstest]
#[case(&["run", "./my-env"], &["echo", "--verbose"])]
#[case(&["sandbox"], &["printf", "--", "human"])]
fn t038b_global_flag_spelled_tokens_after_separator_are_forwarded_verbatim_not_reinterpreted(
    #[case] prefix: &[&str],
    #[case] command_args: &[&str],
) {
    let mut args: Vec<&str> = prefix.to_vec();
    args.push("--verbose");
    args.push("--");
    args.extend_from_slice(command_args);
    let (code, stdout, _stderr) = run_allez(&args);
    assert_eq!(code, 0, "args={args:?}");
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let pass_through = &v["parsed"]["pass_through"];
    let program = pass_through["program"]
        .as_str()
        .expect("program is a string");
    assert_eq!(program, command_args[0]);
    let forwarded_args: Vec<String> = pass_through["args"]
        .as_array()
        .expect("args is an array")
        .iter()
        .map(|value| value.as_str().expect("arg is a string").to_string())
        .collect();
    assert_eq!(forwarded_args, command_args[1..]);
}

/// Extra, unconsumable positional arguments (`list foo`, `remove ./env
/// extra`, `sandbox foo`) previously rendered `message: "unrecognized
/// flag"` even though no flag was involved — clap's `UnknownArgument` kind
/// covers both cases, but the fixed per-category `AllezError::Display`
/// text used to hardcode flag-specific wording. `category` stays
/// `unknown_flag`, but `message` must now come from clap's own precise
/// text instead.
#[rstest]
#[case(&["list", "foo"])]
#[case(&["remove", "./my-env", "extra"])]
#[case(&["sandbox", "foo"])]
fn t059_extra_positional_argument_message_does_not_claim_flag(#[case] args: &[&str]) {
    let v = assert_usage_error(args, "unknown_flag");
    let message = v["message"].as_str().expect("message is a string");
    assert!(
        !message.to_lowercase().contains("flag"),
        "args={args:?} message should not blame a flag for an extra \
         positional argument: {message}"
    );
    assert!(
        message.to_lowercase().contains("argument"),
        "args={args:?} message should mention the unexpected argument: {message}"
    );
}

/// `oneshot`/`run`/`sandbox --help` must literally name `COMMAND` in their
/// usage synopsis, matching contracts/cli-schema.md's documented
/// `-- <COMMAND> [COMMAND ARGS...]` shape — previously all three showed
/// only the generic `[-- <ARGS>...]`.
#[rstest]
#[case("oneshot")]
#[case("run")]
#[case("sandbox")]
fn t060_pass_through_help_names_command_distinctly(#[case] subcommand: &str) {
    let (code, stdout, _stderr) = run_allez(&[subcommand, "--help"]);
    assert_eq!(code, 0, "subcommand={subcommand}");
    assert!(
        stdout.contains("<COMMAND>"),
        "subcommand={subcommand} help text should name COMMAND: {stdout}"
    );
}

/// A repeated global boolean flag (before/after the subcommand, split, or
/// clustered as `-vv`) must be idempotent, not a usage error — previously
/// clap's `global = true` propagation treated any second occurrence as an
/// `ArgumentConflict`, exiting `2` for the ordinary, unsurprising act of
/// repeating a boolean flag.
#[rstest]
#[case(&["--human", "--human", "list"])]
#[case(&["--human", "list", "--human"])]
#[case(&["list", "-v", "-v"])]
#[case(&["list", "-vv"])]
fn t061_repeated_global_flags_are_idempotent_not_a_usage_error(#[case] args: &[&str]) {
    let (code, _stdout, stderr) = run_allez(args);
    assert_eq!(code, 0, "args={args:?} stderr={stderr}");
}

/// `allez --human` (or any global flag) alone, with no subcommand, must
/// behave identically to bare `allez` — clap's full multi-line usage/help
/// dump on stderr, exit `2` — rather than the terse one-line JSON/human
/// error `render_error` renders for every other usage error. Previously
/// these two "no subcommand given" cases diverged in presentation purely
/// because an unrelated flag happened to be present.
#[rstest]
#[case(&["--human"])]
#[case(&["--verbose"])]
#[case(&["-v"])]
fn t062_flag_only_no_subcommand_matches_bare_invocation_help_dump(#[case] args: &[&str]) {
    let (bare_code, bare_stdout, bare_stderr) = run_allez(&[]);
    let (code, stdout, stderr) = run_allez(args);
    assert_eq!(code, bare_code, "args={args:?}");
    assert_eq!(stdout, bare_stdout, "args={args:?}");
    assert!(
        stderr.contains("Usage") || stderr.contains("usage"),
        "args={args:?} stderr should be the full usage dump, not a terse \
         error: {stderr}"
    );
    // Not a byte-for-byte match with bare `allez`'s stderr (clap's
    // suggestion list can mention the flag actually given), so only
    // compare shape/exit code, not exact stderr text against bare_stderr.
    let _ = bare_stderr;
}

/// `RUST_LOG` must have zero effect on stderr when unset: emitting
/// `tracing::info!` events with a default, nonzero log level would
/// pollute the fixed single-JSON-object stderr contract every other
/// usage-error test above depends on.
#[test]
fn t063_rust_log_unset_stays_silent() {
    let output = assert_cmd::Command::cargo_bin("allez")
        .expect("allez binary should build")
        .arg("list")
        .env_remove("RUST_LOG")
        .output()
        .expect("allez should run");
    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty with RUST_LOG unset: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `RUST_LOG=debug` (or any explicit level) must produce at least one
/// structured event on stderr — `observability::init()` used to only be
/// called after a successful parse, with no `tracing::*!` call site
/// anywhere in `src/`.
#[test]
fn t063a_rust_log_debug_produces_structured_output() {
    let output = assert_cmd::Command::cargo_bin("allez")
        .expect("allez binary should build")
        .arg("list")
        .env("RUST_LOG", "debug")
        .output()
        .expect("allez should run");
    assert_eq!(output.status.code(), Some(0));
    assert!(
        !output.stderr.is_empty(),
        "RUST_LOG=debug should produce non-empty stderr"
    );
    let stderr_line: Value = serde_json::from_str(
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .next()
            .expect("at least one stderr line"),
    )
    .expect("stderr log line should be valid JSON (default, non---human formatter)");
    assert_eq!(stderr_line["fields"]["operation"], "list");
}

/// Same as t063a, but also confirms `RUST_LOG` still applies to
/// parse-time usage errors (previously it didn't: `observability::init()`
/// used to run only in the `Ok(cli)` branch).
#[test]
fn t063b_rust_log_debug_applies_to_parse_errors_too() {
    let output = assert_cmd::Command::cargo_bin("allez")
        .expect("allez binary should build")
        .arg("frobnicate")
        .env("RUST_LOG", "debug")
        .output()
        .expect("allez should run");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        !output.stderr.is_empty(),
        "RUST_LOG=debug should produce non-empty stderr even on a parse error"
    );
}
