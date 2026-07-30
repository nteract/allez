//! `EphemeralEnvError`, `CreationFailure`, `ActivationError`.

use std::fmt;

use crate::error::CategorizedError;

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
    /// Resolution failed without identifying one specific requested package.
    ResolutionFailed,
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
            Self::ResolutionFailed => formatter.write_str("ResolutionFailed"),
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
            Self::UnresolvablePackage { .. } | Self::ResolutionFailed => "unresolvable_package",
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
            Self::ResolutionFailed => write!(formatter, "could not resolve requested packages"),
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
                EphemeralEnvError::IntegrityVerificationFailed {
                    package: "numpy".to_string(),
                },
                "integrity_verification_failed",
            ),
            (EphemeralEnvError::UnwritableLocation, "unwritable_location"),
            (EphemeralEnvError::TeardownFailed, "teardown_failed"),
        ];

        for (error, category) in cases {
            assert_eq!(CategorizedError::category(&error), category);
        }
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
