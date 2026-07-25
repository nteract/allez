//! `Bool`/`NullableBool` (and `ssl_verify`'s boolish branch) coercion — conda's `boolify`
//! semantics. See spec.md FR-012/FR-013/FR-024 and docs/condarc_research.md §2.2.

use super::{CoercionError, input_repr};
use crate::model::SslVerify;
use crate::parse::RawValue;

/// `conda/auxlib/type_coercion.py`'s `BOOLISH_TRUE` tuple, case-folded (docs/condarc_research.md
/// §2.2).
const BOOLISH_TRUE: [&str; 4] = ["true", "yes", "on", "y"];
/// `BOOLISH_FALSE`.
const BOOLISH_FALSE: [&str; 7] = ["false", "off", "n", "no", "non", "none", ""];
/// `NULL_STRINGS` — only consulted when `nullable` (FR-013).
const NULL_STRINGS: [&str; 4] = ["none", "~", "null", "\0"];

/// The outcome of one `boolify()` call, before the caller (a `ValueKind`-specific wrapper below)
/// narrows it to what that setting shape can actually represent.
enum Boolified {
    Bool(bool),
    /// Only produced when `nullable` is set.
    Null,
    /// Only produced when `return_string` is set and no other branch matched: the *original*,
    /// untouched string (not the lowercased/trimmed probe) passes through unchanged (`ssl_verify`
    /// only).
    Str(String),
}

/// Port of `conda/auxlib/type_coercion.py::boolify` (docs/condarc_research.md §2.2). `nullable`
/// enables the `NULL_STRINGS` branch (FR-013); `return_string` enables the final passthrough
/// instead of erroring on an unboolifiable string (`ssl_verify`'s `(str, bool)` shape, FR-024).
fn boolify(
    value: &RawValue,
    nullable: bool,
    return_string: bool,
) -> Result<Boolified, CoercionError> {
    // Step 1: `isinstance(value, BOOL_COERCEABLE_TYPES)` -> plain Python truthiness. `NoneType`
    // is deliberately *not* in that tuple, so a bare YAML null falls through to the string probe
    // below (it's stringified as `"None"` first, exactly like Python's `str(None)`).
    match value {
        RawValue::Bool(b) => return Ok(Boolified::Bool(*b)),
        RawValue::Int(i) => return Ok(Boolified::Bool(*i != 0)),
        RawValue::Float(f) => return Ok(Boolified::Bool(*f != 0.0)),
        RawValue::Seq(items) => return Ok(Boolified::Bool(!items.is_empty())),
        RawValue::Map(map) => return Ok(Boolified::Bool(!map.is_empty())),
        RawValue::Null | RawValue::Str(_) => {}
    }

    let original_str = match value {
        RawValue::Str(s) => Some(s.clone()),
        _ => None,
    };
    let stringified = original_str.clone().unwrap_or_else(|| "None".to_string());

    // Step 2's probe string: lowercase, whitespace-trimmed, first '.' deleted (not all dots —
    // see docs/condarc_research.md §2.2's three consequences of this single deletion).
    let val = stringified.trim().to_lowercase().replacen('.', "", 1);

    // `val.isnumeric()` -> `bool(float(val))`. Restricted to ASCII digits (this crate's A4
    // simplification extends naturally to this probe too; no committed fixture exercises a
    // non-ASCII digit here).
    if !val.is_empty()
        && val.chars().all(|c| c.is_ascii_digit())
        && let Ok(f) = val.parse::<f64>()
    {
        return Ok(Boolified::Bool(f != 0.0));
    }

    if BOOLISH_TRUE.contains(&val.as_str()) {
        return Ok(Boolified::Bool(true));
    }
    if nullable && NULL_STRINGS.contains(&val.as_str()) {
        return Ok(Boolified::Null);
    }
    if BOOLISH_FALSE.contains(&val.as_str()) {
        return Ok(Boolified::Bool(false));
    }
    if let Some(truthy) = parse_complex_truthy(&val) {
        return Ok(Boolified::Bool(truthy));
    }

    if return_string && let Some(s) = original_str {
        return Ok(Boolified::Str(s));
    }

    Err(CoercionError::simple(
        format!("the value {stringified:?} cannot be boolified"),
        input_repr(value),
    ))
}

/// `Bool` — plain, non-nullable `bool` (FR-012). Covers all Category A keys (`debug`,
/// `always_copy`, `override_channels_enabled`, ...).
pub(crate) fn coerce_bool(value: &RawValue) -> Result<bool, CoercionError> {
    match boolify(value, false, false)? {
        Boolified::Bool(b) => Ok(b),
        Boolified::Null | Boolified::Str(_) => {
            unreachable!("nullable=false, return_string=false never produce Null/Str")
        }
    }
}

/// `NullableBool` — `(bool, None)` (FR-013). Covers `always_yes`, `report_errors`,
/// `show_channel_urls`, `use_only_tar_bz2`.
pub(crate) fn coerce_nullable_bool(value: &RawValue) -> Result<Option<bool>, CoercionError> {
    match boolify(value, true, false)? {
        Boolified::Bool(b) => Ok(Some(b)),
        Boolified::Null => Ok(None),
        Boolified::Str(_) => unreachable!("return_string=false never produces Str"),
    }
}

/// `ssl_verify`'s `(str, bool)` shape (FR-024): boolean/boolish coercion first, then the literal
/// `"truststore"`, then anything else is an unverified certificate-path string. The opt-in
/// filesystem-existence check (`ParseOptions::ssl_verify_fs_check`) is applied later, in
/// `validate.rs` — this function is the side-effect-free default branch only.
pub(crate) fn coerce_ssl_verify(value: &RawValue) -> Result<SslVerify, CoercionError> {
    match boolify(value, false, true)? {
        Boolified::Bool(b) => Ok(SslVerify::Bool(b)),
        Boolified::Null => unreachable!("nullable=false never produces Null"),
        Boolified::Str(s) if s == "truststore" => Ok(SslVerify::Truststore),
        Boolified::Str(s) => Ok(SslVerify::Path(s)),
    }
}

/// Best-effort port of Python's `complex(val)` constructor, restricted to what `boolify()`'s step
/// 6 fallback actually needs: whether the parsed value is nonzero (`bool(complex(val))`), not the
/// value itself. Supports real/imaginary decimal forms with PEP-515 single-underscore digit
/// groups, optional sign, decimal point, and exponent — e.g. `"-1"`, `"1e10"`, `"1+2j"`, `"3j"`,
/// `"-4-5j"`.
fn parse_complex_truthy(val: &str) -> Option<bool> {
    if val.is_empty() {
        return None;
    }
    if let Some(f) = parse_pep515_f64(val) {
        return Some(f != 0.0);
    }
    let rest = val.strip_suffix('j')?;
    if rest.is_empty() || rest == "+" {
        return Some(true); // "j" / "+j" == 0+1j, nonzero
    }
    if rest == "-" {
        return Some(true); // "-j" == -1j, nonzero
    }
    if let Some(imag) = parse_pep515_f64(rest) {
        return Some(imag != 0.0);
    }
    // Combined real+imag form: split at the last +/- that isn't an exponent sign.
    let bytes = rest.as_bytes();
    for i in (1..bytes.len()).rev() {
        let c = bytes[i];
        if (c == b'+' || c == b'-') && !matches!(bytes[i - 1], b'e' | b'E') {
            let (real_part, imag_part) = (&rest[..i], &rest[i..]);
            let real = parse_pep515_f64(real_part);
            let imag = match imag_part {
                "+" => Some(1.0),
                "-" => Some(-1.0),
                other => parse_pep515_f64(other),
            };
            if let (Some(r), Some(im)) = (real, imag) {
                return Some(r != 0.0 || im != 0.0);
            }
            break;
        }
    }
    None
}

/// Strip PEP-515 single-underscore digit-group separators (underscore must sit strictly between
/// two ASCII digits) and parse the result as `f64`; `None` if an underscore is misplaced or the
/// result doesn't parse.
fn parse_pep515_f64(s: &str) -> Option<f64> {
    let chars: Vec<char> = s.chars().collect();
    let mut cleaned = String::with_capacity(chars.len());
    for (i, c) in chars.iter().enumerate() {
        if *c == '_' {
            let prev_digit = i > 0 && chars[i - 1].is_ascii_digit();
            let next_digit = i + 1 < chars.len() && chars[i + 1].is_ascii_digit();
            if !(prev_digit && next_digit) {
                return None;
            }
        } else {
            cleaned.push(*c);
        }
    }
    cleaned.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> RawValue {
        RawValue::Str(text.to_string())
    }

    // ---- Bool truth table (FR-012) ----

    #[test]
    fn bool_accepts_native_bool() {
        assert_eq!(coerce_bool(&RawValue::Bool(true)), Ok(true));
        assert_eq!(coerce_bool(&RawValue::Bool(false)), Ok(false));
    }

    #[test]
    fn bool_accepts_numbers_via_truthiness() {
        assert_eq!(coerce_bool(&RawValue::Int(42)), Ok(true));
        assert_eq!(coerce_bool(&RawValue::Int(0)), Ok(false));
        assert_eq!(coerce_bool(&RawValue::Int(-1)), Ok(true));
        assert_eq!(coerce_bool(&RawValue::Float(1.5)), Ok(true));
        assert_eq!(coerce_bool(&RawValue::Float(0.0)), Ok(false));
    }

    #[test]
    fn bool_accepts_collections_via_truthiness() {
        assert_eq!(coerce_bool(&RawValue::Seq(vec![])), Ok(false));
        assert_eq!(
            coerce_bool(&RawValue::Seq(vec![RawValue::Int(1)])),
            Ok(true)
        );
    }

    #[test]
    fn bool_accepts_boolish_true_strings_any_case() {
        for token in ["true", "TRUE", "True", "yes", "YES", "on", "ON", "y", "Y"] {
            assert_eq!(coerce_bool(&s(token)), Ok(true), "token={token}");
        }
    }

    #[test]
    fn bool_accepts_boolish_false_strings_any_case() {
        for token in [
            "false", "FALSE", "off", "OFF", "n", "N", "no", "NO", "non", "none", "",
        ] {
            assert_eq!(coerce_bool(&s(token)), Ok(false), "token={token}");
        }
    }

    #[test]
    fn bool_trims_whitespace_before_matching() {
        assert_eq!(coerce_bool(&s(" true ")), Ok(true));
        assert_eq!(coerce_bool(&s("\t\nyes\n\t")), Ok(true));
    }

    #[test]
    fn bool_accepts_decimal_string_via_dot_deletion_probe() {
        assert_eq!(coerce_bool(&s("1.0")), Ok(true));
        assert_eq!(coerce_bool(&s("0.0")), Ok(false));
        assert_eq!(coerce_bool(&s("3.5")), Ok(true));
    }

    #[test]
    fn bool_accepts_numeric_strings_via_complex_fallback() {
        assert_eq!(coerce_bool(&s("-1")), Ok(true));
        assert_eq!(coerce_bool(&s("0")), Ok(false));
        assert_eq!(coerce_bool(&s("1_000")), Ok(true));
    }

    #[test]
    fn bool_accepts_complex_number_string() {
        assert_eq!(coerce_bool(&s("1+2j")), Ok(true));
    }

    #[test]
    fn bool_null_literal_stringifies_to_none_and_becomes_false() {
        // str(None).lower() == "none", a BOOLISH_FALSE token -- resolved via step 5 for a
        // non-nullable field (docs/condarc_research.md §8 item 13).
        assert_eq!(coerce_bool(&RawValue::Null), Ok(false));
    }

    #[test]
    fn bool_rejects_non_boolish_word() {
        assert!(coerce_bool(&s("banana")).is_err());
    }

    // ---- NullableBool (FR-013) ----

    #[test]
    fn nullable_bool_accepts_true_false_and_null_tokens() {
        assert_eq!(coerce_nullable_bool(&s("true")), Ok(Some(true)));
        assert_eq!(coerce_nullable_bool(&s("false")), Ok(Some(false)));
        assert_eq!(coerce_nullable_bool(&RawValue::Null), Ok(None));
        assert_eq!(coerce_nullable_bool(&s("null")), Ok(None));
        assert_eq!(coerce_nullable_bool(&s("~")), Ok(None));
        assert_eq!(coerce_nullable_bool(&s("\0")), Ok(None));
    }

    #[test]
    fn nullable_bool_rejects_non_boolish_word() {
        assert!(coerce_nullable_bool(&s("banana")).is_err());
    }

    // ---- ssl_verify (str, bool) default branch (FR-024) ----

    #[test]
    fn ssl_verify_accepts_bool_and_boolish() {
        assert_eq!(
            coerce_ssl_verify(&RawValue::Bool(true)),
            Ok(SslVerify::Bool(true))
        );
        assert_eq!(coerce_ssl_verify(&s("yes")), Ok(SslVerify::Bool(true)));
        assert_eq!(coerce_ssl_verify(&s("no")), Ok(SslVerify::Bool(false)));
    }

    #[test]
    fn ssl_verify_accepts_truststore_literal_exactly() {
        assert_eq!(
            coerce_ssl_verify(&s("truststore")),
            Ok(SslVerify::Truststore)
        );
    }

    #[test]
    fn ssl_verify_treats_wrong_case_truststore_as_a_path() {
        assert_eq!(
            coerce_ssl_verify(&s("Truststore")),
            Ok(SslVerify::Path("Truststore".to_string()))
        );
    }

    #[test]
    fn ssl_verify_treats_arbitrary_non_boolish_string_as_unverified_path() {
        assert_eq!(
            coerce_ssl_verify(&s("/etc/ssl/certs/ca.pem")),
            Ok(SslVerify::Path("/etc/ssl/certs/ca.pem".to_string()))
        );
        // never errors on a non-boolish string -- return_string=true passthrough (FR-024).
        assert_eq!(
            coerce_ssl_verify(&s("banana")),
            Ok(SslVerify::Path("banana".to_string()))
        );
    }

    #[test]
    fn ssl_verify_dot_reduces_to_false_via_boolify_probe_not_a_path() {
        // ".".replace(".", "", 1) == "" -> a BOOLISH_FALSE token, so this loads as `false`
        // and never reaches the path branch at all (docs/condarc_research.md A3 note).
        assert_eq!(coerce_ssl_verify(&s(".")), Ok(SslVerify::Bool(false)));
    }
}
