# Quickstart: Validating Default Package Resolution and Precedence

This is a validation/run guide, not an implementation walkthrough — see `contracts/default_package_resolution_contract.md` for the precedence rule itself and `data-model.md` for the exact types/functions involved. Task-by-task implementation breakdown belongs to a separate artifact (`/speckit.tasks`), out of this guide's scope.

## Prerequisites

- Rust toolchain matching `Cargo.toml`'s `edition = "2024"`.
- GEN-24's checked-in local `file://` fixture channel (`tests/fixtures/ephemeral_channel/`), reused with no fixture-package or generator change. `fixture-default-alpha` and `fixture-default-beta` — documented there as "meant for tests that need an explicit, fixture-resolvable package request" — are this ticket's own default-package stand-ins.
- `tests/oneshot_exec.rs` requires the `test-config-override` Cargo feature, the same feature `make test`/CI already default to; no new feature flag.
- Each end-to-end test sets `ALLEZ_CONDARC_PATH`/`ALLEZ_EPHEMERAL_ROOT` per-invocation via `assert_cmd`'s `Command::env(...)`, using the `OneshotHarness` harness in `tests/oneshot_exec.rs`. Scenarios add a `create_default_packages:` key to the fixture-pointing test `.condarc` contents `OneshotHarness::with_condarc_contents` supports.
- `create_default_packages`'s own parsing correctness against real conda is conformance-tested at the `condarc` crate level (`tests/condarc_conformance.rs`); nothing below re-tests that — every scenario here exercises only `allez`'s own consumption of an already-parsed value (research.md's "inherited, not re-tested" Decision).

## Running the tests

```console
cargo test --all --features test-config-override
```

Runs every test below. A bare `cargo test --all` (no explicit feature) compiles and runs every unit-level test (`src/ephemeral/defaults.rs`, `src/ephemeral/solve.rs`, `src/default_packages_config.rs`, `src/channel_config/document.rs`) and `tests/skills_doc.rs` (plain file read, no feature needed), skipping `tests/oneshot_exec.rs` (needs `test-config-override`) and `tests/condarc_conformance.rs` (needs its own separate `conformance-tests` feature, unrelated to this ticket).

## Acceptance-scenario → test mapping

| Spec ID | Scenario | Test |
|---|---|---|
| US1-AS1 / FR-001 / SC-001 | `.condarc` configures `create_default_packages`, no per-invocation packages ⇒ Effective Package Set is exactly that list. | `scenario_gen30_1_1_default_packages_from_condarc_with_no_overrides` (`tests/oneshot_exec.rs`) — `create_default_packages: [fixture-default-alpha]`, `allez oneshot -- echo hi` installs exactly `{fixture-default-alpha}`. |
| Zero-package solve | `Gateway::query`/`SolverTask` must actually succeed with an empty top-level spec list. | `solve_packages_empty_input_returns_no_records` (`src/ephemeral/solve.rs` test, network-free against the local fixture channel). |
| US1-AS2 / FR-002 / SC-001 | `create_default_packages` unset, no per-invocation packages ⇒ Effective Package Set is empty, no `allez`-substituted list. | `scenario_gen30_1_2_no_default_packages_configured_creates_empty_environment` — no `create_default_packages` key at all; environment creation succeeds with zero installed packages, proving FR-002's "no fallback." |
| FR-001 (unreadable file, real file I/O) | `~/.condarc` exists but is unreadable ⇒ falls back exactly like `channel_config`'s own channel-resolution fallback (same file, same read, same `CondarcDocument::FellBack`). | `resolve_document_from_unreadable_file_falls_back_and_emits_event` (`src/channel_config/document.rs` unit test, real temp file containing invalid-UTF-8 bytes — `channel_config`'s own existing technique, portable across platforms and not bypassable by a privileged test-runner account the way removing a file's read permission would be). |
| FR-001 (malformed YAML, real file I/O) | `~/.condarc` exists but `condarc::parse()` rejects it ⇒ falls back the same way. | `resolve_document_from_malformed_yaml_falls_back_and_emits_event` (`src/channel_config/document.rs` unit test, real temp file with invalid YAML). |
| FR-001 (missing file, real file I/O) | No `~/.condarc` at all ⇒ falls back the same way (silent, no event). | `resolve_document_from_missing_path_resolves_to_absent` (`src/channel_config/document.rs` unit test). |
| FR-001 (unreadable file, extraction layer) | `default_packages_config` resolves an already-unreadable-classified document to empty, independent of how `channel_config::document` classified it. | `create_default_packages_from_document_unreadable_falls_back_to_empty` (`src/default_packages_config.rs` unit test, constructing `CondarcDocument::FellBack(FallbackReason::Unreadable)` directly — no real unreadable file needed at this layer, since I/O happens in `channel_config::document`). |
| FR-001 (malformed YAML, extraction layer) | Same, for the parse-rejected case. | `create_default_packages_from_document_rejected_falls_back_to_empty` (unit test, `CondarcDocument::FellBack(FallbackReason::Rejected)`). |
| FR-001 (missing file, extraction layer) | Same, for the absent-file case. | `create_default_packages_from_document_absent_falls_back_to_empty` (unit test, `CondarcDocument::Absent`). |
| FR-002 (explicit empty) | `create_default_packages: []` ⇒ Effective Package Set is empty, same as unset. | `create_default_packages_from_document_parsed_empty_list_resolves_to_empty` (unit test, `CondarcDocument::Parsed` with `create_default_packages: Some(vec![])`). |
| FR-002 (key omitted from a parsed document) | `create_default_packages` absent from an otherwise-successfully-parsed `.condarc` ⇒ empty, same as unset or an absent file. | `create_default_packages_from_document_parsed_none_resolves_to_empty` (unit test, `CondarcDocument::Parsed` with `create_default_packages: None`). |
| FR-002 (empty inputs, merge layer) | Empty `explicit` and empty `defaults` ⇒ empty Effective Package Set, at the merge-algorithm layer directly (US1-AS2 above proves the same outcome end-to-end). | `effective_packages_empty_explicit_and_empty_defaults_returns_empty` (`src/ephemeral/defaults.rs` unit test). |
| US2-AS1 / FR-003 / SC-002 | Per-invocation package sharing no bare name with the default set ⇒ additive. | `scenario_gen30_2_1_per_invocation_package_adds_to_default_set` (`tests/oneshot_exec.rs`) — `create_default_packages: [fixture-default-alpha]`, `allez oneshot fixture-probe -- fixture-probe` installs `{fixture-default-alpha, fixture-probe}`. |
| US2-AS2 / FR-004 / SC-003 | Per-invocation package's bare name matches a default entry ⇒ supersedes, never both. | `scenario_gen30_2_2_per_invocation_package_supersedes_matching_default_entry` — `create_default_packages: [fixture-default-alpha=9.9.9]` (a version absent from the fixture channel — unresolvable on its own), `allez oneshot fixture-default-alpha -- echo hi` (unconstrained per-invocation spec) **succeeds**, installing `fixture-default-alpha` `1.0.0`. This is the real proof: if the default entry survived alongside the per-invocation one, the solve would fail outright (no version satisfies both `9.9.9` and an unconstrained request) — success is only possible if supersede actually dropped the default entry. |
| US2-AS3 / FR-005 | `skills/allez-oneshot.md` documents both the `.condarc` source and the precedence rule. | `skills_doc_states_default_package_source_and_precedence` (`tests/skills_doc.rs`) — reads `skills/allez-oneshot.md` and asserts its `### Default packages` subsection contains substrings naming `create_default_packages`/`~/.condarc` (source) and the add/supersede precedence; `contracts/default_package_resolution_contract.md`'s own examples table is the source of truth the doc prose must not contradict. |
| SC-004 case 1 | No per-invocation packages ⇒ Effective Package Set equals the default set exactly. | `effective_packages_no_explicit_packages_returns_defaults_exactly` (`src/ephemeral/defaults.rs` unit test). |
| SC-004 case 2 | Per-invocation package, no shared bare name ⇒ additive. | `effective_packages_disjoint_bare_names_is_additive` (unit test). |
| SC-004 case 3 | Per-invocation package's bare name matches a default entry ⇒ supersede. | `effective_packages_matching_bare_name_supersedes_default_entry` (unit test). |
| FR-004 (channel-qualified) | A channel-qualified per-invocation spec still supersedes a bare default entry sharing the same name. | `effective_packages_channel_qualified_explicit_supersedes_bare_default` (unit test — `conda-forge::numpy` vs. `numpy`). |
| FR-004 (case-folded) | A differently-cased per-invocation spec still supersedes a default entry sharing the same name. | `effective_packages_case_folded_explicit_supersedes_default` (unit test — `Pandas` vs. `pandas`). |
| FR-004 (constraint on the per-invocation side) | Supersede applies regardless of which side carries the version/build constraint — proven here with the constraint on the per-invocation entry rather than the default entry (SC-004 case 3/US2-AS2 both exercise the constraint on the *default* side only). | `effective_packages_constrained_explicit_supersedes_bare_default` (unit test — `numpy=2.0=py311h_0` per-invocation vs. bare `numpy` default). |
| Intra-list duplicates (default-set entries: research.md's own design decision, FR-002-backed; per-invocation entries: spec.md Assumptions, scoped to `allez oneshot`'s existing solver path per GEN-25) | Two default-set entries sharing a bare name, or two per-invocation entries sharing a bare name, are never deduplicated against each other. | `effective_packages_duplicate_bare_names_within_defaults_are_preserved`, `effective_packages_duplicate_bare_names_within_explicit_are_preserved` (unit tests). |
| Arbitrary resolved-entry strings (e.g. an empty string) | `allez` never rejects any resolved `create_default_packages` entry at extraction, regardless of its own shape: every one becomes a `PackageSpec` via `from_resolved_default`, with no `MatchSpec` parse at that layer at all, and is left to fail (or not) at solve time like any other package name. | `create_default_packages_from_document_empty_string_entry_resolves_successfully`, `create_default_packages_from_document_arbitrary_string_entry_resolves_successfully` (unit tests); `bare_name_of_an_unparseable_resolved_default_spec_returns_none` (`src/ephemeral/defaults.rs` unit test — the `None` path the empty/whitespace-only shape exercises). |
| Whitespace-only default entry | A resolved default entry that is only whitespace is preserved unchanged through extraction, exactly like the empty-string case, and reaches the solver as an ordinary entry. | `create_default_packages_from_document_whitespace_only_entry_resolves_successfully` (`src/default_packages_config.rs` unit test). |
| Project/repo-local config (spec.md Edge Cases) | A project- or repository-local configuration file, distinct from the invoking user's own `~/.condarc`, is never consulted. | `scenario_gen30_ignores_project_local_condarc` (`tests/oneshot_exec.rs`) — the subprocess's own working directory holds a decoy `.condarc` naming a fixture package the real `ALLEZ_CONDARC_PATH`-pointed test `.condarc` never names; the resolved default set reflects only the latter. Subprocess-scoped `Command::env`/`.current_dir(...)` (`OneshotHarness`'s existing command-scoped environment pattern, extended with `.current_dir(...)`), not process-wide mutation. |

## Merge coverage layers

Three layers together prove the merge (`research.md`'s own "Merge coverage layers" section has the full rationale for why each is needed): the direct `effective_packages` unit tests above; `create_ephemeral_environment_supersedes_matching_default_entry_by_bare_name` (`tests/support/defaults.rs`); and the US2-AS2 end-to-end scenario.

Under FR-002, empty per-invocation packages plus an empty resolved default set succeeds with zero installed packages — US1-AS2 above covers this end-to-end; no test at any layer asserts a failure for that input.

## Manual smoke check

Requires an `allez` binary built with the `test-config-override` feature (the `ALLEZ_CONDARC_PATH` seam is gated on it — data-model.md's `src/channel_config/document.rs` section).

```console
mkdir -p /tmp/allez-gen30-smoke
cat > /tmp/allez-gen30-smoke/condarc <<'EOF'
channels: [conda-forge]
create_default_packages: [python]
EOF
ALLEZ_CONDARC_PATH=/tmp/allez-gen30-smoke/condarc \
  allez oneshot -- python --version
```

Expected: `allez` resolves `python` from `create_default_packages` (no package named on the command line) and runs `python --version` inside the resulting environment. Requires real network access to `conda-forge` — not part of the automated default test suite (see `research.md` § Test strategy's fixture-only scope), included here only as an end-to-end sanity check a developer or reviewer can run by hand.
