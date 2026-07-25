//! Test-only conformance adapter (GEN-36 User Story 3, research R9/R10).
//!
//! Renders a parsed [`condarc::Config`] into the same JSON shape as
//! `conformance/condarc/expected/*.json`, for exact comparison by the `Crate` checker in
//! `tests/condarc_conformance.rs`. This module is conformance-harness support code, not a
//! feature of the published `condarc` crate -- see `specs/GEN-36_condarc_parser_library/
//! contracts/adapter-output.md` for the full behavioral contract this implements.

use condarc::{
    BoolOrInt, ChannelPriority, ChannelSetting, Config, ListField, PathConflict, SafetyChecks,
    SatSolver, SslVerify,
};
use serde_json::{Map, Value};

// ---------------------------------------------------------------------
// Per-`ValueKind`-shape encoders (contracts/adapter-output.md "Value encoding")
// ---------------------------------------------------------------------

/// `f64` -> JSON number, except non-finite values encode as the source-text strings
/// `"Infinity"`/`"-Infinity"`/`"NaN"` (FR-041) -- `serde_json` cannot serialize `NaN`/`±inf`
/// directly.
fn encode_f64(value: f64) -> Value {
    if value.is_nan() {
        Value::String("NaN".to_string())
    } else if value.is_infinite() {
        Value::String(if value.is_sign_positive() {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        })
    } else {
        serde_json::json!(value)
    }
}

fn encode_string_vec(values: Vec<String>) -> Value {
    Value::Array(values.into_iter().map(Value::String).collect())
}

fn encode_string_map(map: std::collections::BTreeMap<String, String>) -> Value {
    let mut out = Map::new();
    for (k, v) in map {
        out.insert(k, Value::String(v));
    }
    Value::Object(out)
}

fn encode_nullable_string_map(map: std::collections::BTreeMap<String, Option<String>>) -> Value {
    let mut out = Map::new();
    for (k, v) in map {
        out.insert(k, v.map(Value::String).unwrap_or(Value::Null));
    }
    Value::Object(out)
}

/// `custom_multichannels` only: multichannel name -> list of member channel names/URLs.
fn encode_string_seq_map(map: std::collections::BTreeMap<String, Vec<String>>) -> Value {
    let mut out = Map::new();
    for (k, v) in map {
        out.insert(k, encode_string_vec(v));
    }
    Value::Object(out)
}

/// `channel_settings` only: one string-to-string map per list entry.
fn encode_channel_settings(values: Vec<ChannelSetting>) -> Value {
    Value::Array(
        values
            .into_iter()
            .map(|setting| encode_string_map(setting.0))
            .collect(),
    )
}

fn encode_channel_priority(value: ChannelPriority) -> Value {
    Value::String(
        match value {
            ChannelPriority::Strict => "strict",
            ChannelPriority::Flexible => "flexible",
            ChannelPriority::Disabled => "disabled",
            // `#[non_exhaustive]`: no other variant is constructible by this crate today.
            _ => unreachable!("unknown ChannelPriority variant"),
        }
        .to_string(),
    )
}

fn encode_path_conflict(value: PathConflict) -> Value {
    Value::String(
        match value {
            PathConflict::Clobber => "clobber",
            PathConflict::Warn => "warn",
            PathConflict::Prevent => "prevent",
            _ => unreachable!("unknown PathConflict variant"),
        }
        .to_string(),
    )
}

fn encode_safety_checks(value: SafetyChecks) -> Value {
    Value::String(
        match value {
            SafetyChecks::Enabled => "enabled",
            SafetyChecks::Warn => "warn",
            SafetyChecks::Disabled => "disabled",
            _ => unreachable!("unknown SafetyChecks variant"),
        }
        .to_string(),
    )
}

fn encode_sat_solver(value: SatSolver) -> Value {
    Value::String(
        match value {
            SatSolver::Pycosat => "pycosat",
            SatSolver::Pycryptosat => "pycryptosat",
            SatSolver::Pysat => "pysat",
            _ => unreachable!("unknown SatSolver variant"),
        }
        .to_string(),
    )
}

fn encode_bool_or_int(value: BoolOrInt) -> Value {
    match value {
        BoolOrInt::Bool(b) => Value::Bool(b),
        BoolOrInt::Int(i) => serde_json::json!(i),
    }
}

fn encode_ssl_verify(value: SslVerify) -> Value {
    match value {
        SslVerify::Bool(b) => Value::Bool(b),
        SslVerify::Truststore => Value::String("truststore".to_string()),
        SslVerify::Path(p) => Value::String(p),
    }
}

/// One closed member of `CONDA_LIST_FIELDS`, exact lowercase snake_case spelling (§5.9).
fn encode_list_field(value: ListField) -> &'static str {
    match value {
        ListField::Arch => "arch",
        ListField::Build => "build",
        ListField::BuildNumber => "build_number",
        ListField::Channel => "channel",
        ListField::ChannelName => "channel_name",
        ListField::Constrains => "constrains",
        ListField::Depends => "depends",
        ListField::DistStr => "dist_str",
        ListField::Features => "features",
        ListField::Fn => "fn",
        ListField::License => "license",
        ListField::LicenseFamily => "license_family",
        ListField::Md5 => "md5",
        ListField::Name => "name",
        ListField::Noarch => "noarch",
        ListField::PackageType => "package_type",
        ListField::RequestedSpec => "requested_spec",
        ListField::RequestedSpecs => "requested_specs",
        ListField::Sha256 => "sha256",
        ListField::Size => "size",
        ListField::Subdir => "subdir",
        ListField::Timestamp => "timestamp",
        ListField::TrackFeatures => "track_features",
        ListField::Url => "url",
        ListField::Version => "version",
        _ => unreachable!("unknown ListField variant"),
    }
}

fn encode_list_fields(values: Vec<ListField>) -> Value {
    Value::Array(
        values
            .into_iter()
            .map(|f| Value::String(encode_list_field(f).to_string()))
            .collect(),
    )
}

// ---------------------------------------------------------------------
// Present-only field insertion macros (contracts/adapter-output.md item 1)
// ---------------------------------------------------------------------

/// A plain `Option<T>` field: emit `canonical: encode(value)` iff `Some`. `canonical` is always
/// the bare field name (`stringify!($field)`) since every `Config` field is already named after
/// conda's canonical loader name (data-model.md §5).
macro_rules! field {
    ($map:expr, $cfg:expr, $field:ident, |$v:ident| $encode:expr) => {
        if let Some($v) = $cfg.$field.clone() {
            $map.insert(stringify!($field).to_string(), $encode);
        }
    };
}

/// A nullable `Option<Option<T>>` field: outer `None` = absent (never emitted); `Some(None)` =
/// present-and-null (emits JSON `null`); `Some(Some(v))` = present (emits `encode(v)`).
macro_rules! nullable_field {
    ($map:expr, $cfg:expr, $field:ident, |$v:ident| $encode:expr) => {
        if let Some(inner) = $cfg.$field.clone() {
            let value = match inner {
                Some($v) => $encode,
                None => Value::Null,
            };
            $map.insert(stringify!($field).to_string(), value);
        }
    };
}

// ---------------------------------------------------------------------
// The adapter itself
// ---------------------------------------------------------------------

/// Render a parsed `.condarc` [`condarc::Config`] into the same JSON shape as
/// `conformance/condarc/expected/*.json`, for conformance comparison only. Not part of the
/// published crate's public API (research R9).
pub fn to_expected_json(config: &Config) -> Value {
    let mut map = Map::new();

    // ---- §4.1 Channel Configuration ----
    field!(map, config, channels, |v| encode_string_vec(v));
    field!(map, config, channel_alias, |v| Value::String(v));
    field!(map, config, channel_settings, |v| encode_channel_settings(
        v
    ));
    field!(map, config, default_channels, |v| encode_string_vec(v));
    field!(map, config, override_channels_enabled, |v| Value::Bool(v));
    field!(map, config, allowlist_channels, |v| encode_string_vec(v));
    field!(map, config, denylist_channels, |v| encode_string_vec(v));
    field!(map, config, custom_channels, |v| encode_string_map(v));
    field!(map, config, custom_multichannels, |v| {
        encode_string_seq_map(v)
    });
    field!(map, config, migrated_channel_aliases, |v| {
        encode_string_vec(v)
    });
    field!(map, config, migrated_custom_channels, |v| {
        encode_string_map(v)
    });
    field!(map, config, add_anaconda_token, |v| Value::Bool(v));
    field!(map, config, allow_non_channel_urls, |v| Value::Bool(v));
    field!(map, config, repodata_fns, |v| encode_string_vec(v));
    nullable_field!(map, config, use_only_tar_bz2, |v| Value::Bool(v));
    field!(map, config, repodata_threads, |v| serde_json::json!(v));
    field!(map, config, fetch_threads, |v| serde_json::json!(v));
    field!(map, config, experimental, |v| encode_string_vec(v));
    field!(map, config, no_lock, |v| Value::Bool(v));
    field!(map, config, repodata_use_zst, |v| Value::Bool(v));
    field!(map, config, repodata_use_shards, |v| Value::Bool(v));

    // ---- §4.2 Basic Conda Configuration ----
    field!(map, config, envs_dirs, |v| encode_string_vec(v));
    field!(map, config, pkgs_dirs, |v| encode_string_vec(v));
    field!(map, config, default_threads, |v| serde_json::json!(v));
    field!(map, config, preview, |v| encode_string_vec(v));

    // ---- §4.3 Network Configuration ----
    nullable_field!(map, config, client_ssl_cert, |v| Value::String(v));
    nullable_field!(map, config, client_ssl_cert_key, |v| Value::String(v));
    field!(map, config, local_repodata_ttl, |v| encode_bool_or_int(v));
    field!(map, config, offline, |v| Value::Bool(v));
    field!(map, config, proxy_servers, |v| encode_nullable_string_map(
        v
    ));
    field!(map, config, remote_connect_timeout_secs, |v| encode_f64(v));
    field!(map, config, remote_max_retries, |v| serde_json::json!(v));
    field!(map, config, remote_backoff_factor, |v| serde_json::json!(v));
    field!(map, config, remote_read_timeout_secs, |v| encode_f64(v));
    field!(map, config, ssl_verify, |v| encode_ssl_verify(v));

    // ---- §4.4 Solver Configuration ----
    field!(map, config, aggressive_update_packages, |v| {
        encode_string_vec(v)
    });
    field!(map, config, auto_update_conda, |v| Value::Bool(v));
    field!(map, config, channel_priority, |v| encode_channel_priority(
        v
    ));
    field!(map, config, create_default_packages, |v| {
        encode_string_vec(v)
    });
    field!(map, config, disallowed_packages, |v| encode_string_vec(v));
    field!(map, config, force_reinstall, |v| Value::Bool(v));
    field!(map, config, pinned_packages, |v| encode_string_vec(v));
    field!(map, config, prefix_data_interoperability, |v| Value::Bool(
        v
    ));
    field!(map, config, track_features, |v| encode_string_vec(v));
    field!(map, config, solver, |v| Value::String(v));

    // ---- §4.5 Package Linking and Install-time Configuration ----
    field!(map, config, allow_softlinks, |v| Value::Bool(v));
    field!(map, config, always_copy, |v| Value::Bool(v));
    field!(map, config, always_softlink, |v| Value::Bool(v));
    field!(map, config, path_conflict, |v| encode_path_conflict(v));
    field!(map, config, rollback_enabled, |v| Value::Bool(v));
    field!(map, config, safety_checks, |v| encode_safety_checks(v));
    field!(map, config, extra_safety_checks, |v| Value::Bool(v));
    nullable_field!(map, config, signing_metadata_url_base, |v| Value::String(v));
    field!(map, config, shortcuts, |v| Value::Bool(v));
    field!(map, config, shortcuts_only, |v| encode_string_vec(v));
    field!(map, config, non_admin_enabled, |v| Value::Bool(v));
    field!(map, config, separate_format_cache, |v| Value::Bool(v));
    field!(map, config, verify_threads, |v| serde_json::json!(v));
    field!(map, config, execute_threads, |v| serde_json::json!(v));

    // ---- §4.7 Output, Prompt, and Flow Control Configuration ----
    nullable_field!(map, config, always_yes, |v| Value::Bool(v));
    field!(map, config, auto_activate, |v| Value::Bool(v));
    field!(map, config, default_activation_env, |v| Value::String(v));
    field!(map, config, auto_stack, |v| serde_json::json!(v));
    field!(map, config, changeps1, |v| Value::Bool(v));
    field!(map, config, env_prompt, |v| Value::String(v));
    field!(map, config, json, |v| Value::Bool(v));
    field!(map, config, console, |v| Value::String(v));
    field!(map, config, notify_outdated_conda, |v| Value::Bool(v));
    field!(map, config, quiet, |v| Value::Bool(v));
    nullable_field!(map, config, report_errors, |v| Value::Bool(v));
    nullable_field!(map, config, show_channel_urls, |v| Value::Bool(v));
    field!(map, config, list_fields, |v| encode_list_fields(v));
    field!(map, config, verbosity, |v| serde_json::json!(v));
    field!(map, config, unsatisfiable_hints, |v| Value::Bool(v));
    field!(map, config, unsatisfiable_hints_check_depth, |v| {
        serde_json::json!(v)
    });
    field!(map, config, number_channel_notices, |v| serde_json::json!(
        v
    ));
    field!(map, config, envvars_force_uppercase, |v| Value::Bool(v));
    field!(map, config, export_platforms, |v| encode_string_vec(v));
    field!(map, config, override_virtual_packages, |v| {
        encode_nullable_string_map(v)
    });

    // ---- §4.9 Hidden and Undocumented ----
    field!(map, config, allow_cycles, |v| Value::Bool(v));
    field!(map, config, allow_conda_downgrades, |v| Value::Bool(v));
    field!(map, config, add_pip_as_python_dependency, |v| Value::Bool(
        v
    ));
    field!(map, config, debug, |v| Value::Bool(v));
    field!(map, config, trace, |v| Value::Bool(v));
    field!(map, config, dev, |v| Value::Bool(v));
    nullable_field!(map, config, default_python, |v| Value::String(v));
    field!(map, config, enable_private_envs, |v| Value::Bool(v));
    field!(map, config, error_upload_url, |v| Value::String(v));
    field!(map, config, force_32bit, |v| Value::Bool(v));
    field!(map, config, root_prefix, |v| Value::String(v));
    field!(map, config, sat_solver, |v| encode_sat_solver(v));
    field!(map, config, solver_ignore_timestamps, |v| Value::Bool(v));
    field!(map, config, subdir, |v| Value::String(v));
    field!(map, config, subdirs, |v| encode_string_vec(v));
    field!(map, config, target_prefix_override, |v| Value::String(v));
    field!(map, config, register_envs, |v| Value::Bool(v));
    field!(map, config, protect_frozen_envs, |v| Value::Bool(v));

    // ---- §4.10 Plugin Configuration ----
    field!(map, config, no_plugins, |v| Value::Bool(v));

    // ---- §4.11 Experimental ----
    nullable_field!(map, config, environment_specifier, |v| Value::String(v));

    // `Config::extra` is never emitted (FR-040) -- deliberately not read here.

    Value::Object(map)
}

// ---------------------------------------------------------------------
// `Crate`-checker A1 divergence list (spec Assumptions A1)
// ---------------------------------------------------------------------

/// `valid/` fixture *file stems* (no directory, no `.json` extension) that real conda accepts
/// but this crate deliberately rejects, per spec Assumption A1 (fixed-width `i64`/`f64` cannot
/// represent Python's arbitrary-precision numerals). These fixtures stay in `valid/` -- the
/// conda/openapi checkers must keep accepting them -- but the `Crate` checker's expected verdict
/// for each is "rejected", not "accepted", and no adapter comparison runs for them at all.
pub const CRATE_A1_DIVERGENCES: &[&str] = &[
    "numeric_values_accept_numeric_string_bignum_exceeds_i64_max",
    "numeric_values_accept_numeric_string_bignum_exceeds_u64_max",
    "numeric_values_accept_numeric_string_bignum_exceeds_f64_max_finite",
    "numeric_values_accept_numeric_string_bignum_negative",
];

/// Whether `fixture_path`'s file stem is a declared `Crate`-checker A1 divergence (see
/// [`CRATE_A1_DIVERGENCES`]).
pub fn is_crate_a1_divergence(fixture_path: &std::path::Path) -> bool {
    fixture_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| CRATE_A1_DIVERGENCES.contains(&stem))
}
