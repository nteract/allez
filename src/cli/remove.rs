//! `remove`'s stub handler: acknowledges the parsed path (FR-009).

use serde_json::json;

use crate::cli::RemoveArgs;
use crate::output::render_success;

/// Renders `remove`'s stub acknowledgment.
pub fn run(args: &RemoveArgs, human: bool) -> String {
    let parsed = json!({ "path": args.path });
    render_success("remove", parsed, human)
}
