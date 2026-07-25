//! The shared coercion engine, one module per `ValueKind` family. Each module implements both
//! the accept-path (User Story 1) and reject-path (User Story 2) of the same coercion functions.

pub mod boolish;
pub mod enums;
pub mod numeric;
pub mod sequences;
pub mod strings;

use crate::error::{InputRepr, PathSegment};
use crate::parse::RawValue;

/// A coercion failure, carrying enough information for the per-key dispatch loop (`parse.rs`) to
/// build a full [`crate::error::ErrorEntry`] once it knows the enclosing setting name. `path` is
/// empty for a directly-invalid setting value, and non-empty for a problem found inside a nested
/// sequence/map element (e.g. one `channel_settings[2]` entry) — see data-model.md §7.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CoercionError {
    pub(crate) message: String,
    pub(crate) input: InputRepr,
    pub(crate) path: Vec<PathSegment>,
}

impl CoercionError {
    /// A setting-level error with no nested path.
    pub(crate) fn simple(message: impl Into<String>, input: InputRepr) -> Self {
        Self {
            message: message.into(),
            input,
            path: Vec::new(),
        }
    }

    /// Prefix a nested-element error (raised deeper in a sequence/map coercer) with one more path
    /// segment, so the outermost caller only has to know its own position, not the whole chain.
    pub(crate) fn nest(mut self, segment: PathSegment) -> Self {
        self.path.insert(0, segment);
        self
    }
}

/// Render a [`RawValue`] as the error report's [`InputRepr`] (data-model.md §7): the offending
/// input value, or a description of it, for an `ErrorEntry`. Non-finite floats are never encoded
/// as `InputRepr::Float` (`serde_json` cannot serialize `NaN`/`±inf`) — they route to
/// `InputRepr::Raw` instead, matching the adapter's own non-finite encoding (FR-041).
pub(crate) fn input_repr(value: &RawValue) -> InputRepr {
    match value {
        RawValue::Null => InputRepr::Null,
        RawValue::Bool(b) => InputRepr::Bool { value: *b },
        RawValue::Int(i) => InputRepr::Int { value: *i },
        RawValue::Float(f) if f.is_nan() => InputRepr::Raw {
            value: "NaN".to_string(),
        },
        RawValue::Float(f) if f.is_infinite() => InputRepr::Raw {
            value: if *f > 0.0 { "Infinity" } else { "-Infinity" }.to_string(),
        },
        RawValue::Float(f) => InputRepr::Float { value: *f },
        RawValue::Str(s) => InputRepr::Str { value: s.clone() },
        RawValue::Seq(_) => InputRepr::Seq,
        RawValue::Map(_) => InputRepr::Map,
    }
}
