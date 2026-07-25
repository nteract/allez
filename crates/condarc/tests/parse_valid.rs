//! Integration tests for User Story 1 (spec.md acceptance scenarios 1–4): feeding a valid
//! `.condarc` document to `condarc::parse` produces a `Config` whose fields are coerced exactly
//! as conda would coerce them, aliases resolved to their canonical setting.

use condarc::{ChannelPriority, Config};

/// Scenario 1: a straightforward valid document is accepted and exposes typed values.
#[test]
fn accepts_a_simple_valid_document_with_typed_values() {
    let cfg = condarc::parse(
        "channels: [conda-forge, defaults]\nchannel_priority: strict\nalways_yes: true\n",
    )
    .expect("valid document must be accepted");

    assert_eq!(
        cfg.channels,
        Some(vec!["conda-forge".to_string(), "defaults".to_string()])
    );
    assert_eq!(cfg.channel_priority, Some(ChannelPriority::Strict));
    assert_eq!(cfg.always_yes, Some(Some(true)));
}

/// Scenario 2: a boolean-typed key given the string `"yes"` is stored as the boolean `true`, not
/// the string `"yes"` (conda's `boolify` coercion, FR-012).
#[test]
fn boolean_typed_key_given_boolish_string_is_stored_as_a_real_bool() {
    let cfg = condarc::parse("always_yes: \"yes\"\n").expect("valid document must be accepted");
    assert_eq!(cfg.always_yes, Some(Some(true)));
}

/// Scenario 3: alias spellings resolve to the canonical setting (FR-011).
#[test]
fn alias_spellings_resolve_to_the_canonical_setting() {
    let cfg = condarc::parse(
        "channel: [conda-forge]\nverify_ssl: true\nyes: true\nauto_activate_base: false\n",
    )
    .expect("valid document must be accepted");

    assert_eq!(cfg.channels, Some(vec!["conda-forge".to_string()]));
    assert_eq!(cfg.ssl_verify, Some(condarc::SslVerify::Bool(true)));
    assert_eq!(cfg.always_yes, Some(Some(true)));
    assert_eq!(cfg.auto_activate, Some(false));
}

/// Scenario 4: an empty / null root is accepted as an empty configuration, no error.
#[test]
fn empty_and_null_roots_are_accepted_as_an_empty_configuration() {
    assert_eq!(
        condarc::parse("").expect("empty input accepted"),
        Config::default()
    );
    assert_eq!(
        condarc::parse("~").expect("bare null accepted"),
        Config::default()
    );
    assert_eq!(
        condarc::parse("null").expect("null literal accepted"),
        Config::default()
    );
    assert_eq!(
        condarc::parse("{}").expect("empty mapping accepted"),
        Config::default()
    );
}

/// Unknown top-level keys are silently accepted and retained in `extra` (FR-036), never causing
/// an otherwise-valid document to fail.
#[test]
fn unknown_top_level_keys_are_accepted_and_retained_in_extra() {
    let cfg = condarc::parse("channels: [defaults]\nsome_typo_key: 42\n")
        .expect("unknown keys must not cause rejection");

    assert_eq!(cfg.channels, Some(vec!["defaults".to_string()]));
    assert_eq!(cfg.extra.get("some_typo_key"), Some(&serde_json::json!(42)));
}

/// A whitespace-padded boolish string is accepted and coerced (conda trims before coercing).
#[test]
fn whitespace_padded_boolish_strings_are_accepted() {
    let cfg =
        condarc::parse("debug: \" true \"\n").expect("whitespace-padded boolish string accepted");
    assert_eq!(cfg.debug, Some(true));
}
