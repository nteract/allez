//! Private channel authentication plumbing.

use rattler_cache::package_cache::{PackageCacheError, PackageCacheLayerError};
use rattler_package_streaming::ExtractError;
use reqwest::header::AUTHORIZATION;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Middleware, Next};

use super::error::EphemeralEnvError;

/// The environment variable supplying the raw private-channel credential.
pub(crate) const CHANNEL_TOKEN_ENV_VAR: &str = "ALLEZ_CHANNEL_TOKEN";

/// A stable `User-Agent`, distinct from `reqwest`'s own default of sending
/// none at all: `repo.anaconda.com`'s CDN has been observed rejecting
/// requests carrying no `User-Agent` header with an HTTP 403 (confirmed
/// empirically), even though the exact same request with any identifying
/// `User-Agent` succeeds. Every HTTP request this feature makes -- both
/// repodata queries in `solve.rs` and package downloads in `install.rs`,
/// which reuse this same client -- goes through this one client, so
/// setting it once here covers both.
pub(crate) const HTTP_USER_AGENT: &str = concat!("allez/", env!("CARGO_PKG_VERSION"));

/// The replacement returned when a configured channel cannot safely be reduced
/// to an HTTP origin.
const UNPARSEABLE_CHANNEL: &str = "<unparseable channel>";

/// An HTTP status failure and the request URL that produced it.
pub(super) struct HttpFailure {
    /// The HTTP status reported by `reqwest`.
    pub(super) status: reqwest::StatusCode,
    /// The request URL retained by `reqwest`, when available.
    pub(super) url: Option<reqwest::Url>,
}

/// Returns the subset of `channels` a `channel_settings` entry both names
/// (exact match or `/*`-prefix match) and marks as needing the token (an
/// `auth` key present, any value). A channel with no matching entry is
/// always excluded, regardless of its own URL scheme or host.
pub(super) fn classify_private_channels(
    channels: &[String],
    channel_settings: &[condarc::ChannelSetting],
) -> Vec<String> {
    channels
        .iter()
        .filter(|channel| {
            channel_settings.iter().any(|setting| {
                let Some(setting_channel) = setting.0.get("channel") else {
                    return false;
                };
                if !setting.0.contains_key("auth") {
                    return false;
                }

                normalize_channel(setting_channel) == normalize_channel(channel)
                    || (setting_channel.ends_with("/*")
                        && setting_channel
                            .strip_suffix('*')
                            .is_some_and(|prefix| channel.starts_with(prefix)))
            })
        })
        .map(|channel| normalize_channel(channel).to_string())
        .collect()
}

fn normalize_channel(channel: &str) -> &str {
    channel.strip_suffix('/').unwrap_or(channel)
}

/// Returns the specific `private_channels` entry `url` falls under — equal
/// to it, or having it as a path-bounded prefix — or `None` if no entry
/// matches. Tries an exact/prefix string comparison first, then a
/// normalized origin+path comparison as a fallback (see
/// [`matches_normalized_origin_and_path`]).
pub(super) fn matching_private_channel<'a>(
    url: &reqwest::Url,
    private_channels: &'a [String],
) -> Option<&'a str> {
    let raw_url = url.as_str();
    private_channels.iter().find_map(|channel| {
        (raw_url == channel
            || raw_url
                .strip_prefix(channel.as_str())
                .is_some_and(|suffix| suffix.starts_with('/'))
            || matches_normalized_origin_and_path(url, channel))
        .then_some(channel.as_str())
    })
}

/// Falls back to structural origin+path comparison when the raw-string
/// comparison above misses due to a `reqwest::Url`-normalization difference
/// the operator's own `channel_settings` entry didn't anticipate (for
/// example, an explicit default port `Url` itself omits when serializing).
/// Deliberately skipped for opaque origins (e.g. `file://`): two separately
/// parsed opaque origins are never equal even for the identical input, so
/// this fallback would otherwise never match a `file://` channel at all —
/// the raw-string comparison above already covers that case correctly.
fn matches_normalized_origin_and_path(url: &reqwest::Url, channel: &str) -> bool {
    let Ok(parsed_channel) = reqwest::Url::parse(channel) else {
        return false;
    };
    if !url.origin().is_tuple() || url.origin() != parsed_channel.origin() {
        return false;
    }
    let channel_path = parsed_channel.path().trim_end_matches('/');
    let url_path = url.path();
    url_path == channel_path
        || url_path
            .strip_prefix(channel_path)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

/// Finds the first HTTP response failure represented in an error chain.
pub(super) fn reqwest_http_failure(
    error: &(dyn std::error::Error + 'static),
) -> Option<HttpFailure> {
    let mut current_error = Some(error);

    while let Some(error) = current_error {
        if let Some(reqwest_error) = error.downcast_ref::<reqwest::Error>()
            && let Some(failure) = http_failure_from_reqwest_error(reqwest_error)
        {
            return Some(failure);
        }

        current_error = error.source();
    }

    None
}

/// Builds an [`HttpFailure`] from a `reqwest::Error`, or `None` when it
/// carries no HTTP status (for example a connect/timeout error).
fn http_failure_from_reqwest_error(error: &reqwest::Error) -> Option<HttpFailure> {
    Some(HttpFailure {
        status: error.status()?,
        url: error.url().cloned(),
    })
}

/// As [`http_failure_from_reqwest_error`], for the wrapping
/// `reqwest_middleware::Error` type, which exposes the same `status()`/
/// `url()` accessors directly rather than requiring an inner
/// `reqwest::Error` downcast.
fn http_failure_from_reqwest_middleware_error(
    error: &reqwest_middleware::Error,
) -> Option<HttpFailure> {
    Some(HttpFailure {
        status: error.status()?,
        url: error.url().cloned(),
    })
}

/// Finds an HTTP response failure nested inside `rattler_cache`'s
/// package-download error chain (`install.rs`'s `InstallerError::FailedToFetch`
/// payload). `PackageCacheError`/`PackageCacheLayerError`'s own
/// `#[error(transparent)]` variants make `Error::source()` skip straight past
/// the concrete error value they wrap and return *that value's own* source
/// instead — so [`reqwest_http_failure`]'s generic chain walk can never
/// downcast to the `reqwest_middleware::Error` underneath; this function
/// downcasts through the concrete container types directly instead.
pub(super) fn package_cache_http_failure(error: &PackageCacheError) -> Option<HttpFailure> {
    let PackageCacheError::LayerError(layer_error) = error else {
        return None;
    };
    let layer_error: &dyn std::error::Error = &**layer_error;
    let PackageCacheLayerError::FetchError(fetch_error) =
        layer_error.downcast_ref::<PackageCacheLayerError>()?
    else {
        return None;
    };
    let fetch_error: &dyn std::error::Error = &**fetch_error;
    let ExtractError::ReqwestError(reqwest_error) = fetch_error.downcast_ref::<ExtractError>()?
    else {
        return None;
    };

    http_failure_from_reqwest_middleware_error(reqwest_error)
}

/// Finds an HTTP response failure in a `rattler_repodata_gateway::GatewayError`
/// returned by `Gateway::query(...).execute()`. Tries the generic
/// [`reqwest_http_failure`] chain walk first; `GatewayError::ReqwestError`
/// and `GatewayError::FetchRepoDataError(FetchRepoDataError::HttpError(_))`
/// are both themselves `#[error(transparent)]`, so `Error::source()` skips
/// past the `reqwest`/`reqwest_middleware::Error` value they wrap — this
/// pattern-matches those two known shapes directly as a fallback.
pub(super) fn gateway_http_failure(
    error: &rattler_repodata_gateway::GatewayError,
) -> Option<HttpFailure> {
    reqwest_http_failure(error).or_else(|| match error {
        rattler_repodata_gateway::GatewayError::ReqwestError(reqwest_error) => {
            http_failure_from_reqwest_error(reqwest_error)
        }
        rattler_repodata_gateway::GatewayError::FetchRepoDataError(
            rattler_repodata_gateway::fetch::FetchRepoDataError::HttpError(
                reqwest_middleware::Error::Reqwest(reqwest_error),
            ),
        ) => http_failure_from_reqwest_error(reqwest_error),
        _ => None,
    })
}

/// Maps an [`HttpFailure`] to [`EphemeralEnvError::ChannelAuthenticationFailed`]
/// when its status is 401 or 403 and its URL falls under one of
/// `private_channels`; every other outcome — including a non-auth status, a
/// missing URL, or a URL outside every private channel (a public channel's
/// own 401/403) — returns `None`, leaving the caller's existing fallback
/// categorization (e.g. `ResolutionFailed`) untouched.
pub(super) fn channel_authentication_failure(
    http_failure: Option<HttpFailure>,
    private_channels: &[String],
) -> Option<EphemeralEnvError> {
    let HttpFailure {
        status,
        url: Some(url),
    } = http_failure?
    else {
        return None;
    };
    if !matches!(
        status,
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        return None;
    }

    let channel = matching_private_channel(&url, private_channels)?;
    Some(EphemeralEnvError::ChannelAuthenticationFailed {
        channel: origin_only(channel),
    })
}

/// Reduces a channel URL to its tuple origin without retaining path content.
pub(super) fn origin_only(channel: &str) -> String {
    let Ok(url) = reqwest::Url::parse(channel) else {
        return UNPARSEABLE_CHANNEL.to_string();
    };
    let origin = url.origin();

    if origin.is_tuple() {
        origin.ascii_serialization()
    } else {
        UNPARSEABLE_CHANNEL.to_string()
    }
}

/// Reads `ALLEZ_CHANNEL_TOKEN` and converts it to a sensitive
/// `http::HeaderValue`. Returns `None` when unset, empty, or not
/// representable as a header value. Re-reads the environment on every
/// call; never caches the token (FR-007).
pub(super) fn read_channel_token_header() -> Option<http::HeaderValue> {
    let token = std::env::var(CHANNEL_TOKEN_ENV_VAR).ok()?;
    if token.is_empty() {
        return None;
    }

    let mut header = http::HeaderValue::from_str(&token).ok()?;
    header.set_sensitive(true);
    Some(header)
}

struct PrivateChannelAuthMiddleware {
    private_channels: Vec<String>,
    token_header: http::HeaderValue,
}

#[async_trait::async_trait]
impl Middleware for PrivateChannelAuthMiddleware {
    async fn handle(
        &self,
        mut req: reqwest::Request,
        extensions: &mut http::Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<reqwest::Response> {
        if matching_private_channel(req.url(), &self.private_channels).is_some() {
            req.headers_mut()
                .insert(AUTHORIZATION, self.token_header.clone());
        }

        next.run(req, extensions).await
    }
}

/// Builds the shared HTTP client `solve.rs`'s `Gateway` and `install.rs`'s
/// `Installer` both use, wrapping it with [`PrivateChannelAuthMiddleware`]
/// only when `private_channels` is non-empty. Returns
/// `Err(MissingChannelToken)` when `private_channels` is non-empty and
/// [`read_channel_token_header`] returns `None` — before any client or
/// network request is built.
pub(super) fn build_channel_auth_client(
    private_channels: Vec<String>,
) -> Result<ClientWithMiddleware, EphemeralEnvError> {
    let token_header = if private_channels.is_empty() {
        None
    } else {
        Some(read_channel_token_header().ok_or(EphemeralEnvError::MissingChannelToken)?)
    };
    let base_client = reqwest::Client::builder()
        .no_proxy()
        .user_agent(HTTP_USER_AGENT)
        .build()
        .map_err(|_| EphemeralEnvError::ResolutionFailed)?;
    let builder = ClientBuilder::new(base_client);

    match token_header {
        None => Ok(builder.build()),
        Some(token_header) => Ok(builder
            .with(PrivateChannelAuthMiddleware {
                private_channels,
                token_header,
            })
            .build()),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, env, ffi::OsString};

    use condarc::ChannelSetting;
    use reqwest::header::AUTHORIZATION;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

    use super::{
        CHANNEL_TOKEN_ENV_VAR, HttpFailure, UNPARSEABLE_CHANNEL, channel_authentication_failure,
        classify_private_channels, gateway_http_failure, matching_private_channel, origin_only,
        package_cache_http_failure, read_channel_token_header, reqwest_http_failure,
    };

    #[derive(Debug)]
    struct ErrorWithSource {
        source: reqwest::Error,
    }

    impl std::fmt::Display for ErrorWithSource {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("wrapped request error")
        }
    }

    impl std::error::Error for ErrorWithSource {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.source)
        }
    }

    async fn status_error(status: u16) -> (reqwest::Error, reqwest::Url) {
        let mock_server = MockServer::start().await;
        Mock::given(path("/private/noarch/repodata.json"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&mock_server)
            .await;
        let request_url = format!("{}/private/noarch/repodata.json", mock_server.uri());
        let error = reqwest::Client::new()
            .get(&request_url)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap_err();

        (error, request_url.parse().unwrap())
    }

    struct ChannelTokenGuard {
        prior_value: Option<OsString>,
    }

    impl ChannelTokenGuard {
        fn set(value: &str) -> Self {
            let prior_value = env::var_os(CHANNEL_TOKEN_ENV_VAR);

            // SAFETY: These tests hold serial_test's process-wide lock.
            unsafe {
                env::set_var(CHANNEL_TOKEN_ENV_VAR, value);
            }

            Self { prior_value }
        }
    }

    impl Drop for ChannelTokenGuard {
        fn drop(&mut self) {
            // SAFETY: These tests hold serial_test's process-wide lock.
            unsafe {
                match &self.prior_value {
                    Some(value) => env::set_var(CHANNEL_TOKEN_ENV_VAR, value),
                    None => env::remove_var(CHANNEL_TOKEN_ENV_VAR),
                }
            }
        }
    }

    fn channel_setting(channel: &str, has_auth: bool) -> ChannelSetting {
        let mut setting = BTreeMap::from([("channel".to_string(), channel.to_string())]);
        if has_auth {
            setting.insert("auth".to_string(), "token".to_string());
        }
        ChannelSetting(setting)
    }

    #[test]
    fn classify_private_channels_accepts_exact_http_matches_with_auth() {
        // Given
        let channels = vec!["http://packages.example.test/private/".to_string()];
        let settings = vec![channel_setting(
            "http://packages.example.test/private",
            true,
        )];

        // When
        let private_channels = classify_private_channels(&channels, &settings);

        // Then
        assert_eq!(
            private_channels,
            vec!["http://packages.example.test/private".to_string()]
        );
    }

    #[test]
    fn classify_private_channels_requires_an_auth_key() {
        // Given
        let channels = vec!["https://packages.example.test/private".to_string()];
        let settings = vec![channel_setting(
            "https://packages.example.test/private",
            false,
        )];

        // When
        let private_channels = classify_private_channels(&channels, &settings);

        // Then
        assert!(private_channels.is_empty());
    }

    #[test]
    fn classify_private_channels_matches_only_wildcard_path_descendants() {
        // Given
        let channels = vec![
            "https://packages.example.test/prefix/sub/path".to_string(),
            "https://packages.example.test/prefixed-differently".to_string(),
        ];
        let settings = vec![channel_setting(
            "https://packages.example.test/prefix/*",
            true,
        )];

        // When
        let private_channels = classify_private_channels(&channels, &settings);

        // Then
        assert_eq!(
            private_channels,
            vec!["https://packages.example.test/prefix/sub/path".to_string()]
        );
    }

    #[test]
    fn classify_private_channels_leaves_unmarked_channels_public() {
        // Given
        let channels = vec!["https://packages.example.test/unmarked".to_string()];
        let settings = Vec::new();

        // When
        let private_channels = classify_private_channels(&channels, &settings);

        // Then
        assert!(private_channels.is_empty());
    }

    #[test]
    fn classify_private_channels_treats_a_star_without_a_preceding_slash_as_a_literal_suffix() {
        // Given: `myorg*` (no slash before the `*`) is never a prefix
        // pattern -- it only participates in the exact-match branch, which
        // it can never satisfy against a real channel URL, so it simply
        // never matches (research.md Decision 1).
        let channels = vec!["https://packages.example.test/myorg2".to_string()];
        let settings = vec![channel_setting(
            "https://packages.example.test/myorg*",
            true,
        )];

        // When
        let private_channels = classify_private_channels(&channels, &settings);

        // Then
        assert!(private_channels.is_empty());
    }

    #[test]
    fn matching_private_channel_matches_its_base_and_descendants() {
        // Given
        let private_channels = vec!["https://packages.example.test/org".to_string()];
        let base = "https://packages.example.test/org".parse().unwrap();
        let repodata = "https://packages.example.test/org/noarch/repodata.json"
            .parse()
            .unwrap();
        let package = "https://packages.example.test/org/pkg.tar.bz2"
            .parse()
            .unwrap();

        // When
        let base_match = matching_private_channel(&base, &private_channels);
        let repodata_match = matching_private_channel(&repodata, &private_channels);
        let package_match = matching_private_channel(&package, &private_channels);

        // Then
        assert_eq!(base_match, Some("https://packages.example.test/org"));
        assert_eq!(repodata_match, Some("https://packages.example.test/org"));
        assert_eq!(package_match, Some("https://packages.example.test/org"));
    }

    #[test]
    fn matching_private_channel_rejects_non_member_urls() {
        // Given
        let private_channels = vec!["https://packages.example.test:8443/org".to_string()];
        let sibling = "https://packages.example.test:8443/organization/repodata.json"
            .parse()
            .unwrap();
        let different_scheme = "http://packages.example.test:8443/org/repodata.json"
            .parse()
            .unwrap();
        let different_host = "https://other.example.test:8443/org/repodata.json"
            .parse()
            .unwrap();
        let different_port = "https://packages.example.test:9443/org/repodata.json"
            .parse()
            .unwrap();

        // When
        let matches = [sibling, different_scheme, different_host, different_port]
            .iter()
            .map(|url| matching_private_channel(url, &private_channels))
            .collect::<Vec<_>>();

        // Then
        assert_eq!(matches, vec![None, None, None, None]);
    }

    #[tokio::test]
    async fn reqwest_http_failure_finds_a_top_level_error() {
        // Given
        let (error, request_url) = status_error(401).await;

        // When
        let failure = reqwest_http_failure(&error).unwrap();

        // Then
        assert_eq!(failure.status, reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(failure.url, Some(request_url));
    }

    #[tokio::test]
    async fn reqwest_http_failure_finds_a_nested_error() {
        // Given
        let (source, request_url) = status_error(403).await;
        let error = ErrorWithSource { source };

        // When
        let failure = reqwest_http_failure(&error).unwrap();

        // Then
        assert_eq!(failure.status, reqwest::StatusCode::FORBIDDEN);
        assert_eq!(failure.url, Some(request_url));
    }

    #[test]
    fn reqwest_http_failure_returns_none_without_a_reqwest_error() {
        // Given
        let error = std::io::Error::other("not an HTTP error");

        // When
        let failure = reqwest_http_failure(&error);

        // Then
        assert!(failure.is_none());
    }

    #[tokio::test]
    async fn package_cache_http_failure_downcasts_through_the_cache_layer_chain() {
        // Given
        let (reqwest_error, request_url) = status_error(401).await;
        let extract_error = rattler_package_streaming::ExtractError::ReqwestError(
            reqwest_middleware::Error::Reqwest(reqwest_error),
        );
        let layer_error = rattler_cache::package_cache::PackageCacheLayerError::FetchError(
            std::sync::Arc::new(extract_error),
        );
        let cache_error =
            rattler_cache::package_cache::PackageCacheError::LayerError(Box::new(layer_error));

        // When
        let failure = package_cache_http_failure(&cache_error).unwrap();

        // Then
        assert_eq!(failure.status, reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(failure.url, Some(request_url));
    }

    #[test]
    fn package_cache_http_failure_returns_none_for_a_non_layer_variant() {
        // Given
        let error = rattler_cache::package_cache::PackageCacheError::NoWritableLayers;

        // When
        let failure = package_cache_http_failure(&error);

        // Then
        assert!(failure.is_none());
    }

    #[tokio::test]
    async fn gateway_http_failure_finds_a_top_level_reqwest_error_variant() {
        // Given: `GatewayError::ReqwestError` is the shape `Gateway::execute()`
        // returns for the sharded-repodata-index fetch path (observed via the
        // real CLI against a private channel returning 401) -- distinct from
        // the `FetchRepoDataError::HttpError` shape the plain `repodata.json`
        // fetch path uses, and, like it, `#[error(transparent)]`, so
        // `reqwest_http_failure`'s generic chain walk alone cannot reach it.
        let (reqwest_error, request_url) = status_error(401).await;
        let error = rattler_repodata_gateway::GatewayError::ReqwestError(reqwest_error);

        // When
        let failure = gateway_http_failure(&error).unwrap();

        // Then
        assert_eq!(failure.status, reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(failure.url, Some(request_url));
    }

    #[test]
    fn gateway_http_failure_returns_none_for_a_non_http_variant() {
        // Given
        let error = rattler_repodata_gateway::GatewayError::Cancelled;

        // When
        let failure = gateway_http_failure(&error);

        // Then
        assert!(failure.is_none());
    }

    #[test]
    fn channel_authentication_failure_maps_401_on_a_matched_private_channel() {
        // Given
        let private_channels = vec!["https://packages.example.test/org".to_string()];
        let http_failure = Some(HttpFailure {
            status: reqwest::StatusCode::UNAUTHORIZED,
            url: Some(
                "https://packages.example.test/org/noarch/repodata.json"
                    .parse()
                    .unwrap(),
            ),
        });

        // When
        let error = channel_authentication_failure(http_failure, &private_channels);

        // Then
        assert_eq!(
            error,
            Some(
                super::super::error::EphemeralEnvError::ChannelAuthenticationFailed {
                    channel: "https://packages.example.test".to_string(),
                }
            )
        );
    }

    #[test]
    fn channel_authentication_failure_returns_none_for_a_public_channel_401() {
        // Given
        let private_channels = vec!["https://packages.example.test/org".to_string()];
        let http_failure = Some(HttpFailure {
            status: reqwest::StatusCode::UNAUTHORIZED,
            url: Some(
                "https://packages.example.test/public/repodata.json"
                    .parse()
                    .unwrap(),
            ),
        });

        // When
        let error = channel_authentication_failure(http_failure, &private_channels);

        // Then
        assert_eq!(error, None);
    }

    #[test]
    fn channel_authentication_failure_returns_none_for_a_non_auth_status() {
        // Given
        let private_channels = vec!["https://packages.example.test/org".to_string()];
        let http_failure = Some(HttpFailure {
            status: reqwest::StatusCode::NOT_FOUND,
            url: Some(
                "https://packages.example.test/org/noarch/repodata.json"
                    .parse()
                    .unwrap(),
            ),
        });

        // When
        let error = channel_authentication_failure(http_failure, &private_channels);

        // Then
        assert_eq!(error, None);
    }

    #[test]
    fn matching_private_channel_matches_a_request_url_missing_an_explicit_default_port() {
        // Given: the operator's own `channel_settings`/`channels` entry
        // retains an explicit default port exactly as written, but the real
        // outgoing `reqwest::Url` (parsed and re-serialized) omits it --
        // exercised via the normalized fallback since the raw string
        // comparison alone would miss this.
        let private_channels = vec!["https://packages.example.test:443/org".to_string()];
        let request_url = "https://packages.example.test/org/noarch/repodata.json"
            .parse()
            .unwrap();

        // When
        let matched = matching_private_channel(&request_url, &private_channels);

        // Then
        assert_eq!(matched, Some("https://packages.example.test:443/org"));
    }

    #[test]
    fn matching_private_channel_normalized_fallback_still_respects_the_path_boundary() {
        // Given
        let private_channels = vec!["https://packages.example.test:443/org".to_string()];
        let sibling_url = "https://packages.example.test/organization/repodata.json"
            .parse()
            .unwrap();

        // When
        let matched = matching_private_channel(&sibling_url, &private_channels);

        // Then
        assert_eq!(matched, None);
    }

    #[test]
    fn injected_authorization_header_is_redacted_from_request_debug_output() {
        // Given
        let token = "super-secret-token-value";
        let mut header = http::HeaderValue::from_str(token).unwrap();
        header.set_sensitive(true);
        let mut request = reqwest::Client::new()
            .get("https://packages.example.test/private/noarch/repodata.json")
            .build()
            .unwrap();
        request.headers_mut().insert(AUTHORIZATION, header);

        // When
        let debug_output = format!("{request:?}");

        // Then
        assert!(!debug_output.contains(token));
    }

    #[test]
    fn origin_only_discards_path_query_and_fragment() {
        // Given
        let channel =
            "https://user:secret@packages.example.test:8443/private?token=secret#fragment";

        // When
        let origin = origin_only(channel);

        // Then
        assert_eq!(origin, "https://packages.example.test:8443");
    }

    #[test]
    fn origin_only_uses_the_fixed_fallback_for_unparseable_or_opaque_channels() {
        // Given
        let unparseable_channel = "not a URL";
        let opaque_channel = "file:///private/channel";

        // When
        let unparseable_origin = origin_only(unparseable_channel);
        let opaque_origin = origin_only(opaque_channel);

        // Then
        assert_eq!(unparseable_origin, UNPARSEABLE_CHANNEL);
        assert_eq!(opaque_origin, UNPARSEABLE_CHANNEL);
    }

    #[test]
    #[serial_test::serial]
    fn read_channel_token_header_reads_the_current_environment_value_each_time() {
        // Given
        let _token_guard = ChannelTokenGuard::set("first-token");
        let first_header = read_channel_token_header().unwrap();

        // When
        // SAFETY: This test holds serial_test's process-wide lock.
        unsafe {
            env::set_var(CHANNEL_TOKEN_ENV_VAR, "second-token");
        }
        let second_header = read_channel_token_header().unwrap();

        // Then
        assert_eq!(first_header.to_str().unwrap(), "first-token");
        assert_eq!(second_header.to_str().unwrap(), "second-token");
    }

    #[test]
    #[serial_test::serial]
    fn read_channel_token_header_rejects_non_header_safe_values() {
        // Given
        let _token_guard = ChannelTokenGuard::set("invalid\nheader");

        // When
        let header = read_channel_token_header();

        // Then
        assert_eq!(header, None);
    }
}
