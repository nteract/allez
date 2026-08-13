//! `EphemeralEnvError`, `CreationFailure`, `ActivationError`.

use std::fmt;

use crate::error::CategorizedError;

use super::channel_auth::origin_only;
use super::channels::redact_channel_url;

/// The fixed category set for ephemeral environment failures.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq)]
pub enum EphemeralEnvError {
    /// No channels remain after applying configured policy.
    NoChannelsConfigured,
    /// A requested package could not be resolved.
    UnresolvablePackage {
        /// The package whose requirements could not be resolved.
        package: String,
    },
    /// A `create_default_packages` entry from the caller's own `.condarc`
    /// could not be resolved. Distinct from [`Self::UnresolvablePackage`]
    /// so the message can point at the configuration file the caller has
    /// to edit, rather than at a command line that never named it.
    UnresolvableDefaultPackage {
        /// The `create_default_packages` entry that could not be resolved.
        package: String,
    },
    /// Resolution failed without identifying one specific requested package.
    ResolutionFailed,
    /// A configured private channel requires a usable `ALLEZ_CHANNEL_TOKEN`.
    MissingChannelToken,
    /// A private channel rejected the credential supplied for its origin.
    ChannelAuthenticationFailed {
        /// The private channel that rejected the credential. `Display`/
        /// `Debug` reduce this to its origin (`origin_only`) defensively,
        /// regardless of what this field itself holds.
        channel: String,
    },
    /// A `<channel>::<package>` qualifier names more than one configured
    /// channel, which one match spec cannot express. Reported rather than
    /// guessed at, so no package is installed from an unintended member.
    AmbiguousChannelQualifier {
        /// The channel name that designates several channels.
        qualifier: String,
    },
    /// Resolution failed for a package set that came entirely from the
    /// caller's own `.condarc` `create_default_packages`, without
    /// identifying one specific entry. The request-level counterpart to
    /// [`Self::UnresolvableDefaultPackage`].
    UnresolvableDefaultPackages,
    /// A package artifact failed integrity verification.
    IntegrityVerificationFailed {
        /// The package whose artifact failed verification.
        package: String,
    },
    /// The environment root cannot be safely written.
    UnwritableLocation,
    /// Removing an ephemeral environment failed.
    TeardownFailed,
}

impl fmt::Debug for EphemeralEnvError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoChannelsConfigured => formatter.write_str("NoChannelsConfigured"),
            Self::UnresolvablePackage { package } => formatter
                .debug_struct("UnresolvablePackage")
                .field("package", &redact_channel_url(package))
                .finish(),
            Self::UnresolvableDefaultPackage { package } => formatter
                .debug_struct("UnresolvableDefaultPackage")
                .field("package", &redact_channel_url(package))
                .finish(),
            Self::ResolutionFailed => formatter.write_str("ResolutionFailed"),
            Self::MissingChannelToken => formatter.write_str("MissingChannelToken"),
            Self::ChannelAuthenticationFailed { channel } => formatter
                .debug_struct("ChannelAuthenticationFailed")
                .field("channel", &origin_only(channel))
                .finish(),
            Self::AmbiguousChannelQualifier { qualifier } => formatter
                .debug_struct("AmbiguousChannelQualifier")
                .field("qualifier", &redact_channel_url(qualifier))
                .finish(),
            Self::UnresolvableDefaultPackages => formatter.write_str("UnresolvableDefaultPackages"),
            Self::IntegrityVerificationFailed { package } => formatter
                .debug_struct("IntegrityVerificationFailed")
                .field("package", &redact_channel_url(package))
                .finish(),
            Self::UnwritableLocation => formatter.write_str("UnwritableLocation"),
            Self::TeardownFailed => formatter.write_str("TeardownFailed"),
        }
    }
}

impl CategorizedError for EphemeralEnvError {
    fn category(&self) -> &'static str {
        match self {
            Self::NoChannelsConfigured => "no_channels_configured",
            Self::UnresolvablePackage { .. }
            | Self::UnresolvableDefaultPackage { .. }
            | Self::UnresolvableDefaultPackages
            | Self::AmbiguousChannelQualifier { .. }
            | Self::ResolutionFailed => "unresolvable_package",
            Self::MissingChannelToken => "missing_channel_token",
            Self::ChannelAuthenticationFailed { .. } => "channel_authentication_failed",
            Self::IntegrityVerificationFailed { .. } => "integrity_verification_failed",
            Self::UnwritableLocation => "unwritable_location",
            Self::TeardownFailed => "teardown_failed",
        }
    }
}

impl fmt::Display for EphemeralEnvError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoChannelsConfigured => write!(formatter, "no channels remain to solve against"),
            Self::UnresolvablePackage { package } => {
                write!(
                    formatter,
                    "could not resolve package `{}`",
                    redact_channel_url(package)
                )
            }
            Self::UnresolvableDefaultPackage { package } => {
                write!(
                    formatter,
                    "could not resolve package `{}` from `.condarc` `create_default_packages`",
                    redact_channel_url(package)
                )
            }
            Self::ResolutionFailed => write!(formatter, "could not resolve requested packages"),
            Self::MissingChannelToken => write!(
                formatter,
                "environment variable `ALLEZ_CHANNEL_TOKEN` is required for a configured private channel but is unset, empty, or not a usable value"
            ),
            Self::ChannelAuthenticationFailed { channel } => write!(
                formatter,
                "channel `{}` rejected the provided credential (HTTP 401 or 403)",
                origin_only(channel)
            ),
            Self::AmbiguousChannelQualifier { qualifier } => write!(
                formatter,
                "channel `{}` names more than one channel; qualify with a single channel URL instead",
                redact_channel_url(qualifier)
            ),
            Self::UnresolvableDefaultPackages => write!(
                formatter,
                "could not resolve packages from `.condarc` `create_default_packages`"
            ),
            Self::IntegrityVerificationFailed { package } => {
                write!(
                    formatter,
                    "integrity verification failed for package `{}`",
                    redact_channel_url(package)
                )
            }
            Self::UnwritableLocation => {
                write!(formatter, "ephemeral environment location is unwritable")
            }
            Self::TeardownFailed => write!(formatter, "ephemeral environment teardown failed"),
        }
    }
}

impl std::error::Error for EphemeralEnvError {}

/// A creation failure and an optional failure while cleaning it up.
#[derive(Clone)]
pub struct CreationFailure {
    /// The environment identifier this failed attempt would have used —
    /// present so a caller/test can correlate this failure with the
    /// `EphemeralLifecycleEvent`s this attempt still emitted (FR-013),
    /// even though no [`super::ReadyEnvironment`] was ever produced.
    pub id: super::EnvironmentId,
    /// Why environment creation failed.
    pub error: EphemeralEnvError,
    /// Why cleanup of a partially-created environment failed, when it did.
    pub cleanup_error: Option<EphemeralEnvError>,
}

impl fmt::Debug for CreationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreationFailure")
            .field("id", &self.id)
            .field("error", &self.error)
            .field("cleanup_error", &self.cleanup_error)
            .finish()
    }
}

impl fmt::Display for CreationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.cleanup_error {
            None => write!(formatter, "{}", self.error),
            Some(cleanup) => write!(formatter, "{} (cleanup also failed: {cleanup})", self.error),
        }
    }
}

impl std::error::Error for CreationFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// A failure computing activation variables for an existing environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationError {
    /// The activation failure's human-readable explanation.
    pub message: String,
}

impl fmt::Display for ActivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ActivationError {}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use crate::error::CategorizedError;

    use super::{CreationFailure, EphemeralEnvError};
    use crate::ephemeral::EnvironmentId;

    #[test]
    fn ephemeral_error_categories_match_the_fixed_contract() {
        let cases = [
            (
                EphemeralEnvError::NoChannelsConfigured,
                "no_channels_configured",
            ),
            (
                EphemeralEnvError::UnresolvablePackage {
                    package: "numpy".to_string(),
                },
                "unresolvable_package",
            ),
            (EphemeralEnvError::ResolutionFailed, "unresolvable_package"),
            (
                EphemeralEnvError::AmbiguousChannelQualifier {
                    qualifier: "bundle".to_string(),
                },
                "unresolvable_package",
            ),
            (
                EphemeralEnvError::UnresolvableDefaultPackages,
                "unresolvable_package",
            ),
            (
                EphemeralEnvError::UnresolvableDefaultPackage {
                    package: "numpy".to_string(),
                },
                "unresolvable_package",
            ),
            (
                EphemeralEnvError::IntegrityVerificationFailed {
                    package: "numpy".to_string(),
                },
                "integrity_verification_failed",
            ),
            (EphemeralEnvError::UnwritableLocation, "unwritable_location"),
            (EphemeralEnvError::TeardownFailed, "teardown_failed"),
            (
                EphemeralEnvError::MissingChannelToken,
                "missing_channel_token",
            ),
            (
                EphemeralEnvError::ChannelAuthenticationFailed {
                    channel: "https://repo.example".to_string(),
                },
                "channel_authentication_failed",
            ),
        ];

        for (error, category) in cases {
            assert_eq!(CategorizedError::category(&error), category);
        }
    }

    #[test]
    fn missing_channel_token_display_matches_the_contract() {
        // Given
        let error = EphemeralEnvError::MissingChannelToken;

        // When
        let display = error.to_string();

        // Then
        assert_eq!(
            display,
            "environment variable `ALLEZ_CHANNEL_TOKEN` is required for a configured private channel but is unset, empty, or not a usable value"
        );
    }

    #[test]
    fn channel_authentication_failed_display_matches_the_contract() {
        // Given
        let error = EphemeralEnvError::ChannelAuthenticationFailed {
            channel: "https://repo.example".to_string(),
        };

        // When
        let display = error.to_string();

        // Then
        assert_eq!(
            display,
            "channel `https://repo.example` rejected the provided credential (HTTP 401 or 403)"
        );
    }

    #[test]
    fn creation_failure_without_cleanup_error_displays_creation_error() {
        let failure = CreationFailure {
            id: EnvironmentId::new(),
            error: EphemeralEnvError::UnwritableLocation,
            cleanup_error: None,
        };

        assert_eq!(
            failure.to_string(),
            "ephemeral environment location is unwritable"
        );
    }

    #[test]
    fn creation_failure_with_cleanup_error_displays_both_errors() {
        let failure = CreationFailure {
            id: EnvironmentId::new(),
            error: EphemeralEnvError::UnwritableLocation,
            cleanup_error: Some(EphemeralEnvError::TeardownFailed),
        };

        assert_eq!(
            failure.to_string(),
            "ephemeral environment location is unwritable (cleanup also failed: ephemeral environment teardown failed)"
        );
    }

    #[test]
    fn creation_failure_source_is_the_original_error() {
        let failure = CreationFailure {
            id: EnvironmentId::new(),
            error: EphemeralEnvError::NoChannelsConfigured,
            cleanup_error: None,
        };

        assert_eq!(
            failure.source().map(ToString::to_string),
            Some("no channels remain to solve against".to_string())
        );
    }

    #[test]
    fn unresolvable_package_display_redacts_a_credential_bearing_spec() {
        let error = EphemeralEnvError::UnresolvablePackage {
            package: "https://user:password@repo.example/t/token-123/conda-forge::numpy"
                .to_string(),
        };

        let message = error.to_string();

        assert!(!message.contains("user:password"));
        assert!(!message.contains("token-123"));
        assert!(message.contains("https://repo.example/conda-forge::numpy"));
    }

    #[test]
    fn request_level_resolution_failure_does_not_name_a_package() {
        let error = EphemeralEnvError::ResolutionFailed;

        assert_eq!(error.to_string(), "could not resolve requested packages");
    }

    #[test]
    fn request_level_default_failure_names_condarc_without_naming_a_package() {
        // Given
        let error = EphemeralEnvError::UnresolvableDefaultPackages;

        // When
        let message = error.to_string();

        // Then
        assert_eq!(
            message,
            "could not resolve packages from `.condarc` `create_default_packages`"
        );
    }

    #[test]
    fn unresolvable_default_package_display_names_its_condarc_source() {
        // Given
        let error = EphemeralEnvError::UnresolvableDefaultPackage {
            package: "[[[not a spec".to_string(),
        };

        // When
        let message = error.to_string();

        // Then
        assert_eq!(
            message,
            "could not resolve package `[[[not a spec` from `.condarc` `create_default_packages`"
        );
    }

    #[test]
    fn unresolvable_default_package_display_redacts_a_credential_bearing_spec() {
        // Given
        let error = EphemeralEnvError::UnresolvableDefaultPackage {
            package: "https://user:password@repo.example/t/token-123/conda-forge::numpy"
                .to_string(),
        };

        // When
        let message = error.to_string();

        // Then
        assert!(!message.contains("user:password"));
        assert!(!message.contains("token-123"));
        assert!(message.contains("create_default_packages"));
    }

    #[test]
    fn unresolvable_default_package_debug_redacts_a_credential_bearing_spec() {
        // Given
        let error = EphemeralEnvError::UnresolvableDefaultPackage {
            package: "https://user:password@repo.example/t/token-123/conda-forge::numpy"
                .to_string(),
        };

        // When
        let message = format!("{error:?}");

        // Then
        assert!(!message.contains("user:password"));
        assert!(!message.contains("token-123"));
        assert!(message.contains("UnresolvableDefaultPackage"));
    }

    #[test]
    fn unresolvable_package_debug_redacts_a_credential_bearing_spec() {
        let error = EphemeralEnvError::UnresolvablePackage {
            package: "https://user:password@repo.example/t/token-123/conda-forge::numpy"
                .to_string(),
        };

        let message = format!("{error:?}");

        assert!(!message.contains("user:password"));
        assert!(!message.contains("token-123"));
        assert!(message.contains("https://repo.example/conda-forge::numpy"));
    }

    #[test]
    fn integrity_verification_failed_display_redacts_a_credential_bearing_spec() {
        let error = EphemeralEnvError::IntegrityVerificationFailed {
            package: "https://user:password@repo.example/t/token-123/numpy".to_string(),
        };

        let message = error.to_string();

        assert!(!message.contains("user:password"));
        assert!(!message.contains("token-123"));
    }

    #[test]
    fn integrity_verification_failed_debug_redacts_a_credential_bearing_spec() {
        let error = EphemeralEnvError::IntegrityVerificationFailed {
            package: "https://user:password@repo.example/t/token-123/numpy".to_string(),
        };

        let message = format!("{error:?}");

        assert!(!message.contains("user:password"));
        assert!(!message.contains("token-123"));
    }

    #[test]
    fn creation_failure_debug_redacts_credential_bearing_errors() {
        let failure = CreationFailure {
            id: EnvironmentId::new(),
            error: EphemeralEnvError::UnresolvablePackage {
                package: "https://user:password@repo.example/t/token-123/numpy".to_string(),
            },
            cleanup_error: Some(EphemeralEnvError::IntegrityVerificationFailed {
                package: "https://other:secret@repo.example/t/token-456/pandas".to_string(),
            }),
        };

        let message = format!("{failure:?}");

        assert!(!message.contains("user:password"));
        assert!(!message.contains("token-123"));
        assert!(!message.contains("other:secret"));
        assert!(!message.contains("token-456"));
    }
}
