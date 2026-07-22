# Project Constitution

## Core Principles

### I. Code Quality

All code MUST adhere to the project's linting and formatting standards enforced via pre-commit hooks.
Code MUST pass `cargo fmt --check` and `cargo clippy -- -D warnings` before merge.
Functions and modules MUST have a single, clear responsibility.
Complexity MUST be justified and documented when unavoidable.
Unsafe code MUST be avoided; any exception MUST be isolated, documented with a `// SAFETY:` comment, and justified in the PR.

**Rationale**: Consistent, high-quality code reduces cognitive load, minimizes bugs, and ensures maintainability across the team.

### II. Testing Standards

All new functionality MUST have corresponding tests written BEFORE implementation (TDD).
Tests MUST follow the Red-Green-Refactor cycle: write failing test → implement → refactor.
Unit tests MUST be isolated, deterministic, and fast, and MUST live alongside the code under test (`#[cfg(test)]` modules) unless testing public API surface, in which case they belong under `tests/`.
Integration tests MUST cover cross-module boundaries and external interfaces.

**Rationale**: Test-first development catches defects early, documents expected behavior, and provides confidence for refactoring.

### III. Dual-Primary Interface (Agent and Human)

allez serves two co-primary audiences: agents/automation consuming structured output, and humans running commands interactively or debugging failures. Neither is a fallback of the other.
Every command MUST offer a fully-featured machine-readable (JSON) output mode with a documented, versioned schema.
Every command MUST also offer a fully-featured human-readable output mode, including verbose/trace output suitable for interactive debugging.
Format selection MUST be explicit (e.g. a `--format`/`--json` flag) or context-aware (e.g. TTY detection choosing the interactive default); either way, the selection rule MUST be documented.
Breaking changes to the JSON schema MUST follow semantic versioning (MAJOR bump); breaking changes to human-readable output or CLI flags SHOULD be communicated to the team before merge.
Exit codes MUST be stable and documented so agents can branch on them without parsing output.

**Rationale**: Agents need stable, parseable contracts; humans need readable output and debuggable failures. Treating either as secondary degrades the other audience's experience.

### IV. DRY (Don't Repeat Yourself)

Duplicated logic MUST be extracted into reusable functions, modules, or crates.
Configuration values MUST be defined in a single location.
Common patterns MUST be abstracted into shared utilities or traits.
Exceptions to DRY MUST be documented with rationale (e.g., intentional decoupling).

**Rationale**: Duplication leads to inconsistent behavior when one copy is updated but others are not.

### V. Explicit Over Implicit

Function behavior MUST be predictable from its signature, types, and doc comments.
Side effects MUST be documented and minimized.
Default values MUST be chosen for safety, not convenience.
Magic behavior (auto-detection, implicit conversion, blanket `From`/`Into` impls with surprising semantics) MUST be opt-in, not default.
Error types MUST be explicit and typed; `Result<T, E>` MUST be used instead of panics for recoverable errors, and `.unwrap()`/`.expect()` MUST NOT appear in library code outside of tests.

**Rationale**: Explicit code is easier to understand, debug, and maintain than clever implicit behavior, and typed errors keep failure modes visible to callers.

### VI. Documentation and Type Safety

All public functions, methods, structs, enums, and traits MUST have doc comments (`///`) describing purpose (less than 30 words), parameters, return values, and errors.
Public APIs MUST leverage Rust's type system to make invalid states unrepresentable (newtypes, enums over booleans/strings, `NonZero*`, etc.) rather than relying on runtime checks alone.
`cargo doc` MUST build without warnings.
Doc comments MUST include a runnable example for non-trivial public functions where practical.

**Rationale**: The type system is Rust's primary correctness tool; leaning on it over comments or runtime checks catches bugs at compile time. Doc comments provide self-documenting code and power `cargo doc`/IDE tooling.

### VII. No Hardcoded Values

Configuration values MUST be externalized (environment variables, config files, or CLI arguments).
Magic numbers MUST be defined as named constants with documentation.
File paths MUST be relative or configurable, never absolute hardcoded paths, and MUST be constructed with `Path`/`PathBuf` rather than manual separator concatenation to remain correct across Linux, macOS, and Windows.
Timeouts, limits, and thresholds MUST be configurable with sensible defaults.

**Rationale**: Hardcoded values prevent customization and require code changes for deployment variations; conda targets multiple platforms, so path handling must not assume one.

### VIII. Mandatory 100% Spec Test Coverage

All functionality defined in specifications MUST have corresponding test coverage (i.e., a spec test).
A spec test is a test that verifies an acceptance criterion defined in the specification document. Each acceptance criterion MUST map to at least one test.

**Rationale**: Complete spec coverage ensures all documented behavior is verified and prevents undocumented regressions.

### IX. Determinism & Idempotency

Given identical inputs (manifest, lockfile, environment), resolution and install operations MUST produce identical results.
Re-running a completed operation MUST be idempotent: no duplicated side effects, no drift from the expected end state.
`Cargo.lock` MUST be committed to version control (allez is an application, not a library).
Any project-level lockfile allez produces for users MUST be similarly deterministic and diff-friendly.

**Rationale**: Agents retry and re-run operations without always inspecting prior state; non-deterministic or non-idempotent behavior silently corrupts environments and erodes trust in automation.

### X. Security & Supply-Chain Integrity

This principle has two distinct scopes:

- **Our development supply chain**: `cargo audit` and `cargo deny` (vulnerabilities, license compliance, banned/duplicate crates) MUST run in CI and block merge on failure.
- **allez's runtime behavior toward its users**: package artifacts allez downloads and installs on behalf of a user or agent MUST be checksum/signature-verified before being written to disk or activated. allez MUST NOT execute arbitrary post-install/build scripts from untrusted sources without explicit, documented consent.

**Rationale**: A package manager is a supply-chain trust boundary in both directions — the crates we depend on to build allez, and the packages allez installs for others. Both must be guarded.

### XI. Structured Observability

Logging MUST use a structured logging framework (e.g. `tracing`) with consistent fields (operation, package, duration, result), not ad hoc `println!`/`eprintln!`.
Observability is dual-primary, matching Principle III: a human-readable formatter MUST be available for interactive debugging, and a structured/JSON formatter MUST be available for agent-driven pipelines and log aggregation.
Errors surfaced across process boundaries (CLI exit, JSON output) MUST carry a stable error code/category in addition to a human-readable message.

**Rationale**: Debugging a failed agent run and debugging a failed interactive run require different lenses on the same underlying events; both must be first-class, not reconstructed after the fact.

## Quality Gates

All code MUST pass before merge:

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo audit` and `cargo deny check`
- Full test suite (`cargo test --all`) with coverage report, run on all supported target platforms (Linux, macOS, Windows)
- `cargo doc` with no warnings
- PR review by at least one maintainer
- No decrease in test coverage percentage

All PRs MUST include:

- Tests for new functionality
- Updated doc comments for changed public APIs
- Changelog entry for user-visible changes

## Development Workflow

1. **Specification**: Define requirements and acceptance criteria before coding.
2. **Test-First**: Write tests that verify the specification.
3. **Implementation**: Write minimal code to pass tests.
4. **Refactor**: Improve code quality while maintaining passing tests.
5. **Review**: Submit PR for peer review against quality gates.
6. **Merge**: Squash merge to main after approval.

All development MUST occur in feature branches.
Main branch MUST always be in a deployable state.
Breaking changes MUST follow semantic versioning, and any change to public crate APIs MUST be evaluated against SemVer compatibility (see the `cargo-semver-checks` tool where applicable).

## Governance

This constitution supersedes all other development practices for this project.

**Amendment Process**:

1. Propose changes via PR with rationale.
2. Obtain approval from project maintainers.
3. Document migration plan for existing code if needed.
4. Update version according to semantic versioning.

**Compliance**:

- All PRs and reviews MUST verify compliance with these principles.
- Deviations MUST be documented and approved by maintainers.
- Periodic audits SHOULD verify codebase adherence.

**Versioning Policy**:

- MAJOR: Backward-incompatible principle changes or removals.
- MINOR: New principles or materially expanded guidance.
- PATCH: Clarifications, wording improvements, typo fixes.

**Version**: 1.0.0 | **Ratified**: 2026-07-21 | **Last Amended**: 2026-07-21
