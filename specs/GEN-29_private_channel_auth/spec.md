# Feature Specification: Private Channel Authentication

**Feature Branch**: `GEN-29_private_channel_auth`

**Created**: 2026-08-10

**Jira**: [GEN-29](https://anaconda.atlassian.net/browse/GEN-29) — Private channel authentication (parent epic: [GEN-19](https://anaconda.atlassian.net/browse/GEN-19))

**Operating Context**: `allez` is invoked by an AI agent, not directly by a human at a terminal (see epic GEN-19). Whether that agent runs inside a sandbox is outside `allez`'s concern — sandboxing is recommended but neither assumed, required, nor managed by `allez` itself. The token, when one is needed, is supplied through the process environment; creating, storing, or refreshing the token is out of scope for this feature (see FR-007).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Install from a private channel using an already-present token (Priority: P1)

An operator has a private conda channel configured and a valid auth token already present in its process environment. It requests packages that live on that channel and expects them to resolve and install normally, exactly as if the channel were public — with no additional invocation flags or manual credential handling on its part.

**Why this priority**: This is the entire reason this feature exists — every other behavior (error handling, failure categorization) only matters once a private channel can actually be reached successfully when the credentials for it are already available.

**Independent Test**: Can be fully tested by configuring a private channel, setting a valid token in the environment, requesting a package that only exists on that channel, and confirming it resolves and installs successfully — independent of any failure-path behavior.

**Acceptance Scenarios**:

1. **Given** a private channel is configured and a valid auth token is present in the environment, **When** a package is requested from that channel, **Then** the request to that channel carries the token as its credential and the package resolves and installs successfully.
2. **Given** both a private channel (needing the token) and a public channel (not needing it) are configured, **When** packages are requested that could come from either, **Then** only requests to the private channel carry the token; requests to the public channel are sent unmodified.

---

### User Story 2 - Get a clear, actionable error when the token is missing (Priority: P1)

An operator has a private channel configured but never set the environment variable that carries the auth token (or set it to an empty value). It needs to know unambiguously that a credential is missing, rather than receiving a generic "package not found" error that gives no hint about the real cause.

**Why this priority**: Without this, a simple configuration mistake (forgetting to set the token) is indistinguishable from a genuinely nonexistent package, wasting an operator's or an agent's time chasing the wrong problem. This is as essential to this feature being usable as the happy path itself.

**Independent Test**: Can be fully tested by configuring a private channel, leaving the designated environment variable unset or empty, requesting a package from that channel, and confirming a clear, actionable error is returned identifying the missing credential — independent of whether the happy path (User Story 1) has been exercised.

**Acceptance Scenarios**:

1. **Given** a private channel is configured and its designated environment variable is unset, **When** a package is requested from that channel, **Then** the operator receives a clear, actionable error naming the missing environment variable.
2. **Given** a private channel is configured and its designated environment variable is set to an empty value, **When** a package is requested from that channel, **Then** the same clear, actionable error is returned as in Scenario 1 — an empty value is treated the same as an unset one.

---

### User Story 3 - Distinguish a rejected credential from a nonexistent package (Priority: P2)

An operator (or an automated caller acting on its behalf) has a token present and attached to its request, but the channel server itself rejects it (for example, the token is invalid, expired, or lacks access to that channel). The caller needs to be able to tell this apart from the requested package genuinely not existing, so it can react appropriately — for example, by not looking for a package that in fact does exist under different credentials.

**Why this priority**: This refines failure reporting rather than introducing new core mechanics — it matters once the credential-present and credential-missing paths already work, so an automated caller has a reliable, branchable signal instead of having to parse free-text error messages to guess what actually happened.

**Independent Test**: Can be fully tested by configuring a private channel, supplying a token that the channel server rejects (simulating a 401 or 403 response), requesting a package from that channel, and confirming the resulting failure is reported under a category distinct from "package could not be resolved" — independent of User Stories 1 and 2.

**Acceptance Scenarios**:

1. **Given** a private channel is configured and the attached token is rejected by the channel server with a 401 or 403 response, **When** a package is requested from that channel, **Then** the failure is reported under a distinct authentication-failure category, not the generic category used when a package simply cannot be resolved.

---

### Edge Cases

- The channel server returns a 401 or 403 for a reason unrelated to the token's presence (for example, the token is invalid, or the account behind it lacks access to that specific channel) — still reported under the same distinct authentication-failure category (User Story 3); this feature does not distinguish an invalid token from a token that lacks access, beyond what the channel server's own response code indicates. A missing token (unset or empty) is a distinct case, governed separately by FR-004.
- A channel URL or any error text that would otherwise reveal a credential (for example, a token embedded directly in a URL) is about to reach a log entry or an error message — the credential-bearing portion is redacted first, regardless of which user story's error path produced it or whether verbose/debug output was requested. "Credential embedded in a channel URL" means a credential carried through one of a URL's own standard credential-transport components — userinfo, a query parameter, a fragment, or a path segment matching a recognized credential-transport convention (for example conda's own `/t/<token>/` path segment); a channel's own scheme, host, or port, which this feature's error messages may still show, is a public identifier, not a credential-transport mechanism, even in the contrived case of an operator naming a host after a secret. Credential material placed in an *unrecognized* path-segment shape (i.e. not the `/t/<token>/` convention) is out of scope for FR-006.
- A configured channel is not marked in `.condarc` as needing the token — it is never treated as private, and no request to it ever carries the token, regardless of what host or URL it uses. There is no default-deny/default-allow guesswork based on a channel's host or URL shape; absence of a marking is conclusive.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST read a private-channel auth token from a single designated environment variable when a private channel is configured.
- **FR-002**: System MUST attach that token as the credential on the channel request's `Authorization` header for every request to a channel that needs it — whether that request is resolving/solving packages against the channel or downloading a matched package from it — forwarding the environment variable's value byte-for-byte with no modification or added scheme prefix, and MUST leave requests to channels that don't need it unmodified.
- **FR-003**: System MUST determine which configured channel(s) need the token directly from the operator's existing `.condarc` configuration, without requiring any new, allez-specific configuration surface beyond the designated environment variable itself.
- **FR-004**: System MUST detect when a private channel's designated environment variable is unset or empty, and MUST report a clear, actionable error naming that variable, distinguishable from a package-resolution failure. A present value that cannot be represented as an HTTP header value is treated as missing for this requirement.
- **FR-005**: System MUST report an authentication failure returned directly by a channel server (HTTP 401 or 403), for a channel this feature attaches a credential to, under a category distinct from the category used when a package simply cannot be resolved, so an automated caller can branch on the difference without parsing free-text messages.
- **FR-006**: System MUST redact the token's value, and any credential embedded in a channel URL via a recognized credential-transport convention (userinfo, a query parameter, a fragment, or the `/t/<token>/` path convention — see Edge Cases), from every log entry and error message this feature produces, in every output mode, with no exception within that recognized scope.
- **FR-007**: System MUST NOT create, persist, cache across invocations, or refresh the token in any way; it only reads the token already present in the environment, forwards it, and checks for its presence — it never assesses whether the token itself is genuinely valid ahead of a request. Holding the token in memory for the duration of a single `allez` invocation, so it can be attached to more than one request within that invocation, is not caching or persistence; nothing may retain it, in any form, beyond that invocation's own process lifetime.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Once a private channel is configured and its designated environment variable contains a token the channel accepts, an operator can install packages from it without any additional invocation flags or manual credential handling.
- **SC-002**: An operator whose required token is missing (unset or empty) can identify the actual problem directly from the error message, without inspecting logs or ruling out a nonexistent-package explanation first.
- **SC-003**: An automated caller can reliably distinguish a channel server's HTTP 401 or 403 response from a package-resolution failure, for every private channel this feature attaches a credential to, without parsing free-text error content.
- **SC-004**: No log entry or error message produced by this feature ever exposes a token's literal value or a credential embedded in a channel URL, in any output mode.

## Assumptions

- A single designated environment variable carries the token for every channel that needs one in a given `allez` invocation; distinct, per-channel tokens are out of scope.
- The exact environment-variable name is a planning-time decision, not fixed by this specification.
- FR-003's classification is driven by `.condarc`'s existing `channel_settings` field (the per-channel setting conda's own documentation already reserves for auth-related configuration), which the operator populates when they configure a private channel. The precise matching rule (e.g. exact channel-URL match versus a prefix pattern, and which `channel_settings` key indicates that a channel needs authentication) is a planning-time decision, not fixed by this specification.
- `allez` targets Anaconda-hosted public and private conda channels specifically; both are served from the same Anaconda domains, so a channel's host or URL shape alone never indicates whether it is private — only an explicit `channel_settings` marking does.
- `.condarc` is a trusted input to this feature: any channel an operator marks in `channel_settings` receives the token, so writing to `.condarc` is equivalent to granting that channel access to it. Protecting `.condarc` itself from unauthorized modification is outside this feature's responsibility.
- Whatever process populates the designated environment variable — manual export, a wrapper script, a sandboxed credential-injection proxy, or anything else — is entirely outside this feature's responsibility; this feature behaves identically regardless of that mechanism.
