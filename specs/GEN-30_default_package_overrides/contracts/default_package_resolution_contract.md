# Interface Contract: Effective Package Set Resolution

This feature's real "interface" is not a CLI flag or JSON shape — the CLI surface itself (`allez oneshot [PACKAGES]... -- <COMMAND> [ARGS...]`, its exit codes, its JSON/human error envelopes) is defined by GEN-25's `contracts/oneshot_cli_contract.md`, out of this contract's own scope. This contract defines the **precedence rule** by which `allez oneshot`'s per-invocation packages and the caller's own `.condarc` combine into one package request — the thing `skills/allez-oneshot.md` documents for a human or agent caller, and the thing SC-004 requires automated test coverage for.

## Inputs

- **Resolved Default Package Set**: whatever the invoking user's own `~/.condarc` `create_default_packages` setting resolves to via `condarc::parse()`, with no `allez`-specific defaulting, fallback, or validation of any kind — including empty, whether from an absent key, an absent/unreadable/malformed file, or an explicit `create_default_packages: []`. `allez` never rejects a resolved entry at this stage, however malformed: every one becomes an input to the "Rule," below, which alone decides whether it survives into the Effective Package Set; a surviving entry only fails (if it does) at the same solve-time step any other unresolvable package name already fails at (see "Failure modes," below). May itself contain two or more entries sharing a bare package name — this contract does not deduplicate or otherwise alter that list beyond what "Rule," below, does.
- **Per-Invocation Package List**: the packages named before `--` on the `allez oneshot` command line, zero or more, each a syntactically valid conda match-spec string. May likewise contain two or more entries sharing a bare package name with each other.

## Output

**Effective Package Set**: the top-level package request handed to environment creation.

## Rule

1. Every entry in the Resolved Default Package Set is included, **unless** its bare package name (the name portion only — ignoring any version/build/channel/subdir constraint on either side of the comparison, and compared using conda's own case-insensitive, normalized package-name identity rather than raw byte-for-byte string equality) matches the bare package name of some entry in the Per-Invocation Package List.
2. Every entry in the Per-Invocation Package List is included, unconditionally.
3. No surviving Default entry ever shares a bare package name with a surviving Per-Invocation entry — where both a default and a per-invocation entry would, only the per-invocation entry's own spec survives. This is the contract's **only** uniqueness guarantee — see "Non-goals" for the two collision shapes it deliberately does not cover.

Equivalently: `Effective = (Defaults \ {d : bare_name(d) ∈ bare_names(PerInvocation)}) ++ PerInvocation`.

This rule is a statement about **membership** — which entries survive, and which one spec wins on a collision — not about order: the solver does not depend on, and this contract does not promise, any particular ordering of the resulting `Vec`.

## Examples

The table below shows one deterministic ordering only so the unit tests exercising these cases have an unambiguous expected value to assert against, not as an ordering promise this contract makes to any caller.

| Resolved Defaults | Per-Invocation | Effective Package Set |
|---|---|---|
| `[]` | `[]` | `[]` |
| `[numpy]` | `[]` | `[numpy]` |
| `[]` | `[numpy]` | `[numpy]` |
| `[numpy]` | `[pandas]` | `[numpy, pandas]` |
| `[numpy=1.2]` | `[numpy=2.0]` | `[numpy=2.0]` |
| `[numpy=1.2, scipy]` | `[numpy=2.0]` | `[scipy, numpy=2.0]` |
| `[numpy]` | `[conda-forge::numpy]` | `[conda-forge::numpy]` (channel-qualified per-invocation spec still supersedes the bare default entry — comparison ignores channel) |
| `[numpy=1.2, numpy=2.0]` | `[]` | `[numpy=1.2, numpy=2.0]` (both default-set entries survive as-is — see Non-goals) |

## Failure modes

- A Per-Invocation entry that is not a syntactically valid match-spec: the whole invocation fails before any resolution/install begins, category `unresolvable_package` — the same category GEN-25's own CLI contract defines for this case.
- A Resolved Default Package Set entry that is not a syntactically valid match-spec (e.g. because `create_default_packages` resolved it to an empty string): **not** a failure at this layer at all — `allez` never rejects it. If it survives the "Rule" above (i.e. it is not superseded by a Per-Invocation entry sharing its bare name), and it does not correspond to a real installable package, it fails at solve time exactly as any other made-up package name would (the same outcome the next bullet describes).
- A Resolved Default Package Set entry that happens to be a syntactically valid match-spec, regardless of what value or type it held in `.condarc` before resolving to that string, is not a failure mode at all; if it survives the "Rule" above and does not correspond to a real installable package, it fails later at solve time exactly as any other made-up package name would.
- An empty Effective Package Set is not a failure under FR-002: environment creation must succeed with zero installed packages. `solve_packages_empty_input_returns_no_records` and the US1-AS2 end-to-end scenario (research.md's Test strategy) verify this directly.

## Non-goals

- Two Per-Invocation entries sharing a bare name with each other (as opposed to with a Default entry): governed entirely by `allez oneshot`'s own existing solver path, out of this contract's scope (spec.md Assumptions).
- Two Resolved Default Package Set entries sharing a bare name with each other: neither entry is dropped or merged by this contract; both reach the existing solver path directly, exactly like two same-named Per-Invocation entries above.
- Any notion of a project- or repository-local configuration distinct from the invoking user's own `~/.condarc`: out of scope (spec.md Assumptions) — this contract only ever consults that one file.
- Whether `create_default_packages` itself parses the way real conda parses it: covered by GEN-36's own conformance suite (`tests/condarc_conformance.rs`) at the `condarc` crate level; this contract governs only what `allez` does with an already-parsed value.
- This contract's `.condarc` scope is exactly one setting: `create_default_packages`. Every other `.condarc` setting — named below or not, whatever its own purpose — is out of scope for this contract without exception; a setting's absence from the illustrative list below is never itself meaningful, since the rule above already excludes it. Representative categories, not an exhaustive enumeration: channel selection/expansion (e.g. `channels`, `default_channels`, `channel_alias`, `allowlist_channels`/`denylist_channels`, `custom_channels`) — `channel_config`'s own contract, unaffected; solver policy (e.g. `channel_priority`, `solver`, `sat_solver`); package-set filtering (e.g. `pinned_packages`, `aggressive_update_packages`, `disallowed_packages`, `track_features`); environment/platform/network settings (e.g. `default_python`, `subdir`, virtual-package overrides, `ssl_verify`, `offline`, `proxy_servers`, repodata caching/format settings). This contract governs only the top-level Effective Package Set — which packages are requested — never how the solver weighs, filters, or fetches them, regardless of which other `.condarc` setting might otherwise seem adjacent to that concern.
- A per-invocation way to suppress the resolved Default Package Set entirely for one invocation is out of this contract's scope: FR-003/FR-004 define only add and single-entry-supersede semantics, never a bulk override. A caller who wants this edits `~/.condarc` directly, the same mechanism spec.md's Assumptions section scopes this ticket to.
