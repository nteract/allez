#!/usr/bin/env python3
"""Generate the `channel_priority_boolish_accept_*` /
`channel_priority_boolish_reject_*` fixture batteries -- coverage for
`channel_priority`'s historical boolean-compat shim (`ChannelPriorityMeta`,
docs/condarc_research.md §2.3, §5.1) that is conspicuously **absent**
from every other battery in this repo.

## Why this battery needs to exist as its own thing

`channel_priority` is *not* a member of `generate_boolish_condarc_
fixtures.py` / `..._reject_fixtures.py`'s shared `KEYS` list (nor
`PLAIN_BOOL_KEYS`, `NULLABLE_BOOL_KEYS`, nor `SSL_VERIFY_KEY`) -- so its
bool-compat shim has never actually been exercised by any existing
fixture in this repo, despite looking superficially "boolish."

Worse, even a naive attempt to fold it into that shared battery would
be *wrong*: `ChannelPriorityMeta.__call__` (docs/condarc_research.md
§2.3) does **not** route through `boolify()` the way the five genuinely
boolish keys (`always_yes`, `report_errors`, `show_channel_urls`,
`use_only_tar_bz2`, `ssl_verify`) do. It instead calls the plain,
no-type-hint `typify()` (`typify_str_no_hint`) on any *string* value --
the exact same narrow, hand-rolled `_Regex` table
(`conda/auxlib/type_coercion.py`) that `local_repodata_ttl` uses (see
docs/condarc_research.md §8 item 6, and this repo's
`generate_local_repodata_ttl_fixtures.py`) -- then only treats the
result as the shim's `True`/`False` if `typify()` happened to produce
an *actual* Python `bool`. Concretely:

    class ChannelPriorityMeta(EnumMeta):
        def __call__(cls, value, *args, **kwargs):
            try:
                return super().__call__(value, *args, **kwargs)
            except ValueError:
                if isinstance(value, str):
                    value = typify(value)  # no-hint regex-guess
                if value is True:
                    value = "flexible"
                elif value is False:
                    value = cls.DISABLED
                return super().__call__(value, *args, **kwargs)

`typify_str_no_hint`'s boolean regexes are `_Regex.BOOLEAN_TRUE =
r'^true$|^yes$|^on$'` / `_Regex.BOOLEAN_FALSE = r'^false$|^no$|^off$'`
(case-insensitive, via `re.IGNORECASE`) -- a strict subset of
`boolify()`'s own `BOOLISH_TRUE = ("true", "yes", "on", "y")` /
`BOOLISH_FALSE = ("false", "off", "n", "no", "non", "none", "")`.
Empirically confirmed against real conda while writing this script:
every value that is universally valid across the shared boolish
battery *except* for this narrower regex vocabulary --
raw ints/numeric strings (`boolify()`'s `BOOL_COERCEABLE_TYPES`/
`str.isnumeric()` paths, which `typify_str_no_hint` has no equivalent
of outside its own separate `_Regex.INT`/`.FLOAT`/`.COMPLEX` patterns --
none of which feed the shim's `is True`/`is False` check, since those
patterns produce `int`/`float`/`complex`, not `bool`), the single-letter
tokens `"y"`/`"n"`, and the `BOOLISH_FALSE`-only tokens `"non"`/`"none"`
-- are all **rejected** for `channel_priority` specifically, unlike
every other boolish key. `null` (a raw, non-string value) is rejected
too, since the shim's `isinstance(value, str)` guard skips it entirely.

Modeled here exactly like `generate_local_repodata_ttl_fixtures.py` /
`..._reject_fixtures.py`: a dedicated, single-key battery, verified
candidate-by-candidate against a real `conda` installation.

Usage:
    python3 scripts/generate_channel_priority_boolish_fixtures.py

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
ACCEPT_PREFIX = "channel_priority_boolish_accept_"
REJECT_PREFIX = "channel_priority_boolish_reject_"

KEY = "channel_priority"

# (slug, value) -- the shim's actual, narrow accepted vocabulary. Casing
# is spelled out per this repo's convention (macOS' default filesystem
# is case-insensitive, so slugs must be filesystem-unique even
# case-folded) even though the underlying regex is itself
# case-insensitive.
ACCEPT_CANDIDATES: list[tuple[str, object]] = [
    ("bool_true", True),
    ("bool_false", False),
    ("string_true_lower", "true"),
    ("string_true_upper", "TRUE"),
    ("string_true_firstupper", "True"),
    ("string_true_mixedcase", "tRuE"),
    ("string_yes_lower", "yes"),
    ("string_yes_upper", "YES"),
    ("string_yes_firstupper", "Yes"),
    ("string_on_lower", "on"),
    ("string_on_upper", "ON"),
    ("string_on_firstupper", "On"),
    ("string_false_lower", "false"),
    ("string_false_upper", "FALSE"),
    ("string_false_firstupper", "False"),
    ("string_no_lower", "no"),
    ("string_no_upper", "NO"),
    ("string_no_firstupper", "No"),
    ("string_off_lower", "off"),
    ("string_off_upper", "OFF"),
    ("string_off_firstupper", "Off"),
    # typify()'s unconditional value.strip() for the no-hint string path
    # (docs/condarc_research.md §2.1) applies here too.
    ("string_whitespace_padded", " true "),
    ("string_whitespace_padded_tabs_and_newlines", "\t\nyes\n\t"),
]

# (slug, value) -- values that are valid boolish tokens for every OTHER
# boolish key (via boolify()) but are specifically rejected for
# channel_priority, because its shim's typify_str_no_hint() vocabulary
# is narrower than boolify()'s. See module docstring for the mechanism
# behind each group. (`""`/`null` are deliberately NOT repeated here --
# they're already covered, via a different/simpler mechanism (no member
# matches, full stop), by generate_enum_condarc_fixtures.py's
# channel_priority_reject_string_empty.json / _null_literal.json.)
REJECT_CANDIDATES: list[tuple[str, object]] = [
    # BOOL_COERCEABLE_TYPES truthiness -- valid for every boolify()-based
    # key; the shim never even calls typify() on a non-string raw value,
    # so plain ints just fail the enum's normal value/name lookup.
    ("int_zero", 0),
    ("int_one", 1),
    ("int_two", 2),
    ("int_negative_one", -1),
    # str.isnumeric() -- boolify()'s numeric-string path; typify_str_no_
    # hint's _Regex.INT matches these too, but converts to a plain int
    # (not bool), so the shim's `is True`/`is False` check still misses.
    ("numeric_string_zero", "0"),
    ("numeric_string_one", "1"),
    ("numeric_string_two", "2"),
    ("numeric_string_negative_one", "-1"),
    # BOOLISH_TRUE/BOOLISH_FALSE's single-letter forms -- boolify() has
    # always accepted these; _Regex.BOOLEAN_TRUE/.BOOLEAN_FALSE has no
    # single-letter forms, so these stay unmatched strings.
    ("string_short_true_token", "y"),
    ("string_short_false_token", "n"),
    # BOOLISH_FALSE-only tokens (not BOOLEAN_FALSE tokens) -- "non"
    # matches no _Regex pattern at all; "none" matches _Regex.NONE and
    # becomes actual Python None (still not True/False).
    ("string_non_token", "non"),
    ("string_none_token", "none"),
    # boolify()'s final bool(complex(val)) fallback accepts all of
    # these for the other boolish keys; typify_str_no_hint's _Regex.
    # FLOAT/.COMPLEX match and convert to real float/complex values
    # instead (not bool), and _Regex.INT has no underscore support at
    # all.
    ("float_truthy", 1.5),
    ("string_complex_number", "1+2j"),
    ("string_underscored_int", "1_000"),
    ("string_decimal_string", "1.0"),
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
        print(f"removed {len(stale)} previously-generated {prefix!r} fixture(s)\n")


def main() -> int:
    python = find_conda_python()
    print(f"using conda oracle: {python}\n")

    VALID_DIR.mkdir(parents=True, exist_ok=True)
    INVALID_DIR.mkdir(parents=True, exist_ok=True)

    print(f"--- {ACCEPT_PREFIX}* ---\n")
    clear_stale(VALID_DIR, ACCEPT_PREFIX)
    accept_written = []
    accept_skipped = []
    for slug, value in ACCEPT_CANDIDATES:
        doc = {KEY: value}
        is_valid, reason = check_candidate(python, doc)
        filename = f"{ACCEPT_PREFIX}{slug}.json"
        if is_valid:
            (VALID_DIR / filename).write_text(json.dumps(doc, indent=2) + "\n")
            accept_written.append(filename)
            print(f"WROTE   {filename}  (value={value!r})")
        else:
            accept_skipped.append((filename, reason))
            print(f"SKIPPED {filename}  (value={value!r}): {reason}")
    print(
        f"\n{len(accept_written)} fixture(s) written, {len(accept_skipped)} "
        "candidate(s) skipped (unexpectedly rejected by conda's shim).\n"
    )

    print(f"--- {REJECT_PREFIX}* ---\n")
    clear_stale(INVALID_DIR, REJECT_PREFIX)
    reject_written = []
    reject_skipped = []
    for slug, value in REJECT_CANDIDATES:
        doc = {KEY: value}
        is_valid, reason = check_candidate(python, doc)
        filename = f"{REJECT_PREFIX}{slug}.json"
        if not is_valid:
            (INVALID_DIR / filename).write_text(json.dumps(doc, indent=2) + "\n")
            reject_written.append(filename)
            print(f"WROTE   {filename}  (value={value!r}): {reason[:100]}")
        else:
            reject_skipped.append(filename)
            print(
                f"SKIPPED {filename}  (value={value!r}): unexpectedly ACCEPTED "
                "by conda's shim -- this candidate belongs in the accept "
                "battery instead"
            )
    print(
        f"\n{len(reject_written)} fixture(s) written, {len(reject_skipped)} "
        "candidate(s) skipped (unexpectedly accepted by conda's shim).\n"
    )

    total_written = len(accept_written) + len(reject_written)
    total_skipped = len(accept_skipped) + len(reject_skipped)
    print(f"=== total: {total_written} fixture(s) written, {total_skipped} skipped ===")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
