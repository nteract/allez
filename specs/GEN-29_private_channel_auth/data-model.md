# Phase 1 Data Model: Private Channel Authentication

"Entities" here are the Rust types/functions that make `spec.md`'s FRs and User Stories representable and testable in code, per Constitution VI's "make invalid states unrepresentable" principle. See `research.md` for the rationale behind every decision reflected below.

## `condarc::expand_channels::ResolvedChannels` (extended)

| Field | Type | Notes |
|---|---|---|
| `channels` | `Vec<String>` | Unchanged. Ordered, concrete channel URLs. |
| `channel_priority` | `ChannelPriority` | Unchanged. |
| `channel_settings` *(new)* | `Vec<condarc::ChannelSetting>` | A direct pass-through of `Config.channel_settings.unwrap_or_default()`; no interpretation, no matching against `channels`, no normalization beyond what `condarc::parse` already applies. |

`ResolvedChannels::from_channels(channels: Vec<String>)` (the `#[non_exhaustive]`-workaround test constructor every existing ephemeral-env test uses) defaults `channel_settings` to an empty `Vec`. Every existing test therefore classifies every channel it configures as public, regardless of that channel's URL scheme, and no existing call site changes.

## `ephemeral::solve::SolvedPackages` (existing type, extended)

| Field | Type | Notes |
|---|---|---|
| `records` | `Vec<RepoDataRecord>` | Unchanged. |
| `client` | `reqwest_middleware::ClientWithMiddleware` | Type changes from today's plain `reqwest::Client` — the same client `install_packages` already reuses via `with_download_client`. |
| `private_channels` *(new)* | `Vec<String>` | The same list `classify_private_channels` produced, carried forward so `install_packages` can call `matching_private_channel` for its own FR-005 correlation check without recomputing classification. |

## `condarc::ChannelSetting` (extended, referenced)

`pub struct ChannelSetting(pub BTreeMap<String, String>)` — an opaque per-entry map, gaining a derived `Eq` (alongside its existing `Debug, Clone, PartialEq, Default`) so `ResolvedChannels`, which derives `Eq`, can hold a `Vec<ChannelSetting>`. This feature reads exactly two conventions from it, both already documented by conda upstream and unenforced by `condarc` itself: a `channel` key naming the entry's target (a full channel URL, or a `/*`-suffixed prefix pattern), and the presence of an `auth` key (any value) as this feature's own binary "needs the token" signal. Every other key an entry may carry (for example `user`) is read as part of the map but never interpreted.

## `allez::ephemeral::channel_auth` (new module)

### Constants

| Name | Value | Notes |
|---|---|---|
| `CHANNEL_TOKEN_ENV_VAR` | `"ALLEZ_CHANNEL_TOKEN"` | The single designated environment variable (FR-001), spelled once here and referenced by both the read site and every error message naming it (FR-004). |
| `HTTP_USER_AGENT` | `concat!("allez/", env!("CARGO_PKG_VERSION"))` | Relocated here from `solve.rs`, its only remaining call site once `build_channel_auth_client` owns base-client construction. |

### Functions

```rust
/// Returns the subset of `channels` classified private: a channel is
/// included iff `channel_settings` contains an entry whose `channel`
/// key names it (exact match, trailing `/` trimmed on both sides, or a
/// prefix match when the entry's `channel` value ends with `/*`) and
/// whose map contains an `auth` key. Scheme-agnostic — a matching
/// entry classifies private regardless of the channel's resolved URL
/// scheme (research.md Decision 1). Pure, deterministic, performs no
/// I/O and no network access. See research.md Decision 1 for the exact
/// boundary semantics.
fn classify_private_channels(
    channels: &[String],
    channel_settings: &[condarc::ChannelSetting],
) -> Vec<String>;

/// Returns the specific entry of `private_channels` that `url` falls
/// under — equal to it, or having it as a path-bounded prefix — or
/// `None` if no entry matches (i.e. does a repodata/package-download
/// or failed request's URL fall under one of the private channels this
/// invocation already decided on, and if so, which configured channel
/// is it?). This is a distinct operation from `classify_private_channels`'s
/// own matching of `channel_settings` patterns against configured
/// channels — by the time `matching_private_channel` runs,
/// `private_channels` holds only resolved, wildcard-free base URLs.
/// Shared between `PrivateChannelAuthMiddleware::handle()` (which only
/// needs `.is_some()`) and Decision 4's failure-attribution check
/// (which uses the returned channel identity directly as
/// `ChannelAuthenticationFailed`'s payload), so the two can never drift
/// apart.
fn matching_private_channel<'a>(
    url: &reqwest::Url,
    private_channels: &'a [String],
) -> Option<&'a str>;

/// Reads `ALLEZ_CHANNEL_TOKEN` and converts it to a sensitive
/// `http::HeaderValue`. Returns `None` when the variable is unset,
/// empty, or not representable as an HTTP header value. Never logs,
/// caches, or stores the token beyond this one call's return value
/// (FR-007).
fn read_channel_token_header() -> Option<http::HeaderValue>;

/// Builds the one shared HTTP client both `solve.rs`'s `Gateway` and
/// `install.rs`'s `Installer` use: the existing base `reqwest::Client`
/// (`.no_proxy().user_agent(HTTP_USER_AGENT)`, using this module's own
/// relocated `HTTP_USER_AGENT` constant), wrapped with
/// a `PrivateChannelAuthMiddleware` only when `private_channels` is
/// non-empty. Returns `Err(EphemeralEnvError::MissingChannelToken)` when
/// `private_channels` is non-empty and `read_channel_token_header()`
/// returns `None` — the one call site FR-004's check happens at, before
/// any network request.
fn build_channel_auth_client(
    private_channels: Vec<String>,
) -> Result<reqwest_middleware::ClientWithMiddleware, EphemeralEnvError>;

/// Walks `error` itself, then `error.source()`, downcasting each link
/// in turn to `reqwest::Error`, returning the first HTTP status and
/// request URL found together. Checking `error` itself first (not only
/// its sources) matters because either the top-level error or a nested
/// one may be the `reqwest::Error` carrying the relevant status/URL.
/// Used by both `solve.rs` and `install.rs`, paired with
/// `matching_private_channel`, to distinguish a 401/403 *on a channel
/// this feature authenticated* from every other failure shape (FR-005).
struct HttpFailure {
    status: reqwest::StatusCode,
    url: Option<reqwest::Url>,
}
fn reqwest_http_failure(error: &(dyn std::error::Error + 'static)) -> Option<HttpFailure>;

/// Reduces `channel` (a `private_channels` entry) to its origin only —
/// `scheme://host[:port]`, by parsing `channel` as a URL and taking
/// `.origin()`, returning `.ascii_serialization()` only when that origin
/// is a tuple origin (`Origin::is_tuple()`) — discarding the entry's own
/// path entirely, not merely a failing request's rattler-appended
/// subpath. Returns the fixed constant `"<unparseable channel>"`, never
/// any part of `channel` itself and never `Origin::Opaque`'s own
/// `"null"` serialization, if parsing fails or yields an opaque origin
/// (not expected in practice; every `private_channels` entry is itself a member of
/// `ResolvedChannels.channels` and already excludes no-host channels).
/// Used to build `EphemeralEnvError::ChannelAuthenticationFailed`'s
/// payload, giving that field an unconditional no-path-content
/// redaction guarantee (FR-006) on every path through this function,
/// including the fallback, stricter than `redact_channel_url`'s own
/// path-preserving behavior.
fn origin_only(channel: &str) -> String;
```

### `PrivateChannelAuthMiddleware` (private struct, implements `reqwest_middleware::Middleware`)

| Field | Type | Notes |
|---|---|---|
| `private_channels` | `Vec<String>` | The exact, normalized (trailing `/` trimmed) channel URLs classification produced, owned directly by the middleware (not `Arc`-wrapped — `reqwest_middleware::ClientBuilder` already stores the middleware itself behind an `Arc`, so a second `Arc` here would be redundant). Small by construction — one entry per private channel actually configured. |
| `token_header` | `http::HeaderValue` | Built once via `read_channel_token_header()`, already marked `.set_sensitive(true)`. `HeaderValue` is `Clone`; no separate raw-string field of the token exists anywhere in this design. |

`handle()`: for the outgoing request's own URL, calls the shared `matching_private_channel` predicate against `private_channels`; on `Some(_)`, clones `token_header` into the request's `Authorization` header and forwards; otherwise forwards the request with zero modification — the public-channel half of FR-002 is structural, not merely tested.

## `EphemeralEnvError` (extended — `src/ephemeral/error.rs`)

Two new `#[non_exhaustive]` variants, alongside the six that exist today:

| Variant | Payload | Category (`CategorizedError`) | Display |
|---|---|---|---|
| `MissingChannelToken` | none | `missing_channel_token` | `` environment variable `ALLEZ_CHANNEL_TOKEN` is required for a configured private channel but is unset, empty, or not a usable value `` |
| `ChannelAuthenticationFailed` | `{ channel: String }` (`channel_auth::origin_only`'s output for the matched `private_channels` entry — `scheme://host[:port]`, no path, no redaction needed since there is no path content) | `channel_authentication_failed` | `` channel `<origin>` rejected the provided credential (HTTP 401 or 403) `` |

Both are additive to the existing `match` arms in `category()`, `fmt::Display`, and `fmt::Debug`; every existing variant is untouched. `EphemeralEnvError` staying `#[non_exhaustive]` (already true today, deliberately, per GEN-24) is why this addition needs no downstream `match` outside this crate to change.

## Test / spec-coverage matrix (Constitution VIII)

Every test that sets, empties, or unsets `ALLEZ_CHANNEL_TOKEN` runs under this workspace's existing `serial_test` discipline (already a dev-dependency, already used elsewhere for process-environment-mutating tests) and restores the variable's prior state on completion, so these tests cannot interfere with each other or with unrelated tests run in the same process.

| Spec item | Test |
|---|---|
| FR-003 classification | Pure unit tests on `classify_private_channels`: an entry with `channel` exact-matching a configured channel and an `auth` key classifies private (regardless of the channel's URL scheme); an entry with `channel` matching but no `auth` key classifies public; an entry whose `channel` ends `/*` correctly matches `prefix/sub/path` but not `prefixed-differently`; a channel absent from `channel_settings` always classifies public regardless of its URL. |
| FR-002/FR-005 request-URL membership | Pure unit tests on `matching_private_channel`: a private base URL matches its own repodata/package descendant paths (e.g. `/org/noarch/repodata.json`, `/org/pkg.tar.bz2`); it rejects a sibling prefix that merely shares a string prefix without the path-boundary (`/org` vs. `/organization/...`), a different scheme, a different host, and a different port. This predicate gates both header injection (FR-002) and 401/403 attribution (FR-005), so its boundary correctness is tested directly rather than only through the integration scenarios below. |
| US1 Acceptance Scenario 1 | A `wiremock` server, configured as a channel with a matching `channel_settings`/`auth` entry, plus `ALLEZ_CHANNEL_TOKEN` set: asserts the server received an `Authorization` header equal to the raw environment value, and the package resolves and installs. |
| US1 Acceptance Scenario 2 | Two `wiremock` servers, one with a matching `channel_settings`/`auth` entry and one without: asserts only the first's captured requests carry `Authorization`; the second's carry none. |
| US2 Acceptance Scenario 1 and 2 | A private channel configured, `ALLEZ_CHANNEL_TOKEN` unset (Scenario 1) and set to an empty string (Scenario 2): both produce `Err(EphemeralEnvError::MissingChannelToken)`, and the mock server receives zero requests in either case. A unit test additionally sets `ALLEZ_CHANNEL_TOKEN` to a value `http::HeaderValue::from_str` rejects (for example, one containing a bare `\r` or `\n` byte), asserting `read_channel_token_header` returns `None` — covering FR-004's "not representable as an HTTP header value" case. |
| US3 Acceptance Scenario 1 | Two real-stack cases against a `wiremock` server configured private: one scripted to reject the repodata request with 401 (then repeated with 403), asserting the error surfaces through the `solve.rs`/`GatewayError` path; a second scripted to serve valid repodata but reject the package download, asserting the error surfaces through the `install.rs`/`InstallerError::FailedToFetch` path. Both assert `Err(EphemeralEnvError::ChannelAuthenticationFailed { channel })` with `channel` equal to the private channel's origin, never `UnresolvablePackage`/`ResolutionFailed`. |
| FR-005 correlation (negative case) | A `wiremock` server configured as a **public** channel (no matching `channel_settings` entry) scripted to reject a request with 401: asserts the failure stays `UnresolvablePackage`/`ResolutionFailed`, confirming a public channel's own 401 is never mislabeled `ChannelAuthenticationFailed`. |
| Edge case: unmarked channel never private | A `wiremock` server with no `channel_settings` entry at all: asserts no `Authorization` header on any request, and no `MissingChannelToken` error even when `ALLEZ_CHANNEL_TOKEN` is unset. |
| Edge case: redaction | A unit test asserting `format!("{request:?}")` on a request the middleware touched never contains the literal token; a unit test asserting `channel_auth::origin_only` returns only `scheme://host[:port]` for a channel URL that includes a path, query string, and fragment (confirming none of those reach `ChannelAuthenticationFailed`); a unit test asserting `origin_only` returns the fixed `"<unparseable channel>"` constant, never a substring of its input, for an unparseable or opaque-origin input; a unit test asserting the extended `redact_channel_url` strips a credential embedded in a query string or fragment from `UnresolvablePackage`/`IntegrityVerificationFailed`, not only userinfo/`/t/token/`; an end-to-end test capturing `RUST_LOG=allez=trace` stderr in both JSON and human formatter modes during a 401 rejection, asserting neither contains the token. |
| FR-007 (no storage) | Enforced structurally by `channel_auth`'s own function signatures never returning or exposing the token beyond one call's return value, verified by code review of the module's public surface; additionally covered by two runtime regression tests: a unit test confirming `read_channel_token_header` reflects the current environment value on every call (no internal caching across repeated calls in one process), and an integration test scanning every file under a successful install's environment root for the literal token value, asserting it appears nowhere on disk (no persistence). |

## Relationships

```text
condarc::expand_channels(&Config)
        |
        v
condarc::ResolvedChannels { channels, channel_priority, channel_settings }
        |  (unchanged threading through channel_config::resolve_channel_config[_from])
        v
ephemeral::create_ephemeral_environment(requested, channels, default_override)
        |
        v
ephemeral::solve::solve_packages(root, config: &ResolvedChannels, packages)
        |
        +-- channel_auth::classify_private_channels(&config.channels, &config.channel_settings)
        |        |
        |        v
        +-- channel_auth::build_channel_auth_client(private_channels) --> Err(MissingChannelToken)?
        |        |
        |        v
        |  ClientWithMiddleware  ------------------> Gateway::with_client(...)
        |        |                                            |
        |        |                          gateway.query(...).execute().await
        |        |                                            |
        |        |                        Err --> channel_auth::reqwest_http_failure(&err)
        |        |                                   401|403 with url matching a private_channels entry --> ChannelAuthenticationFailed { channel: origin_only(<matched entry>) }
        |        |                                   other (including 401|403 on a public channel) --> ResolutionFailed (unchanged)
        |        v
        +-- SolvedPackages { records, client: ClientWithMiddleware, private_channels }
                 |
                 v
ephemeral::install::install_packages(root, prefix, solution)
        |
        +-- Installer::with_download_client(solution.client)
        |
        +-- installer.install(...).await
                 |
           Err(InstallerError::FailedToFetch(id, source)) --> channel_auth::reqwest_http_failure(source)
                                                                  401|403 with url matching a solution.private_channels entry --> ChannelAuthenticationFailed { channel: origin_only(<matched entry>) }
                                                                  "hash mismatch" (unchanged) --> IntegrityVerificationFailed
                                                                  other (including 401|403 on a public channel) --> UnresolvablePackage
```
