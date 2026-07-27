# Contract: Conformance Adapter (`Config` → `expected/*.json`)

**Status**: Phase 1 design contract. **Test-only** — per research R9, this adapter is
conformance-harness support code, not a feature of the published `condarc` crate. It lives at
`tests/support/adapter.rs`, i.e. inside the *same test target* as the harness that calls it
(`tests/condarc_conformance.rs`, via `mod support;`). It consumes the library's own public `Config`
from outside the crate exactly as any other downstream caller would, and is invoked only by that
harness's `Crate` checker. It deliberately does **not** live under `crates/condarc/tests/`: a file in
another crate's `tests/` directory is unreachable from this harness (raised in PR review, confirmed
empirically — research R10). It is documented here, separately from `public-api.md`, precisely because
it is *not* part of the API a real caller (GEN-23, future publication) uses — see `public-api.md`'s
"Adapter" section for the pointer back.

## Signature

```rust
/// Render a parsed `.condarc` [`condarc::Config`] into the same JSON shape
/// as `conformance/condarc/expected/*.json`, for conformance comparison
/// only. Not part of the published crate's public API (research R9).
pub fn to_expected_json(config: &condarc::Config) -> serde_json::Value;
```

## Behavior (FR-040 / FR-041)

1. **Present-only**: emits a JSON object key **iff** the corresponding `Config` field is
   `Some(_)` (present in the original document). A field that is `None` (absent) is never emitted
   — matching `expected/*.json`, which likewise records only present keys.
2. **Canonical loader-attribute naming**: object keys use conda's canonical internal setting name
   (aliases resolved), matching every key already used as the `canonical` field in
   `data-model.md` §5's `CATALOG` (e.g. `channels`, `always_yes`, `ssl_verify`, `solver`). That table
   was cross-checked mechanically against a live conda 26.5.3 `Context`, which is how the one
   discrepancy PR review spotted got settled: the canonical name is **`auto_activate`**
   (`ParameterLoader.name`), and `auto_activate_base` — the spelling `settings.rst` documents — is its
   *alias*. `expected/*.json` records `auto_activate`, so that is what the adapter emits.
3. **Value encoding**:
   - `bool` / nullable-`bool` → JSON `true` / `false` / `null`.
   - enum (`ChannelPriority`, `PathConflict`, `SafetyChecks`, `SatSolver`) → canonical lowercase
     value string, e.g. `ChannelPriority::Strict` → `"strict"`.
   - `i64` → JSON number (guaranteed in-range by A1 — the adapter never has to encode an
     over-bound value, since out-of-range numerals are rejected at parse time).
   - `f64` → JSON number, **except** non-finite values: `+inf` → `"Infinity"`, `-inf` →
     `"-Infinity"`, `NaN` → `"NaN"` (JSON strings, per research §8 item 15 / FR-041). This
     applies uniformly whether the non-finite value came from an explicitly-spelled
     `inf`/`infinity`/`nan` token or from an ordinary numeral that overflowed `f64`'s finite
     range (e.g. `"1e400"`) — unlike `i64`, `f64` magnitude overflow is not rejected at parse
     time (A1), so this is the one float-specific encoding rule that *does* have live inputs to
     handle, in contrast to the `i64` row above.
   - `String` / nullable-`String` → JSON string / `null`.
   - `Vec<String>` → JSON array of strings.
   - `BTreeMap<String, String>` / `BTreeMap<String, Option<String>>` → JSON object (map values
     `null` where the inner `Option` is `None`).
   - `BTreeMap<String, Vec<String>>` (`custom_multichannels`) → JSON object of arrays.
   - `Vec<ChannelSetting>` → JSON array of string→string objects.
   - `BoolOrInt` → JSON `true`/`false` or JSON number, matching whichever variant was coerced.
   - `SslVerify` → JSON `true`/`false`, the string `"truststore"`, or the path string.
   - `Vec<ListField>` → JSON array of the closed-vocabulary strings (exact case, per §5.9).
   - `Config::extra` is **never** emitted (FR-040) — the adapter's whole purpose is the *known*
     catalog; unrecognized keys have no place in `expected/*.json` by construction.

## Comparison semantics (FR-040, exact)

For each `valid/*.json` fixture (re-interpreted as YAML input):

```rust
// the harness's `Crate` checker opts into conda's ssl_verify FS check (spec A3)
let cfg = condarc::parse_with_options(
    &fixture_yaml,
    condarc::ParseOptions { ssl_verify_fs_check: true, ..Default::default() },
)?;
let adapted = to_expected_json(&cfg);
let expected: serde_json::Value = serde_json::from_str(&fixture_expected_json)?;
assert_eq!(adapted, expected, "adapted representation must equal expected/ exactly");
```

**Exact object equality, not a subset check.** PR review pointed out that a subset comparison only
proves the expected keys are present and correct — it can't catch an adapter emitting keys it
shouldn't, which is precisely the "present only" rule FR-040 exists to enforce. So the comparison
asserts full equality: a missing key, an extra key, a renamed key, or a wrong value all fail.

This is achievable because the two sides are constructed to agree exactly:

- `expected/*.json` contains one entry per top-level key of the source document, canonicalized (see
  `scripts/generate_zzz_condarc_expected_fixtures.py`), and the adapter emits one entry per **present**
  setting, canonicalized identically. Verified mechanically across all 388 object-rooted `valid/`
  fixtures: the canonical names of a document's keys are exactly the corresponding `expected/`
  fixture's key set.
- Absent settings are `None` and are never emitted (FR-038 — there is no defaulting layer that could
  invent one).
- `Config::extra` is never emitted, so an unrecognized key cannot add a spurious entry. The flip side:
  a *future* fixture that sets a conda parameter missing from the catalog (e.g. the CLI-flag
  parameters listed in `data-model.md` §5) would fail this comparison — which is the intended signal to
  add the setting to the catalog, not to weaken the comparison.
- `valid/null_root.json` has no `expected/` counterpart (the generator skips non-object roots), so the
  adapter comparison is skipped for it, exactly as it already is for the conda checker.
- Fixtures listed as `Crate`-checker divergences (spec A1's bignum cases) are rejected by the crate, so
  no adapter comparison happens for them at all.

## Non-goals

- **Not round-trip**: does not reconstruct YAML, comments, or key ordering (explicitly out of
  scope for GEN-36).
- **Not a public crate feature**: no real caller consumes `expected/*.json`'s exact shape; real
  callers read typed `Config` fields directly (see `public-api.md` §"Using the typed values").
- **Not versioned independently**: because it is test-only code, it has no SemVer contract of its
  own; it changes freely alongside the fixture corpus and the harness.
