# Contract: Conformance Adapter (`Config` → `expected/*.json`)

**Status**: Phase 1 design contract. **Test-only** — per research R9, this adapter is
conformance-test support code, not a feature of the published `condarc` crate. It lives at
`crates/condarc/tests/conformance_support.rs` (or a sibling test-support module), consumes the
library's own public `Config` from outside the crate exactly as any other downstream caller would,
and is invoked only by `tests/condarc_conformance.rs`'s `Crate` checker. It is documented here,
separately from `public-api.md`, precisely because it is *not* part of the API a real caller
(GEN-23, future publication) uses — see `public-api.md`'s "Adapter" section for the pointer back.

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
   `data-model.md` §5's `CATALOG` (e.g. `channels`, `always_yes`, `ssl_verify`, `solver`).
3. **Value encoding**:
   - `bool` / nullable-`bool` → JSON `true` / `false` / `null`.
   - enum (`ChannelPriority`, `PathConflict`, `SafetyChecks`, `SatSolver`) → canonical lowercase
     value string, e.g. `ChannelPriority::Strict` → `"strict"`.
   - `i64` → JSON number (guaranteed in-range by A1 — the adapter never has to encode an
     over-bound value, since out-of-range numerals are rejected at parse time).
   - `f64` → JSON number, **except** non-finite values: `+inf` → `"Infinity"`, `-inf` →
     `"-Infinity"`, `NaN` → `"NaN"` (JSON strings, per research §8 item 15 / FR-041).
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

## Comparison semantics (FR-040, subset-based)

For each `valid/*.json` fixture (re-interpreted as YAML input):

```rust
let cfg = condarc::parse(&fixture_yaml)?;
let adapted = to_expected_json(&cfg);
let expected: serde_json::Value = serde_json::from_str(&fixture_expected_json)?;
for (key, expected_value) in expected.as_object().unwrap() {
    assert_eq!(adapted.get(key), Some(expected_value), "mismatch at key {key}");
}
```

Subset (not exact-object-equality) comparison: only keys **present in `expected/`** are asserted
against the adapted output. This keeps the harness robust even if the crate later exercises the
FR-039 boolean-default option for some setting (an off-state boolean modeled as `false` rather
than `Option<bool>`), since that choice never changes what the adapter emits for an *absent*
setting (FR-040 still applies) — it only ever affects the value for a setting that was **present**,
which is exactly what subset comparison already checks.

## Non-goals

- **Not round-trip**: does not reconstruct YAML, comments, or key ordering (explicitly out of
  scope for GEN-36).
- **Not a public crate feature**: no real caller consumes `expected/*.json`'s exact shape; real
  callers read typed `Config` fields directly (see `public-api.md` §"Using the typed values").
- **Not versioned independently**: because it is test-only code, it has no SemVer contract of its
  own; it changes freely alongside the fixture corpus and the harness.
