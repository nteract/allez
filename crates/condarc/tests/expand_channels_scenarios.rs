use std::collections::BTreeMap;

use condarc::{ChannelPriority, ChannelSetting, Config, ExpandChannelsError, ResolvedChannels};

#[cfg(not(windows))]
const BUILTIN_DEFAULT_CHANNELS: &[&str] = &[
    "https://repo.anaconda.com/pkgs/main",
    "https://repo.anaconda.com/pkgs/r",
];
#[cfg(windows)]
const BUILTIN_DEFAULT_CHANNELS: &[&str] = &[
    "https://repo.anaconda.com/pkgs/main",
    "https://repo.anaconda.com/pkgs/r",
    "https://repo.anaconda.com/pkgs/msys2",
];

fn resolve(yaml: &str) -> ResolvedChannels {
    let config = condarc::parse(yaml).expect("scenario must parse");
    condarc::expand_channels(&config).expect("scenario must resolve")
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn sc_003_01_bare_names_resolve_through_channel_alias_in_order() {
    let resolved = resolve("channels: [alpha, beta]\nchannel_alias: https://repo.example.org\n");

    assert_eq!(
        resolved.channels,
        strings(&[
            "https://repo.example.org/alpha",
            "https://repo.example.org/beta",
        ])
    );
}

#[test]
fn sc_003_02_custom_channels_exact_match_resolves_with_custom_base() {
    let resolved = resolve(
        r#"
channels: [acme]
custom_channels:
  acme: https://internal.example.com
"#,
    );

    assert_eq!(
        resolved.channels,
        strings(&["https://internal.example.com/acme"])
    );
}

#[test]
fn sc_003_03_custom_channels_prefix_match_uses_original_full_entry() {
    let resolved = resolve(
        r#"
channels: [acme/label/dev]
custom_channels:
  acme: https://internal.example.com
"#,
    );

    assert_eq!(
        resolved.channels,
        strings(&["https://internal.example.com/acme/label/dev"])
    );
}

#[test]
fn sc_003_04_multichannel_member_naming_multichannel_is_not_reexpanded() {
    let resolved = resolve(
        r#"
channels: [outer]
channel_alias: https://repo.example.org
custom_multichannels:
  outer: [inner]
  inner: [https://unused.example.org/channel]
"#,
    );

    assert_eq!(
        resolved.channels,
        strings(&["https://repo.example.org/inner"])
    );
}

#[test]
fn sc_003_05_multichannel_member_naming_custom_channel_is_not_reexpanded() {
    let resolved = resolve(
        r#"
channels: [outer]
channel_alias: https://repo.example.org
custom_channels:
  acme: https://internal.example.com
custom_multichannels:
  outer: [acme]
"#,
    );

    assert_eq!(
        resolved.channels,
        strings(&["https://repo.example.org/acme"])
    );
}

#[test]
fn sc_003_06_explicit_defaults_uses_default_channels() {
    let resolved =
        resolve("channels: [defaults]\ndefault_channels: [https://repo.example.org/main]\n");

    assert_eq!(
        resolved.channels,
        strings(&["https://repo.example.org/main"])
    );
}

#[test]
fn sc_003_07_absent_channels_uses_defaults_substitution() {
    let resolved = resolve("default_channels: [https://repo.example.org/main]\n");

    assert_eq!(
        resolved.channels,
        strings(&["https://repo.example.org/main"])
    );
}

#[test]
fn sc_003_08_null_channels_uses_defaults_substitution() {
    let resolved = resolve("channels: null\ndefault_channels: [https://repo.example.org/main]\n");

    assert_eq!(
        resolved.channels,
        strings(&["https://repo.example.org/main"])
    );
}

#[test]
fn sc_003_09_empty_channels_uses_defaults_substitution() {
    let resolved = resolve("channels: []\ndefault_channels: [https://repo.example.org/main]\n");

    assert_eq!(
        resolved.channels,
        strings(&["https://repo.example.org/main"])
    );
}

#[test]
fn sc_003_10_user_default_channels_replace_builtin_defaults() {
    let resolved =
        resolve("channels: [defaults]\ndefault_channels: [https://packages.example.org/stable]\n");

    assert_eq!(
        resolved.channels,
        strings(&["https://packages.example.org/stable"])
    );
}

#[test]
fn sc_003_11_strict_channel_priority_is_preserved() {
    let resolved = resolve("channels: [https://repo.example.org/main]\nchannel_priority: strict\n");

    assert_eq!(resolved.channel_priority, ChannelPriority::Strict);
}

#[test]
fn sc_003_12_flexible_channel_priority_is_preserved() {
    let resolved =
        resolve("channels: [https://repo.example.org/main]\nchannel_priority: flexible\n");

    assert_eq!(resolved.channel_priority, ChannelPriority::Flexible);
}

#[test]
fn sc_003_13_disabled_channel_priority_is_preserved() {
    let resolved =
        resolve("channels: [https://repo.example.org/main]\nchannel_priority: disabled\n");

    assert_eq!(resolved.channel_priority, ChannelPriority::Disabled);
}

#[test]
fn sc_003_14_absent_channel_priority_defaults_to_flexible() {
    let resolved = resolve("channels: [https://repo.example.org/main]\n");

    assert_eq!(resolved.channel_priority, ChannelPriority::Flexible);
}

#[test]
fn sc_003_15_legacy_true_channel_priority_resolves_to_flexible() {
    let resolved = resolve("channels: [https://repo.example.org/main]\nchannel_priority: true\n");

    assert_eq!(resolved.channel_priority, ChannelPriority::Flexible);
}

#[test]
fn sc_003_16_legacy_false_channel_priority_resolves_to_disabled() {
    let resolved = resolve("channels: [https://repo.example.org/main]\nchannel_priority: false\n");

    assert_eq!(resolved.channel_priority, ChannelPriority::Disabled);
}

#[test]
fn sc_003_17_bare_denylist_entry_expands_before_removing_channel() {
    let resolved = resolve(
        r#"
channels: [alpha, beta]
channel_alias: https://repo.example.org
denylist_channels: [alpha]
"#,
    );

    assert_eq!(
        resolved.channels,
        strings(&["https://repo.example.org/beta"])
    );
}

#[test]
fn sc_003_18_bare_allowlist_entry_expands_before_retaining_channel() {
    let resolved = resolve(
        r#"
channels: [alpha, beta]
channel_alias: https://repo.example.org
allowlist_channels: [alpha]
"#,
    );

    assert_eq!(
        resolved.channels,
        strings(&["https://repo.example.org/alpha"])
    );
}

#[test]
fn user_story_3_acceptance_03_deny_wins_and_allowlist_filters_remaining_channels() {
    let resolved = resolve(
        r#"
channels: [alpha, beta, gamma, delta]
channel_alias: https://repo.example.org
allowlist_channels: [alpha, beta, gamma]
denylist_channels: [beta, delta]
"#,
    );

    assert_eq!(
        resolved.channels,
        strings(&[
            "https://repo.example.org/alpha",
            "https://repo.example.org/gamma",
        ])
    );
}

#[test]
fn sc_003_19_channels_alias_collision_is_rejected_by_parse() {
    let parsed = condarc::parse("channels: [alpha]\nchannel: [beta]\n");

    assert!(parsed.is_err());
}

#[test]
fn sc_003_20_allowlist_alias_collision_is_rejected_by_parse() {
    let parsed = condarc::parse("allowlist_channels: [alpha]\nwhitelist_channels: [beta]\n");

    assert!(parsed.is_err());
}

#[test]
fn sc_003_21_trailing_alias_slash_produces_single_joining_slash() {
    let resolved = resolve("channels: [alpha]\nchannel_alias: https://repo.example.org/\n");

    assert_eq!(
        resolved.channels,
        strings(&["https://repo.example.org/alpha"])
    );
}

#[test]
fn sc_003_22_dot_containing_bare_name_still_uses_channel_alias() {
    let resolved =
        resolve("channels: [packages.example.org]\nchannel_alias: https://repo.example.org\n");

    assert_eq!(
        resolved.channels,
        strings(&["https://repo.example.org/packages.example.org"])
    );
}

#[test]
fn sc_005_empty_channel_alias_returns_entry_specific_error() {
    let config = condarc::parse("channels: [alpha]\nchannel_alias: \"\"\n")
        .expect("empty alias is valid at parse time");

    let error = condarc::expand_channels(&config).expect_err("bare entry requires alias join");

    assert_eq!(
        error,
        ExpandChannelsError::EmptyChannelAlias {
            entry: "alpha".to_string(),
        }
    );
    assert!(error.to_string().contains("alpha"));
}

#[test]
fn config_default_resolves_builtin_channels_and_flexible_priority() {
    let resolved = condarc::expand_channels(&Config::default())
        .expect("Config::default uses a non-empty built-in alias");

    assert_eq!(resolved.channels, strings(BUILTIN_DEFAULT_CHANNELS));
    assert_eq!(resolved.channel_priority, ChannelPriority::Flexible);
}

#[test]
fn acceptance_04_nonempty_channels_never_append_default_channels() {
    let resolved = resolve(
        r#"
channels: [conda-forge, https://example.com/x]
default_channels: [https://repo.example.org/main]
"#,
    );

    assert_eq!(
        resolved.channels,
        strings(&[
            "https://conda.anaconda.org/conda-forge",
            "https://example.com/x",
        ])
    );
}

#[test]
fn acceptance_05_multichannel_members_always_use_restricted_precedence() {
    let resolved = resolve(
        r#"
channels: [bundle]
channel_alias: https://repo.example.org
custom_channels:
  acme: https://internal.example.com
custom_multichannels:
  bundle: [other, acme, bundle]
  other: [https://unused.example.org/channel]
"#,
    );

    assert_eq!(
        resolved.channels,
        strings(&[
            "https://repo.example.org/other",
            "https://repo.example.org/acme",
            "https://repo.example.org/bundle",
        ])
    );
}

#[test]
fn override_channels_enabled_true_has_no_effect() {
    let resolved = resolve("channels: [alpha]\noverride_channels_enabled: true\n");

    assert_eq!(
        resolved.channels,
        strings(&["https://conda.anaconda.org/alpha"])
    );
}

#[test]
fn override_channels_enabled_false_has_no_effect() {
    let resolved = resolve("channels: [alpha]\noverride_channels_enabled: false\n");

    assert_eq!(
        resolved.channels,
        strings(&["https://conda.anaconda.org/alpha"])
    );
}

#[test]
fn coincidental_duplicate_urls_survive_resolution() {
    let resolved = resolve(
        r#"
channels: [first, second]
custom_multichannels:
  first: [https://repo.example.org/shared]
  second: [https://repo.example.org/shared]
"#,
    );

    assert_eq!(
        resolved.channels,
        strings(&[
            "https://repo.example.org/shared",
            "https://repo.example.org/shared",
        ])
    );
}

#[test]
fn channel_settings_pass_through_without_affecting_resolved_channels() {
    // Given
    let resolved = resolve(
        r#"
channels: [alpha]
channel_settings:
  - channel: https://private.example.org
    auth: secret
"#,
    );

    // Then
    assert_eq!(
        resolved.channels,
        strings(&["https://conda.anaconda.org/alpha"])
    );
    assert_eq!(
        resolved.channel_settings,
        vec![ChannelSetting(BTreeMap::from([
            (
                "channel".to_string(),
                "https://private.example.org".to_string(),
            ),
            ("auth".to_string(), "secret".to_string()),
        ]))]
    );
}

#[test]
fn explicit_empty_default_channels_stays_empty_while_absent_uses_builtin() {
    let explicit_empty = resolve("channels: [defaults]\ndefault_channels: []\n");
    let absent = resolve("channels: [conda-forge]\n");

    assert_eq!(explicit_empty.channels, Vec::<String>::new());
    assert_eq!(
        absent.channels,
        strings(&["https://conda.anaconda.org/conda-forge"])
    );
}

#[test]
fn explicit_empty_custom_channels_uses_alias_instead_of_builtin_mapping() {
    let resolved = resolve("channels: [pkgs/pro]\ncustom_channels: {}\n");

    assert_eq!(
        resolved.channels,
        strings(&["https://conda.anaconda.org/pkgs/pro"])
    );
}
