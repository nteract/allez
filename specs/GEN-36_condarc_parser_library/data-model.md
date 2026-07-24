# Phase 1 Data Model: `.condarc` Parser Library (GEN-36)

Entities are derived from the spec's Key Entities plus research R1–R10 and the full parameter
catalog in `docs/condarc_research.md` §4 (99 settings, cross-checked against
`docs/condarc_openapi.json`). Field names are the intended Rust API; exact `///` docs and derives
are written during implementation. Every public type is designed so invalid states are
unrepresentable (Constitution VI) — **this document enumerates every field of `Config`, not a
representative subset**, so a caller can see the complete surface it will consume.

---

## 1. `RawValue` — the internal parsed-YAML representation (`parse.rs`, crate-private)

**Background — what `yaml-rust2` gives us.** `yaml-rust2` is a *generic/dynamic* YAML parser, not
a derive-into-your-struct one (research R1). `YamlLoader::load_from_str(&str)` returns
`yaml_rust2::Yaml`, a dynamic tree whose scalars are **already resolved by type**:
`Yaml::Boolean(bool)`, `Yaml::Integer(i64)`, `Yaml::Real(String)` (kept as the original decimal
text so it can be re-parsed to `f64` without lossy round-tripping through `Yaml`'s own storage),
`Yaml::String(String)`, `Yaml::Null`, `Yaml::Array(Vec<Yaml>)`, `Yaml::Hash(LinkedHashMap<Yaml,
Yaml>)`. Crucially this resolution *preserves* the distinction the coercion engine needs — bare
`true`/`7`/`~` resolve to `Boolean`/`Integer`/`Null`, while quoted `"true"`/`"7"` resolve to
`String` — so the quoted-vs-bare signal survives (research R1). An over-range integer numeral
(exceeding `i64`) fails `yaml_rust2`'s own integer parse and falls back to `Yaml::String`, which is
exactly the shape the A1 range check in `coerce/numeric.rs` needs to reject it as `Str`, not crash
on an `Integer` overflow.

> **"Typed" here means the scalar *kind* (bool/int/float/string), NOT deserialization into
> `Config`.** `yaml-rust2` has no `from_str::<T>()` mode, does not take our `Config` type, and
> never matches keys to struct fields — it always returns the generic tree; *we* do all key→field
> matching and value coercion ourselves (§4/§5 catalog + `coerce/`). The serde-derive-style path
> (offered by the now-deprecated `serde_yaml`) was deliberately **rejected** in research R1,
> because conda's coercion (`"yes"` → `true`, `7` → `"7"`, value-or-name enum lookup, all-errors-
> accumulate) cannot be expressed through serde's `Deserializer` model.

`RawValue` is a small owned mirror of that tree that `parse.rs` builds via one hand-written
recursive match over `yaml_rust2::Yaml`. It exists so no other module depends on `yaml-rust2`'s
types (R1: parser swap = one module). It records the resolved scalar type so coercion can
distinguish `true` from `"true"` and `null` from `"null"`.

```rust
enum RawValue {
    Null,                              // Yaml::Null (bare ~/null/empty)
    Bool(bool),                        // Yaml::Boolean
    Int(i64),                          // Yaml::Integer
    Float(f64),                        // Yaml::Real, parsed once here (never re-parsed downstream)
    Str(String),                       // Yaml::String (quoted or plain; whitespace preserved;
                                        // also holds bignum/over-range numeral text, per A1)
    Seq(Vec<RawValue>),
    Map(IndexMap<String, RawValue>),   // insertion-ordered -> deterministic MultipleKeys detection
}
```

- **Rules**: root `Null` → empty `Config` (FR-005); root `Map` → normal parse (FR-006); root
  `Seq`/scalar → single `root_shape` error entry (FR-007, FR-032). A YAML syntax error
  (`yaml_rust2::ScanError`) never produces a `RawValue` → single `yaml_syntax` entry (FR-008,
  FR-032).
- Numeric coercion & the A1 `i64`/`f64` bound check happen in `coerce/numeric.rs`: a
  `RawValue::Int` is already in `i64`; an over-range numeral arrives as `RawValue::Str` and is
  range-checked there (rejected per A1). Parsing itself never panics or overflows.
- **Duplicate vs. alias-collision keys.** A YAML document repeating the *same* key
  (`always_yes: true` then `always_yes: false`) is collapsed by `yaml-rust2`'s own mapping
  construction before we see it — only one entry survives, so this is not our concern. Conda's
  `MultipleKeysError` (FR-029) is different: it fires on two *distinct alias spellings* of one
  setting (`always_yes` + `yes`), which arrive as two separate map entries, so `validate.rs` still
  sees both and detects the collision.
- **How "unknown" is even determined.** `yaml-rust2` is schema-free: it resolves a mapping into an
  undifferentiated map of *all* its entries and never classifies a key as known/unknown. The
  known/unknown split is entirely `parse.rs`'s logic: it iterates the root `RawValue::Map` and, per
  entry, does a `catalog.rs` lookup — a hit (canonical name or known alias, §5) is coerced into the
  matching `Config` field; a miss is lowered to `serde_json::Value` (via a plain recursive match,
  not serde deserialization) and inserted into `Config::extra` (§2, research R2).

---

## 2. `Config` — the typed configuration (`model.rs`)  *(primary output — Key Entity: Configuration)*

One field per recognized setting, keyed by **canonical** (user-facing, per
`docs/condarc_research.md` §4 / `docs/condarc_openapi.json`) name — aliases already resolved to
this name during parsing (FR-011). Absent = `None` (FR-038/R8). `#[non_exhaustive]` so adding a
future setting is a MINOR, non-breaking change.

Nullable settings (conda `(T, NoneType)`) use `Option<Option<T>>`: outer `None` = key absent;
`Some(None)` = key present and explicitly `null`; `Some(Some(v))` = present with a value. This is
the honest model that distinguishes "unset" from "explicitly null" — needed because, e.g.,
`always_yes` present-as-null is a real, distinct accepted state (research §8 item 13).

### 2.1 The full struct (99 fields + `extra`)

```rust
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
    /// Alias: `auto_activate`.
    pub auto_activate_base: Option<bool>,
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
```

**Field count check**: 21 (§4.1) + 4 (§4.2) + 10 (§4.3) + 10 (§4.4) + 14 (§4.5) + 20 (§4.7) + 18
(§4.9) + 1 (§4.10) + 1 (§4.11) = **99 recognized settings**, plus `extra`. This is the complete
catalog; conda-build's four keys (`bld_path`, `croot`, `anaconda_upload`/`binstar_upload`,
`conda_build`) are explicitly out of scope (FR-009) and land in `extra` like any other unmodeled
key — see the `extra_as::<CondaBuildConfig>()` worked example in research R2.

### 2.2 `Config::extra_as` — the caller-participation method (research R2)

```rust
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
    pub fn extra_as<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error>;
}
```

This is the one-call answer to "my application requires these specific, caller-known settings"
(e.g. a future conda-build-rewrite caller wanting `croot`/`bld_path`/`anaconda_upload`/
`conda_build`) without `condarc` ever needing to know about them. A caller wanting only
introspection (which unrecognized keys are present) reads `cfg.extra.keys()` / `cfg.extra.get(k)`
directly — no special path required. Both use cases are available simultaneously on the same
`Config` value (research R2's rationale for rejecting the generic-type-parameter alternative).

---

## 3. Setting value types (`model.rs`)  *(Key Entity: Setting value types)*

Typed shapes that make conda's accepted value set representable and invalid states hard to
represent (FR-010).

```rust
/// `channel_priority` — accepts the lowercase value, the SHOUTY-CASE member
/// name, or a JSON boolean / boolish string via the historical compat shim
/// (FR-016/017). Adapter emits the lowercase value (`"strict"`).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPriority { Strict, Flexible, Disabled }

/// `path_conflict`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathConflict { Clobber, Warn, Prevent }

/// `safety_checks`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyChecks { Enabled, Warn, Disabled }

/// `sat_solver`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SatSolver { Pycosat, Pycryptosat, Pysat }

/// `local_repodata_ttl`'s `(bool, int)` element type — narrower boolish
/// vocabulary than plain `bool` (FR-020, research §8 items 6/11).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoolOrInt { Bool(bool), Int(i64) }

/// `ssl_verify`'s `(str, bool)` element type (FR-024).
#[derive(Debug, Clone, PartialEq)]
pub enum SslVerify {
    Bool(bool),
    /// The literal string `"truststore"`.
    Truststore,
    /// A string that is boolish/numeric-looking but not a JSON bool, or
    /// (with `ParseOptions::ssl_verify_fs_check` set) an existing
    /// filesystem path (research R6).
    Path(String),
}

/// One closed member of `CONDA_LIST_FIELDS` (FR-022, §5.9) — 25 members,
/// matched exactly and case-sensitively with no trimming.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListField {
    Arch, Build, BuildNumber, Channel, ChannelName, Constrains, Depends,
    DistStr, Features, Fn, License, LicenseFamily, Md5, Name, Noarch,
    PackageType, RequestedSpec, RequestedSpecs, Sha256, Size, Subdir,
    Timestamp, TrackFeatures, Url, Version,
}

/// One `channel_settings` list entry — a string-to-string map. The
/// `channel` key is a documentation convention (settings.rst), **not**
/// source-enforced by any `context.py` validation callable (research §6),
/// so it is not required at parse time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChannelSetting(pub BTreeMap<String, String>);
```

**Validation rules embedded in these types**:
- `ChannelPriority`/`PathConflict`/`SafetyChecks`/`SatSolver` are constructed only via the
  value-or-name lookup coercer (FR-016), so an out-of-vocabulary casing is unrepresentable.
- `channel_priority` additionally accepts a JSON bool + boolish strings via the shim
  (FR-017/research §2.3): `true`/`yes`/`on` (any casing) → `Flexible`; `false`/`no`/`off` (any
  casing) → `Disabled`.
- `ListField` is parsed from the exact-case closed vocabulary; any other element produces a
  `type_coercion` or `semantic_validation` error entry (FR-022).

---

## 4. `ValueKind` — the coercion dispatch descriptor (`catalog.rs`)  *(DRY driver)*

Each setting's `element_type` (research §2.1) as a Rust enum, so one catalog table drives all
coercion for all 99 settings (FR-009/010/011):

```rust
enum ValueKind {
    Bool,                 // plain non-nullable bool
    NullableBool,         // (bool, None)
    SslVerifyKind,        // (str, bool), return_string passthrough
    BoolOrIntKind,        // (bool, int) — local_repodata_ttl, narrower vocab
    Int,
    Float,                // both A1 range-checked
    PlainString,          // str
    NullableString,       // (str, None); "none" (case-insensitive) -> null
    Enum(EnumKind),        // ChannelPriority | PathConflict | SafetyChecks | SatSolver
    StringSeq,            // SequenceParameter(str)
    ListFieldsSeq,        // SequenceParameter(str) + closed vocab
    StringMap,            // MapParameter(str)
    NullableStringMap,    // MapParameter((str, None))
    StringSeqMap,         // MapParameter(SequenceParameter(str)) — custom_multichannels only
    ChannelSettingsSeq,   // SequenceParameter(MapParameter(str)) — channel_settings only
}
```

## 5. `Setting` catalog entry (`catalog.rs`) — the full 99-entry table

```rust
struct Setting {
    canonical: &'static str,            // user-facing canonical name (Config field, adapter key)
    aliases: &'static [&'static str],   // other accepted spellings (FR-011); empty if none
    kind: ValueKind,                    // coercion shape (§4)
    validator: Option<SemanticValidator>, // channel_alias | default_python | ssl_verify | list_fields
}
static CATALOG: &[Setting] = &[
    // §4.1 Channel Configuration
    Setting { canonical: "channels", aliases: &["channel"], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "channel_alias", aliases: &[], kind: ValueKind::PlainString, validator: Some(SemanticValidator::ChannelAlias) },
    Setting { canonical: "channel_settings", aliases: &[], kind: ValueKind::ChannelSettingsSeq, validator: None },
    Setting { canonical: "default_channels", aliases: &[], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "override_channels_enabled", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "allowlist_channels", aliases: &["whitelist_channels"], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "denylist_channels", aliases: &[], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "custom_channels", aliases: &[], kind: ValueKind::StringMap, validator: None },
    Setting { canonical: "custom_multichannels", aliases: &[], kind: ValueKind::StringSeqMap, validator: None },
    Setting { canonical: "migrated_channel_aliases", aliases: &[], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "migrated_custom_channels", aliases: &[], kind: ValueKind::StringMap, validator: None },
    Setting { canonical: "add_anaconda_token", aliases: &["add_binstar_token"], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "allow_non_channel_urls", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "repodata_fns", aliases: &[], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "use_only_tar_bz2", aliases: &[], kind: ValueKind::NullableBool, validator: None },
    Setting { canonical: "repodata_threads", aliases: &[], kind: ValueKind::Int, validator: None },
    Setting { canonical: "fetch_threads", aliases: &[], kind: ValueKind::Int, validator: None },
    Setting { canonical: "experimental", aliases: &[], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "no_lock", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "repodata_use_zst", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "repodata_use_shards", aliases: &[], kind: ValueKind::Bool, validator: None },
    // §4.2 Basic Conda Configuration
    Setting { canonical: "envs_dirs", aliases: &["envs_path"], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "pkgs_dirs", aliases: &[], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "default_threads", aliases: &[], kind: ValueKind::Int, validator: None },
    Setting { canonical: "preview", aliases: &[], kind: ValueKind::StringSeq, validator: None },
    // §4.3 Network Configuration
    Setting { canonical: "client_ssl_cert", aliases: &["client_cert"], kind: ValueKind::NullableString, validator: None },
    Setting { canonical: "client_ssl_cert_key", aliases: &["client_cert_key"], kind: ValueKind::NullableString, validator: None },
    Setting { canonical: "local_repodata_ttl", aliases: &[], kind: ValueKind::BoolOrIntKind, validator: None },
    Setting { canonical: "offline", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "proxy_servers", aliases: &[], kind: ValueKind::NullableStringMap, validator: None },
    Setting { canonical: "remote_connect_timeout_secs", aliases: &[], kind: ValueKind::Float, validator: None },
    Setting { canonical: "remote_max_retries", aliases: &[], kind: ValueKind::Int, validator: None },
    Setting { canonical: "remote_backoff_factor", aliases: &[], kind: ValueKind::Int, validator: None },
    Setting { canonical: "remote_read_timeout_secs", aliases: &[], kind: ValueKind::Float, validator: None },
    Setting { canonical: "ssl_verify", aliases: &["verify_ssl"], kind: ValueKind::SslVerifyKind, validator: Some(SemanticValidator::SslVerify) },
    // §4.4 Solver Configuration
    Setting { canonical: "aggressive_update_packages", aliases: &[], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "auto_update_conda", aliases: &["self_update"], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "channel_priority", aliases: &[], kind: ValueKind::Enum(EnumKind::ChannelPriority), validator: None },
    Setting { canonical: "create_default_packages", aliases: &[], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "disallowed_packages", aliases: &["disallow"], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "force_reinstall", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "pinned_packages", aliases: &[], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "prefix_data_interoperability", aliases: &["pip_interop_enabled"], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "track_features", aliases: &[], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "solver", aliases: &["experimental_solver"], kind: ValueKind::PlainString, validator: None },
    // §4.5 Package Linking and Install-time Configuration
    Setting { canonical: "allow_softlinks", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "always_copy", aliases: &["copy"], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "always_softlink", aliases: &["softlink"], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "path_conflict", aliases: &[], kind: ValueKind::Enum(EnumKind::PathConflict), validator: None },
    Setting { canonical: "rollback_enabled", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "safety_checks", aliases: &[], kind: ValueKind::Enum(EnumKind::SafetyChecks), validator: None },
    Setting { canonical: "extra_safety_checks", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "signing_metadata_url_base", aliases: &[], kind: ValueKind::NullableString, validator: None },
    Setting { canonical: "shortcuts", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "shortcuts_only", aliases: &[], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "non_admin_enabled", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "separate_format_cache", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "verify_threads", aliases: &[], kind: ValueKind::Int, validator: None },
    Setting { canonical: "execute_threads", aliases: &[], kind: ValueKind::Int, validator: None },
    // §4.7 Output, Prompt, and Flow Control Configuration
    Setting { canonical: "always_yes", aliases: &["yes"], kind: ValueKind::NullableBool, validator: None },
    Setting { canonical: "auto_activate_base", aliases: &["auto_activate"], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "default_activation_env", aliases: &[], kind: ValueKind::PlainString, validator: None },
    Setting { canonical: "auto_stack", aliases: &[], kind: ValueKind::Int, validator: None },
    Setting { canonical: "changeps1", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "env_prompt", aliases: &[], kind: ValueKind::PlainString, validator: None },
    Setting { canonical: "json", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "console", aliases: &[], kind: ValueKind::PlainString, validator: None },
    Setting { canonical: "notify_outdated_conda", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "quiet", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "report_errors", aliases: &[], kind: ValueKind::NullableBool, validator: None },
    Setting { canonical: "show_channel_urls", aliases: &[], kind: ValueKind::NullableBool, validator: None },
    Setting { canonical: "list_fields", aliases: &[], kind: ValueKind::ListFieldsSeq, validator: Some(SemanticValidator::ListFields) },
    Setting { canonical: "verbosity", aliases: &["verbose"], kind: ValueKind::Int, validator: None },
    Setting { canonical: "unsatisfiable_hints", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "unsatisfiable_hints_check_depth", aliases: &[], kind: ValueKind::Int, validator: None },
    Setting { canonical: "number_channel_notices", aliases: &[], kind: ValueKind::Int, validator: None },
    Setting { canonical: "envvars_force_uppercase", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "export_platforms", aliases: &["extra_platforms"], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "override_virtual_packages", aliases: &["virtual_packages"], kind: ValueKind::NullableStringMap, validator: None },
    // §4.9 Hidden and Undocumented
    Setting { canonical: "allow_cycles", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "allow_conda_downgrades", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "add_pip_as_python_dependency", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "debug", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "trace", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "dev", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "default_python", aliases: &[], kind: ValueKind::NullableString, validator: Some(SemanticValidator::DefaultPython) },
    Setting { canonical: "enable_private_envs", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "error_upload_url", aliases: &[], kind: ValueKind::PlainString, validator: None },
    Setting { canonical: "force_32bit", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "root_prefix", aliases: &["root_dir"], kind: ValueKind::PlainString, validator: None },
    Setting { canonical: "sat_solver", aliases: &[], kind: ValueKind::Enum(EnumKind::SatSolver), validator: None },
    Setting { canonical: "solver_ignore_timestamps", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "subdir", aliases: &[], kind: ValueKind::PlainString, validator: None },
    Setting { canonical: "subdirs", aliases: &[], kind: ValueKind::StringSeq, validator: None },
    Setting { canonical: "target_prefix_override", aliases: &[], kind: ValueKind::PlainString, validator: None },
    Setting { canonical: "register_envs", aliases: &[], kind: ValueKind::Bool, validator: None },
    Setting { canonical: "protect_frozen_envs", aliases: &[], kind: ValueKind::Bool, validator: None },
    // §4.10 Plugin Configuration
    Setting { canonical: "no_plugins", aliases: &[], kind: ValueKind::Bool, validator: None },
    // §4.11 Experimental
    Setting { canonical: "environment_specifier", aliases: &["env_spec"], kind: ValueKind::NullableString, validator: None },
];
```

The catalog is the single source of truth for names, aliases, types, and validators — no per-key
logic is duplicated elsewhere (Constitution IV). Its declaration order (as written above) defines
error-entry ordering (research R7) and adapter key ordering.

**Alias pairs** (20 total, FR-029 / `multiple_keys_error_*` fixtures — each pair's two spellings
MUST NOT both appear in one document): `channels`/`channel`, `always_yes`/`yes`,
`ssl_verify`/`verify_ssl`, `always_copy`/`copy`, `always_softlink`/`softlink`,
`auto_update_conda`/`self_update`, `auto_activate_base`/`auto_activate`,
`prefix_data_interoperability`/`pip_interop_enabled`, `allowlist_channels`/`whitelist_channels`,
`disallowed_packages`/`disallow`, `client_ssl_cert`/`client_cert`,
`client_ssl_cert_key`/`client_cert_key`, `add_anaconda_token`/`add_binstar_token`,
`export_platforms`/`extra_platforms`, `override_virtual_packages`/`virtual_packages`,
`root_prefix`/`root_dir`, `solver`/`experimental_solver`, `verbosity`/`verbose`,
`environment_specifier`/`env_spec`, `envs_dirs`/`envs_path`.

---

## 6. `ParseOptions` — opt-in runtime behavior (`model.rs`, research R6)

```rust
/// Runtime options for [`parse_with_options`]. `ParseOptions::default()`
/// is the hermetic, conformance-portable behavior used by [`parse`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParseOptions {
    /// When `true`, a non-boolish, non-`truststore` `ssl_verify` string is
    /// additionally accepted if it names an existing filesystem path
    /// (matching conda's runtime behavior exactly). Default `false`: only
    /// the portable subset (bool/boolish/numeric/`truststore`) is
    /// accepted — the only environment access this crate ever performs,
    /// and only when this flag is explicitly set (FR-002, FR-024).
    pub ssl_verify_fs_check: bool,
}
```

A plain struct argument (not a Cargo feature) because the choice is per-call, not per-build: Cargo
features unify across the whole dependency graph of a binary, so one dependent enabling a feature
would silently turn it on for every other dependent — wrong for a behavior GEN-23 wants on and a
hermetic test suite wants off, from the same compiled library (research R6, full rationale there).

---

## 7. Error model (`error.rs`)  *(Key Entity: Parse/validation error)*

```rust
/// Accumulates every independent problem found while parsing one
/// document (FR-030/031). Implements [`std::error::Error`] (usable with
/// `?`) and [`serde::Serialize`] (the versioned JSON contract in
/// `contracts/error-report.schema.json`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ValidationReport {
    entries: Vec<ErrorEntry>,
}
impl ValidationReport {
    /// Every accumulated problem, in the deterministic order documented
    /// in research R7 (never the input document's key order).
    pub fn entries(&self) -> &[ErrorEntry] { &self.entries }
}
impl std::fmt::Display for ValidationReport { /* one line per entry, human-readable */ }
impl std::error::Error for ValidationReport {}

/// One problem in the document (FR-033). Every field is individually
/// accessible and typed — never only a formatted message string — so an
/// agent can branch on `kind` and locate `location` without parsing text.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ErrorEntry {
    pub location: Location,
    pub kind: ErrorKind,
    pub message: String,
    pub input: InputRepr,
    /// Populated only for `AliasCollision`/`CrossField`: every setting
    /// name involved (FR-033).
    pub involved: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum Location {
    Root,
    Setting(String),
    Nested { setting: String, path: Vec<PathSegment> }, // e.g. channel_settings[2].auth
}
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum PathSegment { Index(usize), Key(String) }

/// Stable, machine-readable problem category (FR-033). `serde` renames to
/// the exact strings in `contracts/error-report.schema.json`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    YamlSyntax,          // non-accumulable (FR-032a) — single-entry report
    RootShape,           // non-accumulable (FR-032b) — single-entry report
    TypeCoercion,
    SemanticValidation,
    AliasCollision,
    CrossField,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum InputRepr { Bool(bool), Int(i64), Float(f64), Str(String), Null, Seq, Map, Raw(String) }
```

**State/flow**: `parse()`/`parse_with_options()` produce exactly one of: `Ok(Config)`; `Err(report)`
with a single non-accumulable entry (`YamlSyntax` OR `RootShape`); or `Err(report)` with ≥1
accumulated per-field/alias/cross-field entries. Never both a `Config` and errors; never a panic.

---

## 8. Entity relationships

```text
&str (YAML text)
  │  yaml-rust2 (R1)                    NOT serde — no Deserializer impl
  ▼
yaml_rust2::Yaml                        resolved scalar type: Boolean/Integer/Real/String/Null/...
  │  parse.rs: lower(&Yaml) -> RawValue         (hand-written match)
  ▼
RawValue (§1, crate-private)
  │
  ├─ root Null  ───────────────────────────────────────────────▶ Ok(Config::default())
  ├─ root Seq/scalar ──────────────────────────────────────────▶ Err(ValidationReport{1 entry: RootShape})
  └─ root Map ─▶ per-key loop over CATALOG (§5)
        │
        ├─ key matches Setting.canonical/alias ─▶ coerce/* (Setting.kind, §4) ─┬─▶ Ok(value)  ─▶ set Config.<canonical>
        │                                                                      └─▶ Err(entry) ─▶ push to report.entries
        └─ key matches nothing in CATALOG ──────▶ RawValue -> serde_json::Value ─▶ Config::extra[key]
  │
  ▼ (after the per-key loop)
alias-collision pass + cross-field pass (validate.rs) ─▶ more entries if violated
  │
  ▼
report.entries.is_empty()?
  ├─ yes ─▶ Ok(Config)
  └─ no  ─▶ Err(ValidationReport)

(YAML syntax error, anywhere above RawValue) ─▶ Err(ValidationReport{1 entry: YamlSyntax})

Config ─▶ (test-crate only, research R9) to_expected_json(&Config) -> serde_json::Value
Config ─▶ Config::extra_as::<T>() -> Result<T, serde_json::Error>   (research R2, public API)
```

## 9. Validation-rule inventory (traceability to FRs)

| Rule | Where | FR |
|---|---|---|
| root shape (null/map ok; seq/scalar err) | `parse.rs` | FR-005..007, FR-032 |
| YAML syntax → single error | `parse.rs` | FR-008, FR-032 |
| boolish coercion (3 variants) | `coerce/boolish.rs` | FR-012/013 |
| numeric coercion + `i64`/`f64` bound | `coerce/numeric.rs` | FR-018/019, A1 |
| `local_repodata_ttl` narrow vocab | `coerce/numeric.rs` | FR-020 |
| enum value-or-name + `channel_priority` shim | `coerce/enums.rs` | FR-016/017 |
| plain/nullable string | `coerce/strings.rs` | FR-014/015 |
| sequence raw-shape + element typify | `coerce/sequences.rs` | FR-021 |
| `list_fields` closed vocab | `coerce/sequences.rs` + `validate.rs` | FR-022 |
| map settings incl. multichannels/channel_settings | `coerce/sequences.rs` | FR-023 |
| `ssl_verify` (bool/boolish/truststore/path) | `coerce/boolish.rs` + `validate.rs` | FR-024, R6 |
| `channel_alias` scheme regex `^$|^[a-z][a-z0-9]{0,11}://` | `validate.rs` | FR-025 |
| `default_python` `^$|^[23]\.[0-9]{1,2}$` | `validate.rs` | FR-026 |
| `client_ssl_cert_key` requires `client_ssl_cert` | `validate.rs` | FR-027 |
| `always_copy` ⊕ `always_softlink` | `validate.rs` | FR-028 |
| alias collision (20 pairs) | `validate.rs` | FR-029 |
| unknown keys accepted silently, retained in `extra` | `parse.rs`/`model.rs` | FR-036 |
| `extra_as::<T>()` caller-participation method | `model.rs` | research R2 |
| `ParseOptions.ssl_verify_fs_check` opt-in FS check | `model.rs`, `coerce/boolish.rs` | FR-002/024, R6 |
