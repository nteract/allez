//! Integration test for `contracts/public-api.md`'s "End-to-end usage" section (GEN-36 User
//! Story 3, quickstart.md Scenario 4): confirms the public API contract itself, not just the
//! conformance corpus — reading a file, handling a missing file, handling a rejected document,
//! using typed values, and interpreting an unmodeled key via `extra_as`.

use std::fs;
use std::path::Path;

use condarc::{ChannelPriority, Config, ErrorKind, Location, ParseOptions, ValidationReport};

// ---------------------------------------------------------------------
// 1. Reading the file and calling `parse` (the caller owns I/O — FR-001)
// ---------------------------------------------------------------------

/// Mirrors `contracts/public-api.md`'s `load_condarc` example exactly: the crate performs no
/// filesystem access itself (FR-002); the caller reads the file and decides how to handle a
/// missing/unreadable file (GEN-23's "tolerate a missing file, fall back to defaults" policy
/// lives in the caller, not in `condarc`).
fn load_condarc(path: &Path) -> Result<Config, LoadError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(LoadError::Io(e)),
    };
    condarc::parse(&text).map_err(LoadError::Invalid)
}

#[derive(Debug)]
#[allow(dead_code)] // the variant payloads exist for a real caller's `Display`/`source`; this
// test only distinguishes the two error paths structurally, via `matches!` below.
enum LoadError {
    Io(std::io::Error),
    Invalid(ValidationReport),
}

#[test]
fn missing_file_falls_back_to_default_config() {
    let path = std::env::temp_dir().join("condarc_public_api_usage_missing_file.condarc");
    let _ = fs::remove_file(&path); // best-effort: make sure it really doesn't exist.

    let cfg = load_condarc(&path).expect("a missing file must fall back to Config::default()");
    assert_eq!(cfg, Config::default());
}

#[test]
fn reading_an_existing_file_parses_its_contents() {
    let path = std::env::temp_dir().join("condarc_public_api_usage_existing_file.condarc");
    fs::write(
        &path,
        "channels: [conda-forge, defaults]\nalways_yes: yes\n",
    )
    .expect("failed to write temp fixture file");

    let cfg = load_condarc(&path).expect("a valid file must parse successfully");
    fs::remove_file(&path).ok();

    assert_eq!(
        cfg.channels.as_deref(),
        Some(&["conda-forge".to_string(), "defaults".to_string()][..])
    );
    assert_eq!(cfg.always_yes, Some(Some(true)));
}

// ---------------------------------------------------------------------
// 2. Handling a rejected document — the full structured report
//    (FR-030/FR-033/FR-037)
// ---------------------------------------------------------------------

#[test]
fn a_rejected_document_can_be_iterated_and_branched_on_by_kind() {
    // Same three independently-invalid settings as
    // `multi_error_accumulation.rs` (SC-005): a caller that wants to *act* on individual
    // problems iterates `entries()` and branches on `kind`, rather than parsing a message
    // string (contracts/public-api.md §"End-to-end usage" #2).
    let yaml = r#"
channel_alias: "no-scheme-here"
remote_max_retries: "99999999999999999999999999999999"
always_copy: "banana"
"#;

    let report = condarc::parse(yaml).expect_err("document has three independent problems");

    let mut saw_type_coercion = 0;
    let mut saw_semantic_validation = 0;
    for entry in report.entries() {
        match entry.kind {
            ErrorKind::TypeCoercion => saw_type_coercion += 1,
            ErrorKind::SemanticValidation => saw_semantic_validation += 1,
            other => panic!("unexpected error kind for this document: {other:?}"),
        }
        // Every entry carries its own location/kind/message/input — confirm at least the
        // location is reachable without parsing any message string.
        match &entry.location {
            Location::Setting { setting } => assert!(!setting.is_empty()),
            other => panic!("expected a Setting location, got {other:?}"),
        }
    }
    assert_eq!(saw_type_coercion, 2); // remote_max_retries, always_copy
    assert_eq!(saw_semantic_validation, 1); // channel_alias

    // Machine-readable form for an agent pipeline (Constitution III): the report must
    // serialize to the documented JSON contract (contracts/error-report.schema.json).
    let json = serde_json::to_value(&report).expect("ValidationReport must serialize");
    assert_eq!(json["schema_version"], "1.0.0");
    assert_eq!(json["entries"].as_array().expect("entries array").len(), 3);
}

// ---------------------------------------------------------------------
// 3. Using the typed values (why the caller doesn't need its own parsing)
// ---------------------------------------------------------------------

#[test]
fn typed_config_fields_read_back_without_caller_side_reparsing() {
    let cfg = condarc::parse("channels: [conda-forge]\nchannel_priority: strict\n")
        .expect("valid document");
    assert_eq!(cfg.channels, Some(vec!["conda-forge".to_string()]));
    assert_eq!(cfg.channel_priority, Some(ChannelPriority::Strict));
}

#[test]
fn always_yes_tri_state_distinguishes_absent_null_and_set() {
    // Option<Option<bool>>: outer None = absent, Some(None) = explicit null, Some(Some(v)) =
    // present with a value (data-model.md §2, research §8 item 13).
    let absent = condarc::parse("channels: [conda-forge]\n").expect("valid document");
    assert_eq!(absent.always_yes, None);

    let explicit_null = condarc::parse("always_yes: null\n").expect("valid document");
    assert_eq!(explicit_null.always_yes, Some(None));

    let explicit_value = condarc::parse("always_yes: true\n").expect("valid document");
    assert_eq!(explicit_value.always_yes, Some(Some(true)));
}

// ---------------------------------------------------------------------
// 4. Opting into the real `ssl_verify` filesystem check (a caller's own
//    runtime config)
// ---------------------------------------------------------------------

#[test]
fn parse_with_options_can_opt_into_the_ssl_verify_filesystem_check() {
    // `std::env::temp_dir()` always exists on every platform this crate targets, so this stays
    // deterministic without needing a dedicated tempfile dependency (spec A3).
    let existing_dir = std::env::temp_dir();
    let yaml = format!("ssl_verify: {:?}\n", existing_dir.display().to_string());

    let options = ParseOptions::default().with_ssl_verify_fs_check(true);
    let cfg = condarc::parse_with_options(&yaml, options)
        .expect("an existing path must be accepted when ssl_verify_fs_check is enabled");
    assert!(cfg.ssl_verify.is_some());

    let nonexistent = std::env::temp_dir().join("condarc_public_api_usage_does_not_exist_dir");
    let yaml_missing = format!("ssl_verify: {:?}\n", nonexistent.display().to_string());
    let result = condarc::parse_with_options(&yaml_missing, options);
    assert!(
        result.is_err(),
        "a nonexistent path must be rejected when ssl_verify_fs_check is enabled"
    );
}

// ---------------------------------------------------------------------
// 5. Interpreting settings the crate doesn't model — the caller's own
//    config struct (research R2)
// ---------------------------------------------------------------------

#[derive(serde::Deserialize, Debug, Default, PartialEq)]
struct CondaBuildSettings {
    #[serde(rename = "root-dir")]
    root_dir: Option<String>,
    pkg_format: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default, PartialEq)]
struct CondaBuildConfig {
    croot: Option<String>,
    bld_path: Option<String>,
    anaconda_upload: Option<bool>,
    conda_build: Option<CondaBuildSettings>,
}

#[test]
fn extra_as_deserializes_conda_builds_out_of_scope_keys_in_one_call() {
    let yaml = r#"
channels: [conda-forge]
croot: /home/user/conda-bld
bld_path: /home/user/conda-bld/output
anaconda_upload: false
conda_build:
  root-dir: /home/user/conda-bld
  pkg_format: "2"
"#;

    let cfg = condarc::parse(yaml)
        .expect("valid document (conda-build keys are unmodeled, not invalid — FR-036)");
    assert_eq!(cfg.channels, Some(vec!["conda-forge".to_string()]));

    let build: CondaBuildConfig = cfg.extra_as().expect("shape matches");
    assert_eq!(
        build,
        CondaBuildConfig {
            croot: Some("/home/user/conda-bld".to_string()),
            bld_path: Some("/home/user/conda-bld/output".to_string()),
            anaconda_upload: Some(false),
            conda_build: Some(CondaBuildSettings {
                root_dir: Some("/home/user/conda-bld".to_string()),
                pkg_format: Some("2".to_string()),
            }),
        }
    );

    // A caller that just wants to introspect what's unrecognized, without declaring a struct,
    // reads the map directly.
    assert!(cfg.extra.contains_key("croot"));
    assert!(cfg.extra.contains_key("conda_build"));
}
