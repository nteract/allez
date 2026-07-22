#![warn(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

//! `allez` CLI entry point: parses argv, initializes observability, and
//! dispatches to each subcommand's stub acknowledgment.

mod cli;
mod error;
mod observability;
mod output;

use clap::Parser;
use clap::error::ErrorKind;

use cli::{Cli, Commands};
use error::AllezError;

/// Parses argv and dispatches to the matched subcommand's stub handler.
///
/// Uses [`Cli::try_parse`] (not the auto-exiting `Cli::parse`) so a usage
/// error is rendered through [`output::render_error`] instead of clap's own
/// default print-and-exit. `--help`/`--version`/`--human`-alone/the
/// no-args-given help (`arg_required_else_help`) bypass `output.rs`
/// entirely per contracts/cli-schema.md's format-selection exemption:
/// [`clap::Error::print`] already writes each of these kinds to the
/// correct stream (stdout for help/version, stderr for the
/// missing-subcommand cases) with the correct exit code — `MissingSubcommand`
/// is grouped with the other three so that "no subcommand given" behaves
/// identically whether or not an unrelated global flag (`--human`,
/// `--verbose`) happened to be present (closes the T055-adjacent UX
/// inconsistency found during review: `allez --human` alone used to render
/// a terse JSON error instead of the same full help dump bare `allez`
/// produces). The [`ErrorKind`] → [`AllezError`] mapping below covers every
/// `ErrorKind` this CLI's own argument shapes can actually produce (T042),
/// verified empirically against the real binary for each of T031-T037's
/// scenarios; the handful of variants clap defines that this CLI has no
/// path to trigger (e.g. `ArgumentConflict`, `NoEquals`) are still mapped
/// explicitly for robustness, alongside the mandatory `_` arm
/// `ErrorKind`'s `#[non_exhaustive]` attribute requires.
///
/// `observability::init` is called unconditionally, before parsing, using
/// [`human_requested_in_argv`] rather than a post-parse `cli.human` — this
/// closes the Constitution XI gap where `RUST_LOG` previously had no effect
/// at all on the parse-error path (the subscriber was only initialized in
/// the `Ok(cli)` branch).
fn main() {
    let human = human_requested_in_argv();
    observability::init(human);
    match Cli::try_parse() {
        Ok(cli) => dispatch(cli),
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
/// the first literal `--` (per FR-012, a token after `--` is never
/// reinterpreted as an allez flag, even for this scan). Used only when
/// `Cli::try_parse()` itself failed — in every other case `cli.human` is
/// already the authoritative, clap-parsed value. Without this, a failed
/// parse would always render its error as JSON regardless of `--human`,
/// contradicting contracts/cli-schema.md's "format selection applies
/// uniformly... including error output" (only `--help`/`--version` are
/// exempted, and neither reaches this function).
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

/// Exhaustive mapping from clap's [`ErrorKind`] to one of the four fixed
/// [`AllezError`] categories (FR-017), used only for parse-time usage
/// errors (see [`main`]'s docs).
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
/// the `error: ` prefix and the trailing usage/help block).
///
/// clap's rendered text already distinguishes an unrecognized flag from an
/// unconsumable extra positional argument (both map to the same
/// [`AllezError::UnknownFlag`] category under FR-017's fixed enum, but the
/// *message* text shouldn't falsely claim "flag" when the actual problem
/// was an unexpected argument) — using it directly, instead of
/// [`AllezError`]'s fixed per-category `Display` string, fixes that
/// mislabeling without changing the stable `category` field callers match
/// on.
fn clap_parse_error_message(e: &clap::Error) -> String {
    e.render()
        .to_string()
        .lines()
        .next()
        .unwrap_or_default()
        .trim_start_matches("error: ")
        .to_string()
}

/// Renders a usage error to stderr and exits `2` (FR-010, FR-011).
///
/// Shared by [`dispatch`]'s `Oneshot`/`Run`/`Sandbox` arms (their
/// respective pass-through-command validation, T039/T040A) and by
/// [`main`]'s own `Cli::try_parse()` error handling (T042) — one rendering
/// path for both dispatch-layer and parse-layer usage errors. `category`
/// and `message` are taken separately (see [`output::render_error`]) so
/// dispatch-layer callers can pass their [`AllezError`]'s own fixed text
/// while [`main`] passes clap's more precise message for parse errors.
fn exit_with_error(category: &str, message: &str, human: bool) -> ! {
    eprintln!("{}", output::render_error(category, message, human));
    std::process::exit(2);
}

/// Routes a successfully-parsed [`Cli`] to its subcommand's stub handler
/// (FR-009), each defined in its own `src/cli/<subcommand>.rs` module.
///
/// The `Oneshot`/`Run` arms call [`cli::validate_pass_through`] immediately
/// after `split_program()`, before invoking the handler (T039); a missing
/// pass-through command exits `2` without the handler ever running. The
/// `Sandbox` arm calls [`cli::sandbox_missing_command`] instead (T040),
/// since its required-ness is conditional on `--` presence rather than
/// unconditional — this preserves the no-`--`-at-all interactive-subshell
/// path (exit `0`) while rejecting `--` present with nothing after it.
///
/// Each arm emits one `tracing` event (Constitution Principle XI) with the
/// `operation` field naming the subcommand and, on success, a `result`
/// field — counts only, never the pass-through command's own redacted
/// `program`/`args` content, to keep the same secrecy guarantee FR-016
/// gives the stdout payload.
fn dispatch(cli: Cli) {
    let operation = cli.command.name();
    match cli.command {
        Commands::Oneshot(mut args) => {
            args.pass_through.split_program();
            if let Err(err) = cli::validate_pass_through(&args.pass_through) {
                tracing::warn!(operation, category = err.category(), "usage error");
                exit_with_error(err.category(), &err.to_string(), cli.human);
            }
            tracing::info!(
                operation,
                packages = args.packages.len(),
                result = "stub_success"
            );
            println!("{}", cli::oneshot::run(&args, cli.human, cli.verbose));
        }
        Commands::Create(args) => {
            tracing::info!(
                operation,
                packages = args.packages.len(),
                result = "stub_success"
            );
            println!("{}", cli::create::run(&args, cli.human));
        }
        Commands::Run(mut args) => {
            args.pass_through.split_program();
            if let Err(err) = cli::validate_pass_through(&args.pass_through) {
                tracing::warn!(operation, category = err.category(), "usage error");
                exit_with_error(err.category(), &err.to_string(), cli.human);
            }
            tracing::info!(operation, result = "stub_success");
            println!("{}", cli::run::run(&args, cli.human, cli.verbose));
        }
        Commands::Sandbox(mut sandbox_args) => {
            sandbox_args.pass_through.split_program();
            let raw_args: Vec<std::ffi::OsString> = std::env::args_os().collect();
            if let Err(err) = cli::sandbox_missing_command(&sandbox_args.pass_through, &raw_args) {
                tracing::warn!(operation, category = err.category(), "usage error");
                exit_with_error(err.category(), &err.to_string(), cli.human);
            }
            tracing::info!(operation, result = "stub_success");
            println!(
                "{}",
                cli::sandbox::run(&sandbox_args.pass_through, cli.human, cli.verbose)
            );
        }
        Commands::List => {
            tracing::info!(operation, result = "stub_success");
            println!("{}", cli::list::run(cli.human));
        }
        Commands::Remove(args) => {
            tracing::info!(operation, result = "stub_success");
            println!("{}", cli::remove::run(&args, cli.human));
        }
    }
}
