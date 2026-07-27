//! YAML entry point, single-document/root-shape gate, `yaml_rust2::Yaml` -> `RawValue` lowering,
//! and the known/unknown key split. See data-model.md §1 and §6.

use std::collections::HashSet;

use indexmap::IndexMap;
use yaml_rust2::{Yaml, YamlLoader};

use crate::catalog::{CATALOG, SemanticValidator, ValueKind};
use crate::coerce::enums::EnumResult;
use crate::coerce::{self, CoercionError, input_repr};
use crate::error::{ErrorEntry, ErrorKind, InputRepr, Location, PathSegment, ValidationReport};
use crate::model::{Config, ParseOptions};

/// The internal, owned mirror of `yaml_rust2::Yaml`'s resolved-scalar-type tree (data-model.md
/// §1). Exists so no other module depends on `yaml-rust2`'s types directly (research R1: a
/// parser swap is then a single-module change). Records the resolved scalar *kind*
/// (bool/int/float/null/string), not a deserialization into [`crate::model::Config`] — that
/// happens later, in `catalog.rs` + `coerce/`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RawValue {
    /// `Yaml::Null` (bare `~`/`null`/empty), or `Yaml::BadValue` (never actually produced by a
    /// successful parse, but total coverage costs nothing).
    Null,
    /// `Yaml::Boolean`.
    Bool(bool),
    /// `Yaml::Integer`.
    Int(i64),
    /// `Yaml::Real`, parsed once here (never re-parsed downstream). A *bare* numeral too large
    /// for `i64` (so `yaml-rust2`'s own integer parse fails) but still parseable as `f64` also
    /// arrives as `Yaml::Real` and lowers here — `f64::parse` succeeds (possibly as `inf`) for
    /// essentially any all-digit string, so this is the fallback a bare over-range numeral
    /// actually takes, not [`RawValue::Str`]. The A1 range check that rejects such a value for
    /// an `Int`-typed setting happens downstream, in `coerce/numeric.rs`.
    Float(f64),
    /// `Yaml::String` (quoted or plain; whitespace preserved). A *quoted* bignum numeral (per
    /// A1's conformance fixtures) lowers here, since quoting always yields `Yaml::String`
    /// regardless of magnitude; a *bare* over-range numeral instead lowers to [`RawValue::Float`]
    /// (see that variant's doc comment) and is range-checked downstream on the float side.
    Str(String),
    Seq(Vec<RawValue>),
    /// Insertion-ordered (`IndexMap`) -> deterministic `MultipleKeysError`/error-entry ordering.
    /// String keys only: a non-string `Yaml::Hash` key has no representable slot here by
    /// construction (data-model.md §1 "Out of scope") and is dropped by [`lower`]; the
    /// caller-facing `type_coercion` entry for that (FR-007b) is raised by the per-key dispatch
    /// loop, which has the enclosing location this function does not.
    Map(IndexMap<String, RawValue>),
}

/// Lower the document root into a [`RawValue`] tree, also collecting a `type_coercion` entry for
/// every non-string mapping key encountered anywhere in the document (FR-007b) instead of
/// silently dropping it as the plain [`lower`] does. A dropped key's location is the nearest
/// enclosing top-level key name as written (or [`Location::Root`] for a root-level offender) —
/// data-model.md §1's "located at the enclosing setting" note; this runs *before* any
/// canonical/alias catalog lookup, so the location is the key exactly as the document spelled it.
pub(crate) fn lower_document(yaml: &Yaml) -> (RawValue, Vec<ErrorEntry>) {
    let mut entries = Vec::new();
    let value = lower_tracking_dropped_keys(yaml, None, &mut Vec::new(), &mut entries);
    (value, entries)
}

fn lower_tracking_dropped_keys(
    yaml: &Yaml,
    enclosing_setting: Option<&str>,
    path: &mut Vec<PathSegment>,
    entries: &mut Vec<ErrorEntry>,
) -> RawValue {
    match yaml {
        Yaml::Array(items) => RawValue::Seq(
            items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    path.push(PathSegment::Index { index });
                    let lowered =
                        lower_tracking_dropped_keys(item, enclosing_setting, path, entries);
                    path.pop();
                    lowered
                })
                .collect(),
        ),
        Yaml::Hash(hash) => {
            let mut map = IndexMap::with_capacity(hash.len());
            for (key, value) in hash {
                match key {
                    Yaml::String(key_str) => {
                        // The first string key we descend through becomes the "enclosing
                        // setting" for everything nested under it; deeper non-string keys keep
                        // attributing to that same top-level name (data-model.md §1).
                        let nested_context = enclosing_setting.or(Some(key_str.as_str()));
                        if enclosing_setting.is_some() {
                            path.push(PathSegment::Key {
                                key: key_str.clone(),
                            });
                        }
                        let lowered = lower_tracking_dropped_keys(
                            value,
                            nested_context,
                            path,
                            entries,
                        );
                        if enclosing_setting.is_some() {
                            path.pop();
                        }
                        map.insert(
                            key_str.clone(),
                            lowered,
                        );
                    }
                    non_string_key => {
                        let location = match enclosing_setting {
                            Some(setting) if path.is_empty() => Location::Setting {
                                setting: setting.to_string(),
                            },
                            Some(setting) => Location::Nested {
                                setting: setting.to_string(),
                                path: path.clone(),
                            },
                            None => Location::Root,
                        };
                        entries.push(ErrorEntry {
                            location,
                            kind: ErrorKind::TypeCoercion,
                            message: "mapping key is not a string and was dropped".to_string(),
                            input: input_repr(&lower(non_string_key)),
                            involved: Vec::new(),
                        });
                    }
                }
            }
            RawValue::Map(map)
        }
        // Scalars/Null/BadValue/Alias never contain nested mapping keys, so the plain, untracked
        // `lower` is equivalent here.
        other => lower(other),
    }
}

/// Lower a single `yaml_rust2::Yaml` value into [`RawValue`] via one hand-written recursive
/// match (research R1). `yaml-rust2` resolves anchors/aliases and merge keys before this point,
/// so whatever they expand to arrives here as an ordinary value; nothing below needs to
/// understand them (data-model.md §1).
pub(crate) fn lower(yaml: &Yaml) -> RawValue {
    match yaml {
        Yaml::Null | Yaml::BadValue | Yaml::Alias(_) => RawValue::Null,
        Yaml::Boolean(b) => RawValue::Bool(*b),
        Yaml::Integer(i) => RawValue::Int(*i),
        Yaml::Real(text) => match yaml.as_f64() {
            Some(f) => RawValue::Float(f),
            // yaml-rust2 only ever classifies a scalar as `Real` once its own scanner has
            // confirmed it parses as a float; this arm exists purely so `lower` stays total.
            None => RawValue::Str(text.clone()),
        },
        Yaml::String(s) => RawValue::Str(s.clone()),
        Yaml::Array(items) => RawValue::Seq(items.iter().map(lower).collect()),
        Yaml::Hash(hash) => {
            let mut map = IndexMap::with_capacity(hash.len());
            for (key, value) in hash {
                if let Yaml::String(key) = key {
                    map.insert(key.clone(), lower(value));
                }
                // A non-`Yaml::String` key is intentionally dropped here (FR-007b); see the
                // `Map` variant's doc comment above.
            }
            RawValue::Map(map)
        }
    }
}

/// One coerced value, tagged by which coercion function produced it. A thin sum type so the
/// per-key dispatch loop can call a single generic [`coerce_value`] dispatcher (driven by
/// `catalog.rs`'s [`ValueKind`], Constitution IV) and then hand the result to
/// [`apply_to_config`], which knows the one [`Config`] field each `canonical` name maps to.
enum Coerced {
    Bool(bool),
    NullableBool(Option<bool>),
    SslVerify(crate::model::SslVerify),
    BoolOrInt(crate::model::BoolOrInt),
    Int(i64),
    Float(f64),
    PlainString(String),
    NullableString(Option<String>),
    Enum(EnumResult),
    StringSeq(Option<Vec<String>>),
    ListFieldsSeq(Option<Vec<crate::model::ListField>>),
    StringMap(Option<std::collections::BTreeMap<String, String>>),
    NullableStringMap(Option<std::collections::BTreeMap<String, Option<String>>>),
    StringSeqMap(Option<std::collections::BTreeMap<String, Vec<String>>>),
    ChannelSettingsSeq(Option<Vec<crate::model::ChannelSetting>>),
}

/// Dispatch one raw value through the coercion function its setting's [`ValueKind`] names
/// (data-model.md §4). The single point where `ValueKind` is turned into an actual coercer call.
fn coerce_value(kind: ValueKind, raw: &RawValue) -> Result<Coerced, CoercionError> {
    match kind {
        ValueKind::Bool => coerce::boolish::coerce_bool(raw).map(Coerced::Bool),
        ValueKind::NullableBool => {
            coerce::boolish::coerce_nullable_bool(raw).map(Coerced::NullableBool)
        }
        ValueKind::SslVerifyKind => coerce::boolish::coerce_ssl_verify(raw).map(Coerced::SslVerify),
        ValueKind::BoolOrIntKind => {
            coerce::numeric::coerce_bool_or_int(raw).map(Coerced::BoolOrInt)
        }
        ValueKind::Int => coerce::numeric::coerce_int(raw).map(Coerced::Int),
        ValueKind::Float => coerce::numeric::coerce_float(raw).map(Coerced::Float),
        ValueKind::PlainString => {
            coerce::strings::coerce_plain_string(raw).map(Coerced::PlainString)
        }
        ValueKind::NullableString => {
            coerce::strings::coerce_nullable_string(raw).map(Coerced::NullableString)
        }
        ValueKind::Enum(enum_kind) => coerce::enums::coerce_enum(raw, enum_kind).map(Coerced::Enum),
        ValueKind::StringSeq => coerce::sequences::coerce_string_seq(raw).map(Coerced::StringSeq),
        ValueKind::ListFieldsSeq => {
            coerce::sequences::coerce_list_fields_seq(raw).map(Coerced::ListFieldsSeq)
        }
        ValueKind::StringMap => coerce::sequences::coerce_string_map(raw).map(Coerced::StringMap),
        ValueKind::NullableStringMap => {
            coerce::sequences::coerce_nullable_string_map(raw).map(Coerced::NullableStringMap)
        }
        ValueKind::StringSeqMap => {
            coerce::sequences::coerce_string_seq_map(raw).map(Coerced::StringSeqMap)
        }
        ValueKind::ChannelSettingsSeq => {
            coerce::sequences::coerce_channel_settings_seq(raw).map(Coerced::ChannelSettingsSeq)
        }
    }
}

/// Conda's own class-level default for a `SequenceParameter`- or `MapParameter`-typed setting,
/// used only by [`apply_null_sequence_map_default`] when
/// [`ParseOptions::null_sequence_map_defaults`] is enabled and the setting's raw value in the
/// document was an explicit YAML `null` (see that option's doc comment, and
/// docs/condarc_research.md item 22, for the full rationale). Returns `None` for every
/// `ValueKind` this option doesn't apply to, so the caller can use this as a single combined
/// "does this option even apply to this setting kind" + "what's the default" lookup.
///
/// Every default here is empty except the five documented non-empty ones (`default_channels`,
/// `repodata_fns`, `aggressive_update_packages`, `custom_channels`, `list_fields`) — see
/// data-model.md §4 / docs/condarc_research.md §4 for each setting's conda-documented default.
/// `default_channels`'s default is genuinely platform-dependent in conda itself (`on_win` adds a
/// third `msys2` URL) — mirrored here via `cfg!(windows)`, the same distinction conda's own
/// `DEFAULT_CHANNELS_WIN`/`DEFAULT_CHANNELS_UNIX` split makes.
fn conda_sequence_map_default(canonical: &str, kind: ValueKind) -> Option<Coerced> {
    match kind {
        ValueKind::StringSeq => Some(Coerced::StringSeq(Some(match canonical {
            "default_channels" => default_channels_default(),
            "repodata_fns" => vec![
                "current_repodata.json".to_string(),
                "repodata.json".to_string(),
            ],
            "aggressive_update_packages" => vec![
                "ca-certificates".to_string(),
                "certifi".to_string(),
                "openssl".to_string(),
            ],
            _ => Vec::new(),
        }))),
        ValueKind::ListFieldsSeq => Some(Coerced::ListFieldsSeq(Some(vec![
            crate::model::ListField::Name,
            crate::model::ListField::Version,
            crate::model::ListField::Build,
            crate::model::ListField::ChannelName,
        ]))),
        ValueKind::StringMap => Some(Coerced::StringMap(Some(match canonical {
            "custom_channels" => std::collections::BTreeMap::from([(
                "pkgs/pro".to_string(),
                "https://repo.anaconda.com".to_string(),
            )]),
            _ => std::collections::BTreeMap::new(),
        }))),
        ValueKind::NullableStringMap => Some(Coerced::NullableStringMap(Some(
            std::collections::BTreeMap::new(),
        ))),
        ValueKind::StringSeqMap => Some(Coerced::StringSeqMap(Some(
            std::collections::BTreeMap::new(),
        ))),
        ValueKind::ChannelSettingsSeq => Some(Coerced::ChannelSettingsSeq(Some(Vec::new()))),
        _ => None,
    }
}

/// `DEFAULT_CHANNELS` (conda's own `on_win`-dependent constant) — see
/// [`conda_sequence_map_default`]'s doc comment.
fn default_channels_default() -> Vec<String> {
    let mut channels = vec![
        "https://repo.anaconda.com/pkgs/main".to_string(),
        "https://repo.anaconda.com/pkgs/r".to_string(),
    ];
    if cfg!(windows) {
        channels.push("https://repo.anaconda.com/pkgs/msys2".to_string());
    }
    channels
}

/// If [`ParseOptions::null_sequence_map_defaults`] is enabled and `raw` is an explicit YAML
/// `null`, replaces `coerced` with conda's own class-level default for `canonical`/`kind` (see
/// [`conda_sequence_map_default`]); otherwise returns `coerced` unchanged. Only ever called
/// after a successful [`coerce_value`] — every `Coerced::*Seq`/`*Map` variant that coercion
/// produces for a `RawValue::Null` input already carries `None` (sequences.rs's "null means
/// unset" rule), so this only ever *replaces* a `None`, never overwrites a value the document
/// actually set.
fn apply_null_sequence_map_default(
    coerced: Coerced,
    canonical: &str,
    kind: ValueKind,
    raw: &RawValue,
    options: &ParseOptions,
) -> Coerced {
    if options.null_sequence_map_defaults
        && matches!(raw, RawValue::Null)
        && let Some(default) = conda_sequence_map_default(canonical, kind)
    {
        default
    } else {
        coerced
    }
}

/// Set the one [`Config`] field `canonical` names to the value `coerced` carries (FR-009/010/011).
/// The `(canonical, variant)` pairing here must agree with `catalog.rs`'s `CATALOG` table; a
/// mismatch is a coding bug in this crate, not a possible user input, hence `unreachable!`.
fn apply_to_config(cfg: &mut Config, canonical: &str, coerced: Coerced) {
    use Coerced::*;
    match (canonical, coerced) {
        ("channels", StringSeq(v)) => cfg.channels = v,
        ("channel_alias", PlainString(v)) => cfg.channel_alias = Some(v),
        ("channel_settings", ChannelSettingsSeq(v)) => cfg.channel_settings = v,
        ("default_channels", StringSeq(v)) => cfg.default_channels = v,
        ("override_channels_enabled", Bool(v)) => cfg.override_channels_enabled = Some(v),
        ("allowlist_channels", StringSeq(v)) => cfg.allowlist_channels = v,
        ("denylist_channels", StringSeq(v)) => cfg.denylist_channels = v,
        ("custom_channels", StringMap(v)) => cfg.custom_channels = v,
        ("custom_multichannels", StringSeqMap(v)) => cfg.custom_multichannels = v,
        ("migrated_channel_aliases", StringSeq(v)) => cfg.migrated_channel_aliases = v,
        ("migrated_custom_channels", StringMap(v)) => cfg.migrated_custom_channels = v,
        ("add_anaconda_token", Bool(v)) => cfg.add_anaconda_token = Some(v),
        ("allow_non_channel_urls", Bool(v)) => cfg.allow_non_channel_urls = Some(v),
        ("repodata_fns", StringSeq(v)) => cfg.repodata_fns = v,
        ("use_only_tar_bz2", NullableBool(v)) => cfg.use_only_tar_bz2 = Some(v),
        ("repodata_threads", Int(v)) => cfg.repodata_threads = Some(v),
        ("fetch_threads", Int(v)) => cfg.fetch_threads = Some(v),
        ("experimental", StringSeq(v)) => cfg.experimental = v,
        ("no_lock", Bool(v)) => cfg.no_lock = Some(v),
        ("repodata_use_zst", Bool(v)) => cfg.repodata_use_zst = Some(v),
        ("repodata_use_shards", Bool(v)) => cfg.repodata_use_shards = Some(v),
        ("envs_dirs", StringSeq(v)) => cfg.envs_dirs = v,
        ("pkgs_dirs", StringSeq(v)) => cfg.pkgs_dirs = v,
        ("default_threads", Int(v)) => cfg.default_threads = Some(v),
        ("preview", StringSeq(v)) => cfg.preview = v,
        ("client_ssl_cert", NullableString(v)) => cfg.client_ssl_cert = Some(v),
        ("client_ssl_cert_key", NullableString(v)) => cfg.client_ssl_cert_key = Some(v),
        ("local_repodata_ttl", BoolOrInt(v)) => cfg.local_repodata_ttl = Some(v),
        ("offline", Bool(v)) => cfg.offline = Some(v),
        ("proxy_servers", NullableStringMap(v)) => cfg.proxy_servers = v,
        ("remote_connect_timeout_secs", Float(v)) => cfg.remote_connect_timeout_secs = Some(v),
        ("remote_max_retries", Int(v)) => cfg.remote_max_retries = Some(v),
        ("remote_backoff_factor", Int(v)) => cfg.remote_backoff_factor = Some(v),
        ("remote_read_timeout_secs", Float(v)) => cfg.remote_read_timeout_secs = Some(v),
        ("ssl_verify", SslVerify(v)) => cfg.ssl_verify = Some(v),
        ("aggressive_update_packages", StringSeq(v)) => cfg.aggressive_update_packages = v,
        ("auto_update_conda", Bool(v)) => cfg.auto_update_conda = Some(v),
        ("channel_priority", Enum(EnumResult::ChannelPriority(v))) => {
            cfg.channel_priority = Some(v)
        }
        ("create_default_packages", StringSeq(v)) => cfg.create_default_packages = v,
        ("disallowed_packages", StringSeq(v)) => cfg.disallowed_packages = v,
        ("force_reinstall", Bool(v)) => cfg.force_reinstall = Some(v),
        ("pinned_packages", StringSeq(v)) => cfg.pinned_packages = v,
        ("prefix_data_interoperability", Bool(v)) => cfg.prefix_data_interoperability = Some(v),
        ("track_features", StringSeq(v)) => cfg.track_features = v,
        ("solver", PlainString(v)) => cfg.solver = Some(v),
        ("allow_softlinks", Bool(v)) => cfg.allow_softlinks = Some(v),
        ("always_copy", Bool(v)) => cfg.always_copy = Some(v),
        ("always_softlink", Bool(v)) => cfg.always_softlink = Some(v),
        ("path_conflict", Enum(EnumResult::PathConflict(v))) => cfg.path_conflict = Some(v),
        ("rollback_enabled", Bool(v)) => cfg.rollback_enabled = Some(v),
        ("safety_checks", Enum(EnumResult::SafetyChecks(v))) => cfg.safety_checks = Some(v),
        ("extra_safety_checks", Bool(v)) => cfg.extra_safety_checks = Some(v),
        ("signing_metadata_url_base", NullableString(v)) => cfg.signing_metadata_url_base = Some(v),
        ("shortcuts", Bool(v)) => cfg.shortcuts = Some(v),
        ("shortcuts_only", StringSeq(v)) => cfg.shortcuts_only = v,
        ("non_admin_enabled", Bool(v)) => cfg.non_admin_enabled = Some(v),
        ("separate_format_cache", Bool(v)) => cfg.separate_format_cache = Some(v),
        ("verify_threads", Int(v)) => cfg.verify_threads = Some(v),
        ("execute_threads", Int(v)) => cfg.execute_threads = Some(v),
        ("always_yes", NullableBool(v)) => cfg.always_yes = Some(v),
        ("auto_activate", Bool(v)) => cfg.auto_activate = Some(v),
        ("default_activation_env", PlainString(v)) => cfg.default_activation_env = Some(v),
        ("auto_stack", Int(v)) => cfg.auto_stack = Some(v),
        ("changeps1", Bool(v)) => cfg.changeps1 = Some(v),
        ("env_prompt", PlainString(v)) => cfg.env_prompt = Some(v),
        ("json", Bool(v)) => cfg.json = Some(v),
        ("console", PlainString(v)) => cfg.console = Some(v),
        ("notify_outdated_conda", Bool(v)) => cfg.notify_outdated_conda = Some(v),
        ("quiet", Bool(v)) => cfg.quiet = Some(v),
        ("report_errors", NullableBool(v)) => cfg.report_errors = Some(v),
        ("show_channel_urls", NullableBool(v)) => cfg.show_channel_urls = Some(v),
        ("list_fields", ListFieldsSeq(v)) => cfg.list_fields = v,
        ("verbosity", Int(v)) => cfg.verbosity = Some(v),
        ("unsatisfiable_hints", Bool(v)) => cfg.unsatisfiable_hints = Some(v),
        ("unsatisfiable_hints_check_depth", Int(v)) => {
            cfg.unsatisfiable_hints_check_depth = Some(v)
        }
        ("number_channel_notices", Int(v)) => cfg.number_channel_notices = Some(v),
        ("envvars_force_uppercase", Bool(v)) => cfg.envvars_force_uppercase = Some(v),
        ("export_platforms", StringSeq(v)) => cfg.export_platforms = v,
        ("override_virtual_packages", NullableStringMap(v)) => cfg.override_virtual_packages = v,
        ("allow_cycles", Bool(v)) => cfg.allow_cycles = Some(v),
        ("allow_conda_downgrades", Bool(v)) => cfg.allow_conda_downgrades = Some(v),
        ("add_pip_as_python_dependency", Bool(v)) => cfg.add_pip_as_python_dependency = Some(v),
        ("debug", Bool(v)) => cfg.debug = Some(v),
        ("trace", Bool(v)) => cfg.trace = Some(v),
        ("dev", Bool(v)) => cfg.dev = Some(v),
        ("default_python", NullableString(v)) => cfg.default_python = Some(v),
        ("enable_private_envs", Bool(v)) => cfg.enable_private_envs = Some(v),
        ("error_upload_url", PlainString(v)) => cfg.error_upload_url = Some(v),
        ("force_32bit", Bool(v)) => cfg.force_32bit = Some(v),
        ("root_prefix", PlainString(v)) => cfg.root_prefix = Some(v),
        ("sat_solver", Enum(EnumResult::SatSolver(v))) => cfg.sat_solver = Some(v),
        ("solver_ignore_timestamps", Bool(v)) => cfg.solver_ignore_timestamps = Some(v),
        ("subdir", PlainString(v)) => cfg.subdir = Some(v),
        ("subdirs", StringSeq(v)) => cfg.subdirs = v,
        ("target_prefix_override", PlainString(v)) => cfg.target_prefix_override = Some(v),
        ("register_envs", Bool(v)) => cfg.register_envs = Some(v),
        ("protect_frozen_envs", Bool(v)) => cfg.protect_frozen_envs = Some(v),
        ("no_plugins", Bool(v)) => cfg.no_plugins = Some(v),
        ("environment_specifier", NullableString(v)) => cfg.environment_specifier = Some(v),
        (other, _) => unreachable!("catalog/dispatch mismatch for setting {other:?}"),
    }
}

/// Lower an unrecognized `RawValue` into a neutral `serde_json::Value` for `Config::extra`
/// (research R2). Bare `true`/`null`/`7`/`1.5` map to the natural JSON type of their *resolved*
/// YAML type; a non-finite float is encoded as its source-text string, matching the adapter's own
/// non-finite convention (research R2's "yaml-rust2 tie-in" note).
fn raw_value_to_json(value: &RawValue) -> serde_json::Value {
    match value {
        RawValue::Null => serde_json::Value::Null,
        RawValue::Bool(b) => serde_json::Value::Bool(*b),
        RawValue::Int(i) => serde_json::Value::Number((*i).into()),
        RawValue::Float(f) if f.is_finite() => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or_else(|| serde_json::Value::String(f.to_string())),
        RawValue::Float(f) if f.is_nan() => serde_json::Value::String("NaN".to_string()),
        RawValue::Float(f) => {
            serde_json::Value::String(if *f > 0.0 { "Infinity" } else { "-Infinity" }.to_string())
        }
        RawValue::Str(s) => serde_json::Value::String(s.clone()),
        RawValue::Seq(items) => {
            serde_json::Value::Array(items.iter().map(raw_value_to_json).collect())
        }
        RawValue::Map(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), raw_value_to_json(v)))
                .collect(),
        ),
    }
}

/// Build the `ErrorEntry` a `CoercionError` becomes once the per-key loop knows which setting
/// (canonical or alias-as-written) produced it (data-model.md §7).
fn build_error_entry(setting_key: &str, err: CoercionError) -> ErrorEntry {
    let location = if err.path.is_empty() {
        Location::Setting {
            setting: setting_key.to_string(),
        }
    } else {
        Location::Nested {
            setting: setting_key.to_string(),
            path: err.path,
        }
    };
    ErrorEntry {
        location,
        kind: ErrorKind::TypeCoercion,
        message: err.message,
        input: err.input,
        involved: Vec::new(),
    }
}

/// Run `setting`'s semantic validator (if any) against its just-coerced value (data-model.md §9,
/// FR-024/025/026), producing a `semantic_validation` entry iff the value is invalid. `Coerced`
/// stays private to this module, so the actual rule logic lives in `validate.rs`'s small, pure,
/// per-type functions; this is just the dispatch between the two.
fn semantic_validation_entry(
    validator: SemanticValidator,
    coerced: &Coerced,
    setting_key: &str,
    options: &ParseOptions,
) -> Option<ErrorEntry> {
    let (message, input) = match validator {
        SemanticValidator::ChannelAlias => {
            let Coerced::PlainString(s) = coerced else {
                unreachable!("catalog/coercer mismatch: ChannelAlias validator expects PlainString")
            };
            (
                crate::validate::channel_alias_error(s)?,
                InputRepr::Str { value: s.clone() },
            )
        }
        SemanticValidator::DefaultPython => {
            let Coerced::NullableString(opt) = coerced else {
                unreachable!(
                    "catalog/coercer mismatch: DefaultPython validator expects NullableString"
                )
            };
            let s = opt.as_ref()?;
            (
                crate::validate::default_python_error(s)?,
                InputRepr::Str { value: s.clone() },
            )
        }
        SemanticValidator::SslVerify => {
            let Coerced::SslVerify(sv) = coerced else {
                unreachable!("catalog/coercer mismatch: SslVerify validator expects SslVerify")
            };
            let message = crate::validate::ssl_verify_error(sv, options)?;
            let crate::model::SslVerify::Path(path) = sv else {
                unreachable!("ssl_verify_error only ever fails for the Path variant")
            };
            (
                message,
                InputRepr::Str {
                    value: path.clone(),
                },
            )
        }
        // The `list_fields` closed-vocabulary check already happens inline, as a
        // `type_coercion` error, in `coerce/sequences.rs::coerce_list_fields_seq` — there is no
        // separate semantic pass to run here.
        SemanticValidator::ListFields => return None,
    };
    Some(ErrorEntry {
        location: Location::Setting {
            setting: setting_key.to_string(),
        },
        kind: ErrorKind::SemanticValidation,
        message,
        input,
        involved: Vec::new(),
    })
}

/// The per-key coercion dispatch loop (FR-009/FR-010/FR-011): for each `CATALOG` entry (in its
/// fixed declaration order, research R7), look up the document's raw value under the setting's
/// canonical name or any alias, coerce it, and either set the matching `Config` field or push a
/// `type_coercion` entry — never short-circuiting on the first failure (FR-030/031). A
/// successfully-coerced value that also has a semantic validator (FR-024/025/026) is checked
/// immediately after coercion: an invalid value still gets written into `Config` (harmless, since
/// any accumulated entry means the whole parse ultimately returns `Err`, so the field's value is
/// never observed by a caller) alongside pushing its `semantic_validation` entry. Keys that match
/// nothing in `CATALOG` are retained in `Config::extra` (FR-036, research R2).
///
/// After the per-key pass, this also runs `validate.rs`'s alias-collision (FR-029) and
/// cross-field (FR-027/028) passes, in the order research R7 documents.
pub(crate) fn parse_map(
    map: IndexMap<String, RawValue>,
    options: &ParseOptions,
) -> (Config, Vec<ErrorEntry>) {
    let mut cfg = Config::default();
    let mut entries = Vec::new();
    let mut consumed: HashSet<String> = HashSet::new();

    for setting in CATALOG {
        let mut found: Option<&str> = None;
        if map.contains_key(setting.canonical) {
            found = Some(setting.canonical);
        }
        for alias in setting.aliases {
            if map.contains_key(*alias) {
                found = Some(alias);
            }
        }
        let Some(matched_key) = found else { continue };
        consumed.insert(matched_key.to_string());

        let raw = &map[matched_key];
        match coerce_value(setting.kind, raw) {
            Ok(coerced) => {
                let coerced = apply_null_sequence_map_default(
                    coerced,
                    setting.canonical,
                    setting.kind,
                    raw,
                    options,
                );
                if let Some(validator) = setting.validator
                    && let Some(entry) =
                        semantic_validation_entry(validator, &coerced, matched_key, options)
                {
                    entries.push(entry);
                }
                apply_to_config(&mut cfg, setting.canonical, coerced);
            }
            Err(err) => entries.push(build_error_entry(matched_key, err)),
        }
    }

    for (key, raw) in &map {
        if !consumed.contains(key) {
            cfg.extra.insert(key.clone(), raw_value_to_json(raw));
        }
    }

    entries.extend(crate::validate::alias_collision_entries(&map));
    entries.extend(crate::validate::cross_field_entries(&cfg));

    (cfg, entries)
}

/// Parse the text of one `.condarc` document all the way to a [`Config`] or a
/// [`ValidationReport`] (FR-001/002/005/006/007/008). This is what `lib.rs`'s public `parse`/
/// `parse_with_options` delegate to.
pub(crate) fn parse_document(
    yaml_text: &str,
    options: &ParseOptions,
) -> Result<Config, ValidationReport> {
    let docs = match YamlLoader::load_from_str(yaml_text) {
        Ok(docs) => docs,
        Err(scan_err) => {
            return Err(ValidationReport::new(vec![ErrorEntry {
                location: Location::Root,
                kind: ErrorKind::YamlSyntax,
                message: format!("invalid YAML: {scan_err}"),
                input: InputRepr::Null,
                involved: Vec::new(),
            }]));
        }
    };

    // Zero documents (empty input) is the same as a single Null document (FR-005); more than one
    // document is out of scope (FR-007a).
    let root = match docs.as_slice() {
        [] => Yaml::Null,
        [only] => only.clone(),
        many => {
            return Err(ValidationReport::new(vec![ErrorEntry {
                location: Location::Root,
                kind: ErrorKind::RootShape,
                message: format!("expected a single YAML document, found {}", many.len()),
                input: InputRepr::Null,
                involved: Vec::new(),
            }]));
        }
    };

    let (lowered, dropped_key_entries) = lower_document(&root);
    match lowered {
        RawValue::Null => {
            if dropped_key_entries.is_empty() {
                Ok(Config::default())
            } else {
                Err(ValidationReport::new(dropped_key_entries))
            }
        }
        RawValue::Map(map) => {
            let (cfg, map_entries) = parse_map(map, options);
            let mut all_entries = dropped_key_entries;
            all_entries.extend(map_entries);
            if all_entries.is_empty() {
                Ok(cfg)
            } else {
                Err(ValidationReport::new(all_entries))
            }
        }
        other => Err(ValidationReport::new(vec![ErrorEntry {
            location: Location::Root,
            kind: ErrorKind::RootShape,
            message: "expected the document root to be a mapping, or empty".to_string(),
            input: input_repr(&other),
            involved: Vec::new(),
        }])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yaml_rust2::YamlLoader;

    fn load_one(yaml_text: &str) -> Yaml {
        let mut docs = YamlLoader::load_from_str(yaml_text).expect("valid YAML for this test");
        assert_eq!(docs.len(), 1, "test fixture must be exactly one document");
        docs.remove(0)
    }

    #[test]
    fn lowers_null_variants() {
        assert_eq!(lower(&load_one("~")), RawValue::Null);
        assert_eq!(lower(&load_one("null")), RawValue::Null);
        assert_eq!(lower(&Yaml::BadValue), RawValue::Null);
        assert_eq!(lower(&Yaml::Alias(0)), RawValue::Null);
    }

    #[test]
    fn lowers_bool() {
        assert_eq!(lower(&load_one("true")), RawValue::Bool(true));
        assert_eq!(lower(&load_one("false")), RawValue::Bool(false));
    }

    #[test]
    fn lowers_int() {
        assert_eq!(lower(&load_one("7")), RawValue::Int(7));
        assert_eq!(lower(&load_one("-42")), RawValue::Int(-42));
    }

    #[test]
    fn lowers_float() {
        assert_eq!(lower(&load_one("1.5")), RawValue::Float(1.5));
        assert_eq!(lower(&load_one(".inf")), RawValue::Float(f64::INFINITY));
        assert!(matches!(lower(&load_one(".nan")), RawValue::Float(f) if f.is_nan()));
    }

    #[test]
    fn lowers_string_quoted_and_bare() {
        assert_eq!(lower(&load_one("foo")), RawValue::Str("foo".to_string()));
        assert_eq!(
            lower(&load_one("\"true\"")),
            RawValue::Str("true".to_string())
        );
        assert_eq!(lower(&load_one("\"7\"")), RawValue::Str("7".to_string()));
    }

    #[test]
    fn bare_over_range_integer_numeral_falls_back_to_real_float() {
        // yaml-rust2 itself fails to parse this as `Yaml::Integer` (exceeds i64), but a bare
        // all-digit numeral still parses as `f64` (Rust's `f64::parse` succeeds, possibly as
        // `inf`, for essentially any digit string), so yaml-rust2 resolves it as `Yaml::Real`,
        // not `Yaml::String`. `lower()` therefore maps it to `RawValue::Float`; the A1 range
        // check that rejects it for an `Int`-typed setting is `coerce/numeric.rs`'s job, applied
        // to this float (or to the *quoted* string form the real conformance fixtures use —
        // see `lowers_string_quoted_and_bare` — not to this bare-numeral case).
        let bignum = "99999999999999999999999999999999";
        let yaml = load_one(bignum);
        assert!(
            matches!(yaml, Yaml::Real(_)),
            "expected yaml-rust2 to resolve this as Real"
        );
        assert_eq!(lower(&yaml), RawValue::Float(1e32));
    }

    #[test]
    fn quoted_bignum_numeral_stays_a_string_regardless_of_magnitude() {
        // The real conformance corpus's bignum fixtures quote the numeral (e.g.
        // `remote_max_retries: "-999...999"`), which always resolves to `Yaml::String`
        // regardless of how large the digit string is — this is the shape the A1 range check in
        // `coerce/numeric.rs` actually operates on for those fixtures.
        let bignum = "\"-99999999999999999999999999999999999999999999999999\"";
        let yaml = load_one(bignum);
        assert!(matches!(yaml, Yaml::String(_)));
        assert_eq!(
            lower(&yaml),
            RawValue::Str("-99999999999999999999999999999999999999999999999999".to_string())
        );
    }

    #[test]
    fn lowers_sequence_recursively() {
        let yaml = load_one("[1, true, foo]");
        assert_eq!(
            lower(&yaml),
            RawValue::Seq(vec![
                RawValue::Int(1),
                RawValue::Bool(true),
                RawValue::Str("foo".to_string()),
            ])
        );
    }

    #[test]
    fn lowers_map_recursively_preserving_insertion_order() {
        let yaml = load_one("b: 2\na: 1\nc: 3");
        let RawValue::Map(map) = lower(&yaml) else {
            panic!("expected a Map")
        };
        assert_eq!(
            map.into_iter().collect::<Vec<_>>(),
            vec![
                ("b".to_string(), RawValue::Int(2)),
                ("a".to_string(), RawValue::Int(1)),
                ("c".to_string(), RawValue::Int(3)),
            ]
        );
    }

    #[test]
    fn drops_non_string_mapping_keys() {
        let yaml = load_one("1: x\nfoo: bar");
        let RawValue::Map(map) = lower(&yaml) else {
            panic!("expected a Map")
        };
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("foo"), Some(&RawValue::Str("bar".to_string())));
    }

    #[test]
    fn lowers_empty_root_to_null() {
        // Zero documents (empty input) is handled by the caller (FR-005); `lower` itself only
        // ever receives one already-selected `Yaml` value.
        assert_eq!(lower(&Yaml::Null), RawValue::Null);
    }
}
