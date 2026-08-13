# Feature Specification: Ephemeral Environment Default Packages and User Overrides

**Feature Branch**: `GEN-30_default_package_overrides`

**Created**: 2026-08-07

**Jira**: [GEN-30](https://anaconda.atlassian.net/browse/GEN-30) — Ephemeral environment default packages and user overrides (parent epic: [GEN-19](https://anaconda.atlassian.net/browse/GEN-19))

**Operating Context**: `allez` runs inside a sandbox, invoked by an AI agent rather than a human (see the parent epic). This feature governs only ephemeral environments (the `allez oneshot` capability, GEN-25) — path-based, persistent environments (GEN-26/GEN-27) already require the caller to name every package explicitly and are unaffected. The default package set for an ephemeral environment is exactly whatever the condarc crate (GEN-36), via the same `.condarc` handling GEN-23 already uses, resolves the user's `create_default_packages` setting to. `allez` adds no behavior of its own on top of that — how that resolution behaves is not this ticket's concern to define, redescribe, or guard against. `allez oneshot`'s existing per-invocation package list is how a caller adds to, or overrides one entry of, that resolved default set.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - My own `.condarc` decides my ephemeral environments' defaults (Priority: P1)

A user who configures (or doesn't configure) `create_default_packages` in their own `~/.condarc` — a real conda setting, not something specific to `allez` — expects `allez oneshot` to honor exactly what it resolves to, without needing any `allez`-specific configuration of its own.

**Why this priority**: This is the entire reason this ticket exists. Every other behavior in this feature only matters once a resolved default set — whatever it is — already exists to add to or override a piece of.

**Independent Test**: Can be fully tested by (a) configuring `create_default_packages` to a list in `.condarc` and confirming an ephemeral environment created with no per-invocation packages gets exactly that list; and (b) leaving it unconfigured (or configuring it as an empty list) and confirming the resulting environment has no default packages beyond whatever `create_default_packages` itself resolves to in that case.

**Acceptance Scenarios**:

1. **Given** the user's `.condarc` configures `create_default_packages` to a list, and no per-invocation packages are named, **When** an ephemeral environment is created, **Then** its Effective Package Set is exactly that list.
2. **Given** the user's `.condarc` does not configure `create_default_packages`, or resolves it to an empty list, and no per-invocation packages are named, **When** an ephemeral environment is created, **Then** its Effective Package Set is empty — `allez` does not substitute a list of its own.

---

### User Story 2 - Add a package, or override one by version, per invocation (Priority: P2)

An agent that mostly wants the resolved default package set, but for this one run also needs an extra package, or a different version of a package the default set already includes, wants to just name it on the `allez oneshot` command line rather than editing `.condarc`.

**Why this priority**: This refines User Story 1 rather than introducing new mechanics — it matters once a resolved default set already exists for a per-invocation request to add to or adjust.

**Independent Test**: Can be fully tested by (a) naming a per-invocation package that shares no name with the resolved default set, and confirming the resulting Effective Package Set is the default set plus that package; and (b) naming a per-invocation package whose bare name matches a default entry, and confirming the per-invocation package's own spec is what ends up in the Effective Package Set for that name, not the default's.

**Acceptance Scenarios**:

1. **Given** one or more per-invocation packages are named that share no bare name with the resolved default package set, **When** `allez oneshot` is invoked, **Then** the Effective Package Set is the default set plus every named package.
2. **Given** a per-invocation package's bare name matches an entry in the resolved default package set, **When** `allez oneshot` is invoked, **Then** the per-invocation package's own spec supersedes that default entry — both are never retained together.
3. **Given** `skills/allez-oneshot.md`, **When** it is read, **Then** it explicitly states both that ephemeral environments' default packages come from the user's own `.condarc` `create_default_packages` setting, and the precedence between per-invocation packages and that resolved default package set.

---

### Edge Cases

- What happens when `create_default_packages` is unconfigured, or resolves to an empty list? (The Effective Package Set has no default entries — `allez` does not substitute a fallback list of its own; see FR-002.)
- What happens when `~/.condarc` itself is missing, malformed, or unreadable? (Treated identically to `create_default_packages` being absent — no default entries — per FR-001's shared location/fallback semantics and FR-002's exact-resolution rule; no allez-specific fallback list is substituted for this case either.)
- What happens when a per-invocation package's bare name matches a resolved default entry? (The per-invocation package supersedes it — see FR-004.)
- What happens to a project- or repository-local configuration file, distinct from the user's own `~/.condarc`? (Not consulted — this ticket, like GEN-23, only ever reads the invoking user's own `~/.condarc`; see Assumptions.)

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: `allez` MUST resolve the default package set for an ephemeral environment from the user's own `.condarc` `create_default_packages` setting, using the exact same `~/.condarc` location and missing-or-malformed-file fallback semantics GEN-23 establishes for channel preferences — not a new, separate `allez`-specific config mechanism or a different fallback behavior for this setting.
- **FR-002**: `allez` MUST use whatever `create_default_packages` resolves to, including an empty list, as the default package set — with no `allez`-specific fallback, validation, or special-casing of its own on top of that resolution.
- **FR-003**: `allez oneshot`'s per-invocation packages (named before `--`) MUST be added to the resolved default package set, not replace it.
- **FR-004**: When a per-invocation package's bare package name matches an entry in the resolved default package set, the per-invocation package MUST supersede that entry — the default entry is dropped and only the per-invocation package's own spec is retained, regardless of which of the two carries a version or build constraint — rather than both being retained.
- **FR-005**: `allez` MUST document, in `skills/allez-oneshot.md` (this project's existing human- and agent-readable reference for `allez oneshot`), both that ephemeral environments' default packages come from the user's own `.condarc` `create_default_packages` setting, and the precedence between per-invocation packages and that resolved default package set.

### Key Entities *(include if feature involves data)*

- **Default Package Set**: Whatever the user's `create_default_packages` setting resolves to, via the existing `.condarc` handling (GEN-23/GEN-36).
- **Effective Package Set**: The top-level package request resolved for a given ephemeral environment: the Default Package Set, with every per-invocation package added on top, superseding any default entry of the same bare package name. Each package's own dependencies are installed in addition, as with any package installation — this entity describes the top-level request only, not the full installed closure.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of ephemeral-environment creations with no per-invocation packages result in an Effective Package Set exactly equal to whatever `create_default_packages` resolves to for that invocation, including empty.
- **SC-002**: 100% of `allez oneshot` invocations that name one or more per-invocation packages result in an Effective Package Set containing every one of those packages, plus every resolved default entry whose bare name isn't superseded by one of them.
- **SC-003**: 100% of Effective Package Set resolutions where a per-invocation package's bare name matches a resolved default entry result in the per-invocation package superseding it — zero cases where both appear.
- **SC-004**: The precedence between per-invocation packages and the resolved default package set (documented per FR-005) is verified by at least one automated test for each of: no per-invocation packages named (Effective Package Set equals the resolved default set exactly), a per-invocation package sharing no bare name with the default set (additive), and a per-invocation package's bare name matching a default entry (supersede).

## Assumptions

- No `allez`-specific built-in default package list exists. Where the Jira ticket's acceptance criteria call for "a documented default package list," that is satisfied by documenting the `create_default_packages` mechanism itself (FR-005), not by `allez` shipping a hardcoded list in its own code.
- Per-invocation packages add to, and can supersede a same-named entry within, the resolved default package set (FR-003/FR-004) — both GEN-24 (User Story 3, Acceptance Scenario 3) and GEN-25 (FR-001) name this ticket as the one that defines that precedence.
- This ticket's requirements (FR-001–FR-004) are authoritative over GEN-24's and GEN-25's own delivered specification text wherever the two disagree: GEN-24's fixed, non-empty, built-in default package set and its own override mechanism (GEN-24 FR-005/FR-006) are retired in favor of FR-001/FR-002's `create_default_packages`-only source, and GEN-24's/GEN-25's replace-only precedence between explicit and default packages is retired in favor of FR-003/FR-004's additive precedence — both are expected discrepancies between this ticket and those two, not unintended contradictions.
- Two or more per-invocation packages sharing the same bare name (e.g. two version constraints for the same package named on one command line) is governed entirely by `allez oneshot`'s own existing per-invocation package-list handling (GEN-25) — FR-004's precedence rule governs only a per-invocation package's bare name colliding with a resolved *default* entry, not a collision among per-invocation entries themselves.
- A project- or repository-local configuration, distinct from the invoking user's own `~/.condarc`, is out of scope — this ticket, like GEN-23, only ever reads the invoking user's own configuration.
- This feature governs ephemeral environments only (`allez oneshot`, GEN-25); path-based, persistent environments (`allez create`/`allez run`, GEN-26/GEN-27) require every package to be named explicitly at creation time and are unaffected.
