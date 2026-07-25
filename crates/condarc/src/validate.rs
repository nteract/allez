//! Per-field semantic validators (`channel_alias`, `default_python`, the opt-in `ssl_verify`
//! filesystem check), alias-collision detection, and the cross-field rules. See spec.md FR-024
//! through FR-029.
//!
//! This module holds two kinds of functions:
//! - Pure, per-value validators (`channel_alias_error`, `default_python_error`,
//!   `ssl_verify_error`) that `parse.rs`'s per-key dispatch loop calls once it has a
//!   successfully-coerced typed value and knows which `catalog.rs` [`crate::catalog::Setting`]
//!   it belongs to (data-model.md §9).
//! - Whole-document passes (`alias_collision_entries`, `cross_field_entries`) that run once,
//!   after the per-key loop, over the raw document map / the fully-coerced [`Config`]
//!   respectively (FR-027/028/029, research R4/R7).

use crate::catalog::CATALOG;
use crate::error::{ErrorEntry, ErrorKind, InputRepr, Location};
use crate::model::{Config, ParseOptions, SslVerify};
use crate::parse::RawValue;

/// `channel_alias` (FR-025): a non-empty value must have a URL scheme matching conda's
/// `has_scheme` rule — `^[a-z][a-z0-9]{0,11}://` — anchored at the start of the string; an empty
/// string is accepted. Returns `Some(message)` iff `value` is invalid.
pub(crate) fn channel_alias_error(value: &str) -> Option<String> {
    if has_valid_channel_alias_scheme(value) {
        None
    } else {
        Some(format!(
            "{value:?} must start with a URL scheme matching '^[a-z][a-z0-9]{{0,11}}://', or be empty"
        ))
    }
}

/// conda's `has_scheme`: the scheme is a run of `[a-z][a-z0-9]{0,11}` (so 1–12 characters total)
/// at the very start of the string, immediately followed by the literal `://`. Since `:` is not
/// itself a valid scheme character, greedily consuming as many `[a-z0-9]` characters as allowed
/// and then checking for `://` is equivalent to the regex (no backtracking is ever needed).
fn has_valid_channel_alias_scheme(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    let bytes = value.as_bytes();
    if !bytes[0].is_ascii_lowercase() {
        return false;
    }
    let mut i = 1;
    while i < bytes.len() && i < 12 && (bytes[i].is_ascii_lowercase() || bytes[i].is_ascii_digit())
    {
        i += 1;
    }
    value[i..].starts_with("://")
}

/// `default_python` (FR-026, A4): an empty string is "no pinning" and always accepted (a `null`
/// value never reaches this function at all — see `parse.rs`'s dispatch, which only calls this
/// for the `Some(String)` case of the already-coerced `NullableString`). Otherwise all three of
/// conda's `default_python_validation` conditions must hold: length >= 3, the character at index
/// 1 is a literal `.`, and the whole (ASCII-only, per A4) string parses as a float in `[2.0,
/// 4.0)`. Returns `Some(message)` iff `value` is invalid.
pub(crate) fn default_python_error(value: &str) -> Option<String> {
    if value.is_empty() || is_default_python_shape_valid(value) {
        None
    } else {
        Some(format!(
            "{value:?} is not a valid default_python value: expected empty, or a string whose \
             second character is '.' and which parses as an ASCII float in [2.0, 4.0)"
        ))
    }
}

fn is_default_python_shape_valid(value: &str) -> bool {
    if value.len() < 3 || value.as_bytes()[1] != b'.' {
        return false;
    }
    if !crate::coerce::numeric::is_ascii_only(value) {
        return false;
    }
    let Some(cleaned) = crate::coerce::numeric::strip_pep515_underscores(value) else {
        return false;
    };
    let Ok(f) = cleaned.parse::<f64>() else {
        return false;
    };
    (2.0..4.0).contains(&f)
}

/// `ssl_verify`'s opt-in filesystem-existence check (FR-024, A3, research R6). A no-op unless
/// `options.ssl_verify_fs_check` is set, and only ever fails for the [`SslVerify::Path`] variant
/// (a `Bool`/`Truststore` value never touches the filesystem). Returns `Some(message)` iff the
/// path does not exist and the check is enabled.
pub(crate) fn ssl_verify_error(value: &SslVerify, options: &ParseOptions) -> Option<String> {
    if !options.ssl_verify_fs_check {
        return None;
    }
    let SslVerify::Path(path) = value else {
        return None;
    };
    if std::path::Path::new(path).exists() {
        None
    } else {
        Some(format!(
            "{path:?} does not exist on the local filesystem (ssl_verify_fs_check is enabled)"
        ))
    }
}

/// FR-029: a document that sets two aliases of the same setting simultaneously is rejected —
/// once per colliding setting, naming every spelling that was present (`involved`), in `CATALOG`
/// declaration order (research R7).
pub(crate) fn alias_collision_entries(
    map: &indexmap::IndexMap<String, RawValue>,
) -> Vec<ErrorEntry> {
    let mut out = Vec::new();
    for setting in CATALOG {
        if setting.aliases.is_empty() {
            continue;
        }
        let mut present: Vec<&str> = Vec::new();
        if map.contains_key(setting.canonical) {
            present.push(setting.canonical);
        }
        for alias in setting.aliases {
            if map.contains_key(*alias) {
                present.push(alias);
            }
        }
        if present.len() > 1 {
            out.push(ErrorEntry {
                location: Location::Root,
                kind: ErrorKind::AliasCollision,
                message: format!(
                    "'{}' was set using multiple alias spellings in the same document: {}",
                    setting.canonical,
                    present.join(", ")
                ),
                input: InputRepr::Map,
                involved: present.iter().map(|s| s.to_string()).collect(),
            });
        }
    }
    out
}

/// FR-027/FR-028: `Context.post_build_validation`'s two cross-field rules, run once over the
/// fully-coerced [`Config`], in their documented order (research R7).
pub(crate) fn cross_field_entries(cfg: &Config) -> Vec<ErrorEntry> {
    let mut out = Vec::new();

    // Rule 1: `client_ssl_cert_key` truthy but `client_ssl_cert` is not (docs/condarc_research.md
    // §1.4 rule 1).
    if is_truthy_nullable_string(&cfg.client_ssl_cert_key)
        && !is_truthy_nullable_string(&cfg.client_ssl_cert)
    {
        out.push(ErrorEntry {
            location: Location::Root,
            kind: ErrorKind::CrossField,
            message: "'client_ssl_cert' is required when 'client_ssl_cert_key' is defined"
                .to_string(),
            input: InputRepr::Map,
            involved: vec![
                "client_ssl_cert".to_string(),
                "client_ssl_cert_key".to_string(),
            ],
        });
    }

    // Rule 2: `always_copy` and `always_softlink` mutually exclusive (docs/condarc_research.md
    // §1.4 rule 2).
    if cfg.always_copy == Some(true) && cfg.always_softlink == Some(true) {
        out.push(ErrorEntry {
            location: Location::Root,
            kind: ErrorKind::CrossField,
            message: "'always_copy' and 'always_softlink' are mutually exclusive. Only one can \
                       be set to true."
                .to_string(),
            input: InputRepr::Map,
            involved: vec!["always_copy".to_string(), "always_softlink".to_string()],
        });
    }

    out
}

/// Python truthiness of a `(str, None)`-shaped coerced value: `None` (absent or explicit null) is
/// falsy, an empty string is falsy, any other string is truthy.
fn is_truthy_nullable_string(value: &Option<Option<String>>) -> bool {
    matches!(value, Some(Some(s)) if !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::RawValue;

    // ---- channel_alias (FR-025) ----

    #[test]
    fn channel_alias_accepts_empty_string() {
        assert_eq!(channel_alias_error(""), None);
    }

    #[test]
    fn channel_alias_accepts_a_well_formed_scheme() {
        assert_eq!(channel_alias_error("https://conda.anaconda.org"), None);
        assert_eq!(channel_alias_error("s3://bucket"), None);
    }

    #[test]
    fn channel_alias_rejects_missing_scheme() {
        assert!(channel_alias_error("conda.anaconda.org").is_some());
        assert!(channel_alias_error("no-scheme-here").is_some());
    }

    #[test]
    fn channel_alias_rejects_uppercase_scheme() {
        assert!(channel_alias_error("HTTPS://conda.anaconda.org").is_some());
    }

    #[test]
    fn channel_alias_rejects_scheme_longer_than_twelve_characters() {
        // 13-character scheme -- one over the `{0,11}` (max 12 total) bound.
        assert!(channel_alias_error("abcdefghijklm://host").is_some());
    }

    #[test]
    fn channel_alias_rejects_colon_without_double_slash() {
        assert!(channel_alias_error("https:conda.anaconda.org").is_some());
    }

    #[test]
    fn channel_alias_rejects_scheme_starting_with_a_digit() {
        assert!(channel_alias_error("3ftp://host").is_some());
    }

    #[test]
    fn channel_alias_rejects_mixed_case_scheme() {
        assert!(channel_alias_error("Https://host").is_some());
        assert!(channel_alias_error("httpS://host").is_some());
    }

    #[test]
    fn channel_alias_rejects_disallowed_character_in_scheme() {
        assert!(channel_alias_error("ft+p://host").is_some());
        assert!(channel_alias_error("ft.p://host").is_some());
    }

    #[test]
    fn channel_alias_rejects_bare_double_slash_with_no_scheme_characters() {
        // The regex's `[a-z]` first character is mandatory, not `{1,}` optional -- a
        // non-empty string that starts directly with "://" has zero scheme characters and must
        // still be rejected (only the *fully empty* string is the accepted special case).
        assert!(channel_alias_error("://host").is_some());
    }

    #[test]
    fn channel_alias_accepts_a_twelve_character_scheme_exactly() {
        // `[a-z][a-z0-9]{0,11}` allows up to 12 total scheme characters.
        assert_eq!(channel_alias_error("abcdefghijkl://host"), None);
    }

    #[test]
    fn channel_alias_rejects_a_scheme_with_extra_content_after_the_max_length() {
        // Greedy consumption stops at 12 characters; anything left over before "://" must not
        // itself accidentally satisfy the check via a different split point.
        assert!(channel_alias_error("abcdefghijklmnop://host").is_some());
    }

    // ---- default_python (FR-026, A4) ----

    #[test]
    fn default_python_accepts_empty_string() {
        assert_eq!(default_python_error(""), None);
    }

    #[test]
    fn default_python_accepts_values_across_the_documented_range() {
        for value in [
            "2.0", "2.00", "3.99", "3.999999", "3.e0", "2.0e0", "2.5E0", "2.5_5",
        ] {
            assert_eq!(default_python_error(value), None, "value={value}");
        }
    }

    #[test]
    fn default_python_rejects_upper_boundary_and_out_of_range_values() {
        for value in ["4.0", "1.9", "9.5", "3.5e-1"] {
            assert!(default_python_error(value).is_some(), "value={value}");
        }
    }

    #[test]
    fn default_python_rejects_wrong_shape_values() {
        for value in [
            "3", "3.", "03.9", "33.9", "-3.5", "3.10.1", "3,9", "3.a", "3e0",
        ] {
            assert!(default_python_error(value).is_some(), "value={value}");
        }
    }

    #[test]
    fn default_python_rejects_underscore_not_between_digits() {
        assert!(default_python_error("3._5").is_some());
    }

    // ---- ssl_verify path existence (FR-024, A3, R6) ----

    #[test]
    fn ssl_verify_default_options_never_check_the_filesystem() {
        assert_eq!(
            ssl_verify_error(
                &SslVerify::Path("/definitely/does/not/exist/anywhere".to_string()),
                &ParseOptions::default()
            ),
            None
        );
    }

    #[test]
    fn ssl_verify_opt_in_check_rejects_a_nonexistent_path() {
        let options = ParseOptions {
            ssl_verify_fs_check: true,
        };
        assert!(
            ssl_verify_error(
                &SslVerify::Path("/definitely/does/not/exist/anywhere".to_string()),
                &options
            )
            .is_some()
        );
    }

    #[test]
    fn ssl_verify_opt_in_check_accepts_an_existing_path() {
        let options = ParseOptions {
            ssl_verify_fs_check: true,
        };
        assert_eq!(
            ssl_verify_error(&SslVerify::Path(".".to_string()), &options),
            None
        );
    }

    #[test]
    fn ssl_verify_opt_in_check_never_touches_bool_or_truststore_variants() {
        let options = ParseOptions {
            ssl_verify_fs_check: true,
        };
        assert_eq!(ssl_verify_error(&SslVerify::Bool(true), &options), None);
        assert_eq!(ssl_verify_error(&SslVerify::Truststore, &options), None);
    }

    // ---- alias collision (FR-029) ----

    #[test]
    fn alias_collision_detects_both_spellings_present() {
        let mut map = indexmap::IndexMap::new();
        map.insert("always_yes".to_string(), RawValue::Bool(true));
        map.insert("yes".to_string(), RawValue::Bool(true));

        let entries = alias_collision_entries(&map);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, ErrorKind::AliasCollision);
        assert_eq!(entries[0].location, Location::Root);
        assert_eq!(
            entries[0].involved,
            vec!["always_yes".to_string(), "yes".to_string()]
        );
    }

    #[test]
    fn alias_collision_detects_the_canonical_and_alias_spelling_in_either_role() {
        // The collision is symmetric: it doesn't matter whether the alias or the canonical
        // spelling appears "first" in the map (IndexMap insertion order), only that both are
        // present.
        let mut map = indexmap::IndexMap::new();
        map.insert("verify_ssl".to_string(), RawValue::Bool(true));
        map.insert("ssl_verify".to_string(), RawValue::Bool(false));

        let entries = alias_collision_entries(&map);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].involved,
            vec!["ssl_verify".to_string(), "verify_ssl".to_string()]
        );
    }

    #[test]
    fn alias_collision_detects_multiple_independent_collisions_in_one_document() {
        let mut map = indexmap::IndexMap::new();
        map.insert("always_yes".to_string(), RawValue::Bool(true));
        map.insert("yes".to_string(), RawValue::Bool(true));
        map.insert("ssl_verify".to_string(), RawValue::Bool(true));
        map.insert("verify_ssl".to_string(), RawValue::Bool(true));
        map.insert("channels".to_string(), RawValue::Seq(vec![]));

        let entries = alias_collision_entries(&map);
        assert_eq!(entries.len(), 2, "expected one entry per colliding setting");
        let involved_settings: Vec<&str> = entries
            .iter()
            .flat_map(|e| e.involved.iter().map(String::as_str))
            .collect();
        assert!(involved_settings.contains(&"always_yes"));
        assert!(involved_settings.contains(&"ssl_verify"));
    }

    #[test]
    fn alias_collision_is_silent_when_only_one_spelling_is_present() {
        let mut map = indexmap::IndexMap::new();
        map.insert("always_yes".to_string(), RawValue::Bool(true));
        assert!(alias_collision_entries(&map).is_empty());
    }

    #[test]
    fn alias_collision_is_silent_for_settings_with_no_aliases_at_all() {
        let mut map = indexmap::IndexMap::new();
        map.insert("debug".to_string(), RawValue::Bool(true));
        map.insert("offline".to_string(), RawValue::Bool(false));
        assert!(alias_collision_entries(&map).is_empty());
    }

    // ---- cross-field rules (FR-027/FR-028) ----

    #[test]
    fn cross_field_rejects_client_ssl_cert_key_without_cert() {
        let cfg = Config {
            client_ssl_cert_key: Some(Some("x".to_string())),
            ..Config::default()
        };
        let entries = cross_field_entries(&cfg);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, ErrorKind::CrossField);
        assert_eq!(
            entries[0].involved,
            vec![
                "client_ssl_cert".to_string(),
                "client_ssl_cert_key".to_string()
            ]
        );
    }

    #[test]
    fn cross_field_accepts_client_ssl_cert_key_with_cert_present() {
        let cfg = Config {
            client_ssl_cert_key: Some(Some("x".to_string())),
            client_ssl_cert: Some(Some("y".to_string())),
            ..Config::default()
        };
        assert!(cross_field_entries(&cfg).is_empty());
    }

    #[test]
    fn cross_field_accepts_client_ssl_cert_key_falsy_values() {
        // Absent, explicit null, and empty string are all falsy -- no cross-field violation.
        assert!(cross_field_entries(&Config::default()).is_empty());
        let null_key = Config {
            client_ssl_cert_key: Some(None),
            ..Config::default()
        };
        assert!(cross_field_entries(&null_key).is_empty());
        let empty_key = Config {
            client_ssl_cert_key: Some(Some(String::new())),
            ..Config::default()
        };
        assert!(cross_field_entries(&empty_key).is_empty());
    }

    #[test]
    fn cross_field_rejects_always_copy_and_always_softlink_both_true() {
        let cfg = Config {
            always_copy: Some(true),
            always_softlink: Some(true),
            ..Config::default()
        };
        let entries = cross_field_entries(&cfg);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, ErrorKind::CrossField);
        assert_eq!(
            entries[0].involved,
            vec!["always_copy".to_string(), "always_softlink".to_string()]
        );
    }

    #[test]
    fn cross_field_accepts_only_one_of_always_copy_or_softlink() {
        let cfg = Config {
            always_copy: Some(true),
            always_softlink: Some(false),
            ..Config::default()
        };
        assert!(cross_field_entries(&cfg).is_empty());
    }

    #[test]
    fn both_cross_field_rules_can_accumulate_together() {
        let cfg = Config {
            client_ssl_cert_key: Some(Some("x".to_string())),
            always_copy: Some(true),
            always_softlink: Some(true),
            ..Config::default()
        };
        assert_eq!(cross_field_entries(&cfg).len(), 2);
    }

    #[test]
    fn cross_field_rules_operate_on_the_coerced_config_regardless_of_which_alias_set_them() {
        // `cross_field_entries` reads the already-coerced, canonical-named `Config` fields, so it
        // doesn't matter whether the document spelled a setting via its alias
        // (`client_cert_key`/`copy`/`softlink`) -- by the time this pass runs, `parse.rs`'s
        // per-key loop has already resolved the alias into the canonical field.
        let cfg = Config {
            client_ssl_cert_key: Some(Some("x".to_string())), // as if set via `client_cert_key`
            ..Config::default()
        };
        assert_eq!(cross_field_entries(&cfg).len(), 1);
    }
}
