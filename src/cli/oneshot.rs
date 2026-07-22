//! `oneshot`'s stub handler: acknowledges parsed packages and pass-through
//! command (FR-009).

use serde_json::json;

use crate::cli::PackagesAndCommandArgs;
use crate::output::{render_pass_through, render_success};

/// Renders `oneshot`'s stub acknowledgment.
///
/// `args.pass_through` must already have
/// [`crate::cli::PassThroughArgs::split_program`] applied by the caller
/// (`main.rs`'s dispatch) before this is called. This handler does not
/// call `validate_pass_through()` itself — that happens at the dispatch
/// layer, before this handler is ever invoked (T007A, T039).
pub fn run(args: &PackagesAndCommandArgs, human: bool, verbose: bool) -> String {
    let parsed = json!({
        "packages": args.packages,
        "pass_through": render_pass_through(&args.pass_through, verbose),
    });
    render_success("oneshot", parsed, human)
}
