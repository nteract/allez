use serde_json::json;

use crate::cli::RemoveArgs;
use crate::output::render_success;

/// Renders `remove`'s stub acknowledgment.
pub fn run(args: &RemoveArgs, human: bool) -> String {
    let parsed = json!({ "path": args.env_path });
    render_success("remove", parsed, human)
}
