//! `allez` CLI entry point.

use clap::Parser;
use clap::error::ErrorKind;

use allez::{cli, error, observability, output};
use cli::oneshot::OneshotOutcome;
use cli::pass_through::PASS_THROUGH_EVENT_SCHEMA_VERSION;
use cli::{Cli, Commands};
use error::AllezError;

/// Uses [`Cli::try_parse`] (not the auto-exiting `Cli::parse`) so a usage
/// error is rendered through [`output::render_error`] instead of clap's own
/// default print-and-exit. `--help`/`--version`/`--human`-alone/the
/// no-args-given help bypass `output.rs` entirely: [`clap::Error::print`]
/// already writes each of these kinds to the correct stream (stdout for
/// help/version, stderr for the missing-subcommand cases) with the correct
/// exit code — grouping `MissingSubcommand` with the other three makes "no
/// subcommand given" behave identically whether or not an unrelated global
/// flag (`--human`, `--verbose`) happened to be present.
///
/// `observability::init` is called unconditionally, before parsing, using
/// [`human_requested_in_argv`] rather than a post-parse `cli.human` — this
/// is what makes `RUST_LOG` take effect even on the parse-error path (the
/// subscriber would otherwise only be initialized in the `Ok(cli)` branch).
///
/// `#[tokio::main]`: `cli::oneshot::run`'s own `create_ephemeral_environment`
/// call (GEN-24) is an `async fn` requiring an active Tokio runtime
/// `Handle`; `flavor = "multi_thread"` matches the `rt-multi-thread`
/// feature already enabled in `Cargo.toml`.
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let human = human_requested_in_argv();
    observability::init(human);
    match Cli::try_parse() {
        Ok(cli) => dispatch(cli).await,
        Err(e) => {
            if matches!(
                e.kind(),
                ErrorKind::DisplayHelp
                    | ErrorKind::DisplayVersion
                    | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
                    | ErrorKind::MissingSubcommand
            ) {
                let _ = e.print();
                std::process::exit(e.exit_code());
            }
            let allez_err = map_parse_error(e.kind());
            tracing::warn!(
                category = allez_err.category(),
                "rejected invocation: {}",
                e.kind()
            );
            exit_with_error(allez_err.category(), &clap_parse_error_message(&e), human);
        }
    }
}

/// Scans the real process argv for a literal `--human` token, stopping at
/// the first literal `--` (a token after `--` is never reinterpreted as an
/// allez flag, even for this scan). Used only when `Cli::try_parse()`
/// itself failed — in every other case `cli.human` is already the
/// authoritative, clap-parsed value. Without this, a failed parse would
/// always render its error as JSON regardless of `--human`.
fn human_requested_in_argv() -> bool {
    for arg in std::env::args_os().skip(1) {
        if arg == std::ffi::OsStr::new("--") {
            break;
        }
        if arg == std::ffi::OsStr::new("--human") {
            return true;
        }
    }
    false
}

fn map_parse_error(kind: ErrorKind) -> AllezError {
    match kind {
        ErrorKind::MissingRequiredArgument
        | ErrorKind::MissingSubcommand
        | ErrorKind::TooFewValues
        | ErrorKind::TooManyValues
        | ErrorKind::WrongNumberOfValues
        | ErrorKind::ValueValidation
        | ErrorKind::InvalidValue => AllezError::MissingArgument,
        ErrorKind::InvalidSubcommand => AllezError::UnknownSubcommand,
        ErrorKind::UnknownArgument | ErrorKind::NoEquals | ErrorKind::ArgumentConflict => {
            AllezError::UnknownFlag
        }
        _ => AllezError::UnknownFlag,
    }
}

/// Extracts clap's own concise error message (its first line, stripped of
/// the `error: ` prefix and the trailing usage/help block) instead of using
/// [`AllezError`]'s fixed per-category `Display` string — clap's rendered
/// text correctly distinguishes an unrecognized flag from an unconsumable
/// extra positional argument, which the fixed string can't, without
/// changing the stable `category` field callers match on.
fn clap_parse_error_message(e: &clap::Error) -> String {
    e.render()
        .to_string()
        .lines()
        .next()
        .unwrap_or_default()
        .trim_start_matches("error: ")
        .to_string()
}

fn exit_with_error(category: &str, message: &str, human: bool) -> ! {
    eprintln!("{}", output::render_error(category, message, human));
    std::process::exit(2);
}

/// Exits with a usage error when `result` (a pass-through-command
/// validation outcome) is `Err`; no-op otherwise. Shared by every
/// `dispatch` arm that validates a pass-through command before running
/// its handler.
fn exit_on_invalid_pass_through(
    operation: &'static str,
    result: Result<(), AllezError>,
    human: bool,
) {
    if let Err(err) = result {
        tracing::warn!(
            operation,
            category = err.category(),
            schema_version = PASS_THROUGH_EVENT_SCHEMA_VERSION,
            "usage error"
        );
        exit_with_error(err.category(), &err.to_string(), human);
    }
}

/// Traces a successful dispatch, then renders and prints the handler's
/// output. `render` is deferred (not a pre-computed `&str`) so the trace
/// fires before the handler runs, matching this crate's pre-existing
/// event-then-effect ordering elsewhere (e.g. `emit_failure` in
/// `ephemeral/mod.rs`, which also traces before performing rollback).
/// `packages` mirrors the per-arm `tracing::info!` field some handlers
/// (`oneshot`, `create`) attach and others omit.
fn emit_stub_success(
    operation: &'static str,
    packages: Option<usize>,
    render: impl FnOnce() -> String,
) {
    match packages {
        Some(count) => tracing::info!(operation, packages = count, result = "stub_success"),
        None => tracing::info!(operation, result = "stub_success"),
    }
    println!("{}", render());
}

/// The `Oneshot`/`Run` arms call [`cli::validate_pass_through`] before
/// invoking the handler; a missing pass-through command exits `2` without
/// the handler ever running. The `Sandbox` arm calls
/// [`cli::sandbox_missing_command`] instead, since its required-ness is
/// conditional on `--` presence rather than unconditional — this
/// preserves the no-`--`-at-all interactive-subshell path (exit `0`)
/// while still rejecting `--` present with nothing after it.
///
/// Each arm's `tracing` event never includes the pass-through command's
/// own redacted `program`/`args` content, keeping the same secrecy
/// guarantee the stdout payload already has.
async fn dispatch(cli: Cli) {
    let operation = cli.command.name();
    match cli.command {
        Commands::Oneshot(args) => {
            exit_on_invalid_pass_through(
                operation,
                cli::validate_pass_through(&args.pass_through),
                cli.human,
            );
            let outcome = cli::oneshot::run(&args, cli.human, cli.verbose).await;
            match &outcome {
                OneshotOutcome::EnvironmentCreationFailed { message }
                | OneshotOutcome::PassThroughFailed { message, .. } => eprintln!("{message}"),
                OneshotOutcome::PassThroughExited { .. } => {}
            }
            std::process::exit(outcome.exit_code());
        }
        Commands::Create(args) => {
            emit_stub_success(operation, Some(args.packages.len()), || {
                cli::create::run(&args, cli.human)
            });
        }
        Commands::Run(args) => {
            exit_on_invalid_pass_through(
                operation,
                cli::validate_pass_through(&args.pass_through),
                cli.human,
            );
            emit_stub_success(operation, None, || {
                cli::run::run(&args, cli.human, cli.verbose)
            });
        }
        Commands::Sandbox(sandbox_args) => {
            let raw_args: Vec<std::ffi::OsString> = std::env::args_os().collect();
            exit_on_invalid_pass_through(
                operation,
                cli::sandbox_missing_command(&sandbox_args.pass_through, &raw_args),
                cli.human,
            );
            emit_stub_success(operation, None, || {
                cli::sandbox::run(&sandbox_args.pass_through, cli.human, cli.verbose)
            });
        }
        Commands::List => {
            emit_stub_success(operation, None, || cli::list::run(cli.human));
        }
        Commands::Remove(args) => {
            emit_stub_success(operation, None, || cli::remove::run(&args, cli.human));
        }
    }
}
