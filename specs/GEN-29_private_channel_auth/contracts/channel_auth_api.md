# Contract: `channel_auth` module surface and `EphemeralEnvError` additions

This feature has no HTTP/CLI-facing API of its own; it changes the behavior of the existing `allez::ephemeral` surface (`create_ephemeral_environment`'s signature is unchanged) and extends the existing, already-public, `#[non_exhaustive]` `EphemeralEnvError` enum. This document is the contract for those extensions.

## Environment contract

| Name | Required? | Empty or unusable-value behavior |
|---|---|---|
| `ALLEZ_CHANNEL_TOKEN` | Only when at least one configured channel classifies as private (see below) | Treated identically to unset: an empty string, or a value that is not representable as an HTTP header value, both produce `MissingChannelToken` |

No other environment variable is read or written by this feature. Populating `ALLEZ_CHANNEL_TOKEN` (manual export, wrapper script, sandboxed credential-injection proxy, or anything else) is entirely outside this feature's concern, per `spec.md`'s Operating Context.

## Classification contract (FR-003)

A configured channel URL is private if and only if `channel_settings` contains an entry `e` such that:

```text
(normalize(e.channel) == normalize(channel_url)
  OR (e.channel ends with "/*" AND channel_url starts with strip_trailing_star(e.channel)))
AND e contains a key named "auth" (any value)
```

where `normalize(url)` trims one trailing `/`, and `strip_trailing_star` removes only the trailing `*` (the value already ends in `/` at that point, since only patterns ending `/*` are treated as prefixes — a `*` not immediately preceded by `/` is never treated as a prefix marker and can only satisfy the exact-match branch, which it cannot). A channel with no matching entry is public, unconditionally. There is no host-based, URL-shape-based, scheme-based, or other fallback. This is a pure function of already-resolved configuration: it performs no I/O and makes no network request.

Every `channel_settings` field this feature reads or ignores, stated explicitly:

| Field | Role in this feature |
|---|---|
| `channel` (per-entry) | Matched against a configured channel's resolved URL, as above. In scope. |
| `auth` (per-entry) | Presence checked (value ignored) as the sole "needs the token" signal. In scope. |
| Any other per-entry key (for example `user`) | Read as part of the raw map, never interpreted or acted on. Explicitly out of scope. |

Every other `condarc::Config` field is irrelevant to this contract for one of two reasons: `channels`, `channel_alias`, `custom_channels`, `custom_multichannels`, `default_channels`, and every other channel-identity-determining field are already fully resolved into `ResolvedChannels.channels` (concrete URLs) by `condarc::expand_channels()` before classification ever runs, so this feature never re-consults them directly; and none of `Config`'s remaining fields — including ones with their own, different authentication-adjacent semantics that conda already defines independently of this feature (for example `add_anaconda_token`, `client_ssl_cert`/`client_ssl_cert_key`) — carry the specific `channel_settings`/`auth`-key signal this feature reads, so none of them participate in FR-003's classification either.

## Header-injection contract (FR-001, FR-002)

For every outgoing HTTP request (repodata fetch and package download both share the one client this feature builds):

- If the request's own URL equals, or starts with, a private channel's URL followed by `/` (via `matching_private_channel`'s exact-or-path-bounded-prefix rule): the request's `Authorization` header is set to `ALLEZ_CHANNEL_TOKEN`'s value, copied byte-for-byte — no `Bearer ` prefix, no trimming, no encoding transform — and marked sensitive (`HeaderValue::set_sensitive(true)`).
- Otherwise: the request is forwarded with zero modification. No header is added, removed, or altered.

## Error contract (FR-004, FR-005)

Two new, additive `EphemeralEnvError` variants (`#[non_exhaustive]`, unchanged category-string contract from every existing variant):

| Scenario | Variant | `category()` | Distinct from |
|---|---|---|---|
| A private channel is configured and `ALLEZ_CHANNEL_TOKEN` is unset, empty, or not a usable header value | `MissingChannelToken` | `"missing_channel_token"` | Never `"unresolvable_package"`; no network request is attempted. |
| A channel this feature attached a credential to responds HTTP 401 or 403 | `ChannelAuthenticationFailed { channel: String }` | `"channel_authentication_failed"` | Never `"unresolvable_package"`; a 401/403 from a **public** channel, or a plain 404/network error from any channel, remains `"unresolvable_package"`/`"resolution_failed"` exactly as today — this variant is only constructed once the failing request's URL is confirmed to fall under a private channel, and its `channel` value is that matched channel's origin only (`scheme://host[:port]`), never a path, whether the operator's own or the failing request's own sub-resource path. |

Both surface through the existing `render_error`/JSON `category` field and the existing human-readable `Display` path unchanged: no new exit code, no output schema version bump.

## Redaction contract (FR-006)

- The token is never retained in a separate raw-string field anywhere in this design: its only retained representation is an `http::HeaderValue` marked `.set_sensitive(true)`, built once and cloned per matching request. (`std::env::var`'s and `HeaderValue::from_str`'s own momentary intermediate `String` values, used only during that one conversion, are not retained afterward.)
- `channels::redact_channel_url` strips a URL's userinfo, `/t/<token>/` path segment, query string, and fragment, for the pre-existing `UnresolvablePackage`/`IntegrityVerificationFailed` variants that already use it.
- `ChannelAuthenticationFailed { channel }`'s `channel` field never carries a path at all: it is the matched private channel's origin only (`scheme://host[:port]`, via `channel_auth::origin_only`), so there is no path content — of any kind — for redaction to need to catch. This is a stricter guarantee than `redact_channel_url`'s own path-preserving behavior; the two pre-existing variants keep their path (needed to show which package/channel failed to resolve), this new field does not need to and does not.
- `MissingChannelToken` carries no payload; there is nothing to redact beyond the fixed environment-variable name, a `&'static str` constant.

## Non-goals

- No API for creating, storing, caching, refreshing, or validating the token beyond checking it is present and usable.
- No implementation of conda's pluggable auth-handler system: `channel_settings`' `auth` key's value is never read, only its presence.
- No HTTPS-only enforcement for the private-channel set (research.md Decision 1) — a matching `channel_settings` entry classifies a channel private regardless of its resolved URL's scheme, unconditionally per FR-002's own wording.
- No defense against a same-origin redirect from a private channel's URL to a different, non-matching path on the same host (research.md Decision 2); no defense against `.condarc` itself being modified by something the operator did not intend (spec.md Assumptions) — both are accepted, stated residual considerations, not implementation gaps this feature closes.
- No new CLI flag, no new output schema field beyond the existing `category`/`message` shape `render_error` already emits for every `CategorizedError`.
