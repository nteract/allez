//! `list`'s stub handler: acknowledges the invocation with an empty
//! `parsed` object (FR-007, FR-009).

use serde_json::json;

use crate::output::render_success;

/// Renders `list`'s stub acknowledgment.
pub fn run(human: bool) -> String {
    render_success("list", json!({}), human)
}
