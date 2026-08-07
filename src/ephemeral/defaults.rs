//! `DEFAULT_PACKAGES`, `PackageSpec`, `RequestedPackages`, and
//! `effective_packages()`.

use std::fmt;

use rattler_conda_types::{MatchSpec, ParseStrictness};

use super::channels::redact_channel_url;

/// Fixed, non-empty default package set. A documented stopgap pending
/// GEN-30's own default/override-authoring mechanism: no mechanism in this
/// codebase surfaces a caller-configured override today, so this constant
/// is `allez oneshot`'s only real default. `python` is a real,
/// near-universally-available package on real channels; it does not
/// resolve against the checked-in local fixture channel
/// (`tests/fixtures/ephemeral_channel/`) used by this crate's own test
/// suite (see `research.md`'s "Default package list" decision).
pub const DEFAULT_PACKAGES: &[&str] = &["python"];

/// An opaque, syntactically-validated conda match-spec string.
#[derive(Clone, PartialEq, Eq)]
pub struct PackageSpec(String);

impl fmt::Debug for PackageSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PackageSpec")
            .field(&redact_channel_url(&self.0))
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
            .map(|_| Self(input.to_string()))
            .map_err(|error| InvalidPackageSpec {
                input: input.to_string(),
                reason: error.to_string(),
            })
    }

    /// The opaque match-spec string this `PackageSpec` wraps.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// The caller's requested package set: either an explicit list, or a
/// request to use the configured override/built-in default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestedPackages {
    /// A caller-supplied package list. An empty list is treated identically
    /// to [`RequestedPackages::UseDefaultOrOverride`] by the crate-internal
    /// `effective_packages` resolution function.
    Explicit(Vec<PackageSpec>),
    /// No packages were explicitly requested; resolve the configured
    /// override, or [`DEFAULT_PACKAGES`] if none is configured.
    UseDefaultOrOverride,
}

impl RequestedPackages {
    /// Translates a raw, possibly-empty caller-supplied package list: an
    /// empty list becomes [`RequestedPackages::UseDefaultOrOverride`]; a
    /// non-empty list is parsed into [`RequestedPackages::Explicit`].
    pub fn from_cli(packages: Vec<String>) -> Result<Self, InvalidPackageSpec> {
        if packages.is_empty() {
            return Ok(Self::UseDefaultOrOverride);
        }
        packages
            .into_iter()
            .map(|package| PackageSpec::parse(&package))
            .collect::<Result<Vec<_>, _>>()
            .map(Self::Explicit)
    }
}

/// Resolves the effective top-level package set per FR-005/FR-006's
/// precedence: a non-empty explicit request wins outright; otherwise the
/// configured override wins if non-empty, else [`DEFAULT_PACKAGES`].
pub fn effective_packages(
    requested: &RequestedPackages,
    default_override: Option<&[PackageSpec]>,
) -> Vec<PackageSpec> {
    if let RequestedPackages::Explicit(packages) = requested
        && !packages.is_empty()
    {
        return packages.clone();
    }
    match default_override {
        Some(override_packages) if !override_packages.is_empty() => override_packages.to_vec(),
        _ => DEFAULT_PACKAGES
            .iter()
            .map(|package| {
                PackageSpec::parse(package).unwrap_or_else(|_| PackageSpec(package.to_string()))
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_PACKAGES, PackageSpec, RequestedPackages, effective_packages};

    fn spec(input: &str) -> PackageSpec {
        PackageSpec::parse(input).unwrap()
    }

    #[test]
    fn from_cli_empty_list_becomes_use_default_or_override() {
        assert_eq!(
            RequestedPackages::from_cli(vec![]).unwrap(),
            RequestedPackages::UseDefaultOrOverride
        );
    }

    #[test]
    fn from_cli_non_empty_list_becomes_explicit() {
        let requested = RequestedPackages::from_cli(vec!["numpy".to_string()]).unwrap();
        assert_eq!(requested, RequestedPackages::Explicit(vec![spec("numpy")]));
    }

    #[test]
    fn explicit_non_empty_wins_over_any_override() {
        let requested = RequestedPackages::Explicit(vec![spec("numpy")]);
        let override_packages = vec![spec("pandas")];

        assert_eq!(
            effective_packages(&requested, Some(&override_packages)),
            vec![spec("numpy")]
        );
    }

    #[test]
    fn explicit_empty_falls_back_like_use_default_or_override() {
        let explicit_empty = RequestedPackages::Explicit(vec![]);
        let use_default = RequestedPackages::UseDefaultOrOverride;
        let override_packages = vec![spec("pandas")];

        assert_eq!(
            effective_packages(&explicit_empty, Some(&override_packages)),
            effective_packages(&use_default, Some(&override_packages)),
        );
    }

    #[test]
    fn non_empty_override_wins_over_default_packages() {
        let override_packages = vec![spec("pandas")];

        assert_eq!(
            effective_packages(
                &RequestedPackages::UseDefaultOrOverride,
                Some(&override_packages)
            ),
            vec![spec("pandas")]
        );
    }

    #[test]
    fn override_resolving_to_empty_falls_back_to_default_packages() {
        let resolved = effective_packages(&RequestedPackages::UseDefaultOrOverride, Some(&[]));
        let expected: Vec<PackageSpec> = DEFAULT_PACKAGES.iter().map(|p| spec(p)).collect();

        assert_eq!(resolved, expected);
    }

    #[test]
    fn no_override_falls_back_to_default_packages() {
        let resolved = effective_packages(&RequestedPackages::UseDefaultOrOverride, None);
        let expected: Vec<PackageSpec> = DEFAULT_PACKAGES.iter().map(|p| spec(p)).collect();

        assert_eq!(resolved, expected);
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
