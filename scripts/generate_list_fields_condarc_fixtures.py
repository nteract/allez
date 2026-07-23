#!/usr/bin/env python3
"""Generate the exhaustive `list_fields_accept_*` / `list_fields_reject_*`
fixture battery for `list_fields`'s custom `validation=` callable
(docs/condarc_research.md §3, §4.7, §5.9).

`list_fields` is declared as `SequenceParameter(PrimitiveParameter("",
element_type=str), default=DEFAULT_CONDA_LIST_FIELDS,
validation=list_fields_validation)` -- a plain list-of-strings shape
(see `generate_list_of_strings_condarc_fixtures.py` for the 18 *other*
list-of-strings keys), but with the one extra constraint
`list_fields_validation` bolts on. Read straight from
`conda/base/context.py`:

```python
def list_fields_validation(value: Iterable[str]) -> str | Literal[True]:
    if invalid := set(value).difference(CONDA_LIST_FIELDS):
        return (
            f"Invalid value(s): {sorted(invalid)}. "
            f"Valid values are: {sorted(CONDA_LIST_FIELDS)}"
        )
    return True
```

`CONDA_LIST_FIELDS` (`conda/base/constants.py`, docs/
condarc_research.md §5.9) is a fixed, closed set of 25 lowercase keys:
`arch`, `build`, `build_number`, `channel`, `channel_name`,
`constrains`, `depends`, `dist_str`, `features`, `fn`, `license`,
`license_family`, `md5`, `name`, `noarch`, `package_type`,
`requested_spec`, `requested_specs`, `sha256`, `size`, `subdir`,
`timestamp`, `track_features`, `url`, `version`. This is a **set
difference check on the whole *typed* sequence at once**, not a
per-element regex/enum lookup the way `path_conflict`/`safety_checks`
are -- every element must simultaneously be a `CONDA_LIST_FIELDS`
member for the document to be valid; a single unrecognized element
invalidates the entire list (`MultiValidationError`-wrapped, but still
attributable to this one key).

Three consequences worth spelling out:

  1. **Exact, case-sensitive string match, same as every other list-of-
     strings key's element typing** -- `list_fields`'s elements go
     through the ordinary `SequenceParameter(str)` element-typify path
     (plain `str(element)` constructor per docs/condarc_research.md
     §2.1's `"a single concrete type T (not a tuple), T otherwise"`
     row), so element whitespace is preserved exactly like every other
     list-of-strings key's elements (`isinstance(element, str) and
     issubclass(str, str)` special-case in `_typify_data_structure`,
     same mechanism as `channel_alias`'s whole-value preservation --
     see `generate_channel_alias_condarc_fixtures.py`'s module
     docstring). `CONDA_LIST_FIELDS` members are all lowercase with no
     surrounding whitespace, so any casing variant or whitespace-padded
     spelling of an otherwise-valid field name fails the set-membership
     check.
  2. **Duplicates are harmless** -- `set(value)` collapses them before
     the `.difference()` check, so `["name", "name"]` is exactly as
     valid as `["name"]`.
  3. **Non-string elements (bool/int/None) are stringified, not
     rejected at the type-coercion stage** -- same `str(element)`
     mechanism as note 1 -- and then almost certainly fail the
     set-membership check (`"True"`, `"1"`, `"None"` are none of them
     `CONDA_LIST_FIELDS` members), so the observable rejection comes
     from `list_fields_validation` itself, not a type error.

An empty list (`[]`) or an empty dict (`{}`, which -- like every other
`SequenceParameter`-backed key, per docs/condarc_research.md §2.4's
`isiterable()` note -- is accepted as a raw value and typifies to an
empty tuple) both trivially satisfy `set(()).difference(...) == set()`,
so both are valid. A bare JSON `null` is filtered out before `.load()`
ever runs (indistinguishable from the key being entirely absent, same
mechanism as every other sequence/map-typed key), so it's valid too.

The collection-shape battery for *elements* (not the outer list itself)
mirrors `generate_list_of_strings_condarc_fixtures.py`'s finding 5: a
nested *empty* list/dict element typifies cleanly to an empty
tuple/frozendict, which then fails the outer list's per-element
`isinstance(_, str)` check with a clean `MultiValidationError`
(wrapping an `InvalidTypeError`); a nested *non-empty* list/dict element
instead crashes with an unhandled `AttributeError` while trying to
`.typify()` a raw (non-`LoadedParameter`) nested element. Both are still
correctly `invalid/` either way. Separately, giving `list_fields`
itself (the *whole* key, not an element) a non-empty *mapping* raw value
crashes with a different, unrelated `AttributeError` (`'str' object has
no attribute 'value'`, from a different code path than the list-element
crash above) -- included here specifically because its crash signature
differs from every other collection-shape fixture in this suite,
despite testing a structurally similar "wrong container shape" input.

Every candidate (both batteries) is verified empirically against a real
`conda` installation before a fixture is written -- an accept candidate
that's unexpectedly *rejected*, or a reject candidate that's
unexpectedly *accepted*, is reported as SKIPPED rather than silently
written to the wrong directory, so this script is self-correcting if
conda's behavior ever changes.

Usage:
    python3 scripts/generate_list_fields_condarc_fixtures.py

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

KEY = "list_fields"

# The closed CONDA_LIST_FIELDS key set (docs/condarc_research.md §5.9),
# sorted for deterministic fixture content.
CONDA_LIST_FIELDS = sorted(
    [
        "arch",
        "build",
        "build_number",
        "channel",
        "channel_name",
        "constrains",
        "depends",
        "dist_str",
        "features",
        "fn",
        "license",
        "license_family",
        "md5",
        "name",
        "noarch",
        "package_type",
        "requested_spec",
        "requested_specs",
        "sha256",
        "size",
        "subdir",
        "timestamp",
        "track_features",
        "url",
        "version",
    ]
)

# (slug, value) accept candidates -- see module docstring for the exact
# `set(value).difference(CONDA_LIST_FIELDS)` check this battery exercises.
ACCEPT_CANDIDATES: list[tuple[str, object]] = [
    # set(()).difference(...) == set() -- trivially valid.
    ("array_size_0_empty", []),
    # A dict raw value is accepted (isiterable()) and typifies to an
    # empty tuple -- the same "SequenceParameter tolerates a raw
    # mapping" leniency documented for every other list-of-strings key.
    ("object_size_0_empty_treated_as_array", {}),
    # A single valid field name.
    ("single_field_name", ["name"]),
    # A field name from elsewhere in the alphabet (not the documented
    # default battery) -- demonstrates every CONDA_LIST_FIELDS member
    # is individually accepted, not just the four documented defaults.
    ("single_field_sha256", ["sha256"]),
    # A few distinct valid field names together.
    ("multiple_distinct_fields", ["name", "version", "build"]),
    # Duplicate elements are harmless -- set() collapses them before
    # the difference() check.
    ("duplicate_field_names", ["name", "name"]),
    # The documented default value, set explicitly.
    ("default_value_explicit", ["name", "version", "build", "channel_name"]),
    # Every single member of CONDA_LIST_FIELDS at once -- the full
    # closed vocabulary, confirming none of the 25 keys are secretly
    # excluded despite being individually valid.
    ("all_known_fields_at_once", CONDA_LIST_FIELDS),
]

# (slug, value) reject candidates.
REJECT_CANDIDATES: list[tuple[str, object]] = [
    # Case sensitivity: CONDA_LIST_FIELDS members are all lowercase;
    # an uppercase spelling of an otherwise-valid field name is not a
    # member of the set.
    ("case_sensitivity_uppercase_name", ["NAME"]),
    ("case_sensitivity_titlecase_name", ["Name"]),
    # An entirely unrecognized/typo'd field name.
    ("unknown_field_name", ["bogus"]),
    # One valid element alongside one invalid element -- the whole
    # list is still rejected (the check is set-difference over the
    # *entire* sequence, not "at least one element is valid").
    ("mixed_valid_and_invalid_fields", ["name", "bogus"]),
    # Whitespace is preserved (not stripped) for list-of-strings
    # elements, same mechanism as every other list-of-strings key --
    # so a padded spelling of a valid field name doesn't match.
    ("whitespace_padded_field_name", ["  name  "]),
    # Non-string elements are stringified (str(1) == "1", str(True) ==
    # "True", str(None) == "None") rather than rejected at the
    # type-coercion stage -- none of these stringified forms are
    # CONDA_LIST_FIELDS members, so rejection comes from
    # list_fields_validation itself.
    ("element_int_values", [1, 2]),
    ("element_bool_true", [True]),
    ("element_null_alongside_valid", ["name", None]),
    # A bare scalar string instead of a list -- SequenceParameter
    # requires a genuine iterable-shaped raw value at the outer level;
    # a bare string is rejected with a clean InvalidTypeError (per
    # docs/condarc_research.md §2.4, isiterable() is conda's own
    # predicate, not Python's character-by-character str iteration).
    ("bare_scalar_string_not_array", "name"),
    # A non-empty mapping as the *whole* key's raw value crashes with a
    # different AttributeError than every other collection-shape
    # fixture in this suite ('str' object has no attribute 'value',
    # not 'YamlRawParameter' object has no attribute 'typify') --
    # included specifically to document that distinct crash signature.
    ("bare_object_nonempty_distinct_crash", {"a": 1}),
    # Nested *empty* list/dict elements typify cleanly to an empty
    # tuple/frozendict, which then fails the outer list's per-element
    # str-isinstance check cleanly (MultiValidationError wrapping
    # InvalidTypeError) -- no crash.
    ("nested_empty_array_element", [[]]),
    ("nested_empty_object_element", [{}]),
    # Nested *non-empty* list/dict elements instead crash with an
    # unhandled AttributeError while trying to .typify() a raw,
    # non-LoadedParameter nested element (docs/condarc_research.md §8
    # item 7's "emptiness, not shape" distinction, one level down).
    ("nested_nonempty_array_element", [[1]]),
    ("nested_nonempty_object_element", [{"a": 1}]),
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
