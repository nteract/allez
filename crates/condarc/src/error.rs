//! The error model — `ValidationReport`, `ErrorEntry`, `Location`, `PathSegment`, `ErrorKind`,
//! `InputRepr` — matching `contracts/error-report.schema.json` exactly. See data-model.md §7.

/// The JSON-contract version this report serializes as. Independent of the crate's own SemVer
/// version (Constitution III); a breaking change to `contracts/error-report.schema.json` is a
/// MAJOR bump of this constant, not of the crate necessarily.
pub(crate) const SCHEMA_VERSION: &str = "1.0.0";

/// A private marker type whose only job is to always serialize as the constant
/// [`SCHEMA_VERSION`] string, so `ValidationReport::schema_version` can never be constructed or
/// overridden by a caller with a different value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct SchemaVersion;

fn serialize_schema_version<S>(_: &SchemaVersion, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(SCHEMA_VERSION)
}

/// Accumulates every independent problem found while parsing one
/// document (FR-030/031). Implements [`std::error::Error`] (usable with
/// `?`) and [`serde::Serialize`] (the versioned JSON contract in
/// `contracts/error-report.schema.json`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ValidationReport {
    /// Serialized as the constant `"1.0.0"` so a machine consumer can
    /// version-check the payload independently of the crate's own SemVer
    /// (schema `required: ["schema_version", "entries"]`). Not
    /// constructible or overridable by callers.
    #[serde(serialize_with = "serialize_schema_version")]
    schema_version: SchemaVersion,
    entries: Vec<ErrorEntry>,
}

impl ValidationReport {
    /// Build a report from its accumulated entries. Crate-private: callers never construct a
    /// `ValidationReport` directly, only receive one from `parse`/`parse_with_options`.
    pub(crate) fn new(entries: Vec<ErrorEntry>) -> Self {
        Self {
            schema_version: SchemaVersion,
            entries,
        }
    }

    /// Every accumulated problem, in the deterministic order documented
    /// in research R7 (never the input document's key order).
    pub fn entries(&self) -> &[ErrorEntry] {
        &self.entries
    }

    /// The JSON-contract version this report serializes as (`"1.0.0"`).
    pub fn schema_version(&self) -> &'static str {
        SCHEMA_VERSION
    }
}

impl std::fmt::Display for ValidationReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, entry) in self.entries.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{}: {} ({})", entry.location, entry.kind, entry.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationReport {}

/// One problem in the document (FR-033). Every field is individually
/// accessible and typed — never only a formatted message string — so an
/// agent can branch on `kind` and locate `location` without parsing text.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ErrorEntry {
    pub location: Location,
    pub kind: ErrorKind,
    pub message: String,
    pub input: InputRepr,
    /// Populated only for `AliasCollision`/`CrossField`: every setting
    /// name involved (FR-033). Always serialized (possibly as `[]`), since
    /// the schema requires the key.
    pub involved: Vec<String>,
}

/// Where the problem is (FR-033). Discriminated by `"type"` on the wire, matching
/// `error-report.schema.json`'s `location` `oneOf`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Location {
    Root,
    Setting {
        setting: String,
    },
    /// e.g. `channel_settings[2].auth`
    Nested {
        setting: String,
        path: Vec<PathSegment>,
    },
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Location::Root => write!(f, "<root>"),
            Location::Setting { setting } => write!(f, "{setting}"),
            Location::Nested { setting, path } => {
                write!(f, "{setting}")?;
                for segment in path {
                    match segment {
                        PathSegment::Index { index } => write!(f, "[{index}]")?,
                        PathSegment::Key { key } => write!(f, ".{key}")?,
                    }
                }
                Ok(())
            }
        }
    }
}

/// One step of a [`Location::Nested`] path — either a sequence index or a map key.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PathSegment {
    Index { index: usize },
    Key { key: String },
}

/// Stable, machine-readable problem category (FR-033). `serde` renames to
/// the exact strings in `contracts/error-report.schema.json`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Non-accumulable (FR-032a) — single-entry report.
    YamlSyntax,
    /// Non-accumulable (FR-032b) — single-entry report, also covers multi-document input
    /// (FR-007a).
    RootShape,
    /// Also covers a non-string mapping key (FR-007b).
    TypeCoercion,
    SemanticValidation,
    AliasCollision,
    CrossField,
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ErrorKind::YamlSyntax => "yaml_syntax",
            ErrorKind::RootShape => "root_shape",
            ErrorKind::TypeCoercion => "type_coercion",
            ErrorKind::SemanticValidation => "semantic_validation",
            ErrorKind::AliasCollision => "alias_collision",
            ErrorKind::CrossField => "cross_field",
        };
        write!(f, "{s}")
    }
}

/// The offending input value, or a description of it (FR-033). Discriminated by `"type"` on the
/// wire, matching `error-report.schema.json`'s `inputRepr` `oneOf`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputRepr {
    Bool {
        value: bool,
    },
    Int {
        value: i64,
    },
    /// Finite values only — see the note below.
    Float {
        value: f64,
    },
    Str {
        value: String,
    },
    Null,
    Seq,
    Map,
    /// A value with no faithful JSON encoding: an over-range numeral
    /// (A1), or a non-finite float (`"NaN"`/`"Infinity"`/`"-Infinity"`),
    /// recorded as its source text.
    Raw {
        value: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> ErrorEntry {
        ErrorEntry {
            location: Location::Setting {
                setting: "channel_alias".to_string(),
            },
            kind: ErrorKind::SemanticValidation,
            message: "'channel_alias' must have a URL scheme".to_string(),
            input: InputRepr::Str {
                value: "conda.anaconda.org".to_string(),
            },
            involved: vec![],
        }
    }

    #[test]
    fn schema_version_is_the_constant() {
        let report = ValidationReport::new(vec![sample_entry()]);
        assert_eq!(report.schema_version(), "1.0.0");
    }

    #[test]
    fn entries_returns_every_accumulated_problem_in_order() {
        let report = ValidationReport::new(vec![sample_entry(), sample_entry()]);
        assert_eq!(report.entries().len(), 2);
    }

    #[test]
    fn serializes_to_the_documented_json_contract_shape() {
        let report = ValidationReport::new(vec![sample_entry()]);
        let json = serde_json::to_value(&report).expect("serializable");
        assert_eq!(json["schema_version"], "1.0.0");
        assert_eq!(json["entries"][0]["location"]["type"], "setting");
        assert_eq!(json["entries"][0]["location"]["setting"], "channel_alias");
        assert_eq!(json["entries"][0]["kind"], "semantic_validation");
        assert_eq!(json["entries"][0]["input"]["type"], "str");
        assert_eq!(json["entries"][0]["input"]["value"], "conda.anaconda.org");
        assert_eq!(json["entries"][0]["involved"], serde_json::json!([]));
    }

    #[test]
    fn display_renders_one_readable_line_per_entry() {
        let report = ValidationReport::new(vec![sample_entry(), sample_entry()]);
        let rendered = report.to_string();
        assert_eq!(rendered.lines().count(), 2);
        assert!(rendered.contains("channel_alias"));
        assert!(rendered.contains("semantic_validation"));
    }

    #[test]
    fn implements_std_error() {
        let report = ValidationReport::new(vec![sample_entry()]);
        let _: &dyn std::error::Error = &report;
    }

    #[test]
    fn nested_location_display_matches_dotted_index_notation() {
        let loc = Location::Nested {
            setting: "channel_settings".to_string(),
            path: vec![
                PathSegment::Index { index: 2 },
                PathSegment::Key {
                    key: "auth".to_string(),
                },
            ],
        };
        assert_eq!(loc.to_string(), "channel_settings[2].auth");
    }
}
