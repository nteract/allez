//! `EphemeralEnvironmentHandle`, `LifecycleState`, `ReadyEnvironment`,
//! `InstalledPackage`.

use std::{fmt, path::PathBuf, sync::Arc};

use rattler_conda_types::Platform;
use rattler_shell::{
    activation::{ActivationVariables, Activator},
    shell,
};
use ulid::Ulid;

use super::{channels::redact_channel_url, cleanup::CleanupGuard, error::ActivationError};

pub use super::handle::{EphemeralEnvironmentHandle, ReclamationStatus};
pub(crate) use super::state::CleanupOutcome;
#[cfg(test)]
pub(crate) use super::state::{CreationOutcomeCell, LifecycleState, TeardownOutcome};

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
    use super::InstalledPackage;

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
}

/// A successfully created environment and its installed packages.
#[derive(Debug, Clone)]
pub struct ReadyEnvironment {
    /// This environment's stable identifier.
    pub id: EnvironmentId,
    /// The environment prefix location.
    pub location: PathBuf,
    /// Packages installed in the environment.
    pub installed_packages: Vec<InstalledPackage>,
    #[expect(
        dead_code,
        reason = "ownership keeps the environment alive until the final clone drops"
    )]
    keep_alive: Arc<CleanupGuard>,
}

impl ReadyEnvironment {
    pub(crate) fn new(
        id: EnvironmentId,
        location: PathBuf,
        installed_packages: Vec<InstalledPackage>,
        keep_alive: Arc<CleanupGuard>,
    ) -> Self {
        Self {
            id,
            location,
            installed_packages,
            keep_alive,
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
            keep_alive: Arc::new(CleanupGuard::test_only()),
        }
    }
}
