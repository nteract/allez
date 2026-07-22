//! `sandbox`'s stub handler: acknowledges the parsed pass-through command,
//! or a distinct interactive-subshell acknowledgment when none was given
//! (FR-006, FR-009).

use serde_json::json;

use crate::cli::PassThroughArgs;
use crate::output::{render_pass_through, render_success};

/// Renders `sandbox`'s stub acknowledgment.
///
/// `pt` must already have [`PassThroughArgs::split_program`] applied by the
/// caller, and [`crate::cli::sandbox_missing_command`] already checked at
/// the dispatch layer before this is called. An empty `pt.program` here
/// always means "no `--` at all" (the interactive-subshell path) — the
/// "`--` present with nothing after it" case was already rejected as a
/// usage error before this handler was ever invoked.
pub fn run(pt: &PassThroughArgs, human: bool, verbose: bool) -> String {
    let parsed = if pt.program.is_empty() {
        json!({ "interactive_subshell": true })
    } else {
        json!({ "pass_through": render_pass_through(pt, verbose) })
    };
    render_success("sandbox", parsed, human)
}
