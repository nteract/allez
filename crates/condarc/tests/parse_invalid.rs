//! Integration tests for User Story 2 (spec.md acceptance scenarios 1–5): feeding malformed or
//! type-invalid `.condarc` text to `condarc::parse` produces a structured, never-panicking
//! `ValidationReport`.

use condarc::{ErrorKind, Location};

/// Scenario 1: `channel_alias` with no URL scheme is rejected with a `semantic_validation` entry
/// located at `channel_alias`, carrying the offending value.
#[test]
fn channel_alias_without_a_scheme_is_a_semantic_validation_entry() {
    let report = condarc::parse("channel_alias: no-scheme-here\n")
        .expect_err("missing scheme must be rejected");

    assert_eq!(report.entries().len(), 1);
    let entry = &report.entries()[0];
    assert_eq!(entry.kind, ErrorKind::SemanticValidation);
    assert_eq!(
        entry.location,
        Location::Setting {
            setting: "channel_alias".to_string()
        }
    );
    assert_eq!(
        entry.input,
        condarc::InputRepr::Str {
            value: "no-scheme-here".to_string()
        }
    );
}

/// Scenario 2: a list or bare-scalar root produces a single, structured `root_shape` entry —
/// never a panic — even though real conda crashes on this input.
#[test]
fn list_and_scalar_roots_produce_a_single_root_shape_entry() {
    for yaml in ["- one\n- two\n", "just a bare scalar\n", "42\n"] {
        let report = condarc::parse(yaml).expect_err("non-mapping root must be rejected");
        assert_eq!(report.entries().len(), 1, "yaml={yaml:?}");
        assert_eq!(
            report.entries()[0].kind,
            ErrorKind::RootShape,
            "yaml={yaml:?}"
        );
        assert_eq!(
            report.entries()[0].location,
            Location::Root,
            "yaml={yaml:?}"
        );
    }
}

/// Syntactically invalid YAML produces a single `yaml_syntax` entry, distinct from a validation
/// error (FR-008).
#[test]
fn syntactically_invalid_yaml_produces_a_single_yaml_syntax_entry() {
    let report =
        condarc::parse("channels: [unterminated\n").expect_err("bad YAML must be rejected");
    assert_eq!(report.entries().len(), 1);
    assert_eq!(report.entries()[0].kind, ErrorKind::YamlSyntax);
    assert_eq!(report.entries()[0].location, Location::Root);
}

/// Scenario 3: setting both aliases of one setting in the same document produces an
/// `alias_collision` entry naming both keys.
#[test]
fn setting_both_aliases_of_one_setting_is_an_alias_collision() {
    let report = condarc::parse("always_yes: true\nyes: true\n")
        .expect_err("alias collision must be rejected");

    // Both spellings coerce to a perfectly valid `bool`, so the alias collision must be the
    // *only* problem reported -- no spurious type_coercion entry alongside it.
    assert_eq!(report.entries().len(), 1);

    let collisions: Vec<_> = report
        .entries()
        .iter()
        .filter(|e| e.kind == ErrorKind::AliasCollision)
        .collect();
    assert_eq!(collisions.len(), 1);
    assert_eq!(
        collisions[0].involved,
        vec!["always_yes".to_string(), "yes".to_string()]
    );
}

/// Scenario 4a: `always_copy` and `always_softlink` both truthy is a `cross_field` violation.
#[test]
fn always_copy_and_always_softlink_both_true_is_a_cross_field_violation() {
    let report = condarc::parse("always_copy: true\nalways_softlink: true\n")
        .expect_err("mutually-exclusive settings must be rejected");

    assert_eq!(report.entries().len(), 1);
    assert_eq!(report.entries()[0].kind, ErrorKind::CrossField);
    assert_eq!(
        report.entries()[0].involved,
        vec!["always_copy".to_string(), "always_softlink".to_string()]
    );
}

/// Scenario 4b: `client_ssl_cert_key` set without `client_ssl_cert` is a `cross_field` violation.
#[test]
fn client_ssl_cert_key_without_client_ssl_cert_is_a_cross_field_violation() {
    let report = condarc::parse("client_ssl_cert_key: /path/to/key\n")
        .expect_err("client_ssl_cert_key without client_ssl_cert must be rejected");

    assert_eq!(report.entries().len(), 1);
    assert_eq!(report.entries()[0].kind, ErrorKind::CrossField);
    assert_eq!(
        report.entries()[0].involved,
        vec![
            "client_ssl_cert".to_string(),
            "client_ssl_cert_key".to_string()
        ]
    );
}

/// Scenario 6: the failure report serializes to a JSON list of entries, each with a location, a
/// machine-readable kind, a human-readable message, and the offending input.
#[test]
fn the_report_serializes_to_a_machine_readable_json_structure() {
    let report = condarc::parse("channel_alias: no-scheme-here\n").expect_err("must be rejected");
    let json = serde_json::to_value(&report).expect("report must serialize");

    assert_eq!(json["schema_version"], "1.0.0");
    let entries = json["entries"].as_array().expect("entries is an array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["location"]["type"], "setting");
    assert_eq!(entries[0]["kind"], "semantic_validation");
    assert!(entries[0]["message"].is_string());
    assert!(entries[0]["input"].is_object());
}

/// Never panics on any of the previous edge cases, nor on a syntactically valid document that is
/// nonetheless nonsensical for every setting it sets (FR-004/FR-035).
#[test]
fn never_panics_on_a_battery_of_malformed_documents() {
    let documents = [
        "",
        "~",
        "- 1\n- 2\n",
        "true\n",
        "channels: not-a-list\n",
        "channel_priority: Strict\n",
        "remote_max_retries: nan\n",
        "list_fields: [not_a_real_field]\n",
        "1: x\nfoo: bar\n",
        "---\nfoo: 1\n---\nbar: 2\n",
    ];
    for doc in documents {
        let _ = condarc::parse(doc);
    }
}

// ---- FR-007b: non-string mapping keys ----

/// A non-string key at the document root produces a single `type_coercion` entry located at
/// `Root`, and does not prevent the rest of the document from parsing (FR-007b).
#[test]
fn root_level_non_string_key_is_a_type_coercion_entry_located_at_root() {
    let report =
        condarc::parse("1: x\nchannels: [conda-forge]\n").expect_err("non-string key rejected");

    assert_eq!(report.entries().len(), 1);
    assert_eq!(report.entries()[0].kind, ErrorKind::TypeCoercion);
    assert_eq!(report.entries()[0].location, Location::Root);
}

/// Multiple non-string keys at the root each produce their own entry (accumulable, per
/// FR-007b/FR-031), not just the first one encountered.
#[test]
fn multiple_root_level_non_string_keys_each_produce_their_own_entry() {
    let report =
        condarc::parse("1: a\n2: b\nchannels: [x]\n").expect_err("non-string keys rejected");
    assert_eq!(report.entries().len(), 2);
    for entry in report.entries() {
        assert_eq!(entry.kind, ErrorKind::TypeCoercion);
        assert_eq!(entry.location, Location::Root);
    }
}

/// A non-string key nested inside a specific setting's value (here, one `channel_settings` list
/// element) is located at that enclosing setting, not at `Root` — and the rest of that element is
/// still parsed (the sibling `channel` key survives).
#[test]
fn nested_non_string_key_is_located_at_the_enclosing_setting() {
    let yaml = "channel_settings:\n  - 1: x\n    channel: y\n";
    let report = condarc::parse(yaml).expect_err("non-string nested key rejected");

    assert_eq!(report.entries().len(), 1);
    assert_eq!(report.entries()[0].kind, ErrorKind::TypeCoercion);
    assert_eq!(
        report.entries()[0].location,
        Location::Setting {
            setting: "channel_settings".to_string()
        }
    );
}

/// A document that is otherwise entirely valid except for one dropped non-string key still fails
/// overall (the crate never silently discards the problem), but the valid settings don't
/// contribute any additional entries.
#[test]
fn dropped_key_is_the_only_entry_when_everything_else_is_valid() {
    let report = condarc::parse("channels: [conda-forge]\nalways_yes: true\ntrue: 1\n")
        .expect_err("the bare-bool key must still be rejected");
    assert_eq!(report.entries().len(), 1);
    assert_eq!(report.entries()[0].kind, ErrorKind::TypeCoercion);
}

// ---- default_python: raw-value-to-string coercion still gets validated (FR-026) ----

#[test]
fn default_python_rejects_a_boolean_coerced_to_string() {
    for yaml in ["default_python: true\n", "default_python: false\n"] {
        let report = condarc::parse(yaml).expect_err("coerced bool must fail the shape check");
        assert_eq!(report.entries().len(), 1, "yaml={yaml:?}");
        assert_eq!(
            report.entries()[0].kind,
            ErrorKind::SemanticValidation,
            "yaml={yaml:?}"
        );
    }
}

#[test]
fn default_python_rejects_an_integer_coerced_to_string() {
    let report =
        condarc::parse("default_python: 0\n").expect_err("coerced int must fail the shape check");
    assert_eq!(report.entries().len(), 1);
    assert_eq!(report.entries()[0].kind, ErrorKind::SemanticValidation);
    assert_eq!(
        report.entries()[0].input,
        condarc::InputRepr::Str {
            value: "0".to_string()
        }
    );
}

#[test]
fn default_python_accepts_explicit_null_and_the_none_string_with_no_error() {
    assert!(condarc::parse("default_python: null\n").is_ok());
    assert!(condarc::parse("default_python: none\n").is_ok());
    assert!(condarc::parse("default_python: \"\"\n").is_ok());
}

// ---- Determinism / ordering (research R7, Constitution IX) ----

/// Entry order follows the fixed `CATALOG` declaration order, then alias-collision, then
/// cross-field — never the input document's own key order (research R7). Reordering the *same*
/// problems in the source document must not change the resulting entry order.
#[test]
fn entry_order_is_independent_of_document_key_order() {
    let forward = "channel_alias: no-scheme-here\nremote_max_retries: nan\nalways_copy: banana\n";
    let backward = "always_copy: banana\nremote_max_retries: nan\nchannel_alias: no-scheme-here\n";

    let report_forward = condarc::parse(forward).expect_err("must be rejected");
    let report_backward = condarc::parse(backward).expect_err("must be rejected");

    let settings_forward: Vec<String> = report_forward
        .entries()
        .iter()
        .map(|e| match &e.location {
            Location::Setting { setting } => setting.clone(),
            other => panic!("unexpected location {other:?}"),
        })
        .collect();
    let settings_backward: Vec<String> = report_backward
        .entries()
        .iter()
        .map(|e| match &e.location {
            Location::Setting { setting } => setting.clone(),
            other => panic!("unexpected location {other:?}"),
        })
        .collect();

    assert_eq!(settings_forward, settings_backward);
    // `channel_alias` precedes `remote_max_retries` in `docs/condarc_research.md` §4's catalog
    // order (§4.1 Channel Configuration before §4.3 Network Configuration), which precedes
    // `always_copy` (§4.5 Package Linking) -- catalog declaration order, not document order.
    assert_eq!(
        settings_forward,
        vec![
            "channel_alias".to_string(),
            "remote_max_retries".to_string(),
            "always_copy".to_string(),
        ]
    );
}

/// Per-field errors are ordered before alias-collision entries, which are ordered before
/// cross-field entries (research R7).
#[test]
fn per_field_errors_precede_alias_collision_which_precedes_cross_field() {
    let yaml = "\
always_copy: true\n\
always_softlink: true\n\
always_yes: true\n\
yes: true\n\
channel_alias: no-scheme-here\n";

    let report = condarc::parse(yaml).expect_err("must be rejected");
    let kinds: Vec<ErrorKind> = report.entries().iter().map(|e| e.kind).collect();

    let semantic_idx = kinds
        .iter()
        .position(|k| *k == ErrorKind::SemanticValidation)
        .expect("channel_alias semantic_validation entry must be present");
    let alias_idx = kinds
        .iter()
        .position(|k| *k == ErrorKind::AliasCollision)
        .expect("always_yes/yes alias_collision entry must be present");
    let cross_idx = kinds
        .iter()
        .position(|k| *k == ErrorKind::CrossField)
        .expect("always_copy/always_softlink cross_field entry must be present");

    assert!(
        semantic_idx < alias_idx,
        "per-field entries must precede alias-collision entries"
    );
    assert!(
        alias_idx < cross_idx,
        "alias-collision entries must precede cross-field entries"
    );
}

// ---- A "kitchen sink" document exercising every error kind at once ----

/// A single document combining a YAML-syntax-adjacent-but-still-valid structure with a type
/// coercion error, a semantic validation error, an alias collision, and both cross-field rules,
/// all in one parse — every one of them must be present, and none must suppress another
/// (FR-030/FR-031).
#[test]
fn every_accumulable_error_kind_can_appear_together_in_one_report() {
    let yaml = "\
1: dropped-root-key\n\
channel_alias: no-scheme-here\n\
remote_max_retries: nan\n\
always_yes: true\n\
yes: true\n\
always_copy: true\n\
always_softlink: true\n\
client_ssl_cert_key: /some/key\n";

    let report = condarc::parse(yaml).expect_err("must be rejected");
    let kinds: Vec<ErrorKind> = report.entries().iter().map(|e| e.kind).collect();

    assert!(kinds.contains(&ErrorKind::TypeCoercion), "{kinds:?}");
    assert!(kinds.contains(&ErrorKind::SemanticValidation), "{kinds:?}");
    assert!(kinds.contains(&ErrorKind::AliasCollision), "{kinds:?}");
    assert_eq!(
        kinds
            .iter()
            .filter(|k| **k == ErrorKind::CrossField)
            .count(),
        2,
        "expected both cross-field rules to fire: {kinds:?}"
    );
    // dropped root key + remote_max_retries = 2 type_coercion entries.
    assert_eq!(
        kinds
            .iter()
            .filter(|k| **k == ErrorKind::TypeCoercion)
            .count(),
        2,
        "{kinds:?}"
    );
    assert_eq!(kinds.len(), 6, "{kinds:?}");
}
