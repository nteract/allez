# Quickstart: Validating Private Channel Authentication

This is a validation guide, not an implementation guide — see `data-model.md` for types/functions and `tasks.md` for the implementation checklist.

## Prerequisites

- Rust toolchain matching this workspace (`cargo --version`; edition 2024).
- `cargo build --workspace` succeeds, confirming the new `astral-reqwest-middleware`/`async-trait`/`wiremock` dependencies resolve cleanly against the existing `Cargo.lock`.

## Scenario 1 — Happy path (US1, FR-001/FR-002)

1. Start a local `wiremock` mock server (bound address `PRIVATE`), scripted to serve a minimal, valid repodata and package response for one fixture package, capturing every request it receives.
2. Configure a channel pointing at `PRIVATE`, plus a `channel_settings` entry whose `channel` value matches that channel's URL and whose map contains an `auth` key.
3. `export ALLEZ_CHANNEL_TOKEN=test-token-value`.
4. Call `ephemeral::create_ephemeral_environment(...)` (or run `allez oneshot <fixture-package>` end to end, with `.condarc` pointed at a temp file carrying the above) against this configuration.
5. Expect: the environment creates successfully, and every request `wiremock` captured for `PRIVATE` carries `Authorization: test-token-value`, exact and unprefixed.

## Scenario 2 — Public channel stays untouched (US1 Acceptance Scenario 2)

1. Add a second `wiremock` server (`PUBLIC`), configured as a channel with no `channel_settings` entry at all, serving the same fixture shape and also capturing requests.
2. Configure both `PRIVATE`'s and `PUBLIC`'s channels, requesting packages from both.
3. Expect: `PRIVATE`'s captured requests carry `Authorization`; `PUBLIC`'s captured requests carry no `Authorization` header.

## Scenario 3 — Missing token (US2, FR-004)

1. Same configuration as Scenario 1, with `unset ALLEZ_CHANNEL_TOKEN`, then a second run with `export ALLEZ_CHANNEL_TOKEN=""`.
2. Expect: `create_ephemeral_environment` returns `Err(CreationFailure { error: EphemeralEnvError::MissingChannelToken, .. })` in both runs, and `PRIVATE` records zero requests, proving the check happens before any network call.
3. Run the equivalent through `allez oneshot --json <fixture-package>` and confirm the JSON error body's `category` is `"missing_channel_token"`, its `message` names `ALLEZ_CHANNEL_TOKEN`, and the exit code is `2`, matching every other rendered error.

## Scenario 4 — Channel-side auth rejection, both HTTP phases, correctly attributed (US3, FR-005)

1. Same configuration as Scenario 1, with `PRIVATE` scripted to respond `401 Unauthorized` to the repodata request. Expect `Err(EphemeralEnvError::ChannelAuthenticationFailed { channel })` with `channel` equal to `PRIVATE`'s origin (`scheme://host[:port]`, no path), not `UnresolvablePackage`/`ResolutionFailed`; confirm via `allez oneshot --json` that the JSON `category` is `"channel_authentication_failed"`. Repeat with `403 Forbidden`.
2. A second run with `PRIVATE` serving valid repodata but responding `401 Unauthorized` to the package-archive download. Expect the same `ChannelAuthenticationFailed` category, confirming the failure is caught on the install path as well as the solve path.
3. A third run with a **public** channel (no matching `channel_settings` entry) scripted to respond `401 Unauthorized`. Expect `Err(EphemeralEnvError::ResolutionFailed)` (or `UnresolvablePackage`, depending on how many packages were requested), not `ChannelAuthenticationFailed` — a public channel's own 401 is never attributed to this feature's credential.

## Scenario 5 — Unmarked channel never becomes private

1. Configure `PRIVATE`'s channel with no `channel_settings` entry, and leave `ALLEZ_CHANNEL_TOKEN` unset.
2. Expect: the environment creates successfully (assuming the fixture package resolves), no request to `PRIVATE` carries `Authorization`, and no `MissingChannelToken` error occurs — absence of a `channel_settings` marking is conclusive, not merely a default.

## Scenario 6 — Redaction (FR-006)

1. Re-run Scenario 4.1's 401 case with `RUST_LOG=allez=trace` set, capturing stderr in both the default JSON log formatter and the `--human` formatter.
2. Expect: neither capture contains the token value used in Scenario 1/4.
3. The extended `redact_channel_url` strips a credential placed in a URL's query string or fragment, not only userinfo or `/t/token/`; a unit test asserts `format!("{request:?}")` on a request the middleware touched never contains the raw token.

## Success criteria recap

Passing every scenario above demonstrates SC-001 through SC-004 end to end: happy-path install with zero extra flags, an identifiable missing-token error without log inspection, a reliably distinguishable 401/403 category across both HTTP phases, and zero token/credential exposure in any output mode.
