# Feature Specification: `.condarc` Parser Library

**Feature Branch**: `GEN-36_condarc_parser_library`

**Created**: 2026-07-23

**Status**: Draft

**Input**: User description: "GEN-36. We have completed the research and conformance testing as to what a valid .condarc requires. That information lives in conformance tests, it lives in an openapi.json file which upholds the conformance tests, and it lives in the docs/condarc_research.md file. Now it is time to write a spec for a rust library which implements this behavior. The goal for this crate is that it should take in a yaml string (the caller can load the file), and generate the internal representation of a .condarc as typed rust objects... The acceptance criteria for the spec is that the library passes all the conformance tests, including matching the expected.json internal state... have a strong preference on good architecture patterns... For some things, which are language differences between python and rust, you should be allowed to make straightforward simplifications... If you make any changes like this, it must be explicitly documented in the spec."

## Overview

This feature is a self-contained, publishable Rust library ("the crate") that turns the text of a `.condarc` configuration file into a validated, strongly-typed in-memory representation. It is the parsing/validation foundation for GEN-23 (Allez reading `~/.condarc` for package-selection preferences: channels, channel priority, default channels, custom channel URLs, proxy/auth settings).

The crate accepts a YAML string (the caller is responsible for locating and reading the file) and produces either a typed representation of the settings that were present, or a descriptive error explaining why the input is not a valid `.condarc`.

> **Deliberate departure from the GEN-36 ticket wording.** The ticket describes a library "which takes a path". This spec is **string-in only**: the crate never opens a file. Callers read the bytes themselves — from `~/.condarc`, from stdin, from an embedded test fixture, from an HTTP response — and hand the crate a `&str`. That keeps the crate hermetic and testable (FR-002), keeps the missing-file policy where it belongs (GEN-23's "tolerate a missing file, fall back to sane defaults"), and makes the stdin/piped-config case expressible at all. FR-001/FR-002 are normative; the ticket's "takes a path" phrasing is superseded.

The authoritative definition of "valid `.condarc`" and "correct parsed value" is not conda's source code but the three artifacts already produced under this ticket:

- `conformance/condarc/valid/*.json` and `conformance/condarc/invalid/*.json` — the accept/reject oracle.
- `conformance/condarc/expected/*.json` — conda's coerced internal representation for every `valid/` fixture.
- `docs/condarc_openapi.json` — a JSON Schema (the `components.schemas.Condarc` subschema) that upholds those fixtures.
- `docs/condarc_research.md` — the prose research backing all of the above.

The crate MUST agree with these artifacts. It is explicitly NOT required to consult conda's own source, the conformance-generation scripts, or the raw conformance documents beyond what is needed to satisfy the fixtures.

## Clarifications

### Session 2026-07-24

- Q: FR-004 requires the crate to never panic on any input, but nothing addresses pathologically deep/large YAML (e.g. thousands of nested levels) that could overflow the process stack during recursive-descent parsing — a failure mode distinct from a Rust panic and not preventable by avoiding `.unwrap()`/`.expect()`. Should this be (A) documented as an accepted out-of-scope limitation, (B) guarded against with an explicit nesting-depth/size limit enforced by the crate, or (C) left to whatever limits the underlying YAML library happens to enforce internally? → A: Documented as an accepted, out-of-scope limitation (Option A) — see Assumptions A5.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Parse a valid `.condarc` into typed settings (Priority: P1)

A developer integrating Allez (or any downstream consumer) passes the text of a user's `.condarc` to the crate and receives a typed object whose fields reflect the settings that were present in the file, with each value coerced to its proper type (booleans, numbers, enums, lists, maps, and strings), exactly as conda would coerce them.

**Why this priority**: This is the core purpose of the crate and the minimum viable product. Without it, GEN-23 cannot read channels, channel priority, default channels, custom channel URLs, or proxy settings.

**Independent Test**: Feed every `conformance/condarc/valid/*.json` fixture (as YAML) to the crate, assert it accepts each one, and assert that its representation — after passing through the documented adapter to a JSON value — equals the corresponding `conformance/condarc/expected/*.json`.

**Acceptance Scenarios**:

1. **Given** a YAML string `{"channels": ["conda-forge", "defaults"], "channel_priority": "strict", "always_yes": true}`, **When** the caller parses it, **Then** the result is accepted and exposes `channels = ["conda-forge", "defaults"]`, `channel_priority = strict`, and `always_yes = true`.
2. **Given** a YAML string setting a boolean-typed key to the string `"yes"`, **When** parsed, **Then** the stored value is the boolean `true` (conda's coercion), not the string `"yes"`.
3. **Given** a YAML string using an alias spelling (e.g. `channel`, `verify_ssl`, `yes`, `auto_activate_base`), **When** parsed, **Then** the value is stored under the canonical setting and the adapted representation uses conda's canonical internal name (e.g. `channels`, `ssl_verify`, `always_yes`, `auto_activate`).
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

A maintainer runs the conformance suite. A documented, **test-only** adapter (it lives with the conformance harness, not in the published crate — see FR-040) renders the crate's internal representation into the same JSON shape as `conformance/condarc/expected/*.json`. Absent settings are represented as absent (see FR-038), so the adapter emits exactly the settings that were present — matching `expected/`, which likewise records only present keys. The conformance comparison is therefore specified as **exact object equality**, not a subset check: the adapted output must equal the `expected/` fixture with no missing and no extra keys.

**Why this priority**: This is the explicit acceptance criterion — "matching the expected.json internal state" — and the ticket states this may be achieved via an adapter layer rather than forcing the internal structure to mirror the JSON.

**Independent Test**: For every `valid/` fixture, parse it, run the adapter, and assert the adapted JSON object equals the corresponding `expected/*.json` exactly.

**Acceptance Scenarios**:

1. **Given** any accepted `valid/` fixture, **When** the adapter renders the parsed configuration, **Then** the adapted JSON object equals the corresponding `expected/` fixture exactly (same key set, canonical internal names, coerced values).
2. **Given** a document containing an unrecognized top-level key, **When** the adapter renders it, **Then** that key is absent from the adapted output (unknown keys are accepted but never adapted — FR-036/FR-040), so exact equality is not perturbed by them.
3. **Given** a numeric value larger than the crate's supported numeric range (see Assumptions / language-simplification note), **When** parsed, **Then** the behavior matches the (adjusted) conformance fixtures per the documented simplification.

---

### Edge Cases

- **Empty / null root**: An empty document or a bare YAML `~`/`null` root is accepted as an empty configuration (matches `valid/null_root.json`, `valid/empty_object.json`).
- **List root / scalar root**: A document whose root is a sequence or a bare scalar is rejected with a structured error. Real conda crashes with an unhandled exception here; the crate MUST instead return a typed error (the conformance oracle only checks accept/reject, so a clean error satisfies the `invalid/` verdict). See `invalid/array_root.json`, `invalid/scalar_root.json`.
- **Only JSON-shaped YAML is in scope**: a `.condarc` is a single YAML document whose mapping keys are strings. Multi-document streams (`---`-separated) and non-string mapping keys (`1: x`, `[a]: b`, `? {}`) are **out of scope** — see FR-007a/FR-007b: the crate rejects them with a structured error rather than attempting to interpret them, and no conformance fixture asserts conda's behavior for them. Anchors/aliases, merge keys, and custom tags are likewise out of scope in the sense that the crate only needs to handle what a JSON-shaped document can express; whatever the YAML parser resolves them to is then treated as ordinary values.
- **Unknown / unrecognized keys**: Unknown top-level keys are **silently accepted** — parsing succeeds and they cause no error (matching real conda and GEN-23's "ignore unrelated `.condarc` keys without failing"). No conformance fixture exercises an unknown top-level key today, so this leniency is consistent with the accept/reject fixtures, and `docs/condarc_openapi.json` already sets `additionalProperties: true`, so the openapi checker agrees. How unknown keys are surfaced to callers (dropped, retained in a raw/extra map for later inspection, etc.) is an implementation-design choice for the plan phase, not a behavioral requirement here — see FR-036.
- **Alias collision across the same document**: Two aliases of one setting in one document → error (`MultipleKeysError`). Aliases across *separate* documents are not this crate's concern (it parses a single document).
- **Whitespace-padded boolish/enum strings**: e.g. `" true "`, `"\t\nyes\n\t"` are accepted and coerced (conda trims before coercing typed scalars).
- **Coercion of non-string scalars into string-typed keys**: e.g. `true` → `"True"`, `7` → `"7"`, `null` → `"None"` for plain-string keys; exact-string keys preserve interior/edge whitespace.
- **`"none"`-string coercion for nullable maps/strings**: the literal string `"none"` (case-insensitive) coerces to a null value for `{str, None}`-typed keys.
- **Sequence keys reject bare scalars**: a sequence-typed key given a bare scalar string is rejected (not auto-wrapped); an empty object `{}` is accepted as an empty sequence.
- **Non-finite and out-of-range numbers**: `inf`/`nan` strings for float keys; integer strings and float values that exceed fixed-width numeric ranges — see the language-simplification note in Assumptions.
- **Filesystem-dependent `ssl_verify`**: a non-boolish, non-`truststore` `ssl_verify` string is valid *in conda* only if it is a path that exists. Parsing is side-effect-free by default, so the crate does not perform that check unless the caller opts in; see FR-024 and Assumptions A3.
- **Pathologically deep/large documents**: Not defended against. The crate does not enforce a nesting-depth or document-size limit, so an adversarially deep or huge document can overflow the process stack during parsing rather than yield a structured error. This is a documented, accepted exception to FR-004 — see Assumptions A5. No conformance fixture exercises this shape, since real conda's behavior here is not part of the accept/reject oracle.

## Requirements *(mandatory)*

### Functional Requirements

**Input / API surface**

- **FR-001**: The crate MUST expose a function that accepts a YAML string and returns either a typed configuration value or a typed error. The crate MUST NOT expose any path-taking or file-reading entry point: locating and reading the `.condarc` (or reading it from stdin, or from anywhere else) is the caller's responsibility. This deliberately supersedes the GEN-36 ticket's "takes a path" phrasing — see the note in the Overview.
- **FR-002**: The crate MUST NOT perform any filesystem, network, or environment access as part of parsing. The single exception is opt-in: the `ssl_verify` path-existence check, which is performed only when the caller explicitly requests it (FR-024). With default options, parsing is a pure function of the input string — the same document yields the same result on every machine.
- **FR-003**: The crate MUST be a standalone library with no dependency on the surrounding Allez binary, suitable for future independent publication.
- **FR-004**: The crate MUST NOT panic on any input, valid or invalid; all failure modes MUST be reported as typed, recoverable errors (no `.unwrap()`/`.expect()` in library code per the project constitution). This guarantee covers all inputs that do not exhaust the process call stack; a pathologically deep/large document that overflows the stack during recursive-descent parsing is a documented exception — see Assumptions A5.

**Root-shape handling**

- **FR-005**: The crate MUST accept a document whose root is an empty/`null` YAML value and treat it as an empty configuration.
- **FR-006**: The crate MUST accept a document whose root is a mapping.
- **FR-007**: The crate MUST reject a document whose root is a sequence or a bare scalar, returning a structured error (not a crash).
- **FR-007a**: The crate MUST reject an input containing more than one YAML document (a `---`-separated stream) with a structured error. Multi-document `.condarc` files are out of scope: a `.condarc` is one document.
- **FR-007b**: The crate MUST reject a mapping key that is not a string (at the root or in any nested map) with a structured error naming the offending location. Non-string keys are out of scope: only JSON-shaped YAML is supported.
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
- **FR-024**: For `ssl_verify`, the crate MUST accept booleans, boolish/numeric strings, and the literal `truststore`. Any other string is a certificate-path value. Whether that path is required to exist is caller-controlled:
  - **Default (side-effect-free)**: the crate performs **no** filesystem access and accepts the string as an unverified path. Parsing therefore yields the same verdict for the same document on every machine (FR-002).
  - **Opt-in**: when the caller enables the path-existence check, a path that does not exist on the local filesystem is rejected, exactly matching conda's runtime behavior. This is the only filesystem access the crate ever performs, and it happens only on explicit request.

  The conformance corpus was generated from real conda, which always performs the check (`invalid/ssl_verify_passthrough_reject_string_nonexistent_path.json`, `..._reject_string_arbitrary_word.json`, `..._reject_truststore_wrong_case.json` are all rejections *because the string is not an existing path*). The conformance harness therefore runs the crate checker with the check **enabled**, so the crate is held to conda's exact behavior; see Assumptions A3.
- **FR-025**: For `channel_alias`, the crate MUST require that a non-empty value has a URL scheme matching conda's `has_scheme` rule (lowercase scheme, `[a-z][a-z0-9]{0,11}` followed by `://`); an empty string is accepted.
- **FR-026**: For `default_python`, the crate MUST implement conda's `default_python_validation` exactly, which is **not** a `[23].[0-9][0-9]?` pattern despite what conda's own `settings.rst` says. A value is accepted iff it is empty/`null`/falsy ("no pinning"), **or** all three of the following hold: (1) its length is at least 3; (2) the character at index 1 is a literal `.`; (3) the *entire* string parses as a floating-point number in `[2.0, 4.0)`. Consequences the crate MUST reproduce, each pinned by a conformance fixture verified against real conda:
  - Any number of fraction digits is allowed — `2.0`, `2.00`, `3.99`, `3.999999`, `3.9999999999` are all accepted; the digit count is never checked, only the resulting numeric range.
  - The characters after the dot need not be digits at all, as long as the whole string still parses as a float: `3.e0`, `2.0e0`, `2.5E0` are accepted (`valid/default_python_accept_exponent_form_*`).
  - PEP-515 digit-group underscores are accepted where a float parser accepts them: `2.5_5` (= 2.55) is valid; `3._5` (underscore not between digits) is rejected.
  - An exponent may move an otherwise well-shaped value out of range: `3.5e-1` (= 0.35) is rejected by the range check, and `3e0` is rejected earlier because index 1 is `e`, not `.`.
  - `4.0` (exclusive upper bound), `1.9`, `9.5` (range), `3`, `3.` (length), `03.9`, `33.9`, `-3.5` (dot not at index 1), `3.10.1`, `3,9`, `3.a` (unparseable) are all rejected.

  See Assumptions A4 for the one deliberate divergence (non-ASCII Unicode decimal digits).

**Cross-field & structural validation**

- **FR-027**: The crate MUST reject a document where `client_ssl_cert_key` is set truthy but `client_ssl_cert` is unset (post-build validation rule 1).
- **FR-028**: The crate MUST reject a document where `always_copy` and `always_softlink` are both truthy (post-build validation rule 2).
- **FR-029**: The crate MUST reject a document that sets two aliases of the same setting simultaneously, for each of the 20 documented alias pairs (`MultipleKeysError`).

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

- **FR-038**: Absent settings MUST be represented as absent, not backfilled with conda's documented default, and there is **no defaulting anywhere in the crate**. Concretely: every setting is modeled as an optional/nullable value that reads back as "not set" when the document did not contain it. The crate does NOT maintain a table of conda defaults, and there is no separate effective-value/defaulting layer — not even for booleans with an obvious off-state. A caller that wants a default applies its own policy explicitly. (Rationale: there are very few settings for which a default matters to this crate's purpose, and modeling absence directly keeps the representation honest and simple, matching the ticket's "missing vs. default is out of scope" stance.)

**Conformance adapter**

- **FR-040**: The **conformance harness** (not the crate) MUST provide an adapter that renders a parsed configuration into a JSON value using conda's canonical internal setting names (aliases resolved to their loader attribute name, e.g. `channels`, `always_yes`, `solver`, and — verified against conda 26.5.3 — `auto_activate`, whose historical spelling `auto_activate_base` is the *alias*), with coerced values, **emitting only settings that were present** in the document. The adapter is test-only support code and is NOT part of the crate's published public API; it consumes the crate's public typed representation exactly as any downstream caller would. The conformance comparison MUST be **exact**: for each `valid/` fixture, the adapted JSON object must equal the corresponding `expected/` fixture — same key set, same values — so an adapter that emits an extra or renamed key fails. (Verified feasible: for all 388 object-rooted `valid/` fixtures, the set of canonical names of the document's keys is exactly the key set of the corresponding `expected/` fixture, once `auto_activate` is treated as canonical.)
- **FR-041**: The adapter MUST encode numeric values consistently with the `expected/` fixtures: non-finite floats (`inf`/`nan` inputs, e.g. `remote_connect_timeout_secs: "inf"`) as the strings `"Infinity"`/`"-Infinity"`/`"NaN"`. Values whose magnitude exceeds the crate's supported numeric range are rejected at parse time (see Assumptions A1), so the adapter never has to encode an over-bound value.
- **FR-042**: The crate MUST pass the conformance harness as the `Crate` checker: accepting every `valid/` fixture, rejecting every exploded `invalid/` case, and (via the exact adapter comparison) matching every `expected/` fixture — with the sole exception of the explicitly enumerated, documented divergences in the Assumptions section, which are declared to the harness as a per-checker divergence list (A1) rather than by re-filing fixtures. The `Crate` checker MUST run with the `ssl_verify` path-existence check enabled (FR-024), since the corpus encodes conda's FS-checking behavior.

### Key Entities

- **Configuration (internal representation)**: The typed, in-memory result of parsing one `.condarc` document. Every setting is optional/nullable: absent means absent, never a backfilled conda default and never a synthesized `false` (FR-038). Is the crate's primary output.
- **Setting value types**: The typed shapes a setting may hold — booleans, nullable booleans, integers, floats, enums (channel priority, path conflict, safety checks, sat solver), plain/nullable strings, string sequences, closed-vocabulary sequences (`list_fields`), string maps, nullable string maps, sequence maps, and channel-settings entries.
- **Parse/validation error**: The typed failure result, categorized (syntax, root shape, type/coercion, semantic, alias collision, cross-field), carrying the offending setting name and a human-readable reason. Unknown top-level keys are not an error.
- **Unknown/extra keys**: Top-level keys not in the recognized catalog. Accepted silently; retention strategy (dropped vs. kept in a raw map) deferred to planning.
- **Conformance adapter**: Test-only harness support code translating the internal representation into the portable JSON shape used by `conformance/condarc/expected/*.json`, compared for exact equality against each expected fixture.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The crate accepts 100% of `conformance/condarc/valid/*.json` fixtures (as YAML) and rejects 100% of `conformance/condarc/invalid/*.json` fixtures (including the per-key explosion the harness applies), with zero panics — modulo the enumerated A1 divergences declared to the harness.
- **SC-002**: For 100% of accepted `valid/` fixtures that have an `expected/` counterpart, the adapter's output equals the corresponding `conformance/condarc/expected/*.json` exactly (same key set, same values).
- **SC-003**: The crate wired in as the `Crate` checker in the conformance harness turns that checker from "skipped" to "passing" for every fixture, with the conda and openapi checkers still passing on the same fixture set.
- **SC-004**: Every documented `.condarc` setting (per `docs/condarc_research.md` §4, conda-build excluded) has at least one accepting and, where applicable, one rejecting fixture exercised against the crate.
- **SC-005**: A document with several independently-invalid settings produces one error-report entry per invalid setting (all of them), verified by at least one multi-error test case.
- **SC-006**: Any deviation from conda's runtime behavior introduced for Rust/Python language reasons is enumerated in this spec's Assumptions section and reflected by a corresponding, committed change to the conformance fixtures and/or harness — no undocumented divergence exists.

## Assumptions

### General

- The caller reads the file (or stdin, or any other source) and passes its contents as a string; file location, existence, and I/O errors are the caller's responsibility (GEN-23 handles the missing-file fallback), consistent with FR-001. The crate has no path-taking API at all.
- The crate parses a **single** `.condarc` document, and only JSON-shaped YAML: one document per input (FR-007a) and string mapping keys only (FR-007b). Conda's multi-source search-path merging and precedence are out of scope (matching the conformance harness, which replaces the search path with a single fixture file).
- Conda-build configuration keys are out of scope (`docs/condarc_research.md` §8 item 5).
- The internal representation is not required to round-trip back to identical YAML; comments, key ordering, and the "missing vs. default" distinction are out of scope (per the GEN-36 ticket). Absent settings are modeled as absent (optional/nullable) with no defaulting layer of any kind — see FR-038. The adapter emits present settings only, and the conformance comparison is exact (FR-040).

### Documented language-difference simplifications (Rust vs. Python)

These are deliberate deviations from conda's exact runtime behavior, permitted by the ticket. Each MUST be enumerated here **and** declared to the conformance harness as a per-checker divergence, so no divergence is undocumented and none is silently tolerated; SC-006 tracks this.

**Divergence mechanism (applies to A1, and to any future divergence of the same shape).** A fixture that real conda *accepts* but the crate deliberately *rejects* cannot simply be re-filed from `valid/` to `invalid/`: the directory is the oracle for **every** checker, so moving it would make the conda checker fail (conda still accepts it) and would misrepresent conda's behavior in the corpus. Instead the harness carries an explicit, commented **`Crate`-checker divergence list**: fixture name → expected crate verdict + the spec assumption (e.g. `A1`) that authorizes it. For a listed fixture the harness asserts the crate produces *that* verdict, and asserts nothing changed for the conda/openapi checkers. The list is therefore an assertion, not a suppression: adding an entry requires a spec assumption, and a listed fixture that stops diverging fails the test.

- **A1 — Numeric magnitude bound**: Python integers are arbitrary-precision and Python floats silently overflow to infinity; the crate uses fixed-width numeric types chosen for the Rust caller experience, not to mirror Python's unbounded semantics. **Decision**: integer-typed settings use Rust's idiomatic signed integer `i64` (thread counts, retry counts, verbosity, cache-depth — none of which have any real need to exceed a few thousand, let alone `i64`'s ~9.2×10¹⁸ ceiling); float-typed settings use `f64` (IEEE-754 double). A value whose magnitude exceeds the supported range is **rejected** with a typed error (not saturated/clamped — silent value changes would violate the constitution's "explicit over implicit" principle). Callers therefore always get either an exact in-range value or an error, never a surprising wrapped/clamped number. The following existing fixtures assert Python's arbitrary-precision / overflow behavior that a fixed-width `i64`/`f64` implementation cannot and should not reproduce. They **stay in `valid/`** (conda really does accept them, and the conda/openapi checkers must keep proving that) and are instead declared in the harness's `Crate` divergence list as "crate rejects, per A1":
  - `conformance/condarc/valid/numeric_values_accept_numeric_string_bignum_exceeds_i64_max.json`
  - `conformance/condarc/valid/numeric_values_accept_numeric_string_bignum_exceeds_u64_max.json`
  - `conformance/condarc/valid/numeric_values_accept_numeric_string_bignum_exceeds_f64_max_finite.json`
  - `conformance/condarc/valid/numeric_values_accept_numeric_string_bignum_negative.json` (only if its magnitude exceeds `i64` range; otherwise it is not a divergence at all and is not listed)

  Their `expected/` fixtures (which encode giant integers, and `"Infinity"`, as strings) also stay in place, since the conda checker's live expected-representation assertion still uses them; the adapter comparison simply never runs for a fixture the crate rejects.

  Note: the `"Infinity"`/`"-Infinity"`/`"NaN"` encoding for *legitimately non-finite* float inputs (e.g. `remote_connect_timeout_secs: "inf"`/`"nan"`, which are small, in-range strings that conda accepts as float specials) is **retained** and is not a divergence — those fixtures are parsed and adapted normally.

- **A2 — Root-shape and other conda crash bugs become clean errors**: Where real conda crashes with an unhandled exception (list/scalar root; non-empty array/object fed to a scalar-typed key; `null`/word/hex-literal fed to certain boolish keys; `local_repodata_ttl` hex-literal), the crate returns a typed error instead. The harness's accept/reject verdict is unaffected (still `invalid/`), so this needs no divergence-list entry; the crate simply never reproduces conda's exact crash text (which the harness does not assert on).

- **A3 — `ssl_verify` filesystem check is opt-in, and conformance opts in**: conda validates a non-boolish, non-`truststore` `ssl_verify` string by asking the local filesystem whether that path exists. Two consequences, resolved as follows (FR-024):
  - **The crate's default is side-effect-free**: no filesystem access, so an arbitrary path string is accepted unverified. This is what makes parsing a pure function of its input — the same `.condarc` yields the same result on a laptop, in CI, and in a container, which is the property FR-002/Constitution IX require.
  - **The existence check is available as an explicit, per-call option**, so a caller that wants conda's exact runtime semantics (GEN-23) gets them, and a caller that wants hermeticity keeps it, from the same compiled library.
  - **Conformance runs with the check enabled.** The corpus was generated from real conda, which always checks, so `invalid/ssl_verify_passthrough_reject_string_arbitrary_word.json` (`"banana"`), `..._reject_string_nonexistent_path.json`, `..._reject_truststore_wrong_case.json` and `..._reject_string_hex_literal_shaped.json` are rejections *precisely because* those strings are not existing paths — with the check off the crate would accept all four. The accepting side of the branch is covered by `valid/ssl_verify_passthrough_accept_existing_path_parent_dir.json` (`".."`) and `..._accept_existing_path_dot_slash.json` (`"./"`), added in this round for exactly that purpose: both are relative paths that exist on every platform, so the check stays deterministic. (Note `valid/..._accept_existing_path_current_dir.json` (`"."`) is *not* a path case despite its name: `boolify`'s `.replace(".", "", 1)` probe reduces `"."` to `""`, a false token, so conda loads it as the boolean `false` — see its `expected/` fixture. The generator's docstring and `docs/condarc_research.md` §2.2 were corrected accordingly.) No fixture moves and no divergence-list entries are needed for A3.
  - The openapi checker cannot perform a filesystem check at all, which remains a documented gap inside `docs/condarc_openapi.json`'s `CondaSslVerify` component (it encodes bool/boolish/`truststore`/`"."` only) — a schema limitation, not a crate divergence.

- **A4 — ASCII-only numeric parsing**: Python's `int()`/`float()` accept **any** Unicode decimal digit, so conda accepts e.g. `default_python: "٣.٩"` (Arabic-Indic 3.9) and `repodata_threads: "٣"`. Verified empirically against conda 26.5.3. Rust's `i64`/`f64` parsers are ASCII-only, and the crate does not carry a Unicode `Nd`→value table to emulate Python here. **Decision**: numeric coercion (and the `default_python` float-range check) accepts ASCII digits `0-9` only; a non-ASCII digit is a coercion error. This affects no committed fixture: the corpus deliberately contains **no** Unicode-digit numeral fixture, precisely so that this divergence needs no divergence-list entry (see `scripts/generate_default_python_condarc_fixtures.py`'s module docstring, which records the verified conda behavior for the record). If this divergence ever needs to be exercised, the A1 divergence-list mechanism above is how to do it.

- **A5 — No stack-depth/size guard against pathologically deep or large documents**: Python's recursive YAML/JSON loaders raise a catchable `RecursionError` once CPython's interpreter recursion limit is hit, so conda's own error handling can in principle surface that as an ordinary exception. Rust's recursive-descent YAML parsing has no equivalent built-in guard: a document with enough nesting (or, per the Overview, fed to the crate from an untrusted source such as an HTTP response) can exhaust the OS thread's call stack and abort the process instead of returning an `Err`. This is not a Rust `panic!` — it is not raised via `.unwrap()`/`.expect()`, and it cannot be intercepted by `std::panic::catch_unwind` — so avoiding those idioms in library code does not prevent it. **Decision**: the crate does not implement an application-level nesting-depth or document-size limit, and FR-004's "MUST NOT panic on any input" is understood to be scoped to inputs that do not exhaust the process stack (see the exception noted on FR-004 and in Edge Cases). This is deliberately proportionate to the crate's intended use — a small, locally-authored `.condarc` (GEN-23 reads it from the user's home directory) — rather than a hardened parser for arbitrary adversarial network payloads; the "from an HTTP response" phrasing in the Overview illustrates the string-based API's source-agnosticism, not a requirement to defend against a hostile sender. No conformance fixture exercises this shape, since it is not part of conda's accept/reject oracle. No divergence-list entry is needed (A1's mechanism is for fixtures the crate rejects that conda accepts; this is an unfixtured input shape, not a re-verdicted one).

### Unknown-key handling

**Decision**: unknown top-level keys are silently accepted (conda-lenient), matching real conda and GEN-23's parent acceptance criteria ("ignore unrelated `.condarc` keys without failing"). No conformance fixture exercises an unknown top-level key, so this is consistent with the existing accept/reject fixtures, and `docs/condarc_openapi.json` already declares `additionalProperties: true`, so the openapi checker agrees with the crate as-is (no schema change needed). How unknown keys are surfaced to callers (dropped vs. retained in a raw/extra map) is deferred to planning per FR-036.
