use serde_json::json;

use crate::output::render_success;

/// Renders `list`'s stub acknowledgment.
pub fn run(human: bool) -> String {
    render_success("list", json!({}), human)
}
