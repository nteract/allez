//! `--human`/`--verbose` output-mode rendering (FR-013, FR-014, FR-016,
//! FR-017): JSON by default, human-readable text under `--human`.

use serde_json::{Value, json};

use crate::cli::PassThroughArgs;

/// Schema version tag carried by every success/error payload (FR-014).
const SCHEMA_VERSION: &str = "0.1.0-unstable";

/// Redacted-by-default JSON rendering of a captured pass-through command
/// (FR-016): `{"program": "<redacted>", "arg_count": N}` unless `verbose`,
/// in which case the full `{"program": "...", "args": [...]}` is returned.
/// `arg_count` counts `args` only, matching `args.len()` in the unredacted
/// form.
///
/// ```rust,ignore
/// // Illustrative only — `allez` is a binary crate, so this isn't run as
/// // a doctest; see src/output.rs's own #[cfg(test)] module for the
/// // executable equivalent.
/// let pt = PassThroughArgs { program: "curl".into(), args: vec!["-H".into(), "Authorization: Bearer secret".into()] };
/// assert_eq!(render_pass_through(&pt, false), json!({ "program": "<redacted>", "arg_count": 2 }));
/// assert_eq!(render_pass_through(&pt, true)["program"], "curl");
/// ```
pub fn render_pass_through(pt: &PassThroughArgs, verbose: bool) -> Value {
    if verbose {
        json!({ "program": pt.program, "args": pt.args })
    } else {
        json!({ "program": "<redacted>", "arg_count": pt.args.len() })
    }
}

/// Renders a subcommand's stub acknowledgment (FR-009, FR-014): the fixed
/// `{schema_version, subcommand, status: "stub", parsed}` shape as compact
/// JSON (the default), or an equivalent one-line human-readable rendering
/// when `human` is set.
///
/// ```rust,ignore
/// // Illustrative only (binary crate, no doctest target) — see this
/// // module's #[cfg(test)] tests for the executable equivalent.
/// let json_out = render_success("list", json!({}), false);
/// assert!(json_out.contains(r#""status":"stub""#));
/// let human_out = render_success("list", json!({}), true);
/// assert_eq!(human_out, "list: stub; parsed={}");
/// ```
pub fn render_success(subcommand: &str, parsed: Value, human: bool) -> String {
    if human {
        format!("{subcommand}: stub; parsed={parsed}")
    } else {
        json!({
            "schema_version": SCHEMA_VERSION,
            "subcommand": subcommand,
            "status": "stub",
            "parsed": parsed,
        })
        .to_string()
    }
}

/// Renders a usage error (FR-010, FR-011, FR-017): the fixed
/// `{schema_version, category, message}` shape as compact JSON (the
/// default), or an equivalent human-readable rendering when `human` is set.
///
/// `category` and `message` are passed separately (rather than derived
/// solely from an [`crate::error::AllezError`]) so that parse-time errors
/// can carry clap's own precise message text — which correctly
/// distinguishes "unknown flag" from "unexpected extra argument" — while
/// dispatch-layer errors ([`crate::cli::validate_pass_through`]/
/// [`crate::cli::sandbox_missing_command`]) still pass their fixed
/// [`crate::error::AllezError::category`]/[`crate::error::AllezError`]'s
/// `Display` text unchanged. `category` MUST always be one of
/// [`crate::error::AllezError::category`]'s fixed FR-017 strings.
///
/// ```rust,ignore
/// // Illustrative only (binary crate, no doctest target) — see this
/// // module's #[cfg(test)] tests for the executable equivalent.
/// let json_out = render_error(AllezError::MissingArgument.category(), "path must not be empty", false);
/// assert!(json_out.contains(r#""category":"missing_argument""#));
/// ```
pub fn render_error(category: &str, message: &str, human: bool) -> String {
    if human {
        format!("error: {message}")
    } else {
        json!({
            "schema_version": SCHEMA_VERSION,
            "category": category,
            "message": message,
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AllezError;

    #[test]
    fn render_success_emits_fixed_minimal_json_shape() {
        let rendered = render_success("list", json!({}), false);
        let parsed: Value = serde_json::from_str(&rendered).expect("valid JSON");
        assert_eq!(parsed["schema_version"], SCHEMA_VERSION);
        assert_eq!(parsed["subcommand"], "list");
        assert_eq!(parsed["status"], "stub");
        assert_eq!(parsed["parsed"], json!({}));
    }

    #[test]
    fn render_pass_through_redacts_by_default() {
        let pt = PassThroughArgs {
            program: "echo".to_string(),
            args: vec!["hello".to_string()],
        };
        assert_eq!(
            render_pass_through(&pt, false),
            json!({ "program": "<redacted>", "arg_count": 1 })
        );
    }

    #[test]
    fn render_pass_through_reveals_full_value_when_verbose() {
        let pt = PassThroughArgs {
            program: "echo".to_string(),
            args: vec!["hello".to_string()],
        };
        assert_eq!(
            render_pass_through(&pt, true),
            json!({ "program": "echo", "args": ["hello"] })
        );
    }

    #[test]
    fn render_error_emits_fixed_json_shape_with_category_from_allez_error() {
        let rendered = render_error(
            AllezError::MissingArgument.category(),
            &AllezError::MissingArgument.to_string(),
            false,
        );
        let parsed: Value = serde_json::from_str(&rendered).expect("valid JSON");
        assert_eq!(parsed["schema_version"], SCHEMA_VERSION);
        assert_eq!(parsed["category"], AllezError::MissingArgument.category());
        assert!(parsed["message"].is_string());
    }

    #[test]
    fn render_error_carries_an_arbitrary_message_independent_of_category() {
        // Locks in the fix for the "unrecognized flag" mislabeling bug:
        // `render_error` must render whatever message it's given, not a
        // category-derived fixed string — callers (main.rs) are
        // responsible for supplying clap's own precise message text.
        let rendered = render_error("unknown_flag", "unexpected argument 'foo' found", false);
        let parsed: Value = serde_json::from_str(&rendered).expect("valid JSON");
        assert_eq!(parsed["category"], "unknown_flag");
        assert_eq!(parsed["message"], "unexpected argument 'foo' found");
    }

    #[test]
    fn render_error_human_mode_uses_the_given_message_verbatim() {
        assert_eq!(
            render_error("unknown_flag", "custom text", true),
            "error: custom text"
        );
    }
}
