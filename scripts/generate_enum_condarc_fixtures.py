#!/usr/bin/env python3
"""Generate the exhaustive `<key>_accept_*` / `<key>_reject_*` fixture
batteries for the four genuinely `Enum`-typed `Context` parameters
(docs/condarc_research.md §2.3, §4, §5.1-§5.4):

    - `channel_priority`  (`ChannelPriority`,  custom `ChannelPriorityMeta`)
    - `path_conflict`     (`PathConflict`)
    - `safety_checks`     (`SafetyChecks`)
    - `sat_solver`        (`SatSolverChoice`)

Per docs/condarc_research.md §2.3, conda's enum coercion tries a
**value**-based lookup first (`type_hint(value)`, matches the member's
lowercase `.value` string), then falls back to a **name**-based lookup
(`type_hint[value]`, matches the member's Python identifier/`.name`) --
both case-sensitive, exact-match only. Two of the four enums
(`PathConflict`, `SafetyChecks`) happen to declare `name == value` for
every member (all lowercase), so they only have one valid spelling per
member; the other two (`ChannelPriority`, `SatSolverChoice`) declare
SHOUTY_CASE member names distinct from their lowercase values, so they
have *two* valid spellings per member (e.g. `"strict"` and `"STRICT"`).

**`channel_priority`'s boolean-compat shim is deliberately excluded
here** -- per the task that commissioned this script, `channel_priority:
true`/`false` and the boolish *strings* that route through the same
shim (`"yes"`/`"no"`/`"on"`/`"off"`/...) are already covered by
`generate_boolish_condarc_fixtures.py` / `..._reject_fixtures.py`'s
`NULLABLE_ONLY_CANDIDATES`-adjacent batteries -- see
`docs/condarc_research.md` §2.3/§5.1. Only `channel_priority`'s genuine
*enum*-member spellings (`"strict"`, `"STRICT"`, `"flexible"`,
`"FLEXIBLE"`, `"disabled"`, `"DISABLED"`) are covered by this script's
accept battery; the shim's `1`/`0` integer non-equivalents
(`1 is not True`, so `channel_priority: 1` is empirically rejected --
see the reject battery) are covered by the reject battery instead.

Accept battery, per key, per enum member:
  - the member's `.value` string (lower-case, e.g. `"strict"`).
  - the member's `.name` string, when it differs from `.value` (e.g.
    `"STRICT"`) -- skipped when `name == value` (`PathConflict`,
    `SafetyChecks`) to avoid a duplicate fixture.
  - one whitespace-padded value-string case per key (empirically
    confirmed: unlike the boolish keys' well-documented `strip()`
    behavior, this isn't spelled out in docs/condarc_research.md §2.1's
    dispatch table for the Enum branch specifically, but was verified
    against real conda while writing this script -- leading/trailing
    whitespace around an otherwise-valid value string is silently
    accepted).

Reject battery, per key:
  - wrong-casing spellings that match neither the value nor the name
    (e.g. `"Strict"`/titlecase for every key; `"CLOBBER"`/uppercase for
    the two `name == value` keys, since uppercasing their value doesn't
    coincidentally land on a valid name the way it does for the other
    two keys).
  - an unrecognized/typo'd string (`"bogus"`).
  - the empty string.
  - a bare JSON `null`.
  - wrong JSON *type* for the three non-`ChannelPriority` keys (`true`,
    an int) -- included specifically to contrast with
    `channel_priority`'s own bool-compat shim, which uniquely *does*
    accept `true`/`false` (see the module docstring above); for
    `channel_priority` itself these are already exercised as *valid*
    fixtures by the boolish generator, so no reject fixture is written
    for them here.
  - the "extra fields" / collection-shape battery (docs/
    condarc_research.md §8 item 7): a scalar-typed (`PrimitiveParameter`)
    key given a raw YAML/JSON list or map instead of a bare scalar. A
    *non-empty* list/map crashes real conda with an unhandled
    `AttributeError` (`'YamlRawParameter' object has no attribute
    'typify'`); an *empty* list/map instead fails cleanly with
    `InvalidTypeError`. Both shapes are still correctly rejected
    (non-zero exit) either way, so both are included, mirroring
    `generate_local_repodata_ttl_reject_fixtures.py`'s
    `array_empty`/`array_nonempty`/`object_empty`/`object_nonempty`
    quartet -- empirically re-confirmed for all four enum keys while
    writing this script.

Every candidate (both batteries) is verified empirically against a real
`conda` installation before a fixture is written -- an accept candidate
that's unexpectedly *rejected*, or a reject candidate that's
unexpectedly *accepted*, is reported as SKIPPED rather than silently
written to the wrong directory, so this script is self-correcting if
conda's behavior ever changes.

Usage:
    python3 scripts/generate_enum_condarc_fixtures.py

Requires a Python interpreter with `conda` importable. Resolution order
matches `tests/condarc_conformance.rs`'s `find_conda_python`:
$ALLEZ_CONFORMANCE_PYTHON, then `python3` on PATH, then the interpreter
shipped alongside whatever `conda` executable is on PATH.
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
INVALID_DIR = REPO_ROOT / "conformance" / "condarc" / "invalid"

# ---------------------------------------------------------------------
# Enum catalog -- (key, [(member_value, member_name), ...]), exactly as
# declared in conda/base/constants.py and recorded in
# docs/condarc_research.md §5.1-§5.4. Order matches each Enum's
# declaration order.
# ---------------------------------------------------------------------
ENUMS: dict[str, list[tuple[str, str]]] = {
    "channel_priority": [
        ("strict", "STRICT"),
        ("flexible", "FLEXIBLE"),
        ("disabled", "DISABLED"),
    ],
    "path_conflict": [
        ("clobber", "clobber"),
        ("warn", "warn"),
        ("prevent", "prevent"),
    ],
    "safety_checks": [
        ("disabled", "disabled"),
        ("warn", "warn"),
        ("enabled", "enabled"),
    ],
    "sat_solver": [
        ("pycosat", "PYCOSAT"),
        ("pycryptosat", "PYCRYPTOSAT"),
        ("pysat", "PYSAT"),
    ],
}

# Keys whose element_type is a *single* Enum class with no bool-compat
# shim (i.e. every ENUMS key except channel_priority) -- these get the
# extra "wrong JSON type" reject candidates (bool / int) that contrast
# with channel_priority's unique shim.
NON_SHIMMED_KEYS = [k for k in ENUMS if k != "channel_priority"]


def accept_candidates_for_key(key: str) -> list[tuple[str, object]]:
    """(slug, value) accept candidates for one enum key: each member's
    value-string, each member's name-string (when distinct), plus one
    whitespace-padded case."""
    candidates: list[tuple[str, object]] = []
    for value, name in ENUMS[key]:
        candidates.append((f"value_{value}", value))
        if name != value:
            candidates.append((f"name_{name}", name))
    # One representative whitespace-padded case per key (first member's
    # value string) -- see module docstring.
    first_value = ENUMS[key][0][0]
    candidates.append(("value_whitespace_padded", f"  {first_value}\t\n"))
    return candidates


def reject_candidates_for_key(key: str) -> list[tuple[str, object]]:
    """(slug, value) reject candidates for one enum key: bad casings,
    an unknown token, empty string, null, collection shapes, and (for
    non-shimmed keys only) wrong-JSON-type bool/int."""
    members = ENUMS[key]
    candidates: list[tuple[str, object]] = []

    for value, name in members:
        # Titlecase never matches value (lowercase) nor name (either
        # identical-lowercase, or SHOUTY_CASE) for any of the four
        # enums -- a uniformly wrong casing across the whole catalog.
        titlecase = value[:1].upper() + value[1:]
        candidates.append((f"casing_titlecase_{value}", titlecase))
        # Fully-uppercased value only misses both value and name for
        # the two keys where name == value (path_conflict,
        # safety_checks); for channel_priority/sat_solver, upper(value)
        # == name, which is already a *valid* candidate, so skip it
        # here to avoid contradicting the accept battery.
        if name == value:
            candidates.append((f"casing_uppercase_{value}", value.upper()))

    candidates.append(("string_unknown_token", "bogus"))
    candidates.append(("string_empty", ""))
    candidates.append(("null_literal", None))

    # "Extra fields" / collection-shape battery -- docs/
    # condarc_research.md §8 item 7. Applies uniformly to every
    # PrimitiveParameter-backed key regardless of element_type shape.
    candidates.append(("array_empty", []))
    candidates.append(("array_nonempty", [members[0][0]]))
    candidates.append(("object_empty", {}))
    candidates.append(("object_nonempty", {"value": members[0][0]}))

    if key in NON_SHIMMED_KEYS:
        # Wrong JSON *type* -- contrasts with channel_priority's unique
        # bool-compat shim (module docstring); channel_priority's own
        # true/false are valid and already covered by the boolish
        # generator, so they're deliberately not reject candidates here.
        candidates.append(("bool_true", True))
        candidates.append(("bool_false", False))
        candidates.append(("int_value", 1))

    return candidates


CONDA_CHECK_SCRIPT = """
import sys
from conda.base.context import reset_context, context

path = sys.argv[1]
try:
    reset_context(search_path=(path,))
    context.validate_all()
except Exception as e:
    print(f"{type(e).__name__}: {e}", file=sys.stderr)
    sys.exit(1)
sys.exit(0)
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

    conda_path = shutil.which("conda")
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


def check_candidate(python: str, doc: dict) -> tuple[bool, str]:
    """Returns (is_valid, reason). reason is empty when valid."""
    text = json.dumps(doc)
    fd, path = tempfile.mkstemp(suffix=".yml")
    try:
        with os.fdopen(fd, "w") as f:
            f.write(text)
        result = subprocess.run(
            [python, "-c", CONDA_CHECK_SCRIPT, path],
            capture_output=True,
            text=True,
            env=conda_free_env(),
            timeout=30,
        )
    finally:
        os.unlink(path)

    if result.returncode == 0:
        return True, ""
    return False, result.stderr.strip()


def clear_stale(directory: Path, prefix: str) -> None:
    stale = sorted(directory.glob(f"{prefix}*.json"))
    for path in stale:
        path.unlink()
    if stale:
        print(f"removed {len(stale)} previously-generated {prefix!r} fixture(s)")


def generate_accept_battery(python: str, key: str) -> tuple[list[str], list[tuple[str, str]]]:
    prefix = f"{key}_accept_"
    clear_stale(VALID_DIR, prefix)

    written: list[str] = []
    skipped: list[tuple[str, str]] = []
    for slug, value in accept_candidates_for_key(key):
        doc = {key: value}
        is_valid, reason = check_candidate(python, doc)
        filename = f"{prefix}{slug}.json"
        if is_valid:
            (VALID_DIR / filename).write_text(json.dumps(doc, indent=2) + "\n")
            written.append(filename)
            print(f"WROTE   {filename}  (value={value!r})")
        else:
            skipped.append((filename, reason))
            print(f"SKIPPED {filename}  (value={value!r}): {reason}")
    return written, skipped


def generate_reject_battery(python: str, key: str) -> tuple[list[str], list[str]]:
    prefix = f"{key}_reject_"
    clear_stale(INVALID_DIR, prefix)

    written: list[str] = []
    unexpectedly_valid: list[str] = []
    for slug, value in reject_candidates_for_key(key):
        doc = {key: value}
        is_valid, reason = check_candidate(python, doc)
        filename = f"{prefix}{slug}.json"
        if not is_valid:
            (INVALID_DIR / filename).write_text(json.dumps(doc, indent=2) + "\n")
            written.append(filename)
            print(f"WROTE   {filename}  (value={value!r}): {reason[:100]}")
        else:
            unexpectedly_valid.append(filename)
            print(
                f"SKIPPED {filename}  (value={value!r}): unexpectedly ACCEPTED "
                "by conda -- this candidate belongs in the accept battery instead"
            )
    return written, unexpectedly_valid


def main() -> int:
    python = find_conda_python()
    print(f"using conda oracle: {python}\n")

    VALID_DIR.mkdir(parents=True, exist_ok=True)
    INVALID_DIR.mkdir(parents=True, exist_ok=True)

    total_written = 0
    total_skipped = 0

    for key in ENUMS:
        print(f"--- {key}_accept_* ---\n")
        written, skipped = generate_accept_battery(python, key)
        total_written += len(written)
        total_skipped += len(skipped)
        print(
            f"\n{len(written)} fixture(s) written, {len(skipped)} candidate(s) "
            f"skipped for {key} (accept).\n"
        )

    for key in ENUMS:
        print(f"--- {key}_reject_* ---\n")
        written, unexpectedly_valid = generate_reject_battery(python, key)
        total_written += len(written)
        total_skipped += len(unexpectedly_valid)
        print(
            f"\n{len(written)} fixture(s) written, {len(unexpectedly_valid)} "
            f"candidate(s) skipped for {key} (reject).\n"
        )

    print(f"=== total: {total_written} fixture(s) written, {total_skipped} skipped ===")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
