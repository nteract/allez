#!/usr/bin/env python3
"""Generate the `ssl_verify_passthrough_reject_*` fixture battery.

Sibling of `generate_ssl_verify_passthrough_fixtures.py` (the *valid*
battery) -- see that script's docstring for the full mechanics of
`boolify()`'s `return_string=True` passthrough and
`ssl_verify_validation`'s exact checks.

Every candidate here is a string that:

  1. `boolify()` itself fails to resolve to a real `bool` for (not
     boolish-true/false, not numeric, not `complex()`-parseable) --
     so it survives via the `return_string=True` passthrough instead of
     raising `TypeCoercionError`, and
  2. then fails `ssl_verify_validation`'s own check: it's not the exact
     string `"truststore"`, and `os.path.exists(value)` is `False`.

This is the key structural difference from the *other* boolish keys'
own invalid batteries (`generate_boolish_condarc_reject_fixtures.py`):
for the tuple-typed nullable keys (`always_yes`, `report_errors`,
`show_channel_urls`, `use_only_tar_bz2`), an unparseable string like
`"banana"` crashes with an unrelated `TypeError` from
`LoadedParameter.typify()`'s buggy `issubclass()` call
(docs/condarc_research.md §8 item 8) -- but for `ssl_verify` specifically,
`return_string=True` means `boolify()` never raises `TypeCoercionError` in
the first place, so that crash path is never even reached. Every
rejection here is instead a clean `CustomValidationError` straight out of
`ssl_verify_validation` -- worth its own fixtures precisely because the
*mechanism* differs from every other boolish key sharing the same
"arbitrary word" input.

Every candidate is still verified empirically against a real `conda`
installation before a fixture is written -- if a candidate unexpectedly
turns out to be *accepted* (e.g. because some CI runner happens to have
a file at the guessed-nonexistent path), it is reported as SKIPPED
rather than silently written to `invalid/`.

Usage:
    python3 scripts/generate_ssl_verify_passthrough_reject_fixtures.py

Requires a Python interpreter with `conda` importable; see the module
docstring of `generate_ssl_verify_passthrough_fixtures.py` for
resolution order.
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
FIXTURE_PREFIX = "ssl_verify_passthrough_reject_"

KEY = "ssl_verify"

# (slug, value) -- see the module docstring for why each of these
# survives boolify()'s passthrough but still fails ssl_verify_validation.
CANDIDATES: list[tuple[str, object]] = [
    # An ordinary English word: not boolish, not numeric, not
    # complex()-parseable, and (short of extraordinary bad luck) not a
    # real path on the filesystem either. Passes through boolify()
    # unchanged, then fails ssl_verify_validation's os.path.exists()
    # check cleanly -- no crash, unlike the same input applied to the
    # tuple-typed nullable boolish keys (see module docstring).
    ("string_arbitrary_word", "banana"),
    # The *string* "null" -- deliberately distinct from the bare JSON/
    # YAML `null` literal (which boolifies to a real `False` for every
    # boolish key including ssl_verify, since str(None).lower() ==
    # "none", itself a BOOLISH_FALSE token -- see
    # docs/condarc_research.md §8 item 11's ssl_verify addendum). The
    # *string* "null" reaches the same str(value).strip().lower() call
    # already lowercased as "null", which is NOT a BOOLISH_FALSE token
    # (only "none" is -- they're spelled differently) and isn't
    # complex()-parseable either, so it passes through unchanged and
    # then fails the filesystem-existence check (there is essentially
    # never a real file/dir literally named "null" at conda's cwd).
    # This is the single nuance the shared boolish battery cannot
    # exercise, since `local_repodata_ttl` was already pulled out of
    # that shared KEYS list for an unrelated reason (item 11) -- this
    # dedicated battery is what actually isolates it.
    ("string_null_token", "null"),
    # A syntactically path-shaped string that (by construction) doesn't
    # exist anywhere -- demonstrates the rejection is genuinely an
    # os.path.exists() check, not merely "is this a recognizable word".
    (
        "string_nonexistent_path",
        "/definitely/does/not/exist/allez-conformance-ssl-verify-fixture-9f3ac2",
    ),
    # ssl_verify_validation's "truststore" special-case is an exact,
    # case-sensitive string comparison (`value != "truststore"`) -- the
    # value returned by boolify()'s passthrough is the *original*,
    # only-outer-stripped value, not lowercased (casing normalization
    # happens on a separate internal `val` variable used only for
    # boolify()'s own numeric/complex probes). So a differently-cased
    # spelling doesn't match the special case, and (assuming no real
    # path is spelled "TRUSTSTORE" at conda's cwd) falls through to the
    # same clean os.path.exists() rejection as any other arbitrary word.
    ("truststore_wrong_case", "TRUSTSTORE"),
    # A hex-literal-shaped string. For local_repodata_ttl's (bool, int)
    # element_type this exact shape triggers a genuine conda crash bug
    # (_Regex.HEX storing the builtin hex() function backwards --
    # docs/condarc_research.md §8 item 9). ssl_verify never reaches that
    # code path at all -- boolify() isn't complex()-parseable for this
    # string, so it passes through unchanged, then fails the ordinary
    # filesystem-existence check just like any other arbitrary string.
    # Included for contrast: same *input shape*, no crash, for this key.
    ("string_hex_literal_shaped", "0x1A"),
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
