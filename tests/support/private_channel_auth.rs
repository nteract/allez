use std::{
    fs,
    path::{Path, PathBuf},
};

use allez::ephemeral::{EphemeralEnvError, create_ephemeral_environment};
use condarc::ResolvedChannels;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

use crate::support::{
    ChannelTokenGuard, PrivateChannelFixture, TestContext, installed_names, package_specs,
};

const CHANNEL_SUBDIRECTORIES: &[&str] = &[
    "noarch",
    "linux-64",
    "linux-aarch64",
    "osx-arm64",
    "win-64",
    "win-arm64",
];

fn fixture_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ephemeral_channel")
}

async fn mount_fixture_channel(mock_server: &MockServer) {
    mount_fixture_repodata(mock_server).await;
    mount_fixture_package(mock_server).await;
}

async fn mount_fixture_repodata(mock_server: &MockServer) {
    for subdirectory in CHANNEL_SUBDIRECTORIES {
        let repodata =
            fs::read(fixture_directory().join(subdirectory).join("repodata.json")).unwrap();
        Mock::given(path(format!("/{subdirectory}/repodata.json")))
            .respond_with(ResponseTemplate::new(200).set_body_raw(repodata, "application/json"))
            .mount(mock_server)
            .await;
    }
}

async fn mount_fixture_package(mock_server: &MockServer) {
    let package = fs::read(
        fixture_directory()
            .join("noarch")
            .join("fixture-default-alpha-1.0.0-0.tar.bz2"),
    )
    .unwrap();
    Mock::given(path("/noarch/fixture-default-alpha-1.0.0-0.tar.bz2"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(package))
        .mount(mock_server)
        .await;
}

async fn mount_repodata_rejection(mock_server: &MockServer, status: u16) {
    for subdirectory in CHANNEL_SUBDIRECTORIES {
        Mock::given(path(format!("/{subdirectory}/repodata.json")))
            .respond_with(ResponseTemplate::new(status))
            .mount(mock_server)
            .await;
    }
}

async fn mount_package_rejection(mock_server: &MockServer, status: u16) {
    Mock::given(path("/noarch/fixture-default-alpha-1.0.0-0.tar.bz2"))
        .respond_with(ResponseTemplate::new(status))
        .mount(mock_server)
        .await;
}

fn config_for_private_channel(fixture: &PrivateChannelFixture) -> ResolvedChannels {
    let mut config = ResolvedChannels::from_channels(vec![fixture.channel.clone()]);
    config.channel_settings = vec![fixture.channel_setting.clone()];
    config
}

fn files_under(directory: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(files_under(&path));
        } else if path.is_file() {
            files.push(path);
        }
    }
    files
}

fn has_authorization(request: &wiremock::Request, token: &str) -> bool {
    request
        .headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        == Some(token)
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn private_channel_requests_use_the_raw_token_and_install_the_package() {
    // Given
    let _context = TestContext::new("private-channel-authenticated-install");
    let token = "private-channel-token";
    let fixture = PrivateChannelFixture::new(token).await;
    mount_fixture_channel(&fixture.mock_server).await;

    // When
    let ready = create_ephemeral_environment(
        package_specs(&["fixture-default-alpha"]),
        config_for_private_channel(&fixture),
        None,
    )
    .await
    .unwrap();
    let requests = fixture.mock_server.received_requests().await.unwrap();

    // Then
    assert_eq!(installed_names(&ready), ["fixture-default-alpha"].into());
    assert!(!requests.is_empty());
    assert!(
        requests
            .iter()
            .all(|request| has_authorization(request, token))
    );
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn private_channel_requests_are_authenticated_while_public_channel_requests_are_not() {
    // Given
    let _context = TestContext::new("private-and-public-channel-auth");
    let token = "private-only-token";
    let fixture = PrivateChannelFixture::new(token).await;
    let public_server = MockServer::start().await;
    mount_fixture_channel(&fixture.mock_server).await;
    mount_fixture_channel(&public_server).await;
    let mut config =
        ResolvedChannels::from_channels(vec![fixture.channel.clone(), public_server.uri()]);
    config.channel_settings = vec![fixture.channel_setting.clone()];

    // When
    let ready =
        create_ephemeral_environment(package_specs(&["fixture-default-alpha"]), config, None)
            .await
            .unwrap();
    let private_requests = fixture.mock_server.received_requests().await.unwrap();
    let public_requests = public_server.received_requests().await.unwrap();

    // Then
    assert_eq!(installed_names(&ready), ["fixture-default-alpha"].into());
    assert!(!private_requests.is_empty());
    assert!(!public_requests.is_empty());
    assert!(
        private_requests
            .iter()
            .all(|request| has_authorization(request, token))
    );
    assert!(
        public_requests
            .iter()
            .all(|request| request.headers.get("authorization").is_none())
    );
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn unmarked_channel_installs_without_a_token_or_authorization_header() {
    // Given
    let _context = TestContext::new("unmarked-public-channel");
    let _token_guard = ChannelTokenGuard::unset();
    let public_server = MockServer::start().await;
    mount_fixture_channel(&public_server).await;
    let config = ResolvedChannels::from_channels(vec![public_server.uri()]);

    // When
    let ready =
        create_ephemeral_environment(package_specs(&["fixture-default-alpha"]), config, None)
            .await
            .unwrap();
    let requests = public_server.received_requests().await.unwrap();

    // Then
    assert_eq!(installed_names(&ready), ["fixture-default-alpha"].into());
    assert!(!requests.is_empty());
    assert!(
        requests
            .iter()
            .all(|request| request.headers.get("authorization").is_none())
    );
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn private_channel_without_a_token_fails_before_making_a_request() {
    // Given
    let _context = TestContext::new("private-channel-missing-token");
    let fixture = PrivateChannelFixture::new("unused-token").await;
    let _token_guard = ChannelTokenGuard::unset();
    mount_fixture_channel(&fixture.mock_server).await;

    // When
    let failure = create_ephemeral_environment(
        package_specs(&["fixture-default-alpha"]),
        config_for_private_channel(&fixture),
        None,
    )
    .await
    .unwrap_err();
    let requests = fixture.mock_server.received_requests().await.unwrap();

    // Then
    assert_eq!(failure.error, EphemeralEnvError::MissingChannelToken);
    assert!(requests.is_empty());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn private_channel_with_an_empty_token_fails_before_making_a_request() {
    // Given
    let _context = TestContext::new("private-channel-empty-token");
    let fixture = PrivateChannelFixture::new("unused-token").await;
    let _token_guard = ChannelTokenGuard::set("");
    mount_fixture_channel(&fixture.mock_server).await;

    // When
    let failure = create_ephemeral_environment(
        package_specs(&["fixture-default-alpha"]),
        config_for_private_channel(&fixture),
        None,
    )
    .await
    .unwrap_err();
    let requests = fixture.mock_server.received_requests().await.unwrap();

    // Then
    assert_eq!(failure.error, EphemeralEnvError::MissingChannelToken);
    assert!(requests.is_empty());
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn private_channel_repodata_401_is_an_authentication_failure() {
    // Given
    let _context = TestContext::new("private-channel-repodata-401");
    let fixture = PrivateChannelFixture::new("rejected-token").await;
    mount_repodata_rejection(&fixture.mock_server, 401).await;

    // When
    let failure = create_ephemeral_environment(
        package_specs(&["fixture-default-alpha"]),
        config_for_private_channel(&fixture),
        None,
    )
    .await
    .unwrap_err();

    // Then
    assert_eq!(
        failure.error,
        EphemeralEnvError::ChannelAuthenticationFailed {
            channel: fixture.mock_server.uri(),
        }
    );
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn private_channel_repodata_403_is_an_authentication_failure() {
    // Given
    let _context = TestContext::new("private-channel-repodata-403");
    let fixture = PrivateChannelFixture::new("rejected-token").await;
    mount_repodata_rejection(&fixture.mock_server, 403).await;

    // When
    let failure = create_ephemeral_environment(
        package_specs(&["fixture-default-alpha"]),
        config_for_private_channel(&fixture),
        None,
    )
    .await
    .unwrap_err();

    // Then
    assert_eq!(
        failure.error,
        EphemeralEnvError::ChannelAuthenticationFailed {
            channel: fixture.mock_server.uri(),
        }
    );
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn public_channel_repodata_401_is_not_an_authentication_failure() {
    // Given
    let _context = TestContext::new("public-channel-repodata-401");
    let _token_guard = ChannelTokenGuard::unset();
    let public_server = MockServer::start().await;
    mount_repodata_rejection(&public_server, 401).await;
    let config = ResolvedChannels::from_channels(vec![public_server.uri()]);

    // When
    let failure =
        create_ephemeral_environment(package_specs(&["fixture-default-alpha"]), config, None)
            .await
            .unwrap_err();

    // Then
    assert!(matches!(
        failure.error,
        EphemeralEnvError::ResolutionFailed | EphemeralEnvError::UnresolvablePackage { .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn public_channel_repodata_403_is_not_an_authentication_failure() {
    // Given
    let _context = TestContext::new("public-channel-repodata-403");
    let _token_guard = ChannelTokenGuard::unset();
    let public_server = MockServer::start().await;
    mount_repodata_rejection(&public_server, 403).await;
    let config = ResolvedChannels::from_channels(vec![public_server.uri()]);

    // When
    let failure =
        create_ephemeral_environment(package_specs(&["fixture-default-alpha"]), config, None)
            .await
            .unwrap_err();

    // Then
    assert!(matches!(
        failure.error,
        EphemeralEnvError::ResolutionFailed | EphemeralEnvError::UnresolvablePackage { .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn private_channel_package_download_401_is_an_authentication_failure() {
    // Given
    let _context = TestContext::new("private-channel-package-download-401");
    let fixture = PrivateChannelFixture::new("rejected-token").await;
    mount_fixture_repodata(&fixture.mock_server).await;
    mount_package_rejection(&fixture.mock_server, 401).await;

    // When
    let failure = create_ephemeral_environment(
        package_specs(&["fixture-default-alpha"]),
        config_for_private_channel(&fixture),
        None,
    )
    .await
    .unwrap_err();

    // Then
    assert_eq!(
        failure.error,
        EphemeralEnvError::ChannelAuthenticationFailed {
            channel: fixture.mock_server.uri(),
        }
    );
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn successful_private_channel_install_never_persists_the_token() {
    // Given
    let _context = TestContext::new("private-channel-token-not-persisted");
    let token = "token-that-must-not-reach-disk";
    let fixture = PrivateChannelFixture::new(token).await;
    mount_fixture_channel(&fixture.mock_server).await;

    // When
    let ready = create_ephemeral_environment(
        package_specs(&["fixture-default-alpha"]),
        config_for_private_channel(&fixture),
        None,
    )
    .await
    .unwrap();
    let root = ready.location.parent().and_then(Path::parent).unwrap();

    // Then
    for file in files_under(root) {
        let contents = fs::read(&file).unwrap();
        assert!(
            !contents
                .windows(token.len())
                .any(|window| window == token.as_bytes()),
            "token was persisted to {}",
            file.display()
        );
    }
}
