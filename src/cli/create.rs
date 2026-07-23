use serde_json::json;

use crate::cli::CreateArgs;
use crate::output::render_success;

/// Renders `create`'s stub acknowledgment.
pub fn run(args: &CreateArgs, human: bool) -> String {
    let parsed = json!({
        "path": args.env_path,
        "packages": args.packages,
    });
    render_success("create", parsed, human)
}
