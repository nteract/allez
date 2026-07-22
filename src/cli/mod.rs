//! CLI argument shapes: `Cli`, `Commands`, shared `Args` structs, and their
//! shared validators (`parse_nonempty_path`, `validate_pass_through`).

use clap::{Args, Parser, Subcommand};
use std::ffi::{OsStr, OsString};

use crate::error::AllezError;

pub mod create;
pub mod list;
pub mod oneshot;
pub mod remove;
pub mod run;
pub mod sandbox;

/// Rejects an empty path argument; returns the value unchanged otherwise.
///
/// Shared `value_parser` for `create`/`run`/`remove`'s path argument (FR-015).
///
/// ```rust,ignore
/// // Illustrative only (binary crate, no doctest target) — see this
/// // module's #[cfg(test)] tests for the executable equivalent.
/// assert_eq!(parse_nonempty_path("./my-env"), Ok("./my-env".to_string()));
/// assert!(parse_nonempty_path("").is_err());
/// ```
pub fn parse_nonempty_path(s: &str) -> Result<String, String> {
    if s.is_empty() {
        return Err("path must not be empty".to_string());
    }
    Ok(s.to_string())
}

/// The pass-through command and its own arguments, captured verbatim after
/// the `--` separator (FR-003, FR-005, FR-006, FR-012).
///
/// Shared by `oneshot` (via [`PackagesAndCommandArgs`]), `run`, and
/// `sandbox`. clap only allows one positional argument per command to carry
/// `last = true`, so `program` cannot itself be a second clap-parsed
/// positional alongside `args` — instead `args` captures every token after
/// `--` (including the would-be program name at index 0), and
/// [`PassThroughArgs::split_program`] must run once, immediately after
/// parsing succeeds, to move that first token into `program`.
#[derive(Debug, Clone, Default, Args)]
pub struct PassThroughArgs {
    /// The pass-through command's name (first token after `--`); empty when
    /// no pass-through command was supplied at all.
    #[arg(skip)]
    pub program: String,
    /// The pass-through command's own arguments (remaining tokens after
    /// `--`), captured verbatim — including anything that looks like a flag.
    #[arg(last = true)]
    pub args: Vec<String>,
}

impl PassThroughArgs {
    /// Moves the first captured token out of `args` and into `program`.
    ///
    /// Must be called once, immediately after `Cli::try_parse()` succeeds,
    /// before `program`/`args` are read anywhere else. Idempotent: a no-op
    /// once `program` is already populated.
    ///
    /// ```rust,ignore
    /// // Illustrative only (binary crate, no doctest target) — see this
    /// // module's #[cfg(test)] tests for the executable equivalent.
    /// let mut pt = PassThroughArgs { program: String::new(), args: vec!["echo".into(), "hi".into()] };
    /// pt.split_program();
    /// assert_eq!(pt.program, "echo");
    /// assert_eq!(pt.args, vec!["hi".to_string()]);
    /// ```
    pub fn split_program(&mut self) {
        if self.program.is_empty() && !self.args.is_empty() {
            self.program = self.args.remove(0);
        }
    }
}

/// `oneshot`'s argument shape: zero or more packages plus the shared
/// pass-through command (FR-003).
///
/// `override_usage` gives this subcommand its own literal
/// `-- <COMMAND> [ARGS...]` synopsis (T057) — it MUST live on this wrapper,
/// not on the shared [`PassThroughArgs`] type itself: `override_usage` on a
/// `#[command(flatten)]`-ed type's own container attributes leaks into
/// every flattening site, which would incorrectly show `sandbox`'s usage
/// line under `oneshot --help` (confirmed empirically against clap 4.6.x).
#[derive(Debug, Args)]
#[command(override_usage = "allez oneshot [PACKAGES]... -- <COMMAND> [ARGS...]")]
pub struct PackagesAndCommandArgs {
    /// Package name tokens (FR-003); zero or more, not an error if empty.
    pub packages: Vec<String>,
    /// The pass-through command captured after `--`; required — enforced by
    /// [`validate_pass_through`] at the dispatch layer, not at the type
    /// level (`sandbox` flattens the same type but stays optional).
    #[command(flatten)]
    pub pass_through: PassThroughArgs,
}

/// `create`'s argument shape: a required non-empty path plus zero or more
/// packages (FR-004). Independent of [`PassThroughArgs`]/
/// [`PackagesAndCommandArgs`] — `create` has no pass-through command at all.
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Target environment path; rejected if empty (FR-004, FR-015).
    #[arg(value_parser = parse_nonempty_path)]
    pub path: String,
    /// Package name tokens; zero or more, not an error if empty.
    pub packages: Vec<String>,
}

/// `run`'s argument shape: a required non-empty path plus the shared
/// pass-through command (FR-005). See [`PackagesAndCommandArgs`]'s doc
/// comment for why `override_usage` lives here rather than on the shared
/// [`PassThroughArgs`] type.
#[derive(Debug, Args)]
#[command(override_usage = "allez run <PATH> -- <COMMAND> [ARGS...]")]
pub struct RunArgs {
    /// Target environment path; rejected if empty (FR-005, FR-015).
    #[arg(value_parser = parse_nonempty_path)]
    pub path: String,
    /// The pass-through command captured after `--`; required — enforced by
    /// [`validate_pass_through`] at the dispatch layer, same as `oneshot`.
    #[command(flatten)]
    pub pass_through: PassThroughArgs,
}

/// `remove`'s argument shape: a single required non-empty path (FR-008).
#[derive(Debug, Args)]
pub struct RemoveArgs {
    /// Target environment path; rejected if empty (FR-008, FR-015).
    #[arg(value_parser = parse_nonempty_path)]
    pub path: String,
}

/// `sandbox`'s argument shape: the shared pass-through command, optional
/// (FR-006). A dedicated wrapper (rather than using [`PassThroughArgs`]
/// directly as `sandbox`'s payload) purely so `sandbox` can carry its own
/// `override_usage` — the *optional* `[-- <COMMAND> [ARGS...]]` synopsis —
/// independent of `oneshot`/`run`'s *required* one, without either leaking
/// into the other (see [`PackagesAndCommandArgs`]'s doc comment).
#[derive(Debug, Args)]
#[command(override_usage = "allez sandbox [-- <COMMAND> [ARGS...]]")]
pub struct SandboxArgs {
    /// The pass-through command captured after `--`; optional — required
    /// only when `--` was present with nothing after it, enforced by
    /// [`sandbox_missing_command`] at the dispatch layer (FR-006).
    #[command(flatten)]
    pub pass_through: PassThroughArgs,
}

/// The six subcommands `allez` routes to (FR-001).
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
    /// This subcommand's name, matching the `subcommand` field FR-014's
    /// stub success payload already reports — used as the `operation`
    /// field on [`crate::dispatch`]'s `tracing` events (Constitution
    /// Principle XI).
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
    /// Selects human-readable output; JSON is the default (FR-013).
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
    /// Reveals redacted pass-through command content (FR-016).
    // See `human`'s `overrides_with` note above; the same rationale
    // applies here.
    #[arg(short = 'v', long, global = true, overrides_with = "verbose")]
    pub verbose: bool,
}

/// Enforces `oneshot`/`run`'s "pass-through command required" rule (FR-003,
/// FR-005).
///
/// Called from `main.rs`'s dispatch, immediately after `Cli::try_parse()`
/// succeeds and before invoking either handler's core logic. Not reused
/// unmodified for `sandbox`, whose required-ness is conditional on `--`
/// presence rather than unconditional (see research.md Decision 1);
/// `sandbox` uses [`sandbox_missing_command`] instead.
///
/// ```rust,ignore
/// // Illustrative only (binary crate, no doctest target) — see this
/// // module's #[cfg(test)] tests for the executable equivalent.
/// let pt = PassThroughArgs { program: "echo".into(), args: vec![] };
/// assert_eq!(validate_pass_through(&pt), Ok(()));
/// assert_eq!(validate_pass_through(&PassThroughArgs::default()), Err(AllezError::MissingPassThroughCommand));
/// ```
pub fn validate_pass_through(pt: &PassThroughArgs) -> Result<(), AllezError> {
    if pt.program.is_empty() {
        return Err(AllezError::MissingPassThroughCommand);
    }
    Ok(())
}

/// Enforces `sandbox`'s conditional "pass-through command required" rule
/// (FR-006): required only when a literal `--` token was present in argv
/// with nothing after it; absent entirely, `sandbox` routes to the
/// interactive-subshell path instead (not an error).
///
/// Only inspects `raw_args` when `pt.program` is empty — clap's own
/// `ArgMatches` state for a `#[arg(last = true)]` field is identical for
/// "no `--` at all" and "`--` present, nothing after it" (empirically
/// confirmed against `clap` 4.6.x; see research.md Decision 1 and
/// data-model.md's "Required-ness enforcement"), so this raw-argv scan is
/// the only way to tell the two apart. `raw_args` is a parameter (not read
/// internally via [`std::env::args_os`]) so this function stays
/// unit-testable with fake argv slices; `main.rs`'s `Sandbox` dispatch arm
/// is the sole call site that supplies the real process argv, safe to scan
/// un-scoped because exactly one subcommand is dispatched per invocation.
///
/// ```rust,ignore
/// // Illustrative only (binary crate, no doctest target) — see this
/// // module's #[cfg(test)] tests for the executable equivalent.
/// let pt = PassThroughArgs::default();
/// let no_separator: Vec<OsString> = vec!["allez".into(), "sandbox".into()];
/// assert_eq!(sandbox_missing_command(&pt, &no_separator), Ok(()));
/// let empty_separator: Vec<OsString> = vec!["allez".into(), "sandbox".into(), "--".into()];
/// assert_eq!(sandbox_missing_command(&pt, &empty_separator), Err(AllezError::MissingPassThroughCommand));
/// ```
pub fn sandbox_missing_command(
    pt: &PassThroughArgs,
    raw_args: &[OsString],
) -> Result<(), AllezError> {
    if !pt.program.is_empty() {
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
        let pt = PassThroughArgs {
            program: String::new(),
            args: vec![],
        };
        assert_eq!(
            validate_pass_through(&pt),
            Err(AllezError::MissingPassThroughCommand)
        );
    }

    #[test]
    fn validate_pass_through_accepts_nonempty_program_without_args() {
        let pt = PassThroughArgs {
            program: "echo".to_string(),
            args: vec![],
        };
        assert_eq!(validate_pass_through(&pt), Ok(()));
    }

    #[test]
    fn validate_pass_through_accepts_nonempty_program_with_args() {
        let pt = PassThroughArgs {
            program: "echo".to_string(),
            args: vec!["hi".to_string()],
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
            program: "python".to_string(),
            args: vec![],
        };
        let raw_args: Vec<OsString> = vec!["allez".into(), "sandbox".into(), "--".into()];
        assert_eq!(sandbox_missing_command(&pt, &raw_args), Ok(()));
    }
}
