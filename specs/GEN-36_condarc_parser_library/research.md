# Phase 0 Research: `.condarc` Parser Library (GEN-36)

This document resolves every `NEEDS CLARIFICATION` from the plan's Technical Context and records
the key technical decisions backing the design. Each entry follows Decision / Rationale /
Alternatives considered. The behavioral oracle throughout is the committed corpus
(`conformance/condarc/{valid,invalid,expected}`) and `docs/condarc_research.md`, not conda source.

---

## R1 — YAML parser dependency

**Decision**: Use **`yaml-rust2`** as the YAML frontend. Parse into `yaml_rust2::Yaml`, then lower
it into the crate's own internal raw-value enum (see data-model.md `RawValue`) that records the
resolved scalar type (bool / int / float / null / string) so conda's type-vs-string distinction
(e.g. bare `true`/`7` vs. quoted `"true"`/`"7"`) is reproducible.

**Ecosystem state**: there is no single library the Rust ecosystem has converged on for YAML, and
the space has been unusually volatile over the last two years:
- `serde_yaml` (dtolnay) was the long-standing default (347M lifetime downloads) but is now formally
  end-of-life: its final release is version-tagged `0.9.34+deprecated`, and its GitHub repo is
  archived (no new issues can be filed). It is backed by `unsafe-libyaml`, a hand-transpiled-from-C
  parser, so it also carries real `unsafe` exposure.
- Its deprecation triggered a wave of forks intended as drop-in successors: `serde_yaml_ng`,
  `serde-norway`, `serde_yml`. None became a viable, maintained replacement — `serde_yml` gained
  early adoption but was low-quality and is now itself archived; `serde_yaml_ng` has not shipped a
  release in over two years; `serde-norway` has negligible adoption. **No maintained serde-native
  YAML crate exists that the ecosystem has actually converged on.**
- The forward-looking answer discussed in the community is the `saphyr` project family (`saphyr`,
  `saphyr-parser`, and a third-party `serde-saphyr`), evaluated and rejected below.

**Candidates considered, in elimination order**:

1. **`serde-saphyr` — rejected as too immature.** A *third-party* serde deserializer built on top of
   `saphyr`, not an official part of the saphyr project (saphyr's own README lists `saphyr-serde` as
   "soon-to-be," i.e. not yet shipped). It has 30 `0.0.x` releases and a single `1.0.0-rc.1` cut
   very recently, with no stable `1.0` yet. Depending on an unofficial pre-1.0 wrapper around a
   library that is itself pre-1.0 stacks two layers of instability we don't need to take on.

2. **`saphyr` — rejected: inconsistent maintenance and materially lower adoption than `yaml-rust2`.**
   `saphyr` is a **fork of `yaml-rust2`** (itself a fork of the original 2015 `yaml-rust`), and is
   maintained by the *same two people* who maintain `yaml-rust2` — this is one small team offering a
   stable build and an experimental build side by side, not two competing communities. Evidence
   against depending on the experimental one:
   - Its release history shows a 13-month gap with zero releases, followed by five patch releases in
     a single week; that week was one maintainer clearing a backlog of external contributor PRs that
     had each sat unreviewed for 6–7 months. That is not a sustained development cadence, and implies
     a multi-month turnaround should we ever need a fix from anyone but the primary maintainer.
   - crates.io lists on the order of 55 reverse dependencies, almost entirely small/hobby crates — no
     widely used production Rust project depends on it directly today.
   - The maintainers describe its API as intentionally unstable and subject to breaking changes
     release-to-release, in explicit contrast to `yaml-rust2`.
   - Its MSRV (1.85) is meaningfully newer than `yaml-rust2`'s, a real constraint for a standalone,
     publishable library (FR-003) whose downstream consumers we don't control.

3. **`yaml-rust2` — selected.** Same lineage as `saphyr`, same two maintainers, but deliberately kept
   API-stable: the maintainers' stated intent is that `yaml-rust2` receives fixes and minor
   improvements only, with new feature development happening on `saphyr` instead — exactly the
   stability posture we want in a dependency we don't intend to churn on. Concretely:
   - It has on the order of 134 reverse dependencies, including `config` (`rust-cli/config-rs`) — a
     widely used, actively maintained, multi-format configuration library for Rust CLI tools — which
     depends on `yaml-rust2` for YAML support as of a release published within the last month. This
     is the closest real-world precedent to what GEN-36 is building, and it made the same choice we
     are making here.
   - `yaml_rust2::Yaml` already separates scalars by resolved type (`Boolean`/`Integer`/`Real`/
     `String`/`Null`/...), which is sufficient to reproduce conda's type-vs-string distinction. It
     does not need scalar-style (quote-style) fidelity beyond that, because round-tripping is
     explicitly out of scope for GEN-36.
   - Pure safe Rust, no `unsafe`. MSRV 1.65, more permissive than `saphyr`'s 1.85. No C dependency,
     keeping the Windows/macOS/Linux build simple (Constitution VII).
   - MIT/Apache-2.0 dual-licensed, already compatible with `deny.toml`'s allow list. **Action for the
     implementation phase**: add the crate, run `cargo deny check` + `cargo audit`, and record the
     resolved version in the committed `Cargo.lock`.

**Revisit condition**: if a serde-native YAML deserializer reaches a genuine stable release *and*
gains broad, verifiable adoption (multiple non-trivial production crates depending on it, not just
community discussion) — most plausibly `saphyr-serde` once shipped as an official first-party crate,
or `serde-saphyr` after it clears 1.0 and accumulates real reverse dependencies — this decision
should be revisited. No such option exists today; `yaml-rust2` is the only candidate with both a
stable API and a credible production track record.

**Alternatives considered (other than the saphyr family)**:
- `serde_yaml` and its forks (`serde_yaml_ng`, `serde-norway`, `serde_yml`) — **rejected**, see
  ecosystem state above.
- Hand-rolled YAML parser — **rejected**: reinventing a YAML scanner is out of scope, error-prone,
  and violates DRY; the fixtures include unicode, control chars, null bytes, and whitespace edge
  cases (research §8) that a mature parser already handles.

**Consequence for the design**: `parse.rs` depends only on `yaml-rust2` and immediately lowers to
the crate's own `RawValue`; no other module touches the YAML library, and the YAML frontend never
appears in `condarc`'s public API (types, errors, and re-exports are all crate-owned). It is a pure
implementation detail, so a future parser swap under the revisit condition above is a single-module
change with zero breakage for callers.

### Does `yaml-rust2` work *with* serde? (explicit — this is a common misread)

**No, and it deliberately does not need to.** `yaml-rust2` is **not** a serde data format: it does
not implement `serde::Deserializer`, and `Config` is **not** built via `#[derive(Deserialize)]`. The
YAML → `Config` path contains **no serde at all**. serde appears only on the *output* and *escape
hatch* sides. Full flow:

```
&str (YAML text)
  │  yaml-rust2                          (NOT serde; yaml-rust2 has no Deserializer impl)
  ▼
yaml_rust2::Yaml value tree              (resolved scalar type: Boolean/Integer/Real/String/Null/...)
  │  parse.rs: lower(&Yaml) -> RawValue          (hand-written match; no serde)
  ▼
RawValue  (crate-private, R1/data-model §1)
  │  catalog.rs + coerce/*: conda coercion          (hand-written boolify/numberify/enum/… — NOT serde)
  ▼
Config  (typed struct, built field-by-field imperatively; NO Deserialize impl)
  │
  ├─ error.rs:   ValidationReport: #[derive(Serialize)]           (out-only, JSON contract)
  └─ Config::extra: HashMap<String, serde_json::Value>            (R2 escape hatch; caller may serde_json::from_value)

(A third, hand-written `to_expected_json(&Config) -> serde_json::Value` conversion exists, but it is
conformance-test support code, not a crate module — see R9. It consumes `Config` from outside the
crate, the same way any other downstream caller would, so it is omitted from this crate-internal
diagram.)
```

**Why serde is *not* used to build `Config`**: conda's coercion (`"yes"` → `true`, `str(7)` → `"7"`,
value-or-name enum lookup, `i64`-range *rejection*, `local_repodata_ttl`'s narrower vocabulary,
alias resolution, multi-error accumulation) is bespoke logic serde's derive/`Deserializer` model has
no hook for. A serde-driven parse would fight the requirements at every turn (serde stops at the
first error — conflicts with FR-030/031 accumulation; serde can't express "coerce `7` to the string
`"7"`"; serde has already collapsed the type-vs-string scalar distinction we depend on). So the parse
path is a plain hand-written pipeline over `RawValue`, and serde is confined to *serialization* out
(the error report, plus the test-only `expected/`-JSON adapter — R9) and the `extra` caller escape
hatch — see R2 for why serde *is* fair game on that one tail and for the `RawValue` →
`serde_json::Value` lowering convention it relies on.

---

## R2 — Unknown/custom-key handling: general Rust idiom, then the yaml-rust2 tie-in (FR-036)

**Question**: FR-036 defers to planning *how* unknown top-level keys are surfaced (dropped vs.
retained), and — the deeper question the user raised — **can/should the caller participate in
deserializing values it cares about that the crate's fixed catalog doesn't model**, either into a
structure of its own choosing or generically into a map? An earlier draft of data-model.md
prescribed a public field `unknown_keys: HashMap<String, RawValue>`. That is under-considered on
two axes: (a) it leaks the crate's internal YAML-frontend type (`RawValue`, tied to `yaml-rust2`)
across the public API, coupling every consumer to the parser choice and breaking R1's "parser swap
is a single-module change" guarantee; (b) it only answers *retention*, not *caller participation*.
This entry first surveys how the Rust ecosystem, in general — independent of YAML or `yaml-rust2` —
lets a caller be part of deserializing data a fixed schema doesn't fully cover, then picks a shape,
then ties that shape to the concrete `yaml-rust2`/`RawValue` pipeline chosen in R1.

### Survey — general Rust idioms for "the schema doesn't cover everything"

| Pattern | How the caller participates | Trade-off |
|---|---|---|
| **serde `#[serde(flatten)] extra: HashMap<String, Value>`** | Unmodeled fields collect into a caller-visible map of a *neutral* value type (`serde_json::Value`, `toml::Value`). Caller re-deserializes what it wants via `serde_json::from_value`. | Simple, ubiquitous. Neutral wire type, not the parser's own AST — no frontend leak. Two explicit steps (collect, then interpret). |
| **Schema-driven codegen `additionalProperties`** (OpenAPI generators, JSON Schema tooling) | Generated structs expose `additional_properties: HashMap<String, serde_json::Value>` / `Option<Map<String, Value>>` for anything outside the declared schema. | Same shape as `flatten`, arrived at independently by a different part of the ecosystem — corroborates it as *the* convention for "declared schema + open tail," not a one-off idiom. |
| **Kubernetes API machinery** (`kube-rs`, `k8s-openapi`; CRDs with `x-kubernetes-preserve-unknown-fields`) | Unknown/unstructured portions of a resource are carried as `serde_json::Value` (e.g. `DynamicObject.data`), sitting alongside strongly-typed known fields (`ObjectMeta`, etc.). | Same convention again, at production scale, for the same "known fields typed, open tail dynamic" split this crate needs. |
| **serde `#[serde(other)]`** (enum) | Routes *all* unknown variants to one catch-all — good for "is this a known key?" but discards the value. | Loses the value entirely; only fits tagged enums, not an open config map. |
| **serde `deny_unknown_fields`** | The opposite: unknown keys are a hard error. | Wrong for conda's silent-accept requirement (FR-036). |
| **`serde::de::DeserializeSeed` / custom `Visitor`** | Caller injects *stateful* logic into the deserialize pass itself, deciding per-key how to interpret a value using runtime context the type alone can't carry. | Maximum power; caller is a first-class part of deserialization. Advanced, verbose, and would force `condarc` to expose serde-`Deserializer` internals — heavy for a config parser. |
| **`config` / `figment` crates** | Merge many sources into a dynamic `Value` tree; caller calls `.get::<T>("key")` / `try_deserialize::<MyStruct>()` on demand. Unknown keys just live in the tree until asked for. | Fully dynamic; caller extracts typed views lazily. Abandons the compile-time typed-struct model (Constitution VI) for the *known* settings too, which this crate doesn't want. |
| **Generic catch-all type parameter with a default** (`struct Container<Extra = serde_json::Value> { known: T, #[serde(flatten)] extra: Extra }`) | Caller picks its own `Extra: DeserializeOwned` type at the call site (`Container<MyExtras>`) and gets it deserialized in the *same* pass — one call instead of collect-then-reinterpret. Defaults to a neutral `Value`/map for the fully-dynamic case. | Genuinely more ergonomic for "I already know the struct I want," but the type parameter infects every signature that touches the container (`Config<E>`, `parse::<E>(path)`, every helper/error type) — a much larger public-API and SemVer surface for a monomorphic library that otherwise wants one plain `Config` type. |
| **Type-erased extension bag** (`http::Extensions`, `anymap`/`TypeMap`) | Caller stores/retrieves already-*constructed* typed Rust values by `TypeId`. | Wrong problem: designed for attaching pre-built typed state at runtime (request-scoped data), not for deserializing untyped wire data the schema didn't cover. Doesn't fit a one-shot parse-from-file API. |
| **Wire-format unknown-field retention** (protobuf/`prost`'s `UnknownFieldSet`, raw byte preservation for forward-compat re-serialization) | Caller can round-trip fields it doesn't understand back onto the wire unchanged. | Solves round-tripping, which this ticket explicitly does not require; would mean preserving raw YAML fragments for zero current consumer need. |
| **Raw whole-document `Value` alongside the typed struct** (`toml`/`serde_json::Value` escape hatch) | Caller can walk *anything*, known or unknown. | Duplicates the typed view and re-introduces the broad dynamically-typed surface the constitution warns against if overused — worse than scoping the dynamic part to just the unmodeled tail. |

**Cross-cutting lesson**: three independent corners of the ecosystem (hand-written `flatten`
configs, generated OpenAPI clients, and Kubernetes' API machinery) converge on the same shape for
exactly this "typed known fields + open tail" problem: collect unmodeled keys into a **neutral,
serde-friendly value type**, and let the caller opt into interpreting them with an **ordinary second
deserialize step over the whole tail at once** (not one call per key — see the worked example below
for why that distinction matters). The generic-type-parameter variant is a real, if less common,
refinement of the same idea that removes the second step at the cost of making the whole container
generic — a reasonable trade only when the container's simplicity isn't already a design goal.
Reserve `DeserializeSeed`/visitor-level participation, and full dynamic-tree access
(`config`/`figment`), for libraries that genuinely need stateful or fully-dynamic parsing, neither
of which applies here: `condarc`'s catalog is fixed and authoritative for *known* settings, so
caller participation is only needed for the *unknown* tail.

### Why serde is available for the unknown tail even though R1 keeps it off the known-field path

R1 rejects a serde-driven parse for *known* settings because conda's coercion is bespoke and
multi-error accumulation needs every field evaluated independently. Neither constraint applies to
*unknown* keys: FR-036 requires them to be accepted and ignored, never coerced — there is no
bespoke truth table to reproduce for a key the catalog doesn't recognize, and a caller-side
`serde_json::from_value::<T>` either succeeds or fails as one ordinary, independent step per caller
call, outside the crate's own multi-error report. So an ordinary serde second pass is fair game for
exactly this tail, which is what makes the table above's dominant idiom directly usable.

### From the caller's perspective: a worked example, pattern by pattern

**Motivation, and a scope correction**: FR-009 excludes conda-build's four `.condarc` keys —
`bld_path`, `croot`, `anaconda_upload` (alias `binstar_upload`), and `conda_build` — from this
crate's catalog entirely (docs/condarc_research.md §4 footnote 5); they configure conda-build, not
conda. But if `condarc` were embedded in a Rust rewrite of `conda-build` itself — a realistic future
consumer, not a hypothetical one — *that* caller needs exactly those four keys, including the nested
`conda_build:` mapping conda-build itself defines (real sub-keys: `root-dir`, `pkg_format`,
`zstd_compression_level`, `no_lock`, `filename_hashing`, …). Critically, **the caller knows all four
key names, and their shape, at compile time** — this is not "some generic bag of unforeseen keys,"
it's "my app requires these specific settings, which map onto a struct I've already written."
Anything present in the document that the caller's struct doesn't declare is simply irrelevant and
gets dropped — ordinary serde behavior, not a special case. Every pattern below is shown solving
*that* task, and each is judged on whether it gets the caller from "parsed `.condarc`" to "my typed
`CondaBuildConfig`" in essentially one step, not N per-field steps. Shared inputs for every sketch:

```yaml
# ~/.condarc
channels: [defaults, conda-forge]     # in-catalog — becomes Config::channels
channel_priority: strict              # in-catalog — becomes Config::channel_priority

croot: /home/user/conda-bld           # conda-build's, not in condarc's catalog (FR-009)
bld_path: /home/user/conda-bld/pkgs   # conda-build's, not in condarc's catalog (FR-009)
anaconda_upload: false                # conda-build's, not in condarc's catalog (FR-009)
conda_build:                          # conda-build's, not in condarc's catalog (FR-009)
  root-dir: /home/user/conda-bld
  pkg_format: '2'
  zstd_compression_level: 19
  no_lock: true
```

```rust
// Caller-owned types. `condarc` has never heard of either of these —
// conda-build's Rust rewrite defines them for itself, once, up front.
// Every `Option<_>` field defaults to `None` if the key is absent from the
// document — ordinary serde behavior for `Option<T>`, no attribute needed.
#[derive(serde::Deserialize, Debug, Default)]
struct CondaBuildSettings {
    #[serde(rename = "root-dir")]
    root_dir: Option<String>,
    pkg_format: Option<String>,
    zstd_compression_level: Option<i64>,
    #[serde(default)]
    no_lock: bool,
}

#[derive(serde::Deserialize, Debug, Default)]
struct CondaBuildConfig {
    croot: Option<String>,
    bld_path: Option<String>,
    anaconda_upload: Option<bool>,
    conda_build: Option<CondaBuildSettings>,
}
```

Four survey rows get no sketch below because they don't fit this problem at all, not merely because
they're less convenient: `#[serde(other)]` only routes unknown *enum variants*, and there is no enum
here — `.condarc`'s open tail is map keys, not a tagged union. `deny_unknown_fields` inverts the
requirement outright (turns unknown keys into a hard parse error, contradicting FR-036). A
type-erased extension bag (`http::Extensions`/`anymap`) stores already-*constructed* Rust values
keyed by `TypeId` — the caller would have to hand `condarc` a finished `CondaBuildSettings` *before*
parsing even happens, which is backwards; nothing in that mechanism actually deserializes YAML.
Wire-format unknown-field retention (protobuf/`prost`-style opaque-byte preservation) exists to
round-trip data unchanged, which this ticket explicitly does not require.

**Pattern — neutral value map, deserialized as a whole** (`extra: HashMap<String,
serde_json::Value>`, the `flatten`/`additionalProperties`/Kubernetes-`DynamicObject` shape, plus one
convenience method that treats the *entire* map as a single JSON object and deserializes it into the
caller's struct in one call — this is the fix for the per-field destructuring shown in the previous
draft, which was a genuinely bad API and is discarded here):

```rust
// condarc's public API
pub struct Config {
    pub channels: Option<Vec<String>>,
    pub channel_priority: Option<ChannelPriority>,
    // … ~120 more known, typed fields …
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}
impl Config {
    /// Deserialize the *entire* unknown-key tail into a caller-chosen struct
    /// `T` in one call. Fields `T` doesn't declare are dropped (ordinary
    /// serde, no `deny_unknown_fields`); fields `T` declares that are absent
    /// from the document deserialize however `T` handles a missing field
    /// (typically `None`, for `Option<_>`).
    pub fn extra_as<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        let obj = serde_json::Value::Object(
            self.extra.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        );
        serde_json::from_value(obj)
    }
}
pub fn parse(path: &std::path::Path) -> Result<Config, condarc::ValidationReport>;
```

```rust
// caller (conda-build rewrite) — one call, whole struct, done
let cfg = condarc::parse(condarc_path)?;
let build: CondaBuildConfig = cfg.extra_as()?;
```

A caller that genuinely wants only *one* key, without declaring a struct for it, still can —
`cfg.extra.get("conda_build").cloned().map(serde_json::from_value::<CondaBuildSettings>).transpose()?`
— but that's the narrow case, not the common one this ticket's caller (a conda-build rewrite) has;
`extra_as` above is the one-call answer to "my app requires these specific settings."

**Pattern — generic catch-all type parameter with a default** (the caller's struct is deserialized
inline, as part of the same parse pass, rather than through a caller-driven second call):

```rust
// condarc's public API
pub struct Config<Extra = std::collections::HashMap<String, serde_json::Value>> {
    pub channels: Option<Vec<String>>,
    pub channel_priority: Option<ChannelPriority>,
    // … known fields …
    pub extra: Extra,
}
pub fn parse<Extra: serde::de::DeserializeOwned>(
    path: &std::path::Path,
) -> Result<Config<Extra>, condarc::ValidationReport>;
```

```rust
// caller — one call, whole struct, done (same outcome as extra_as above)
let cfg: condarc::Config<CondaBuildConfig> = condarc::parse(condarc_path)?;
let build = cfg.extra;

// the generic default still makes the "I don't have a struct" case free:
let generic_cfg = condarc::parse(condarc_path)?; // Config<HashMap<String, serde_json::Value>>
```

Now that the scope is "caller has a known struct, wants one call," this pattern and `extra_as` reach
the *identical* caller-visible outcome — the earlier argument that the generic parameter is "more
ergonomic" no longer holds, because `extra_as` closes that gap without a generic parameter. The
remaining difference is purely about who pays for the open tail: here, every signature that touches
`Config` (`parse`, any future helper, error types that embed a `Config`) becomes generic, a
permanent, public-API/SemVer cost, in exchange for a call site that's `cfg.extra` instead of
`cfg.extra_as()?` — one non-generic method call. The other genuine difference is runtime key
discovery: `Config<Extra>::extra` *is* `Extra` directly, so a caller can no longer ask "what
unrecognized keys are actually present?" the way `Config::extra.keys()` allows — irrelevant for the
"known struct" scope here, but a real capability loss for a different caller (e.g. a linter that
wants to warn about typo'd keys).

**Pattern — `DeserializeSeed`/hook-style builder registration** (heaviest option: the caller
registers, *before* parsing, how to interpret specific keys; the crate applies those hooks during
its own pass and hands back type-erased results):

```rust
// condarc's public API
pub struct Builder {
    hooks: std::collections::HashMap<
        String,
        Box<dyn Fn(&serde_json::Value) -> Result<Box<dyn std::any::Any>, serde_json::Error>>,
    >,
}
impl Builder {
    pub fn new() -> Self { Self { hooks: Default::default() } }
    pub fn with_extra<T>(mut self, key: &str) -> Self
    where
        T: serde::de::DeserializeOwned + 'static,
    {
        self.hooks.insert(key.to_string(), Box::new(|v| {
            serde_json::from_value::<T>(v.clone()).map(|t| Box::new(t) as Box<dyn std::any::Any>)
        }));
        self
    }
    pub fn parse(self, path: &std::path::Path) -> Result<Config, condarc::ValidationReport> { /* … */ }
}
impl Config {
    pub fn typed_extra<T: 'static>(&self, key: &str) -> Option<&T> {
        self.extra_typed.get(key)?.downcast_ref::<T>()
    }
}
```

```rust
// caller — still fundamentally per-key, on *both* ends
let cfg = condarc::Builder::new()
    .with_extra::<String>("croot")
    .with_extra::<String>("bld_path")
    .with_extra::<bool>("anaconda_upload")
    .with_extra::<CondaBuildSettings>("conda_build")
    .parse(condarc_path)?;

let croot: Option<&String> = cfg.typed_extra("croot");
let build: Option<&CondaBuildSettings> = cfg.typed_extra("conda_build");
```

Given the clarified scope, this pattern fits worse than the two above, not just "more heavily": it
never gets the caller to a single `CondaBuildConfig` value at all — the caller still assembles one
by hand from N separate `typed_extra` calls (the exact per-field shape being discarded elsewhere in
this section), just with the boilerplate moved to registration time instead of extraction time.
Registration must also happen *before* parsing (a key not registered up front can't be retrieved
later, unlike the two patterns above where the whole tail is always available); and retrieval is a
runtime, string-keyed downcast (`typed_extra::<T>("key")` with a mismatched `T` compiles fine and
just returns `None` — no compile-time link between what was registered and what's retrieved).

**Pattern — whole-document raw `Value` alongside the typed struct**:

```rust
// condarc's public API
pub struct Config {
    pub channels: Option<Vec<String>>,
    // … known fields — NO `extra` field at all …
}
pub fn parse(
    path: &std::path::Path,
) -> Result<(Config, serde_json::Value), condarc::ValidationReport>;
```

```rust
// caller — one call, whole struct, done (the raw document is one big
// serde_json::Value::Object already, so an ordinary from_value works)
let (cfg, raw) = condarc::parse(condarc_path)?;
let build: CondaBuildConfig = serde_json::from_value(raw.clone())?;

// but `raw` *also* contains every known key (channels, channel_priority, …),
// duplicated with `cfg` — harmlessly ignored here because `CondaBuildConfig`
// doesn't declare fields for them, but still un-coerced in `raw` itself
// (e.g. `channel_priority: strict` stays the bare YAML string `"strict"` in
// `raw`, not the `ChannelPriority` enum `cfg` exposes, since `raw` is the
// pre-`coerce/` document).
```

### Decision

**Neutral value map + whole-tail deserialize**: `Config::extra: HashMap<String,
serde_json::Value>`, plus `Config::extra_as::<T: DeserializeOwned>(&self) ->
Result<T, serde_json::Error>`. Unknown top-level keys are retained in `extra`, never an error
(FR-036), and never emitted by the adapter (FR-040). A caller with a known struct in mind — the
common, motivating case (§ conda-build example above) — gets there in one call:
`cfg.extra_as::<CondaBuildConfig>()?`. A caller that instead wants to introspect what's actually
present — enumerate unrecognized keys, look for typos, build tooling around `.condarc` itself —
reads `cfg.extra` directly, no special path required.

**Rationale**:
- **No generics propagate through the public API.** This was the deciding factor. `Config`,
  `parse()`, `ValidationReport`, and every future helper stay fully monomorphic. The generic
  catch-all type parameter (`Config<Extra>`) reaches the *identical* caller-visible outcome for the
  known-struct case (one call) once `extra_as` exists, so it bought nothing there — it only cost a
  permanent SemVer/API-surface tax (every signature touching `Config` becomes generic, forever, for
  every consumer, whether or not they use the `extra` field at all). Rejecting it removes real
  complexity for zero loss of caller capability.
- **`extra` as a plain map is the more capable default, not merely the simpler one.** Because
  `extra` is a first-class, always-populated field rather than something hidden behind a generic
  parameter, both use cases — "deserialize into my known struct" (`extra_as`) and "introspect what's
  there" (`cfg.extra.keys()`, `cfg.extra.get(key)`) — are available on the *same* `Config` value,
  simultaneously, with no upfront choice the caller has to commit to at the type level. Under the
  generic-parameter pattern those two use cases are mutually exclusive per call (`Config<MyStruct>`
  vs. `Config<HashMap<..>>`) — a caller wanting both would have to parse twice.
- **Strictly additive to the normal workflow.** A caller that never touches `extra`/`extra_as` at
  all pays nothing beyond the one field on `Config` — `parse()`'s signature, error type, and every
  known-setting field are exactly as they'd be without this feature. `extra_as` is an opt-in method
  call, not a required step, so callers who only care about the ~120 catalog settings are
  unaffected. This matches Constitution VI (typed model for known settings) without making the
  unknown-tail feature a tax on anyone who doesn't need it.
- **No frontend leak, idiomatic** — carried over from the general survey: neutral
  `serde_json::Value` (already a workspace dependency) keeps `yaml-rust2`/`RawValue` out of the
  public API (R1's "parser swap is one module" property holds); the shape mirrors serde's own
  `flatten`-into-`Value` convention (the survey's own first row uses `HashMap<String, Value>`),
  corroborated independently by OpenAPI codegen's `additionalProperties` and Kubernetes'
  `DynamicObject`.

**Alternatives considered** (full trade-offs and code sketches above):
- **Generic catch-all type parameter** (`Config<Extra = HashMap<..>>`) — rejected: once `extra_as`
  exists, it has no caller-visible advantage left for the known-struct case, and it costs a
  permanent generic parameter on every public signature. Not revisited unless a concrete consumer
  needs the *inline, single-pass* deserialize badly enough to justify that cost — would layer in as
  a separate, additive `parse_with_extra::<E>()` rather than changing `Config`/`parse()` today.
- **`DeserializeSeed`/hook-style builder registration** — rejected: never actually produces one
  struct value; the caller still hand-assembles it from N `typed_extra` calls, just with
  registration-time boilerplate instead of extraction-time boilerplate, plus real `Box<dyn Any>`
  machinery and a runtime string-keyed downcast with no compile-time link to what was registered.
  Confirmed YAGNI: no requirement needs *stateful*, context-driven parsing of unknown keys.
- **Whole-document raw `Value` alongside the typed struct** — rejected: duplicates the typed view of
  every known setting and re-introduces a broad dynamically-typed surface (Constitution VI), and
  that duplicate copy is un-coerced (pre-`coerce/`), an easy source of caller confusion (e.g. reading
  `channel_priority` from `raw` gives the bare YAML string, not the `ChannelPriority` enum `cfg`
  exposes for the same setting).
- **Drop unknown keys entirely** — rejected: FR-036 explicitly leaves retention open, and the parent
  GEN-23 use case (inspecting a user's real `.condarc`) and the conda-build-rewrite motivating case
  both benefit from retention; dropping forecloses both for no upside beyond a marginally smaller
  struct.
- **`#[serde(other)]`, `deny_unknown_fields`, type-erased extension bags, protobuf-style wire
  retention** — rejected outright as mismatched to the problem (see the survey table and the
  "four survey rows get no sketch" note above), not merely less preferred.

**The yaml-rust2 tie-in — the one conversion boundary that needs a stated convention**
(`RawValue` → `serde_json::Value`, used only when populating `Config::extra`): map each `RawValue`
scalar to the natural JSON type of its **resolved YAML type** — bare `true`/`null`/`7`/`1.5` → JSON
`true`/`null`/`7`/`1.5`; quoted or plain string → JSON string; seq → array; map → object. Because
`extra` holds only *unknown* keys (never coerced, never adapter-emitted), this lowering is not
required to preserve the type-vs-string distinction that the *known*-setting coercion path needs —
that distinction is consumed earlier, on the `RawValue` (not the `serde_json::Value`) side, before
`extra` is ever built. Integers outside `i64`/`f64` range in an unknown key are lowered as a JSON
string (same convention as the adapter's bignum/non-finite encoding, research §8 item 15), so this
conversion, like everything else, is total and panic-free. `parse.rs` performs this lowering for
each unrecognized top-level key; no other module needs to know `RawValue` exists.

---

## R3 — Reproducing conda's coercion semantics (the core correctness problem)

**Decision**: Implement a dedicated coercion engine (`coerce/`) whose functions are a faithful,
test-anchored port of `conda/auxlib/type_coercion.py`'s `typify`/`boolify`/`numberify` and the
`SequenceParameter`/`MapParameter` raw-shape rules — dispatched by a per-setting `ValueKind`
(the Rust analogue of conda's `element_type`) declared once in `catalog.rs`.

**Rationale**: The spec's acceptance criterion is byte-for-byte agreement with `expected/*.json`,
so the coercion rules cannot be approximated — they must match conda's exact truth tables
(research §2.2 boolify, §2.3 enum lookup, §8 items 6/11 `local_repodata_ttl`, §8 item 14 numeric).
Centralizing them by shape (one `boolify`, one numeric coercer, etc.) and driving them from a
single catalog keeps behavior consistent across the ~120 keys (DRY) and makes each rule unit-
testable in isolation (Constitution II/VIII). The exact rule set to port:
- **boolish** (`bool`, `(bool,None)`, `(str,bool)`): `BOOLISH_TRUE`/`BOOLISH_FALSE`/`NULL_STRINGS`
  tables, numeric-string truthiness, `complex()`-parseable fallback, `return_string` passthrough
  for `ssl_verify` (§2.2, §8 items 8/12/13).
- **numeric** (`int`, `float`): `int()`/`float()` vocabularies incl. PEP-515 single underscores,
  leading zeros, truncation toward zero, float-only decimals/scientific/`nan`/`inf`; **plus the A1
  `i64`/`f64` range check** (§8 item 14).
- **`(bool,int)`** `local_repodata_ttl`: the *narrower* `_Regex` boolean vocabulary, not `boolify`
  (§8 items 6/11).
- **enum** value-or-name lookup, case-sensitive; `channel_priority` bool/boolish shim (§2.3).
- **string** `str(x)` coercion (`true`→`"True"`, `7`→`"7"`, `null`→`"None"`), interior/edge
  whitespace preserved for already-string values; nullable `"none"`→null (§2.1, §2.4).
- **sequence/map** raw-shape gate (`isiterable`): reject bare scalar, accept `{}`/`null`, element
  typify (§2.4).

**Alternatives considered**: Wrapping/embedding Python conda at runtime — **rejected**: violates
FR-002/FR-003 (standalone, no external process/env), defeats the purpose of a Rust crate, and
isn't portable/publishable. Deriving coercion from the openapi JSON Schema alone — **rejected**:
the schema encodes accept/reject but not the *coerced value* (`"yes"`→`true`), which `expected/`
requires.

---

## R4 — Error accumulation architecture (Pydantic-style)

**Decision**: Parsing returns `Result<Config, ValidationReport>`. `ValidationReport` owns
`Vec<ErrorEntry>`. The pipeline evaluates **every** setting independently, pushing an entry per
failure rather than short-circuiting, then runs cross-field/alias-collision checks, then returns
`Err(report)` if non-empty. The two non-accumulable classes (YAML syntax error; non-mapping root)
return a single-entry report immediately, before any per-field evaluation (FR-032).

**Rationale**: FR-030/FR-031/SC-005 mandate collecting *all* independent problems in one pass.
Because coercion of one key is independent of another (conda evaluates each `ParameterLoader`
separately — research §1.3), the Rust port can `match` each key into `Ok(value)` / `Err(entry)`
and collect the `Err`s, mirroring Pydantic's `ValidationError.errors()` list. Alias collisions and
the two cross-field rules are computed from the *raw resolved keys* / *collected values* after the
per-key pass, so they too accumulate. This is straightforward, deterministic, and has no ordering
hazard because there is no early return inside the per-field loop.

**Alternatives considered**: First-error-only (`?`-based) — **rejected** by FR-030 explicitly.
Panicking on the first bad value — **rejected** by FR-004/FR-035 (no panics).

---

## R5 — Error type / trait implementation

**Decision**: Hand-implement `std::error::Error` + `Display` on `ValidationReport` and derive
`serde::Serialize` for the machine-readable JSON form (contract in
`contracts/error-report.schema.json`). Use **`thiserror`** to reduce boilerplate on the internal
error/kind enums *only if* it clears `cargo deny` (it is MIT/Apache and already common); otherwise
hand-roll. `ErrorEntry` fields (`location`, `kind`, `message`, `input`) are public and typed
(FR-037); `ErrorKind` is a `#[non_exhaustive]` enum with stable serde rename strings
(`type_coercion`, `semantic_validation`, `alias_collision`, `cross_field`, `root_shape`,
`yaml_syntax`) matching FR-033.

**Rationale**: FR-034/FR-037 require the report to implement Rust's standard error trait (usable
with `?`) *and* serialize to a stable, versioned JSON contract, *and* render human-readably. Deriving
`Serialize` gives the machine form; a `Display` impl that lists one entry per line gives the human
form (Constitution III dual-interface). Keeping `ErrorKind` `#[non_exhaustive]` and versioning the
JSON schema (contracts/) honors SemVer expectations (Constitution, plan III).

**Alternatives considered**: `anyhow` — **rejected**: erases the typed, branchable structure agents
need (FR-037). Stringly-typed errors — **rejected** by FR-037 explicitly.

---

## R6 — `ssl_verify` filesystem-existence branch (FR-024 / FR-002 / A3)

**Decision**: Parsing is **side-effect-free by default**, and the `os.path.exists` branch of
`ssl_verify` is an **opt-in runtime option** — not a Cargo feature, and not the default. Concretely:

- `ParseOptions::default()` (used by `condarc::parse(yaml)`) performs **no** filesystem access at
  all. A non-boolish, non-`truststore` `ssl_verify` string is taken at face value as a certificate
  path (`SslVerify::Path`). Two runs of the same document on two different machines therefore agree,
  which is exactly the determinism Constitution IX and FR-002 ask for.
- `condarc::parse_with_options(yaml, ParseOptions { ssl_verify_fs_check: true, ..Default::default() })`
  additionally requires that such a path exist, rejecting it otherwise — conda's exact runtime rule.
- **The conformance harness passes `ssl_verify_fs_check: true`.** The corpus was generated *from real
  conda*, which always performs the check, so the corpus encodes FS-checking behavior:
  `invalid/ssl_verify_passthrough_reject_string_arbitrary_word.json` (`"banana"`),
  `..._reject_string_nonexistent_path.json`, `..._reject_truststore_wrong_case.json` and
  `..._reject_string_hex_literal_shaped.json` are rejections *because* those strings are not existing
  paths (with the flag off the crate would accept all four), while
  `valid/ssl_verify_passthrough_accept_existing_path_parent_dir.json` (`".."`) and
  `..._accept_existing_path_dot_slash.json` (`"./"`) are acceptances *because* those paths exist. Both
  accepting fixtures were added in this review round: the pre-existing `"."` fixture does **not**
  exercise the path branch at all, because `boolify`'s `.replace(".", "", 1)` probe reduces `"."` to
  `""`, a `BOOLISH_FALSE` token, so conda loads it as the boolean `false` (its `expected/` fixture
  records exactly that). The corpus's paths are chosen so the check stays deterministic (`..` and `./`
  exist relative to any working directory; the rejected paths never exist), so opting in costs the
  harness no reproducibility — and it means the crate is held to conda's real behavior rather than to a
  weaker portable subset.

**Rationale**: FR-002 wants a pure parse; conda's real rule wants a stat(2). Splitting them along an
explicit, caller-supplied runtime option satisfies both, with the *safe, surprise-free* choice as the
default (Constitution V): a library that silently touches the filesystem because of a string in its
input is the more surprising design, so that behavior must be asked for. The alternative default —
*rejecting* any unverifiable path when the check is off — was considered and rejected: it would make
`condarc::parse` refuse a perfectly ordinary `ssl_verify: /etc/ssl/certs/ca.pem` for no reason the
caller can see, i.e. it would trade a side effect for a false negative. Accepting-unverified is the
honest "I did not check" answer, and callers who need the check can ask for it.

**Alternatives considered**: Always doing the FS check — **rejected**: non-deterministic across
environments and forces an environment dependency onto every consumer with no way to opt out. Never
doing it — **rejected**: loses runtime fidelity for GEN-23 *and* makes the `ssl_verify` conformance
fixtures unsatisfiable (they only make sense against a filesystem). A Cargo feature flag
(`ssl-verify-fs-check`) — **rejected**: compile-time only, subject to Cargo's whole-build-graph
feature unification (enabling it anywhere in the dependency tree enables it everywhere, for callers
who never opted in), unable to vary per call or per environment within one binary, and a heavier,
more surprising control surface than an ordinary argument for what is fundamentally a per-call
choice, not a build-time capability.

---

## R7 — Determinism of error-entry ordering

**Decision**: Error entries are ordered deterministically by **(1) the two non-accumulable classes
first (single entry, no ordering question), then (2) per-field errors in the fixed catalog
declaration order, then (3) alias-collision entries in catalog order, then (4) the two cross-field
rules in their documented order** (`client_ssl_cert` rule, then `always_copy`/`always_softlink`
rule — research §1.4). Within a nested location (e.g. `channel_settings[2].auth`), entries follow
document index / map-key sorted order.

**Rationale**: Constitution IX requires identical inputs → identical output, including the error
report. Ordering by the *catalog's* fixed order (not the document's hash-map iteration order) makes
the report stable regardless of YAML key order in the input. This is a documented part of the JSON
contract (contracts/error-report.schema.json notes ordering is stable but SHOULD NOT be relied on
for semantics — consumers branch on `kind`+`location`, not index).

---

## R8 — Absent settings: no defaulting layer at all (FR-038)

**Decision**: `Config` models every setting as `Option<_>` (or `Option<Option<_>>` for conda's
nullable `(T, None)` settings). Absent means absent: `None`. There is **no** defaults table, **no**
effective-value layer, and **no** "sensible off-state" shortcut — a plain boolean that conda documents
as defaulting to `false` is still `Option<bool>` and still reads back as `None` when the document did
not set it. Callers apply their own default policy explicitly, at the point of use.

**Rationale**: It is the simplest honest model: "was this set?" is answerable, the adapter's present-only contract (FR-040)
holds without special-casing, and the crate never has to track upstream default changes. An earlier
draft of this spec permitted storing off-state booleans as bare `false` (the old FR-039); that
permission has been **removed** from the spec, so uniform `Option` is now the requirement, not merely
the preferred option.

**Alternatives considered**: Backfilling conda defaults — **rejected** by FR-038 and by the ticket
("missing-vs-default is explicitly out of scope"). Storing plain bools as `bool` defaulting to `false`
— **rejected**: it silently invents a value the document never contained, makes "unset" and
"explicitly false" indistinguishable to the caller, and risks the adapter emitting a key for a setting
that was absent.

---

## R9 — Adapter (internal → `expected/` JSON) details (FR-040 / FR-041)

**Decision**: The `Config` → `expected/`-shaped-JSON adapter is **conformance-harness support code,
not a library feature**. It lives in the *same test target* as the harness that uses it —
`tests/support/adapter.rs`, declared as `mod support;` from `tests/condarc_conformance.rs` — and **not**
under `crates/condarc/src/`, and **not** under `crates/condarc/tests/` (see R10: a test file in another
crate's `tests/` directory is unreachable from this harness; that was raised in PR review and confirmed
empirically). Concretely, a function with the shape
`to_expected_json(&condarc::Config) -> serde_json::Value` — consuming the library's own public `Config`
from outside the crate, exactly as any downstream caller would — emits a JSON object keyed by conda's
**canonical loader attribute names** (aliases resolved: `channel`→`channels`,
`verify_ssl`→`ssl_verify`, `yes`→`always_yes`, `virtual_packages`→`override_virtual_packages`,
etc. — the full map derived from research §1.3 / §4 and cross-checked against every `expected/`
fixture), containing **only present settings**. Value encoding:
- booleans/nullable-bool → JSON `true`/`false`/`null`;
- enums → their canonical lowercase value string (`ChannelPriority::Strict`→`"strict"`);
- integers → JSON number (guaranteed in `i64` by A1);
- floats → JSON number, **except** non-finite: `+inf`→`"Infinity"`, `-inf`→`"-Infinity"`,
  `NaN`→`"NaN"` (JSON strings, matching `expected/` and research §8 item 15 / FR-041);
- strings → JSON string; nullable-string null → JSON `null`;
- sequences → JSON array; maps → JSON object; `channel_settings` → array of string→string objects.

Conformance comparison is **exact** (FR-040): for each `valid/` fixture, the adapted JSON object must
equal the whole `expected/` fixture — no missing keys, no extra keys, no renamed keys. PR review asked
whether subset comparison could hide an adapter emitting keys it shouldn't; it could, so the weaker
comparison was dropped. Exactness was verified to be achievable *before* committing to it: for all 388
object-rooted `valid/` fixtures, the set of canonical names of the document's keys equals the key set of
the corresponding `expected/` fixture — but only after correcting one catalog entry, `auto_activate`
(canonical) vs. `auto_activate_base` (alias), which the original draft had backwards. `valid/null_root.json`
has no `expected/` file (the generator skips non-object roots) and is therefore exempt from the adapter
comparison, exactly as it is for the conda checker.

**Rationale**: The literal acceptance criterion (SC-002) is that the *conformance harness* can prove
byte-for-byte agreement with the committed `expected/*.json` oracle — nothing in FR-040/FR-041 or the
ticket description asks the published crate itself to expose an `expected/`-JSON-shaped view of
`Config` to real callers (GEN-23, a future conda-build rewrite, etc.); those callers consume typed
`Config` fields directly. The canonical-name map and non-finite string encoding this function
implements are artifacts of matching *this repo's fixture format*, not a general-purpose
serialization the library needs to offer or support long-term. Keeping it in the test crate:
- avoids adding a permanent, publishable-surface feature (naming map, non-finite string encoding,
  etc.) whose only consumer, today or foreseeably, is this repo's own conformance suite;
- keeps `condarc`'s public API exactly `Config` + `parse`/`parse_with_options` + `ValidationReport`
  (R10's workspace boundary), so the crate has no dependency on, or knowledge of, the `expected/`
  fixture shape at all;
- matches Constitution VI (internal type safety should not be constrained by an external wire
  shape) more strongly than "adapter is a permitted internal module" would — the adapter isn't part
  of the crate's internals either, so `Config`'s design is free to evolve without touching a
  test-only concern living in a different crate/module tree.

The canonical-name map and the non-finite string encoding are both dictated by the committed
`expected/` fixtures (verified above against
`expected/aliases_accept_alias_spellings_all_params.json` and
`expected/numeric_values_accept_float_only_string_inf_lower.json`), and the whole catalog's
canonical/alias table was additionally cross-checked mechanically against a live conda 26.5.3
`Context` (all 99 entries agree — see data-model.md §5).

**Alternatives considered**:
- Forcing `Config`'s internal layout to mirror the JSON exactly — **rejected** by the ticket
  (round-tripping / wire-shape fidelity is explicitly out of scope) and by Constitution VI (internal
  type safety should not be constrained by an external wire shape).
- Shipping the adapter as a private (`pub(crate)`) module inside `crates/condarc/src/` —
  **rejected**: "private" still means it ships in the compiled crate and its source is part of what
  gets published/reviewed as library code, for a function no non-test caller will ever invoke. There
  is no library requirement it serves; its sole reason to exist is proving conformance, so it belongs
  in the test code that has that requirement (Constitution II: don't grow the library surface for a
  test-only concern).
- Shipping it as a separate, publishable crate (e.g. `condarc-conformance-adapter`) — **rejected** as
  unnecessary ceremony: it has exactly one consumer (`tests/condarc_conformance.rs` in this repo), so
  a plain test-support module is sufficient; nothing about it needs independent versioning or
  publication.

---

## R10 — Workspace restructure (FR-003) and where the harness/adapter live

**Decision**: Convert the repo-root `Cargo.toml` into a workspace **without moving the existing
`allez` package**: the root manifest keeps its `[package]` table and gains
`[workspace] members = [".", "crates/condarc"]`. The new library is added at `crates/condarc/`, and
`allez` takes it as a `path` dependency (a dev-dependency until GEN-23 consumes it for real). The
conformance harness stays exactly where it is (`tests/condarc_conformance.rs`, a test target of the
root `allez` package), and the test-only adapter lives beside it as `tests/support/adapter.rs`,
included via `mod support;` from the harness.

**Why not `crates/allez/` + `crates/condarc/tests/conformance_support.rs`** (the earlier draft):
PR review asked how a root `tests/` file could reach adapter code living under another crate's
`tests/` directory. It can't, and both halves of the problem were verified empirically in a scratch
workspace before deciding:
1. `condarc::conformance_support::…` fails to compile (`error[E0433]: cannot find
   'conformance_support' in 'condarc'`) — a crate's `tests/` directory is a set of separate test
   binaries, not part of the library, so nothing outside that crate's own test targets can name it.
2. The only way to make it "work" is `#[path = "../crates/condarc/tests/conformance_support.rs"] mod
   support;`, which compiles the same file into two different test binaries and drags that file's own
   `#[test]` functions into the harness binary as a side effect. That's a hack, not a design.
3. Keeping the adapter as `tests/support/adapter.rs` in the *same* target as the harness compiles
   cleanly, and cargo does **not** auto-discover files in `tests/` subdirectories as additional test
   targets, so `support` exists only as a module of the harness — also verified.

Keeping `allez` as the root package (rather than moving it to `crates/allez/`) additionally means:
- `rstest`'s `#[files("conformance/condarc/valid/*.json")]` globs, which expand relative to the test's
  `CARGO_MANIFEST_DIR`, keep resolving — the harness's manifest dir is still the repo root. Moving the
  package would have required rewriting every glob and `env!("CARGO_MANIFEST_DIR")`-relative path in
  the harness to `../../…`, which is exactly the fragility the review was pointing at.
- `Makefile` and `.github/workflows/ci.yml` need **no** path changes: `cargo test --test
  condarc_conformance --features conformance-tests` still selects the root package, and `touch
  tests/condarc_conformance.rs` still targets the right file.
- `cargo test --all` / `--workspace` picks up `condarc`'s own unit and integration tests too.

The cost is cosmetic asymmetry (one package at the root, one under `crates/`), which is a normal Cargo
layout and a smaller price than rewriting the harness's path handling. `condarc` remains independently
publishable (FR-003): it has its own manifest, no dependency on `allez`, and no knowledge of the
`expected/` fixture format (the adapter lives on the `allez`/harness side of the boundary).

**Alternatives considered**: Keep `condarc` as a module inside the `allez` binary — **rejected** by
FR-003 (not independently publishable, couples to the binary). Move `allez` into `crates/allez/`
anyway — **rejected**: pure churn plus harness path rewrites, for symmetry alone. Ship the adapter as
its own workspace crate — **rejected** as ceremony for a single-consumer test helper (see R9). Publish
the adapter from `condarc/src/` — **rejected** by R9 (permanent public surface for a test-only need).

---

## Resolved unknowns summary

| Plan unknown | Resolution |
|---|---|
| YAML parser choice | `yaml-rust2` (R1); stable API, matches `config-rs` ecosystem precedent, pending `cargo deny`/`audit` confirmation |
| Unknown/custom-key surfacing | retain as `extra: HashMap<String, serde_json::Value>`, plus an `extra_as::<T>()` convenience method; caller re-deserializes on demand (R2) |
| Error crate / trait impl | hand `Error`+`Display`, derive `Serialize`, optional `thiserror` (R5) |
| FR-024 `ssl_verify` FS access | opt-in runtime `ParseOptions.ssl_verify_fs_check` flag, default off (parse is side-effect-free; unverified paths accepted), not a Cargo feature; conformance harness opts in (R6) |
| Error ordering / determinism | fixed catalog order, documented (R7) |
| Absent-setting representation | uniform `Option<T>`, no defaulting layer at all; adapter emits present-only (R8) |
| Workspace vs. module | Cargo workspace: root package `allez` stays put, library added as `crates/condarc`; harness + test-only adapter both live in the root package's `tests/` target (R10) |

All `NEEDS CLARIFICATION` items are resolved. No open blockers for Phase 1.
