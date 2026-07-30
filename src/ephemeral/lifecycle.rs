//! `EnvironmentId`, `InstalledPackage`, `ReadyEnvironment`.

use std::{fmt, path::PathBuf};

use rattler_conda_types::Platform;
use rattler_shell::{
    activation::{ActivationVariables, Activator},
    shell,
};
use ulid::Ulid;

use super::{channels::redact_channel_url, error::ActivationError};

/// A unique identifier for one ephemeral environment lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvironmentId(Ulid);

impl EnvironmentId {
    /// Generates a new ULID-backed environment identifier.
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        value.parse().ok().map(Self)
    }
}

impl Default for EnvironmentId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for EnvironmentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// An installed package projected without exposing rattler types.
#[derive(Clone)]
pub struct InstalledPackage {
    /// The package name.
    pub name: String,
    /// The resolved package version.
    pub version: String,
    /// The package's source channel.
    pub channel: String,
}

impl fmt::Debug for InstalledPackage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InstalledPackage")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("channel", &redact_channel_url(&self.channel))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{InstalledPackage, ReadyEnvironment};
    use crate::ephemeral::error::ActivationError;

    #[test]
    fn installed_package_debug_redacts_channel_credentials() {
        let package = InstalledPackage {
            name: "numpy".to_string(),
            version: "2.0.0".to_string(),
            channel: "https://user:password@repo.example/t/token-123/channel".to_string(),
        };

        let message = format!("{package:?}");

        assert!(!message.contains("user:password"));
        assert!(!message.contains("token-123"));
        assert!(message.contains("https://repo.example/channel"));
    }

    #[test]
    fn activation_environment_when_prefix_has_executables_includes_prefix_path() {
        // Given
        let temporary_directory = tempfile::tempdir().unwrap();
        let prefix = temporary_directory.path().join("environment");
        #[cfg(windows)]
        let executable_directory = prefix.join("Scripts");
        #[cfg(not(windows))]
        let executable_directory = prefix.join("bin");
        fs::create_dir_all(&executable_directory).unwrap();
        let environment = ReadyEnvironment::test_with_location(&prefix);

        // When
        let overlay = environment.activation_environment().unwrap();

        // Then
        let path = overlay
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| value)
            .unwrap();
        assert!(std::env::split_paths(path).any(|entry| entry == executable_directory));
    }

    #[test]
    fn activation_environment_when_prefix_state_is_malformed_returns_activation_error() {
        // Given
        let temporary_directory = tempfile::tempdir().unwrap();
        let prefix = temporary_directory.path().join("environment");
        fs::create_dir_all(prefix.join("conda-meta")).unwrap();
        fs::write(prefix.join("conda-meta/state"), "{").unwrap();
        let environment = ReadyEnvironment::test_with_location(&prefix);

        // When
        let result = environment.activation_environment();

        // Then
        assert!(matches!(result, Err(ActivationError { .. })));
    }
}

/// A successfully created environment and its installed packages.
///
/// **Not torn down automatically** — see the GEN-24 spec's "Explicit
/// reap, no automatic reaping" decision: this value carries no cleanup
/// guard of any kind, and dropping it (or every clone of it) has no
/// effect on the environment's directory. The environment persists on
/// disk until a caller explicitly calls
/// [`super::reap_ephemeral_environments`].
#[derive(Debug, Clone)]
pub struct ReadyEnvironment {
    /// This environment's stable identifier.
    pub id: EnvironmentId,
    /// The environment prefix location.
    pub location: PathBuf,
    /// Packages installed in the environment.
    pub installed_packages: Vec<InstalledPackage>,
}

impl ReadyEnvironment {
    pub(crate) fn new(
        id: EnvironmentId,
        location: PathBuf,
        installed_packages: Vec<InstalledPackage>,
    ) -> Self {
        Self {
            id,
            location,
            installed_packages,
        }
    }

    /// Returns the environment-variable overlay needed to activate this prefix.
    pub fn activation_environment(&self) -> Result<Vec<(String, String)>, ActivationError> {
        let variables = ActivationVariables::from_env().map_err(|error| ActivationError {
            message: error.to_string(),
        })?;
        let activation = {
            #[cfg(unix)]
            {
                Activator::from_path(&self.location, shell::Bash::default(), Platform::current())
                    .and_then(|activator| activator.run_activation(variables, None))
            }
            #[cfg(windows)]
            {
                Activator::from_path(&self.location, shell::CmdExe, Platform::current())
                    .and_then(|activator| activator.run_activation(variables, None))
            }
        };

        activation
            .map(|variables| variables.into_iter().collect())
            .map_err(|error| ActivationError {
                message: error.to_string(),
            })
    }
}

#[cfg(test)]
impl ReadyEnvironment {
    pub(crate) fn test_with_location(location: impl Into<PathBuf>) -> Self {
        Self {
            id: EnvironmentId::new(),
            location: location.into(),
            installed_packages: Vec::new(),
        }
    }
}
