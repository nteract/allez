# CLI Command Contract: `allez`

**Feature**: [spec.md](../spec.md) | **Type**: command-line interface (no network/library API in this ticket)

This is the user/agent-facing contract this scaffold ticket establishes: the six subcommands, their argument shapes, help text expectations, output-format selection, and the exit-code convention. Downstream tickets (GEN-23 through GEN-30) implement the real behavior behind each stub; they MUST NOT change this contract's shape without a new spec. The `--` separator rule below (FR-012) is one consistent, cross-cutting parsing invariant applied identically to `oneshot`, `run`, and `sandbox` — not bespoke per-subcommand logic — even though each subcommand's table is documented separately for readability.

## Top-Level Command

```text
allez [--human] [--verbose] [--help] [--version] <SUBCOMMAND> [SUBCOMMAND ARGS]
allez                              # no subcommand → usage/help to stderr, exit 2
allez --help                       # lists all 6 subcommands + usage, exit 0
allez <subcommand> --help          # per-subcommand help, exit 0
```

- Machine-readable JSON is the default when `--human` is omitted; `--human` selects human-readable, free-form output instead (FR-013). There is no `--format`/`--json` flag and no context-aware (TTY-based) default — see Output Format Contract below for why.
- `--human` is one global flag on the top-level `Cli`. It MAY appear before the subcommand name (`allez --human list`) **or** after the subcommand name but before any `--` separator (`allez list --human`, `allez oneshot pkg1 --human -- echo hi`) — both placements are equally valid and behave identically. Per FR-012, it MUST NOT appear after `--`: a token positioned after `--` is always part of the pass-through command, never reinterpreted as an allez flag, regardless of what it looks like (see `oneshot`/`run`/`sandbox` tables below).
- Format selection applies uniformly across all six subcommands' own success/error acknowledgments, including error output. It does NOT apply to `--help`/`--version` output (see below).
- `--verbose`/`-v` (see Output Format Contract below) is a separate global flag controlling whether pass-through command content is redacted, independent of `--human`.
- `--version`/`-V` prints the crate version and exits `0` (FR-018).
- **`--help`/`--version` are exempt from format selection**: this output is generated directly by the argument-parsing library and bypasses `output.rs` entirely; it is always plain text, never JSON, regardless of `--human` or the JSON default (FR-013's Scope exclusion).

## Subcommands

### `oneshot`

```text
allez oneshot [PACKAGE...] -- <COMMAND> [COMMAND ARGS...]
```

- `PACKAGE...`: zero or more package name tokens (FR-003).
- `--`: required literal separator.
- `<COMMAND> [COMMAND ARGS...]`: required pass-through command; missing → usage error, exit `2`.

| Invocation | Result |
|---|---|
| `allez oneshot pkg1 pkg2 -- echo hi` | routes to `oneshot` stub with packages=[pkg1,pkg2], command=echo, args=[hi] (redacted by default, see Output Format Contract); exit `0` |
| `allez oneshot -- echo hi` | routes to `oneshot` stub with packages=[]; exit `0` |
| `allez oneshot pkg1 --human -- echo hi` | `--human` placed before `--` selects human-readable output for the whole invocation; exit `0` |
| `allez oneshot pkg1 -- echo hi --human --unusual-flag` (both tokens after `--`) | both `--human` and `--unusual-flag` are forwarded verbatim as arguments to `echo`, per FR-012 — NOT parsed as allez flags, NOT a usage error; exit `0` |
| `allez oneshot pkg1 --` | usage error: missing pass-through command; exit `2` |
| `allez oneshot pkg1 echo hi` (no `--`) | usage error: pass-through command not delimited; exit `2` |

### `create`

```text
allez create <PATH> [PACKAGE...]
```

- `<PATH>`: required, exactly one, non-empty — rejected by the shared `parse_nonempty_path` validator (FR-004, FR-015).
- `PACKAGE...`: zero or more package name tokens.

| Invocation | Result |
|---|---|
| `allez create ./my-env pkg1 pkg2` | routes to `create` stub with path=./my-env, packages=[pkg1,pkg2]; exit `0` |
| `allez create ./my-env` | routes to `create` stub with packages=[]; exit `0` |
| `allez create` | usage error: missing path; exit `2` |
| `allez create ""` | usage error: empty path rejected by `parse_nonempty_path`, same `missing_argument` category as a missing path; exit `2` (FR-015) |

### `run`

```text
allez run <PATH> -- <COMMAND> [COMMAND ARGS...]
```

- `<PATH>`: required, exactly one, non-empty — rejected by the shared `parse_nonempty_path` validator (FR-005, FR-015).
- `--` and pass-through command: required, same rules as `oneshot`.

| Invocation | Result |
|---|---|
| `allez run ./my-env -- echo hi` | routes to `run` stub with path=./my-env, command=echo, args=[hi] (redacted by default); exit `0` |
| `allez run ./my-env` (no `--`) | usage error: missing pass-through command; exit `2` |
| `allez run -- echo hi` | usage error: missing path; exit `2` |
| `allez run "" -- echo hi` | usage error: empty path rejected by `parse_nonempty_path`; exit `2` (FR-015) |

### `sandbox`

```text
allez sandbox [-- <COMMAND> [COMMAND ARGS...]]
```

- `--` and pass-through command: optional (FR-006). Absent (no `--` token at all) → routes to a distinct "interactive subshell" stub path (not an error). Present with zero tokens after it (`allez sandbox --`) → usage error, `category: missing_pass_through_command`, exit `2` — the same category `oneshot`/`run` use for their own required-but-empty case; there is no plausible intent behind supplying `--` with nothing after it, so this is no longer treated as equivalent to omitting `--` entirely.

| Invocation | Result |
|---|---|
| `allez sandbox -- python -c "print(1)"` | routes to `sandbox` stub with command=python, args=[-c, print(1)] (redacted by default); exit `0` |
| `allez sandbox` | no `--` token present anywhere — routes to `sandbox` stub, interactive-subshell path; exit `0` |
| `allez sandbox --` (nothing after) | usage error: `--` present but no pass-through command follows it; exit `2` (`category: missing_pass_through_command`, per FR-006 — distinct from `allez sandbox`'s no-separator case above) |

### `list`

```text
allez list
```

- No positional arguments (FR-007).

| Invocation | Result |
|---|---|
| `allez list` | routes to `list` stub; exit `0` |
| `allez list --bogus-flag` | usage error: unknown flag; exit `2` |

### `remove`

```text
allez remove <PATH>
```

- `<PATH>`: required, exactly one, non-empty — rejected by the shared `parse_nonempty_path` validator (FR-008, FR-015).

| Invocation | Result |
|---|---|
| `allez remove ./my-env` | routes to `remove` stub with path=./my-env; exit `0` |
| `allez remove` | usage error: missing path; exit `2` |
| `allez remove ""` | usage error: empty path rejected by `parse_nonempty_path`; exit `2` (FR-015) |

## Exit Code Convention (FR-011)

| Code | Meaning |
|---|---|
| `0` | Success — includes reaching any subcommand's stub handler with valid, parsed arguments |
| `2` | Usage error — unrecognized subcommand, unknown flag, missing required argument, missing/empty pass-through command |

No other exit codes are defined by this ticket. Finer-grained error category (e.g., "which specific argument was missing") travels in the human-readable message and/or the default JSON error body, not in the exit code.

## Output Format Contract (FR-013, FR-014, FR-016, FR-017)

- Default (JSON, whenever `--human` is omitted): a JSON object to stdout for success, a JSON object to **stderr** for usage errors — this is a fixed, normative part of the contract (not an implementation choice): stdout is reserved exclusively for the primary result payload; all diagnostics and errors, human or JSON, go to stderr.
  - **Success payload** MUST be `{"schema_version": "0.1.0-unstable", "subcommand": "<name>", "status": "stub", "parsed": {...}}` — these four outer keys are the normatively fixed minimal shape (FR-014); `parsed`'s internal fields are subcommand-specific. This satisfies the constitution's "documented, versioned schema" requirement from this ticket onward and is precise enough that two independent implementations of the same handler produce test-comparable output. Fields *inside* `parsed` stay minimal until each subcommand's real behavior lands in its own later ticket, at which point a stable (non-`"-unstable"`) `schema_version` is committed (see [data-model.md](../data-model.md#output-contract-cross-cutting-not-a-domain-entity)).
  - **Error payload** MUST be `{"schema_version": "0.1.0-unstable", "category": "<one of the fixed enum below>", "message": "..."}` — per FR-011, FR-017, and constitution Principle XI. `category` is drawn from one fixed, closed enum for this ticket's usage-error surface: `missing_argument`, `unknown_subcommand`, `unknown_flag`, `missing_pass_through_command`. Extending this enum in a later ticket is additive; renaming/removing an existing value is a breaking contract change requiring a new spec.
  - **Pass-through argv redaction** (FR-016): for `oneshot`/`run`/`sandbox`, `parsed`'s pass-through field is `{"program": "<redacted>", "arg_count": N}` by default — never the caller's real `program`/`args` — because pass-through arguments may carry secrets (e.g. `allez sandbox -- curl -H "Authorization: Bearer <token>"`). A `--verbose`/`-v` global flag switches this to the full unredacted `{"program": "...", "args": [...]}`, in both human and JSON modes, for interactive debugging. `packages` and `path` fields are never redacted.
- `--human`: free-form text to stdout for success, free-form text to stderr for usage errors — carrying the same facts as the JSON shape above.
- **No `--format`/`--json` flag and no context-aware default**: JSON is the unconditional default rather than a TTY-detected one, specifically because a caller's terminal-attachment state cannot reliably distinguish an automated/agent caller from a human one (agent/coding-assistant frameworks routinely run subprocesses through a pseudo-terminal for unrelated reasons — color/streaming support — which would make `is_terminal()`-based detection misclassify an agent caller as human). `--human` is the only way to opt into human-readable output; there is no boolean shorthand and no flag-conflict rule to reconcile, since there is only one flag.
- **`--human` placement**: valid before or after the subcommand name, never after `--` (see Top-Level Command above). A token identical to `--human`/`-v` placed after `--` is simply forwarded to the pass-through command as an ordinary argument, per FR-012 — not a usage error, not specially detected.

## Forward-Looking Constraints for Downstream Tickets

This contract's data model (Pass-Through Command, Environment Path) intentionally captures untyped/unvalidated strings in this ticket, since no real environment or process operations exist yet. The constraints below are **not** requirements on GEN-22 itself — they bind whichever later ticket (GEN-25, GEN-26, GEN-27, GEN-28) implements real behavior against this contract's captured data, so that the safety properties implied by the current design aren't silently lost during implementation:

- **Pass-through command execution** (GEN-25 `oneshot`, GEN-26 `run`, GEN-28 `sandbox`): the captured `program`/`args` (`Vec<String>`) MUST be executed via direct argv APIs (e.g. Rust's `std::process::Command::new(program).args(args)`). Implementations MUST NOT join `program`+`args` into a single shell command string or invoke a shell (`sh -c`, `cmd /c`) to run it, unless the user's stated intent is explicitly to launch a shell as the target program itself. This preserves the "verbatim, byte-for-byte, never allez-parsed" guarantee this ticket already establishes for pass-through tokens (FR-012) all the way through to actual execution.
- **Environment Path filesystem safety** (GEN-24 ephemeral core, GEN-26 `create`/`run`, GEN-27 `remove`): the captured `path: String` MUST be converted to `PathBuf` (per constitution Principle VII) and canonicalized before any filesystem-touching operation. At minimum, whatever policy is documented MUST: canonicalize before acting; reject (not silently follow) a target that resolves through a symlink outside the originally-specified path; and reject absolute paths escaping the intended working area unless the caller explicitly opted in. This minimum applies with particular force to `remove`, where following an unexpected symlink or traversal segment could destroy data outside the intended target — a later ticket's implementation is not free to define this minimum away while still claiming compliance with this contract.
- **Argv/log data exposure in real logging** (beyond this ticket's own stub redaction, FR-016, which is already normative for GEN-22 itself — see Output Format Contract above): once GEN-25/26/28 add real logging around actual command execution, that logging MUST NOT echo full pass-through argv verbatim at default (non-debug) log levels; gate full-argv logging behind an explicit debug flag or apply redaction for recognizable secret patterns. This upgrades the prior draft's "SHOULD NOT" to a binding MUST NOT, consistent with FR-016 already making the equivalent behavior mandatory for this ticket's own stub output.

## Out of Scope for This Contract

- Actual package resolution, environment creation/teardown, sandboxing, or condarc parsing — all deferred to GEN-23 through GEN-30.
- Path-existence validation, package-name validation against a registry.
- Any additional subcommands, aliases, or global flags beyond `--help`, `--version`, `--human`, `--verbose`/`-v`.
