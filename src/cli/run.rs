//! `run`'s stub handler: acknowledges parsed path and pass-through command
//! (FR-009).

use serde_json::json;

use crate::cli::RunArgs;
use crate::output::{render_pass_through, render_success};

/// Renders `run`'s stub acknowledgment.
///
/// `args.pass_through` must already have
/// [`crate::cli::PassThroughArgs::split_program`] applied by the caller.
/// Like `oneshot`, this handler does not call `validate_pass_through()`
/// itself — see T039.
pub fn run(args: &RunArgs, human: bool, verbose: bool) -> String {
    let parsed = json!({
        "path": args.path,
        "pass_through": render_pass_through(&args.pass_through, verbose),
    });
    render_success("run", parsed, human)
}
