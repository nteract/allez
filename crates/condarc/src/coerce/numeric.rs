//! `Int`/`Float` coercion, the A1 `i64`/`f64` range check, A4 ASCII-only-digit rule, and
//! `local_repodata_ttl`'s `BoolOrInt` narrow vocabulary. See spec.md FR-018/FR-019/FR-020 and
//! docs/condarc_research.md §2.1, §8 items 6/11/14.

use super::{CoercionError, input_repr};
use crate::model::BoolOrInt;
use crate::parse::RawValue;

/// Strip PEP-515 single-underscore digit-group separators (underscore must sit strictly between
/// two ASCII digits; leading/trailing/doubled underscores are rejected) or return `None` if any
/// underscore is misplaced.
fn strip_pep515_underscores(s: &str) -> Option<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(chars.len());
    for (i, c) in chars.iter().enumerate() {
        if *c == '_' {
            let prev_digit = i > 0 && chars[i - 1].is_ascii_digit();
            let next_digit = i + 1 < chars.len() && chars[i + 1].is_ascii_digit();
            if !(prev_digit && next_digit) {
                return None;
            }
        } else {
            out.push(*c);
        }
    }
    Some(out)
}

/// A4: reject any non-ASCII character up front — Rust's numeric parsers are ASCII-only and this
/// crate deliberately does not carry a Unicode `Nd`->value table (docs/condarc_research.md A4).
fn is_ascii_only(s: &str) -> bool {
    s.is_ascii()
}

/// `int()`-equivalent parse of an underscore-stripped, ASCII-only numeral string: optional sign,
/// ASCII digits only, no decimal point/exponent (FR-018 explicitly rejects those for integers).
fn parse_int_literal(cleaned: &str) -> Option<i64> {
    let (sign, digits) = match cleaned.strip_prefix('-') {
        Some(rest) => (-1i64, rest),
        None => (1i64, cleaned.strip_prefix('+').unwrap_or(cleaned)),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse::<i64>().ok().map(|v| v * sign)
}

/// `Int` — integer settings (FR-018): JSON booleans and numbers (floats truncated toward zero),
/// and integer-looking strings (optional sign, PEP-515 underscores, leading zeros),
/// whitespace-trimmed. The A1 `i64` range check rejects magnitudes outside `i64`.
pub(crate) fn coerce_int(value: &RawValue) -> Result<i64, CoercionError> {
    match value {
        RawValue::Bool(b) => Ok(if *b { 1 } else { 0 }),
        RawValue::Int(i) => Ok(*i),
        // int(float) truncates toward zero (FR-018) -- Rust's `as i64` cast does the same for
        // in-range values; out-of-range (incl. non-finite) values are rejected per A1.
        RawValue::Float(f) => {
            if !f.is_finite()
                || *f >= 9_223_372_036_854_775_808.0
                || *f < -9_223_372_036_854_775_808.0
            {
                Err(CoercionError::simple(
                    format!("the numeric value {f} is out of range for a 64-bit integer"),
                    input_repr(value),
                ))
            } else {
                Ok(*f as i64)
            }
        }
        RawValue::Str(s) => {
            let trimmed = s.trim();
            if !is_ascii_only(trimmed) {
                return Err(CoercionError::simple(
                    "non-ASCII digits are not supported".to_string(),
                    input_repr(value),
                ));
            }
            let cleaned = strip_pep515_underscores(trimmed).ok_or_else(|| {
                CoercionError::simple(
                    format!("{s:?} is not a valid integer literal"),
                    input_repr(value),
                )
            })?;
            parse_int_literal(&cleaned).ok_or_else(|| {
                CoercionError::simple(
                    format!("the over-range or malformed numeral {s:?} cannot be represented as a 64-bit integer"),
                    input_repr(value),
                )
            })
        }
        RawValue::Null | RawValue::Seq(_) | RawValue::Map(_) => Err(CoercionError::simple(
            "expected an integer".to_string(),
            input_repr(value),
        )),
    }
}

/// `Float` — float settings (FR-019): everything `Int` accepts, plus decimal, scientific
/// notation, and `nan`/`inf`/`infinity` strings (case-insensitive, optionally signed).
pub(crate) fn coerce_float(value: &RawValue) -> Result<f64, CoercionError> {
    match value {
        RawValue::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        RawValue::Int(i) => Ok(*i as f64),
        RawValue::Float(f) => Ok(*f),
        RawValue::Str(s) => {
            let trimmed = s.trim();
            if !is_ascii_only(trimmed) {
                return Err(CoercionError::simple(
                    "non-ASCII digits are not supported".to_string(),
                    input_repr(value),
                ));
            }
            let cleaned = strip_pep515_underscores(trimmed).ok_or_else(|| {
                CoercionError::simple(
                    format!("{s:?} is not a valid float literal"),
                    input_repr(value),
                )
            })?;
            parse_float_literal(&cleaned).ok_or_else(|| {
                CoercionError::simple(
                    format!("{s:?} is not a valid float literal"),
                    input_repr(value),
                )
            })
        }
        RawValue::Null | RawValue::Seq(_) | RawValue::Map(_) => Err(CoercionError::simple(
            "expected a float".to_string(),
            input_repr(value),
        )),
    }
}

/// `float()`-equivalent parse: decimal/scientific notation, or a case-insensitive, optionally
/// signed `nan`/`inf`/`infinity` token. Rust's own `f64::from_str` already accepts all of these
/// (including case-insensitive `inf`/`infinity`/`nan`), so this is mostly a thin, ASCII-checked
/// wrapper -- kept as its own function so the non-decimal-base rejection (`"0x1a"` etc., FR-018)
/// stays alongside its int counterpart's reasoning.
fn parse_float_literal(cleaned: &str) -> Option<f64> {
    if cleaned.is_empty() {
        return None;
    }
    cleaned.parse::<f64>().ok()
}

/// `local_repodata_ttl`'s `(bool, int)` narrower boolish vocabulary (FR-020,
/// docs/condarc_research.md §8 items 6/11): `typify_str_no_hint`'s hand-rolled regex table, a
/// *strict subset* of `boolify()`'s own `BOOLISH_TRUE`/`BOOLISH_FALSE` — no single-letter `y`/`n`,
/// no `non`/`none`/`null`/`~`/empty-string tokens, no non-decimal-base literals.
pub(crate) fn coerce_bool_or_int(value: &RawValue) -> Result<BoolOrInt, CoercionError> {
    match value {
        RawValue::Bool(b) => return Ok(BoolOrInt::Bool(*b)),
        RawValue::Int(i) => return Ok(BoolOrInt::Int(*i)),
        RawValue::Float(f) => {
            if f.is_finite() && *f >= i64::MIN as f64 && *f < 9_223_372_036_854_775_808.0 {
                return Ok(BoolOrInt::Int(*f as i64));
            }
            return Err(CoercionError::simple(
                format!("the numeric value {f} is out of range for local_repodata_ttl"),
                input_repr(value),
            ));
        }
        RawValue::Str(_) => {}
        RawValue::Null | RawValue::Seq(_) | RawValue::Map(_) => {
            return Err(CoercionError::simple(
                "expected a boolean or integer".to_string(),
                input_repr(value),
            ));
        }
    }

    let RawValue::Str(raw) = value else {
        unreachable!("only the Str arm falls through to here")
    };
    let trimmed = raw.trim();
    let lower = trimmed.to_lowercase();

    // `BOOLEAN_TRUE = r'^true$|^yes$|^on$'`, `BOOLEAN_FALSE = r'^false$|^no$|^off$'`
    // (case-insensitive; narrower than boolify()'s own tables -- no "y"/"n"/"non"/"none"/"").
    if matches!(lower.as_str(), "true" | "yes" | "on") {
        return Ok(BoolOrInt::Bool(true));
    }
    if matches!(lower.as_str(), "false" | "no" | "off") {
        return Ok(BoolOrInt::Bool(false));
    }

    // Otherwise, fall back to plain integer parsing (still ASCII/PEP-515 only; no hex/oct/bin).
    if is_ascii_only(trimmed)
        && let Some(cleaned) = strip_pep515_underscores(trimmed)
        && let Some(i) = parse_int_literal(&cleaned)
    {
        return Ok(BoolOrInt::Int(i));
    }

    Err(CoercionError::simple(
        format!(
            "{raw:?} is not a valid local_repodata_ttl value (expected true/yes/on, false/no/off, or an integer)"
        ),
        input_repr(value),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> RawValue {
        RawValue::Str(text.to_string())
    }

    // ---- Int (FR-018) ----

    #[test]
    fn int_accepts_native_int_and_bool() {
        assert_eq!(coerce_int(&RawValue::Int(7)), Ok(7));
        assert_eq!(coerce_int(&RawValue::Int(-42)), Ok(-42));
        assert_eq!(coerce_int(&RawValue::Bool(true)), Ok(1));
        assert_eq!(coerce_int(&RawValue::Bool(false)), Ok(0));
    }

    #[test]
    fn int_truncates_float_toward_zero() {
        assert_eq!(coerce_int(&RawValue::Float(3.7)), Ok(3));
        assert_eq!(coerce_int(&RawValue::Float(-3.7)), Ok(-3));
    }

    #[test]
    fn int_accepts_signed_and_leading_zero_strings() {
        assert_eq!(coerce_int(&s("42")), Ok(42));
        assert_eq!(coerce_int(&s("-42")), Ok(-42));
        assert_eq!(coerce_int(&s("+42")), Ok(42));
        assert_eq!(coerce_int(&s("007")), Ok(7));
    }

    #[test]
    fn int_accepts_pep515_underscore_digit_groups() {
        assert_eq!(coerce_int(&s("1_000")), Ok(1000));
    }

    #[test]
    fn int_trims_whitespace() {
        assert_eq!(coerce_int(&s("  42  ")), Ok(42));
    }

    #[test]
    fn int_rejects_decimals_and_scientific_notation() {
        assert!(coerce_int(&s("3.5")).is_err());
        assert!(coerce_int(&s("1e3")).is_err());
    }

    #[test]
    fn int_rejects_nan_inf_and_empty_string() {
        assert!(coerce_int(&s("nan")).is_err());
        assert!(coerce_int(&s("inf")).is_err());
        assert!(coerce_int(&s("")).is_err());
    }

    #[test]
    fn int_rejects_non_decimal_base_literals() {
        assert!(coerce_int(&s("0x1A")).is_err());
        assert!(coerce_int(&s("0o17")).is_err());
        assert!(coerce_int(&s("0b101")).is_err());
    }

    #[test]
    fn int_rejects_malformed_underscores() {
        assert!(coerce_int(&s("_1000")).is_err());
        assert!(coerce_int(&s("1000_")).is_err());
        assert!(coerce_int(&s("1__000")).is_err());
    }

    // ---- Float (FR-019) ----

    #[test]
    fn float_accepts_everything_int_accepts() {
        assert_eq!(coerce_float(&RawValue::Int(7)), Ok(7.0));
        assert_eq!(coerce_float(&RawValue::Bool(true)), Ok(1.0));
        assert_eq!(coerce_float(&s("1_000")), Ok(1000.0));
    }

    #[test]
    fn float_accepts_decimal_and_scientific_notation() {
        assert_eq!(coerce_float(&s("3.5")), Ok(3.5));
        assert_eq!(coerce_float(&s("1e3")), Ok(1000.0));
        assert_eq!(coerce_float(&s("1e-3")), Ok(0.001));
    }

    #[test]
    fn float_accepts_nan_inf_infinity_case_insensitive_and_signed() {
        assert!(coerce_float(&s("nan")).unwrap().is_nan());
        assert!(coerce_float(&s("NaN")).unwrap().is_nan());
        assert_eq!(coerce_float(&s("inf")), Ok(f64::INFINITY));
        assert_eq!(coerce_float(&s("-inf")), Ok(f64::NEG_INFINITY));
        assert_eq!(coerce_float(&s("Infinity")), Ok(f64::INFINITY));
    }

    #[test]
    fn float_rejects_non_decimal_base_literals() {
        assert!(coerce_float(&s("0x1A")).is_err());
    }

    // ---- BoolOrInt / local_repodata_ttl (FR-020) ----

    #[test]
    fn bool_or_int_accepts_narrow_boolish_vocabulary() {
        assert_eq!(coerce_bool_or_int(&s("true")), Ok(BoolOrInt::Bool(true)));
        assert_eq!(coerce_bool_or_int(&s("YES")), Ok(BoolOrInt::Bool(true)));
        assert_eq!(coerce_bool_or_int(&s("on")), Ok(BoolOrInt::Bool(true)));
        assert_eq!(coerce_bool_or_int(&s("false")), Ok(BoolOrInt::Bool(false)));
        assert_eq!(coerce_bool_or_int(&s("NO")), Ok(BoolOrInt::Bool(false)));
        assert_eq!(coerce_bool_or_int(&s("off")), Ok(BoolOrInt::Bool(false)));
    }

    #[test]
    fn bool_or_int_accepts_native_bool_and_int() {
        assert_eq!(
            coerce_bool_or_int(&RawValue::Bool(true)),
            Ok(BoolOrInt::Bool(true))
        );
        assert_eq!(coerce_bool_or_int(&RawValue::Int(5)), Ok(BoolOrInt::Int(5)));
    }

    #[test]
    fn bool_or_int_accepts_integer_strings() {
        assert_eq!(coerce_bool_or_int(&s("5")), Ok(BoolOrInt::Int(5)));
        assert_eq!(coerce_bool_or_int(&s("-3")), Ok(BoolOrInt::Int(-3)));
    }

    #[test]
    fn bool_or_int_rejects_narrower_tokens_valid_elsewhere() {
        // "y"/"n"/"non"/"none"/"" are valid boolish tokens for other keys, but not here
        // (docs/condarc_research.md §8 item 6).
        for token in ["y", "n", "non", "none", ""] {
            assert!(coerce_bool_or_int(&s(token)).is_err(), "token={token}");
        }
    }

    #[test]
    fn bool_or_int_rejects_hex_literal_strings() {
        assert!(coerce_bool_or_int(&s("0x1A")).is_err());
    }
}
