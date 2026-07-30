use clap::{Args, Parser, Subcommand};
use std::ffi::{OsStr, OsString};

use crate::error::AllezError;

/// `allez create` subcommand.
pub mod create;
/// `allez list` subcommand.
pub mod list;
/// `allez oneshot` subcommand.
pub mod oneshot;
/// `allez remove` subcommand.
pub mod remove;
/// `allez run` subcommand.
pub mod run;
/// `allez sandbox` subcommand.
pub mod sandbox;

/// Rejects an empty path; returns it unchanged otherwise.
pub fn parse_nonempty_path(s: &str) -> Result<String, String> {
    if s.is_empty() {
        return Err("path must not be empty".to_string());
    }
    Ok(s.to_string())
}

/// Shared by `oneshot` (via [`PackagesAndCommandArgs`]), `run`, and
/// `sandbox`. clap only allows one positional argument per command to
/// carry `last = true`, so the pass-through command's name and its own
/// arguments can't be two separate clap-parsed fields — they're captured
/// together as `raw` and split lazily by [`Self::program`]/[`Self::args`].
#[derive(Debug, Clone, Default, Args)]
pub struct PassThroughArgs {
    /// Everything after `--`: the pass-through command's name followed by
    /// its own arguments.
    #[arg(last = true)]
    pub(crate) raw: Vec<String>,
}

impl PassThroughArgs {
    /// The pass-through command's name, if one was supplied.
    pub fn program(&self) -> Option<&str> {
        self.raw.first().map(String::as_str)
    }

    /// The pass-through command's own arguments (excludes the name).
    pub fn args(&self) -> &[String] {
        self.raw.get(1..).unwrap_or(&[])
    }
}

/// `override_usage` gives this subcommand its own literal
/// `-- <COMMAND> [ARGS...]` synopsis — it MUST live on this wrapper, not on
/// the shared [`PassThroughArgs`] type itself: `override_usage` on a
/// `#[command(flatten)]`-ed type's own container attributes leaks into
/// every flattening site, which would incorrectly show `sandbox`'s usage
/// line under `oneshot --help` (confirmed empirically against clap 4.6.x).
#[derive(Debug, Args)]
#[command(override_usage = "allez oneshot [PACKAGES]... -- <COMMAND> [ARGS...]")]
pub struct PackagesAndCommandArgs {
    /// Package name tokens; zero or more, not an error if empty.
    pub packages: Vec<String>,
    /// The pass-through command captured after `--`; required — enforced
    /// by [`validate_pass_through`] at the dispatch layer, not at the type
    /// level (`sandbox` flattens the same type but stays optional).
    #[command(flatten)]
    pub pass_through: PassThroughArgs,
}

/// `create`'s argument shape: independent of [`PassThroughArgs`]/
/// [`PackagesAndCommandArgs`] — `create` has no pass-through command.
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Target environment path; rejected if empty.
    #[arg(value_parser = parse_nonempty_path)]
    pub env_path: String,
    /// Package name tokens; zero or more, not an error if empty.
    pub packages: Vec<String>,
}

/// `run`'s argument shape.
#[derive(Debug, Args)]
#[command(override_usage = "allez run <PATH> -- <COMMAND> [ARGS...]")]
pub struct RunArgs {
    /// Target environment path; rejected if empty.
    #[arg(value_parser = parse_nonempty_path)]
    pub env_path: String,
    /// The pass-through command captured after `--`; required — enforced
    /// by [`validate_pass_through`] at the dispatch layer, same as
    /// `oneshot`.
    #[command(flatten)]
    pub pass_through: PassThroughArgs,
}

/// `remove`'s argument shape: a single required non-empty path.
#[derive(Debug, Args)]
pub struct RemoveArgs {
    /// Target environment path; rejected if empty.
    #[arg(value_parser = parse_nonempty_path)]
    pub env_path: String,
}

/// A dedicated wrapper (rather than using [`PassThroughArgs`] directly as
/// `sandbox`'s payload) purely so `sandbox` can carry its own
/// `override_usage` — the *optional* `[-- <COMMAND> [ARGS...]]` synopsis —
/// independent of `oneshot`/`run`'s *required* one, without either leaking
/// into the other (see [`PackagesAndCommandArgs`]'s doc comment).
#[derive(Debug, Args)]
#[command(override_usage = "allez sandbox [-- <COMMAND> [ARGS...]]")]
pub struct SandboxArgs {
    /// The pass-through command captured after `--`; optional — required
    /// only when `--` was present with nothing after it, enforced by
    /// [`sandbox_missing_command`] at the dispatch layer.
    #[command(flatten)]
    pub pass_through: PassThroughArgs,
}

/// The six subcommands `allez` routes to.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Run a pass-through command in an ephemeral, one-shot environment.
    Oneshot(PackagesAndCommandArgs),
    /// Create a persistent environment at a target path.
    Create(CreateArgs),
    /// Run a pass-through command in an existing environment.
    Run(RunArgs),
    /// Enter an interactive subshell, or run a one-shot pass-through command.
    Sandbox(SandboxArgs),
    /// List known environments.
    List,
    /// Remove an environment at a target path.
    Remove(RemoveArgs),
}

impl Commands {
    /// This subcommand's name, used as the `operation` field on
    /// `main.rs`'s `dispatch` function's `tracing` events (not an
    /// intra-doc link: `dispatch` lives in the `allez` binary target, not
    /// this library target, so `rustdoc` cannot resolve it from here).
    pub fn name(&self) -> &'static str {
        match self {
            Self::Oneshot(_) => "oneshot",
            Self::Create(_) => "create",
            Self::Run(_) => "run",
            Self::Sandbox(_) => "sandbox",
            Self::List => "list",
            Self::Remove(_) => "remove",
        }
    }
}

/// Top-level `allez` CLI: the six subcommands plus global output flags.
#[derive(Debug, Parser)]
#[command(version, subcommand_required = true, arg_required_else_help = true)]
pub struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Commands,
    /// Selects human-readable output; JSON is the default.
    // `overrides_with = "human"` makes a repeated occurrence (e.g.
    // `allez --human --human list`, or split before/after the subcommand
    // name) replace rather than conflict with the earlier one — without
    // it, clap's `global = true` propagation treats any second occurrence
    // of the same global boolean flag as an `ArgumentConflict` usage error
    // (confirmed empirically against clap 4.6.x), which is a surprising,
    // undocumented failure mode for a plain boolean flag. Plain `//`
    // rather than `///` deliberately: this is implementation rationale,
    // not user-facing help text — clap renders the whole doc comment as
    // `--help` output, and end users don't need clap-internals trivia.
    #[arg(long, global = true, overrides_with = "human")]
    pub human: bool,
    /// Reveals redacted pass-through command content.
    // See `human`'s `overrides_with` note above; the same rationale
    // applies here.
    #[arg(short = 'v', long, global = true, overrides_with = "verbose")]
    pub verbose: bool,
}

/// Called from `main.rs`'s dispatch, immediately after `Cli::try_parse()`
/// succeeds and before invoking either handler's core logic. Not reused
/// unmodified for `sandbox`, whose required-ness is conditional on `--`
/// presence rather than unconditional — `sandbox` uses
/// [`sandbox_missing_command`] instead.
pub fn validate_pass_through(pt: &PassThroughArgs) -> Result<(), AllezError> {
    if pt.program().is_none() {
        return Err(AllezError::MissingPassThroughCommand);
    }
    Ok(())
}

/// Required only when a literal `--` token was present in argv with
/// nothing after it; absent entirely, `sandbox` routes to the
/// interactive-subshell path instead (not an error).
///
/// Only inspects `raw_args` when `pt.program()` is `None` — clap's own
/// `ArgMatches` state for a `#[arg(last = true)]` field is identical for
/// "no `--` at all" and "`--` present, nothing after it" (empirically
/// confirmed against `clap` 4.6.x), so this raw-argv scan is the only way
/// to tell the two apart. `raw_args` is a parameter (not read internally
/// via [`std::env::args_os`]) so this function stays unit-testable with
/// fake argv slices.
pub fn sandbox_missing_command(
    pt: &PassThroughArgs,
    raw_args: &[OsString],
) -> Result<(), AllezError> {
    if pt.program().is_some() {
        return Ok(());
    }
    if raw_args.iter().any(|a| a == OsStr::new("--")) {
        return Err(AllezError::MissingPassThroughCommand);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_nonempty_path_rejects_empty_string() {
        assert!(parse_nonempty_path("").is_err());
    }

    #[test]
    fn parse_nonempty_path_accepts_nonempty_string_unchanged() {
        assert_eq!(parse_nonempty_path("./my-env"), Ok("./my-env".to_string()));
    }

    #[test]
    fn parse_nonempty_path_treats_whitespace_only_as_nonempty() {
        assert_eq!(parse_nonempty_path(" "), Ok(" ".to_string()));
    }

    #[test]
    fn validate_pass_through_rejects_empty_program() {
        let pt = PassThroughArgs::default();
        assert_eq!(
            validate_pass_through(&pt),
            Err(AllezError::MissingPassThroughCommand)
        );
    }

    #[test]
    fn validate_pass_through_accepts_nonempty_program_without_args() {
        let pt = PassThroughArgs {
            raw: vec!["echo".to_string()],
        };
        assert_eq!(validate_pass_through(&pt), Ok(()));
    }

    #[test]
    fn validate_pass_through_accepts_nonempty_program_with_args() {
        let pt = PassThroughArgs {
            raw: vec!["echo".to_string(), "hi".to_string()],
        };
        assert_eq!(validate_pass_through(&pt), Ok(()));
    }

    #[test]
    fn sandbox_missing_command_rejects_empty_program_when_separator_present() {
        let pt = PassThroughArgs::default();
        let raw_args: Vec<OsString> = vec!["allez".into(), "sandbox".into(), "--".into()];
        assert_eq!(
            sandbox_missing_command(&pt, &raw_args),
            Err(AllezError::MissingPassThroughCommand)
        );
    }

    #[test]
    fn sandbox_missing_command_accepts_empty_program_when_separator_absent() {
        let pt = PassThroughArgs::default();
        let raw_args: Vec<OsString> = vec!["allez".into(), "sandbox".into()];
        assert_eq!(sandbox_missing_command(&pt, &raw_args), Ok(()));
    }

    #[test]
    fn sandbox_missing_command_accepts_nonempty_program_regardless_of_raw_args() {
        let pt = PassThroughArgs {
            raw: vec!["python".to_string()],
        };
        let raw_args: Vec<OsString> = vec!["allez".into(), "sandbox".into(), "--".into()];
        assert_eq!(sandbox_missing_command(&pt, &raw_args), Ok(()));
    }
}
