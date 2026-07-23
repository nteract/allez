use serde_json::json;

use crate::cli::RunArgs;
use crate::output::{render_pass_through, render_success};

/// Like `oneshot`, this handler does not call `validate_pass_through()`
/// itself — that already happened at the dispatch layer.
pub fn run(args: &RunArgs, human: bool, verbose: bool) -> String {
    let parsed = json!({
        "path": args.env_path,
        "pass_through": render_pass_through(&args.pass_through, verbose),
    });
    render_success("run", parsed, human)
}
