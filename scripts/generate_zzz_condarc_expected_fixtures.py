#!/usr/bin/env python3
"""Generate `conformance/condarc/expected/*.json`: the internal
representation conda's real `Context` parses each `conformance/condarc/
valid/*.json` fixture into, once type coercion (boolify/numberify/typify),
alias resolution, and list/dict normalization have all run.

For every `conformance/condarc/valid/<name>.json`, this writes a sibling
`conformance/condarc/expected/<name>.json` mapping each of the fixture's
top-level keys (resolved to its *canonical* parameter name -- see "Alias
resolution" below) to conda's fully-typified value for that key, as plain
JSON (`bool`/`int`/`float`/`str`/`list`/`dict`/`null`).

Deliberately **`invalid/` is out of scope** -- there is no successful
parse to record for a fixture that conda rejects.

## Why this needs its own generator, not just `getattr(context, key)`

Most `Context` parameters (the plain boolish/numeric/list-of-strings/
dict-of-strings/enum ones the other `generate_*.py` scripts exercise) are
`ParameterLoader` descriptors whose typed value *is* the public attribute
-- `context.override_channels_enabled` already gives you the coerced
`bool`, no further processing.

But ~30 parameters are **shadowed**: the raw `ParameterLoader` is stored
under a leading-underscore name (e.g. `_custom_multichannels`), and the
public attribute of the same name minus the underscore
(`custom_multichannels`) is a `@property` that layers *business logic* on
top of the raw typed value -- for `custom_multichannels`/`custom_channels`
that business logic resolves plain channel-name strings into full
`Channel` objects with real (possibly network-dependent) URLs, and even
branches on `on_win`/`self.subdir` (so `context.custom_multichannels`'s
exact shape can differ between an ARM Mac and a Linux CI runner given the
*same* `.condarc`). That's the opposite of what a portable, checked-in
`expected/*.json` fixture needs.

The fix, verified against a real conda install while building this
script: **always** read the value off the `ParameterLoader`'s own
attribute name (`loader._name`, e.g. `_channels`, `_custom_multichannels`)
rather than the public name -- `context._channels` skips the `channels`
@property's `Channel`-object resolution entirely, the same way
`context._custom_multichannels` skips `custom_multichannels`'s. For
parameters that *aren't* shadowed (the vast majority), `loader._name` has
no leading underscore in the first place, so this is a no-op there --
one uniform rule handles both cases. See `docs/condarc_research.md` for
the full write-up (list of shadowed keys, examples of the resulting diff
between raw and resolved values).

## Alias resolution

A fixture may set a deprecated/alternate spelling of a key (e.g.
`whitelist_channels`, `verify_ssl`, `yes` -- see
`generate_alias_multiplekeys_condarc_fixtures.py`). The expected output
uses the **canonical** name as the JSON key, since that's what conda
actually parsed it into internally -- this is also what makes the alias
fixtures' whole point (many spellings, one canonical parameter) visible
in the recorded expected value.

Canonical-name resolution is done by hand (`ALIAS_TO_LOADER_NAME` in
`CONDA_EXPECTED_SCRIPT` below), not via `Configuration.name_for_alias()`
-- that public API has a real gap, confirmed empirically while writing
this script: its default (and only used) mode, `ignore_private=True`,
excludes any parameter whose *primary* loader name is itself
underscore-prefixed. That's exactly the set of shadowed parameters this
script most needs to resolve correctly -- e.g. `name_for_alias("channel")`
returns `None` (not `"channels"`), because `channels`' loader's primary
name is the private `_channels`. `ALIAS_TO_LOADER_NAME` is built directly
from every loader's `_names` (which has no such gap) instead.

## Naming: why `generate_zzz_condarc_expected_fixtures.py`

`make regenerate-condarc-fixtures` runs every `scripts/generate_*.py` in
(lexicographic) glob order. This script must run **strictly last**: it
reads whatever ends up in `conformance/condarc/valid/` after every other
generator has added/removed its own fixtures, so it has to observe their
final state, not a partial one. The `zzz` infix (rather than, say,
restructuring the Makefile to invoke this script as a separate step) is
the smallest-diff way to guarantee that ordering from within the existing
`for script in scripts/generate_*.py` loop -- don't rename this script to
sort earlier than any other `generate_*_condarc_*` script without also
re-checking/fixing that ordering assumption.

Usage:
    python3 scripts/generate_zzz_condarc_expected_fixtures.py
        Regenerates every conformance/condarc/expected/*.json from
        conformance/condarc/valid/*.json (deleting stale ones first).

    python3 scripts/generate_zzz_condarc_expected_fixtures.py --fixture PATH
        Computes and prints PATH's expected representation to stdout
        only -- touches nothing under conformance/condarc/expected/.
        This is what tests/condarc_conformance.rs shells out to (see
        `assert_conda_expected_representation` there), so the test can
        assert conda's *live* behavior for a fixture still matches the
        checked-in expected/*.json without the test run itself ever
        regenerating that file.

Requires a Python interpreter with `conda` importable. In no-argument
mode, resolution order matches `tests/condarc_conformance.rs`'s
`find_conda_python` (see other `generate_*.py` scripts for the identical
helper) -- this mode is a standalone local-dev command, so it must locate
a conda-capable interpreter itself. `--fixture` mode instead just uses
`sys.executable` (see `main()`): it's only ever invoked by
`tests/condarc_conformance.rs`, which has already resolved and verified
one, and runs this script under it directly.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
VALID_DIR = REPO_ROOT / "conformance" / "condarc" / "valid"
EXPECTED_DIR = REPO_ROOT / "conformance" / "condarc" / "expected"

# Fixtures whose JSON root isn't an object have no top-level keys to
# resolve/canonicalize against `Context` -- there's nothing meaningful to
# record. (Only `null_root.json` exists under `valid/` today; kept as a
# substring match rather than an exact filename list so a future
# non-object-root fixture doesn't silently produce a bogus expected file.)
def has_no_keys_to_resolve(doc) -> bool:
    return not isinstance(doc, dict)


# Two `valid/` fixtures exist purely to exercise *this crate's* (lack of a) limit on
# flow-mapping key length, not real conda's -- see docs/condarc_research.md item 21 and
# tests/condarc_conformance.rs's `RUST_ONLY_FIXTURES`. Both use a
# 1023-character raw key, one character past `ruamel.yaml`'s 1024-character "simple key"
# scanner limit, so real conda genuinely raises a `ParserError` for them -- there is no live
# conda value to record. Rather than letting `compute_expected`'s "every valid/ fixture must be
# conda-acceptable, a failure here is a bug in this script" assumption hard-fail the entire
# regeneration run on these two, their `expected/*.json` is hand-authored here once (verified
# against the crate's own `tests/support::adapter::to_expected_json` output at the time these
# fixtures were added) and short-circuits both this script's normal per-fixture conda-oracle
# call and `--fixture` mode. `tests/condarc_conformance.rs`'s `Conda` (and `OpenApi`) checker
# never actually calls into either code path for these two fixtures -- `Checker::check` skips
# them unconditionally, *before* running anything, because their names are listed in that
# module's `RUST_ONLY_FIXTURES` -- this table exists for `make
# regenerate-condarc-fixtures` correctness (so a full regen doesn't delete-then-fail to
# recreate these two `expected/*.json` files) and as a documented, single source of truth for
# what the crate is expected to produce, not because any test currently reads it via the
# `--fixture` path.
CRATE_ONLY_YAML_KEY_LENGTH_FIXTURES: dict[str, dict] = {
    "custom_multichannels_values_accept_key_exceeds_yaml_simple_key_length_limit": {
        "custom_multichannels": {"k" * 1023: ["a"]},
    },
    "dict_of_strings_values_accept_key_exceeds_yaml_simple_key_length_limit": {
        "custom_channels": {"k" * 1023: "val"},
    },
}


# Run inside the conda-oracle interpreter. Mirrors CONDA_CHECK_SCRIPT in
# the other generate_*.py scripts / tests/condarc_conformance.rs, plus
# the canonicalization + shadowed-attribute logic described in this
# script's module docstring.
CONDA_EXPECTED_SCRIPT = r"""
import json
import sys
from collections.abc import Mapping
from enum import Enum

from conda.base.context import Context, context, reset_context
from conda.common.configuration import ParameterLoader

path = sys.argv[1]

try:
    reset_context(search_path=(path,))
    context.validate_all()
except Exception as e:
    print(f"{type(e).__name__}: {e}", file=sys.stderr)
    sys.exit(1)

with open(path) as f:
    doc = json.load(f)

if not isinstance(doc, dict):
    # Nothing to resolve -- caller should not have invoked us with this
    # fixture; treat it as a no-op success with an empty result.
    print(json.dumps({}, indent=2))
    sys.exit(0)

# Every ParameterLoader's own attribute name (e.g. `_channels`,
# `override_channels_enabled`) always yields the raw, pre-business-logic
# typed value when read directly -- `context._channels` skips the
# `channels` @property's Channel-object resolution entirely, the same way
# `context._custom_multichannels` skips `custom_multichannels`'s. So
# resolution only needs one map: every recognized alias name (including
# each parameter's own primary name) -> that parameter's *loader*
# attribute name, built from `loader._names`/`loader._name` directly
# rather than via `Configuration.name_for_alias()` -- that public API
# has a gap (confirmed empirically while writing this script): it
# excludes any parameter whose primary loader name is itself
# underscore-prefixed (`ignore_private=True` is its default and only
# used mode here), which is exactly the set of shadowed parameters this
# script most needs to resolve correctly (e.g. the alias `"channel"`
# for `channels`/`_channels` would otherwise fail to resolve at all).
# Computed the same way ConfigurationType itself discovers parameter
# names (`cls.__dict__.items()`, not a full MRO walk) -- see module
# docstring in generate_zzz_condarc_expected_fixtures.py.
_loaders = {
    name: val for name, val in Context.__dict__.items() if isinstance(val, ParameterLoader)
}
ALIAS_TO_LOADER_NAME = {
    alias: raw_name for raw_name, loader in _loaders.items() for alias in loader._names
}


def canonical_name(raw_loader_name: str) -> str:
    return raw_loader_name[1:] if raw_loader_name.startswith("_") else raw_loader_name


def canonicalize(value):
    if isinstance(value, Enum):
        # Checked before bool/int/float below: all of conda's own
        # Context-parameter enums (SafetyChecks, PathConflict,
        # SatSolverChoice, ChannelPriority, ...) are plain str-valued
        # Enums today, but this ordering means a future IntEnum-valued
        # parameter would still hit this branch (and get `.value`)
        # instead of being misidentified as a plain int by the branch
        # below.
        return value.value
    if isinstance(value, bool) or value is None or isinstance(value, str):
        return value
    if isinstance(value, int):
        # Python ints are unbounded -- conda's own numberify()/typify()
        # do zero bounds-checking (see the
        # numeric_values_accept_numeric_string_bignum_* fixtures, which
        # exist specifically to exercise this: conda happily accepts,
        # say, `repodata_threads: "1e351"`-ish digit strings as a
        # int-typed parameter's value). But serde_json::Value's default
        # `Number` (no `arbitrary_precision` feature enabled -- and nor
        # does this repo's Cargo.toml enable it) can only represent an
        # i64, u64, or f64. A bare JSON integer literal outside that
        # combined range is syntactically valid JSON that Rust's
        # serde_json still can't parse (confirmed empirically: this is
        # exactly what broke `assert_conda_expected_representation` in
        # tests/condarc_conformance.rs before this check existed).
        # Encode as a decimal-digit string instead in that case --
        # mirrors the NaN/Infinity/-Infinity string-encoding for floats
        # just below, for the identical reason: no native
        # strict-JSON-number representation every consumer of this
        # fixture can parse.
        if -(2**63) <= value <= 2**64 - 1:
            return value
        return str(value)
    if isinstance(value, float):
        # `NaN`/`Infinity`/`-Infinity` are what Python's `json` module
        # emits for these by default, but they're not valid JSON tokens
        # (no strict JSON parser -- including Rust's serde_json, the
        # eventual consumer of these fixtures -- accepts them). Encode
        # as the equivalent string instead so this file stays parseable
        # as strict JSON; a comparison against these fixtures needs to
        # special-case these three strings back to float NaN/inf/-inf.
        if value != value:  # NaN != NaN is the portable NaN check
            return "NaN"
        if value == float("inf"):
            return "Infinity"
        if value == float("-inf"):
            return "-Infinity"
        return value
    if isinstance(value, Mapping):
        return {str(k): canonicalize(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [canonicalize(v) for v in value]
    if isinstance(value, (set, frozenset)):
        return sorted(canonicalize(v) for v in value)
    raise TypeError(
        f"generate_zzz_condarc_expected_fixtures.py: don't know how to "
        f"canonicalize {type(value).__name__!r} value {value!r} to JSON -- "
        f"this fixture exercises a Context parameter type this script "
        f"doesn't handle yet"
    )


result = {}
for original_key in doc:
    raw_name = ALIAS_TO_LOADER_NAME.get(original_key)
    if raw_name is None:
        print(
            f"{original_key!r} is not a recognized Context parameter or "
            f"alias -- conda apparently ignored it silently rather than "
            f"parsing it into anything (this fixture may need review)",
            file=sys.stderr,
        )
        sys.exit(1)
    try:
        raw_value = getattr(context, raw_name)
    except AttributeError as e:
        print(
            f"AttributeError resolving {original_key!r} (attr={raw_name!r}): {e}",
            file=sys.stderr,
        )
        sys.exit(1)
    result[canonical_name(raw_name)] = canonicalize(raw_value)

print(json.dumps(result, indent=2, sort_keys=True, allow_nan=False))
"""


def conda_free_env() -> dict[str, str]:
    """Environment with every CONDA*-prefixed variable stripped, so the
    oracle only ever sees the single fixture file passed via search_path."""
    return {k: v for k, v in os.environ.items() if not k.startswith("CONDA")}


def python_has_conda(python: str) -> bool:
    try:
        result = subprocess.run(
            [python, "-c", "import conda"],
            capture_output=True,
            env=conda_free_env(),
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired):
        return False
    return result.returncode == 0


def which(program: str) -> str | None:
    return shutil.which(program)


def find_conda_python() -> str:
    """Mirrors tests/condarc_conformance.rs's find_conda_python()."""
    env_python = os.environ.get("ALLEZ_CONFORMANCE_PYTHON")
    if env_python:
        if python_has_conda(env_python):
            return env_python
        sys.exit(
            f"$ALLEZ_CONFORMANCE_PYTHON={env_python!r} does not have `conda` importable"
        )

    if python_has_conda("python3"):
        return "python3"

    conda_path = which("conda")
    if conda_path:
        conda_dir = os.path.dirname(conda_path)
        for name in ("python3", "python"):
            candidate = os.path.join(conda_dir, name)
            if os.path.isfile(candidate) and python_has_conda(candidate):
                return candidate

    sys.exit(
        "no python interpreter with `conda` importable found (checked "
        "$ALLEZ_CONFORMANCE_PYTHON, `python3` on PATH, and the interpreter "
        "shipped alongside `conda` on PATH) -- this generator requires the "
        "same conda oracle tests/condarc_conformance.rs uses"
    )


def compute_expected(python: str, fixture_path: Path) -> str:
    """Runs CONDA_EXPECTED_SCRIPT against `fixture_path` and returns the
    resulting JSON text (already indented/sorted). Raises SystemExit with
    a clear message on any failure -- every fixture under valid/ has
    already been proven acceptable by the conda oracle elsewhere, so a
    failure here is a bug in this script, not an expected "skip"."""
    fd, tmp_path = tempfile.mkstemp(suffix=".yml")
    try:
        with os.fdopen(fd, "w") as f:
            f.write(fixture_path.read_text())
        result = subprocess.run(
            [python, "-c", CONDA_EXPECTED_SCRIPT, tmp_path],
            capture_output=True,
            text=True,
            env=conda_free_env(),
            timeout=30,
        )
    finally:
        os.unlink(tmp_path)

    if result.returncode != 0:
        sys.exit(
            f"failed to compute expected representation for {fixture_path.name}:\n"
            f"{result.stderr.strip()}"
        )
    return result.stdout


def emit_single_fixture(python: str, fixture_path: Path) -> str:
    """Computes and returns one fixture's expected-representation JSON
    text, *without* reading, writing, or removing anything under
    `EXPECTED_DIR` -- unlike `main()`'s default (no-argument) mode, which
    regenerates the entire checked-in `conformance/condarc/expected/`
    directory from scratch.

    This is what `--fixture PATH` (see `main()`) exposes on the CLI, and
    what `tests/condarc_conformance.rs`'s `assert_conda_expected_
    representation` shells out to: it re-runs the exact same conda-oracle
    logic `main()` uses to *generate* `conformance/condarc/expected/
    <name>.json` in the first place, so the test can assert conda's
    *live* behavior right now still matches that checked-in file, without
    the test itself ever overwriting it (regenerating expected/ on disk
    is a deliberate, explicit `make regenerate-condarc-fixtures` action,
    never a side effect of merely running the test suite).
    """
    doc = json.loads(fixture_path.read_text())
    if has_no_keys_to_resolve(doc):
        return json.dumps({}, indent=2, sort_keys=True) + "\n"
    hand_authored = CRATE_ONLY_YAML_KEY_LENGTH_FIXTURES.get(fixture_path.stem)
    if hand_authored is not None:
        return json.dumps(hand_authored, indent=2, sort_keys=True) + "\n"
    return compute_expected(python, fixture_path)


def main() -> int:
    args = sys.argv[1:]

    if args:
        if len(args) != 2 or args[0] != "--fixture":
            sys.exit(
                "usage: generate_zzz_condarc_expected_fixtures.py [--fixture PATH]\n"
                "\n"
                "  (no arguments)   regenerate every conformance/condarc/expected/*.json\n"
                "                   from conformance/condarc/valid/*.json\n"
                "  --fixture PATH   compute and print PATH's expected representation to\n"
                "                   stdout only -- does not read, write, or remove\n"
                "                   anything under conformance/condarc/expected/. Used by\n"
                "                   tests/condarc_conformance.rs to check conda's live\n"
                "                   behavior against the checked-in expected fixture\n"
                "                   without regenerating it."
            )
        # Unlike the no-argument mode below (a standalone local-dev
        # command, `make regenerate-condarc-fixtures`, which must locate
        # a conda-capable interpreter itself via `find_conda_python()`),
        # `--fixture` mode is only ever invoked by
        # `tests/condarc_conformance.rs`'s `assert_conda_expected_
        # representation`, which has *already* resolved and verified a
        # conda-capable interpreter and is running this very script
        # under it. `sys.executable` -- this process's own interpreter --
        # is therefore always correct here, without re-deriving anything
        # from `$PATH`/`$CONDA`: it sidesteps this script's independent
        # (and, on CI runners whose `conda` launcher lives in a
        # `condabin/` separate from the real interpreter's `bin/`,
        # previously buggy) copy of that PATH-probing logic entirely.
        python = sys.executable
        sys.stdout.write(emit_single_fixture(python, Path(args[1])))
        return 0

    python = find_conda_python()
    print(f"using conda oracle: {python}\n")

    if EXPECTED_DIR.exists():
        stale = sorted(EXPECTED_DIR.glob("*.json"))
        for path in stale:
            path.unlink()
        if stale:
            print(f"removed {len(stale)} previously-generated expected fixture(s)\n")
    EXPECTED_DIR.mkdir(parents=True, exist_ok=True)

    fixtures = sorted(VALID_DIR.glob("*.json"))
    written = 0
    skipped = 0
    for fixture_path in fixtures:
        doc = json.loads(fixture_path.read_text())
        if has_no_keys_to_resolve(doc):
            print(f"SKIPPED {fixture_path.name}  (root is not a JSON object)")
            skipped += 1
            continue

        hand_authored = CRATE_ONLY_YAML_KEY_LENGTH_FIXTURES.get(fixture_path.stem)
        if hand_authored is not None:
            out_path = EXPECTED_DIR / fixture_path.name
            out_path.write_text(json.dumps(hand_authored, indent=2, sort_keys=True) + "\n")
            written += 1
            print(f"WROTE   {fixture_path.name}  (hand-authored -- real conda rejects this fixture, see CRATE_ONLY_YAML_KEY_LENGTH_FIXTURES)")
            continue

        expected_json = compute_expected(python, fixture_path)
        out_path = EXPECTED_DIR / fixture_path.name
        out_path.write_text(expected_json)
        written += 1
        print(f"WROTE   {fixture_path.name}")

    print(f"\n{written} expected fixture(s) written, {skipped} skipped.\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
