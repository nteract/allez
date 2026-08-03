//! `rattler_repodata_gateway::Gateway` + `rattler_solve` wiring.

use rattler_conda_types::{
    Channel, ChannelConfig as RattlerChannelConfig, MatchSpec, ParseStrictness, Platform,
    RepoDataRecord,
};
use rattler_repodata_gateway::Gateway;
use rattler_solve::{ChannelPriority, SolverImpl, SolverTask, resolvo::Solver};
use rattler_virtual_packages::{Override, VirtualPackageOverrides, VirtualPackages};

use super::{defaults::PackageSpec, error::EphemeralEnvError, paths::VerifiedRoot};

/// A stable `User-Agent`, distinct from `reqwest`'s own default of sending
/// none at all: `repo.anaconda.com`'s CDN has been observed rejecting
/// requests carrying no `User-Agent` header with an HTTP 403 (confirmed
/// empirically), even though the exact same request with any identifying
/// `User-Agent` succeeds. Every HTTP request this feature makes -- both
/// repodata queries here and package downloads in `install.rs`, which reuses
/// this same client -- goes through this one client, so setting it once
/// here covers both.
const HTTP_USER_AGENT: &str = concat!("allez/", env!("CARGO_PKG_VERSION"));

pub(crate) struct SolvedPackages {
    pub(crate) records: Vec<RepoDataRecord>,
    pub(crate) client: reqwest::Client,
}

pub(crate) async fn solve_packages(
    root: &VerifiedRoot,
    config: &condarc::ResolvedChannels,
    packages: &[PackageSpec],
) -> Result<SolvedPackages, EphemeralEnvError> {
    if config.channels.is_empty() {
        return Err(EphemeralEnvError::NoChannelsConfigured);
    }

    let client = reqwest::Client::builder()
        .no_proxy()
        .user_agent(HTTP_USER_AGENT)
        .build()
        .map_err(|_| EphemeralEnvError::ResolutionFailed)?;
    let channel_config = RattlerChannelConfig::default_with_root_dir(root.path().to_path_buf());
    let sources = config
        .channels
        .iter()
        .map(|channel| Channel::from_str(channel, &channel_config))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| EphemeralEnvError::ResolutionFailed)?;
    let specs = packages
        .iter()
        .map(|package| {
            MatchSpec::from_str(package.as_str(), ParseStrictness::Strict).map_err(|_| {
                EphemeralEnvError::UnresolvablePackage {
                    package: package.as_str().to_string(),
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let gateway = Gateway::builder()
        .with_client(client.clone())
        .with_cache_dir(root.path().join("cache/repodata"))
        .finish();
    let repodata = gateway
        .query(
            sources,
            [Platform::current(), Platform::NoArch],
            specs.clone(),
        )
        .recursive(true)
        .execute()
        .await
        .map_err(|_| EphemeralEnvError::ResolutionFailed)?;

    let mut task: SolverTask<_> = repodata.iter().map(|data| data.iter()).collect();
    task.channel_priority = solver_priority(config.channel_priority);
    task.specs = specs;
    let mut virtual_package_overrides = VirtualPackageOverrides::default();
    virtual_package_overrides.cuda = Some(Override::String(String::new()));
    virtual_package_overrides.cuda_arch = Some(Override::String(String::new()));
    task.virtual_packages = VirtualPackages::detect(&virtual_package_overrides)
        .map_err(|_| EphemeralEnvError::ResolutionFailed)?
        .into_generic_virtual_packages()
        .collect();

    // Runs synchronously on this async task's own worker thread, not via
    // `tokio::task::spawn_blocking` (unlike this feature's own filesystem
    // code -- see `mod.rs`'s `run_blocking`): `rattler_solve::SolverTask`
    // borrows `repodata` by design (its own doc comments state this is
    // deliberate, so callers keep ownership of the underlying storage),
    // so it cannot be moved into a `'static` `spawn_blocking` closure
    // without first cloning every candidate record -- disproportionate for
    // what is, in practice, a fast, small solve against a handful of
    // requested packages; a genuinely large/slow solve remains a known,
    // accepted limitation rather than a fixed one.
    let mut solver = Solver;
    let result = solver.solve(task).map_err(|_| match packages {
        [package] => EphemeralEnvError::UnresolvablePackage {
            package: package.as_str().to_string(),
        },
        _ => EphemeralEnvError::ResolutionFailed,
    })?;

    Ok(SolvedPackages {
        records: result.records,
        client,
    })
}

const fn solver_priority(priority: condarc::ChannelPriority) -> ChannelPriority {
    match priority {
        condarc::ChannelPriority::Strict => ChannelPriority::Strict,
        condarc::ChannelPriority::Flexible | condarc::ChannelPriority::Disabled => {
            ChannelPriority::Disabled
        }
        _ => ChannelPriority::Disabled,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rattler_conda_types::Channel;

    use super::solve_packages;
    use crate::ephemeral::{EphemeralEnvError, PackageSpec};

    /// Shared by every test below: a fresh temporary, verified root. The
    /// returned `TempDir` must be kept alive alongside `VerifiedRoot` --
    /// dropping it early removes the directory `VerifiedRoot` still refers to.
    fn test_root() -> (tempfile::TempDir, super::super::paths::VerifiedRoot) {
        let temporary_directory = tempfile::tempdir().unwrap();
        let root =
            super::super::paths::verified_root(&temporary_directory.path().join("root")).unwrap();
        (temporary_directory, root)
    }

    #[tokio::test]
    async fn solve_packages_when_channels_are_empty_returns_no_channels() {
        // Given
        let (_temporary_directory, root) = test_root();
        let config = condarc::ResolvedChannels::from_channels(Vec::new());
        let packages = vec![PackageSpec::parse("fixture-default-alpha").unwrap()];

        // When
        let result = solve_packages(&root, &config, &packages).await;

        // Then
        assert!(matches!(
            result,
            Err(EphemeralEnvError::NoChannelsConfigured)
        ));
    }

    #[tokio::test]
    async fn channel_parse_failure_is_not_attributed_to_the_first_package() {
        // Given
        let (_temporary_directory, root) = test_root();
        let config = condarc::ResolvedChannels::from_channels(vec!["https://[".to_string()]);
        let packages = vec![
            PackageSpec::parse("known-good-first").unwrap(),
            PackageSpec::parse("unrelated-second").unwrap(),
        ];

        // When
        let result = solve_packages(&root, &config, &packages).await;

        // Then
        assert!(matches!(result, Err(EphemeralEnvError::ResolutionFailed)));
    }

    #[tokio::test]
    async fn multi_package_solver_failure_is_not_attributed_to_the_first_package() {
        // Given
        let (_temporary_directory, root) = test_root();
        let fixture_directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ephemeral_channel");
        let channel = Channel::try_from_directory(&fixture_directory)
            .unwrap()
            .canonical_name();
        let config = condarc::ResolvedChannels::from_channels(vec![channel]);
        let packages = vec![
            PackageSpec::parse("fixture-default-alpha").unwrap(),
            PackageSpec::parse("missing-later-package").unwrap(),
        ];

        // When
        let result = solve_packages(&root, &config, &packages).await;

        // Then
        assert!(matches!(result, Err(EphemeralEnvError::ResolutionFailed)));
    }

    #[test]
    fn solver_priority_when_flexible_uses_disabled_priority() {
        // Given
        let priority = condarc::ChannelPriority::Flexible;

        // When
        let resolved_priority = super::solver_priority(priority);

        // Then
        assert_eq!(resolved_priority, rattler_solve::ChannelPriority::Disabled);
    }
}
