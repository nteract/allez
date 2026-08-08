# Implementation Plan: Ephemeral Environment Default Packages and User Overrides

**Branch**: `GEN-30_default_package_overrides` | **Date**: 2026-08-07 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/GEN-30_default_package_overrides/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command; its definition describes the execution workflow.

## Summary

`allez oneshot`'s default package set for an ephemeral environment is exactly whatever the user's own `~/.condarc` `create_default_packages` setting resolves to, via `condarc::Config`'s already-parsed `create_default_packages` field (GEN-36); no compiled-in `allez`-authored default list exists. Per-invocation packages (named before `--`) are added to that resolved set; a per-invocation package whose bare name matches a default entry supersedes that one entry (FR-003/FR-004), never both.

Technical approach: a new `src/channel_config/document.rs` submodule — nested inside the existing `channel_config` module, not a new crate-root file; `locate.rs`/`events.rs` stay exactly where they are — owns `.condarc` location/read/parse/fallback logic and hands one parsed `CondarcDocument` per invocation to two independent consumers — `channel_config` itself (channels) and the new top-level `default_packages_config` module (`create_default_packages` extraction, via research.md's decisions). `src/ephemeral/defaults.rs`'s `effective_packages(explicit, defaults)` implements FR-003/FR-004's additive/supersede merge by bare package name; `create_ephemeral_environment(explicit: Vec<PackageSpec>, defaults: Vec<PackageSpec>, channels: ResolvedChannels)` calls it itself, as its own first step, so every caller of this feature's one public entry point gets that precedence applied identically, not only `oneshot.rs`. `skills/allez-oneshot.md` gains the FR-005-required documentation. Full design rationale lives in `research.md`; this plan covers scope, structure, and the constitution gate only.

## Technical Context

**Language/Version**: Rust, `edition = "2024"`.

**Primary Dependencies**: No new crate. `condarc` already parses `create_default_packages` into `Config::create_default_packages: Option<Vec<String>>`. `rattler_conda_types` gains one new call site: a `MatchSpec::from_str` reparse of an already-validated `PackageSpec`'s own string, to extract its `PackageNameMatcher` for bare-name comparison (research.md) — no `Cargo.toml` feature change.

**Storage**: N/A. The one file this feature reads is `~/.condarc` itself, read-only, never cached across invocations, read at most once per `allez oneshot` invocation — skipped entirely if per-invocation package parsing fails first, since that failure returns before `.condarc` resolution runs (`data-model.md`'s `src/cli/oneshot.rs` orchestration, step 1 before step 2).

**Testing**: `cargo test --all --features test-config-override`. New/changed unit tests in `src/ephemeral/defaults.rs` (the merge algorithm), `src/default_packages_config.rs` (new), and `src/channel_config/document.rs` (the new `.condarc`-document tests, colocated with `locate.rs`/`events.rs` rather than relocating either); new end-to-end scenarios in `tests/oneshot_exec.rs`, including `scenario_gen30_ignores_project_local_condarc` (spec.md's project-local-config Edge Case, run via subprocess-scoped `.current_dir(...)` rather than any in-process state mutation); one new `tests/support/defaults.rs` test exercising the merge directly at the `create_ephemeral_environment` layer; a new standalone `tests/skills_doc.rs` binary asserting FR-005's documentation content. GEN-24's enum-based, replace-only `RequestedPackages`/`default_override` shape no longer exists in any form, so `tests/support/defaults.rs`'s three existing replace-only-precedence tests have no one-for-one equivalents at that layer — see this plan's own Project Structure entry for that file for the exact per-test mapping, and `research.md` § Test strategy and `quickstart.md` for the full SC-00n/FR-0nn → test mapping.

**Target Platform**: Windows amd64, macOS aarch64, Linux aarch64/amd64. Nothing in this ticket's scope is platform-sensitive — `create_default_packages` is a plain string list, and bare-name extraction is pure string processing with no OS dependency.

**Project Type**: Internal library restructuring inside `allez`'s existing single binary/library target — no new CLI surface, no new public crate. One new private module (`src/default_packages_config.rs`), one new private submodule nested inside an existing one (`src/channel_config/document.rs` — `locate.rs`/`events.rs` are not moved or deleted), and targeted signature changes to two existing modules (`src/ephemeral/{defaults.rs,mod.rs}`) plus their orchestration call site (`src/cli/oneshot.rs`).

**Performance Goals**: Not defined by this ticket (no Performance Goal in spec.md). `effective_packages`/bare-name extraction operate on a small, in-memory package list, not a hot path. This ticket's design reads `~/.condarc` at most once per invocation and hands the parsed result to both the channel-resolution and default-package-resolution steps.

**Constraints**: `default_packages_config::create_default_packages_from_document` performs no I/O of its own (FR-002's "no fallback... of its own" implies no independent read/retry logic), never panics, and is infallible — it returns `Vec<PackageSpec>` directly, not a `Result`, since FR-002 forbids `allez` from rejecting any value `create_default_packages` itself resolves to (research.md's "never rejects a resolved entry" Decision) — same `clippy::unwrap_used`/`clippy::expect_used` deny already enforced crate-wide (`src/lib.rs`). `effective_packages` never mutates its inputs and is a pure function of `(explicit, defaults)` — Constitution IX (Determinism): identical inputs always produce an identical Effective Package Set as a deterministic implementation property (order is not a guarantee the contract makes to callers — `contracts/default_package_resolution_contract.md`). `create_default_packages` is never written to (`~/.condarc` stays read-only). No `allez`-authored fallback package list exists in this design (FR-002; research.md's "No built-in fallback package list" Decision). `allez`'s own `condarc::parse()` call site uses `ParseOptions::default()`; this does not affect `create_default_packages`'s own resolution (research.md's "inherited, not re-tested" Decision spells out why).

**Scale/Scope**: One new top-level module (`src/default_packages_config.rs`) and one new nested submodule (`src/channel_config/document.rs`), targeted signature/delegation changes across `src/channel_config/mod.rs`, `src/ephemeral/{defaults.rs,mod.rs}`, `src/ephemeral/solve.rs` (test-only — see below), `src/cli/oneshot.rs`, `src/lib.rs`'s module declarations, and `examples/ephemeral_smoke.rs`'s call site; one documentation addition (`skills/allez-oneshot.md`) and one documentation correction (`tests/fixtures/ephemeral_channel/README.md`'s "Default-package candidates" section); several `tests/support/*.rs` call-site updates and one new test in `src/ephemeral/solve.rs` (asserting that `solve_packages` succeeds on an empty `packages` slice, returning an empty `records` list, per FR-002/US1-AS2). No production code in `solve.rs` needs to change for this: `Gateway::query(...).execute()` already short-circuits on an empty spec list (`rattler_repodata_gateway`'s own `execute()` returns `Ok(RepoDataQueryOutput::default())` before any network call when `self.specs.is_empty()`), and `resolvo`'s solve path builds its root-requirement set from `task.specs.into_iter().flat_map(...)`, which is simply empty for an empty input — no rejection logic exists in either dependency for a zero-length spec list; the new test locks in this already-correct behavior as a regression guard, it does not drive a production fix. `tests/support/creation.rs`'s `lifecycle_events_include_consistent_ids_packages_and_durations` asserts the lifecycle event's `packages` field against its input — that field's schema is unchanged, but its contents reflect the merged Effective Package Set, so this test's own fixture input needs re-verifying against the merge, not just a call-site signature update. No schema/migration/multi-service scope; `condarc::Config`'s `create_default_packages` field is consumed as-is, unchanged (GEN-36).

## Constitution Check

*GATE: Every principle below MUST PASS.*

| Principle | Assessment | Gate |
|-----------|-----------|------|
| I. Code Quality | Each touched/new module keeps one clear responsibility: `channel_config::document` (I/O + fallback observability only), `default_packages_config` (pure `create_default_packages` extraction), `channel_config` (channel semantics, unchanged public API). `src/ephemeral/defaults.rs` has no enum variant whose name would misdescribe additive/supersede precedence. | PASS |
| II. Testing Standards | TDD: `research.md` § Test strategy and `quickstart.md` name every test the implementation must satisfy, each written before its corresponding implementation per this workspace's existing convention. Unit tests for `effective_packages`/`create_default_packages_from_document`/`channel_config::document` live alongside their code (`#[cfg(test)]`); end-to-end coverage lives under `tests/`. | PASS |
| III. Dual-Primary Interface | No CLI/JSON contract change (GEN-25's `contracts/oneshot_cli_contract.md` governs the CLI surface itself — same exit codes, same error envelope shape; only which packages get resolved changes, reusing the existing `unresolvable_package` category for a malformed default-set entry). `skills/allez-oneshot.md`'s new subsection is this ticket's own human-*and*-agent documentation update (FR-005), matching this principle's dual-audience framing directly. | PASS |
| IV. DRY | The entire architectural core of this ticket (research.md's first Decision) exists specifically to satisfy this principle: sharing one `.condarc` read/parse/fallback implementation across two consumers, instead of a second, independently-fallback-logic'd copy for default packages. | PASS |
| V. Explicit Over Implicit | `create_ephemeral_environment`'s `(explicit, defaults, channels)` signature takes both raw inputs directly and resolves them into the Effective Package Set itself, via `effective_packages`, as its own first step — no enum variant for either input, so FR-003/FR-004's precedence is enforced inside the one function every caller of this feature already goes through, not hidden behind a caller-side precondition. `explicit`/`defaults` are adjacent, same-typed `Vec<PackageSpec>` parameters rather than a wrapper struct: a dedicated newtype pair would guard against a swapped-argument mistake that SC-004's own three test cases (distinguishing which side wins a collision) already catch, at the cost of a wrapper this ticket's own call sites don't otherwise need. `PackageSpec::from_resolved_default` is itself an explicit, narrowly-documented second constructor (FR-002's "never rejects a resolved entry" requirement), not a hidden bypass of `PackageSpec`'s validated-by-default contract. Typed `Result`/`InvalidPackageSpec` errors throughout; no new `.unwrap()`/`.expect()` in this ticket's own code. | PASS |
| VI. Documentation & Type Safety | Every new `pub(crate)` item gets a `///` doc comment, matching this codebase's existing convention. `default_packages_config`/`channel_config::document` are private; `effective_packages`/`parse_explicit_packages`/`bare_name` are `pub(crate)`. This ticket changes `create_ephemeral_environment`'s signature to `(explicit, defaults, channels)` and removes `DEFAULT_PACKAGES`/`RequestedPackages` outright — the minimal, mechanical consequence of enforcing FR-003/FR-004's additive/supersede precedence inside the one function every caller already goes through; `InvalidPackageSpec`/`PackageSpec` and `ephemeral`'s other public items are unaffected. This is a breaking change to `allez`'s public library API, acceptable without a compatibility shim: `Cargo.toml` sets `version = "0.1.0"`, `allez` has no published release, and every direct caller (`examples/ephemeral_smoke.rs`, `tests/support/*.rs`, `src/cli/oneshot.rs`) is updated within this same ticket. | PASS |
| VII. No Hardcoded Values | No `allez`-authored default package list exists in this design: the default package set is always externally configured via `.condarc`, never a compiled-in constant. | PASS |
| VIII. 100% Spec Test Coverage | `quickstart.md`'s acceptance-scenario → test table maps every US1-AS1/US1-AS2/US2-AS1/US2-AS2/SC-004 case, plus FR-005, to a named test — `skills_doc_states_default_package_source_and_precedence` (`tests/skills_doc.rs`) gives FR-005 an automated substring-content assertion rather than relying on human review alone. | PASS |
| IX. Determinism & Idempotency | `effective_packages` is a pure function; `.condarc` resolution is read at most once per invocation, never cached across invocations — identical `.condarc` + identical CLI packages always produce an identical Effective Package Set (as a deterministic implementation property, not an ordering guarantee the contract makes to callers — see `contracts/default_package_resolution_contract.md`'s own note on this). This ticket does not touch `create_ephemeral_environment`'s own fresh-environment-per-call behavior — GEN-24's Constitution Check already records, as a team-approved deviation, that each call is a new one-shot operation by product design rather than a deduplicated retry, so Principle IX's "re-running a completed operation MUST be idempotent" clause does not apply to it; GEN-25's own Constitution Check cites the same precedent rather than re-litigating it, and this ticket does likewise. | PASS |
| X. Security & Supply-Chain | No new dependency; no new package-artifact trust boundary — a `create_default_packages` entry that survives the merge (is not superseded per FR-004) reaches the exact same checksum-verified solve/install path (`src/ephemeral/solve.rs`) an explicit CLI package already does, whether or not it happens to be a valid match-spec (research.md's "never rejects a resolved entry" Decision) — `solve_packages`'s own existing per-package `MatchSpec::from_str` call is what ultimately rejects a malformed one, uniformly regardless of source. | PASS |
| XI. Structured Observability | No new `tracing` event type. `channel_config::resolve_document`'s single fallback event serves both `channel_config` and `default_packages_config`; `channel_config::channels_from_document` may additionally emit its own `FallbackReason::Rejected` event for a channel-expansion-specific failure distinct from document parsing — this ticket does not change that existing behavior. | PASS |

**Gate: PASS.** No principle violations requiring Complexity Tracking — every design choice above is a direct, minimal reading of FR-001–FR-005 plus this workspace's own existing DRY/single-responsibility conventions, not an added abstraction layer.

The Quality Gates section's "changelog entry for user-visible changes" requirement (`.specify/memory/constitution.md`) is a per-PR merge-time deliverable, not a `/speckit.plan`-phase one: no changelog file is part of this plan's own Project Structure.

## Project Structure

### Documentation (this feature)

```text
specs/GEN-30_default_package_overrides/
├── plan.md                                        # This file (/speckit.plan command output)
├── research.md                                    # Phase 0 output (/speckit.plan command)
├── data-model.md                                  # Phase 1 output (/speckit.plan command)
├── quickstart.md                                  # Phase 1 output (/speckit.plan command)
├── contracts/
│   └── default_package_resolution_contract.md     # Phase 1 output (/speckit.plan command)
└── tasks.md                                        # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source code (repository root)

Single-project Rust workspace (unchanged shape from GEN-23/24/25/36 — no new crate, no new binary/frontend split).

```text
crates/condarc/                    # UNCHANGED — create_default_packages already parsed (GEN-36)

src/
├── lib.rs                          # CHANGED — one new private `mod` declaration: default_packages_config
├── default_packages_config.rs      # NEW (private) — create_default_packages_from_document():
│                                    #   create_default_packages -> Vec<PackageSpec>, infallible
├── channel_config/
│   ├── mod.rs                      # CHANGED — declares new `mod document;`; resolve_channel_config_from
│   │                                #   delegates to document::resolve_document_from + new channels_from_document;
│   │                                #   public API/behavior unchanged
│   ├── document.rs                 # NEW (private, nested — not a crate-root module) — CondarcDocument,
│   │                                #   resolve_document[_from], built on locate.rs's pub(super) read_condarc,
│   │                                #   mod.rs's own private default_condarc_path/condarc_path_override
│   │                                #   (already visible to a child module, no visibility change needed),
│   │                                #   and events.rs's pub(super) emit_fallback (no file move)
│   ├── locate.rs                   # UNCHANGED — ReadOutcome/
│   │                                #   read_condarc stay exactly where they are
│   └── events.rs                   # UNCHANGED — FallbackReason (stays pub), emit_fallback stay exactly
│                                    #   where they are
├── ephemeral/
│   ├── defaults.rs                 # CHANGED — `DEFAULT_PACKAGES` (the constant) and `RequestedPackages`
│   │                                #   (the enum) are deleted outright, along with every one of their
│   │                                #   own existing unit tests (e.g. `no_override_falls_back_to_default_packages`,
│   │                                #   `override_resolving_to_empty_falls_back_to_default_packages`,
│   │                                #   `explicit_non_empty_wins_over_any_override`) — the additive/supersede
│   │                                #   precedence this ticket introduces has no "use the built-in default
│   │                                #   instead" branch for either to represent; public re-exports are
│   │                                #   `InvalidPackageSpec`/`PackageSpec` only (Constitution VI); the existing
│   │                                #   `effective_packages` is CHANGED, not added — its signature rewrites
│   │                                #   from `(requested: &RequestedPackages, default_override: Option<&[PackageSpec]>)`
│   │                                #   to `(explicit: &[PackageSpec], defaults: &[PackageSpec])`, its semantics
│   │                                #   from replace-only to additive/supersede, and its visibility narrows
│   │                                #   from `pub` to `pub(crate)`; its existing sole call site inside
│   │                                #   `create_ephemeral_environment`'s own body (`src/ephemeral/mod.rs`) is
│   │                                #   re-signatured in place, not newly wired; new: pub(crate)
│   │                                #   parse_explicit_packages, bare_name (-> Option<String>), and pub(crate)
│   │                                #   PackageSpec::from_resolved_default (non-validating, infallible
│   │                                #   second constructor, used only for condarc-resolved entries)
│   ├── mod.rs                      # CHANGED — create_ephemeral_environment(explicit, defaults,
│   │                                #   channels): calls effective_packages(&explicit, &defaults)
│   │                                #   itself, as its own first step, before installing anything —
│   │                                #   a breaking public-API signature change (Constitution V/VI)
│   └── solve.rs                    # CHANGED (test only) — new solve_packages_empty_input_returns_no_records
│                                    #   test against the fixture channel: an empty `packages` input must
│                                    #   succeed through the same `Gateway::query`/`SolverTask` path any
│                                    #   other input uses, returning an empty `records` list, per FR-002/US1-AS2;
│                                    #   non-empty-input behavior stays untouched
└── cli/
    └── oneshot.rs                  # CHANGED — orchestrates channel_config::document + channel_config +
                                     #   default_packages_config, then calls create_ephemeral_environment
                                     #   directly with the unmerged explicit/defaults lists; one
                                     #   .condarc read per invocation; never calls effective_packages itself

examples/
└── ephemeral_smoke.rs              # CHANGED — call-site update for create_ephemeral_environment's new signature

tests/
├── ephemeral_env.rs                 # UNCHANGED — the test binary target; wires in the four
│                                     #   tests/support/*.rs files below directly via its own
│                                     #   `#[path = "support/*.rs"] mod ...;` declarations, not
│                                     #   through tests/support/mod.rs (that file only declares
│                                     #   `pub mod adapter;`, for the unrelated tests/condarc_conformance.rs
│                                     #   test binary, GEN-36 — out of this ticket's scope)
├── support/
│   ├── ephemeral.rs                 # CHANGED — package_specs/explicit_package_specs helpers use plain
│   │                                 #   Vec<PackageSpec> throughout
│   ├── creation.rs                  # CHANGED — call-site updates for the new signature; its own
│   │                                 #   `empty_package_list_resolves_default_packages_the_fixture_channel_cannot_satisfy`
│   │                                 #   is removed outright — under FR-002, empty explicit packages plus
│   │                                 #   empty defaults succeeds with zero installed packages, an outcome
│   │                                 #   US1-AS2 covers end-to-end; `lifecycle_events_include_consistent_ids_packages_and_durations`
│   │                                 #   needs its `packages`-field assertion re-verified against the new merge
│   ├── failures.rs                  # CHANGED — call-site updates for the new signature (behavior unchanged)
│   └── defaults.rs                  # CHANGED — its own three existing replace-only-precedence tests are
│                                     #   removed with no one-for-one equivalents for their retired
│                                     #   semantics: `no_packages_with_an_override_installs_the_override_instead_of_defaults`'s
│                                     #   own scenario matches US1-AS1's (no per-invocation packages, resolved
│                                     #   defaults alone); `an_override_resolving_to_empty_falls_back_to_default_packages_...`'s
│                                     #   matches US1-AS2's (empty resolved defaults); `explicit_packages_alongside_an_override_ignore_the_override_entirely`'s
│                                     #   own two packages (`fixture-default-alpha`/`fixture-default-beta`)
│                                     #   share no bare name, so its own replace-all premise (explicit present
│                                     #   at all fully replaces defaults, regardless of any name match) has no
│                                     #   equivalent under additive precedence at all — a non-colliding explicit
│                                     #   package is added, never a full replacement — and its own concrete
│                                     #   scenario matches US2-AS1's; none of these three tests ever named
│                                     #   colliding bare names in the first place (GEN-24's replace-only rule
│                                     #   never depended on name matching); this file's own new test,
│                                     #   `create_ephemeral_environment_supersedes_matching_default_entry_by_bare_name`,
│                                     #   calls `create_ephemeral_environment` directly with a colliding
│                                     #   `(explicit, defaults)` pair, proving the merge is wired into the
│                                     #   function's own body (research.md's "Merge coverage layers") — the
│                                     #   actual bare-name-collision proof this file's own removed tests never
│                                     #   attempted (GEN-24's replace-only rule never depended on name matching);
│                                     #   `oneshot_exec.rs`'s US2-AS2 end-to-end scenario is the other, heavier
│                                     #   layer proving the same collision — neither replaces the other
├── oneshot_exec.rs                  # CHANGED — new end-to-end scenarios (quickstart.md's US1-AS1/AS2,
│                                     #   US2-AS1/AS2), reusing fixture-default-alpha via a test .condarc's
│                                     #   create_default_packages: key; new scenario_gen30_ignores_project_local_condarc
│                                     #   (spec.md's project-local-config Edge Case) runs the subprocess
│                                     #   with `.current_dir(...)` pointed at a temporary directory holding
│                                     #   its own decoy `.condarc` (`create_default_packages: [fixture-default-beta]`)
│                                     #   while `ALLEZ_CONDARC_PATH` still points at the harness's own real
│                                     #   test `.condarc` naming fixture-default-alpha — asserting only
│                                     #   fixture-default-alpha is installed, never fixture-default-beta;
│                                     #   `scenario_1_2_zero_packages_routes_through_resolution_not_usage_error`
│                                     #   is removed outright — its own assertion (empty per-invocation
│                                     #   packages with no `create_default_packages` key must fail as
│                                     #   `unresolvable_package`) is incompatible with FR-002, under which
│                                     #   that same invocation succeeds with zero installed packages (the
│                                     #   new US1-AS2 scenario)
└── skills_doc.rs                    # NEW — standalone test binary: reads skills/allez-oneshot.md and
                                      #   asserts its ### Default packages subsection states both the
                                      #   create_default_packages/~/.condarc source fact and the
                                      #   add/supersede precedence fact (FR-005)

skills/
└── allez-oneshot.md                 # CHANGED — new ### Default packages subsection inside ## Usage,
                                      #   before ### Examples, documenting default-package source + precedence (FR-005)

tests/fixtures/
└── ephemeral_channel/README.md     # CHANGED — "Default-package candidates" section states that no
                                      #   built-in default package list exists and that the zero-packages
                                      #   case succeeds with an empty installed set
```

**Structure Decision**: Internal-only restructuring within the existing single-crate `allez` binary/library plus its unmodified `condarc` library dependency — no new project, crate, or CLI surface. `default_packages_config.rs` is a new private (`mod`, not `pub mod`) module at the crate root, matching where `src/error.rs`/`src/output.rs` already live as crate-root sibling files, since its single responsibility (`create_default_packages` extraction) is distinct from `channel_config`'s own (channel semantics). `channel_config/document.rs` is a new private submodule nested inside the existing `channel_config` module rather than a second crate-root file: it shares `.condarc` I/O with `channel_config`'s own `locate.rs`/`events.rs`, so it lives alongside them instead of introducing a third top-level file for the same underlying resource.

## Complexity Tracking

*No entries — Constitution Check above is a full PASS with no violations to justify.*
