//! `PackageSpec` plus the additive/supersede `effective_packages()` merge.

use std::collections::BTreeSet;
use std::fmt;

use rattler_conda_types::{MatchSpec, ParseStrictness};

use super::channels::redact_channel_url;

/// Where a [`PackageSpec`] came from, so a spec that turns out to be
/// unresolvable can be reported against the input the caller actually
/// controls. A `create_default_packages` entry is never validated on the
/// way in (FR-002), so its provenance and position are the only things
/// that make the eventual failure actionable without echoing a value this
/// crate never parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackageOrigin {
    /// Named by the caller for this one invocation.
    Explicit,
    /// Resolved from the caller's own `.condarc` `create_default_packages`,
    /// at this zero-based position in that list.
    CreateDefaultPackages {
        /// Zero-based position in the resolved `create_default_packages` list.
        index: usize,
    },
}

/// An opaque conda match-spec string plus the origin it came from.
/// Syntactically validated when built by [`PackageSpec::parse`], and
/// deliberately unvalidated when resolved from a caller's own
/// `create_default_packages` setting.
#[derive(Clone, PartialEq, Eq)]
pub struct PackageSpec {
    spec: String,
    origin: PackageOrigin,
}

impl fmt::Debug for PackageSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PackageSpec")
            .field(&redact_channel_url(&self.spec))
            .finish()
    }
}

/// A syntactically invalid match-spec string rejected by [`PackageSpec::parse`].
#[derive(Clone, PartialEq, Eq)]
pub struct InvalidPackageSpec {
    /// The rejected input string.
    pub input: String,
    /// A human-readable explanation of why parsing failed.
    pub reason: String,
    /// Zero-based position of the rejected entry in the list it came from,
    /// when it came from one, so a caller can identify it without echoing its
    /// raw value. `None` for a standalone [`PackageSpec::parse`] call, which
    /// has no list position to report.
    pub index: Option<usize>,
}

impl fmt::Debug for InvalidPackageSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InvalidPackageSpec")
            .field("input", &redact_channel_url(&self.input))
            .field("reason", &redact_channel_url(&self.reason))
            .finish()
    }
}

impl fmt::Display for InvalidPackageSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid package spec `{}`: {}",
            redact_channel_url(&self.input),
            redact_channel_url(&self.reason)
        )
    }
}

impl std::error::Error for InvalidPackageSpec {}

impl PackageSpec {
    /// Parses a raw match-spec string, rejecting anything
    /// `rattler_conda_types::MatchSpec` cannot syntactically represent.
    pub fn parse(input: &str) -> Result<Self, InvalidPackageSpec> {
        MatchSpec::from_str(input, ParseStrictness::Strict)
            .map(|_| Self {
                spec: input.to_string(),
                origin: PackageOrigin::Explicit,
            })
            .map_err(|error| InvalidPackageSpec {
                input: input.to_string(),
                reason: error.to_string(),
                index: None,
            })
    }

    /// The opaque match-spec string this `PackageSpec` wraps.
    pub(crate) fn as_str(&self) -> &str {
        &self.spec
    }

    /// Where this spec came from.
    pub(crate) fn origin(&self) -> PackageOrigin {
        self.origin
    }

    /// Wraps `input` with no `MatchSpec` validation at all — unlike
    /// [`PackageSpec::parse`], this cannot fail. Used only for a
    /// `create_default_packages` entry, which FR-002 forbids rejecting.
    /// `index` is that entry's own position in the resolved list, so a
    /// later failure can identify it without echoing its raw value.
    pub(crate) fn from_resolved_default(input: String, index: usize) -> Self {
        Self {
            spec: input,
            origin: PackageOrigin::CreateDefaultPackages { index },
        }
    }

    /// A label safe to render in an error message or a tracing event.
    ///
    /// A `create_default_packages` entry is arbitrary text this crate never
    /// validated, so it is identified only by its one-based position and never
    /// echoed. Parsing it would prove it contains no URL structure, but not
    /// that it contains no secret: a credential can consist entirely of
    /// characters a package name allows, so a parsed name is not a safe
    /// projection of an untrusted value either.
    ///
    /// A command-line package is the caller's own argument, already visible to
    /// whoever invoked the process, so its parsed name is reported. Its raw
    /// text is still withheld, since only the strictly-parsed name is used.
    pub(crate) fn safe_label(&self) -> String {
        match self.origin {
            PackageOrigin::CreateDefaultPackages { index } => {
                format!("<create_default_packages entry {}>", index + 1)
            }
            PackageOrigin::Explicit => self
                .bare_name()
                .unwrap_or_else(|| "<unparseable command-line package>".to_string()),
        }
    }

    /// The same spec attributed to `origin`, so a caller's own request
    /// position, not a value's construction site, decides its provenance.
    pub(crate) fn with_origin(&self, origin: PackageOrigin) -> Self {
        Self {
            spec: self.spec.clone(),
            origin,
        }
    }

    /// This spec's bare package name, via a `MatchSpec` reparse that
    /// strips any version/build/channel qualifier and normalizes case.
    /// `None` when the reparse yields no exact name — an entry with no
    /// bare name to compare by, which always survives the merge.
    pub(crate) fn bare_name(&self) -> Option<String> {
        MatchSpec::from_str(self.as_str(), ParseStrictness::Strict)
            .ok()
            .and_then(|parsed| {
                parsed
                    .name
                    .as_exact()
                    .map(|name| name.as_normalized().to_string())
            })
    }
}

/// Parses zero or more raw, caller-supplied per-invocation package
/// strings into validated specs. An empty input list is not a special
/// case: it simply parses to an empty `Vec`.
pub(crate) fn parse_explicit_packages(
    packages: Vec<String>,
) -> Result<Vec<PackageSpec>, InvalidPackageSpec> {
    packages
        .into_iter()
        .enumerate()
        .map(|(index, package)| {
            PackageSpec::parse(&package).map_err(|invalid| InvalidPackageSpec {
                index: Some(index),
                ..invalid
            })
        })
        .collect()
}

/// Restamps every spec with the origin its position in the caller's request
/// implies, so provenance follows which list a value was placed in rather than
/// how it happened to be constructed. A public caller can only build
/// `Explicit` specs, so without this a spec passed as a default would be
/// reported against the command line.
pub(crate) fn attribute(packages: &[PackageSpec], origin: PackageOrigin) -> Vec<PackageSpec> {
    packages
        .iter()
        .map(|package| package.with_origin(origin))
        .collect()
}

/// [`attribute`] for the resolved default set, numbering each entry by its own
/// position so a failure can name it without echoing its value.
pub(crate) fn attribute_defaults(packages: &[PackageSpec]) -> Vec<PackageSpec> {
    packages
        .iter()
        .enumerate()
        .map(|(index, package)| package.with_origin(PackageOrigin::CreateDefaultPackages { index }))
        .collect()
}

/// Resolves the Effective Package Set per FR-003/FR-004: every `defaults`
/// entry survives unless its bare name matches an `explicit` entry's,
/// then every `explicit` entry is appended. Entries sharing a bare name
/// within the same input list are never deduplicated against each other.
pub(crate) fn effective_packages(
    explicit: &[PackageSpec],
    defaults: &[PackageSpec],
) -> Vec<PackageSpec> {
    let superseded: BTreeSet<String> = explicit.iter().filter_map(PackageSpec::bare_name).collect();
    let mut effective: Vec<PackageSpec> = defaults
        .iter()
        .filter(|default| {
            default
                .bare_name()
                .is_none_or(|name| !superseded.contains(&name))
        })
        .cloned()
        .collect();
    effective.extend(explicit.iter().cloned());
    effective
}

#[cfg(test)]
mod tests {
    use super::{PackageOrigin, PackageSpec, effective_packages, parse_explicit_packages};

    fn spec(input: &str) -> PackageSpec {
        PackageSpec::parse(input).unwrap()
    }

    #[test]
    fn parse_marks_a_spec_as_explicitly_named() {
        assert_eq!(spec("numpy").origin(), PackageOrigin::Explicit);
    }

    #[test]
    fn from_resolved_default_marks_a_spec_as_condarc_sourced() {
        // Given
        let resolved_default = PackageSpec::from_resolved_default("numpy".to_string(), 0);

        // When
        let origin = resolved_default.origin();

        // Then
        assert_eq!(origin, PackageOrigin::CreateDefaultPackages { index: 0 });
    }

    #[test]
    fn parse_explicit_packages_empty_input_returns_empty() {
        assert_eq!(parse_explicit_packages(Vec::new()).unwrap(), Vec::new());
    }

    #[test]
    fn effective_packages_no_explicit_packages_returns_defaults_exactly() {
        // Given
        let defaults = vec![spec("numpy=1.2"), spec("scipy")];

        // When
        let effective = effective_packages(&[], &defaults);

        // Then
        assert_eq!(effective, defaults);
    }

    #[test]
    fn effective_packages_empty_explicit_and_empty_defaults_returns_empty() {
        assert_eq!(effective_packages(&[], &[]), Vec::new());
    }

    #[test]
    fn bare_name_of_an_unparseable_resolved_default_spec_returns_none() {
        // Given
        let resolved_default = PackageSpec::from_resolved_default(String::new(), 0);

        // When
        let bare_name = resolved_default.bare_name();

        // Then
        assert_eq!(bare_name, None);
    }

    #[test]
    fn effective_packages_disjoint_bare_names_is_additive() {
        // Given
        let defaults = vec![spec("numpy")];
        let explicit = vec![spec("pandas")];

        // When
        let effective = effective_packages(&explicit, &defaults);

        // Then
        assert_eq!(effective, vec![spec("numpy"), spec("pandas")]);
    }

    #[test]
    fn effective_packages_matching_bare_name_supersedes_default_entry() {
        // Given
        let defaults = vec![spec("numpy=1.2"), spec("scipy")];
        let explicit = vec![spec("numpy")];

        // When
        let effective = effective_packages(&explicit, &defaults);

        // Then
        assert_eq!(effective, vec![spec("scipy"), spec("numpy")]);
    }

    #[test]
    fn effective_packages_constrained_explicit_supersedes_bare_default() {
        // Given
        let defaults = vec![spec("numpy")];
        let explicit = vec![spec("numpy=2.0[build=py311h_0]")];

        // When
        let effective = effective_packages(&explicit, &defaults);

        // Then
        assert_eq!(effective, vec![spec("numpy=2.0[build=py311h_0]")]);
    }

    #[test]
    fn effective_packages_channel_qualified_explicit_supersedes_bare_default() {
        // Given
        let defaults = vec![spec("numpy")];
        let explicit = vec![spec("conda-forge::numpy")];

        // When
        let effective = effective_packages(&explicit, &defaults);

        // Then
        assert_eq!(effective, vec![spec("conda-forge::numpy")]);
    }

    #[test]
    fn effective_packages_case_folded_explicit_supersedes_default() {
        // Given
        let defaults = vec![spec("pandas")];
        let explicit = vec![spec("Pandas")];

        // When
        let effective = effective_packages(&explicit, &defaults);

        // Then
        assert_eq!(effective, vec![spec("Pandas")]);
    }

    #[test]
    fn effective_packages_duplicate_bare_names_within_defaults_are_preserved() {
        // Given
        let defaults = vec![spec("numpy=1.2"), spec("numpy=2.0")];

        // When
        let effective = effective_packages(&[], &defaults);

        // Then
        assert_eq!(effective, defaults);
    }

    #[test]
    fn effective_packages_duplicate_bare_names_within_explicit_are_preserved() {
        // Given
        let explicit = vec![spec("numpy=1.2"), spec("numpy=2.0")];

        // When
        let effective = effective_packages(&explicit, &[]);

        // Then
        assert_eq!(effective, explicit);
    }

    #[test]
    fn parse_rejects_a_syntactically_invalid_match_spec() {
        assert!(PackageSpec::parse("[[[not a spec").is_err());
    }

    #[test]
    fn invalid_package_spec_display_redacts_a_credential_bearing_input() {
        let error = PackageSpec::parse("[[[https://user:password@repo.example/t/token-123/pkg")
            .unwrap_err();

        let message = error.to_string();

        assert!(!message.contains("user:password"));
        assert!(!message.contains("token-123"));
        assert!(message.contains("https://repo.example/pkg"));
    }

    #[test]
    fn invalid_package_spec_debug_redacts_credential_bearing_fields() {
        let error = super::InvalidPackageSpec {
            input: "https://user:password@repo.example/t/token-123/pkg".to_string(),
            reason: "failed near password@repo.example/t/token-456/pkg".to_string(),
            index: None,
        };

        let message = format!("{error:?}");

        assert!(!message.contains("user:password"));
        assert!(!message.contains("token-123"));
        assert!(!message.contains("token-456"));
        assert!(message.contains("https://repo.example/pkg"));
    }

    #[test]
    fn package_spec_debug_redacts_a_credential_bearing_spec() {
        let package = spec("https://user:password@repo.example/t/token-123/channel::numpy");

        let message = format!("{package:?}");

        assert!(!message.contains("user:password"));
        assert!(!message.contains("token-123"));
        assert!(message.contains("https://repo.example/channel::numpy"));
    }
}
