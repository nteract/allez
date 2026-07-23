#!/usr/bin/env python3
"""Generate the exhaustive `local_repodata_ttl_reject_*` fixture battery.

Sibling of `generate_local_repodata_ttl_fixtures.py` (the *valid*
battery) -- see that script's docstring, and docs/condarc_research.md §8
item 11, for why `local_repodata_ttl` gets its own dedicated, single-key
battery instead of living in the shared `boolish_values_accept_*`/
`boolish_values_reject_*` battery.

Every candidate here is a value that is a perfectly valid `boolify()`
token/coercion for the *other* five boolish keys, but fails specifically
for `local_repodata_ttl` because it routes through a different, narrower
coercion function (`typify_str_no_hint`, a hand-rolled regex table) --
plus the two type-agnostic collection-crash cases (§8 item 7) that apply
here too, for completeness of this key's own battery.

Every candidate is still verified empirically against a real `conda`
installation before a fixture is written -- if a candidate unexpectedly
turns out to be *accepted*, it is reported as SKIPPED rather than
silently written to `invalid/`.

Usage:
    python3 scripts/generate_local_repodata_ttl_reject_fixtures.py

Requires a Python interpreter with `conda` importable; see the module
docstring of `generate_local_repodata_ttl_fixtures.py` for resolution
order.
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
INVALID_DIR = REPO_ROOT / "conformance" / "condarc" / "invalid"
FIXTURE_PREFIX = "local_repodata_ttl_reject_"

KEY = "local_repodata_ttl"

# (slug, value) -- see the module docstring for the general shape of why
# each of these fails specifically for local_repodata_ttl.
CANDIDATES: list[tuple[str, object]] = [
    # BOOLISH_TRUE/BOOLISH_FALSE's single-letter forms exist in
    # boolify()'s own vocabulary, but _Regex.BOOLEAN_TRUE/.BOOLEAN_FALSE
    # (`^true$|^yes$|^on$` / `^false$|^no$|^off$`) have no single-letter
    # forms -- stays an unmatched `str`, fails the final (bool, int)
    # isinstance check cleanly (InvalidTypeError).
    ("string_short_true_token", "y"),
    ("string_short_false_token", "n"),
    # The empty string is a BOOLISH_FALSE token in boolify() itself, but
    # matches no _Regex pattern here either -- same clean rejection.
    ("string_empty", ""),
    # "non"/"none"/"null" all fail here too, but via two different
    # sub-mechanisms worth keeping distinct: "non" matches no _Regex
    # pattern at all (stays the str "non"); "none"/"null" match
    # _Regex.NONE (`^none$|^null$`) and become the *actual* Python `None`
    # -- still not in (bool, int), but a structurally different typed
    # result than "non"'s unmatched string. Both representative tokens
    # are kept.
    ("string_non_token", "non"),
    ("string_null_token", "null"),
    # A complex-number-shaped string: boolify()'s final bool(complex(val))
    # fallback would accept this for the other five keys, but
    # local_repodata_ttl's _Regex.COMPLEX pattern matches it *and actually
    # converts it* to a real `complex` value -- which still isn't in
    # (bool, int) (successfully coerced to the wrong type, rather than
    # failing to coerce at all).
    ("string_complex_number", "1+2j"),
    # PEP 515 underscore-separated numeral: valid complex()/float() syntax
    # (so boolify() succeeds for the other five keys), but _Regex.INT has
    # no underscore support -- stays an unmatched `str` here.
    ("string_underscored_int", "1_000"),
    # A decimal-looking string: _Regex.FLOAT matches and converts it to an
    # actual `float` -- also not in (bool, int); same "coerced to the
    # wrong type" shape as the complex-number case above, distinct
    # mechanism from the two unmatched-string cases.
    ("string_decimal_string", "1.0"),
    # A raw JSON float -- BOOL_COERCEABLE_TYPES (used by boolify() for the
    # other five keys) includes `float`, but local_repodata_ttl's (bool,
    # int) tuple has no float member, and typify_str_no_hint never even
    # gets a chance to run its regex since the *raw* value is already a
    # float, not a string -- str(1.5) = "1.5" still matches _Regex.FLOAT
    # and converts to an actual float, same wrong-type outcome.
    ("float", 1.5),
    # A hex-literal-shaped string: not parseable by complex() (unlike the
    # complex-number/underscored-int/decimal-string cases above), and
    # local_repodata_ttl's _Regex.HEX matches it and calls the builtin
    # `hex()` function on the *string* itself -- a genuine conda crash bug
    # (`_Regex.HEX`/`.OCT`/`.BIN` store `hex`/`oct`/`bin`, which convert
    # int-to-string, backwards from what's needed here). Raises an
    # unhandled `TypeError: 'str' object cannot be interpreted as an
    # integer` -- see docs/condarc_research.md §8 item 9.
    ("string_hex_literal", "0x1A"),
    # A bare YAML/JSON null literal. Not in (bool, int) -- and unlike the
    # boolify()-based keys (where str(None).lower() == "none" is itself a
    # BOOLISH_FALSE token, so None round-trips to False), local_repodata_ttl
    # never calls boolify() at all: typify_str_no_hint(str(None)) =
    # typify_str_no_hint("None") *does* match _Regex.NONE
    # (case-insensitive) and becomes the actual Python `None` -- still not
    # in (bool, int), so this is INVALID here despite being VALID for the
    # other five boolish keys (see docs/condarc_research.md §8 item 11).
    ("null_literal", None),
    # Collections -- not specific to local_repodata_ttl (every
    # PrimitiveParameter-backed key behaves this way, §8 item 7), but
    # included for completeness of this key's own standalone battery.
    ("array_empty", []),
    ("array_nonempty", [1, 2, 3]),
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


def main() -> int:
    python = find_conda_python()
    print(f"using conda oracle: {python}\n")

    INVALID_DIR.mkdir(parents=True, exist_ok=True)

    stale = sorted(INVALID_DIR.glob(f"{FIXTURE_PREFIX}*.json"))
    for path in stale:
        path.unlink()
    if stale:
        print(f"removed {len(stale)} previously-generated fixture(s)\n")

    written = []
    unexpectedly_valid = []
    for slug, value in CANDIDATES:
        doc = {KEY: value}
        is_valid, reason = check_candidate(python, doc)
        filename = f"{FIXTURE_PREFIX}{slug}.json"
        if not is_valid:
            out_path = INVALID_DIR / filename
            out_path.write_text(json.dumps(doc, indent=2) + "\n")
            written.append(filename)
            print(f"WROTE   {filename}  (value={value!r}): {reason[:100]}")
        else:
            unexpectedly_valid.append(filename)
            print(
                f"SKIPPED {filename}  (value={value!r}): unexpectedly ACCEPTED "
                "by conda -- this candidate belongs in the valid battery instead"
            )

    print(
        f"\n{len(written)} fixture(s) written, {len(unexpectedly_valid)} "
        "candidate(s) skipped (unexpectedly valid)."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
