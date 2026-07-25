//! YAML entry point, single-document/root-shape gate, `yaml_rust2::Yaml` -> `RawValue` lowering,
//! and the known/unknown key split. See data-model.md §1 and §6.

use indexmap::IndexMap;
use yaml_rust2::Yaml;

/// The internal, owned mirror of `yaml_rust2::Yaml`'s resolved-scalar-type tree (data-model.md
/// §1). Exists so no other module depends on `yaml-rust2`'s types directly (research R1: a
/// parser swap is then a single-module change). Records the resolved scalar *kind*
/// (bool/int/float/null/string), not a deserialization into [`crate::model::Config`] — that
/// happens later, in `catalog.rs` + `coerce/`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RawValue {
    /// `Yaml::Null` (bare `~`/`null`/empty), or `Yaml::BadValue` (never actually produced by a
    /// successful parse, but total coverage costs nothing).
    Null,
    /// `Yaml::Boolean`.
    Bool(bool),
    /// `Yaml::Integer`.
    Int(i64),
    /// `Yaml::Real`, parsed once here (never re-parsed downstream). A *bare* numeral too large
    /// for `i64` (so `yaml-rust2`'s own integer parse fails) but still parseable as `f64` also
    /// arrives as `Yaml::Real` and lowers here — `f64::parse` succeeds (possibly as `inf`) for
    /// essentially any all-digit string, so this is the fallback a bare over-range numeral
    /// actually takes, not [`RawValue::Str`]. The A1 range check that rejects such a value for
    /// an `Int`-typed setting happens downstream, in `coerce/numeric.rs`.
    Float(f64),
    /// `Yaml::String` (quoted or plain; whitespace preserved). A *quoted* bignum numeral (per
    /// A1's conformance fixtures) lowers here, since quoting always yields `Yaml::String`
    /// regardless of magnitude; a *bare* over-range numeral instead lowers to [`RawValue::Float`]
    /// (see that variant's doc comment) and is range-checked downstream on the float side.
    Str(String),
    Seq(Vec<RawValue>),
    /// Insertion-ordered (`IndexMap`) -> deterministic `MultipleKeysError`/error-entry ordering.
    /// String keys only: a non-string `Yaml::Hash` key has no representable slot here by
    /// construction (data-model.md §1 "Out of scope") and is dropped by [`lower`]; the
    /// caller-facing `type_coercion` entry for that (FR-007b) is raised by the per-key dispatch
    /// loop, which has the enclosing location this function does not.
    Map(IndexMap<String, RawValue>),
}

/// Lower a single `yaml_rust2::Yaml` value into [`RawValue`] via one hand-written recursive
/// match (research R1). `yaml-rust2` resolves anchors/aliases and merge keys before this point,
/// so whatever they expand to arrives here as an ordinary value; nothing below needs to
/// understand them (data-model.md §1).
pub(crate) fn lower(yaml: &Yaml) -> RawValue {
    match yaml {
        Yaml::Null | Yaml::BadValue | Yaml::Alias(_) => RawValue::Null,
        Yaml::Boolean(b) => RawValue::Bool(*b),
        Yaml::Integer(i) => RawValue::Int(*i),
        Yaml::Real(text) => match yaml.as_f64() {
            Some(f) => RawValue::Float(f),
            // yaml-rust2 only ever classifies a scalar as `Real` once its own scanner has
            // confirmed it parses as a float; this arm exists purely so `lower` stays total.
            None => RawValue::Str(text.clone()),
        },
        Yaml::String(s) => RawValue::Str(s.clone()),
        Yaml::Array(items) => RawValue::Seq(items.iter().map(lower).collect()),
        Yaml::Hash(hash) => {
            let mut map = IndexMap::with_capacity(hash.len());
            for (key, value) in hash {
                if let Yaml::String(key) = key {
                    map.insert(key.clone(), lower(value));
                }
                // A non-`Yaml::String` key is intentionally dropped here (FR-007b); see the
                // `Map` variant's doc comment above.
            }
            RawValue::Map(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yaml_rust2::YamlLoader;

    fn load_one(yaml_text: &str) -> Yaml {
        let mut docs = YamlLoader::load_from_str(yaml_text).expect("valid YAML for this test");
        assert_eq!(docs.len(), 1, "test fixture must be exactly one document");
        docs.remove(0)
    }

    #[test]
    fn lowers_null_variants() {
        assert_eq!(lower(&load_one("~")), RawValue::Null);
        assert_eq!(lower(&load_one("null")), RawValue::Null);
        assert_eq!(lower(&Yaml::BadValue), RawValue::Null);
        assert_eq!(lower(&Yaml::Alias(0)), RawValue::Null);
    }

    #[test]
    fn lowers_bool() {
        assert_eq!(lower(&load_one("true")), RawValue::Bool(true));
        assert_eq!(lower(&load_one("false")), RawValue::Bool(false));
    }

    #[test]
    fn lowers_int() {
        assert_eq!(lower(&load_one("7")), RawValue::Int(7));
        assert_eq!(lower(&load_one("-42")), RawValue::Int(-42));
    }

    #[test]
    fn lowers_float() {
        assert_eq!(lower(&load_one("1.5")), RawValue::Float(1.5));
        assert_eq!(lower(&load_one(".inf")), RawValue::Float(f64::INFINITY));
        assert!(matches!(lower(&load_one(".nan")), RawValue::Float(f) if f.is_nan()));
    }

    #[test]
    fn lowers_string_quoted_and_bare() {
        assert_eq!(lower(&load_one("foo")), RawValue::Str("foo".to_string()));
        assert_eq!(
            lower(&load_one("\"true\"")),
            RawValue::Str("true".to_string())
        );
        assert_eq!(lower(&load_one("\"7\"")), RawValue::Str("7".to_string()));
    }

    #[test]
    fn bare_over_range_integer_numeral_falls_back_to_real_float() {
        // yaml-rust2 itself fails to parse this as `Yaml::Integer` (exceeds i64), but a bare
        // all-digit numeral still parses as `f64` (Rust's `f64::parse` succeeds, possibly as
        // `inf`, for essentially any digit string), so yaml-rust2 resolves it as `Yaml::Real`,
        // not `Yaml::String`. `lower()` therefore maps it to `RawValue::Float`; the A1 range
        // check that rejects it for an `Int`-typed setting is `coerce/numeric.rs`'s job, applied
        // to this float (or to the *quoted* string form the real conformance fixtures use —
        // see `lowers_string_quoted_and_bare` — not to this bare-numeral case).
        let bignum = "99999999999999999999999999999999";
        let yaml = load_one(bignum);
        assert!(
            matches!(yaml, Yaml::Real(_)),
            "expected yaml-rust2 to resolve this as Real"
        );
        assert_eq!(lower(&yaml), RawValue::Float(1e32));
    }

    #[test]
    fn quoted_bignum_numeral_stays_a_string_regardless_of_magnitude() {
        // The real conformance corpus's bignum fixtures quote the numeral (e.g.
        // `remote_max_retries: "-999...999"`), which always resolves to `Yaml::String`
        // regardless of how large the digit string is — this is the shape the A1 range check in
        // `coerce/numeric.rs` actually operates on for those fixtures.
        let bignum = "\"-99999999999999999999999999999999999999999999999999\"";
        let yaml = load_one(bignum);
        assert!(matches!(yaml, Yaml::String(_)));
        assert_eq!(
            lower(&yaml),
            RawValue::Str("-99999999999999999999999999999999999999999999999999".to_string())
        );
    }

    #[test]
    fn lowers_sequence_recursively() {
        let yaml = load_one("[1, true, foo]");
        assert_eq!(
            lower(&yaml),
            RawValue::Seq(vec![
                RawValue::Int(1),
                RawValue::Bool(true),
                RawValue::Str("foo".to_string()),
            ])
        );
    }

    #[test]
    fn lowers_map_recursively_preserving_insertion_order() {
        let yaml = load_one("b: 2\na: 1\nc: 3");
        let RawValue::Map(map) = lower(&yaml) else {
            panic!("expected a Map")
        };
        assert_eq!(
            map.into_iter().collect::<Vec<_>>(),
            vec![
                ("b".to_string(), RawValue::Int(2)),
                ("a".to_string(), RawValue::Int(1)),
                ("c".to_string(), RawValue::Int(3)),
            ]
        );
    }

    #[test]
    fn drops_non_string_mapping_keys() {
        let yaml = load_one("1: x\nfoo: bar");
        let RawValue::Map(map) = lower(&yaml) else {
            panic!("expected a Map")
        };
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("foo"), Some(&RawValue::Str("bar".to_string())));
    }

    #[test]
    fn lowers_empty_root_to_null() {
        // Zero documents (empty input) is handled by the caller (FR-005); `lower` itself only
        // ever receives one already-selected `Yaml` value.
        assert_eq!(lower(&Yaml::Null), RawValue::Null);
    }
}
