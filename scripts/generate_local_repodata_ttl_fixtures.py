#!/usr/bin/env python3
"""Generate the exhaustive `local_repodata_ttl_accept_*` fixture battery.

`local_repodata_ttl` is boolish-*shaped* (`element_type=(bool, int)`, per
docs/condarc_research.md §2.1/§4.3) but, unlike the other five boolish
keys, does NOT go through `boolify()` -- it routes through
`typify_str_no_hint` (`conda/auxlib/type_coercion.py`), a hand-rolled
regex table (`_Regex`) with a narrower vocabulary. Mixing it into the
shared `boolish_values_accept_*`/`boolish_values_reject_*` battery meant
several individually-valid `boolify()` tokens (`"y"`, `"n"`, `""`, floats,
`"1_000"`, `"1+2j"`, ...) showed up as *rejected* purely because of this
one outlier -- see docs/condarc_research.md §8 item 11 for the full
writeup of why it was pulled out into this standalone, single-key
battery instead.

Unlike the shared boolish battery, fixtures here set *only*
`local_repodata_ttl` (not five/six keys at once) -- there's no
"simultaneously valid for multiple keys" question for a dedicated,
single-key battery.

Every candidate is still verified empirically against a real `conda`
installation before a fixture is written, for the same self-correcting
reason as the other generators in this directory.

Usage:
    python3 scripts/generate_local_repodata_ttl_fixtures.py

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
FIXTURE_PREFIX = "local_repodata_ttl_accept_"

KEY = "local_repodata_ttl"

# (slug, value) -- slug must be filesystem-unique even case-folded (macOS'
# default filesystem is case-insensitive), so casing is spelled out in
# words (`_lower`/`_upper`/`_firstupper`) rather than baked into
# case-sensitive-only filename characters.
CANDIDATES: list[tuple[str, object]] = [
    # Exact bool literals -- isinstance(value, BOOL_COERCEABLE_TYPES) in
    # boolify() would catch these, but local_repodata_ttl doesn't call
    # boolify() at all; typify_str_no_hint(str(True)) = typify_str_no_hint
    # ("True") matches the BOOLEAN_TRUE regex directly.
    ("bool_true", True),
    ("bool_false", False),
    # _Regex.BOOLEAN_TRUE / .BOOLEAN_FALSE (conda/auxlib/type_coercion.py)
    # -- `re.IGNORECASE`, so casing never actually matters, but every
    # casing pattern is exercised explicitly per this repo's convention.
    ("string_true_lower", "true"),
    ("string_true_upper", "TRUE"),
    ("string_true_firstupper", "True"),
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
    # _Regex.INT -- any bare integer, or an integer-shaped string, matches
    # `^[-+]?\\d+$` and converts via `int(...)`.
    ("int_zero", 0),
    ("int_one", 1),
    ("int_two", 2),
    ("int_negative_one", -1),
    ("numeric_string_zero", "0"),
    ("numeric_string_one", "1"),
    ("numeric_string_two", "2"),
    ("numeric_string_negative_one", "-1"),
    # Whitespace-padded tokens -- `typify()` unconditionally strips any
    # string value before dispatching, same as the shared boolish battery
    # (docs/condarc_research.md §8 item 10); local_repodata_ttl's
    # element_type is a tuple (not the single, exact `str` type), so the
    # whitespace-preserving special case never applies here either.
    ("string_whitespace_padded", " true "),
    ("string_whitespace_padded_tabs_and_newlines", "\t\nyes\n\t"),
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

    VALID_DIR.mkdir(parents=True, exist_ok=True)

    stale = sorted(VALID_DIR.glob(f"{FIXTURE_PREFIX}*.json"))
    for path in stale:
        path.unlink()
    if stale:
        print(f"removed {len(stale)} previously-generated fixture(s)\n")

    written = []
    skipped = []
    for slug, value in CANDIDATES:
        doc = {KEY: value}
        is_valid, reason = check_candidate(python, doc)
        filename = f"{FIXTURE_PREFIX}{slug}.json"
        if is_valid:
            out_path = VALID_DIR / filename
            out_path.write_text(json.dumps(doc, indent=2) + "\n")
            written.append(filename)
            print(f"WROTE   {filename}  (value={value!r})")
        else:
            skipped.append((filename, reason))
            print(f"SKIPPED {filename}  (value={value!r}): {reason}")

    print(
        f"\n{len(written)} fixture(s) written, {len(skipped)} candidate(s) "
        "skipped (not valid for local_repodata_ttl)."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
