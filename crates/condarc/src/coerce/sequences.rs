//! Sequence/map coercion for all six `ValueKind` shapes (`StringSeq`, `ListFieldsSeq`,
//! `StringMap`, `NullableStringMap`, `StringSeqMap`, `ChannelSettingsSeq`): raw-shape gate plus
//! element typify. See spec.md FR-021/FR-022/FR-023 and docs/condarc_research.md §2.4.

use std::collections::BTreeMap;

use super::{CoercionError, input_repr, strings};
use crate::error::PathSegment;
use crate::model::{ChannelSetting, ListField};
use crate::parse::RawValue;

/// The closed `CONDA_LIST_FIELDS` vocabulary (FR-022, docs/condarc_research.md §5.9) — matched
/// exactly, case-sensitively, with no trimming.
const LIST_FIELDS: &[(&str, ListField)] = &[
    ("arch", ListField::Arch),
    ("build", ListField::Build),
    ("build_number", ListField::BuildNumber),
    ("channel", ListField::Channel),
    ("channel_name", ListField::ChannelName),
    ("constrains", ListField::Constrains),
    ("depends", ListField::Depends),
    ("dist_str", ListField::DistStr),
    ("features", ListField::Features),
    ("fn", ListField::Fn),
    ("license", ListField::License),
    ("license_family", ListField::LicenseFamily),
    ("md5", ListField::Md5),
    ("name", ListField::Name),
    ("noarch", ListField::Noarch),
    ("package_type", ListField::PackageType),
    ("requested_spec", ListField::RequestedSpec),
    ("requested_specs", ListField::RequestedSpecs),
    ("sha256", ListField::Sha256),
    ("size", ListField::Size),
    ("subdir", ListField::Subdir),
    ("timestamp", ListField::Timestamp),
    ("track_features", ListField::TrackFeatures),
    ("url", ListField::Url),
    ("version", ListField::Version),
];

/// The raw-shape gate shared by every sequence-typed setting (FR-021): a bare scalar is rejected
/// (never auto-wrapped into a one-element list), a YAML `null` means "unset" (`Ok(None)`, handled
/// identically to the key being absent), and an empty mapping `{}` is accepted as an empty
/// sequence (conda's own `isiterable()` accepts any empty/non-empty `Mapping` or `Sequence`
/// alike, docs/condarc_research.md §2.4).
fn sequence_items(value: &RawValue) -> Result<Option<Vec<RawValue>>, CoercionError> {
    match value {
        RawValue::Null => Ok(None),
        RawValue::Seq(items) => Ok(Some(items.clone())),
        RawValue::Map(map) if map.is_empty() => Ok(Some(Vec::new())),
        _ => Err(CoercionError::simple(
            "expected a list (a bare scalar is not auto-wrapped)".to_string(),
            input_repr(value),
        )),
    }
}

/// The raw-shape gate shared by every map-typed setting (FR-023): require an object, or `null`
/// (unset). **Not** symmetric with [`sequence_items`]'s empty-`{}` allowance: an empty (or
/// non-empty) YAML *list* is unconditionally rejected here, matching real conda's
/// `MapParameter.load()`, which gates on `isinstance(value, Mapping)` -- a `list`/`tuple` is
/// never a `Mapping` instance, empty or not, so `custom_multichannels: []` (and every other
/// map-typed setting given a bare list) raises `InvalidTypeError` regardless of the list's
/// length. This *looks* like it should mirror `sequence_items`'s `isiterable()`-based leniency
/// (a `dict` is iterable, so an empty one is accepted in a list-typed slot) but doesn't, because
/// `MapParameter` and `SequenceParameter` use two different, non-symmetric raw-shape gates in
/// conda's own source (`common/configuration.py`) -- `isinstance(_, Mapping)` for the former,
/// `isiterable(_)` for the latter. Empirically confirmed via the real conda oracle: `channels:
/// {}` is accepted (`()`), but `custom_channels: []` raises `InvalidTypeError` ("has type tuple.
/// Valid types: frozendict") -- see `conformance/condarc/invalid/
/// {custom_multichannels,dict_of_strings}_values_reject_bare_list_empty.json` and
/// `channel_settings_reject_array_nested_empty_array_element.json` (an empty-list *element*
/// inside a `channel_settings` entry hits this same gate, since each element is itself
/// map-shaped).
fn map_entries(
    value: &RawValue,
) -> Result<Option<indexmap::IndexMap<String, RawValue>>, CoercionError> {
    match value {
        RawValue::Null => Ok(None),
        RawValue::Map(map) => Ok(Some(map.clone())),
        _ => Err(CoercionError::simple(
            "expected a mapping".to_string(),
            input_repr(value),
        )),
    }
}

/// Deduplicate `items`, keeping each value's *first* occurrence and dropping later exact
/// repeats. conda's own `SequenceLoadedParameter.merge()` (`common/configuration.py`) always
/// runs a sequence-typed setting's *matches* through `unique()` before returning them -- and it
/// does so unconditionally, even for a single-source `.condarc` with nothing to merge, so a
/// document's own internal duplicates are deduplicated too, not just duplicates arising *across*
/// multiple config sources. Shared by every `SequenceParameter`-shaped `ValueKind`
/// (`StringSeq`/`ListFieldsSeq`/`ChannelSettingsSeq`) -- empirically confirmed via the real conda
/// oracle for all three: `channels: [a, a]` reads back as `('a',)`; `list_fields: [name, name]`
/// as `('name',)`; `channel_settings: [{channel: x}, {channel: x}]` as a single-element tuple.
fn dedup_preserving_order<T: PartialEq>(items: Vec<T>) -> Vec<T> {
    let mut out: Vec<T> = Vec::with_capacity(items.len());
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// `StringSeq` — `SequenceParameter(str)` (FR-021).
pub(crate) fn coerce_string_seq(value: &RawValue) -> Result<Option<Vec<String>>, CoercionError> {
    let Some(items) = sequence_items(value)? else {
        return Ok(None);
    };
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let coerced = strings::coerce_plain_string(item)
            .map_err(|e| e.nest(PathSegment::Index { index: i }))?;
        out.push(coerced);
    }
    Ok(Some(dedup_preserving_order(out)))
}

/// `ListFieldsSeq` — `StringSeq` further restricted to the closed `CONDA_LIST_FIELDS` vocabulary
/// (FR-022): every element must be an exact, case-sensitive, untrimmed match.
pub(crate) fn coerce_list_fields_seq(
    value: &RawValue,
) -> Result<Option<Vec<ListField>>, CoercionError> {
    let Some(items) = sequence_items(value)? else {
        return Ok(None);
    };
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let RawValue::Str(s) = item else {
            return Err(CoercionError::simple(
                "expected a list_fields member name string".to_string(),
                input_repr(item),
            )
            .nest(PathSegment::Index { index: i }));
        };
        let member = LIST_FIELDS
            .iter()
            .find(|(name, _)| name == s)
            .map(|(_, member)| *member)
            .ok_or_else(|| {
                CoercionError::simple(
                    format!("{s:?} is not a recognized list_fields member"),
                    input_repr(item),
                )
                .nest(PathSegment::Index { index: i })
            })?;
        out.push(member);
    }
    Ok(Some(dedup_preserving_order(out)))
}

/// `StringMap` — `MapParameter(str)` (FR-023).
pub(crate) fn coerce_string_map(
    value: &RawValue,
) -> Result<Option<BTreeMap<String, String>>, CoercionError> {
    let Some(entries) = map_entries(value)? else {
        return Ok(None);
    };
    let mut out = BTreeMap::new();
    for (key, raw) in entries {
        let coerced = strings::coerce_plain_string(&raw)
            .map_err(|e| e.nest(PathSegment::Key { key: key.clone() }))?;
        out.insert(key, coerced);
    }
    Ok(Some(out))
}

/// `NullableStringMap` — `MapParameter((str, None))` (FR-023).
pub(crate) fn coerce_nullable_string_map(
    value: &RawValue,
) -> Result<Option<BTreeMap<String, Option<String>>>, CoercionError> {
    let Some(entries) = map_entries(value)? else {
        return Ok(None);
    };
    let mut out = BTreeMap::new();
    for (key, raw) in entries {
        let coerced = strings::coerce_nullable_string(&raw)
            .map_err(|e| e.nest(PathSegment::Key { key: key.clone() }))?;
        out.insert(key, coerced);
    }
    Ok(Some(out))
}

/// `StringSeqMap` — `MapParameter(SequenceParameter(str))` (`custom_multichannels` only,
/// FR-023). Each value is itself sequence-coerced; a `null` inner value has no "unset" concept to
/// propagate to (there is no absent-vs-present distinction for one entry inside a map), so it is
/// treated as an empty list.
pub(crate) fn coerce_string_seq_map(
    value: &RawValue,
) -> Result<Option<BTreeMap<String, Vec<String>>>, CoercionError> {
    let Some(entries) = map_entries(value)? else {
        return Ok(None);
    };
    let mut out = BTreeMap::new();
    for (key, raw) in entries {
        let coerced = coerce_string_seq(&raw)
            .map_err(|e| e.nest(PathSegment::Key { key: key.clone() }))?
            .unwrap_or_default();
        out.insert(key, coerced);
    }
    Ok(Some(out))
}

/// `ChannelSettingsSeq` — `SequenceParameter(MapParameter(str))` (`channel_settings` only,
/// FR-023). Each element must itself be a string-to-string map; a `null` element has no "unset"
/// concept to propagate to (there is no absent-vs-present distinction for one entry inside a
/// sequence, the same reasoning as `coerce_string_seq_map`'s inner-`null`-to-empty-list handling
/// above), so it is treated as an empty map -- matching real conda's `MapParameter.load()`,
/// which returns an empty `MapLoadedParameter` outright when `match.value(...)` is `None`
/// (confirmed via the oracle: `channel_settings: [null]` reads back as `({},)`, not a rejection;
/// see `conformance/condarc/valid/channel_settings_accept_array_element_null_treated_as_empty_map.json`
/// and its `array_size_2_*` siblings). The sequence as a whole is still deduplicated afterward
/// (`dedup_preserving_order`), so `[null, null]` collapses to a single `{}` entry.
pub(crate) fn coerce_channel_settings_seq(
    value: &RawValue,
) -> Result<Option<Vec<ChannelSetting>>, CoercionError> {
    let Some(items) = sequence_items(value)? else {
        return Ok(None);
    };
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let entry = coerce_string_map(item)
            .map_err(|e| e.nest(PathSegment::Index { index: i }))?
            .unwrap_or_default();
        out.push(ChannelSetting(entry));
    }
    Ok(Some(dedup_preserving_order(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn str_seq(items: &[&str]) -> RawValue {
        RawValue::Seq(items.iter().map(|s| RawValue::Str(s.to_string())).collect())
    }

    // ---- StringSeq (FR-021) ----

    #[test]
    fn string_seq_accepts_a_real_list() {
        assert_eq!(
            coerce_string_seq(&str_seq(&["conda-forge", "defaults"])),
            Ok(Some(vec![
                "conda-forge".to_string(),
                "defaults".to_string()
            ]))
        );
    }

    #[test]
    fn string_seq_accepts_empty_object_as_empty_sequence() {
        assert_eq!(
            coerce_string_seq(&RawValue::Map(indexmap::IndexMap::new())),
            Ok(Some(vec![]))
        );
    }

    #[test]
    fn string_seq_null_means_unset() {
        assert_eq!(coerce_string_seq(&RawValue::Null), Ok(None));
    }

    #[test]
    fn string_seq_rejects_bare_scalar() {
        assert!(coerce_string_seq(&RawValue::Str("just-a-string".to_string())).is_err());
    }

    #[test]
    fn string_seq_string_coerces_non_string_elements() {
        assert_eq!(
            coerce_string_seq(&RawValue::Seq(vec![RawValue::Int(7), RawValue::Bool(true)])),
            Ok(Some(vec!["7".to_string(), "True".to_string()]))
        );
    }

    #[test]
    fn string_seq_rejects_nested_arrays_as_elements() {
        assert!(coerce_string_seq(&RawValue::Seq(vec![RawValue::Seq(vec![])])).is_err());
    }

    // ---- ListFieldsSeq (FR-022) ----

    #[test]
    fn list_fields_accepts_closed_vocabulary_members() {
        assert_eq!(
            coerce_list_fields_seq(&str_seq(&["name", "version", "build", "channel_name"])),
            Ok(Some(vec![
                ListField::Name,
                ListField::Version,
                ListField::Build,
                ListField::ChannelName,
            ]))
        );
    }

    #[test]
    fn list_fields_rejects_out_of_vocabulary_member() {
        assert!(coerce_list_fields_seq(&str_seq(&["not_a_real_field"])).is_err());
    }

    #[test]
    fn list_fields_is_case_sensitive() {
        assert!(coerce_list_fields_seq(&str_seq(&["NAME"])).is_err());
    }

    // ---- StringMap / NullableStringMap (FR-023) ----

    #[test]
    fn string_map_accepts_object_and_string_coerces_values() {
        let mut map = indexmap::IndexMap::new();
        map.insert(
            "pkgs/pro".to_string(),
            RawValue::Str("https://x".to_string()),
        );
        let result = coerce_string_map(&RawValue::Map(map)).unwrap().unwrap();
        assert_eq!(result.get("pkgs/pro"), Some(&"https://x".to_string()));
    }

    #[test]
    fn nullable_string_map_accepts_explicit_null_values() {
        let mut map = indexmap::IndexMap::new();
        map.insert("http".to_string(), RawValue::Null);
        let result = coerce_nullable_string_map(&RawValue::Map(map))
            .unwrap()
            .unwrap();
        assert_eq!(result.get("http"), Some(&None));
    }

    #[test]
    fn string_map_null_means_unset() {
        assert_eq!(coerce_string_map(&RawValue::Null), Ok(None));
    }

    #[test]
    fn string_map_rejects_bare_scalar() {
        assert!(coerce_string_map(&RawValue::Str("just-a-string".to_string())).is_err());
        assert!(coerce_string_map(&RawValue::Int(7)).is_err());
    }

    #[test]
    fn string_map_rejects_a_nonempty_sequence() {
        assert!(coerce_string_map(&str_seq(&["a"])).is_err());
    }

    #[test]
    fn string_map_rejects_an_empty_sequence_not_symmetric_with_sequence_items() {
        // `MapParameter.load()` gates on `isinstance(value, Mapping)`, not `isiterable()` --
        // a bare list is never a mapping, empty or not (see `map_entries`'s doc comment).
        assert!(coerce_string_map(&RawValue::Seq(vec![])).is_err());
    }

    #[test]
    fn nullable_string_map_rejects_bare_scalar() {
        assert!(coerce_nullable_string_map(&RawValue::Str("just-a-string".to_string())).is_err());
    }

    // ---- StringSeqMap (custom_multichannels, FR-023) ----

    #[test]
    fn string_seq_map_coerces_each_value_as_a_sequence() {
        let mut map = indexmap::IndexMap::new();
        map.insert("defaults".to_string(), str_seq(&["main", "r"]));
        let result = coerce_string_seq_map(&RawValue::Map(map)).unwrap().unwrap();
        assert_eq!(
            result.get("defaults"),
            Some(&vec!["main".to_string(), "r".to_string()])
        );
    }

    #[test]
    fn string_seq_map_rejects_a_bare_scalar_root() {
        assert!(coerce_string_seq_map(&RawValue::Str("nope".to_string())).is_err());
    }

    #[test]
    fn string_seq_map_rejects_a_bare_scalar_value() {
        let mut map = indexmap::IndexMap::new();
        map.insert(
            "defaults".to_string(),
            RawValue::Str("not-a-list".to_string()),
        );
        assert!(coerce_string_seq_map(&RawValue::Map(map)).is_err());
    }

    // ---- ChannelSettingsSeq (FR-023) ----

    #[test]
    fn channel_settings_seq_coerces_each_element_as_a_string_map() {
        let mut entry = indexmap::IndexMap::new();
        entry.insert(
            "channel".to_string(),
            RawValue::Str("https://x".to_string()),
        );
        entry.insert("auth".to_string(), RawValue::Str("token".to_string()));
        let result = coerce_channel_settings_seq(&RawValue::Seq(vec![RawValue::Map(entry)]))
            .unwrap()
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.get("channel"), Some(&"https://x".to_string()));
    }

    #[test]
    fn channel_settings_seq_rejects_a_scalar_element() {
        assert!(
            coerce_channel_settings_seq(&RawValue::Seq(vec![RawValue::Str("x".to_string())]))
                .is_err()
        );
    }

    #[test]
    fn channel_settings_seq_treats_a_null_element_as_an_empty_map() {
        let result = coerce_channel_settings_seq(&RawValue::Seq(vec![RawValue::Null]))
            .unwrap()
            .unwrap();
        assert_eq!(result, vec![ChannelSetting(BTreeMap::new())]);
    }

    #[test]
    fn channel_settings_seq_dedupes_identical_elements_preserving_order() {
        let mut a = indexmap::IndexMap::new();
        a.insert("channel".to_string(), RawValue::Str("x".to_string()));
        let mut b = indexmap::IndexMap::new();
        b.insert("channel".to_string(), RawValue::Str("x".to_string()));
        let result = coerce_channel_settings_seq(&RawValue::Seq(vec![
            RawValue::Map(a),
            RawValue::Map(b),
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn channel_settings_seq_rejects_a_bare_scalar_root() {
        assert!(coerce_channel_settings_seq(&RawValue::Str("nope".to_string())).is_err());
    }
}
