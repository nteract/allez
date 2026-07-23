#!/usr/bin/env python3
"""Generate the exhaustive `default_python_accept_*` / `default_python_reject_*`
fixture battery for `default_python`'s custom `validation=` callable
(docs/condarc_research.md §3, §4.9).

`default_python` is declared with a *tuple* `element_type=(str,
NoneType)` (nullable) plus `validation=default_python_validation`. Read
straight from `conda/base/context.py`:

```python
def default_python_validation(value: str) -> str | Literal[True]:
    if value:
        if len(value) >= 3 and value[1] == ".":
            try:
                value = float(value)
                if 2.0 <= value < 4.0:
                    return True
            except ValueError:  # pragma: no cover
                pass
    else:
        # Set to None or '' meaning no python pinning
        return True

    return f"default_python value '{value}' not of the form '[23].[0-9][0-9]?' or ''"
```

Three structural checks, all of which must pass:

  1. `len(value) >= 3` -- so `"3"` (len 1) and `"3."` (len 2) are both
     rejected purely on length, even though `"3."[1] == "."`.
  2. `value[1] == "."` -- the *second character* (index 1) must be a
     literal dot. This means the major-version digit must be exactly
     one character: `"33.9"` fails here (`value[1] == "3"`, not `"."`),
     and so does a leading zero like `"03.9"` (`value[1] == "3"` too --
     the zero occupies index 0, pushing the real digit to index 1).
  3. `2.0 <= float(value) < 4.0` -- note this parses the *entire*
     string as a float, not just a "major.minor" pair, so
     `float("3.10") == 3.1` (not 3.10 as two components) and
     `float("3.999999") == 3.999999` -- **any** number of digits after
     the dot is accepted as long as the resulting float lands in
     `[2.0, 4.0)`, not just the 1-2 digits `settings.rst`'s `'[23].
     [0-9][0-9]?'` description literally suggests. `"3.10.1"` (three
     dot-separated parts) fails at this step: `value[1] == "."` still
     passes, but `float("3.10.1")` raises `ValueError`, caught and
     turned into the same failure message.

**Whitespace IS stripped for this key**, unlike `channel_alias` (see
`generate_channel_alias_condarc_fixtures.py`'s module docstring for the
contrasting case). `default_python`'s `element_type` is the *tuple*
`(str, NoneType)`, not a single, exact `str` class, so
`LoadedParameter._typify_data_structure`'s `isinstance(type_hint, type)`
guard (`conda/common/configuration.py`) is `False` for a tuple -- it
falls through to plain `typify()`, which unconditionally
`.strip()`s any string value before dispatching on the type hint. So
`"  3.9\t\n"` loads as the already-stripped `"3.9"` by the time
`default_python_validation` ever sees it, and is therefore valid.

**Falsy values mean "no pinning" and are unconditionally valid**: both
the empty string `""` and a bare JSON `null` (which round-trips to
Python `None` via the `(str, NoneType)` tuple-coercion branch's
`"none".lower() == "none"` special case) hit the `else: return True`
branch, since both are falsy in the `if value:` check.

Non-string/non-null JSON scalars (`bool`, `int`, `float`) do not error
at the type-coercion stage -- `typify()`'s `{str, NoneType}` tuple
branch (docs/condarc_research.md §2.1) calls `str(value)`
unconditionally (except for the literal `"none"` string), so `True` ->
`"True"`, `0` -> `"0"`, `100` -> `"100"`, and `2.0` (a JSON float) ->
`"2.0"` (which happens to be numerically in-range and structurally
valid, i.e. `float("2.0") == 2.0` and `"2.0"[1] == "."`, so this one
specific float IS accepted -- included in the accept battery for
exactly that reason). Every other stringified non-string scalar in the
reject battery below fails `default_python_validation` itself, not
type coercion.

The collection-shape battery (list/dict raw values, empty vs.
non-empty) mirrors every other `PrimitiveParameter`-backed key's crash/
clean-rejection boundary (docs/condarc_research.md §8 item 7): an
*empty* list/dict fails cleanly with `InvalidTypeError`; a non-empty
one crashes with an unhandled `AttributeError`. Both are still
correctly `invalid/` either way.

Every candidate (both batteries) is verified empirically against a real
`conda` installation before a fixture is written -- an accept candidate
that's unexpectedly *rejected*, or a reject candidate that's
unexpectedly *accepted*, is reported as SKIPPED rather than silently
written to the wrong directory, so this script is self-correcting if
conda's behavior ever changes.

Usage:
    python3 scripts/generate_default_python_condarc_fixtures.py

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

KEY = "default_python"

# (slug, value) accept candidates -- see module docstring for the exact
# three-step `default_python_validation` check this battery exercises.
ACCEPT_CANDIDATES: list[tuple[str, object]] = [
    # Falsy values mean "no pinning" -- unconditionally valid regardless
    # of the length/dot/range checks.
    ("empty_string_means_no_pinning", ""),
    ("null_literal_means_no_pinning", None),
    # Exact lower boundary of the [2.0, 4.0) range.
    ("range_lower_boundary_2_0", "2.0"),
    ("value_2_7", "2.7"),
    ("value_3_0", "3.0"),
    ("value_3_9", "3.9"),
    # A real-world, two-digit-minor Python version string -- notable
    # because float("3.10") == 3.1 (NOT parsed as major=3/minor=10),
    # which is still comfortably < 4.0, so this passes "by accident"
    # relative to what the version string actually means.
    ("value_3_10_float_truncation_quirk", "3.10"),
    # More than two digits after the dot -- settings.rst's own
    # '[23].[0-9][0-9]?' description would suggest this is invalid
    # (at most 2 digits), but the real check just does float(value) and
    # range-checks it, so any digit count that keeps the float in
    # range is accepted. Included specifically to document this gap
    # between the documented-looking pattern and the real check.
    ("value_3_99_three_significant_digits", "3.99"),
    ("value_3_999999_many_digits_still_in_range", "3.999999"),
    # A value extremely close to (but still strictly below) the
    # exclusive upper boundary.
    ("range_upper_boundary_just_below_4_0", "3.9999999999"),
    # Whitespace IS stripped for this tuple-typed key (contrast with
    # channel_alias) -- this loads identically to "3.9".
    ("value_whitespace_padded_tabs_and_newlines", "  3.9\t\n"),
    # A JSON float (not a JSON string) -- str(2.0) == "2.0", which is
    # both length- and range-valid once stringified.
    ("float_2_0_stringifies_in_range", 2.0),
]

# (slug, value) reject candidates.
REJECT_CANDIDATES: list[tuple[str, object]] = [
    # Exact, exclusive upper boundary: float("4.0") == 4.0, which fails
    # the strict "< 4.0" comparison.
    ("range_upper_boundary_exact_4_0", "4.0"),
    # value[1] == "." passes, but the float is below the [2.0, ...)
    # lower boundary.
    ("range_below_lower_boundary_1_9", "1.9"),
    # A major digit above the documented 2/3 range but still matching
    # value[1] == "." -- fails the range check, not the shape check.
    ("range_above_upper_boundary_9_5", "9.5"),
    # len(value) >= 3 fails: "3." is only 2 characters.
    ("length_too_short_trailing_dot_only", "3."),
    # len(value) >= 3 fails: "3" is only 1 character.
    ("length_too_short_bare_digit", "3"),
    # value[1] == "." fails: the second character is "9", not a dot.
    ("no_dot_at_index_1", "39"),
    # value[1] == "." fails: the character at index 1 is a comma.
    ("wrong_separator_comma", "3,9"),
    # A leading zero shifts the real digit to index 1, so value[1] ==
    # "3", not ".".
    ("leading_zero_shifts_dot_position", "03.9"),
    # Two-digit major version -- value[1] == "3", not ".".
    ("two_digit_major_version", "33.9"),
    # value[1] == "." passes, but float("3.a") raises ValueError.
    ("non_numeric_after_dot", "3.a"),
    # value[1] == "." passes, but float("3.10.1") raises ValueError
    # (three dot-separated parts is not a valid float literal).
    ("three_part_version_string", "3.10.1"),
    # value[1] == "." passes, but float("3e0") -- wait, value[1] here
    # is "e", not "." ("3e0"[1] == "e") -- fails the shape check before
    # float() is ever attempted.
    ("scientific_notation_shape_mismatch", "3e0"),
    # A negative-looking string: value[0] == "-", value[1] == "3", not
    # ".", so it fails the shape check outright.
    ("leading_minus_sign_shifts_dot_position", "-3.5"),
    # bool/int values are stringified (str(True) == "True", str(0) ==
    # "0", str(100) == "100") rather than rejected at the type-coercion
    # stage -- all three then fail default_python_validation's own
    # shape/length checks, not type coercion.
    ("bool_true_coerces_to_string", True),
    ("bool_false_coerces_to_string", False),
    ("int_zero_coerces_to_string", 0),
    ("int_hundred_coerces_to_string", 100),
    # Collection-shape battery (docs/condarc_research.md §8 item 7):
    # empty list/dict fails cleanly; non-empty list/dict crashes with
    # an unhandled AttributeError. Both are still correctly invalid.
    ("array_empty", []),
    ("array_nonempty", [1, 2]),
    ("object_empty", {}),
    ("object_nonempty", {"a": 1}),
]

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


def generate_accept_battery(python: str) -> tuple[list[str], list[tuple[str, str]]]:
    prefix = f"{KEY}_accept_"
    clear_stale(VALID_DIR, prefix)

    written: list[str] = []
    skipped: list[tuple[str, str]] = []
    for slug, value in ACCEPT_CANDIDATES:
        doc = {KEY: value}
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


def generate_reject_battery(python: str) -> tuple[list[str], list[str]]:
    prefix = f"{KEY}_reject_"
    clear_stale(INVALID_DIR, prefix)

    written: list[str] = []
    unexpectedly_valid: list[str] = []
    for slug, value in REJECT_CANDIDATES:
        doc = {KEY: value}
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

    print(f"--- {KEY}_accept_* ---\n")
    written_accept, skipped_accept = generate_accept_battery(python)
    print(
        f"\n{len(written_accept)} fixture(s) written, {len(skipped_accept)} "
        "candidate(s) skipped (accept).\n"
    )

    print(f"--- {KEY}_reject_* ---\n")
    written_reject, skipped_reject = generate_reject_battery(python)
    print(
        f"\n{len(written_reject)} fixture(s) written, {len(skipped_reject)} "
        "candidate(s) skipped (reject).\n"
    )

    total_written = len(written_accept) + len(written_reject)
    total_skipped = len(skipped_accept) + len(skipped_reject)
    print(f"=== total: {total_written} fixture(s) written, {total_skipped} skipped ===")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
