use condarc::{ErrorKind, Location, PathSegment, parse};

#[test]
fn rejects_float_literals_that_overflow_to_infinity() {
    let report = parse("remote_connect_timeout_secs: \"1e400\"")
        .expect_err("an overflowing finite literal must be rejected");

    assert_eq!(report.entries().len(), 1);
    assert_eq!(report.entries()[0].kind, ErrorKind::TypeCoercion);
}

#[test]
fn accepts_the_minimum_signed_integer_literal() {
    let config = parse("remote_max_retries: \"-9223372036854775808\"")
        .expect("the minimum i64 value is in range");

    assert_eq!(config.remote_max_retries, Some(i64::MIN));
}

#[test]
fn reports_the_nested_path_to_a_non_string_mapping_key() {
    let report = parse("channel_settings:\n  - channel: x\n    1: bad")
        .expect_err("non-string mapping keys must be rejected");

    assert_eq!(report.entries().len(), 1);
    assert_eq!(
        report.entries()[0].location,
        Location::Nested {
            setting: "channel_settings".to_string(),
            path: vec![PathSegment::Index { index: 0 }],
        }
    );
}

#[test]
fn stringifies_floats_like_python_and_conda() {
    let config = parse("solver: 1e16").expect("a float scalar is string-coercible");

    assert_eq!(config.solver.as_deref(), Some("1e+16"));
}

#[test]
fn pads_small_float_exponents_like_python_and_conda() {
    let config = parse("solver: 1e-5").expect("a float scalar is string-coercible");

    assert_eq!(config.solver.as_deref(), Some("1e-05"));
}
