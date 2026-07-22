# Data Model: CLI Scaffold and Subcommand Routing

**Feature**: [spec.md](./spec.md) | **Scope**: parsing/routing only — no persistence, no filesystem or network side effects.

All entities below are transient, in-process parse results. None are stored, serialized to disk, or shared across invocations. Validation rules come directly from the spec's Functional Requirements (FR-XXX).

## Entities

### Subcommand

One of the six named operations the top-level `allez` command routes to.

| Field | Type | Notes |
|---|---|---|
| `name` | enum discriminant | One of `Oneshot`, `Create`, `Run`, `Sandbox`, `List`, `Remove` (FR-001) |
| `usage` | static string | One-line usage description surfaced in `allez --help` (FR-001) and expanded per-subcommand help (FR-002) |
| `args` | subcommand-specific struct | See below — each subcommand's own required/optional argument shape |

**Validation rules**:
- Exactly six variants exist; no others are recognized (FR-001, FR-010).
- Subcommand names are case-sensitive; an unrecognized name (including wrong-case) is a usage error, exit code `2` (FR-010, FR-011).

### Package Reference

A package name string supplied to `oneshot` or `create`.

| Field | Type | Notes |
|---|---|---|
| `name` | `String` | Opaque token as typed by the caller; not validated against a registry or resolved in this ticket |

**Validation rules**:
- Zero or more may be supplied (FR-003, FR-004); an empty list is valid, not an error.
- No format/version-constraint parsing occurs here (deferred to condarc/resolution tickets, GEN-23/GEN-24).

### Environment Path

The target location argument supplied to `create`, `run`, or `remove`.

| Field | Type | Notes |
|---|---|---|
| `path` | `String` (carried opaquely; not yet a validated filesystem path) | Required, exactly one, non-empty (FR-004, FR-005, FR-008, FR-015) |

**Validation rules**:
- Required and must be non-empty for `create`, `run`, `remove` — a missing or empty value is a usage error, exit code `2` (FR-004, FR-005, FR-008, FR-010, FR-015).
- Non-emptiness is enforced by one shared, named validator (`parse_nonempty_path(s: &str) -> Result<String, String>` in `src/cli/mod.rs`, wired as each field's clap `value_parser`) applied identically by all three subcommands — not three independent per-subcommand checks — mirroring the `validate_pass_through()` sharing pattern below (FR-015).
- Existence on disk is NOT checked at this layer (deferred to later tickets that implement real environment operations).

### Pass-Through Command

The command and its own arguments captured after the `--` separator, for `oneshot`, `run`, and `sandbox`. A single shared struct (`program: String`, `args: Vec<String>`) backs all three subcommands — see "Required-ness enforcement" below for why this is safe despite `oneshot`/`run` requiring it and `sandbox` not.

| Field | Type | Notes |
|---|---|---|
| `program` | `String` | First token after `--` |
| `args` | `Vec<String>` | Remaining tokens after `--`, captured verbatim (including anything that looks like a flag) |

**Validation rules**:
- Required (non-empty) for `oneshot` and `run` — `--` present with nothing after it, or `--` absent entirely, is a usage error, exit code `2` (FR-003, FR-005, FR-010).
- For `sandbox`, the empty case is split by whether a literal `--` token was present at all (FR-006): `--` absent entirely → not an error, routes to the distinct "interactive subshell" stub path; `--` present with nothing after it → usage error, exit code `2`, same `missing_pass_through_command` category as `oneshot`/`run`'s empty-`--` case. This split cannot be made from the parsed `PassThroughArgs` value alone — see "Required-ness enforcement" below.
- Tokens after `--` are never interpreted as allez's own flags (FR-012); they pass through byte-for-byte to the `program`/`args` split.

**Required-ness enforcement (clap attribute vs. post-parse validation)**: The `program`/`args` field carries **no** `required = true` attribute at the type level, because `oneshot`, `run`, and `sandbox` all flatten the *same* `PassThroughArgs` struct (research.md Decision 1's DRY rationale) — clap's `#[command(flatten)]` cannot apply a `required` attribute to only some of the sites that flatten a given struct. Marking the field required in the type definition would silently make it required for `sandbox` too, breaking FR-006. Instead, a shared `validate_pass_through(pt: &PassThroughArgs) -> Result<(), AllezError>` function (defined in `src/cli/mod.rs`) is called from **`main.rs`'s dispatch, immediately after `Cli::try_parse()` succeeds and before invoking `oneshot`'s or `run`'s stub-handler function at all** — not from inside those handler bodies. This is deliberate, not incidental: FR-010 requires that a malformed invocation be rejected "before invoking any stub handler's core logic," so the validation must happen at the dispatch layer, where rejection and successful routing are structurally mutually exclusive, rather than as the first statement inside a handler that has technically already been entered.

`sandbox`'s dispatch arm does **not** reuse `validate_pass_through()` as-is, because that function only inspects `pt.program.is_empty()` — it has no way to tell "no `--` at all" apart from "`--` present, nothing after it," and clap's own `ArgMatches` state for a `#[arg(last = true)]` field is identical in both cases (empirically confirmed against `clap` 4.6.x: `contains_id`, `value_source`, `index_of`, and `get_many` all report "not present" for both). Since FR-006 now requires these two cases to be handled differently, `sandbox`'s dispatch arm instead calls a distinct `sandbox_missing_command(pt: &PassThroughArgs, raw_args: &[OsString]) -> Result<(), AllezError>`-shaped check that, only when `pt.program` is empty, additionally scans the invocation's raw argv (`std::env::args_os()`) for a literal `"--"` token to decide which of FR-006's two branches applies. This scan is scoped to running only inside the `Sandbox` match arm — at that point exactly one subcommand (`sandbox`) has been dispatched for the whole process invocation, so any `--` token found anywhere in argv can only belong to `sandbox`'s own trailing pass-through-command position (allez's global flags — `--human`, `--verbose`, `--help`, `--version` — are none of them capable of containing a literal `--` token as part of their own value), making a full-argv scan safe without needing to slice out "just the sandbox portion" of argv by hand.

## Output Contract (cross-cutting, not a domain entity)

Every subcommand's stub handler acknowledgment (FR-009, FR-014) is emitted through one of two formatters selected by `--human` (FR-013):

| Mode | Destination | Shape (this ticket) |
|---|---|---|
| Machine-readable (default) | stdout | `{"schema_version": "0.1.0-unstable", "subcommand": "<name>", "status": "stub", "parsed": {...}}` — the four outer keys are the normatively fixed minimal shape (FR-014); `parsed`'s internal fields are subcommand-specific (e.g. `{"packages": [...], "path": "..."}` for `create`) |
| Human-readable (`--human`) | stdout | One-line text confirming which subcommand was reached and its parsed arguments, carrying the same facts as the JSON `parsed` object above |

**`--human` placement (FR-013)**: this is one global flag on the top-level `Cli`, valid before or after the subcommand name, but — per FR-012's absolute invariant — never after `--`; a token after `--` is always forwarded to the pass-through command verbatim, never reinterpreted as an allez flag. There is no `--format`/`--json` flag and no conflict rule to reconcile, since `--human` is the sole flag governing format selection; omitting it always yields JSON — see spec.md's Clarifications for why a TTY-based default was considered and rejected (it cannot reliably distinguish an agent caller from a human one).

**Pass-through argv redaction (FR-016)**: for `oneshot`, `run`, and `sandbox`, the `parsed.pass_through` field (or human-readable equivalent) is **redacted by default**: `{"program": "<redacted>", "arg_count": N}`, never the caller's actual `program`/`args` values, because those may carry secrets (e.g. `allez sandbox -- curl -H "Authorization: Bearer <token>"`). A `--verbose`/`-v` flag (see research.md §5) switches this to the full unredacted `{"program": "...", "args": [...]}` for interactive debugging. `packages` and `path` are never redacted — they are structural identifiers, not caller-supplied free-form command content.

Errors (usage errors, exit code `2`) follow the same two-mode split and are always written to **stderr** — both the human-readable message and the machine-readable (default) error body, matching stdout's reservation for the success-path result payload (data-model.md/contracts/cli-schema.md's "stdout = result, stderr = diagnostics/errors" convention) and the human-error convention this ticket already establishes. The error body MUST include a `category` field drawn from one fixed, closed enum for this ticket — `missing_argument`, `unknown_subcommand`, `unknown_flag`, `missing_pass_through_command` (FR-011, FR-017) — in addition to the human-readable message, not folded into the message text: `{"schema_version": "0.1.0-unstable", "category": "missing_argument", "message": "..."}`.

## State Transitions

None. Parsing and routing for this ticket is a single-pass, stateless operation: `argv → parse → validate → route to stub handler (or fail with usage error)`. No entity here persists past process exit.
