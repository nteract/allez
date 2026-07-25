//! The public `Config` struct and its supporting value types (`ChannelPriority`, `PathConflict`,
//! `SafetyChecks`, `SatSolver`, `BoolOrInt`, `SslVerify`, `ListField`, `ChannelSetting`,
//! `ParseOptions`). See data-model.md §2, §3, §6.

use std::collections::{BTreeMap, HashMap};

/// The typed, in-memory result of parsing one `.condarc` document. Every
/// field mirrors one recognized conda setting (`docs/condarc_research.md`
/// §4); a field is `None` iff the setting was absent from the document —
/// there is no defaults table (FR-038). Aliases (FR-011) are resolved to
/// the canonical field shown here before the document is parsed into this
/// struct: setting either `channels` or its alias `channel` in the YAML
/// populates `Config::channels`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Config {
    // ---- 2.1.1 Channel Configuration (§4.1) ----
    /// Channel names/URLs to search, in priority order. Alias: `channel`.
    pub channels: Option<Vec<String>>,
    /// Base URL prepended to bare channel names. Must have a URL scheme
    /// unless empty (FR-025).
    pub channel_alias: Option<String>,
    /// Per-channel auth/proxy settings. `channel` key is a documentation
    /// convention, not source-enforced (research §6).
    pub channel_settings: Option<Vec<ChannelSetting>>,
    /// Channels implied by the `defaults` multichannel name.
    pub default_channels: Option<Vec<String>>,
    pub override_channels_enabled: Option<bool>,
    /// Alias: `whitelist_channels`.
    pub allowlist_channels: Option<Vec<String>>,
    pub denylist_channels: Option<Vec<String>>,
    /// Custom channel name -> base URL.
    pub custom_channels: Option<BTreeMap<String, String>>,
    /// Multichannel name -> list of member channel names/URLs.
    pub custom_multichannels: Option<BTreeMap<String, Vec<String>>>,
    pub migrated_channel_aliases: Option<Vec<String>>,
    pub migrated_custom_channels: Option<BTreeMap<String, String>>,
    /// Alias: `add_binstar_token`.
    pub add_anaconda_token: Option<bool>,
    pub allow_non_channel_urls: Option<bool>,
    pub repodata_fns: Option<Vec<String>>,
    /// Nullable: tri-state like `always_yes` (FR-013).
    pub use_only_tar_bz2: Option<Option<bool>>,
    pub repodata_threads: Option<i64>,
    pub fetch_threads: Option<i64>,
    /// Free-form experimental feature-flag strings (not a closed enum).
    pub experimental: Option<Vec<String>>,
    pub no_lock: Option<bool>,
    pub repodata_use_zst: Option<bool>,
    pub repodata_use_shards: Option<bool>,

    // ---- 2.1.2 Basic Conda Configuration (§4.2) ----
    /// Alias: `envs_path`.
    pub envs_dirs: Option<Vec<String>>,
    pub pkgs_dirs: Option<Vec<String>>,
    pub default_threads: Option<i64>,
    /// Free-form preview feature-flag strings.
    pub preview: Option<Vec<String>>,

    // ---- 2.1.3 Network Configuration (§4.3) ----
    /// Nullable. Alias: `client_cert`. Required if `client_ssl_cert_key`
    /// is set (FR-027).
    pub client_ssl_cert: Option<Option<String>>,
    /// Nullable. Alias: `client_cert_key`.
    pub client_ssl_cert_key: Option<Option<String>>,
    /// `(bool, int)`, narrower boolish vocabulary than plain `bool`
    /// (FR-020).
    pub local_repodata_ttl: Option<BoolOrInt>,
    pub offline: Option<bool>,
    /// Scheme/host -> proxy URL, or explicit `null`.
    pub proxy_servers: Option<BTreeMap<String, Option<String>>>,
    pub remote_connect_timeout_secs: Option<f64>,
    pub remote_max_retries: Option<i64>,
    /// `int`, not `float`, despite the name (research §4.3 note).
    pub remote_backoff_factor: Option<i64>,
    pub remote_read_timeout_secs: Option<f64>,
    /// `(str, bool)` + the opt-in filesystem-existence branch (FR-024,
    /// research R6). Alias: `verify_ssl`.
    pub ssl_verify: Option<SslVerify>,

    // ---- 2.1.4 Solver Configuration (§4.4) ----
    pub aggressive_update_packages: Option<Vec<String>>,
    /// Alias: `self_update`.
    pub auto_update_conda: Option<bool>,
    pub channel_priority: Option<ChannelPriority>,
    pub create_default_packages: Option<Vec<String>>,
    /// Alias: `disallow`.
    pub disallowed_packages: Option<Vec<String>>,
    pub force_reinstall: Option<bool>,
    pub pinned_packages: Option<Vec<String>>,
    /// Alias: `pip_interop_enabled`.
    pub prefix_data_interoperability: Option<bool>,
    pub track_features: Option<Vec<String>>,
    /// Plain string, **not** a closed enum — solver plugins register
    /// arbitrary names (research §4.4 note). Alias: `experimental_solver`.
    pub solver: Option<String>,

    // ---- 2.1.5 Package Linking and Install-time Configuration (§4.5) ----
    pub allow_softlinks: Option<bool>,
    /// Alias: `copy`. Mutually exclusive with `always_softlink` (FR-028).
    pub always_copy: Option<bool>,
    /// Alias: `softlink`.
    pub always_softlink: Option<bool>,
    pub path_conflict: Option<PathConflict>,
    pub rollback_enabled: Option<bool>,
    pub safety_checks: Option<SafetyChecks>,
    pub extra_safety_checks: Option<bool>,
    /// Nullable.
    pub signing_metadata_url_base: Option<Option<String>>,
    pub shortcuts: Option<bool>,
    /// Package names to restrict shortcut creation to.
    pub shortcuts_only: Option<Vec<String>>,
    pub non_admin_enabled: Option<bool>,
    pub separate_format_cache: Option<bool>,
    pub verify_threads: Option<i64>,
    pub execute_threads: Option<i64>,

    // ---- 2.1.6 Output, Prompt, and Flow Control Configuration (§4.7) ----
    /// Nullable. Alias: `yes`.
    pub always_yes: Option<Option<bool>>,
    /// Auto-activate the base environment in new shells. **Canonical name is
    /// `auto_activate`**; the historically-documented `auto_activate_base`
    /// spelling is the *alias* (verified against conda 26.5.3:
    /// `Context.auto_activate`'s `ParameterLoader.name == "auto_activate"`,
    /// `aliases == ("auto_activate_base",)`), which is also why every
    /// `expected/*.json` fixture records this setting as `auto_activate`.
    pub auto_activate: Option<bool>,
    pub default_activation_env: Option<String>,
    pub auto_stack: Option<i64>,
    pub changeps1: Option<bool>,
    pub env_prompt: Option<String>,
    pub json: Option<bool>,
    /// Plain string, not a closed enum — reporter-backend plugins register
    /// arbitrary names.
    pub console: Option<String>,
    pub notify_outdated_conda: Option<bool>,
    pub quiet: Option<bool>,
    /// Nullable. Deprecated upstream but still a settable/loadable
    /// parameter.
    pub report_errors: Option<Option<bool>>,
    /// Nullable.
    pub show_channel_urls: Option<Option<bool>>,
    /// Elements restricted to the closed `CONDA_LIST_FIELDS` vocabulary
    /// (FR-022, §3.3).
    pub list_fields: Option<Vec<ListField>>,
    /// Alias: `verbose`.
    pub verbosity: Option<i64>,
    pub unsatisfiable_hints: Option<bool>,
    pub unsatisfiable_hints_check_depth: Option<i64>,
    pub number_channel_notices: Option<i64>,
    pub envvars_force_uppercase: Option<bool>,
    /// Alias: `extra_platforms`.
    pub export_platforms: Option<Vec<String>>,
    /// Virtual-package name -> version-or-build override, or explicit
    /// `null`. Alias: `virtual_packages`.
    pub override_virtual_packages: Option<BTreeMap<String, Option<String>>>,

    // ---- 2.1.7 Hidden and Undocumented (§4.9) ----
    pub allow_cycles: Option<bool>,
    pub allow_conda_downgrades: Option<bool>,
    pub add_pip_as_python_dependency: Option<bool>,
    pub debug: Option<bool>,
    pub trace: Option<bool>,
    pub dev: Option<bool>,
    /// Nullable. Empty string / null = no pinning; else `2.x`/`3.x`
    /// (FR-026).
    pub default_python: Option<Option<String>>,
    pub enable_private_envs: Option<bool>,
    /// Deprecated upstream read-side property; still loadable.
    pub error_upload_url: Option<String>,
    pub force_32bit: Option<bool>,
    /// Alias: `root_dir`.
    pub root_prefix: Option<String>,
    pub sat_solver: Option<SatSolver>,
    pub solver_ignore_timestamps: Option<bool>,
    pub subdir: Option<String>,
    pub subdirs: Option<Vec<String>>,
    pub target_prefix_override: Option<String>,
    pub register_envs: Option<bool>,
    pub protect_frozen_envs: Option<bool>,

    // ---- 2.1.8 Plugin Configuration (§4.10) ----
    pub no_plugins: Option<bool>,

    // ---- 2.1.9 Experimental (§4.11) ----
    /// Nullable. **EXPERIMENTAL** upstream; expect breaking changes.
    /// Alias: `env_spec`.
    pub environment_specifier: Option<Option<String>>,

    // ---- 2.1.10 Unknown-key escape hatch (research R2, FR-036) ----
    /// Top-level keys not in the recognized catalog above. Never an error
    /// (FR-036), never adapter-emitted (FR-040). Values are lowered from
    /// `RawValue` to a neutral `serde_json::Value` so this frontend-
    /// independent type is the only YAML-derived thing that crosses the
    /// public API (research R1/R2). Use [`Config::extra_as`] to
    /// deserialize the whole tail into a caller-defined struct in one call.
    pub extra: HashMap<String, serde_json::Value>,
}

impl Config {
    /// Deserialize the entire unknown-key tail into a caller-chosen type
    /// `T` in one call. Fields `T` doesn't declare are dropped (ordinary
    /// serde; `extra` is not `deny_unknown_fields`); fields `T` declares
    /// that are absent from the document deserialize however `T` handles
    /// a missing field (typically `None` for `Option<_>`).
    ///
    /// # Errors
    /// Returns `serde_json::Error` if `T`'s shape doesn't match the
    /// retained values (e.g. a declared field's JSON value has the wrong
    /// type for `T`'s corresponding field).
    pub fn extra_as<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        let obj = serde_json::Value::Object(
            self.extra
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        );
        serde_json::from_value(obj)
    }
}

/// `channel_priority` — accepts the lowercase value, the SHOUTY-CASE member
/// name, or a JSON boolean / boolish string via the historical compat shim
/// (FR-016/017). Adapter emits the lowercase value (`"strict"`).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPriority {
    Strict,
    Flexible,
    Disabled,
}

/// `path_conflict`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathConflict {
    Clobber,
    Warn,
    Prevent,
}

/// `safety_checks`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyChecks {
    Enabled,
    Warn,
    Disabled,
}

/// `sat_solver`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SatSolver {
    Pycosat,
    Pycryptosat,
    Pysat,
}

/// `local_repodata_ttl`'s `(bool, int)` element type — narrower boolish
/// vocabulary than plain `bool` (FR-020, research §8 items 6/11).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoolOrInt {
    Bool(bool),
    Int(i64),
}

/// `ssl_verify`'s `(str, bool)` element type (FR-024).
#[derive(Debug, Clone, PartialEq)]
pub enum SslVerify {
    Bool(bool),
    /// The literal string `"truststore"`.
    Truststore,
    /// A certificate path (any non-boolish, non-`truststore` string). With
    /// the default, side-effect-free options this is accepted *without*
    /// consulting the filesystem; with
    /// `ParseOptions::ssl_verify_fs_check` set, a path that does not exist
    /// is rejected instead of producing this variant (research R6, §6).
    Path(String),
}

/// One closed member of `CONDA_LIST_FIELDS` (FR-022, §5.9) — 25 members,
/// matched exactly and case-sensitively with no trimming.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListField {
    Arch,
    Build,
    BuildNumber,
    Channel,
    ChannelName,
    Constrains,
    Depends,
    DistStr,
    Features,
    Fn,
    License,
    LicenseFamily,
    Md5,
    Name,
    Noarch,
    PackageType,
    RequestedSpec,
    RequestedSpecs,
    Sha256,
    Size,
    Subdir,
    Timestamp,
    TrackFeatures,
    Url,
    Version,
}

/// One `channel_settings` list entry — a string-to-string map. The
/// `channel` key is a documentation convention (settings.rst), **not**
/// source-enforced by any `context.py` validation callable (research §6),
/// so it is not required at parse time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChannelSetting(pub BTreeMap<String, String>);

/// Runtime options for [`crate::parse_with_options`]. `ParseOptions::default()`
/// is the side-effect-free behavior used by [`crate::parse`]: parsing is then a
/// pure function of the input string, with no filesystem, network, or
/// environment access at all (FR-002).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParseOptions {
    /// Opt into conda's `ssl_verify` path-existence check.
    ///
    /// Default `false`: a non-boolish, non-`truststore` `ssl_verify`
    /// string is accepted as an unverified certificate path
    /// ([`SslVerify::Path`]), and the crate touches nothing outside its
    /// input.
    ///
    /// Set to `true` to additionally require that such a path exists on
    /// the local filesystem, rejecting it otherwise — exactly conda's
    /// runtime rule (FR-024). This is the only filesystem access the
    /// crate can ever perform, and only on explicit request.
    pub ssl_verify_fs_check: bool,

    /// Opt into conda's own class-level default for an explicit YAML `null` on a
    /// `SequenceParameter`- or `MapParameter`-typed setting (`channels`, `custom_channels`,
    /// `channel_settings`, and similar list-/dict-shaped settings — data-model.md §4 marks each
    /// one's [`crate::catalog::ValueKind`]).
    ///
    /// Default `false`: an explicit top-level `null` for one of these settings is treated
    /// identically to that key being entirely absent from the document (FR-038's "no
    /// defaulting" posture) — the crate never maintains a conda-defaults table, so the
    /// resulting `Config` field stays `None` either way.
    ///
    /// Set to `true` to instead resolve an explicit `null` for one of these settings to conda's
    /// own default for it. This exists because conda's `SequenceParameter`/`MapParameter` raw-
    /// value matching filters out `None`-valued matches *before* `.load()` ever sees them —
    /// making an explicit top-level `null` genuinely indistinguishable, at that layer, from the
    /// key never having appeared in the file at all — so conda's own class-level default (the
    /// empty list/dict for most such settings; a documented non-empty default for
    /// `custom_channels`, `default_channels`, `repodata_fns`, `aggressive_update_packages`, and
    /// `list_fields`) is what conda's live `context.<setting>` actually reads back as. This is
    /// deliberately *not* the default, and it deliberately does not apply to an absent key —
    /// only an explicit `null` on one of these specific setting kinds resolves to conda's
    /// default; every other setting kind, and every absent key of any kind, is completely
    /// unaffected regardless of this option (still `None`, still FR-038). See
    /// docs/condarc_research.md item 22.
    pub null_sequence_map_defaults: bool,
}

impl ParseOptions {
    /// Builder-style setter for [`ssl_verify_fs_check`](Self::ssl_verify_fs_check).
    ///
    /// `#[non_exhaustive]` deliberately disallows struct-literal construction (even with
    /// `..Default::default()`) from outside this crate, so an external caller opts in via
    /// `ParseOptions::default().with_ssl_verify_fs_check(true)` rather than a struct literal.
    /// Adding a future option field only needs a new builder method here, never a breaking
    /// change to this one (Constitution VI's "adding an option field is MINOR").
    #[must_use]
    pub fn with_ssl_verify_fs_check(mut self, value: bool) -> Self {
        self.ssl_verify_fs_check = value;
        self
    }

    /// Builder-style setter for
    /// [`null_sequence_map_defaults`](Self::null_sequence_map_defaults).
    #[must_use]
    pub fn with_null_sequence_map_defaults(mut self, value: bool) -> Self {
        self.null_sequence_map_defaults = value;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Config::default()` has every field `None`/empty (FR-038). Every
    /// field is checked explicitly (not just a representative sample) so
    /// this test fails loudly if a future field forgets to default to
    /// "absent."
    #[test]
    fn default_config_has_every_field_absent() {
        let cfg = Config::default();

        // §4.1 Channel Configuration
        assert_eq!(cfg.channels, None);
        assert_eq!(cfg.channel_alias, None);
        assert_eq!(cfg.channel_settings, None);
        assert_eq!(cfg.default_channels, None);
        assert_eq!(cfg.override_channels_enabled, None);
        assert_eq!(cfg.allowlist_channels, None);
        assert_eq!(cfg.denylist_channels, None);
        assert_eq!(cfg.custom_channels, None);
        assert_eq!(cfg.custom_multichannels, None);
        assert_eq!(cfg.migrated_channel_aliases, None);
        assert_eq!(cfg.migrated_custom_channels, None);
        assert_eq!(cfg.add_anaconda_token, None);
        assert_eq!(cfg.allow_non_channel_urls, None);
        assert_eq!(cfg.repodata_fns, None);
        assert_eq!(cfg.use_only_tar_bz2, None);
        assert_eq!(cfg.repodata_threads, None);
        assert_eq!(cfg.fetch_threads, None);
        assert_eq!(cfg.experimental, None);
        assert_eq!(cfg.no_lock, None);
        assert_eq!(cfg.repodata_use_zst, None);
        assert_eq!(cfg.repodata_use_shards, None);

        // §4.2 Basic Conda Configuration
        assert_eq!(cfg.envs_dirs, None);
        assert_eq!(cfg.pkgs_dirs, None);
        assert_eq!(cfg.default_threads, None);
        assert_eq!(cfg.preview, None);

        // §4.3 Network Configuration
        assert_eq!(cfg.client_ssl_cert, None);
        assert_eq!(cfg.client_ssl_cert_key, None);
        assert_eq!(cfg.local_repodata_ttl, None);
        assert_eq!(cfg.offline, None);
        assert_eq!(cfg.proxy_servers, None);
        assert_eq!(cfg.remote_connect_timeout_secs, None);
        assert_eq!(cfg.remote_max_retries, None);
        assert_eq!(cfg.remote_backoff_factor, None);
        assert_eq!(cfg.remote_read_timeout_secs, None);
        assert_eq!(cfg.ssl_verify, None);

        // §4.4 Solver Configuration
        assert_eq!(cfg.aggressive_update_packages, None);
        assert_eq!(cfg.auto_update_conda, None);
        assert_eq!(cfg.channel_priority, None);
        assert_eq!(cfg.create_default_packages, None);
        assert_eq!(cfg.disallowed_packages, None);
        assert_eq!(cfg.force_reinstall, None);
        assert_eq!(cfg.pinned_packages, None);
        assert_eq!(cfg.prefix_data_interoperability, None);
        assert_eq!(cfg.track_features, None);
        assert_eq!(cfg.solver, None);

        // §4.5 Package Linking and Install-time Configuration
        assert_eq!(cfg.allow_softlinks, None);
        assert_eq!(cfg.always_copy, None);
        assert_eq!(cfg.always_softlink, None);
        assert_eq!(cfg.path_conflict, None);
        assert_eq!(cfg.rollback_enabled, None);
        assert_eq!(cfg.safety_checks, None);
        assert_eq!(cfg.extra_safety_checks, None);
        assert_eq!(cfg.signing_metadata_url_base, None);
        assert_eq!(cfg.shortcuts, None);
        assert_eq!(cfg.shortcuts_only, None);
        assert_eq!(cfg.non_admin_enabled, None);
        assert_eq!(cfg.separate_format_cache, None);
        assert_eq!(cfg.verify_threads, None);
        assert_eq!(cfg.execute_threads, None);

        // §4.7 Output, Prompt, and Flow Control Configuration
        assert_eq!(cfg.always_yes, None);
        assert_eq!(cfg.auto_activate, None);
        assert_eq!(cfg.default_activation_env, None);
        assert_eq!(cfg.auto_stack, None);
        assert_eq!(cfg.changeps1, None);
        assert_eq!(cfg.env_prompt, None);
        assert_eq!(cfg.json, None);
        assert_eq!(cfg.console, None);
        assert_eq!(cfg.notify_outdated_conda, None);
        assert_eq!(cfg.quiet, None);
        assert_eq!(cfg.report_errors, None);
        assert_eq!(cfg.show_channel_urls, None);
        assert_eq!(cfg.list_fields, None);
        assert_eq!(cfg.verbosity, None);
        assert_eq!(cfg.unsatisfiable_hints, None);
        assert_eq!(cfg.unsatisfiable_hints_check_depth, None);
        assert_eq!(cfg.number_channel_notices, None);
        assert_eq!(cfg.envvars_force_uppercase, None);
        assert_eq!(cfg.export_platforms, None);
        assert_eq!(cfg.override_virtual_packages, None);

        // §4.9 Hidden and Undocumented
        assert_eq!(cfg.allow_cycles, None);
        assert_eq!(cfg.allow_conda_downgrades, None);
        assert_eq!(cfg.add_pip_as_python_dependency, None);
        assert_eq!(cfg.debug, None);
        assert_eq!(cfg.trace, None);
        assert_eq!(cfg.dev, None);
        assert_eq!(cfg.default_python, None);
        assert_eq!(cfg.enable_private_envs, None);
        assert_eq!(cfg.error_upload_url, None);
        assert_eq!(cfg.force_32bit, None);
        assert_eq!(cfg.root_prefix, None);
        assert_eq!(cfg.sat_solver, None);
        assert_eq!(cfg.solver_ignore_timestamps, None);
        assert_eq!(cfg.subdir, None);
        assert_eq!(cfg.subdirs, None);
        assert_eq!(cfg.target_prefix_override, None);
        assert_eq!(cfg.register_envs, None);
        assert_eq!(cfg.protect_frozen_envs, None);

        // §4.10 Plugin Configuration
        assert_eq!(cfg.no_plugins, None);

        // §4.11 Experimental
        assert_eq!(cfg.environment_specifier, None);

        // Unknown-key escape hatch
        assert!(cfg.extra.is_empty());
    }

    #[test]
    fn extra_as_deserializes_the_whole_tail_in_one_call() {
        #[derive(serde::Deserialize, Debug, Default, PartialEq)]
        struct CondaBuildConfig {
            croot: Option<String>,
            bld_path: Option<String>,
            anaconda_upload: Option<bool>,
        }

        let mut cfg = Config::default();
        cfg.extra.insert(
            "croot".to_string(),
            serde_json::json!("/home/user/conda-bld"),
        );
        cfg.extra
            .insert("anaconda_upload".to_string(), serde_json::json!(false));
        cfg.extra.insert(
            "some_other_unmodeled_key".to_string(),
            serde_json::json!(42),
        );

        let build: CondaBuildConfig = cfg.extra_as().expect("shape matches");
        assert_eq!(
            build,
            CondaBuildConfig {
                croot: Some("/home/user/conda-bld".to_string()),
                bld_path: None,
                anaconda_upload: Some(false),
            }
        );
    }

    #[test]
    fn parse_options_default_disables_ssl_verify_fs_check() {
        assert_eq!(
            ParseOptions::default(),
            ParseOptions {
                ssl_verify_fs_check: false,
                null_sequence_map_defaults: false,
            }
        );
    }
}
