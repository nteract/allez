//! `PlainString`/`NullableString` coercion — conda's `str()` conversion semantics. See spec.md
//! FR-014/FR-015 and docs/condarc_research.md §2.1, §2.4.

use super::CoercionError;
use crate::parse::RawValue;

/// conda's `str(x)` conversion for a non-string scalar (FR-014): `true`->`"True"`,
/// `false`->`"False"`, `7`->`"7"`, `null`->`"None"`. An already-`str` value is returned
/// unchanged, byte-for-byte — interior/edge whitespace is preserved (the `str`-typed special case
/// in `_typify_data_structure` that skips `typify()`'s unconditional `.strip()` entirely,
/// docs/condarc_research.md §2.1).
fn stringify(value: &RawValue) -> Result<String, CoercionError> {
    Ok(match value {
        RawValue::Str(s) => s.clone(),
        RawValue::Bool(true) => "True".to_string(),
        RawValue::Bool(false) => "False".to_string(),
        RawValue::Int(i) => i.to_string(),
        RawValue::Float(f) => python_repr_float(*f),
        RawValue::Null => "None".to_string(),
        RawValue::Seq(_) | RawValue::Map(_) => {
            return Err(CoercionError::simple(
                "expected a string (or a scalar convertible to one)".to_string(),
                super::input_repr(value),
            ));
        }
    })
}

/// conda's `str(float)` conversion is CPython's `repr(float)` (`PyOS_double_to_string(val, 'r',
/// ...)`, mode `'r'`). Exercised by the `plain_string_values_accept_float_*`
/// conformance fixtures, including the notation-switching battery documented below --
/// `.condarc` settings with a plain `str` element_type (`solver`, `console`, etc.) silently
/// stringify any non-string scalar, and a bare (unquoted) YAML numeral shaped like a float
/// lowers to exactly this path (`RawValue::Float`, `parse.rs`'s `lower()`).
///
/// CPython's `repr(float)` and Rust's `f64::to_string()`/`{:e}` both compute the *same*
/// shortest decimal digit string that round-trips to the exact same `f64` (a
/// well-specified, essentially unique target both a David Gay's-dtoa-style algorithm and
/// Rust's Grisu3/Dragon4-based formatter converge on) -- they differ only in
/// *presentation*: CPython's `format_float_short` switches from fixed-point to
/// `d[.ddd]e±NN` scientific notation once the value's decimal-point position (`decpt`,
/// computed below) falls outside `-4 < decpt <= 16`, whereas Rust's own `Display` for `f64`
/// never does that -- it always prints full fixed-point digits, however many that takes. So
/// this reformats Rust's own (correct) digits per CPython's threshold rather than
/// re-deriving them: no new dependency (e.g. `ryu`) is needed, since digit generation was
/// never the problem.
fn python_repr_float(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf" } else { "-inf" }.to_string();
    }

    // Work from the magnitude; -0.0's sign (`f.is_sign_negative()`, unlike `f < 0.0`, is true
    // for -0.0) is reattached at the end so it isn't lost when zero's digit string is
    // computed from `f.abs()`.
    let negative = f.is_sign_negative();
    let (digits, exponent) = shortest_digits_and_exponent(f.abs());
    // Decimal-point position: `digits` is `d1 d2 d3 ...` representing `d1.d2d3... * 10^exponent`,
    // so in fixed notation there are `exponent + 1` digits before the decimal point (CPython's
    // `decpt`, e.g. `1e16` -> digits "1", exponent 16, decpt 17: one digit, moved 17 places).
    let decpt = exponent + 1;

    let mut out = String::with_capacity(digits.len() + 8);
    if negative {
        out.push('-');
    }

    if decpt <= -4 || decpt > 16 {
        // Scientific: single leading digit, remaining digits (if any) after a '.', then a
        // *signed*, zero-padded-to-at-least-2-digits exponent -- `1e+16`, `1.5e+300`, `1e-05`.
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if exponent < 0 { '-' } else { '+' });
        let magnitude = exponent.unsigned_abs();
        if magnitude < 10 {
            out.push('0');
        }
        out.push_str(&magnitude.to_string());
    } else if decpt <= 0 {
        // Purely fractional with leading zeros after the point: digits="1", decpt=-3 -> "0.0001".
        out.push_str("0.");
        for _ in 0..(-decpt) {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        let decpt = decpt as usize;
        if decpt < digits.len() {
            // Decimal point lands inside the digit string: digits="123456", decpt=3 -> "123.456".
            out.push_str(&digits[..decpt]);
            out.push('.');
            out.push_str(&digits[decpt..]);
        } else {
            // Whole number: pad with trailing zeros out to decpt digits, then force a
            // `repr(float)`-style trailing ".0" (digits="1", decpt=16 -> "1000000000000000.0").
            out.push_str(&digits);
            for _ in 0..(decpt - digits.len()) {
                out.push('0');
            }
            out.push_str(".0");
        }
    }
    out
}

/// Significant digits (no sign, no decimal point, no exponent marker) and base-10 exponent of
/// a finite, non-negative `f64`, from Rust's own shortest-round-trip `{:e}` formatting --
/// e.g. `1.5e300` -> `("15", 300)`, `1e16` -> `("1", 16)`. `LowerExp` always emits exactly one
/// digit before an optional `.`-separated fractional part, then a bare `e` and a decimal
/// exponent with no `+` and no leading zeros -- see `python_repr_float`'s doc comment for why
/// these digits already match CPython's `repr(float)` digit-for-digit.
fn shortest_digits_and_exponent(f: f64) -> (String, i32) {
    debug_assert!(f.is_finite() && f.is_sign_positive());
    let formatted = format!("{f:e}");
    // Structurally unreachable for any `f64` (see this function's doc comment on `LowerExp`'s
    // guaranteed output shape) -- can't be driven by a unit test either, unlike every other
    // branch in this file; see `shortest_digits_and_exponent_extracts_digits_and_exponent`'s
    // module-level note in `tests` below.
    let Some((mantissa, exp_str)) = formatted.split_once('e') else {
        unreachable!("LowerExp always emits an 'e'")
    };
    // Same as above: structurally unreachable, not unit-testable.
    let Ok(exponent) = exp_str.parse::<i32>() else {
        unreachable!("LowerExp's exponent is always a plain base-10 integer")
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    (digits, exponent)
}

/// `PlainString` — `str` (FR-014).
pub(crate) fn coerce_plain_string(value: &RawValue) -> Result<String, CoercionError> {
    stringify(value)
}

/// `NullableString` — `(str, None)` (FR-015): like `PlainString`, except:
///
/// - conda's `LoadedParameter.typify()` unconditionally strips a *string* value's leading/
///   trailing whitespace before any further dispatch (`if isinstance(value, str): value =
///   value.strip()`) whenever `element_type` is a *tuple* (e.g. `(str, NoneType)`) rather than
///   the single class `str` -- the whitespace-*preserving* short-circuit in
///   `_typify_data_structure` (`PlainString`'s behavior above) only fires for a single concrete
///   `str` `element_type`, per `isinstance(type_hint, type)` (docs/condarc_research.md §2.1/§2.4
///   item 5). A non-string scalar (bool/int/null) is unaffected -- only an already-`str` value is
///   stripped.
/// - after stripping, the literal string `"none"` (case-insensitive) coerces to a null value,
///   and a bare YAML `null` is null outright.
///
/// Empirically confirmed via the real conda oracle: `client_ssl_cert_key: "  NoNe\t"` strips to
/// `"NoNe"` then null-folds (case-insensitively) to unset; `override_virtual_packages: {k: "   "}`
/// strips its value to `""` (conformance/condarc/valid/
/// {default_python,dict_of_strings,post_build_validation,ssl_verify_passthrough}_*whitespace*
/// fixtures) -- and that stripped, possibly-empty string is what downstream validators (e.g.
/// `default_python`'s range check, `post_build_validation`'s `client_ssl_cert`/
/// `client_ssl_cert_key` cross-field truthiness check) actually see.
pub(crate) fn coerce_nullable_string(value: &RawValue) -> Result<Option<String>, CoercionError> {
    if matches!(value, RawValue::Null) {
        return Ok(None);
    }
    let s = stringify(value)?;
    let s = match value {
        RawValue::Str(_) => s.trim().to_string(),
        _ => s,
    };
    if s.eq_ignore_ascii_case("none") {
        Ok(None)
    } else {
        Ok(Some(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> RawValue {
        RawValue::Str(text.to_string())
    }

    // -----------------------------------------------------------------
    // `shortest_digits_and_exponent` -- tested directly, independent of
    // `python_repr_float`'s own reformatting logic.
    // -----------------------------------------------------------------

    /// A representative spread of magnitudes/digit-counts, checked against exact
    /// (digits, exponent) tuples hand-derived from `f64`'s IEEE-754 value (cross-checked via
    /// `rustc`'s own `{:e}` output for each literal below). Covers: a single-digit mantissa, a
    /// multi-digit mantissa, a positive exponent, a negative exponent, zero, and one of the
    /// widest mantissas this crate's fixtures exercise (16 significant digits).
    #[test]
    fn shortest_digits_and_exponent_extracts_digits_and_exponent() {
        let cases: &[(f64, &str, i32)] = &[
            (0.0, "0", 0),
            (1.0, "1", 0),
            (3.5, "35", 0),
            (123.456, "123456", 2),
            (1e16, "1", 16),
            (1e-4, "1", -4),
            (1.5e300, "15", 300),
            (9999999999999998.0, "9999999999999998", 15),
        ];
        for (input, expected_digits, expected_exponent) in cases {
            let (digits, exponent) = shortest_digits_and_exponent(*input);
            assert_eq!(digits, *expected_digits, "digits of {input:?}");
            assert_eq!(exponent, *expected_exponent, "exponent of {input:?}");
        }
    }

    // The two `let-else { unreachable!() }` branches inside `shortest_digits_and_exponent`
    // (a `format!("{f:e}")` string missing an `'e'`, or an `exp_str` that fails to parse as
    // `i32`) are guaranteed unreachable by `LowerExp`'s documented output shape for every
    // finite, non-negative `f64` -- there's no `f64` input that can drive them, so unlike
    // every other branch in this file they cannot be exercised by a unit test. Nothing to add
    // here beyond this note; see the two `unreachable!()` call sites' own comments.

    // -----------------------------------------------------------------
    // `python_repr_float` -- one test per branch (and, where a branch has interesting
    // sub-cases -- an inner `if`, a loop that can run zero times, a sign, an exact boundary --
    // one assertion per sub-case). Every value below was cross-checked against a real conda
    // oracle (either directly, or as part of the `plain_string_values_accept_float_*`
    // conformance battery under conformance/condarc/{valid,expected}/) -- this module exists so
    // the same coverage also runs without a conda install available.
    // -----------------------------------------------------------------

    #[test]
    fn python_repr_float_nan_and_infinite() {
        // `is_nan()` true.
        assert_eq!(python_repr_float(f64::NAN), "nan");
        // `is_infinite()` true, both signs of its inner `if f > 0.0 { .. } else { .. }`.
        assert_eq!(python_repr_float(f64::INFINITY), "inf");
        assert_eq!(python_repr_float(f64::NEG_INFINITY), "-inf");
    }

    /// Scientific-notation branch (`decpt <= -4 || decpt > 16`), single-digit mantissa
    /// (`digits.len() > 1` false) -- plus both signs and both exponent-sign directions.
    #[test]
    fn python_repr_float_scientific_single_digit_mantissa() {
        // Large-side boundary, exactly at the switchover (decpt=17, the first decpt that's
        // `> 16`) and one step further out -- the reviewer's own original example.
        assert_eq!(python_repr_float(1e16), "1e+16");
        assert_eq!(python_repr_float(1e17), "1e+17");
        // Sign preserved across the switch.
        assert_eq!(python_repr_float(-1e16), "-1e+16");
        // Small-side boundary (decpt=-4, the first decpt that's `<= -4`), both signs --
        // magnitude=5 here also exercises the exponent zero-padding `if magnitude < 10`
        // (true branch).
        assert_eq!(python_repr_float(1e-5), "1e-05");
        assert_eq!(python_repr_float(-1e-5), "-1e-05");
        // `magnitude < 10` false branch, at the exact boundary (magnitude == 10, so the
        // 2-digit exponent needs no leading zero) -- only reachable on the small-exponent
        // side: a large-side scientific value always has `exponent >= 16` (magnitude already
        // >= 2 digits), so this branch's `false` case can only be demonstrated with a
        // negative exponent.
        assert_eq!(python_repr_float(1e-10), "1e-10");
    }

    /// Scientific-notation branch, multi-digit mantissa (`digits.len() > 1` true) -- all four
    /// sign combinations of {value sign} x {exponent sign}.
    #[test]
    fn python_repr_float_scientific_multi_digit_mantissa() {
        assert_eq!(python_repr_float(1.5e300), "1.5e+300"); // +value, +exponent
        assert_eq!(python_repr_float(-1.5e300), "-1.5e+300"); // -value, +exponent
        assert_eq!(python_repr_float(1.5e-10), "1.5e-10"); // +value, -exponent
        assert_eq!(python_repr_float(-1.5e-10), "-1.5e-10"); // -value, -exponent
    }

    /// Fixed-notation, `decpt <= 0` branch (purely fractional, leading zeros after the point):
    /// every loop-iteration count from 0 up to the branch's largest reachable value (3, since
    /// `decpt <= -4` would otherwise switch to scientific), plus a negative-sign case.
    #[test]
    fn python_repr_float_fixed_fractional_leading_zeros() {
        assert_eq!(python_repr_float(0.5), "0.5"); // decpt=0  -> 0 leading zeros
        assert_eq!(python_repr_float(0.05), "0.05"); // decpt=-1 -> 1 leading zero
        assert_eq!(python_repr_float(0.005), "0.005"); // decpt=-2 -> 2 leading zeros
        assert_eq!(python_repr_float(1e-4), "0.0001"); // decpt=-3 -> 3 leading zeros (small-side
        // boundary, the last decpt before switching to scientific -- pairs with
        // `python_repr_float_scientific_single_digit_mantissa`'s `1e-5`)
        assert_eq!(python_repr_float(-1e-4), "-0.0001"); // same boundary, negative sign
    }

    /// Fixed-notation, `decpt` in `1..=16` branch, `decpt < digits.len()` sub-branch (decimal
    /// point lands inside the digit string, no trailing-zero padding or forced ".0" needed).
    #[test]
    fn python_repr_float_fixed_decimal_point_inside_digits() {
        assert_eq!(python_repr_float(3.5), "3.5"); // decpt=1 < len=2
        assert_eq!(python_repr_float(-3.5), "-3.5"); // same, negative sign
        assert_eq!(python_repr_float(123.456), "123.456"); // decpt=3 < len=6
    }

    /// Fixed-notation, `decpt` in `1..=16` branch, `decpt >= digits.len()` sub-branch (whole
    /// number, forced trailing ".0"): both the zero-iteration edge of the padding loop
    /// (`decpt == digits.len()`) and the multi-iteration case (`decpt > digits.len()`), each
    /// with both signs. Zero itself is the smallest instance of the zero-iteration edge
    /// (`digits="0"`, `decpt == len == 1`).
    #[test]
    fn python_repr_float_fixed_whole_number() {
        // `decpt == digits.len()`: padding loop runs 0 times.
        assert_eq!(python_repr_float(123.0), "123.0");
        assert_eq!(python_repr_float(-123.0), "-123.0");
        assert_eq!(python_repr_float(0.0), "0.0");
        assert_eq!(python_repr_float(-0.0), "-0.0"); // `f.is_sign_negative()` true for -0.0,
        // unlike `f < 0.0` -- the whole reason that check (not a plain sign comparison) is
        // used before the digits/exponent are computed from `f.abs()`.
        // Largest-magnitude integral value still one step inside the large-side boundary
        // (decpt=16, digits.len()=16, still a zero-iteration case despite the large value).
        assert_eq!(python_repr_float(9999999999999998.0), "9999999999999998.0");
        // `decpt > digits.len()`: padding loop runs >= 1 time.
        assert_eq!(python_repr_float(100.0), "100.0"); // 2 zeros
        assert_eq!(python_repr_float(-100.0), "-100.0"); // same, negative sign
        // Large-side boundary, exactly one step inside the switchover (decpt=16, the last
        // decpt that's not `> 16` -- pairs with the scientific test's `1e16`); digits.len()=1,
        // so this needs the largest padding-loop iteration count (15) reachable in fixed
        // notation.
        assert_eq!(python_repr_float(1e15), "1000000000000000.0");
    }

    #[test]

    fn plain_string_preserves_whitespace_on_an_already_string_value() {
        assert_eq!(
            coerce_plain_string(&s("  padded  ")),
            Ok("  padded  ".to_string())
        );
    }

    #[test]
    fn plain_string_str_coerces_non_string_scalars() {
        assert_eq!(
            coerce_plain_string(&RawValue::Bool(true)),
            Ok("True".to_string())
        );
        assert_eq!(
            coerce_plain_string(&RawValue::Bool(false)),
            Ok("False".to_string())
        );
        assert_eq!(coerce_plain_string(&RawValue::Int(7)), Ok("7".to_string()));
        assert_eq!(coerce_plain_string(&RawValue::Null), Ok("None".to_string()));
    }

    #[test]
    fn plain_string_rejects_sequences_and_maps() {
        assert!(coerce_plain_string(&RawValue::Seq(vec![])).is_err());
    }

    #[test]
    fn nullable_string_null_literal_and_none_string_both_become_none() {
        assert_eq!(coerce_nullable_string(&RawValue::Null), Ok(None));
        assert_eq!(coerce_nullable_string(&s("none")), Ok(None));
        assert_eq!(coerce_nullable_string(&s("NONE")), Ok(None));
        assert_eq!(coerce_nullable_string(&s("None")), Ok(None));
    }

    #[test]
    fn nullable_string_strips_whitespace_before_the_none_check_and_the_return_value() {
        // Unlike `PlainString`, a `(str, None)` element type is a tuple, so conda's
        // whitespace-preserving `_typify_data_structure` short-circuit does not fire -- the
        // ordinary `typify()` unconditional `.strip()` applies instead.
        assert_eq!(coerce_nullable_string(&s("  NoNe\t")), Ok(None));
        assert_eq!(
            coerce_nullable_string(&s("  hello  ")),
            Ok(Some("hello".to_string()))
        );
        assert_eq!(coerce_nullable_string(&s("   ")), Ok(Some(String::new())));
        assert_eq!(coerce_nullable_string(&s("\t\n")), Ok(Some(String::new())));
    }

    #[test]
    fn nullable_string_coerces_non_string_scalars_without_stripping() {
        assert_eq!(
            coerce_nullable_string(&RawValue::Int(7)),
            Ok(Some("7".to_string()))
        );
        assert_eq!(
            coerce_nullable_string(&RawValue::Bool(false)),
            Ok(Some("False".to_string()))
        );
    }
}
