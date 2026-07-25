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

/// A reasonable `repr(float)`-shaped rendering for the handful of scalar-to-string conversions
/// this crate needs (no `.condarc` fixture actually string-coerces a float-typed value into a
/// string-typed setting; this exists for totality, not to satisfy a specific accept fixture).
fn python_repr_float(f: f64) -> String {
    if f.is_nan() {
        "nan".to_string()
    } else if f.is_infinite() {
        if f > 0.0 { "inf" } else { "-inf" }.to_string()
    } else if f == f.trunc() && f.abs() < 1e16 {
        format!("{f:.1}")
    } else {
        f.to_string()
    }
}

/// `PlainString` — `str` (FR-014).
pub(crate) fn coerce_plain_string(value: &RawValue) -> Result<String, CoercionError> {
    stringify(value)
}

/// `NullableString` — `(str, None)` (FR-015): like `PlainString`, except the literal string
/// `"none"` (case-insensitive) coerces to a null value, and a bare YAML `null` is null outright.
pub(crate) fn coerce_nullable_string(value: &RawValue) -> Result<Option<String>, CoercionError> {
    if matches!(value, RawValue::Null) {
        return Ok(None);
    }
    let s = stringify(value)?;
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
    fn nullable_string_behaves_like_plain_string_otherwise() {
        assert_eq!(
            coerce_nullable_string(&s("hello")),
            Ok(Some("hello".to_string()))
        );
        assert_eq!(
            coerce_nullable_string(&RawValue::Int(7)),
            Ok(Some("7".to_string()))
        );
    }
}
