---

description: "Task list template for feature implementation"
---

# Tasks: Ephemeral Environment Default Packages and User Overrides

**Input**: Design documents from `/specs/GEN-30_default_package_overrides/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/default_package_resolution_contract.md, quickstart.md (all present)

**Tests**: Test tasks are included below — this ticket's own plan.md/research.md explicitly name every test the implementation must satisfy (Constitution II, TDD), and Constitution VIII requires 100% spec-requirement test coverage.

**Organization**: Tasks are grouped by user story (spec.md's User Story 1/2) to enable independent implementation and testing of each story, preceded by one Foundational phase that both stories depend on (the merge algorithm and `.condarc`-resolution extraction this whole ticket is built around).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2)
- Every task below includes its exact file path(s)

## Path Conventions

Single-project Rust workspace at the repository root (`allez`'s existing single binary/library crate, `crates/condarc/` unchanged). All paths below are relative to the repository root.

---

## Phase 1: Setup

**Purpose**: Confirm a green baseline before this ticket's refactor begins — no code changes in this phase.

- [X] T001 Run `cargo build --all-features` and `cargo test --all --features test-config-override` at the repository root to confirm the pre-GEN-30 baseline is green before any file in this ticket's scope is touched.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Extract the shared `.condarc` resolution step, introduce the new `default_packages_config` module, and rewrite the additive/supersede merge and its one call site (`create_ephemeral_environment`) — the machinery every downstream test in Phase 3/4 depends on.

**⚠️ CRITICAL**: No User Story 1 or User Story 2 test can compile or pass until this phase is complete.

**Test-first note (Constitution II)**: T002 lists its own permanent unit tests alongside the function they cover; write those named tests first against the not-yet-existing signature (red — they fail to compile), then add the minimal implementation beneath them so they compile and pass (green). T004/T009/T011 instead write one throwaway/exploratory assertion first to drive their own implementation (red-green), then leave the actual named, permanent test battery to their own dedicated Phase 3/4 task (T023/T024–T025/T021–T023+T032–T037 respectively) — this is TDD at each task's own granularity, not a duplicate test obligation between a Phase 2 task and its Phase 3/4 counterpart. Every task above colocates its own tests with its own source in `#[cfg(test)] mod tests`, per this workspace's existing convention. Phase 3/4's "Tests for User Story N" sections are integration- and end-to-end-level tests (`tests/oneshot_exec.rs`, `tests/support/defaults.rs`) that exercise this phase's already-unit-tested machinery from the outside through a compiled binary or a public async function — the testing pyramid's outer layers necessarily run after the code they exercise exists, which is not a TDD violation at that layer.

- [X] T002 [P] Test-first: write this task's new unit tests below against the not-yet-existing `CondarcDocument`/`resolve_document[_from]` signatures (red), then create a new private submodule `src/channel_config/document.rs` — nested inside the existing `channel_config` module, not a new crate-root file: declare `mod document;` in `src/channel_config/mod.rs` (this is the only task that adds this declaration — T005 builds on it, it does not re-declare it), plus a `pub(crate) use document::{CondarcDocument, resolve_document, resolve_document_from};` re-export immediately alongside it for crate-wide reachability by `default_packages_config` (T004) and `oneshot.rs` (T013); add the new three-variant `pub(crate) enum CondarcDocument { Absent, FellBack(FallbackReason), Parsed(condarc::Config) }` (referencing the existing `channel_config::events::FallbackReason`, unchanged) and `pub(crate) fn resolve_document() -> CondarcDocument` / `pub(crate) fn resolve_document_from(path: Option<&Path>) -> CondarcDocument` per research.md's "sum type, not a tuple" Decision and data-model.md's exact signatures, built directly on top of `channel_config`'s existing `locate::read_condarc` (`pub(super)`, in `locate.rs`), `super::default_condarc_path`/`super::condarc_path_override` (private free functions defined in `channel_config/mod.rs` itself, not `locate.rs` — already visible to this new child module without any visibility change, since Rust module privacy grants a module's own descendants access to its private items), and `events::emit_fallback` (`pub(super)`, in `events.rs`) — Rust module privacy is tree-scoped, so none of this needs widening, and neither `locate.rs` nor `events.rs` is moved, deleted, or otherwise touched: `resolve_document_from(None)` resolves directly to `Absent` without calling `default_condarc_path()`; only the no-argument `resolve_document()` explicitly passes `default_condarc_path().as_deref()` into it. Add new unit tests in `document.rs` against real temp-file fixtures (matching `channel_config`'s existing technique): `resolve_document_from_missing_path_resolves_to_absent` (no file at the given path ⇒ `Absent`, silent, no event); `resolve_document_from_unreadable_file_falls_back_and_emits_event` (invalid-UTF-8 bytes ⇒ `FellBack(FallbackReason::Unreadable)`, one `emit_fallback` call); `resolve_document_from_malformed_yaml_falls_back_and_emits_event` (a file `condarc::parse()` rejects ⇒ `FellBack(FallbackReason::Rejected)`, one `emit_fallback` call); `resolve_document_from_valid_file_returns_parsed` (a well-formed `.condarc` ⇒ `Parsed(config)` with the expected fields); `consecutive_calls_read_changed_contents_without_caching` (two calls against a path whose contents changed between them return two different results — no caching).

- [X] T003 Declare `mod default_packages_config;` (private, no `pub`) in `src/lib.rs`, alongside the existing `pub mod channel_config;`/`pub mod cli;`/`pub mod ephemeral;`/`pub mod error;`/`pub mod observability;`/`pub mod output;`. `channel_config::document` (T002) needs no top-level declaration of its own — it is declared inside `src/channel_config/mod.rs` directly. Depends on T002 (for `default_packages_config` to reference `CondarcDocument`).

- [X] T004 Test-first: before implementing, write one throwaway/exploratory assertion against this function's not-yet-existing signature (e.g. `create_default_packages_from_document(&CondarcDocument::Absent)` returns an empty `Vec`), confirm it fails to compile (red), then create new private module `src/default_packages_config.rs` with `pub(crate) fn create_default_packages_from_document(document: &CondarcDocument) -> Vec<crate::ephemeral::PackageSpec>` (infallible, no `Result`): for `CondarcDocument::Parsed(config)`, `config.create_default_packages.clone().unwrap_or_default()`; for `Absent`/`FellBack(_)`, an empty `Vec`; each resolved string becomes a `PackageSpec` via `PackageSpec::from_resolved_default` (added in T009) — never the validating `PackageSpec::parse`. This task's own exploratory check is not one of the module's permanent tests — T024/T025 (Phase 3) add the actual named, permanent module-level test battery afterward (every `CondarcDocument` variant plus empty-string/whitespace/arbitrary-string entries), not a duplicate of this task's scratch check. No public "resolve everything from scratch" convenience wrapper (this module has exactly one real caller, `src/cli/oneshot.rs`, T013). Depends on T002 (`CondarcDocument`), T003 (module declaration), T009 (`from_resolved_default`).

- [X] T005 Rewrite `src/channel_config/mod.rs`: keep the existing `mod locate;`/`mod events;` declarations and every item inside them exactly as they are (T002 does not move or delete either file); keep T002's `mod document;` declaration and its re-export as-is (this task does not re-declare either); add `pub(crate) fn channels_from_document(document: &CondarcDocument) -> ChannelConfigResolution` (channel-specific expansion only: `Parsed(config)` → `condarc::expand_channels(config)`, emitting its own `FallbackReason::Rejected` via `events::emit_fallback` if expansion fails; `Absent`/`FellBack(_)` → `condarc::expand_channels(&condarc::Config::default())`); keep `pub fn resolve_channel_config()` and `pub(crate) fn resolve_channel_config_from(path: Option<&Path>)` at their existing signatures/visibility: `resolve_channel_config_from(path)` becomes a two-line composition of `document::resolve_document_from(path)` then `channels_from_document(&document)`; `resolve_channel_config()` (which takes no `path` argument) becomes a two-line composition of `document::resolve_document()` (the no-argument variant) then `channels_from_document(&document)` — the two functions call the correspondingly-arity `resolve_document`/`resolve_document_from`, not the same one. `ChannelConfigResolution` and `into_resolution` are unchanged. Depends on T002.

- [X] T006 Run `cargo test --lib channel_config` to confirm `locate.rs`'s and `events.rs`'s own existing unit tests still pass completely unmodified (this single filter matches both submodules, since their tests live at `channel_config::locate::...`/`channel_config::events::...`) — T002/T005 add a new sibling `document.rs` alongside them but never touch either file's own content, so no regression is possible here by construction; this task makes that guarantee explicit rather than assumed. Depends on T002, T005.

- [X] T007 Run `cargo test --lib channel_config` to confirm every one of `channel_config/mod.rs`'s existing tests still passes unchanged against the new `document`-backed implementation: `parse_rejection_emits_one_rejected_fallback_event`, `invalid_utf8_emits_one_unreadable_fallback_event`, `expansion_failure_emits_one_rejected_fallback_event_with_error_detail`, `missing_file_emits_no_fallback_event`, `nonexistent_path_returns_ready_defaults_without_fallback_reason`, `parse_rejection_returns_ready_defaults_with_rejected_reason`, `invalid_utf8_returns_ready_defaults_with_unreadable_reason`, `expansion_failure_returns_ready_defaults_with_rejected_reason`, `populated_file_returns_its_resolved_channels_without_fallback_reason`, `absent_path_argument_returns_ready_defaults_without_fallback_reason`, `real_world_samples_pass_through_expanded_config_unchanged`, `empty_defaults_multichannel_returns_no_channels`, `filtering_nonempty_channels_to_empty_returns_no_channels`, `resolution_never_changes_existing_file_contents_or_mtime`, `unrecognized_top_level_key_resolves_exactly_as_if_absent`, `consecutive_calls_read_changed_contents_without_caching`. Depends on T005, T006.

- [X] T008 [P] In `src/ephemeral/defaults.rs`, delete the `DEFAULT_PACKAGES` constant and the `RequestedPackages` enum outright, along with their own unit tests: `no_override_falls_back_to_default_packages`, `override_resolving_to_empty_falls_back_to_default_packages`, `explicit_non_empty_wins_over_any_override`, `explicit_empty_falls_back_like_use_default_or_override`, `non_empty_override_wins_over_default_packages`, `from_cli_empty_list_becomes_use_default_or_override`, `from_cli_non_empty_list_becomes_explicit`. Keep `PackageSpec`, `InvalidPackageSpec`, `PackageSpec::parse`, `PackageSpec::as_str` and their existing tests (`parse_rejects_a_syntactically_invalid_match_spec`, `invalid_package_spec_display_redacts_a_credential_bearing_input`, `invalid_package_spec_debug_redacts_credential_bearing_fields`, `package_spec_debug_redacts_a_credential_bearing_spec`) unchanged.

- [X] T009 Test-first: before implementing, write one throwaway/exploratory assertion against `bare_name`'s not-yet-existing signature (e.g. `PackageSpec::from_resolved_default(String::new()).bare_name()` returns `None`), confirm it fails to compile (red), then in `src/ephemeral/defaults.rs`, add `pub(crate) fn from_resolved_default(input: String) -> Self` on `impl PackageSpec` (stores `input` opaquely with no `MatchSpec` parse at all; cannot fail; used only by `default_packages_config::create_default_packages_from_document`), and `pub(crate) fn bare_name(&self) -> Option<String>` on `impl PackageSpec` (re-parses `self.as_str()` via `rattler_conda_types::MatchSpec::from_str(self.as_str(), ParseStrictness::Strict)`, returning `parsed.name.as_exact().map(|n| n.as_normalized().to_string())` on success, `None` otherwise — including when the reparse fails outright). This task's own exploratory check is not the module's permanent test — T023 (Phase 3) adds the actual named, permanent test (`bare_name_of_an_unparseable_resolved_default_spec_returns_none`), not a duplicate of this task's scratch check. Depends on T008.

- [X] T010 Test-first: write a parse_explicit_packages test asserting an empty input list parses to an empty `Vec` before implementing it (this behavior has no dedicated unit-test task ID above US1/US2's end-to-end coverage, which exercises it indirectly — add the direct unit test alongside this task's own implementation), then in `src/ephemeral/defaults.rs`, add `pub(crate) fn parse_explicit_packages(packages: Vec<String>) -> Result<Vec<PackageSpec>, InvalidPackageSpec>` (parses every string via the existing validating `PackageSpec::parse`; an empty input list is not a special case — it simply parses to an empty `Vec`). Depends on T008.

- [X] T011 Test-first: before implementing, write one throwaway/exploratory assertion against `effective_packages`'s new `(explicit, defaults)` signature (e.g. empty `explicit` returns `defaults` exactly), confirm it fails to compile (red), then in `src/ephemeral/defaults.rs`, replace `effective_packages`'s old `(requested: &RequestedPackages, default_override: Option<&[PackageSpec]>)` signature and body with `pub(crate) fn effective_packages(explicit: &[PackageSpec], defaults: &[PackageSpec]) -> Vec<PackageSpec>` per research.md's "Additive, supersede-by-bare-name merge" Decision: collect the `Some` bare names present in `explicit`; walk `defaults` in its own original order, keeping every entry whose `bare_name()` is `None` (always survives) or whose `Some` bare name is not in that set; append every `explicit` entry, in its own original order, to the end. No enum branching anywhere in this function. This task's own exploratory check is not one of the function's permanent tests — T021–T023 (Phase 3) and T032–T037 (Phase 4) add the actual named, permanent SC-004/edge-case test battery afterward, not a duplicate of this task's scratch check. Depends on T009, T010.

- [X] T012 In `src/ephemeral/mod.rs`, change `create_ephemeral_environment`'s signature to `pub async fn create_ephemeral_environment(explicit: Vec<PackageSpec>, defaults: Vec<PackageSpec>, channels: condarc::ResolvedChannels) -> Result<ReadyEnvironment, CreationFailure>`; as its own first step, before any filesystem or network work begins, call `let packages = defaults::effective_packages(&explicit, &defaults);` and use `packages` everywhere the old `requested`/`default_override`-derived package list was consumed. Update the module's re-export line to `pub use defaults::{InvalidPackageSpec, PackageSpec};` plus a new `pub(crate) use defaults::parse_explicit_packages;`. Depends on T011.

- [X] T013 Rewrite `src/cli/oneshot.rs`'s `run()` orchestration per data-model.md's exact 5-step sequence: (1) `let explicit = match ephemeral::parse_explicit_packages(args.packages.clone()) { Ok(v) => v, Err(invalid) => return environment_creation_failed(..., human) };`; (2) `let document = channel_config::resolve_document();` — the one `.condarc` read for this invocation; (3) `let channels = match channel_config::channels_from_document(&document) { ChannelConfigResolution::Ready { config, .. } => config, ChannelConfigResolution::NoChannels => return environment_creation_failed(..., human) };`; (4) `let defaults = default_packages_config::create_default_packages_from_document(&document);` — infallible, no failure path; (5) `create_ephemeral_environment(explicit, defaults, channels).await` — `create_ephemeral_environment` performs the FR-003/FR-004 merge itself, as its own first step; `oneshot.rs` never calls `effective_packages` directly. Every branch returns through an explicit `match`/`environment_creation_failed(...)` pattern (`run()` returns `OneshotOutcome`, not `Result`); `OneshotOutcome`'s `exit_code()` mapping and JSON/human rendering are unchanged, governed entirely by `contracts/oneshot_cli_contract.md` (GEN-25) — this task changes *which* packages get resolved, not how a resolution failure is reported. Depends on T004 (`create_default_packages_from_document`), T005 (`channels_from_document`), T012 (`create_ephemeral_environment`'s new signature).

- [X] T014 [P] Update `examples/ephemeral_smoke.rs`'s single `create_ephemeral_environment` call site to the new three-argument signature: `create_ephemeral_environment(vec![allez::ephemeral::PackageSpec::parse("fixture-probe")?], Vec::new(), channels).await?`; drop the now-unused `RequestedPackages` import (`use allez::ephemeral::{create_ephemeral_environment};` plus the fully-qualified `PackageSpec::parse` call already present). Depends on T012.

- [X] T015 [P] Update `tests/support/ephemeral.rs`: change `pub(crate) fn package_specs(packages: &[&str]) -> RequestedPackages` to `pub(crate) fn package_specs(packages: &[&str]) -> Vec<PackageSpec>` (parsing each via `PackageSpec::parse(package).unwrap()` directly, no `RequestedPackages::Explicit` wrapper); delete `explicit_package_specs` entirely (now identical to the rewritten `package_specs` — every one of its own callers switches to `package_specs` in T018). Depends on T012.

- [X] T016 Update every `create_ephemeral_environment` call site in `tests/support/creation.rs` (`resolvable_packages_are_installed_and_the_probe_is_usable`, `flexible_channel_priority_solves_successfully`, `strict_channel_priority_selects_the_first_channels_version`, `lifecycle_events_include_consistent_ids_packages_and_durations`, `a_solve_stage_failure_still_emits_an_install_failure_event`, `a_ready_environment_is_not_torn_down_on_its_own`, `resolved_defaults_channel_installs_a_real_package`) to the new `(explicit, defaults, channels)` shape, passing `Vec::new()` for `defaults` in each (none of these exercise the resolved-default-set path); remove the `empty_package_list_resolves_default_packages_the_fixture_channel_cannot_satisfy` test outright (its `DEFAULT_PACKAGES`-fallback premise no longer exists); drop the `DEFAULT_PACKAGES`/`RequestedPackages` import, keeping `EphemeralEnvError`/`create_ephemeral_environment`. Re-verify `lifecycle_events_include_consistent_ids_packages_and_durations`'s `packages` field assertions (`["fixture-probe"]`/`["fixture-corrupt-checksum"]`) still hold now that the merge runs inside `create_ephemeral_environment` itself. Depends on T012, T015.

- [X] T017 Update every `create_ephemeral_environment` call site in `tests/support/failures.rs` (`unresolvable_package_returns_failure_without_a_partial_directory`, `deny_filtered_channel_list_returns_no_channels`, `allow_filtered_channel_list_returns_no_channels`, `corrupt_checksum_returns_integrity_failure_without_a_partial_directory`, `empty_resolved_channels_return_no_channels_without_network`, `root_resolution_failure_does_not_attempt_cleanup`) to the new `(explicit, defaults, channels)` shape, passing `Vec::new()` for `defaults` in each. Depends on T012, T015.

- [X] T018 In `tests/support/defaults.rs`, remove the three obsolete replace-only-precedence tests outright — `no_packages_with_an_override_installs_the_override_instead_of_defaults`, `explicit_packages_alongside_an_override_ignore_the_override_entirely`, `an_override_resolving_to_empty_falls_back_to_default_packages_the_fixture_channel_cannot_satisfy` — and drop the `DEFAULT_PACKAGES`/`RequestedPackages`/`explicit_package_specs` imports (`use allez::ephemeral::{DEFAULT_PACKAGES, EphemeralEnvError, RequestedPackages, create_ephemeral_environment};` and the `explicit_package_specs` import from `crate::support`). The file is intentionally left holding only its remaining imports pending T038's new test. Depends on T012, T015.

- [X] T019 Run `cargo build --all-features` and `cargo test --all --features test-config-override --no-run` at the repository root to confirm every call site across `src/`, `examples/`, and `tests/` compiles cleanly against the new `create_ephemeral_environment`/`effective_packages` signatures before any new test content is added in Phase 3/4. Depends on T002–T018 (all of Phase 2).

**Checkpoint**: Foundation ready — the merge algorithm, `.condarc` extraction, and every existing call site compile against the new signatures. User Story 1 and User Story 2 test work can now begin (US2 also depends on T018 having emptied `tests/support/defaults.rs`, so its US2-phase tasks against that file run after Phase 2, not in parallel with it).

---

## Phase 3: User Story 1 - My own `.condarc` decides my ephemeral environments' defaults (Priority: P1) 🎯 MVP

**Goal**: An ephemeral environment created with no per-invocation packages gets exactly whatever the user's own `~/.condarc` `create_default_packages` setting resolves to — including empty, with no `allez`-authored fallback list.

**Independent Test**: Configure `create_default_packages` to a list in `.condarc` and confirm a package-free `allez oneshot` invocation gets exactly that list; leave it unconfigured (or empty) and confirm the resulting environment has no default packages.

### Tests for User Story 1

- [X] T020 [P] [US1] Add `solve_packages_empty_input_returns_no_records` test to `src/ephemeral/solve.rs`'s `#[cfg(test)] mod tests`: calls `solve_packages` directly against the checked-in local fixture channel (`tests/fixtures/ephemeral_channel/`) with an empty `packages` slice, asserting `Ok` with an empty `records` list — proves `Gateway::query`/`SolverTask` succeed on zero specs (FR-002, the zero-package solve case US1-AS2 needs at the cheapest layer capable of proving it).

- [X] T021 [P] [US1] Add `effective_packages_no_explicit_packages_returns_defaults_exactly` unit test to `src/ephemeral/defaults.rs`'s `#[cfg(test)] mod tests`: empty `explicit`, non-empty `defaults` ⇒ result equals `defaults` exactly, in order (SC-004 case 1).

- [X] T022 [P] [US1] Add `effective_packages_empty_explicit_and_empty_defaults_returns_empty` unit test to `src/ephemeral/defaults.rs`: empty `explicit` and empty `defaults` ⇒ empty result (FR-002, no built-in fallback of any kind).

- [X] T023 [P] [US1] Add `bare_name_of_an_unparseable_resolved_default_spec_returns_none` unit test to `src/ephemeral/defaults.rs`: constructs `PackageSpec::from_resolved_default(String::new())` directly and asserts `.bare_name()` returns `None`, not `Some("")` — locks in this as reachable, deliberate behavior, not dead code.

- [X] T024 [P] [US1] Add five module-level unit tests to `src/default_packages_config.rs`'s `#[cfg(test)] mod tests`, each constructing a `CondarcDocument` directly (no I/O): `create_default_packages_from_document_absent_falls_back_to_empty` (`CondarcDocument::Absent` ⇒ empty `Vec`), `create_default_packages_from_document_unreadable_falls_back_to_empty` (`FellBack(FallbackReason::Unreadable)` ⇒ empty), `create_default_packages_from_document_rejected_falls_back_to_empty` (`FellBack(FallbackReason::Rejected)` ⇒ empty), `create_default_packages_from_document_parsed_none_resolves_to_empty` (`Parsed` with `create_default_packages: None` ⇒ empty), `create_default_packages_from_document_parsed_empty_list_resolves_to_empty` (`Parsed` with `create_default_packages: Some(vec![])` ⇒ empty).

- [X] T025 [P] [US1] Add three more module-level unit tests to `src/default_packages_config.rs`, each asserting a `Parsed` document's populated `create_default_packages` list passes every entry through unchanged via `from_resolved_default` — none rejected at this extraction layer: `create_default_packages_from_document_empty_string_entry_resolves_successfully`, `create_default_packages_from_document_whitespace_only_entry_resolves_successfully`, `create_default_packages_from_document_arbitrary_string_entry_resolves_successfully`.

- [X] T026 [US1] In `tests/oneshot_exec.rs`, remove `scenario_1_2_zero_packages_routes_through_resolution_not_usage_error`: this test asserts an `unresolvable_package` failure for zero per-invocation packages plus no `create_default_packages` key, which contradicts FR-002 — that invocation must succeed with zero installed packages.

- [X] T027 [US1] In `tests/oneshot_exec.rs`, add `scenario_gen30_1_1_default_packages_from_condarc_with_no_overrides`: use `OneshotHarness::with_condarc_contents` with a `channels:` entry plus `create_default_packages: [fixture-default-alpha]`; run `allez oneshot -- echo hi` with zero per-invocation packages; assert the installed set is exactly `{fixture-default-alpha}` (US1-AS1/FR-001). Depends on T019.

- [X] T028 [US1] In `tests/oneshot_exec.rs`, add `scenario_gen30_1_2_no_default_packages_configured_creates_empty_environment`: a test `.condarc` with a `channels:` entry but no `create_default_packages` key at all; run `allez oneshot -- echo hi` with zero per-invocation packages; assert environment creation succeeds with an empty installed set, proving FR-002's "no fallback" directly (US1-AS2/FR-002). Depends on T019.

- [X] T029 [US1] In `tests/oneshot_exec.rs`, add `scenario_gen30_ignores_project_local_condarc`: extend `OneshotHarness`'s command builder with `.current_dir(...)` pointed at a dedicated temporary directory holding its own decoy `.condarc` (a `create_default_packages:` entry naming a fixture package — e.g. `fixture-default-beta` — the real test `.condarc` never names), while `ALLEZ_CONDARC_PATH` still points at the harness's own real test `.condarc` (naming `fixture-default-alpha`); assert only `fixture-default-alpha` is installed, never the decoy (spec.md's project-local-config Edge Case). Depends on T019.

### Implementation for User Story 1

- [X] T030 [US1] Update `tests/fixtures/ephemeral_channel/README.md`'s "Default-package candidates" section to state: no built-in `allez`-authored default package list exists at all (removing the `["python"]`-stopgap language, per FR-002); defaults come exclusively from the user's own `create_default_packages` setting; and the zero-packages case succeeds with an empty installed set.

**Checkpoint**: At this point, User Story 1 is fully functional and independently testable — `allez oneshot` with no per-invocation packages honors `.condarc`'s `create_default_packages` exactly, including the empty case, with no project-local-config leakage.

- [X] T031 [US1] Run `cargo test --lib` (covering T020–T025's new unit/module tests) and `cargo test --test oneshot_exec --features test-config-override` (covering T026–T029) to confirm every User Story 1 test above passes. Depends on T020–T030.

---

## Phase 4: User Story 2 - Add a package, or override one by version, per invocation (Priority: P2)

**Goal**: A per-invocation package (named before `--`) is added to the resolved default package set; a per-invocation package whose bare name matches a default entry supersedes that entry rather than both being retained.

**Independent Test**: Name a per-invocation package sharing no bare name with the resolved defaults and confirm the result is additive; name one whose bare name matches a default entry and confirm the per-invocation spec wins, never both.

### Tests for User Story 2

- [X] T032 [P] [US2] Add `effective_packages_disjoint_bare_names_is_additive` unit test to `src/ephemeral/defaults.rs`: a per-invocation package sharing no bare name with `defaults` ⇒ result is `defaults` followed by every `explicit` entry (SC-004 case 2).

- [X] T033 [P] [US2] Add `effective_packages_matching_bare_name_supersedes_default_entry` unit test to `src/ephemeral/defaults.rs`: a per-invocation package's bare name matches a `defaults` entry, with the version/build constraint on the *default* side (e.g. `numpy=1.2` default vs. bare `numpy` per-invocation) ⇒ that one `defaults` entry is dropped, the per-invocation spec is retained, no other `defaults` entry affected (SC-004 case 3).

- [X] T034 [P] [US2] Add `effective_packages_constrained_explicit_supersedes_bare_default` unit test to `src/ephemeral/defaults.rs`: the same collision with the constraint on the *per-invocation* side instead (bare `numpy` default vs. `numpy=2.0=py311h_0` per-invocation) ⇒ the default entry is still dropped — proves FR-004's "regardless of which side carries the constraint" clause symmetrically.

- [X] T035 [P] [US2] Add `effective_packages_channel_qualified_explicit_supersedes_bare_default` unit test to `src/ephemeral/defaults.rs`: a channel-qualified per-invocation spec (`conda-forge::numpy`) supersedes a bare `numpy` default entry — asserts `bare_name()` compares by name only, ignoring the channel qualifier on either side.

- [X] T036 [P] [US2] Add `effective_packages_case_folded_explicit_supersedes_default` unit test to `src/ephemeral/defaults.rs`: a differently-cased per-invocation spec (`Pandas`) supersedes a default entry named `pandas` — asserts `bare_name()`'s case-folding comparison, not raw byte-for-byte string comparison.

- [X] T037 [P] [US2] Add `effective_packages_duplicate_bare_names_within_defaults_are_preserved` and `effective_packages_duplicate_bare_names_within_explicit_are_preserved` unit tests to `src/ephemeral/defaults.rs`: two `defaults`-list entries (or two `explicit`-list entries) sharing a bare name with each other are never deduplicated against each other — both survive unchanged in the result.

- [X] T038 [US2] In `tests/support/defaults.rs`, add `create_ephemeral_environment_supersedes_matching_default_entry_by_bare_name`: calls `create_ephemeral_environment` directly with a colliding `(explicit, defaults)` pair against the fixture channel; asserts the installed set reflects supersede-not-duplicate — proves the merge is actually wired into `create_ephemeral_environment`'s own body, not merely available to call separately. Depends on T018 (file emptied of the three obsolete tests).

- [X] T039 [US2] In `tests/oneshot_exec.rs`, add `scenario_gen30_2_1_per_invocation_package_adds_to_default_set`: `create_default_packages: [fixture-default-alpha]`, per-invocation `fixture-probe` (no shared bare name) via `allez oneshot fixture-probe -- fixture-probe`; assert installed set is `{fixture-default-alpha, fixture-probe}` (US2-AS1/FR-003). Depends on T019.

- [X] T040 [US2] In `tests/oneshot_exec.rs`, add `scenario_gen30_2_2_per_invocation_package_supersedes_matching_default_entry`: `create_default_packages: [fixture-default-alpha=9.9.9]` (a version absent from the fixture channel — unresolvable on its own), per-invocation `fixture-default-alpha` (unconstrained) via `allez oneshot fixture-default-alpha -- echo hi`; assert the invocation **succeeds**, installing `fixture-default-alpha` `1.0.0` — the real proof: if the default entry survived alongside the per-invocation one, the solve would fail outright (US2-AS2/FR-004). Depends on T019.

### Implementation for User Story 2

- [X] T041 [US2] Add a new `### Default packages` subsection to `skills/allez-oneshot.md`'s `## Usage` section, positioned immediately before its `### Examples` sibling subsection, stating plainly: (1) with no packages named before `--`, the environment gets exactly whatever the caller's own `~/.condarc` `create_default_packages` setting resolves to; (2) named packages are *added* to that resolved set; (3) a named package whose bare name matches a default-set entry replaces that one entry (version/build included) rather than both ending up installed (FR-005/US2-AS3). No wording change needed to the existing `allez oneshot numpy pandas -- python script.py` example under `### Examples`.

- [X] T042 [US2] Create new standalone test binary `tests/skills_doc.rs` with `skills_doc_states_default_package_source_and_precedence`: reads `skills/allez-oneshot.md` and asserts its `### Default packages` subsection's text contains a substring naming `create_default_packages` and `~/.condarc` together (source fact), and a substring stating both that named packages are added and that a matching bare name replaces the default entry (precedence fact) — a substring/content assertion, not a full-prose match. Depends on T041. No Cargo feature or `[[test]]` entry needed (plain file read, autodiscovered).

**Checkpoint**: At this point, User Stories 1 AND 2 both work independently — per-invocation packages add to and can supersede the `.condarc`-resolved default set exactly per FR-003/FR-004, and `skills/allez-oneshot.md` documents both facts.

- [X] T043 [US2] Run `cargo test --lib` (covering T032–T037), `cargo test --test ephemeral_env --features test-config-override` (covering T038), `cargo test --test oneshot_exec --features test-config-override` (covering T039–T040), and `cargo test --test skills_doc` (covering T042) to confirm every User Story 2 test above passes. Depends on T032–T042.

---

## Phase 5: Polish & Cross-Cutting Concerns

**Purpose**: Final quality gates across the whole ticket, per Constitution I/VI/VIII and the "changelog entry for user-visible changes" Quality Gate.

- [X] T044 [P] Run `cargo clippy --all-targets --all-features -- -D warnings` at the repository root and fix any new lint findings introduced by this ticket's changes (`clippy::unwrap_used`/`clippy::expect_used` deny is already enforced crate-wide via `src/lib.rs`).

- [X] T045 [P] Confirm every new `pub(crate)` item in `src/channel_config/document.rs`, `src/default_packages_config.rs`, and `src/ephemeral/defaults.rs` (`CondarcDocument`, `resolve_document`, `resolve_document_from`, `create_default_packages_from_document`, `from_resolved_default`, `bare_name`, `parse_explicit_packages`, `effective_packages`) carries a `///` doc comment, matching this codebase's existing convention of documenting `pub(crate)` items despite `#![warn(missing_docs)]` only strictly requiring it for `pub` items.

- [X] T046 Run `cargo test --all --features test-config-override` at the repository root; confirm zero failures and zero regressions versus the T001 baseline.

- [X] T047 Walk `specs/GEN-30_default_package_overrides/quickstart.md`'s full acceptance-scenario → test mapping table as a manual checklist, confirming every named test in the table exists in the codebase and passes.

- [X] T048 Add a `CHANGELOG.md` entry (creating the file at the repository root if it does not yet exist, per GEN-22 tasks.md T054's established precedent) documenting this ticket's user-visible changes: ephemeral-environment default packages now come from the user's own `~/.condarc` `create_default_packages` setting instead of a built-in `["python"]` list; per-invocation packages are added to (not replaced by) that resolved set, superseding a same-bare-name default entry; `skills/allez-oneshot.md`'s new `### Default packages` documentation; and the breaking `create_ephemeral_environment(explicit, defaults, channels)` public library API signature change (removal of `DEFAULT_PACKAGES`/`RequestedPackages`). Depends on T046 (reflects manually-verified, passing behavior).

- [X] T049 [P] Run `cargo fmt --check` and `cargo doc --no-deps --all-features` at the repository root; fix any formatting drift and any `rustdoc` warnings on the new/changed `pub`/`pub(crate)` items from this ticket.

- [X] T050 [P] Run `cargo audit` and `cargo deny check` at the repository root (both already part of this workspace's CI per `.specify/memory/constitution.md`'s Security & Supply-Chain gate) to confirm this ticket's zero-new-dependency change introduces no new advisory or license/ban violation.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — run first.
- **Foundational (Phase 2)**: Depends on Setup completion — BLOCKS both user stories. T002 → T003/T005; T005 → T006 → T007; T008 → T009/T010 → T011 → T012 → T013 (`oneshot.rs` orchestration, also depends on T004/T005) → T014/T015; T015 → T016/T017/T018; all of T002–T018 → T019 (compile gate).
- **User Story 1 (Phase 3)**: Depends on Phase 2 (T019) completion. T027/T028/T029 additionally depend on T019 directly (they run the compiled binary).
- **User Story 2 (Phase 4)**: Depends on Phase 2 (T019) completion. T038 additionally depends on T018 (the file it extends). T039/T040 depend on T019. T042 depends on T041.
- **Polish (Phase 5)**: Depends on both User Story phases being complete. T048 depends on T046. T049/T050 have no dependency on T046/T048 and can run any time after Phase 2 completes.

### User Story Dependencies

- **User Story 1 (P1)**: Can start after Foundational (Phase 2) completes. No dependency on User Story 2.
- **User Story 2 (P2)**: Can start after Foundational (Phase 2) completes. Shares `src/ephemeral/defaults.rs` and `tests/oneshot_exec.rs` with User Story 1 as edit targets (not a logical dependency) — running US1's file edits (T021–T023, T026–T029) to completion before US2's (T032–T037, T039–T040) in the same files avoids merge conflicts, but neither story's *passing* depends on the other's.

### Within Each User Story

- Phase 2's own tasks write their unit tests test-first (Phase 2 header's Test-first note); Phase 3/4 add the integration- and end-to-end-level tests the testing pyramid's outer layers require, which necessarily run against Phase 2's already-compiled code (see plan.md's Summary).
- Unit-level tests (`src/ephemeral/defaults.rs`, `src/default_packages_config.rs`, `src/ephemeral/solve.rs`) before end-to-end tests (`tests/oneshot_exec.rs`), matching research.md's "Merge coverage layers" (algorithm → wiring → full invocation).
- Story complete before moving to Polish.

### Parallel Opportunities

- T002 and T008 can run in parallel (different files: `src/channel_config/document.rs` vs `src/ephemeral/defaults.rs`, no shared dependency).
- T014 and T015 can run in parallel once T012 completes (different files: `examples/ephemeral_smoke.rs` vs `tests/support/ephemeral.rs`).
- All of T020–T025 (US1 unit/module tests) can run in parallel — different files or independent additions within `src/ephemeral/defaults.rs`'s own test module.
- All of T032–T037 (US2 unit tests in `src/ephemeral/defaults.rs`) can run in parallel with each other, and with T020–T025 if US1 and US2 are staffed concurrently (same file, but each adds an independent, non-overlapping test function).
- T044 and T045 (Polish) can run in parallel — independent checks, no shared files.
- T049 and T050 (Polish) can run in parallel with each other and with T044/T045 — independent checks, no shared files.

---

## Parallel Example: Foundational Phase

```bash
# Launch these two independent extractions together:
Task: "Create src/channel_config/document.rs (add CondarcDocument/resolve_document, locate.rs/events.rs untouched)"
Task: "Rewrite src/ephemeral/defaults.rs (delete DEFAULT_PACKAGES/RequestedPackages + their tests)"
```

## Parallel Example: User Story 1

```bash
# Launch all independent US1 unit/module tests together:
Task: "Add solve_packages_empty_input_returns_no_records to src/ephemeral/solve.rs"
Task: "Add effective_packages_no_explicit_packages_returns_defaults_exactly to src/ephemeral/defaults.rs"
Task: "Add effective_packages_empty_explicit_and_empty_defaults_returns_empty to src/ephemeral/defaults.rs"
Task: "Add bare_name_of_an_unparseable_resolved_default_spec_returns_none to src/ephemeral/defaults.rs"
Task: "Add five module-level tests to src/default_packages_config.rs (absent/unreadable/rejected/none/empty-list)"
Task: "Add three module-level tests to src/default_packages_config.rs (empty-string/whitespace/arbitrary entries)"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup.
2. Complete Phase 2: Foundational (CRITICAL — the merge algorithm and `.condarc` extraction both stories depend on).
3. Complete Phase 3: User Story 1.
4. **STOP and VALIDATE**: Run T031's test commands; confirm User Story 1 passes independently.
5. This alone delivers spec.md's stated priority: "This is the entire reason this ticket exists" (US1's own Why-this-priority).

### Incremental Delivery

1. Complete Setup + Foundational → Foundation ready (the production code path is fully correct at this point; only test coverage and documentation remain).
2. Add User Story 1 → Test independently → this is the MVP.
3. Add User Story 2 → Test independently → per-invocation add/supersede precedence is now proven and documented.
4. Complete Polish → clippy/doc/changelog gates satisfied.

### Parallel Team Strategy

With two developers, after Phase 2 (Foundational) completes:

- Developer A: User Story 1 (T020–T031)
- Developer B: User Story 2 (T032–T043)

Both touch `src/ephemeral/defaults.rs`'s test module and `tests/oneshot_exec.rs` — coordinate to avoid the same file being edited by both at once, or sequence US1 before US2 in those two files specifically.

---

## Notes

- [P] tasks = different files, or independent, non-overlapping additions within the same file's test module — verify no other in-flight task touches the same lines before running in parallel.
- [Story] label maps task to spec.md's User Story 1/User Story 2 for traceability.
- This ticket's "implementation" is front-loaded into Phase 2 (Foundational) — plan.md's own Summary states the merge/extraction is complete at that point; Phase 3/4 are almost entirely test-coverage and documentation tasks proving Phase 2's code is correct per FR-001–FR-005/SC-001–SC-004.
- Every new/changed test runs under the existing `test-config-override` Cargo feature (end-to-end tests) or as a plain unit/module test — no new Cargo feature, dependency, or fixture asset is introduced anywhere in this ticket.
- Commit after each task or logical group; stop at either Phase 3 or Phase 4's checkpoint to validate that story independently before continuing.
</content>
