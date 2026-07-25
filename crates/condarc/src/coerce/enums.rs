//! Value-or-name enum lookup for `ChannelPriority`/`PathConflict`/`SafetyChecks`/`SatSolver`,
//! plus `channel_priority`'s bool/boolish shim. See spec.md FR-016/FR-017 and
//! docs/condarc_research.md §2.3.

use super::{CoercionError, input_repr};
use crate::catalog::EnumKind;
use crate::model::{ChannelPriority, PathConflict, SafetyChecks, SatSolver};
use crate::parse::RawValue;

/// One accepted spelling of an enum member: its lowercase documented value, and/or its exact
/// Python member-name spelling (docs/condarc_research.md §2.3) — both case-sensitive, whitespace-
/// trimmed, with no folding in between (`"Strict"` matches neither for `ChannelPriority`).
struct EnumMember<T> {
    value: &'static str,
    name: &'static str,
    result: T,
}

fn lookup<T: Copy>(trimmed: &str, members: &[EnumMember<T>]) -> Option<T> {
    members
        .iter()
        .find(|m| m.value == trimmed || m.name == trimmed)
        .map(|m| m.result)
}

const CHANNEL_PRIORITY_MEMBERS: &[EnumMember<ChannelPriority>] = &[
    EnumMember {
        value: "strict",
        name: "STRICT",
        result: ChannelPriority::Strict,
    },
    EnumMember {
        value: "flexible",
        name: "FLEXIBLE",
        result: ChannelPriority::Flexible,
    },
    EnumMember {
        value: "disabled",
        name: "DISABLED",
        result: ChannelPriority::Disabled,
    },
];

const PATH_CONFLICT_MEMBERS: &[EnumMember<PathConflict>] = &[
    EnumMember {
        value: "clobber",
        name: "clobber",
        result: PathConflict::Clobber,
    },
    EnumMember {
        value: "warn",
        name: "warn",
        result: PathConflict::Warn,
    },
    EnumMember {
        value: "prevent",
        name: "prevent",
        result: PathConflict::Prevent,
    },
];

const SAFETY_CHECKS_MEMBERS: &[EnumMember<SafetyChecks>] = &[
    EnumMember {
        value: "enabled",
        name: "enabled",
        result: SafetyChecks::Enabled,
    },
    EnumMember {
        value: "warn",
        name: "warn",
        result: SafetyChecks::Warn,
    },
    EnumMember {
        value: "disabled",
        name: "disabled",
        result: SafetyChecks::Disabled,
    },
];

const SAT_SOLVER_MEMBERS: &[EnumMember<SatSolver>] = &[
    EnumMember {
        value: "pycosat",
        name: "PYCOSAT",
        result: SatSolver::Pycosat,
    },
    EnumMember {
        value: "pycryptosat",
        name: "PYCRYPTOSAT",
        result: SatSolver::Pycryptosat,
    },
    EnumMember {
        value: "pysat",
        name: "PYSAT",
        result: SatSolver::Pysat,
    },
];

/// Value-or-name lookup for one of the four closed-vocabulary enum settings, dispatched by
/// [`EnumKind`] (FR-016). Only a `RawValue::Str` can ever match (a bare `RawValue::Bool` for
/// `channel_priority` is handled by [`coerce_channel_priority`]'s own shim, not here).
pub(crate) fn coerce_enum(value: &RawValue, kind: EnumKind) -> Result<EnumResult, CoercionError> {
    if kind == EnumKind::ChannelPriority {
        return coerce_channel_priority(value).map(EnumResult::ChannelPriority);
    }

    let RawValue::Str(raw) = value else {
        return Err(CoercionError::simple(
            "expected an enum value or member name string".to_string(),
            input_repr(value),
        ));
    };
    let trimmed = raw.trim();

    match kind {
        EnumKind::ChannelPriority => unreachable!("handled above"),
        EnumKind::PathConflict => {
            lookup(trimmed, PATH_CONFLICT_MEMBERS).map(EnumResult::PathConflict)
        }
        EnumKind::SafetyChecks => {
            lookup(trimmed, SAFETY_CHECKS_MEMBERS).map(EnumResult::SafetyChecks)
        }
        EnumKind::SatSolver => lookup(trimmed, SAT_SOLVER_MEMBERS).map(EnumResult::SatSolver),
    }
    .ok_or_else(|| {
        CoercionError::simple(
            format!("{raw:?} is not a recognized value or member name for this setting"),
            input_repr(value),
        )
    })
}

/// One coerced enum value, tagged by which of the four closed vocabularies it came from — a thin
/// wrapper so [`coerce_enum`] stays a single dispatch function per `catalog.rs`'s `EnumKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnumResult {
    ChannelPriority(ChannelPriority),
    PathConflict(PathConflict),
    SafetyChecks(SafetyChecks),
    SatSolver(SatSolver),
}

/// `channel_priority`'s historical bool-compat shim (FR-017, docs/condarc_research.md §2.3): a
/// JSON boolean, or a boolish string (`true`/`yes`/`on` -> flexible; `false`/`no`/`off` ->
/// disabled, any casing), is accepted in addition to the ordinary value-or-name lookup.
fn coerce_channel_priority(value: &RawValue) -> Result<ChannelPriority, CoercionError> {
    match value {
        RawValue::Bool(true) => return Ok(ChannelPriority::Flexible),
        RawValue::Bool(false) => return Ok(ChannelPriority::Disabled),
        RawValue::Str(raw) => {
            let trimmed = raw.trim();
            if let Some(result) = lookup(trimmed, CHANNEL_PRIORITY_MEMBERS) {
                return Ok(result);
            }
            let lower = trimmed.to_lowercase();
            if matches!(lower.as_str(), "true" | "yes" | "on") {
                return Ok(ChannelPriority::Flexible);
            }
            if matches!(lower.as_str(), "false" | "no" | "off") {
                return Ok(ChannelPriority::Disabled);
            }
        }
        _ => {}
    }
    Err(CoercionError::simple(
        "expected \"strict\"/\"flexible\"/\"disabled\", a member name, or a boolean/boolish value"
            .to_string(),
        input_repr(value),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> RawValue {
        RawValue::Str(text.to_string())
    }

    #[test]
    fn channel_priority_accepts_lowercase_value() {
        assert_eq!(
            coerce_channel_priority(&s("strict")),
            Ok(ChannelPriority::Strict)
        );
        assert_eq!(
            coerce_channel_priority(&s("flexible")),
            Ok(ChannelPriority::Flexible)
        );
        assert_eq!(
            coerce_channel_priority(&s("disabled")),
            Ok(ChannelPriority::Disabled)
        );
    }

    #[test]
    fn channel_priority_accepts_shouty_case_member_name() {
        assert_eq!(
            coerce_channel_priority(&s("STRICT")),
            Ok(ChannelPriority::Strict)
        );
    }

    #[test]
    fn channel_priority_rejects_other_casing() {
        assert!(coerce_channel_priority(&s("Strict")).is_err());
    }

    #[test]
    fn channel_priority_accepts_bool_and_boolish_shim() {
        assert_eq!(
            coerce_channel_priority(&RawValue::Bool(true)),
            Ok(ChannelPriority::Flexible)
        );
        assert_eq!(
            coerce_channel_priority(&RawValue::Bool(false)),
            Ok(ChannelPriority::Disabled)
        );
        assert_eq!(
            coerce_channel_priority(&s("yes")),
            Ok(ChannelPriority::Flexible)
        );
        assert_eq!(
            coerce_channel_priority(&s("OFF")),
            Ok(ChannelPriority::Disabled)
        );
    }

    #[test]
    fn path_conflict_accepts_value_and_rejects_bad_casing() {
        assert_eq!(
            coerce_enum(&s("clobber"), EnumKind::PathConflict),
            Ok(EnumResult::PathConflict(PathConflict::Clobber))
        );
        assert!(coerce_enum(&s("Clobber"), EnumKind::PathConflict).is_err());
    }

    #[test]
    fn safety_checks_name_and_value_are_identical_lowercase() {
        assert_eq!(
            coerce_enum(&s("enabled"), EnumKind::SafetyChecks),
            Ok(EnumResult::SafetyChecks(SafetyChecks::Enabled))
        );
        assert!(coerce_enum(&s("ENABLED"), EnumKind::SafetyChecks).is_err());
    }

    #[test]
    fn sat_solver_accepts_value_and_shouty_name() {
        assert_eq!(
            coerce_enum(&s("pycosat"), EnumKind::SatSolver),
            Ok(EnumResult::SatSolver(SatSolver::Pycosat))
        );
        assert_eq!(
            coerce_enum(&s("PYCRYPTOSAT"), EnumKind::SatSolver),
            Ok(EnumResult::SatSolver(SatSolver::Pycryptosat))
        );
    }

    #[test]
    fn sat_solver_rejects_wrong_casing() {
        assert!(coerce_enum(&s("Pycosat"), EnumKind::SatSolver).is_err());
        assert!(coerce_enum(&s("pycoSat"), EnumKind::SatSolver).is_err());
    }

    #[test]
    fn sat_solver_rejects_unrecognized_value() {
        assert!(coerce_enum(&s("minisat"), EnumKind::SatSolver).is_err());
    }

    #[test]
    fn enum_rejects_non_string_scalar() {
        assert!(coerce_enum(&RawValue::Int(1), EnumKind::PathConflict).is_err());
    }

    #[test]
    fn enum_rejects_non_string_scalar_for_every_kind() {
        // `coerce_enum`'s `RawValue::Str` gate applies uniformly regardless of `EnumKind` --
        // exercise all four so a future kind-specific special case can't silently reintroduce a
        // non-string acceptance path.
        for kind in [
            EnumKind::PathConflict,
            EnumKind::SafetyChecks,
            EnumKind::SatSolver,
        ] {
            assert!(
                coerce_enum(&RawValue::Int(1), kind).is_err(),
                "kind={kind:?}"
            );
            assert!(
                coerce_enum(&RawValue::Seq(vec![]), kind).is_err(),
                "kind={kind:?}"
            );
        }
        // `channel_priority` is the one enum with its own bool/boolish shim, so a bare `RawValue`
        // that isn't boolish either must still be rejected through the ordinary `coerce_enum`
        // dispatch path.
        assert!(coerce_enum(&RawValue::Seq(vec![]), EnumKind::ChannelPriority).is_err());
    }

    #[test]
    fn channel_priority_rejects_non_string_non_bool_scalar() {
        assert!(coerce_channel_priority(&RawValue::Int(1)).is_err());
        assert!(coerce_channel_priority(&RawValue::Null).is_err());
        assert!(coerce_channel_priority(&RawValue::Seq(vec![])).is_err());
    }

    #[test]
    fn channel_priority_rejects_unrecognized_string() {
        assert!(coerce_channel_priority(&s("maybe")).is_err());
    }
}
