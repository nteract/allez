use condarc::{ErrorKind, parse};

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
