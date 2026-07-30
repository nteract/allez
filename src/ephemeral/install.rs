//! `rattler::install::Installer` wiring.
//!
//! The installer uses the shared `<root>/cache/packages` cache, but explicitly
//! disables hard links before linking a package into an environment prefix.
//! `rattler::install::LinkOptions::allow_hard_links` provides that option, so a
//! per-environment cache fallback is unnecessary.

use std::{error::Error as _, path::Path};

use rattler::install::{Installer, InstallerError, LinkOptions};
use rattler_cache::package_cache::PackageCache;
use rattler_conda_types::RepoDataRecord;

use super::{
    channels::redact_channel_url, error::EphemeralEnvError, lifecycle::InstalledPackage,
    paths::VerifiedRoot, solve::SolvedPackages,
};

pub(crate) async fn install_packages(
    root: &VerifiedRoot,
    prefix: &Path,
    solution: SolvedPackages,
) -> Result<Vec<InstalledPackage>, EphemeralEnvError> {
    let records = validate_file_records(solution.records, validate_file_record).await?;
    let installed_packages = records
        .iter()
        .map(|record| InstalledPackage {
            name: record.package_record.name.as_source().to_string(),
            version: record.package_record.version.to_string(),
            channel: record
                .channel
                .as_deref()
                .map(redact_channel_url)
                .unwrap_or_default(),
        })
        .collect();
    let package_names = records
        .iter()
        .map(|record| {
            (
                record.identifier.to_string(),
                record.package_record.name.as_source().to_string(),
            )
        })
        .collect::<Vec<_>>();
    let installer = Installer::new()
        .with_package_cache(PackageCache::new(root.path().join("cache/packages")))
        .with_download_client(solution.client)
        .with_execute_link_scripts(true)
        .with_link_options(LinkOptions {
            allow_hard_links: Some(false),
            ..LinkOptions::default()
        });

    installer
        .install(prefix, records)
        .await
        .map_err(|error| match error {
            InstallerError::FailedToFetch(identifier, source) => {
                let mut cause = source.source();
                let mut integrity_failure = false;
                while let Some(error) = cause {
                    if error.to_string().contains("hash mismatch") {
                        integrity_failure = true;
                        break;
                    }
                    cause = error.source();
                }
                let package = package_for_identifier(&package_names, identifier);
                if integrity_failure {
                    EphemeralEnvError::IntegrityVerificationFailed { package }
                } else {
                    EphemeralEnvError::UnresolvablePackage { package }
                }
            }
            // Both carry their own package identifier (the same identifier
            // `FailedToFetch` above already maps back to a package name),
            // unlike the fallback-attributed variants below.
            InstallerError::LinkError(identifier, _)
            | InstallerError::UnlinkError(identifier, _) => {
                EphemeralEnvError::UnresolvablePackage {
                    package: package_for_identifier(&package_names, identifier),
                }
            }
            // Carries its own package name(s) directly; using the first one
            // is accurate here, unlike the fallback-attributed variants
            // below, where `fallback_package` is a guess.
            InstallerError::PlatformSpecificPackagesWithNoarchPlatform(packages) => {
                EphemeralEnvError::UnresolvablePackage {
                    package: packages
                        .into_iter()
                        .next()
                        .unwrap_or_else(|| "unknown".to_string()),
                }
            }
            // Filesystem/cache-resource failures, not attributable to any
            // one requested package.
            InstallerError::FailedToCreatePrefix(_, _)
            | InstallerError::IoError(_, _)
            | InstallerError::FailedToAcquireCacheLock(_) => EphemeralEnvError::UnwritableLocation,
            InstallerError::Cancelled => EphemeralEnvError::ResolutionFailed,
            InstallerError::FailedToDetectInstalledPackages(_)
            | InstallerError::FailedToConstructTransaction(_)
            | InstallerError::UnsafePackageRecord(_)
            | InstallerError::PreProcessingFailed(_)
            | InstallerError::PostProcessingFailed(_)
            | InstallerError::ClobberError(_)
            | InstallerError::ClobberingDetected(_) => EphemeralEnvError::ResolutionFailed,
        })?;

    Ok(installed_packages)
}

async fn validate_file_records(
    records: Vec<RepoDataRecord>,
    mut validator: impl FnMut(&RepoDataRecord) -> Result<(), EphemeralEnvError> + Send + 'static,
) -> Result<Vec<RepoDataRecord>, EphemeralEnvError> {
    super::run_blocking(EphemeralEnvError::ResolutionFailed, move || {
        for record in &records {
            validator(record)?;
        }
        Ok(records)
    })
    .await
}

/// Maps a rattler-internal archive identifier back to the requested
/// package name it belongs to, falling back to the raw identifier itself
/// (still more specific than an unrelated package) if no match is found.
fn package_for_identifier(package_names: &[(String, String)], identifier: String) -> String {
    package_names
        .iter()
        .find(|(archive, _)| archive == &identifier)
        .map(|(_, package)| package.clone())
        .unwrap_or(identifier)
}

fn validate_file_record(record: &RepoDataRecord) -> Result<(), EphemeralEnvError> {
    if record.url.scheme() != "file" {
        return Ok(());
    }

    let package = record.package_record.name.as_source().to_string();
    let path = record
        .url
        .to_file_path()
        .map_err(|_| EphemeralEnvError::UnresolvablePackage {
            package: package.clone(),
        })?;
    let matches = match (record.package_record.sha256, record.package_record.md5) {
        (Some(expected), _) => rattler_digest::compute_file_digest::<rattler_digest::Sha256>(&path)
            .map(|actual| actual == expected),
        (None, Some(expected)) => rattler_digest::compute_file_digest::<rattler_digest::Md5>(&path)
            .map(|actual| actual == expected),
        // A local `file://` record declaring neither checksum has nothing
        // this feature's own check can verify against, so it cannot be
        // treated as passing integrity verification (FR-011/SC-006).
        (None, None) => return Err(EphemeralEnvError::IntegrityVerificationFailed { package }),
    }
    .map_err(|_| EphemeralEnvError::UnresolvablePackage {
        package: package.clone(),
    })?;

    if matches {
        Ok(())
    } else {
        Err(EphemeralEnvError::IntegrityVerificationFailed { package })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rattler_conda_types::Channel;

    use super::install_packages;
    use crate::ephemeral::{ChannelConfig, EphemeralEnvError, PackageSpec};

    fn fixture_channel_url() -> String {
        let fixture_directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ephemeral_channel");
        Channel::try_from_directory(&fixture_directory)
            .unwrap()
            .canonical_name()
    }

    #[tokio::test]
    async fn install_packages_when_archive_checksum_is_corrupt_returns_integrity_error() {
        // Given
        let temporary_directory = tempfile::tempdir().unwrap();
        let root =
            super::super::paths::verified_root(&temporary_directory.path().join("root")).unwrap();
        let packages = vec![PackageSpec::parse("fixture-corrupt-checksum").unwrap()];
        let solution = super::super::solve::solve_packages(
            &root,
            &ChannelConfig::from_urls(vec![fixture_channel_url()]),
            &packages,
        )
        .await
        .unwrap();
        let prefix = root.path().join("envs/corrupt-checksum");

        // When
        let error = install_packages(&root, &prefix, solution)
            .await
            .unwrap_err();

        // Then
        assert_eq!(
            error,
            EphemeralEnvError::IntegrityVerificationFailed {
                package: "fixture-corrupt-checksum".to_string(),
            }
        );
    }

    #[test]
    fn validate_file_record_rejects_a_file_record_with_no_declared_checksum() {
        use rattler_conda_types::{
            PackageName, PackageRecord, RepoDataRecord, VersionWithSource,
            package::DistArchiveIdentifier,
        };

        // Given
        let channel = Channel::try_from_directory(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ephemeral_channel"),
        )
        .unwrap();
        let archive_url = channel
            .base_url
            .url()
            .join("noarch/fixture-none-1.0.0-0.tar.bz2")
            .unwrap();
        let record = RepoDataRecord {
            package_record: PackageRecord::new(
                "fixture-none".parse::<PackageName>().unwrap(),
                "1.0.0".parse::<VersionWithSource>().unwrap(),
                "0".to_string(),
            ),
            identifier: "fixture-none-1.0.0-0.tar.bz2"
                .parse::<DistArchiveIdentifier>()
                .unwrap(),
            url: archive_url,
            channel: None,
        };

        // When
        let error = super::validate_file_record(&record).unwrap_err();

        // Then
        assert_eq!(
            error,
            EphemeralEnvError::IntegrityVerificationFailed {
                package: "fixture-none".to_string(),
            }
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn file_record_validation_runs_without_blocking_the_async_worker() {
        use std::sync::{Arc, Barrier};

        use rattler_conda_types::{
            PackageName, PackageRecord, RepoDataRecord, VersionWithSource,
            package::DistArchiveIdentifier,
        };
        use tokio::sync::oneshot;

        // Given
        let record = RepoDataRecord {
            package_record: PackageRecord::new(
                "fixture-none".parse::<PackageName>().unwrap(),
                "1.0.0".parse::<VersionWithSource>().unwrap(),
                "0".to_string(),
            ),
            identifier: "fixture-none-1.0.0-0.tar.bz2"
                .parse::<DistArchiveIdentifier>()
                .unwrap(),
            url: "file:///unused".parse().unwrap(),
            channel: None,
        };
        let (started_tx, started_rx) = oneshot::channel();
        let release = Arc::new(Barrier::new(2));
        let validation_release = Arc::clone(&release);
        let mut started_tx = Some(started_tx);

        // When
        let validation = tokio::spawn(super::validate_file_records(vec![record], move |_| {
            started_tx.take().unwrap().send(()).unwrap();
            validation_release.wait();
            Ok(())
        }));
        started_rx.await.unwrap();
        release.wait();
        let records = validation.await.unwrap().unwrap();

        // Then
        assert_eq!(records.len(), 1);
    }
}
