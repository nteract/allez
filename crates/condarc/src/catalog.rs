//! The static `CATALOG` table: every recognized `.condarc` setting's canonical name, aliases,
//! `ValueKind`, and validator, declared exactly once. See data-model.md §5.

/// One closed-vocabulary enum a [`ValueKind::Enum`] setting coerces into. See data-model.md §3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnumKind {
    ChannelPriority,
    PathConflict,
    SafetyChecks,
    SatSolver,
}

/// Each setting's coercion shape — the Rust analogue of conda's `element_type` (data-model.md
/// §4). One catalog table (below) drives all coercion for all 99 settings (FR-009/010/011).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueKind {
    /// Plain non-nullable `bool`.
    Bool,
    /// `(bool, None)`.
    NullableBool,
    /// `(str, bool)`, `return_string` passthrough — `ssl_verify` only.
    SslVerifyKind,
    /// `(bool, int)` — `local_repodata_ttl`'s narrower boolish vocabulary.
    BoolOrIntKind,
    Int,
    Float,
    /// `str`.
    PlainString,
    /// `(str, None)`; `"none"` (case-insensitive) -> null.
    NullableString,
    Enum(EnumKind),
    /// `SequenceParameter(str)`.
    StringSeq,
    /// `SequenceParameter(str)` restricted to the closed `CONDA_LIST_FIELDS` vocabulary.
    ListFieldsSeq,
    /// `MapParameter(str)`.
    StringMap,
    /// `MapParameter((str, None))`.
    NullableStringMap,
    /// `MapParameter(SequenceParameter(str))` — `custom_multichannels` only.
    StringSeqMap,
    /// `SequenceParameter(MapParameter(str))` — `channel_settings` only.
    ChannelSettingsSeq,
}

/// Which per-field semantic validator (`validate.rs`) applies to a setting, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticValidator {
    ChannelAlias,
    DefaultPython,
    SslVerify,
    ListFields,
}

/// One catalog entry: a recognized setting's canonical name, accepted aliases, coercion shape,
/// and optional semantic validator (data-model.md §5).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Setting {
    /// Conda's canonical loader name (the `Config` field name, and the adapter's JSON key).
    pub(crate) canonical: &'static str,
    /// Other accepted spellings (FR-011); empty if none.
    pub(crate) aliases: &'static [&'static str],
    /// Coercion shape (§4).
    pub(crate) kind: ValueKind,
    /// Semantic validator to run on the coerced value, if any.
    pub(crate) validator: Option<SemanticValidator>,
}

/// The full 99-entry settings table, in declaration order (research R7: this order is also the
/// deterministic per-field error-entry ordering). Single source of truth for every setting's
/// canonical name, aliases, coercion shape, and validator (Constitution IV).
pub(crate) static CATALOG: &[Setting] = &[
    // §4.1 Channel Configuration
    Setting {
        canonical: "channels",
        aliases: &["channel"],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "channel_alias",
        aliases: &[],
        kind: ValueKind::PlainString,
        validator: Some(SemanticValidator::ChannelAlias),
    },
    Setting {
        canonical: "channel_settings",
        aliases: &[],
        kind: ValueKind::ChannelSettingsSeq,
        validator: None,
    },
    Setting {
        canonical: "default_channels",
        aliases: &[],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "override_channels_enabled",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "allowlist_channels",
        aliases: &["whitelist_channels"],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "denylist_channels",
        aliases: &[],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "custom_channels",
        aliases: &[],
        kind: ValueKind::StringMap,
        validator: None,
    },
    Setting {
        canonical: "custom_multichannels",
        aliases: &[],
        kind: ValueKind::StringSeqMap,
        validator: None,
    },
    Setting {
        canonical: "migrated_channel_aliases",
        aliases: &[],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "migrated_custom_channels",
        aliases: &[],
        kind: ValueKind::StringMap,
        validator: None,
    },
    Setting {
        canonical: "add_anaconda_token",
        aliases: &["add_binstar_token"],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "allow_non_channel_urls",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "repodata_fns",
        aliases: &[],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "use_only_tar_bz2",
        aliases: &[],
        kind: ValueKind::NullableBool,
        validator: None,
    },
    Setting {
        canonical: "repodata_threads",
        aliases: &[],
        kind: ValueKind::Int,
        validator: None,
    },
    Setting {
        canonical: "fetch_threads",
        aliases: &[],
        kind: ValueKind::Int,
        validator: None,
    },
    Setting {
        canonical: "experimental",
        aliases: &[],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "no_lock",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "repodata_use_zst",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "repodata_use_shards",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    // §4.2 Basic Conda Configuration
    Setting {
        canonical: "envs_dirs",
        aliases: &["envs_path"],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "pkgs_dirs",
        aliases: &[],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "default_threads",
        aliases: &[],
        kind: ValueKind::Int,
        validator: None,
    },
    Setting {
        canonical: "preview",
        aliases: &[],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    // §4.3 Network Configuration
    Setting {
        canonical: "client_ssl_cert",
        aliases: &["client_cert"],
        kind: ValueKind::NullableString,
        validator: None,
    },
    Setting {
        canonical: "client_ssl_cert_key",
        aliases: &["client_cert_key"],
        kind: ValueKind::NullableString,
        validator: None,
    },
    Setting {
        canonical: "local_repodata_ttl",
        aliases: &[],
        kind: ValueKind::BoolOrIntKind,
        validator: None,
    },
    Setting {
        canonical: "offline",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "proxy_servers",
        aliases: &[],
        kind: ValueKind::NullableStringMap,
        validator: None,
    },
    Setting {
        canonical: "remote_connect_timeout_secs",
        aliases: &[],
        kind: ValueKind::Float,
        validator: None,
    },
    Setting {
        canonical: "remote_max_retries",
        aliases: &[],
        kind: ValueKind::Int,
        validator: None,
    },
    Setting {
        canonical: "remote_backoff_factor",
        aliases: &[],
        kind: ValueKind::Int,
        validator: None,
    },
    Setting {
        canonical: "remote_read_timeout_secs",
        aliases: &[],
        kind: ValueKind::Float,
        validator: None,
    },
    Setting {
        canonical: "ssl_verify",
        aliases: &["verify_ssl"],
        kind: ValueKind::SslVerifyKind,
        validator: Some(SemanticValidator::SslVerify),
    },
    // §4.4 Solver Configuration
    Setting {
        canonical: "aggressive_update_packages",
        aliases: &[],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "auto_update_conda",
        aliases: &["self_update"],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "channel_priority",
        aliases: &[],
        kind: ValueKind::Enum(EnumKind::ChannelPriority),
        validator: None,
    },
    Setting {
        canonical: "create_default_packages",
        aliases: &[],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "disallowed_packages",
        aliases: &["disallow"],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "force_reinstall",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "pinned_packages",
        aliases: &[],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "prefix_data_interoperability",
        aliases: &["pip_interop_enabled"],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "track_features",
        aliases: &[],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "solver",
        aliases: &["experimental_solver"],
        kind: ValueKind::PlainString,
        validator: None,
    },
    // §4.5 Package Linking and Install-time Configuration
    Setting {
        canonical: "allow_softlinks",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "always_copy",
        aliases: &["copy"],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "always_softlink",
        aliases: &["softlink"],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "path_conflict",
        aliases: &[],
        kind: ValueKind::Enum(EnumKind::PathConflict),
        validator: None,
    },
    Setting {
        canonical: "rollback_enabled",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "safety_checks",
        aliases: &[],
        kind: ValueKind::Enum(EnumKind::SafetyChecks),
        validator: None,
    },
    Setting {
        canonical: "extra_safety_checks",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "signing_metadata_url_base",
        aliases: &[],
        kind: ValueKind::NullableString,
        validator: None,
    },
    Setting {
        canonical: "shortcuts",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "shortcuts_only",
        aliases: &[],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "non_admin_enabled",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "separate_format_cache",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "verify_threads",
        aliases: &[],
        kind: ValueKind::Int,
        validator: None,
    },
    Setting {
        canonical: "execute_threads",
        aliases: &[],
        kind: ValueKind::Int,
        validator: None,
    },
    // §4.7 Output, Prompt, and Flow Control Configuration
    Setting {
        canonical: "always_yes",
        aliases: &["yes"],
        kind: ValueKind::NullableBool,
        validator: None,
    },
    Setting {
        canonical: "auto_activate",
        aliases: &["auto_activate_base"],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "default_activation_env",
        aliases: &[],
        kind: ValueKind::PlainString,
        validator: None,
    },
    Setting {
        canonical: "auto_stack",
        aliases: &[],
        kind: ValueKind::Int,
        validator: None,
    },
    Setting {
        canonical: "changeps1",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "env_prompt",
        aliases: &[],
        kind: ValueKind::PlainString,
        validator: None,
    },
    Setting {
        canonical: "json",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "console",
        aliases: &[],
        kind: ValueKind::PlainString,
        validator: None,
    },
    Setting {
        canonical: "notify_outdated_conda",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "quiet",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "report_errors",
        aliases: &[],
        kind: ValueKind::NullableBool,
        validator: None,
    },
    Setting {
        canonical: "show_channel_urls",
        aliases: &[],
        kind: ValueKind::NullableBool,
        validator: None,
    },
    Setting {
        canonical: "list_fields",
        aliases: &[],
        kind: ValueKind::ListFieldsSeq,
        validator: Some(SemanticValidator::ListFields),
    },
    Setting {
        canonical: "verbosity",
        aliases: &["verbose"],
        kind: ValueKind::Int,
        validator: None,
    },
    Setting {
        canonical: "unsatisfiable_hints",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "unsatisfiable_hints_check_depth",
        aliases: &[],
        kind: ValueKind::Int,
        validator: None,
    },
    Setting {
        canonical: "number_channel_notices",
        aliases: &[],
        kind: ValueKind::Int,
        validator: None,
    },
    Setting {
        canonical: "envvars_force_uppercase",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "export_platforms",
        aliases: &["extra_platforms"],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "override_virtual_packages",
        aliases: &["virtual_packages"],
        kind: ValueKind::NullableStringMap,
        validator: None,
    },
    // §4.9 Hidden and Undocumented
    Setting {
        canonical: "allow_cycles",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "allow_conda_downgrades",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "add_pip_as_python_dependency",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "debug",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "trace",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "dev",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "default_python",
        aliases: &[],
        kind: ValueKind::NullableString,
        validator: Some(SemanticValidator::DefaultPython),
    },
    Setting {
        canonical: "enable_private_envs",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "error_upload_url",
        aliases: &[],
        kind: ValueKind::PlainString,
        validator: None,
    },
    Setting {
        canonical: "force_32bit",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "root_prefix",
        aliases: &["root_dir"],
        kind: ValueKind::PlainString,
        validator: None,
    },
    Setting {
        canonical: "sat_solver",
        aliases: &[],
        kind: ValueKind::Enum(EnumKind::SatSolver),
        validator: None,
    },
    Setting {
        canonical: "solver_ignore_timestamps",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "subdir",
        aliases: &[],
        kind: ValueKind::PlainString,
        validator: None,
    },
    Setting {
        canonical: "subdirs",
        aliases: &[],
        kind: ValueKind::StringSeq,
        validator: None,
    },
    Setting {
        canonical: "target_prefix_override",
        aliases: &[],
        kind: ValueKind::PlainString,
        validator: None,
    },
    Setting {
        canonical: "register_envs",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    Setting {
        canonical: "protect_frozen_envs",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    // §4.10 Plugin Configuration
    Setting {
        canonical: "no_plugins",
        aliases: &[],
        kind: ValueKind::Bool,
        validator: None,
    },
    // §4.11 Experimental
    Setting {
        canonical: "environment_specifier",
        aliases: &["env_spec"],
        kind: ValueKind::NullableString,
        validator: None,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_has_99_entries() {
        assert_eq!(CATALOG.len(), 99);
    }

    #[test]
    fn canonical_names_and_aliases_are_all_unique() {
        let mut seen: HashSet<&str> = HashSet::new();
        for setting in CATALOG {
            assert!(
                seen.insert(setting.canonical),
                "duplicate name: {}",
                setting.canonical
            );
            for alias in setting.aliases {
                assert!(seen.insert(alias), "duplicate name: {alias}");
            }
        }
    }
}
