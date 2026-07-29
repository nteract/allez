# Feature Specification: Resolve `.condarc` Channel Preferences for Package Selection

**Feature Branch**: `GEN-23_condarc_package_selection`

**Created**: 2026-07-29

**Status**: Draft (revised 2026-07-30 — see Operating Context for the scope change; further refined the same day per follow-up spec review — see Assumptions)

**Input**: User description: "GEN-23 Parse ~/.condarc for package-selection preferences: read the user's ~/.condarc on startup and extract channels, channel_priority, default_channels, and per-channel auth/proxy settings that affect resolution, tolerating missing or malformed files." *(Predates three scope corrections: per-channel auth/proxy extraction is out of scope for this ticket's output — see FR-009 — the channel-resolution algorithm is being added as a new capability of GEN-36's own crate rather than as code inside `allez`, and that new crate-side capability is this ticket's own work to build and test — see Operating Context.)*

**Jira**: [GEN-23](https://anaconda.atlassian.net/browse/GEN-23) — Parse `~/.condarc` for package-selection preferences (parent epic: [GEN-19](https://anaconda.atlassian.net/browse/GEN-19); sub-task: [GEN-36](https://anaconda.atlassian.net/browse/GEN-36))

**Operating Context**: `allez` runs inside a sandbox, invoked by an AI agent rather than a human (see the parent epic). This feature is read-only: it never writes to or manages `~/.condarc`.

As originally drafted, this ticket's work was a single, `allez`-internal piece of code that both located the file and resolved its contents into a concrete channel list. Following review, that changed: the channel-resolution algorithm itself — expanding bare channel names, substituting the `defaults` placeholder, normalizing channel priority, and resolving the allow/deny lists — is generic conda behavior a consumer other than `allez` (starting with nteract's own desktop work) may need independently, so it belongs in the condarc crate (GEN-36) rather than in `allez`.

GEN-36's crate is already delivered, but only as a **parser**: it turns `~/.condarc`'s text into a typed, validated `Config` with **no general defaulting** (per that crate's own FR-038: "a caller that wants a default applies its own policy explicitly"; the one narrow, existing exception is `ParseOptions.null_sequence_map_defaults`, discussed in Assumptions). It does not yet resolve channels — there is today no function in that crate that expands a bare name, substitutes `defaults`, or resolves an allow/deny list. Closing that gap is itself part of this ticket's own work, not a precondition already met elsewhere. Concretely, this ticket's work is delivered in two places, both fully specified below and both tracked, planned, reviewed, and tested entirely under this one ticket — no separate ticket is filed for either part, and GEN-36's own (closed) Jira ticket is not reopened, even though one part lands in a different crate's codebase:

1. **A new, additive channel-resolution capability inside the condarc crate** (`crates/condarc`): a `resolve` capability, layered on top of — and never altering — the crate's existing `parse()`/`Config` (see Assumptions). This is generic conda behavior with no dependency on `allez`, independently callable by any consumer of the crate (including a future nteract caller) that wants a fully-resolved channel configuration rather than the raw parsed document.
2. **File handling and adaptation inside `allez`**: locate and read `~/.condarc`, fall back to conda's own documented defaults whenever the file is absent, rejected, or unreadable, hand the file's contents to the crate's `parse()`, hand `parse()`'s result to the crate's new `resolve()`, and adapt `resolve()`'s output into the four-part shape the ephemeral-environment-creation capability (GEN-24, delivered) requires as input — so the `allez oneshot` capability (GEN-25) can wire the crate and GEN-24 together directly, with no further translation. (An earlier plan for this ticket also included retiring supposedly-duplicate code in `allez`'s existing ephemeral-environment module; on inspection, none of that code turned out to be a genuine duplicate of this ticket's own work — see Assumptions/Design Decisions, "Cleanup boundary".)

Requirements below are grouped by which codebase location implements them (`crates/condarc` vs. `allez`'s own source), but both groups are this one ticket's own deliverable.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Resolve real channel preferences into one ready-to-use list (Priority: P1)

An operator (an AI agent, via the `allez oneshot` capability that wires this ticket's output into environment creation) needs the channels a user has actually configured in `~/.condarc` — bare names, custom channel/multichannel definitions, and the `defaults` placeholder alike — turned into a single, ordered list of concrete channel identifiers, so environment creation honors the user's real preferences instead of a generic guess.

**Why this priority**: This is the core value of this ticket's work — every other behavior (graceful fallback, priority mode, allow/deny) refines this central resolution step.

**Independent Test**: Supplying a populated `.condarc` exercising `channels`, `channel_alias`, `custom_channels`, `custom_multichannels`, and `default_channels` together, verify the resolved output is one flat, ordered list of concrete channel identifiers with every bare name and `defaults` reference already expanded.

**Acceptance Scenarios**:

1. **Given** a `.condarc` that lists one or more channels by bare name, **When** those preferences are resolved, **Then** the output is a single, ordered list of concrete channel identifiers, preserving the original ordering exactly, with each bare name expanded according to this precedence: an already-fully-qualified URL is used as-is (aside from credential stripping, see FR-005); otherwise a matching `custom_multichannels` name, expanded to that multichannel's own members; otherwise a `custom_channels` name matched via progressive prefix stripping; otherwise `channel_alias`.
2. **Given** a `.condarc` whose `channels` list includes the special `defaults` name — or omits `channels` entirely, sets it to an explicit `null`, or sets it to an empty list — **When** those preferences are resolved, **Then** every occurrence of `defaults` is resolved via `custom_multichannels`: the user's own `custom_multichannels.defaults` definition if configured, otherwise the channel(s) from `default_channels`.
3. **Given** a `.condarc` that sets `override_channels_enabled`, **When** those preferences are resolved, **Then** that setting has no effect on the resolved channel list — this ticket's `.condarc`-only input has no separate override-request concept for it to gate.
4. **Given** a `.condarc` that sets `channels` to a non-empty list that does not include the `defaults` placeholder, **When** those preferences are resolved, **Then** the resolved output contains only the channels derived from that list — `default_channels` is not automatically appended alongside it.
5. **Given** a `.condarc` whose `custom_multichannels` definitions include a member entry that itself names another multichannel, a `custom_channels` entry, or the same multichannel being defined, **When** those preferences are resolved, **Then** that member is resolved as an ordinary bare name via `channel_alias` — it is not expanded further.
6. **Given** a `.condarc` that sets both `channels` and its alias `channel`, **When** those preferences are resolved, **Then** this is treated as malformed input, rather than picking one value over the other.

---

### User Story 2 - Never block on a missing or broken preferences file (Priority: P1)

An operator needs environment creation to keep working even when the current user has no `~/.condarc` at all, or has one that is malformed, invalid, or unreadable — the absence or brokenness of a human-authored config file should never be the reason an unattended, agent-driven operation fails.

**Why this priority**: `allez` runs unattended on behalf of an agent; a configuration problem entirely outside `allez`'s control must not block every subsequent operation.

**Independent Test**: Resolve preferences when `~/.condarc` (a) does not exist, (b) exists but the crate rejects its contents, and (c) exists but cannot be read due to an OS permission error — and confirm all three complete successfully using conda's own documented default channel configuration.

**Acceptance Scenarios**:

1. **Given** no `~/.condarc` file exists, **When** channel preferences are resolved, **Then** resolution completes successfully using conda's own documented default channel configuration, with no error surfaced to the caller and no observability record needed for this ordinary, expected case.
2. **Given** a `~/.condarc` file whose contents GEN-36's crate rejects, **When** channel preferences are resolved, **Then** resolution completes successfully using conda's own documented default channel configuration, the same as a missing file, and the condition is recorded through this project's structured observability — using the per-problem detail the crate already produces — rather than being silently indistinguishable from a normal, empty configuration.
3. **Given** a `~/.condarc` file that exists but cannot be read due to an OS permission error, **When** channel preferences are resolved, **Then** resolution completes successfully using conda's own documented default channel configuration, the same as a malformed file, and the condition is recorded through this project's structured observability, distinct from the silent "file absent" case in Acceptance Scenario 1.
4. **Given** any resolution request, regardless of outcome, **When** channel preferences are resolved, **Then** the `~/.condarc` file itself is never modified, moved, or deleted (see FR-015).

---

### User Story 3 - Preserve priority and access-restriction preferences exactly (Priority: P2)

An operator needs a user's declared channel-priority strictness and any explicit channel allow/deny restrictions to survive resolution with their *intent* intact, so a downstream capability enforcing those restrictions can act on exactly the channels the user meant to allow or deny.

**Why this priority**: This refines User Story 1's core resolved list rather than introducing new mechanics.

**Independent Test**: Resolving a `.condarc` that sets each of the three `channel_priority` string values, plus its two boolean spellings, confirming each resolves to the correct mode; separately, resolving a `.condarc` that sets `allowlist_channels`/`denylist_channels` and confirming both resolve into the same concrete identifier form the main channel list uses.

**Acceptance Scenarios**:

1. **Given** a `.condarc` that sets `channel_priority` to `strict`, `flexible`, or `disabled`, **When** those preferences are resolved, **Then** the resolved output carries that exact value.
2. **Given** a `.condarc` whose `channel_priority` key is entirely absent, **When** those preferences are resolved, **Then** the resolved output uses conda's own documented default (`flexible` — see FR-002's defaults table).
3. **Given** a `.condarc` that sets `allowlist_channels` and/or `denylist_channels`, **When** those preferences are resolved, **Then** both lists appear in the resolved output resolved into the same concrete identifier form the main channel list uses, with any embedded credential material stripped first.
4. **Given** a `.condarc` that sets both `allowlist_channels` and its alias `whitelist_channels`, **When** those preferences are resolved, **Then** this is treated as malformed input, rather than picking one value over the other.
5. **Given** a `.condarc` that sets `channel_priority` to the legacy boolean spelling `true` or `false`, **When** those preferences are resolved, **Then** `true` resolves to `flexible` and `false` resolves to `disabled`.

---

### Edge Cases

- What happens when `~/.condarc` does not exist? (Resolution succeeds using conda's own documented default channel configuration, no observability record needed.)
- What happens when `~/.condarc` exists but the crate rejects its contents? (Treated the same as a missing file — resolution succeeds using conda's own documented defaults, with the crate's own per-problem detail recorded via structured observability.)
- What happens when `~/.condarc` exists but cannot be read due to an OS permission or other I/O error? (Treated the same as a malformed file — resolution succeeds, with a minimal condition record via structured observability, distinct from the silent "file absent" case.)
- What happens when `~/.condarc` sets `channels` to an explicit `null` value, an empty list, or omits it entirely? (All three resolve the same as an explicit `channels: [defaults]`.)
- What happens when `~/.condarc` sets the singular `channel` key instead of, or in addition to, `channels`? (`channel` populates the same list `channels` does; both keys present in the same file is malformed input.)
- What happens when `channel_priority` is set to the legacy boolean spelling `true` or `false`? (`true` resolves to `flexible` and `false` resolves to `disabled`.)
- What happens when a `.condarc` setting this ticket's work does not recognize at all is present, and is unrelated to channel/package selection? (Ignored without causing a failure — inherited directly from the crate's own unknown-key tolerance; see FR-016.)
- What happens when a bare channel name matches both a `custom_channels` entry and a `custom_multichannels` entry? (The `custom_multichannels` match takes precedence — see FR-001(b) before FR-001(c).)
- What happens when a bare channel name is a sub-path under a `custom_channels` entry's name rather than an exact match (e.g. `acme/label/dev` when `custom_channels` configures `acme`)? (Matched via progressive prefix stripping, resolved using the matched entry's base URL joined with the *original, full* name.)
- What happens when the same channel appears in both `allowlist_channels` and `denylist_channels`? (Both lists are resolved and passed through as values; reconciling a conflict between them is a downstream capability's own responsibility.)
- What happens if a resolved value contains embedded credentials? (Stripped before becoming part of the resolved output, and never allowed to reach any error message or observability record, with the fact that stripping occurred for that entry itself recorded via structured observability — see FR-005/FR-013.)
- What happens when `~/.condarc` triggers one of the crate's own documented parsing divergences from real conda (an out-of-range integer in an integer-typed setting, a non-ASCII decimal digit in a numeric-parsed setting, an over-1024-character YAML key, or a pathologically deep/large document)? (Bounded entirely by the crate's own behavior, not re-validated by this ticket's work — see Known Limitations.)

## Requirements *(mandatory)*

### Channel Resolution (new capability to be added to the condarc crate — `crates/condarc`)

- **FR-001**: MUST resolve the user's configured channels — populated via `channels` or its alias `channel` (both present in the same file is malformed input), or conda's own effective default of a single `defaults` entry when absent or empty — into a single, ordered list of concrete channel identifiers (see Key Entities), preserving original ordering, via this precedence: (a) an already-fully-qualified URL, used as-is aside from credential stripping (FR-005); (b) a `custom_multichannels` name, expanded to that multichannel's own members (the name `defaults` is always resolved here first); (c) a `custom_channels` name matched via progressive prefix stripping, joined with the entry's original full name; (d) otherwise, `channel_alias`.
- **FR-002**: MUST apply conda's own documented default for any relevant setting whose key is absent from `~/.condarc`, distinguishing an absent key from an explicit `null` and an explicit empty list/map exactly as conda itself does. This defaulting is a property of the new `resolve` capability only; it MUST NOT alter `parse()`'s own existing, unchanged behavior of representing an absent setting as absent (see Assumptions). The exact default alias, platform-specific `default_channels` value, and built-in `custom_channels` mapping are conda's own already-documented default constants (see `docs/condarc_research.md` and the condarc crate's own already-delivered handling of them), not values this ticket invents:

  | Setting | Default when key absent | Explicit `null` | Explicit empty (`[]`/`{}`) |
  |---|---|---|---|
  | `channels` (or its alias `channel`) | Single `defaults` entry | Same as absent — single `defaults` entry | Same as absent — single `defaults` entry |
  | `channel_alias` | Conda's documented default alias | Malformed † | N/A — not list/map-shaped |
  | `default_channels` | Platform-specific conda default | Same as absent — conda's default applies | Distinct, valid value — stays empty, not defaulted |
  | `custom_channels` | Conda's built-in `pkgs/pro` mapping | Same as absent — conda's default applies | Distinct, valid value — stays empty, not defaulted |
  | `channel_priority` | `flexible` | Malformed † | N/A — not list/map-shaped |
  | `allowlist_channels` (or alias `whitelist_channels`) / `denylist_channels` / `custom_multichannels` | Empty (`()`/`{}`) | Same as absent | Same as absent — already empty regardless |

  † `parse()` rejects an explicit `null` on this scalar setting outright (a whole-document rejection, not setting-level defaulting); FR-012's whole-file fallback applies, the same as any other crate-rejected document.

- **FR-003**: MUST resolve `channel_priority` to exactly one of conda's own three documented modes (`strict`, `flexible`, `disabled` — defaulting to `flexible` per FR-002's table when the key is absent), including its legacy boolean spellings (`true`→`flexible`, `false`→`disabled`).
- **FR-004**: MUST resolve the channel allow-list (`allowlist_channels`, alias `whitelist_channels`) and deny-list (`denylist_channels`) through the same precedence FR-001 defines, so every entry uses the same concrete-identifier form as the main channel list. Both lists are empty when their setting is absent.
- **FR-005**: MUST strip any embedded credential material (see Key Entities for the exact forms) from a resolved channel identifier before it appears anywhere in the resolved output, and MUST report — as part of `resolve`'s own output, identifying the affected entry by its position/role rather than by the stripped material itself — that stripping occurred for that entry. This report is a plain fact `resolve` returns to its caller; the crate MUST NOT assume anything about any particular caller's own observability conventions (turning this report into `allez`'s own structured-observability record is `allez`'s job — see FR-013).
- **FR-006**: MUST treat a document that sets both a setting's canonical name and its alias (`channels`/`channel`, `allowlist_channels`/`whitelist_channels`) as malformed input, rather than picking one value over the other. This is a `parse()`-level rejection (an alias collision, the same category `parse()` already detects for any other setting), not a `resolve()`-level decision — a document with such a collision never reaches `resolve()` at all; it takes the whole-document fallback path FR-012 already defines.
- **FR-007**: MUST NOT apply `override_channels_enabled` to the resolved channel list — this ticket's `.condarc`-only input has no separate override-request concept for it to gate.
- **FR-008**: MUST NOT add a deduplication pass of its own over the resolved channel list, allow-list, or deny-list: a literal, raw duplicate within one setting's own list is already removed by `parse()`'s own sequence handling (matching conda's own merge-time `unique()` behavior) before resolution ever sees it, so resolution has nothing left to deduplicate there — but if resolving two different entries happens to coincide on the same concrete identifier (e.g., two different bare names that both expand to the same URL), that coincidental repeat is preserved as-is, not collapsed.
- **FR-009**: MUST NOT include per-channel authentication or proxy settings (`channel_settings`) anywhere in the resolved output.

### File Handling and Adaptation (in `allez`)

- **FR-010**: MUST locate and read `~/.condarc`, tolerating its absence by proceeding with conda's own documented default channel configuration rather than failing, with no observability record needed for this ordinary, expected case.
- **FR-011**: MUST hand `~/.condarc`'s contents to the condarc crate's `parse()`, and hand `parse()`'s result to the crate's `resolve()`, for parsing and channel resolution respectively; MUST NOT re-implement any part of either.
- **FR-012**: MUST tolerate a `~/.condarc` file the crate's `parse()` rejects, or one that exists but cannot be read due to an OS permission or other I/O error, by proceeding with conda's own documented default channel configuration, the same as a missing file, while recording the specific condition — including the crate's own per-problem detail, for the rejected-content case — through structured observability, distinct from the silent "file absent" case in FR-010. What counts as "rejects" here is bounded entirely by the crate's own documented behavior, including its own accepted divergences from real conda's validation (see Known Limitations) — this ticket's work does not re-validate, tighten, or loosen what the crate itself accepts or rejects.
- **FR-013**: MUST record, via this project's structured observability, every credential-stripping event the crate's `resolve()` reports for a given resolution (see FR-005), distinct from a malformed-input record (FR-012).
- **FR-014**: MUST adapt the crate's resolved channel configuration into the four-part shape the ephemeral-environment-creation capability (GEN-24) requires as input — an ordered list of concrete channel identifiers, a channel-priority mode, an allow-list, and a deny-list, all always present (never absent, even when empty) — via a direct, lossless, field-by-field construction requiring no additional resolution or transformation logic (see Key Entities and SC-001).
- **FR-015**: MUST NOT write to, modify, or otherwise manage `~/.condarc`.
- **FR-016**: MUST NOT fail, and MUST proceed through ordinary resolution, when `~/.condarc` contains a setting this ticket's work does not itself recognize and that is unrelated to channel/package selection. This is inherited directly from the condarc crate's own unknown-key tolerance and MUST NOT be separately re-implemented, narrowed, or weakened by `allez`'s own handling.
- **FR-017**: MUST resolve fresh from `~/.condarc` on every invocation; MUST NOT cache or reuse a previous resolution result across separate invocations.
- **FR-018**: MUST NOT remove, weaken, or reimplement any of `allez`'s existing, already-delivered ephemeral-environment behaviors that happen to overlap in subject matter with this ticket's own work — specifically GEN-24's empty-channel-list fallback (GEN-24 FR-015), GEN-24's allow/deny filtering, and GEN-24's defense-in-depth credential redaction on channel identifiers it formats (GEN-24 FR-003/FR-013) — on the mistaken assumption that this ticket's `resolve()` capability (FR-001–FR-005) supersedes them: each MUST keep passing its own existing, already-delivered GEN-24 tests unchanged after this ticket's adaptation layer (FR-014) is wired in (see Assumptions/Design Decisions, "Cleanup boundary", for why none of them is actually a duplicate). `allez`'s existing credential-redaction function is separately used for unrelated, non-channel-config purposes (redacting package-spec strings in error/debug output); this FR does not require touching that usage either.

### Key Entities *(include if feature involves data)*

- **User Channel Preferences**: The raw `~/.condarc` document as the user has configured it (or its absence). Read-only input to this ticket's work; never written to or otherwise managed by `allez`.
- **Resolved Channel Configuration**: The four-part artifact produced by the condarc crate's `resolve()` capability and adapted by `allez` into GEN-24's required shape (falling back to conda's own documented defaults wherever `~/.condarc` is missing, unreadable, or rejected): an ordered list of concrete channel identifiers, a channel-priority mode, an allow-list, and a deny-list, all four parts always present. This is the artifact every downstream capability that needs real channel preferences actually consumes, with no further translation. It is specifically these four parts — `resolve()`'s complete return value also includes the credential-stripping report FR-005 defines, which is a separate, non-channel-list part of that return value, not a fifth part of this entity.
- **Concrete Channel Identifier**: The form every entry in the resolved channel list, allow-list, and deny-list takes once FR-001's precedence rule has been applied — a fully-qualified channel URL in every supported case, including the `defaults` placeholder and any `custom_multichannels`/`custom_channels` bare name, since each of those resolves to a URL (through `channel_alias`, a multichannel's own members, or a `custom_channels` base-URL join, respectively) — except the one documented out-of-scope corner in Known Limitations (an explicit, empty-string `channel_alias`). GEN-24's own consuming field (`ChannelSpec.url_or_name`) is a plain, opaque string that would also accept a bare name — but this ticket's work never hands it one in the supported case; adapting a fully-qualified URL into that field is a direct, lossless string assignment (see FR-014), not a claim that no construction code exists at all.
- **Credential Material**: The two forms this ticket's work strips: URL userinfo (a `user:password@` prefix before the host) and a conda access-token path segment (a `/t/<token>/` segment). Never a broader or fuzzier definition than these two forms.
- **Channel Priority**: One of exactly three modes (`strict`, `flexible`, `disabled`) governing how strongly earlier channels in the resolved list are preferred over later ones during package resolution.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A caller obtains the complete four-part resolved channel configuration from any `.condarc` input within this ticket's supported scope (populated, missing, rejected, or unreadable — subject to the documented Known Limitations, notably GEN-36 Assumption A5's process-abort exception) — in exactly the shape the ephemeral-environment-creation capability (GEN-24) requires as its own input, constructible via a direct, lossless, field-by-field mapping with no additional resolution or transformation logic needed, verified by a dedicated contract test that constructs GEN-24's actual input type directly from the resolved output for at least 5 distinct, real-world-shaped `.condarc` samples.
- **SC-002**: A missing, rejected, or unreadable (permission/I-O error) `~/.condarc` never causes resolution itself to fail — 100% of such cases, within this ticket's supported scope (subject to the same Known Limitations exception as SC-001), produce a valid four-part resolved channel configuration using conda's own documented defaults, verified by automated tests covering a populated file, a missing file, a rejected file, and an unreadable file.
- **SC-003**: Every one of the originally-targeted `.condarc` channel-selection settings (`channels`, `channel_alias`, `custom_channels`, `custom_multichannels`, `default_channels`, `channel_priority`, `allowlist_channels`, `denylist_channels`) has at least one dedicated automated test asserting its exact expected resolved value, covering at minimum these 20 distinct scenarios:

  1. Bare-name/alias-only case (`channel_alias`)
  2. `custom_channels` exact-match case
  3. `custom_channels` progressive-prefix-match case
  4. `custom_multichannels` member naming another multichannel (not expanded further)
  5. `custom_multichannels` member naming a `custom_channels` entry (not expanded further)
  6. `defaults`-substitution: `channels` explicitly `[defaults]`
  7. `defaults`-substitution: `channels` absent
  8. `defaults`-substitution: `channels` explicit `null`
  9. `defaults`-substitution: `channels` explicit `[]`
  10. `default_channels` supplies a user-configured (non-built-in) value that is what actually gets substituted for `defaults`
  11. `channel_priority` = `strict`
  12. `channel_priority` = `flexible`
  13. `channel_priority` = `disabled`
  14. `channel_priority` absent (defaults to `flexible`)
  15. `channel_priority` = legacy boolean `true`
  16. `channel_priority` = legacy boolean `false`
  17. `allowlist_channels` case requiring FR-001 expansion
  18. `denylist_channels` case requiring FR-001 expansion
  19. `channels`/`channel` alias-collision (malformed)
  20. `allowlist_channels`/`whitelist_channels` alias-collision (malformed)

- **SC-004**: Every rejected-file fallback is recorded with the crate's own actionable, per-problem detail; every unreadable-file fallback is recorded with at least a minimal signal distinguishing it from the silent missing-file case. Verified by a dedicated test for each, with zero test runs where either fallback path emits no record at all.
- **SC-005**: Zero occurrences of credential material appear in the resolved output of a successful resolution, or in the structured-observability record this ticket's own work derives from the crate's stripping report (FR-013), verified by a dedicated test for each successfully-resolved credential-bearing case (in `channel_alias`, in `custom_channels`, in `default_channels`, in a `custom_multichannels` member, directly in a fully-qualified `channels` URL, and in an `allowlist_channels`/`denylist_channels` entry) asserting both the absence of credential material and the presence of the corresponding FR-013 record. Whether a rejected file's own per-problem detail (FR-012) can itself contain credential material is bounded entirely by the crate's own already-delivered error-reporting behavior (GEN-36) — out of this ticket's control to re-guarantee, consistent with FR-012's own scope — so no stripping report or FR-013 record is expected for that path, since `resolve()` never runs when `parse()` rejects the document.

## Assumptions

### Dependency & Process

- **GEN-36 boundary**: see Operating Context above for why and how this ticket's work spans both the condarc crate and `allez`. GEN-36's own Jira ticket is already closed and is not reopened; the new crate-side `resolve()` work is planned, implemented, and reviewed entirely as part of this ticket's own deliverable, in the crate's own codebase location (`crates/condarc`). This mirrors an existing crate precedent: `ParseOptions.null_sequence_map_defaults` (GEN-36's own Assumption A7) is an existing, narrowly-scoped, explicit opt-in that resolves one specific case to a conda default without touching FR-038's general no-defaulting posture — `resolve()` follows the same additive-opt-in spirit, just as a separate function rather than a `ParseOptions` flag, since it does substantially more than default substitution (full bare-name expansion and allow/deny resolution).
- **GEN-24 contract**: The ephemeral-environment-creation capability (GEN-24) is delivered and defines the exact four-part input shape this ticket's adapted output must match (see FR-014). The `allez oneshot` capability (GEN-25) is the production caller: it reads `~/.condarc` through this ticket's work and passes the result to GEN-24 directly.
- **No CLI or human-facing surface of its own**: This ticket's work produces its adapted output for a consuming capability to use directly.

### Design Decisions

- **Empty resolved list, two legitimate causes**: The resolved channel list can end up empty for more than one reason, none of which is a bug: a user explicitly configuring a named `custom_multichannels` definition (including one named `defaults`) or `default_channels` itself as an empty list is one; a downstream capability's own allow/deny filtering, applied *after* consuming this ticket's output, is another. Neither is invented by this ticket, and neither substitutes for the other.
- **Malformed/unreadable = missing, for resolution purposes**: this project runs unattended on behalf of an AI agent, so a `.condarc` problem entirely outside `allez`'s control must not block every subsequent operation — that is why FR-012 treats a crate-rejected or unreadable file the same as an absent one, rather than failing outright. What differs between the three cases is only observability (silent for absence, recorded with detail for the other two — FR-010/FR-012), never the outcome.
- **Allow/deny value-level conflicts vs. key-level alias collisions**: Reconciling a channel identifier that appears in both the resolved allow-list and deny-list is a downstream capability's own responsibility; this ticket's work only resolves both lists' entries, it does not compare or reconcile them. This is distinct from an alias *collision* (both `allowlist_channels` and `whitelist_channels` present as keys), which is treated as malformed input (FR-006).
- **Cleanup boundary**: The 2026-07-30 decision to move channel resolution into the condarc crate originally also scoped in removing "duplicate code in the ephemeral module, like credential redaction, once the crate covers it." On inspection, no code in `allez`'s existing ephemeral-environment module turned out to be a genuine duplicate: its defense-in-depth credential redaction independently fulfills GEN-24's own already-delivered contract (FR-003/FR-013) — a guarantee GEN-24 makes for *any* caller and *any* channel identifier it formats, not only the `.condarc`-sourced case this ticket's `resolve()` addresses. Its empty-channel-list fallback likewise fulfills GEN-24's own contract (FR-015) for the *initially-supplied* empty-list case specifically — GEN-24's own guarantee explicitly excludes, and continues to exclude, the case where allow/deny filtering is what causes the emptiness, which still fails cleanly rather than falling back; this ticket's `resolve()`-level defaulting (FR-001/FR-002) addresses a different, earlier case (an empty/absent/null `channels` key in `.condarc` itself) and does not change that exclusion. Its allow/deny filtering is GEN-24's own separate downstream concern (this ticket only resolves allow/deny *entries*, per FR-004, never applies them). Retiring any of them would regress GEN-24's own delivered guarantees, not remove a duplicate — FR-018 exists to prevent that regression, not to perform a cleanup.

### Known Limitations (Out of Scope)

| Behavior / Input | What real conda does | What this ticket's work does instead | Why it's out of scope |
|---|---|---|---|
| GEN-36's own numeric-parsing divergences (crate Assumptions A1, A4) | Accepts arbitrary-precision integers in any integer-typed setting (A1), and any Unicode decimal digit in a numeric-parsed setting, including `default_python`'s own numeric validation (A4) | Rejects an out-of-range integer in an integer-typed setting (A1), or a non-ASCII decimal digit in a numeric-parsed setting (A4), as ordinary crate-rejected content (FR-012), the same as any other rejection — an over-rejection relative to real conda, bounded entirely by the crate's own already-documented behavior | Delegated to the crate rather than reimplemented; see GEN-36 Assumptions A1/A4 |
| Empty-string `channel_alias` | Real conda accepts it (the scheme check is skipped for this one value) but the resulting bare-name-through-alias join is not a meaningful URL | Resolution behavior for a bare name expanded through an empty `channel_alias` is unspecified — an out-of-scope corner, the one documented exception to the Concrete Channel Identifier's fully-qualified-URL guarantee | No evidence any real `.condarc` sets `channel_alias` to an empty string; GEN-36's crate accepts it only because real conda's own validation happens to skip the scheme check for this one value, not because it is meaningful configuration |
| Empty-string, whitespace-only, or whitespace-padded bare channel-list entries (e.g. `channels: [""]`, a whitespace-padded name) | `Channel.from_value()` has documented, specific handling for these shapes | FR-001's precedence rule does not separately define scheme-detection, whitespace-trimming, or empty-name-entry behavior beyond whatever `channel_alias`-based expansion naturally produces from the literal string it's given | No evidence this ticket's target `.condarc` samples contain such entries; reproducing conda's full `Channel.from_value()` shape-detection logic is out of scope, matching the URL-canonicalization limitation below |
| GEN-36's own YAML key-length gap (crate Assumption A6) | Rejects a YAML simple key longer than 1024 characters | Accepts it (including as a `custom_multichannels` mapping key — a multichannel's own name) — an under-rejection relative to real conda, since the crate itself accepts it and this ticket's work never re-validates what the crate already accepted | Delegated to the crate; see GEN-36 Assumption A6 |
| GEN-36's own stack-depth guard gap (crate Assumption A5) | Raises a catchable error for a pathologically deep/large document | Has no equivalent guard and can abort the whole process rather than return a typed rejection FR-012 can catch | Documented, accepted limitation of the crate this ticket depends on; see GEN-36 Assumption A5 — this ticket's "never block" guarantee (User Story 2) does not cover a process abort of this kind |
| `custom_channels` via multichannel-member flattening | `context.custom_channels` additionally includes every multichannel's own member names | Matches only against the effective `custom_channels` map, without conda's own member-flattening | No evidence any real `.condarc` relies on this cross-referencing |
| Schemeless `custom_channels` value | Falls back to `channel_alias`'s own location | Unspecified — an out-of-scope corner | No evidence any real `.condarc` configures `custom_channels` with a schemeless value |
| `local` multichannel (conda-build) | Refers to a local package-build output directory | Treated as an ordinary bare name, expanded via `channel_alias` | Conda-build workflows are outside the parent epic's scope |
| Local filesystem path channel entries | Converted to a `file://` URL before any custom-map lookup | Treated as an ordinary bare name, expanded via `channel_alias` | This ticket's target usage has no expected need for local-path channels |
| URL canonicalization / channel equality | Parses and compares channels by parsed location/name/platform components | Treats a URL entry as an opaque string; no normalization | Reproducing conda's full URL-parsing/equality model is a materially larger undertaking than this ticket's resolution step |
| Recognized platform/subdir suffix (e.g. `defaults/linux-64`) | Stripped before lookup; platform applied to every resulting member | Left embedded in the entry's text, resolved with no platform semantics | Platform/subdir selection is `allez`'s own separate, downstream, environment-creation-time concern |
| "Unknown channel" sentinel / package-artifact filename | Special-cased | Resolved as an ordinary entry | Not a `.condarc`-configured channel-selection concept |
| `channel_settings` (per-channel auth/proxy) | Configures per-channel auth/proxy behavior | Never appears anywhere in the resolved output (FR-009) | Deferred to the private-channel-authentication capability (GEN-29) |
| `migrated_channel_aliases` / `migrated_custom_channels` | Legacy channel-migration settings | Not resolved at all | Out of scope; only the channel-selection settings known downstream consumers actually need are resolved |
