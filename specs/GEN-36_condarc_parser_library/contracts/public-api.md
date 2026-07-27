# Contract: `condarc` Public Rust API

**Status**: Phase 1 design contract (not yet implemented). Version: `0.1.0` (pre-1.0; SemVer per
Constitution & plan §III). Breaking changes to any item below require a MAJOR bump once ≥1.0.

This is the interface the crate exposes to callers (GEN-23, the conformance harness, future
publication, e.g. a Rust rewrite of `conda-build`). Signatures are indicative Rust; every public
item ships with `///` docs and, where practical, a runnable example (Constitution VI). The full
field-by-field shape of `Config` and its supporting types lives in `../data-model.md` §2–§7; this
document is the *usage* contract — how a caller drives the API end-to-end, including error
handling and its own downstream configuration.

## Entry points

```rust
/// Parse the text of a single `.condarc` document into a typed [`Config`],
/// or a [`ValidationReport`] accumulating every problem found. Equivalent
/// to `parse_with_options(yaml, ParseOptions::default())` — the hermetic,
/// conformance-portable path (no filesystem access).
///
/// The caller is responsible for locating and reading the file (FR-001).
/// Never panics on any input (FR-004).
///
/// # Errors
/// Returns `Err(ValidationReport)` on YAML syntax errors, non-mapping
/// roots, or any accumulated per-setting / alias / cross-field validation
/// failure.
///
/// # Example
/// ```
/// let cfg = condarc::parse("channels: [conda-forge, defaults]\nalways_yes: yes")?;
/// assert_eq!(cfg.channels.as_deref(), Some(&["conda-forge".to_string(), "defaults".to_string()][..]));
/// assert_eq!(cfg.always_yes, Some(Some(true)));
/// # Ok::<(), condarc::ValidationReport>(())
/// ```
pub fn parse(yaml: &str) -> Result<Config, ValidationReport>;

/// Parse with explicit [`ParseOptions`]. Use this to opt into the real
/// filesystem `ssl_verify` existence check (`ssl_verify_fs_check: true`)
/// for exact conda runtime fidelity; the default [`parse`] never touches
/// the filesystem and accepts an `ssl_verify` path string unverified
/// (FR-002/FR-024, research R6).
///
/// # Errors
/// Same failure modes as [`parse`].
pub fn parse_with_options(yaml: &str, options: ParseOptions) -> Result<Config, ValidationReport>;
```

## Types (re-exported from crate root)

| Type | Kind | Contract |
|---|---|---|
| `Config` | struct, `#[non_exhaustive]` | One `Option<_>` (or `Option<Option<_>>` for nullable settings) field per recognized setting, canonical-named (conda's loader name — e.g. `auto_activate`, not `auto_activate_base`) — full field list in `data-model.md` §2.1. Absent = `None`, uniformly, with no defaulting layer of any kind (FR-038). Plus `extra: HashMap<String, serde_json::Value>` for retained unknown keys (research R2) and the `extra_as::<T>()` method. All fields are `pub`. |
| `ParseOptions` | struct, `#[non_exhaustive]`, `Default` | `ssl_verify_fs_check: bool` (default `false` = no filesystem access, `ssl_verify` paths accepted unverified) and `null_sequence_map_defaults: bool` (default `false` = an explicit `null` on a sequence-/map-typed setting is treated as absent, no conda default backfilled; `true` resolves it to conda's own class-level default, Assumption A7) — see `data-model.md` §6. |
| `ValidationReport` | struct | Owns the accumulated entries; `impl std::error::Error + Display + Serialize`. `entries()` returns `&[ErrorEntry]` (FR-037). |
| `ErrorEntry` | struct | Public fields `location`, `kind`, `message`, `input`, `involved` (FR-033/037). `Serialize`. |
| `ErrorKind` | enum, `#[non_exhaustive]` | Stable serde strings (see `error-report.schema.json`). Consumers branch on this (FR-037). |
| `Location`, `PathSegment` | enum | Nested-location addressing (FR-033); struct variants, internally tagged as `"type"` on the wire to match `error-report.schema.json`. |
| `ChannelPriority`, `PathConflict`, `SafetyChecks`, `SatSolver`, `ListField` | enum, `#[non_exhaustive]` | Closed-vocabulary enums (FR-010/016/022). |
| `BoolOrInt`, `SslVerify`, `ChannelSetting` | enum/struct | Mixed-shape setting values — `data-model.md` §3. |

## End-to-end usage (how a caller actually drives this crate)

### 1. Reading the file and calling `parse` (the caller owns I/O — FR-001)

```rust
use std::fs;

fn load_condarc(path: &std::path::Path) -> Result<condarc::Config, LoadError> {
    // The crate performs NO filesystem access itself (FR-002); the caller
    // reads the file and decides how to handle a missing/unreadable file
    // (GEN-23's "tolerate a missing file, fall back to defaults" policy
    // lives in the caller, not in condarc).
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(condarc::Config::default()),
        Err(e) => return Err(LoadError::Io(e)),
    };
    condarc::parse(&text).map_err(LoadError::Invalid)
}

#[derive(Debug)]
enum LoadError {
    Io(std::io::Error),
    Invalid(condarc::ValidationReport),
}
impl std::fmt::Display for LoadError { /* ... */ }
impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LoadError::Io(e) => Some(e),
            LoadError::Invalid(e) => Some(e), // ValidationReport implements Error (FR-034/037)
        }
    }
}
```

### 2. Handling a rejected document — the full structured report (FR-030/FR-033/FR-037)

A caller that wants to *act* on individual problems (an editor plugin, an agent auto-repairing a
file, a CLI printing every issue at once) iterates `entries()` and branches on `kind`, rather than
parsing a message string:

```rust
match condarc::parse(&text) {
    Ok(cfg) => use_config(cfg),
    Err(report) => {
        for entry in report.entries() {
            match entry.kind {
                condarc::ErrorKind::TypeCoercion | condarc::ErrorKind::SemanticValidation => {
                    eprintln!("{}: {} (got {:?})", describe(&entry.location), entry.message, entry.input);
                }
                condarc::ErrorKind::AliasCollision | condarc::ErrorKind::CrossField => {
                    eprintln!("{} (involves: {})", entry.message, entry.involved.join(", "));
                }
                condarc::ErrorKind::RootShape | condarc::ErrorKind::YamlSyntax => {
                    eprintln!("cannot parse .condarc at all: {}", entry.message);
                }
                // #[non_exhaustive]: a future ErrorKind variant falls here,
                // not a compile error (Constitution: additive, non-breaking).
                _ => eprintln!("{}", entry.message),
            }
        }
        // Machine-readable form for an agent pipeline (Constitution III):
        let json = serde_json::to_string_pretty(&report)?; // schema: contracts/error-report.schema.json
        return Err(json.into());
    }
}

fn describe(loc: &condarc::Location) -> String {
    match loc {
        condarc::Location::Root => "<root>".to_string(),
        condarc::Location::Setting { setting } => setting.clone(),
        condarc::Location::Nested { setting, path } => {
            format!("{setting}{}", path.iter().map(|s| match s {
                condarc::PathSegment::Index { index } => format!("[{index}]"),
                condarc::PathSegment::Key { key } => format!(".{key}"),
            }).collect::<String>())
        }
    }
}
```

Every entry carries its own `location`/`kind`/`message`/`input` — a rejected document with, say, a
bad `channel_alias`, an out-of-range `remote_max_retries`, and a non-boolish `always_copy` yields
**three** entries in one `parse()` call, not just the first (SC-005); the caller sees and can fix
all of them in a single pass, exactly like Pydantic's `ValidationError.errors()`.

### 3. Using the typed values (why the caller doesn't need its own parsing)

```rust
fn build_channel_list(cfg: &condarc::Config) -> Vec<String> {
    let mut channels = cfg.channels.clone().unwrap_or_default();
    if channels.is_empty() {
        channels.push("defaults".to_string()); // caller's own default policy — condarc never backfills (FR-038)
    }
    channels
}

fn solver_strictness(cfg: &condarc::Config) -> bool {
    // Config::channel_priority is already the typed enum — no re-parsing
    // "strict"/"STRICT"/true/"yes" by hand; condarc already normalized it.
    matches!(cfg.channel_priority, Some(condarc::ChannelPriority::Strict))
}

fn effective_ssl_verify(cfg: &condarc::Config) -> Option<&condarc::SslVerify> {
    cfg.ssl_verify.as_ref() // Option::None means "absent", never a made-up default (FR-038)
}

fn wants_prompt_confirmation(cfg: &condarc::Config) -> bool {
    // always_yes is Option<Option<bool>>: None = absent, Some(None) = explicit
    // null, Some(Some(v)) = set. A caller collapses that 3-state space to
    // its own 2-state policy explicitly, rather than condarc guessing:
    !matches!(cfg.always_yes, Some(Some(true)))
}
```

### 4. Opting into the real `ssl_verify` filesystem check (a caller's own runtime config)

```rust
// GEN-23's CLI wants exact conda runtime fidelity in production (reject an
// ssl_verify path that doesn't exist), but its own test suite wants the
// side-effect-free default (no filesystem dependency; an ssl_verify path is
// accepted unverified). Both are the same compiled `condarc`; the choice is
// a plain call-site value, never a build-time switch (research R6). The
// conformance harness passes `true` for the same reason GEN-23 does.
fn parse_for_runtime(yaml: &str) -> Result<condarc::Config, condarc::ValidationReport> {
    let options = condarc::ParseOptions::default().with_ssl_verify_fs_check(true);
    condarc::parse_with_options(yaml, options)
}

#[cfg(test)]
fn parse_for_test(yaml: &str) -> Result<condarc::Config, condarc::ValidationReport> {
    condarc::parse(yaml) // ParseOptions::default(): ssl_verify_fs_check = false
}
```

### 4b. Opting into conda's own default for an explicit `null` sequence/map setting (Assumption A7)

```rust
// Default (`ParseOptions::default()` / plain `parse`): an explicit `null` for a
// SequenceParameter-/MapParameter-typed setting (`channels`, `custom_channels`, ...) is
// indistinguishable from that key being entirely absent -- both read back as `None`, per FR-038.
let cfg = condarc::parse("custom_channels: null\nchannels: null\n")?;
assert_eq!(cfg.custom_channels, None);
assert_eq!(cfg.channels, None);

// Opt-in: resolves to conda's own class-level default instead -- non-empty for
// `custom_channels` (DEFAULT_CUSTOM_CHANNELS), empty for `channels`. Absent keys are still
// always `None` regardless of this option; only an *explicit* `null` is affected.
let options = condarc::ParseOptions::default().with_null_sequence_map_defaults(true);
let cfg = condarc::parse_with_options("custom_channels: null\nchannels: null\n", options)?;
assert_eq!(cfg.channels, Some(Vec::new()));
# Ok::<(), condarc::ValidationReport>(())
```

### 5. Interpreting settings the crate doesn't model — the caller's own config struct (research R2)

`condarc`'s catalog is fixed to the ~99 settings in `data-model.md` §2.1; conda-build's four keys
(`bld_path`, `croot`, `anaconda_upload`/`binstar_upload`, `conda_build`) are explicitly out of
scope (FR-009) but still present in a real `.condarc`. A caller that needs them defines its own
struct once and gets it in one call, alongside the settings `condarc` already parsed:

```rust
#[derive(serde::Deserialize, Debug, Default)]
struct CondaBuildSettings {
    #[serde(rename = "root-dir")]
    root_dir: Option<String>,
    pkg_format: Option<String>,
    zstd_compression_level: Option<i64>,
    #[serde(default)]
    no_lock: bool,
}

#[derive(serde::Deserialize, Debug, Default)]
struct CondaBuildConfig {
    croot: Option<String>,
    bld_path: Option<String>,
    anaconda_upload: Option<bool>,
    conda_build: Option<CondaBuildSettings>,
}

fn load_for_conda_build(path: &std::path::Path) -> anyhow::Result<(condarc::Config, CondaBuildConfig)> {
    let text = std::fs::read_to_string(path)?;
    let cfg = condarc::parse(&text)?; // channels, channel_priority, ... typed as usual
    let build: CondaBuildConfig = cfg.extra_as()?; // one call over the whole unknown-key tail
    Ok((cfg, build))
}

// A caller that just wants to *introspect* what's unrecognized, without
// declaring a struct, uses the map directly — no special path required:
fn warn_on_typos(cfg: &condarc::Config, known_typo_hints: &[&str]) {
    for key in cfg.extra.keys() {
        if known_typo_hints.contains(&key.as_str()) {
            eprintln!("warning: '{key}' looks like a typo of a real setting");
        }
    }
}
```

Fields `CondaBuildConfig` doesn't declare are dropped (ordinary serde; `extra_as` is not
`deny_unknown_fields`); fields it declares that are absent deserialize to `None`/their `Default`.
`extra` and `extra_as` are always available on every `Config`, whether or not a caller uses them —
paying nothing for callers who only touch the ~99 catalog settings (research R2's rationale for
rejecting a generic `Config<Extra>` type parameter).

## Adapter (test-only — not part of the published crate's surface)

`to_expected_json(&Config) -> serde_json::Value` renders the internal representation into the
`conformance/condarc/expected/*.json` shape. Per research R9/R10 it lives in the **conformance
harness's own test target** (`tests/support/adapter.rs`, used via `mod support;` from
`tests/condarc_conformance.rs`) — not in `crates/condarc/src/`, and not in `crates/condarc/tests/`
(unreachable from the harness) — and is documented separately in `adapter-output.md`. It is not part of
this public-API contract because no real caller (GEN-23, a future publication) needs conda's exact wire
shape — they consume typed `Config` fields directly (§ "Using the typed values" above).

## Guarantees (invariants callers may rely on)

1. **Total, panic-free**: every `&str` input yields `Ok`/`Err`, never a panic (FR-004/035).
2. **Deterministic**: identical input → identical `Config` and identical `ValidationReport`
   (including entry order — research R7) (Constitution IX).
3. **Complete errors**: a rejected document reports *all* independent problems, except the two
   non-accumulable single-entry classes `YamlSyntax`/`RootShape` (FR-030/031/032).
4. **Absent means absent**: no defaults table, no effective-value layer, and no synthesized `false`
   for off-state booleans; a field is `None` iff the setting was not present in the document
   (FR-038). Callers apply their own default policy explicitly (§3 above). The sole exception is
   the opt-in `ParseOptions.null_sequence_map_defaults` (§4b, Assumption A7): even when enabled,
   an absent key is still always `None` — only an *explicit* `null` on a sequence-/map-typed
   setting resolves to conda's own default instead.
5. **Silent unknowns**: unknown top-level keys never cause an error (FR-036); they are retained in
   `Config::extra: HashMap<String, serde_json::Value>` — a neutral, parser-independent type — for
   inspection or `extra_as::<T>()` deserialization (research R2).
6. **No implicit I/O**: there is no path-taking entry point at all (FR-001 — callers read the file,
   stdin, or anything else themselves), and `parse()` never touches the filesystem/network/env, so
   its result is a pure function of the input string. `parse_with_options()` touches the filesystem
   only for `ssl_verify` path existence, and only when the caller explicitly sets
   `ParseOptions.ssl_verify_fs_check = true` (FR-002/024, research R6).

## Stability / SemVer notes

- `Config`, `ParseOptions`, and `ErrorKind` are `#[non_exhaustive]`: adding a setting / option
  field / error kind is MINOR.
- Renaming/removing a public field, or an `ErrorKind` serde string, is MAJOR.
- The JSON error-report schema is versioned independently (`error-report.schema.json`); a breaking
  JSON change is a MAJOR bump (Constitution III).
- `parse`/`parse_with_options` never gain a generic type parameter (research R2's rejected
  alternative) — the unknown-key tail is handled entirely through `Config::extra`/`extra_as`.
