use serde_json::json;

use crate::cli::PackagesAndCommandArgs;
use crate::output::{render_pass_through, render_success};

/// This handler does not call `validate_pass_through()` itself — that
/// already happened at the dispatch layer, before this handler is ever
/// invoked.
pub fn run(args: &PackagesAndCommandArgs, human: bool, verbose: bool) -> String {
    let parsed = json!({
        "packages": args.packages,
        "pass_through": render_pass_through(&args.pass_through, verbose),
    });
    render_success("oneshot", parsed, human)
}
