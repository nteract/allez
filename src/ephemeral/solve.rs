//! `rattler_repodata_gateway::Gateway` + `rattler_solve` wiring.

use rattler_conda_types::{
    Channel, ChannelConfig as RattlerChannelConfig, MatchSpec, ParseStrictness, Platform,
    RepoDataRecord,
};
use rattler_repodata_gateway::Gateway;
use rattler_solve::{ChannelPriority, SolverImpl, SolverTask, resolvo::Solver};
use rattler_virtual_packages::{Override, VirtualPackageOverrides, VirtualPackages};

use super::{
    defaults::{PackageOrigin, PackageSpec},
    error::EphemeralEnvError,
    paths::VerifiedRoot,
};

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
            let spec = qualify_against_configured_channels(package, config)?;
            MatchSpec::from_str(&spec, ParseStrictness::Strict).map_err(|_| unresolvable(package))
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
        [package] => unresolvable(package),
        _ => request_level_failure(packages),
    })?;

    Ok(SolvedPackages {
        records: result.records,
        client,
    })
}

/// Reports one unresolvable package against the input the caller can
/// actually edit: its own command line, or its `.condarc`.
fn unresolvable(package: &PackageSpec) -> EphemeralEnvError {
    let label = package.safe_label();
    match package.origin() {
        PackageOrigin::Explicit => EphemeralEnvError::UnresolvablePackage { package: label },
        PackageOrigin::CreateDefaultPackages { .. } => {
            EphemeralEnvError::UnresolvableDefaultPackage { package: label }
        }
    }
}

/// Reports a solver failure that names no single culprit, still pointing
/// at `.condarc` when every requested package came from there. A mixed
/// set stays generic rather than blaming either source falsely.
fn request_level_failure(packages: &[PackageSpec]) -> EphemeralEnvError {
    let all_from_condarc = !packages.is_empty()
        && packages.iter().all(|package| {
            matches!(
                package.origin(),
                PackageOrigin::CreateDefaultPackages { .. }
            )
        });
    if all_from_condarc {
        EphemeralEnvError::UnresolvableDefaultPackages
    } else {
        EphemeralEnvError::ResolutionFailed
    }
}

/// Resolves a `<name>::<package>` channel qualifier to the concrete channel
/// URL the caller's own configuration designates, mirroring conda's measured
/// behavior (see `channel_qualifier_matrix` in this module's tests, which
/// encodes a conda 26.7.0 comparison as its oracle).
///
/// `MatchSpec::from_str` resolves a bare channel name against
/// `rattler_conda_types`' own default `channel_alias`
/// (`https://conda.anaconda.org`), with no way to inject `.condarc`'s
/// `channel_alias`/`custom_channels`/`custom_multichannels` mapping, so
/// without this a qualifier naming a configured channel resolves to a URL
/// that is not among the configured channels and matches nothing.
///
/// Resolution order, each step matching conda:
/// 1. A name the caller declared (`custom_channels`, `custom_multichannels`,
///    `defaults`, or a bare name joined to `channel_alias`) resolves to the
///    URL it was declared as. Declared identity wins over URL shape, so two
///    configured channels sharing a final path segment cannot be confused.
/// 2. Otherwise the qualifier is matched against the final path segment of a
///    configured channel URL, which is how conda resolves a qualifier naming
///    an entry that was configured as a URL and so declared no name.
/// 3. A qualifier that designates more than one *distinct* channel under
///    either step is REJECTED, because one match spec cannot express "any
///    one of these channels" and silently binding to one member would
///    install a package the caller did not ask for. Conda resolves these;
///    this is a deliberate, reported divergence rather than a silent guess.
///    Two candidates naming the same channel (a duplicated multichannel
///    member, or the same URL with/without a trailing slash) are not
///    "more than one" — they are deduplicated before this check, since
///    a config quirk that repeats one channel is not the same thing as a
///    name that genuinely designates several channels.
///
/// Every candidate comes from the post-filter channel list, so this can never
/// select a channel the caller's configuration does not already allow.
fn qualify_against_configured_channels(
    package: &PackageSpec,
    channels: &condarc::ResolvedChannels,
) -> Result<String, EphemeralEnvError> {
    let spec = package.as_str();
    let Some((qualifier, name)) = spec.split_once("::") else {
        return Ok(spec.to_string());
    };
    let declared = channels.channel_urls_by_name.get(qualifier);
    let candidates: Vec<&String> = match declared {
        Some(urls) => urls.iter().collect(),
        None => channels
            .channels
            .iter()
            .filter(|url| {
                url.trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .is_some_and(|segment| segment == qualifier)
            })
            .collect(),
    };
    let mut seen = std::collections::HashSet::new();
    let distinct: Vec<&String> = candidates
        .into_iter()
        .filter(|url| seen.insert(url.trim_end_matches('/')))
        .collect();
    match distinct.as_slice() {
        [] => Ok(spec.to_string()),
        [url] => Ok(format!("{}::{name}", url.trim_end_matches('/'))),
        _ => Err(EphemeralEnvError::AmbiguousChannelQualifier {
            qualifier: ambiguous_qualifier_label(package, qualifier),
        }),
    }
}

/// The value [`EphemeralEnvError::AmbiguousChannelQualifier`] reports for
/// `package`: the real qualifier text for an [`PackageOrigin::Explicit`]
/// (command-line) package, which is the caller's own argument and already
/// visible to them — or a position-only [`PackageSpec::safe_label`] for a
/// [`PackageOrigin::CreateDefaultPackages`] entry, which is never validated
/// and so may itself be, or embed, a credential no denylist-based
/// `redact_channel_url` pass can be guaranteed to catch (an unlisted
/// credential-key spelling bypasses it entirely; a positional label leaks
/// nothing at all, by construction).
fn ambiguous_qualifier_label(package: &PackageSpec, qualifier: &str) -> String {
    match package.origin() {
        PackageOrigin::Explicit => qualifier.to_string(),
        PackageOrigin::CreateDefaultPackages { .. } => package.safe_label(),
    }
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

    #[tokio::test]
    async fn solve_packages_empty_input_returns_no_records() {
        // Given
        let (_temporary_directory, root) = test_root();
        let fixture_directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ephemeral_channel");
        let channel = Channel::try_from_directory(&fixture_directory)
            .unwrap()
            .canonical_name();
        let config = condarc::ResolvedChannels::from_channels(vec![channel]);

        // When
        let solution = solve_packages(&root, &config, &[]).await.unwrap();

        // Then
        assert!(solution.records.is_empty());
    }

    #[tokio::test]
    async fn solve_packages_rejects_a_malformed_condarc_default_against_its_own_source() {
        // Given
        let (_temporary_directory, root) = test_root();
        let fixture_directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ephemeral_channel");
        let channel = Channel::try_from_directory(&fixture_directory)
            .unwrap()
            .canonical_name();
        let config = condarc::ResolvedChannels::from_channels(vec![channel]);
        let packages = [PackageSpec::from_resolved_default(
            "[[[not a spec".to_string(),
            0,
        )];

        // When
        let result = solve_packages(&root, &config, &packages).await;

        // Then
        assert_eq!(
            result.err(),
            Some(EphemeralEnvError::UnresolvableDefaultPackage {
                package: "<create_default_packages entry 1>".to_string()
            })
        );
    }

    #[tokio::test]
    async fn solve_packages_attributes_an_unsolvable_condarc_default_to_condarc() {
        // Given
        let (_temporary_directory, root) = test_root();
        let fixture_directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ephemeral_channel");
        let channel = Channel::try_from_directory(&fixture_directory)
            .unwrap()
            .canonical_name();
        let config = condarc::ResolvedChannels::from_channels(vec![channel]);
        let packages = [PackageSpec::from_resolved_default(
            "fixture-does-not-exist".to_string(),
            0,
        )];

        // When
        let result = solve_packages(&root, &config, &packages).await;

        // Then: an entry that parses but cannot be solved is still reported
        // against `.condarc`, and still by position rather than by its own
        // configured text.
        assert_eq!(
            result.err(),
            Some(EphemeralEnvError::UnresolvableDefaultPackage {
                package: "<create_default_packages entry 1>".to_string()
            })
        );
    }

    #[tokio::test]
    async fn solve_packages_resolves_a_channel_qualified_default_against_its_configured_channel() {
        // Given: a qualifier naming a channel the caller configured by name,
        // which rattler would otherwise resolve against its own default
        // channel alias and fail to match.
        let (_temporary_directory, root) = test_root();
        let fixture_directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ephemeral_channel");
        let channel = Channel::try_from_directory(&fixture_directory)
            .unwrap()
            .canonical_name();
        let mut config = condarc::ResolvedChannels::from_channels(vec![channel.clone()]);
        config.channel_urls_by_name = [("ephemeral_channel".to_string(), vec![channel])]
            .into_iter()
            .collect();
        let packages = [PackageSpec::from_resolved_default(
            "ephemeral_channel::fixture-default-alpha".to_string(),
            0,
        )];

        // When
        let solution = solve_packages(&root, &config, &packages).await.unwrap();

        // Then
        assert_eq!(solution.records.len(), 1);
        assert_eq!(
            solution.records[0].package_record.name.as_normalized(),
            "fixture-default-alpha"
        );
    }

    #[tokio::test]
    async fn solve_packages_rejects_a_qualifier_naming_an_unconfigured_channel() {
        // Given
        let (_temporary_directory, root) = test_root();
        let fixture_directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ephemeral_channel");
        let channel = Channel::try_from_directory(&fixture_directory)
            .unwrap()
            .canonical_name();
        let config = condarc::ResolvedChannels::from_channels(vec![channel]);
        let packages = [PackageSpec::from_resolved_default(
            "not-a-configured-channel::fixture-default-alpha".to_string(),
            0,
        )];

        // When
        let result = solve_packages(&root, &config, &packages).await;

        // Then: an unmatched qualifier is left alone and still fails, so
        // this rewrite can never widen configured channel access.
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn solve_packages_attributes_an_all_default_multi_package_failure_to_condarc() {
        // Given
        let (_temporary_directory, root) = test_root();
        let fixture_directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ephemeral_channel");
        let channel = Channel::try_from_directory(&fixture_directory)
            .unwrap()
            .canonical_name();
        let config = condarc::ResolvedChannels::from_channels(vec![channel]);
        let packages = [
            PackageSpec::from_resolved_default("fixture-default-alpha".to_string(), 0),
            PackageSpec::from_resolved_default("fixture-does-not-exist".to_string(), 1),
        ];

        // When
        let result = solve_packages(&root, &config, &packages).await;

        // Then
        assert_eq!(
            result.err(),
            Some(EphemeralEnvError::UnresolvableDefaultPackages)
        );
    }

    #[tokio::test]
    async fn solve_packages_keeps_a_mixed_origin_multi_package_failure_generic() {
        // Given
        let (_temporary_directory, root) = test_root();
        let fixture_directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ephemeral_channel");
        let channel = Channel::try_from_directory(&fixture_directory)
            .unwrap()
            .canonical_name();
        let config = condarc::ResolvedChannels::from_channels(vec![channel]);
        let packages = [
            PackageSpec::from_resolved_default("fixture-default-alpha".to_string(), 0),
            PackageSpec::parse("fixture-does-not-exist").unwrap(),
        ];

        // When
        let result = solve_packages(&root, &config, &packages).await;

        // Then: neither source is blamed when the culprit is ambiguous.
        assert_eq!(result.err(), Some(EphemeralEnvError::ResolutionFailed));
    }

    fn channels_named(urls: &[&str], named: &[(&str, &[&str])]) -> condarc::ResolvedChannels {
        let mut resolved = condarc::ResolvedChannels::from_channels(
            urls.iter().map(|url| (*url).to_string()).collect(),
        );
        resolved.channel_urls_by_name = named
            .iter()
            .map(|(name, name_urls)| {
                (
                    (*name).to_string(),
                    name_urls.iter().map(|url| (*url).to_string()).collect(),
                )
            })
            .collect();
        resolved
    }

    /// Conda's own measured behavior for `<channel>::<package>` qualifiers,
    /// captured from conda 26.7.0 against local file channels, used here as
    /// this resolver's oracle. Each row is (case, configured channel URLs,
    /// declared name mappings, qualified spec, expectation).
    ///
    /// `Ok(Some(url))` means conda resolved the qualifier to that channel;
    /// `Ok(None)` means conda left it unresolved (and so must we, letting the
    /// solver report it); `Err` marks the cases where conda resolves but one
    /// match spec cannot express the result, which this crate reports instead
    /// of guessing.
    #[test]
    fn channel_qualifier_matrix_matches_conda() {
        enum Expect {
            ResolvesTo(&'static str),
            Unresolved,
            Ambiguous,
        }
        use Expect::{Ambiguous, ResolvesTo, Unresolved};

        struct Case {
            name: &'static str,
            configured: &'static [&'static str],
            declared: &'static [(&'static str, &'static [&'static str])],
            spec: &'static str,
            expect: Expect,
        }

        let cases: &[Case] = &[
            Case {
                name: "custom_channels declares the name",
                configured: &["file:///base/ephemeral_channel"],
                declared: &[("ephemeral_channel", &["file:///base/ephemeral_channel"])],
                spec: "ephemeral_channel::numpy",
                expect: ResolvesTo("file:///base/ephemeral_channel::numpy"),
            },
            Case {
                name: "channel_alias joins a bare name",
                configured: &["https://conda.example.org/community"],
                declared: &[("community", &["https://conda.example.org/community"])],
                spec: "community::numpy",
                expect: ResolvesTo("https://conda.example.org/community::numpy"),
            },
            Case {
                name: "a URL entry declares no name, conda still matches its last segment",
                configured: &["file:///base/ephemeral_channel"],
                declared: &[],
                spec: "ephemeral_channel::numpy",
                expect: ResolvesTo("file:///base/ephemeral_channel::numpy"),
            },
            Case {
                name: "declared identity beats an unrelated same-segment URL",
                configured: &["file:///site-b/dup", "file:///site-a/dup"],
                declared: &[("dup", &["file:///site-a/dup"])],
                spec: "dup::fixture",
                expect: ResolvesTo("file:///site-a/dup::fixture"),
            },
            Case {
                name: "a single-member multichannel resolves",
                configured: &["file:///a/one"],
                declared: &[("bundle", &["file:///a/one"])],
                spec: "bundle::numpy",
                expect: ResolvesTo("file:///a/one::numpy"),
            },
            Case {
                name: "a multi-member multichannel is reported, never guessed",
                configured: &["file:///a/one", "file:///a/two"],
                declared: &[("bundle", &["file:///a/one", "file:///a/two"])],
                spec: "bundle::numpy",
                expect: Ambiguous,
            },
            Case {
                name: "two URL entries sharing a segment are reported, never guessed",
                configured: &["file:///site-a/dup", "file:///site-b/dup"],
                declared: &[],
                spec: "dup::fixture",
                expect: Ambiguous,
            },
            Case {
                name: "an unknown qualifier stays unresolved",
                configured: &["file:///base/ephemeral_channel"],
                declared: &[("ephemeral_channel", &["file:///base/ephemeral_channel"])],
                spec: "other::numpy",
                expect: Unresolved,
            },
            Case {
                name: "matching is case-sensitive, like condarc keys",
                configured: &["file:///base/ephemeral_channel"],
                declared: &[("ephemeral_channel", &["file:///base/ephemeral_channel"])],
                spec: "EPHEMERAL_CHANNEL::numpy",
                expect: Unresolved,
            },
            Case {
                name: "an unqualified spec is untouched",
                configured: &["file:///base/ephemeral_channel"],
                declared: &[],
                spec: "numpy=1.2",
                expect: Unresolved,
            },
            Case {
                name: "an absolute URL qualifier is already concrete",
                configured: &["file:///base/ephemeral_channel"],
                declared: &[],
                spec: "https://repo.example/chan::numpy",
                expect: Unresolved,
            },
            Case {
                name: "a trailing slash on the configured URL is normalized away",
                configured: &["file:///base/ephemeral_channel/"],
                declared: &[("ephemeral_channel", &["file:///base/ephemeral_channel/"])],
                spec: "ephemeral_channel::numpy",
                expect: ResolvesTo("file:///base/ephemeral_channel::numpy"),
            },
        ];

        for case in cases {
            // Given
            let channels = channels_named(case.configured, case.declared);
            let name = case.name;
            let package = PackageSpec::from_resolved_default(case.spec.to_string(), 0);

            // When
            let resolved = super::qualify_against_configured_channels(&package, &channels);

            // Then
            match case.expect {
                ResolvesTo(expected) => {
                    assert_eq!(resolved.as_deref(), Ok(expected), "case: {name}");
                }
                Unresolved => assert_eq!(resolved.as_deref(), Ok(case.spec), "case: {name}"),
                Ambiguous => assert!(
                    matches!(
                        resolved,
                        Err(EphemeralEnvError::AmbiguousChannelQualifier { .. })
                    ),
                    "case: {name}, got {resolved:?}"
                ),
            }
        }
    }

    #[test]
    fn qualify_against_configured_channels_deduplicates_a_repeated_multichannel_member() {
        // Given: a config quirk, not two distinct channels — one URL
        // declared twice under the same multichannel name.
        let channels = channels_named(
            &["file:///a/one"],
            &[("bundle", &["file:///a/one", "file:///a/one"])],
        );
        let package = PackageSpec::parse("bundle::numpy").unwrap();

        // When
        let resolved = super::qualify_against_configured_channels(&package, &channels);

        // Then
        assert_eq!(resolved.as_deref(), Ok("file:///a/one::numpy"));
    }

    #[test]
    fn qualify_against_configured_channels_deduplicates_trailing_slash_variants() {
        // Given: the same channel declared with and without a trailing
        // slash under one multichannel name.
        let channels = channels_named(
            &["file:///a/one"],
            &[("bundle", &["file:///a/one", "file:///a/one/"])],
        );
        let package = PackageSpec::parse("bundle::numpy").unwrap();

        // When
        let resolved = super::qualify_against_configured_channels(&package, &channels);

        // Then
        assert_eq!(resolved.as_deref(), Ok("file:///a/one::numpy"));
    }

    #[test]
    fn qualify_against_configured_channels_reports_a_safe_label_for_an_ambiguous_default_entry() {
        // Given: an ambiguous qualifier sourced from `create_default_packages`,
        // which is never validated and so could itself embed a credential no
        // denylist-based redaction is guaranteed to catch.
        let channels = channels_named(
            &["file:///a/one", "file:///a/two"],
            &[("bundle", &["file:///a/one", "file:///a/two"])],
        );
        let package = PackageSpec::from_resolved_default("bundle::numpy".to_string(), 2);

        // When
        let result = super::qualify_against_configured_channels(&package, &channels);

        // Then: the reported qualifier is a position label, never raw text.
        assert_eq!(
            result,
            Err(EphemeralEnvError::AmbiguousChannelQualifier {
                qualifier: "<create_default_packages entry 3>".to_string()
            })
        );
    }

    #[test]
    fn qualify_against_configured_channels_reports_the_real_qualifier_for_an_ambiguous_explicit_package()
     {
        // Given: the same ambiguity, but the package was named on the
        // command line, where showing the caller's own argument is safe.
        let channels = channels_named(
            &["file:///a/one", "file:///a/two"],
            &[("bundle", &["file:///a/one", "file:///a/two"])],
        );
        let package = PackageSpec::parse("bundle::numpy").unwrap();

        // When
        let result = super::qualify_against_configured_channels(&package, &channels);

        // Then
        assert_eq!(
            result,
            Err(EphemeralEnvError::AmbiguousChannelQualifier {
                qualifier: "bundle".to_string()
            })
        );
    }

    #[tokio::test]
    async fn solve_packages_reports_an_ambiguous_qualifier_without_installing_a_member() {
        // Given: two configured channels share a final path segment, so the
        // qualifier designates both. The colliding package comes from
        // `create_default_packages`, so the reported qualifier must be a
        // safe position label, not the raw text.
        let (_temporary_directory, root) = test_root();
        let fixture_directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ephemeral_channel");
        let a = Channel::try_from_directory(&fixture_directory.join("priority-a"))
            .unwrap()
            .canonical_name();
        let b = Channel::try_from_directory(&fixture_directory.join("priority-b"))
            .unwrap()
            .canonical_name();
        let mut config = condarc::ResolvedChannels::from_channels(vec![a.clone(), b.clone()]);
        config.channel_urls_by_name = [("bundle".to_string(), vec![a, b])].into_iter().collect();
        let packages = [PackageSpec::from_resolved_default(
            "bundle::fixture-priority".to_string(),
            0,
        )];

        // When
        let result = solve_packages(&root, &config, &packages).await;

        // Then
        assert_eq!(
            result.err(),
            Some(EphemeralEnvError::AmbiguousChannelQualifier {
                qualifier: "<create_default_packages entry 1>".to_string()
            })
        );
    }

    #[test]
    fn request_level_failure_on_an_empty_slice_is_not_attributed_to_condarc() {
        // Given/When
        let error = super::request_level_failure(&[]);

        // Then
        assert_eq!(error, EphemeralEnvError::ResolutionFailed);
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
