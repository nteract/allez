# Feature Specification: `.condarc` Parser Library

**Feature Branch**: `GEN-36_condarc_parser_library`

**Created**: 2026-07-23

**Status**: Draft

**Input**: User description: "GEN-36. We have completed the research and conformance testing as to what a valid .condarc requires. That information lives in conformance tests, it lives in an openapi.json file which upholds the conformance tests, and it lives in the docs/condarc_research.md file. Now it is time to write a spec for a rust library which implements this behavior. The goal for this crate is that it should take in a yaml string (the caller can load the file), and generate the internal representation of a .condarc as typed rust objects... The acceptance criteria for the spec is that the library passes all the conformance tests, including matching the expected.json internal state... have a strong preference on good architecture patterns... For some things, which are language differences between python and rust, you should be allowed to make straightforward simplifications... If you make any changes like this, it must be explicitly documented in the spec."

## Overview

This feature is a self-contained, publishable Rust library ("the crate") that turns the text of a `.condarc` configuration file into a validated, strongly-typed in-memory representation. It is the parsing/validation foundation for GEN-23 (Allez reading `~/.condarc` for package-selection preferences: channels, channel priority, default channels, custom channel URLs, proxy/auth settings).

The crate accepts a YAML string (the caller is responsible for locating and reading the file) and produces either a typed representation of the settings that were present, or a descriptive error explaining why the input is not a valid `.condarc`.

The authoritative definition of "valid `.condarc`" and "correct parsed value" is not conda's source code but the three artifacts already produced under this ticket:

- `conformance/condarc/valid/*.json` and `conformance/condarc/invalid/*.json` — the accept/reject oracle.
- `conformance/condarc/expected/*.json` — conda's coerced internal representation for every `valid/` fixture.
- `docs/condarc_openapi.json` — a JSON Schema (the `components.schemas.Condarc` subschema) that upholds those fixtures.
- `docs/condarc_research.md` — the prose research backing all of the above.

The crate MUST agree with these artifacts. It is explicitly NOT required to consult conda's own source, the conformance-generation scripts, or the raw conformance documents beyond what is needed to satisfy the fixtures.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Parse a valid `.condarc` into typed settings (Priority: P1)

A developer integrating Allez (or any downstream consumer) passes the text of a user's `.condarc` to the crate and receives a typed object whose fields reflect the settings that were present in the file, with each value coerced to its proper type (booleans, numbers, enums, lists, maps, and strings), exactly as conda would coerce them.

**Why this priority**: This is the core purpose of the crate and the minimum viable product. Without it, GEN-23 cannot read channels, channel priority, default channels, custom channel URLs, or proxy settings.

**Independent Test**: Feed every `conformance/condarc/valid/*.json` fixture (as YAML) to the crate, assert it accepts each one, and assert that its representation — after passing through the documented adapter to a JSON value — equals the corresponding `conformance/condarc/expected/*.json`.

**Acceptance Scenarios**:

1. **Given** a YAML string `{"channels": ["conda-forge", "defaults"], "channel_priority": "strict", "always_yes": true}`, **When** the caller parses it, **Then** the result is accepted and exposes `channels = ["conda-forge", "defaults"]`, `channel_priority = strict`, and `always_yes = true`.
2. **Given** a YAML string setting a boolean-typed key to the string `"yes"`, **When** parsed, **Then** the stored value is the boolean `true` (conda's coercion), not the string `"yes"`.
3. **Given** a YAML string using an alias spelling (e.g. `channel`, `verify_ssl`, `yes`, `auto_activate`), **When** parsed, **Then** the value is stored under the canonical setting and the adapted representation uses conda's canonical internal name (e.g. `channels`, `ssl_verify`, `always_yes`, `auto_activate`).
4. **Given** a YAML string whose root is empty / `null` (empty file), **When** parsed, **Then** the result is an accepted, empty configuration (no error).

---

### User Story 2 - Reject an invalid `.condarc` with a complete, structured error report (Priority: P1)

A developer passes malformed or type-invalid `.condarc` text to the crate and receives a structured report of **all** the problems in the document at once — not just the first one — so they can see and fix everything in a single pass, instead of the fix-one-rerun-discover-the-next loop that first-error-only reporting forces. Each problem carries where it is (which setting), what is wrong (a human message and a stable machine code), and what value caused it.

**Why this priority**: GEN-23's acceptance criteria require the crate to "return an error if the `.condarc` is not valid" with "appropriate error messages." Complete, structured error collection (in the style of Pydantic's `ValidationError`, which accumulates every field error into a list rather than aborting on the first) is materially better for both humans editing a `.condarc` and agents programmatically repairing one, and directly serves the project's dual-interface (human + machine) principle.

**Independent Test**: Feed every `conformance/condarc/invalid/*.json` fixture (as YAML) to the crate — including the per-key "explosion" the conformance harness performs on non-`_combined` object fixtures — and assert it rejects each with a structured error report (never a panic). Additionally, feed a hand-built document with several independently-invalid settings and assert the report contains an entry for every one of them.

**Acceptance Scenarios**:

1. **Given** a YAML string setting `channel_alias` to a value with no URL scheme, **When** parsed, **Then** the crate returns a failure whose report contains an entry located at `channel_alias` with a validation error code and the offending value.
2. **Given** a YAML string whose root is a list or a bare scalar, **When** parsed, **Then** the crate returns a structured failure (not a panic) with a single root-shape entry, even though real conda crashes on this input.
3. **Given** a YAML string that sets both aliases of one setting in the same document (e.g. `always_yes` and `yes`), **When** parsed, **Then** the report contains an alias-collision entry naming both keys.
4. **Given** a YAML string that sets `always_copy` and `always_softlink` both truthy, or sets `client_ssl_cert_key` without `client_ssl_cert`, **When** parsed, **Then** the report contains the corresponding cross-field validation entry.
5. **Given** a YAML string in which multiple distinct settings are each independently invalid (e.g. a bad `channel_alias`, an out-of-range integer for `remote_max_retries`, and a non-boolish `always_copy`), **When** parsed, **Then** the report contains one entry per invalid setting — all of them — not only the first encountered.
6. **Given** the structured failure report, **When** a consumer serializes it to JSON, **Then** it is a list of entries each with a location, a machine-readable code, a human-readable message, and the offending input, suitable for programmatic consumption.

---

### User Story 3 - Adapter to a portable representation for conformance (Priority: P2)

A maintainer runs the conformance suite. The crate provides a documented adapter that renders its internal representation into the same JSON shape as `conformance/condarc/expected/*.json`. Absent settings are represented as absent (see FR-038), so the adapter naturally emits only the settings that were present — matching `expected/`, which likewise records only present keys. The conformance comparison is nonetheless specified as **subset-based** (every key present in the `expected/` fixture must match the adapted output) so it remains robust even if the crate later chooses to emit an occasional sensible default.

**Why this priority**: This is the explicit acceptance criterion — "matching the expected.json internal state" — and the ticket states this may be achieved via an adapter layer rather than forcing the internal structure to mirror the JSON.

**Independent Test**: For every `valid/` fixture, parse it, run the adapter, and assert that for every key present in the matching `expected/` fixture, the adapted output has an equal value.

**Acceptance Scenarios**:

1. **Given** any accepted `valid/` fixture, **When** the adapter renders the parsed configuration, **Then** for every key present in the corresponding `expected/` fixture the adapted value equals the expected value (canonical internal names, coerced values).
2. **Given** a numeric value larger than the crate's supported numeric range (see Assumptions / language-simplification note), **When** parsed, **Then** the behavior matches the (adjusted) conformance fixtures per the documented simplification.

---

### Edge Cases

- **Empty / null root**: An empty document or a bare YAML `~`/`null` root is accepted as an empty configuration (matches `valid/null_root.json`, `valid/empty_object.json`).
- **List root / scalar root**: A document whose root is a sequence or a bare scalar is rejected with a structured error. Real conda crashes with an unhandled exception here; the crate MUST instead return a typed error (the conformance oracle only checks accept/reject, so a clean error satisfies the `invalid/` verdict). See `invalid/array_root.json`, `invalid/scalar_root.json`.
- **Unknown / unrecognized keys**: Unknown top-level keys are **silently accepted** — parsing succeeds and they cause no error (matching real conda and GEN-23's "ignore unrelated `.condarc` keys without failing"). No conformance fixture exercises an unknown top-level key today, so this leniency is consistent with the accept/reject fixtures; it does, however, diverge from `docs/condarc_openapi.json`'s `additionalProperties: false`, which MUST be relaxed to `additionalProperties: true` (or otherwise reconciled) so the openapi checker agrees with the crate. How unknown keys are surfaced to callers (dropped, retained in a raw/extra map for later inspection, etc.) is an implementation-design choice for the plan phase, not a behavioral requirement here — see FR-036.
- **Alias collision across the same document**: Two aliases of one setting in one document → error (`MultipleKeysError`). Aliases across *separate* documents are not this crate's concern (it parses a single document).
- **Whitespace-padded boolish/enum strings**: e.g. `" true "`, `"\t\nyes\n\t"` are accepted and coerced (conda trims before coercing typed scalars).
- **Coercion of non-string scalars into string-typed keys**: e.g. `true` → `"True"`, `7` → `"7"`, `null` → `"None"` for plain-string keys; exact-string keys preserve interior/edge whitespace.
- **`"none"`-string coercion for nullable maps/strings**: the literal string `"none"` (case-insensitive) coerces to a null value for `{str, None}`-typed keys.
- **Sequence keys reject bare scalars**: a sequence-typed key given a bare scalar string is rejected (not auto-wrapped); an empty object `{}` is accepted as an empty sequence.
- **Non-finite and out-of-range numbers**: `inf`/`nan` strings for float keys; integer strings and float values that exceed fixed-width numeric ranges — see the language-simplification note in Assumptions.
- **Filesystem-dependent `ssl_verify`**: a non-boolish, non-`truststore` `ssl_verify` string is valid only if it is a path that exists. This is environment-dependent and cannot be verified portably; see Assumptions.

## Requirements *(mandatory)*

### Functional Requirements

**Input / API surface**

- **FR-001**: The crate MUST expose a function that accepts a YAML string and returns either a typed configuration value or a typed error. The caller (not the crate) is responsible for locating and reading the `.condarc` file.
- **FR-002**: The crate MUST NOT perform any filesystem, network, or environment access as part of parsing, except where a setting's validity is inherently defined in terms of the local filesystem (`ssl_verify` path existence — see FR-024).
- **FR-003**: The crate MUST be a standalone library with no dependency on the surrounding Allez binary, suitable for future independent publication.
- **FR-004**: The crate MUST NOT panic on any input, valid or invalid; all failure modes MUST be reported as typed, recoverable errors (no `.unwrap()`/`.expect()` in library code per the project constitution).

**Root-shape handling**

- **FR-005**: The crate MUST accept a document whose root is an empty/`null` YAML value and treat it as an empty configuration.
- **FR-006**: The crate MUST accept a document whose root is a mapping.
- **FR-007**: The crate MUST reject a document whose root is a sequence or a bare scalar, returning a structured error (not a crash).
- **FR-008**: The crate MUST reject YAML that is syntactically invalid, returning a structured parse error distinct from validation errors.

**Setting catalog & typing**

- **FR-009**: The crate MUST recognize every user-facing `.condarc` setting documented in `docs/condarc_research.md` §4 and modeled in `docs/condarc_openapi.json` (channel configuration, basic, network, solver, package-linking, output/prompt/flow, hidden/undocumented, plugin, experimental groups). Conda-build configuration keys (`bld_path`, `croot`, `anaconda_upload`, `conda_build`) are out of scope.
- **FR-010**: The crate MUST represent each setting with a type that makes conda's accepted value set representable and invalid states hard to represent (typed enums for enum settings, typed booleans, typed numbers, typed collections), per the project constitution's type-safety principle.
- **FR-011**: The crate MUST recognize each setting's documented aliases and treat an alias spelling identically to the canonical spelling.

**Value coercion (must match conda / `expected/`)**

- **FR-012**: For boolean-typed settings, the crate MUST coerce values using conda's boolean rules: JSON booleans; any number via truthiness; the boolish string vocabulary (`true`/`yes`/`on`/`y`, `false`/`off`/`n`/`no`/`non`/`none`/empty) case-insensitively and whitespace-trimmed; and complex/numeric-looking strings. Non-boolish, non-numeric-parseable strings (e.g. `"banana"`) MUST be rejected.
- **FR-013**: For nullable boolean settings (`always_yes`, `report_errors`, `show_channel_urls`, `use_only_tar_bz2`), the crate MUST additionally accept the null-string tokens `"null"`, `"~"`, and the null byte, coercing them (and a YAML `null`) to a null value.
- **FR-014**: For plain-string settings, the crate MUST coerce non-string scalars via string conversion (`true`→`"True"`, `7`→`"7"`, `null`→`"None"`) and MUST preserve interior and edge whitespace for values that are already strings.
- **FR-015**: For nullable-string settings, the crate MUST behave like plain-string settings except that the literal string `"none"` (case-insensitive) coerces to a null value.
- **FR-016**: For enum settings (`channel_priority`, `path_conflict`, `safety_checks`, `sat_solver`), the crate MUST accept either the lowercase value string or the exact Python member-name spelling, case-sensitively and whitespace-trimmed, and reject other casings.
- **FR-017**: For `channel_priority` specifically, the crate MUST additionally accept a JSON boolean and boolish strings (`true`/`yes`/`on` → `flexible`; `false`/`no`/`off` → `disabled`, any casing).
- **FR-018**: For integer settings, the crate MUST coerce via integer conversion: JSON booleans and numbers (floats truncated toward zero), and integer-looking strings (optional sign, PEP-515 single-underscore digit groups, leading zeros), whitespace-trimmed. Decimals, scientific notation, `nan`/`inf`, empty strings, non-decimal-base literals, and malformed underscores MUST be rejected.
- **FR-019**: For float settings, the crate MUST accept everything integer settings accept plus decimal, scientific-notation, and `nan`/`inf`/`infinity` strings (case-insensitive, optionally signed).
- **FR-020**: For `local_repodata_ttl` (`(bool, int)`), the crate MUST use the narrower boolish vocabulary (`true`/`yes`/`on`, `false`/`no`/`off` only) plus integers, and MUST reject the single-letter `y`/`n`, `non`/`none`/`null`/`~`/empty-string tokens and non-decimal-base literals.
- **FR-021**: For sequence-of-string settings, the crate MUST require a real list at the raw level (reject a bare scalar), MUST accept an empty object as an empty sequence and a YAML `null` as unset, and MUST string-coerce each element (rejecting nested arrays/objects as elements).
- **FR-022**: For `list_fields`, the crate MUST additionally restrict every element to the closed `CONDA_LIST_FIELDS` vocabulary (§5.9), matched exactly and case-sensitively with no trimming.
- **FR-023**: For map settings (`custom_channels`, `migrated_custom_channels`, `override_virtual_packages`, `proxy_servers`, `custom_multichannels`, `channel_settings`), the crate MUST require an object (or `null`/empty), string-coerce values (or sequence-of-strings for `custom_multichannels`; maps for `channel_settings` entries), and reject scalar/list values where an object is required.
- **FR-024**: For `ssl_verify`, the crate MUST accept booleans, boolish/numeric strings, the literal `truststore`, and a path that exists on the local filesystem; a non-boolish string that is not `truststore` and does not exist as a path MUST be rejected. The filesystem-existence check is the sole permitted environment access (see FR-002) and is a documented non-portable gap (see Assumptions).
- **FR-025**: For `channel_alias`, the crate MUST require that a non-empty value has a URL scheme matching conda's `has_scheme` rule (lowercase scheme, `[a-z][a-z0-9]{0,11}` followed by `://`); an empty string is accepted.
- **FR-026**: For `default_python`, the crate MUST accept an empty string / null (no pinning) or a `2.x`/`3.x` version string (leading `2` or `3`, a dot, then digits), and reject out-of-range or malformed forms.

**Cross-field & structural validation**

- **FR-027**: The crate MUST reject a document where `client_ssl_cert_key` is set truthy but `client_ssl_cert` is unset (post-build validation rule 1).
- **FR-028**: The crate MUST reject a document where `always_copy` and `always_softlink` are both truthy (post-build validation rule 2).
- **FR-029**: The crate MUST reject a document that sets two aliases of the same setting simultaneously, for each of the 20 documented alias pairs (`MultipleKeysError`).

**Errors**

**Errors**

- **FR-030**: On rejection, the crate MUST return a **structured error report that accumulates every independent problem in the document**, not just the first one encountered. The report MUST be a collection of individual error entries. Modeled on Pydantic's `ValidationError` (which gathers every field error into a list), but Rust-idiomatic (see FR-037 for the entry shape).
- **FR-031**: The crate MUST collect, in a single parse, all independently-detectable problems: every per-setting type/coercion error, every per-setting semantic validation error (e.g. `channel_alias` scheme, `default_python` range, `list_fields` vocabulary, `ssl_verify`), every alias collision, and every cross-field violation. Detecting one invalid setting MUST NOT prevent the crate from evaluating and reporting the others. (Errors that make per-field evaluation impossible are the documented exception — see FR-032.)
- **FR-032**: Two failure classes preclude per-field accumulation and MUST each be reported as a single-entry report: (a) a YAML syntax error (the document cannot be parsed into a value at all), and (b) a wrong root shape — a list or scalar root (there are no fields to evaluate). These are distinct entry categories from per-setting errors.
- **FR-033**: Each error entry MUST carry, at minimum: a **location** identifying the offending setting (the key as written in the document, plus a path for nested locations such as a `channel_settings` list index or a map key); a stable, machine-readable **error code/kind** (e.g. `type_coercion`, `semantic_validation`, `alias_collision`, `cross_field`, `root_shape`, `yaml_syntax`); a **human-readable message**; and the **offending input value** (or a description of it). Alias-collision and cross-field entries MUST name all settings involved.
- **FR-034**: The error report MUST be renderable both as a human-readable summary (all entries, one per problem, suitable for a CLI or interactive editing) and as a machine-readable JSON structure (a list of entries with the fields in FR-033), per the project's dual-interface principle. The JSON structure's shape is a documented, versioned contract.
- **FR-035**: The crate MUST NOT panic on any input, and MUST NOT abort accumulation on the first error, except for the two non-accumulable classes in FR-032.
- **FR-036**: Unknown top-level keys MUST NOT cause a parse failure or produce an error entry; the crate MUST accept them silently. Whether such keys are discarded or retained (e.g. in a raw/extra map for later inspection) is an implementation-design choice deferred to planning; the only behavioral requirement is that their presence never turns an otherwise-valid document into a failure.
- **FR-037**: The report's entry shape is a first-class, typed part of the public API (not stringly-typed): the location, kind/code, message, and offending input are individually accessible fields, so agents can branch on the kind and locate the setting without parsing a message string. The overall failure type MUST expose the full list of entries and MUST implement Rust's standard error trait for ergonomic use with `?` and error-handling libraries.

**Absent settings**

- **FR-038**: Absent settings MUST be represented as absent, not backfilled with conda's documented default. Concretely: a setting that was not present in the document is modeled as an optional/nullable value that reads back as "not set." The crate does NOT maintain a table of conda defaults, and there is no separate effective-value/defaulting layer. (Rationale: there are very few settings for which a default matters to this crate's purpose, and modeling absence directly keeps the representation honest and simple, matching the ticket's "missing vs. default is out of scope" stance.)
- **FR-039**: Where a plain, non-nullable boolean setting has an unambiguously safe off-state (e.g. flags that default to `false` in conda), the crate MAY represent its absent state as `false` rather than as optional, when doing so yields a saner, less-optional caller API. Any such choice MUST be a safe default (per the constitution's "defaults chosen for safety") and MUST NOT change the adapter output for a document where the setting was absent (the adapter still omits absent settings — see FR-040). This is the only form of defaulting permitted; no non-boolean setting is backfilled.

**Conformance adapter**

- **FR-040**: The crate MUST provide an adapter that renders the parsed configuration into a JSON value using conda's canonical internal setting names (aliases resolved to their loader attribute name, e.g. `auto_activate`, `channels`, `always_yes`, `solver`) and the coerced values, **emitting only settings that were present** in the document (matching `expected/`, which records only present keys). The conformance comparison MUST be **subset-based**: for each `valid/` fixture, the harness takes the keys present in the corresponding `expected/` fixture and asserts each has an equal value in the adapted output; this remains correct even in the FR-039 boolean-default case.
- **FR-041**: The adapter MUST encode numeric values consistently with the `expected/` fixtures: non-finite floats (`inf`/`nan` inputs, e.g. `remote_connect_timeout_secs: "inf"`) as the strings `"Infinity"`/`"-Infinity"`/`"NaN"`. Values whose magnitude exceeds the crate's supported numeric range are rejected at parse time (see Assumptions A1), so the adapter never has to encode an over-bound value.
- **FR-042**: The crate MUST pass the existing conformance harness (`tests/condarc_conformance.rs`) as the `Crate` checker: accepting every `valid/` fixture, rejecting every exploded `invalid/` case, and (via the subset-based adapter comparison) matching every `expected/` fixture — subject to the documented conformance-fixture and harness adjustments in the Assumptions section.

### Key Entities

- **Configuration (internal representation)**: The typed, in-memory result of parsing one `.condarc` document. Settings that were absent are represented as absent (optional/nullable), not backfilled with conda defaults, except that plain off-state booleans may model absence as `false` (FR-039). Is the crate's primary output.
- **Setting value types**: The typed shapes a setting may hold — booleans, nullable booleans, integers, floats, enums (channel priority, path conflict, safety checks, sat solver), plain/nullable strings, string sequences, closed-vocabulary sequences (`list_fields`), string maps, nullable string maps, sequence maps, and channel-settings entries.
- **Parse/validation error**: The typed failure result, categorized (syntax, root shape, type/coercion, semantic, alias collision, cross-field), carrying the offending setting name and a human-readable reason. Unknown top-level keys are not an error.
- **Unknown/extra keys**: Top-level keys not in the recognized catalog. Accepted silently; retention strategy (dropped vs. kept in a raw map) deferred to planning.
- **Conformance adapter**: The translation from the internal representation to the portable JSON shape used by `conformance/condarc/expected/*.json`, compared subset-wise against the keys present in each expected fixture.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The crate accepts 100% of `conformance/condarc/valid/*.json` fixtures (as YAML) and rejects 100% of `conformance/condarc/invalid/*.json` fixtures (including the per-key explosion the harness applies), with zero panics.
- **SC-002**: For 100% of accepted `valid/` fixtures, the adapter's output matches the corresponding `conformance/condarc/expected/*.json` under subset comparison (every key present in the expected fixture has an equal value in the adapted output).
- **SC-003**: The crate wired in as the `Crate` checker in `tests/condarc_conformance.rs` turns that checker from "skipped" to "passing" for every fixture, with the conda and openapi checkers still passing on the same fixture set (openapi's `additionalProperties` relaxed per the unknown-key decision).
- **SC-004**: Every documented `.condarc` setting (per `docs/condarc_research.md` §4, conda-build excluded) has at least one accepting and, where applicable, one rejecting fixture exercised against the crate.
- **SC-005**: A document with several independently-invalid settings produces one error-report entry per invalid setting (all of them), verified by at least one multi-error test case.
- **SC-006**: Any deviation from conda's runtime behavior introduced for Rust/Python language reasons is enumerated in this spec's Assumptions section and reflected by a corresponding, committed change to the conformance fixtures and/or harness — no undocumented divergence exists.

## Assumptions

### General

- The caller reads the file and passes its contents as a string; file location, existence, and I/O errors are the caller's responsibility (GEN-23 handles the missing-file fallback), consistent with FR-001.
- The crate parses a **single** `.condarc` document. Conda's multi-source search-path merging and precedence are out of scope (matching the conformance harness, which replaces the search path with a single fixture file).
- Conda-build configuration keys are out of scope (`docs/condarc_research.md` §8 item 5).
- The internal representation is not required to round-trip back to identical YAML; comments, key ordering, and the "missing vs. default" distinction are out of scope (per the GEN-36 ticket). Absent settings are modeled as absent (optional/nullable), not backfilled with conda defaults — see FR-038/FR-039. The conformance comparison is subset-based (FR-040) so the adapter only needs to emit present settings.

### Documented language-difference simplifications (Rust vs. Python)

These are deliberate deviations from conda's exact runtime behavior, permitted by the ticket, each requiring a matching adjustment to the conformance fixtures. Each MUST be reflected in a committed fixture change; SC-006 tracks this.

- **A1 — Numeric magnitude bound**: Python integers are arbitrary-precision and Python floats silently overflow to infinity; the crate uses fixed-width numeric types chosen for the Rust caller experience, not to mirror Python's unbounded semantics. **Decision**: integer-typed settings use Rust's idiomatic signed integer `i64` (thread counts, retry counts, verbosity, cache-depth — none of which have any real need to exceed a few thousand, let alone `i64`'s ~9.2×10¹⁸ ceiling); float-typed settings use `f64` (IEEE-754 double). A value whose magnitude exceeds the supported range is **rejected** with a typed error (not saturated/clamped — silent value changes would violate the constitution's "explicit over implicit" principle). Callers therefore always get either an exact in-range value or an error, never a surprising wrapped/clamped number. The following existing fixtures assert Python's arbitrary-precision / overflow behavior that a fixed-width `i64`/`f64` implementation cannot and should not reproduce, and MUST be moved from `valid/` to `invalid/` (with their `expected/` entries removed), since these magnitudes are now rejections:
  - `conformance/condarc/valid/numeric_values_accept_numeric_string_bignum_exceeds_i64_max.json`
  - `conformance/condarc/valid/numeric_values_accept_numeric_string_bignum_exceeds_u64_max.json`
  - `conformance/condarc/valid/numeric_values_accept_numeric_string_bignum_exceeds_f64_max_finite.json`
  - `conformance/condarc/valid/numeric_values_accept_numeric_string_bignum_negative.json` (if its magnitude exceeds `i64` range; otherwise it stays `valid/`)
  - and their corresponding `conformance/condarc/expected/*.json` entries (which encode giant integers and `"Infinity"` as strings).

  Note: the `"Infinity"`/`"-Infinity"`/`"NaN"` encoding for *legitimately non-finite* float inputs (e.g. `remote_connect_timeout_secs: "inf"`/`"nan"`, which are small, in-range strings that conda accepts as float specials) is **retained** — those fixtures stay `valid/`. Only the *overflow-from-a-huge-finite-numeral* fixtures above are affected by the magnitude bound.

- **A2 — Root-shape and other conda crash bugs become clean errors**: Where real conda crashes with an unhandled exception (list/scalar root; non-empty array/object fed to a scalar-typed key; `null`/word/hex-literal fed to certain boolish keys; `local_repodata_ttl` hex-literal), the crate returns a typed error instead. The conformance harness's accept/reject verdict is unaffected (still `invalid/`), so no fixture changes are required for A2; the crate simply never reproduces conda's exact crash text (which the harness does not assert on).

- **A3 — `ssl_verify` filesystem check**: The path-existence branch of `ssl_verify` is environment-dependent and not portably encodable. The crate matches the reference schema's approach: accept `truststore`, boolish/numeric values, and the current-directory path `"."`; other arbitrary paths cannot be verified without touching the filesystem. The crate MAY perform the real filesystem-existence check (FR-024) to match conda exactly at runtime, but conformance only guarantees the portable subset the schema encodes.

### Unknown-key handling

**Decision**: unknown top-level keys are silently accepted (conda-lenient), matching real conda and GEN-23's parent acceptance criteria ("ignore unrelated `.condarc` keys without failing"). No conformance fixture exercises an unknown top-level key, so this is consistent with the existing accept/reject fixtures. Because `docs/condarc_openapi.json` currently declares `additionalProperties: false`, that schema MUST be relaxed (to `additionalProperties: true`, or otherwise reconciled) so the openapi checker agrees with the crate; this is a committed, documented change tracked by SC-006. How unknown keys are surfaced to callers (dropped vs. retained in a raw/extra map) is deferred to planning per FR-036.
