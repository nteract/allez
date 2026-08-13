# Phase 1 Data Model: Default Package Resolution and Precedence

Types/functions are grouped by codebase location. See `research.md` for the rationale behind each choice below; see `contracts/default_package_resolution_contract.md` for the precedence algorithm's own black-box contract.

## `src/channel_config/document.rs`

A new private submodule nested inside the existing `channel_config` module — not a new crate-root file. It owns the shared `.condarc` location/read/parse step `channel_config` and `default_packages_config` both build on (research.md's first Decision), reusing `channel_config`'s existing `locate.rs`/`events.rs` in place rather than moving or deleting either. Nothing here is `pub`; every item is `pub(crate)`, reachable crate-wide from the module root — except `FallbackReason` itself, which stays defined in `channel_config::events` (unchanged) at its existing `pub` visibility.

`FallbackReason` (unchanged, still defined in `channel_config::events`):

```rust
/// Distinguishes the two recorded `.condarc` resolution-failure cases.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    /// `.condarc` exists and is readable, but `condarc::parse()`
    /// rejected it (or, for `channel_config`'s own caller,
    /// `condarc::expand_channels()` failed on an otherwise-valid parse).
    Rejected,
    /// `.condarc` exists but could not be read (OS permission/I/O error).
    Unreadable,
}
```

`document.rs`'s own new content:

```rust
/// The result of locating, reading, and parsing `~/.condarc` exactly
/// once (research.md's "sum type, not a tuple" Decision — the three
/// variants are mutually exclusive by construction, so no invalid
/// `(Some, Some)`-shaped combination is representable).
pub(crate) enum CondarcDocument {
    /// No file exists (GEN-23's own spec.md FR-009 silent-missing case,
    /// reused per FR-001) — every caller treats this identically to a
    /// file that exists but configures nothing.
    Absent,
    /// The file exists but is unreadable, or `condarc::parse()`
    /// rejected it. `emit_fallback(reason, ...)` has already fired
    /// exactly once before this variant is returned.
    FellBack(FallbackReason),
    /// A successfully parsed document, handed to callers as `&Config`
    /// so both `channel_config::channels_from_document` and
    /// `default_packages_config::create_default_packages_from_document`
    /// can read different fields of the same parse result without a
    /// second file read.
    Parsed(condarc::Config),
}

/// Locates, reads, and parses the current user's `~/.condarc`.
pub(crate) fn resolve_document() -> CondarcDocument;

/// Test-seam variant of [`resolve_document`] taking an explicit path.
/// `path == None` means "no path was supplied for this call" and
/// resolves directly to [`CondarcDocument::Absent`] — it does **not**
/// re-resolve [`default_condarc_path`]; only the no-argument
/// [`resolve_document`] does that, by explicitly passing
/// `default_condarc_path().as_deref()` into this function.
pub(crate) fn resolve_document_from(path: Option<&Path>) -> CondarcDocument;
```

`default_condarc_path()`, `condarc_path_override()` (the `test-config-override`-gated `ALLEZ_CONDARC_PATH` seam) stay exactly where they already are, as private free functions in `channel_config/mod.rs` itself (not in `locate.rs`); `ReadOutcome` and `read_condarc()` stay exactly where they already are, in `channel_config::locate` (unchanged); `emit_fallback()` and the `ChannelConfigFallbackEvent` struct plus its schema-version constant stay exactly where they already are, in `channel_config::events` (unchanged). `document.rs` calls into both directly as a sibling submodule — no visibility widening, move, or deletion of either file.

## `src/channel_config/`

`ChannelConfigResolution`, `resolve_channel_config()` (`pub`), `resolve_channel_config_from()` (`pub(crate)`), and `FallbackReason` (`pub`, defined in `channel_config::events`, unchanged) form this module's public surface, per GEN-23's own contract. One `pub(crate)` function handles channel-specific expansion:

```rust
/// The channel-specific half of resolution, separate from I/O
/// (research.md's first Decision): expands `document`'s channel
/// settings if [`CondarcDocument::Parsed`], else falls back to
/// `condarc::expand_channels(&Config::default())` for
/// [`CondarcDocument::Absent`]/[`CondarcDocument::FellBack`]. May
/// additionally emit its own `FallbackReason::Rejected` event if
/// `document` is `Parsed` but its own channel settings fail
/// `condarc::expand_channels` — a distinct failure from the shared
/// read/parse step, still classified as `Rejected`.
pub(crate) fn channels_from_document(document: &CondarcDocument) -> ChannelConfigResolution;
```

`resolve_channel_config_from(path)` is a two-line composition: `document::resolve_document_from(path)` then `channels_from_document(&document)` — used by its own tests and by any other standalone caller; `src/cli/oneshot.rs` calls the two lower-level pieces directly instead, so one invocation reads `.condarc` once, not twice.

## `src/default_packages_config.rs`

```rust
/// Resolves the default package set from an already-resolved `.condarc`
/// document (FR-001/FR-002): for [`CondarcDocument::Parsed`], the
/// document's own `create_default_packages.clone().unwrap_or_default()`;
/// for [`CondarcDocument::Absent`]/[`CondarcDocument::FellBack`], an
/// empty `Vec` — no defaulting, validation, or fallback beyond what that
/// resolution already provides. Each entry becomes a `PackageSpec` via
/// [`crate::ephemeral::PackageSpec::from_resolved_default`] — never via the
/// validating `PackageSpec::parse`, so this function cannot fail:
/// every resolved entry, however malformed, is handed to
/// [`crate::ephemeral::create_ephemeral_environment`]'s own internal
/// `effective_packages` merge unchanged; whether it then
/// survives into the Effective Package Set (FR-004's supersede rule) or
/// fails naturally at solve time if it does not name a real package is
/// that later step's and the solver's concern, not this one's
/// (research.md's "never rejects a resolved entry" Decision).
pub(crate) fn create_default_packages_from_document(
    document: &CondarcDocument,
) -> Vec<crate::ephemeral::PackageSpec>;
```

No public top-level "resolve everything from scratch" convenience function exists here (unlike `channel_config::resolve_channel_config()`): this module has exactly one real caller (`src/cli/oneshot.rs`), which already holds the shared `document` from its own single `channel_config::resolve_document()` call (the re-exported path — `document` itself is a private submodule, unreachable as `channel_config::document::...` from outside `channel_config`) — a convenience wrapper would only ever be used to immediately re-read `.condarc` a second time, the exact duplication this ticket's design avoids.

## `src/ephemeral/defaults.rs`

`InvalidPackageSpec` is unaffected. `DEFAULT_PACKAGES` (the constant) and `RequestedPackages` (the enum) are deleted outright, along with every one of their own existing unit tests (`no_override_falls_back_to_default_packages`, `override_resolving_to_empty_falls_back_to_default_packages`, `explicit_non_empty_wins_over_any_override`, `explicit_empty_falls_back_like_use_default_or_override`, `non_empty_override_wins_over_default_packages`, `from_cli_empty_list_becomes_use_default_or_override`, `from_cli_non_empty_list_becomes_explicit`): the additive/supersede precedence this ticket introduces has no "use the built-in default instead" branch for either type to represent. `PackageSpec`'s own type-level invariant is "an opaque conda package-request string, syntactically validated when constructed via `parse()`" — `PackageSpec::parse()` remains the only validating, public-facing constructor (every per-invocation CLI entry uses it); a second, crate-private constructor exists specifically for the one case FR-002 requires bypassing that validation:

```rust
/// Wraps `input` as a `PackageSpec` with no `MatchSpec` validation at
/// all — unlike `PackageSpec::parse`, this cannot fail. Used only by
/// `default_packages_config::create_default_packages_from_document`,
/// for a `create_default_packages` entry FR-002 forbids rejecting; no
/// other caller may construct a `PackageSpec` this way.
/// `bare_name()` (below) already has a defined, safe behavior for a
/// `PackageSpec` built this way that does not re-parse as a valid
/// match-spec.
pub(crate) fn from_resolved_default(input: String) -> Self; // on impl PackageSpec
```

Three more `pub(crate)` functions:

  ```rust
  /// Parses zero or more raw, caller-supplied package strings
  /// (`allez oneshot`'s per-invocation package list, named before `--`)
  /// into validated `PackageSpec`s. An empty input list is not a special
  /// case: it simply parses to an empty `Vec`, which `effective_packages`
  /// then treats as "supersede nothing, add nothing."
  pub(crate) fn parse_explicit_packages(packages: Vec<String>) -> Result<Vec<PackageSpec>, InvalidPackageSpec>;

  /// Extracts this spec's bare package name via a `MatchSpec` reparse
  /// (research.md's Decision — not `PackageName::from_matchspec_str_unchecked`,
  /// which does not strip a channel qualifier and would compare
  /// `conda-forge::numpy` unequal to `numpy`). `Some` when the reparse
  /// yields an exact name matcher: always, for a `PackageSpec` built via
  /// the validating `PackageSpec::parse` (every per-invocation entry);
  /// sometimes, for one built via `from_resolved_default` (every default-sourced
  /// entry). `None` when it does not — an empty or whitespace-only
  /// `from_resolved_default` string is exactly this case — meaning this entry
  /// has no bare name to compare by and always survives `effective_packages`'s
  /// merge unaffected. Used only for [`effective_packages`]'s supersede
  /// comparison — resolution/solving itself still consumes each
  /// `PackageSpec`'s full, opaque match-spec text unchanged.
  pub(crate) fn bare_name(&self) -> Option<String>; // on `impl PackageSpec`

  /// Resolves the Effective Package Set (spec.md's Key Entity) per
  /// FR-003/FR-004: every `defaults` entry survives, in its own original
  /// order, unless its `bare_name()` is `Some` and matches some `explicit`
  /// entry's `Some` bare name (in which case it is dropped — that entry
  /// is *not* also appended a second time from `defaults`'s side); a
  /// `defaults` entry whose `bare_name()` is `None` always survives, since
  /// it has nothing to be compared against. Every `explicit` entry is
  /// then appended, in its own original order. Two entries sharing a
  /// bare name *within* the same input list (either list) are never
  /// deduplicated against each other — see research.md's "Additive,
  /// supersede-by-bare-name merge" Decision; this function's
  /// own uniqueness guarantee is scoped to "no surviving `defaults` entry
  /// shares a bare name with a surviving `explicit` entry," not to global
  /// uniqueness across the whole result.
  pub(crate) fn effective_packages(explicit: &[PackageSpec], defaults: &[PackageSpec]) -> Vec<PackageSpec>;
  ```

`parse_explicit_packages` and `effective_packages` are `pub(crate)` free functions of the private `defaults` module (`mod defaults;`, unchanged visibility in `src/ephemeral/mod.rs`). `parse_explicit_packages`'s only caller, `src/cli/oneshot.rs`, lives outside `ephemeral`'s own module tree, so `ephemeral::mod.rs` re-exports it at its own level — `pub(crate) use defaults::parse_explicit_packages;`, alongside its existing `pub use defaults::{InvalidPackageSpec, PackageSpec};` — making it reachable as `crate::ephemeral::parse_explicit_packages` from any module in the crate, without making `defaults` itself, or any other item in it, part of `ephemeral`'s public API. `effective_packages`'s only caller is `create_ephemeral_environment`'s own body, in the parent `ephemeral` module (§ below) — reachable there directly as `defaults::effective_packages(...)` (already `pub(crate)`-visible crate-wide, so no re-export is needed for a same-crate parent-module caller); `oneshot.rs` never calls it. `bare_name` is an inherent `pub(crate)` method on `PackageSpec` (not a free function needing its own re-export): since `PackageSpec` itself is already `pub use`-exported from `ephemeral`, any crate-internal code holding a `PackageSpec` value can already call `.bare_name()` on it via ordinary method-call syntax; `effective_packages` is `bare_name`'s only caller, internal to `defaults.rs`. Neither `examples/ephemeral_smoke.rs` nor the integration tests under `tests/` need any of these directly (they call `create_ephemeral_environment` itself, which performs the merge internally — § below) — `effective_packages`'s own unit tests live inside `src/ephemeral/defaults.rs` itself (`#[cfg(test)] mod tests`), which has access regardless of visibility.

## `src/ephemeral/mod.rs`

```rust
/// Creates a new ephemeral environment populated with the Effective
/// Package Set (spec.md's Key Entity): `explicit` merged additively
/// over `defaults` by bare package name (FR-003/FR-004), resolved and
/// installed against `channels`.
///
/// `explicit`: the caller's own per-invocation package list — for
/// `allez oneshot`, the result of `ephemeral::parse_explicit_packages`;
/// for any other caller with no notion of a configured default (e.g.
/// `examples/ephemeral_smoke.rs`), simply its own package list, with
/// `defaults` left empty.
///
/// `defaults`: the caller's own resolved default package set (spec.md's
/// Default Package Set) — for `allez oneshot`, the result of
/// `default_packages_config::create_default_packages_from_document`.
///
/// The merge (`defaults::effective_packages`) runs as this function's
/// own first step, before any filesystem or network work begins, so
/// every caller gets FR-003/FR-004's precedence applied identically
/// without needing to call `effective_packages` (a `pub(crate)`
/// function, unreachable outside this crate) itself first.
///
/// Returns the [`ReadyEnvironment`] once solve and install both succeed;
/// it is not torn down when dropped, matching this module's own
/// no-automatic-teardown contract (GEN-24).
///
/// Returns [`CreationFailure`] if root creation, solve, or install
/// fails — including an unresolvable entry from either `explicit` or
/// `defaults` (`EphemeralEnvError::UnresolvablePackage`/`ResolutionFailed`,
/// unchanged by this ticket) — carrying the failed attempt's own
/// [`EnvironmentId`] and any rollback error alongside the original one.
pub async fn create_ephemeral_environment(
    explicit: Vec<PackageSpec>,
    defaults: Vec<PackageSpec>,
    channels: condarc::ResolvedChannels,
) -> Result<ReadyEnvironment, CreationFailure>;
```

`create_ephemeral_environment` is `pub`. Its `(explicit, defaults, channels)` signature — a breaking change to `allez`'s public library API — is the minimal, mechanical consequence of enforcing FR-003/FR-004's additive/supersede precedence inside the one function every caller of this feature already goes through (plan.md's Constitution V/VI addresses why this break is acceptable at this project's current stage). Every direct caller (`examples/ephemeral_smoke.rs`, `tests/support/*.rs`, and `src/cli/oneshot.rs` itself) is updated to this three-parameter shape within this same ticket (plan.md's Project Structure); `tests/oneshot_exec.rs` exercises it only indirectly, through the compiled `allez` binary, and needs no source-level call-site update.

`pub use defaults::{InvalidPackageSpec, PackageSpec};` is this module's public re-export list; `parse_explicit_packages` is re-exported alongside it as `pub(crate) use defaults::parse_explicit_packages;` (§ `src/ephemeral/defaults.rs`, above) — reachable crate-wide, not part of the public API. `effective_packages` needs no re-export at all, being called only from within this same module's own `create_ephemeral_environment` body. `bare_name` needs no separate re-export either, being a method on the already-exported `PackageSpec` type.

## `src/cli/oneshot.rs` (orchestration; public `OneshotOutcome` contract)

`run()`'s body:

1. `let explicit = match ephemeral::parse_explicit_packages(args.packages.clone()) { Ok(v) => v, Err(invalid) => return environment_creation_failed(..., human) };`
2. `let document = channel_config::resolve_document();` — the one `.condarc` read for this invocation (the re-exported path; `document` itself is a private submodule, so `channel_config::document::resolve_document()` is not reachable from `oneshot.rs`).
3. `let channels = match channel_config::channels_from_document(&document) { ChannelConfigResolution::Ready { config, .. } => config, ChannelConfigResolution::NoChannels => return environment_creation_failed(..., human) };`
4. `let defaults = default_packages_config::create_default_packages_from_document(&document);` — infallible; no failure path (research.md's "never rejects a resolved entry" Decision).
5. `create_ephemeral_environment(explicit, defaults, channels).await` — `create_ephemeral_environment` performs the merge (FR-003/FR-004) itself, as its own first step; `oneshot.rs` never calls `effective_packages` directly.

Every branch above returns through an explicit `match`/`environment_creation_failed(...)` pattern (`run()` returns `OneshotOutcome`, not `Result`, so no `?`-operator shorthand). `OneshotOutcome`, its `exit_code()` mapping, and JSON/human rendering are governed entirely by `contracts/oneshot_cli_contract.md` (GEN-25) and are outside this contract's scope: this ticket changes *which* packages get resolved, not how a resolution failure is reported.

## `src/lib.rs`

`mod default_packages_config;` — one new private module declaration, alongside `pub mod channel_config;`/`pub mod cli;`/`pub mod ephemeral;`. `default_packages_config` is not `pub`: it is reachable crate-wide (Rust module privacy is tree-scoped, not caller-scoped) but exposes nothing outside the crate, so `#![warn(missing_docs)]` imposes no public-doc obligation on it. `channel_config::document` needs no top-level declaration of its own — it is declared inside `src/channel_config/mod.rs` directly (§ above).

## `skills/allez-oneshot.md`

A `### Default packages` subsection (research.md's own Decision has the exact content) inside `## Usage`, positioned before its `### Examples` sibling subsection. The file's frontmatter and its examples remain accurate under this precedence rule.
