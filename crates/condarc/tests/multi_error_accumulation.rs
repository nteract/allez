//! Integration test for SC-005 / spec.md's US2 acceptance scenario 5: a document with several
//! independently-invalid settings produces one report entry per invalid setting — all of them,
//! not just the first encountered (FR-030/FR-031).

use condarc::ErrorKind;

/// A hand-built document with three independently-invalid settings — a `channel_alias` with no
/// URL scheme, an out-of-range `remote_max_retries`, and a non-boolish `always_copy` — must yield
/// exactly three accumulated entries, one per problem (SC-005).
#[test]
fn three_independently_invalid_settings_yield_exactly_three_entries() {
    let yaml = r#"
channel_alias: "no-scheme-here"
remote_max_retries: "99999999999999999999999999999999"
always_copy: "banana"
"#;

    let report = condarc::parse(yaml).expect_err("document has three independent problems");
    assert_eq!(
        report.entries().len(),
        3,
        "expected exactly one entry per invalid setting, got: {report:#?}"
    );

    let mut kinds_by_setting: Vec<(String, ErrorKind)> = report
        .entries()
        .iter()
        .map(|entry| {
            let setting = match &entry.location {
                condarc::Location::Setting { setting } => setting.clone(),
                other => panic!("expected a Setting location, got {other:?}"),
            };
            (setting, entry.kind)
        })
        .collect();
    kinds_by_setting.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(
        kinds_by_setting,
        vec![
            ("always_copy".to_string(), ErrorKind::TypeCoercion),
            ("channel_alias".to_string(), ErrorKind::SemanticValidation),
            ("remote_max_retries".to_string(), ErrorKind::TypeCoercion),
        ]
    );
}

/// A valid document alongside the same three problems still yields exactly three entries: the
/// valid settings must not perturb accumulation, and detecting one invalid setting must not
/// prevent evaluating the others (FR-031).
#[test]
fn valid_settings_alongside_invalid_ones_do_not_change_the_error_count() {
    let yaml = r#"
channels: [conda-forge, defaults]
channel_priority: strict
channel_alias: "no-scheme-here"
remote_max_retries: "99999999999999999999999999999999"
always_copy: "banana"
"#;

    let report = condarc::parse(yaml).expect_err("document still has three independent problems");
    assert_eq!(report.entries().len(), 3);
}
