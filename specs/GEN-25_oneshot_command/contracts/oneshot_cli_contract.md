# Interface Contract: `allez oneshot` CLI surface

This feature exposes a **CLI subcommand contract**: exit codes, JSON/
human stdout/stderr shapes, and the `tracing` observability schema
extension — not a new Rust library API (that's GEN-23/GEN-24's contracts,
consumed here unmodified). This is what an automated caller (spec.md's
Operating Context: an AI agent, not a human at a terminal) can rely on.

## Invocation shape (unchanged from GEN-22's existing scaffold)

```text
allez oneshot [PACKAGES]... -- <COMMAND> [ARGS...]
```

- `PACKAGES`: zero or more package-name tokens before `--`. Zero is valid
  (FR-001's Acceptance Scenario 2) — resolves to the configured default/
  override package set, not an error.
- `-- <COMMAND> [ARGS...]`: required (FR-011); rejected as a usage error
  (exit `2`, category `missing_pass_through_command`) if `--` is absent,
  or present with nothing after it — enforced at the dispatch layer by the
  existing `cli::validate_pass_through`, unchanged by this ticket.

## Exit codes (FR-006)

| Code | Meaning | Caller-facing category (stderr) | FR-012 observability category |
|---|---|---|---|
| `0` | Pass-through command started and exited `0`. | N/A | N/A |
| `1..=255` (pass-through's own code) | Pass-through command started and exited normally with that code. **Authoritative once the command starts** — even if it happens to equal one of the values below (FR-006's own explicit caveat). | None — FR-013: no envelope at all once started. | None (a normal exit carries no `failure_category`). |
| `1` | Environment creation failed (FR-010), activation of an already-created environment failed, **or** registering a signal listener failed (this plan's `activation_failed`/`signal_setup_failed`; see `research.md`). | One of `unresolvable_package`, `integrity_verification_failed`, `unwritable_location`, `no_channels_configured`, `activation_failed`, or `signal_setup_failed`. `teardown_failed` never appears here — it appears only as `cleanup_category` in the dual-failure case below, alongside one of the other four `EphemeralEnvError` categories. | Same category, mirrored. |
| `2` | Usage error (FR-011) — no `--`, or `--` with nothing after it. | `missing_pass_through_command`. | N/A — this case never reaches `oneshot::run` at all; covered instead by `main.rs`'s own pre-existing, schema-versioned usage-rejection record (see Observability contract below). |
| `126` | Pass-through program found but could not be executed (FR-008). | `pass_through_not_executable`. | Same category, mirrored. |
| `127` | Pass-through program's name could not be found (FR-008). | `pass_through_not_found`. | Same category, mirrored. |
| `128 + N` | Pass-through program terminated by signal `N`, on a platform where that concept exists (FR-007). | **None** — the command already started, so FR-013 forbids any caller-facing message or category for this outcome; `pass_through_terminated_by_signal` exists *only* as an FR-012 observability category, never on stderr. | `pass_through_terminated_by_signal`. |

Every other subcommand's own exit-code surface is unaffected (FR-006).

## stdout/stderr contract

**Before the pass-through program starts** (usage error, environment-
creation failure, or pass-through-not-found/not-executable/activation
failure): identical to every other existing `allez` subcommand's failure
convention — `output::render_error` (or the new, additive `output::
render_ephemeral_creation_failure` for the dual-failure case below) on
stderr; stdout is empty. JSON is the default; `--human` selects the
human-readable form. Shape (JSON default):

```json
{"schema_version": "0.1.0-unstable", "category": "unresolvable_package", "message": "could not resolve package `nonexistent-pkg-xyz`"}
```

**FR-010 dual-failure case** (creation failed *and* its own rollback also
failed) — additive, optional fields, so this remains backward-compatible
with every existing single-failure JSON consumer that only reads
`category`/`message`:

```json
{
  "schema_version": "0.1.0-unstable",
  "category": "unwritable_location",
  "message": "ephemeral environment location is unwritable",
  "cleanup_category": "teardown_failed",
  "cleanup_message": "ephemeral environment teardown failed"
}
```

`--human` mode renders both via one line, reusing `CreationFailure`'s own
existing `Display` impl (GEN-24) verbatim — it already produces `"{error}
(cleanup also failed: {cleanup})"` — so no separate human-mode dual-field
logic is needed; only the JSON path needs the two new optional fields.

**Once the pass-through program has successfully started** (FR-013):
`allez` writes no further caller-facing result payload of its own to
stdout or stderr, ever again, for this invocation. The pass-through
program's own stdout/stderr — streamed live, each stream kept separate
(FR-004) — *is* the entire visible output; `allez`'s own final exit code
is the entire machine-actionable signal. This is the one documented
exception to Constitution III's dual-format convention (spec.md FR-013
itself says so) — there is no JSON/human rendering to reconcile because
there is no separate `allez`-authored result payload at all for this
outcome. The `RUST_LOG`-gated `tracing` channel (Observability contract,
below) is not this result payload: it is silent unless the caller
explicitly opts in via `RUST_LOG`, and even then carries only the fixed,
schema-versioned `OneshotOutcomeEvent` fields — never a second copy of,
or a substitute for, the pass-through program's own output or exit code.

## Observability contract (FR-012, additive extension)

The "rejected as a usage error, never attempted" case (no `--`, or `--`
with nothing after it) is already covered by `main.rs`'s existing
`exit_on_invalid_pass_through`/`tracing::warn!(operation, category, ...)`
call site — that rejection happens before `oneshot::run` (and therefore
before any `EnvironmentId` exists to correlate a new event by) is ever
reached, so it needs no new event type. This ticket adds one small,
additive field to that pre-existing call, `schema_version` (the same
value `OneshotOutcomeEvent` below uses), so the record satisfies FR-012's
"carrying its own documented schema version" clause explicitly; no
correlation identifier is added, since a rejected invocation never
produces more than this one record, and FR-012's correlation requirement
exists to tie multiple records together.

For every invocation that passes usage validation, exactly one `tracing`
event (in addition to whatever `ephemeral::mod.rs`'s own `create`/
`install`/`teardown` events already emit — unchanged by this ticket):

1. Immediately after `create_ephemeral_environment` resolves — carries
   `pass_through_started: false` if it failed (with `failure_category`/
   `message`, plus `cleanup_category`/`cleanup_message` for the dual-
   failure case). Emitted only on failure; a success is not separately
   recorded here, since the terminal event below already reports
   `pass_through_started: true` for that case, and this ticket emits
   exactly one event per invocation that reaches `oneshot::run`, never
   two.
2. Immediately after the pass-through program's own outcome is known
   (started-and-exited, started-and-signaled, or could-not-start) —
   `pass_through_started: true` unless the failure is `NotFound`/
   `NotExecutable`/`ActivationFailed`/`SignalSetupFailed`, all four of
   which are pre-start.

Exactly one of the two above ever fires for a given invocation — never
both, and never during the pass-through program's own execution. Both are
emitted strictly outside the FR-004 streaming window — never while the
pass-through program is running — per `OneshotOutcomeEvent`
(`data-model.md`). `RUST_LOG` gates visibility identically to every other
`tracing::*!` call site already in this codebase (`observability.rs`,
unchanged): silent unless explicitly set, matching the existing "never
interleave with the single-JSON-object stderr error contract" guarantee
`tests/cli_scaffold.rs`'s `t063`/`t063a`/`t063b` already lock in for other
subcommands.

## Non-goals of this contract

- No change to `create`/`list`/`remove`'s own exit-code/JSON surface.
- No change to `run`/`sandbox`'s pass-through behavior — this ticket wires
  only `oneshot`; `pass_through.rs`'s functions are written as plain
  `pub(crate)` building blocks, but wiring `run`/`sandbox` to them is
  explicitly out of this ticket's scope, and no future consumer is
  assumed.
- No environment-removal/teardown surface of any kind (FR-009) — GEN-24
  exposes no environment-removal API at all for this ticket to call.
- No new CLI flag — `oneshot`'s argument shape (`PackagesAndCommandArgs`)
  is unchanged from GEN-22's existing scaffold. `ALLEZ_CONDARC_PATH` (see
  `research.md` § Test strategy) is a test-only internal environment
  variable, gated behind the non-default `test-config-override` Cargo
  feature — a release build of `allez` has no code path that reads it at
  all, and it is not part of this contract.
