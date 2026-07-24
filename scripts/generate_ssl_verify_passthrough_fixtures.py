#!/usr/bin/env python3
"""Generate the `ssl_verify_passthrough_accept_*` fixture battery.

`ssl_verify` is already exercised as one of the shared `KEYS` in
`generate_boolish_condarc_fixtures.py` / `generate_boolish_condarc_reject_
fixtures.py`, but that shared battery only ever probes the tokens that
`boolify()` itself resolves to a real `bool` (`"true"`/`"yes"`/`"1"`/...).
`ssl_verify`'s `element_type=(str, bool)` is unique among the boolish keys
in that `boolify(value, return_string=True)` (`conda/auxlib/type_coercion
.py`) lets any string `boolify()` *fails* to resolve pass through
**unchanged** instead of raising -- see docs/condarc_research.md §2.2 step
6 and §3's `ssl_verify_validation` writeup. That passthrough string then
hits `ssl_verify_validation` (`conda/base/context.py`), which accepts it
only if it's exactly the literal `"truststore"` (Python >= 3.10) or an
`os.path.exists()`-real path. This is the "yuck" case the shared battery
was never built to cover -- this script fills that gap with a small,
dedicated, single-key battery.

Read straight from `conda/auxlib/type_coercion.py`'s `boolify()`:

```python
def boolify(value, nullable=False, return_string=False):
    if isinstance(value, BOOL_COERCEABLE_TYPES):
        return bool(value)
    val = str(value).strip().lower().replace(".", "", 1)
    if val.isnumeric():
        return bool(float(val))
    elif val in BOOLISH_TRUE:
        return True
    elif nullable and val in NULL_STRINGS:
        return None
    elif val in BOOLISH_FALSE:
        return False
    else:
        try:
            return bool(complex(val))
        except ValueError:
            if isinstance(value, str) and return_string:
                return value          # <-- the passthrough
            raise TypeCoercionError(...)
```

Note the returned value on the passthrough branch is the *original*
`value` (only pre-stripped once, by `typify()`, before `boolify()` ever
runs) -- not the lowercased/dot-stripped `val` used internally for the
numeric/complex probes. That's why casing is preserved into
`ssl_verify_validation`'s case-sensitive `value != "truststore"` check
(see `ssl_verify_reject_truststore_wrong_case.json` in the sibling reject
script), and why only *leading/trailing* whitespace is invisible to it
(`ssl_verify_accept_truststore_whitespace_padded.json` /
`..._existing_path_whitespace_padded.json` below).

And straight from `conda/base/context.py`:

```python
def ssl_verify_validation(value: str) -> str | Literal[True]:
    if isinstance(value, str):
        if sys.version_info < (3, 10) and value == "truststore":
            return "`ssl_verify: truststore` is only supported on Python 3.10 or later"
        elif value != "truststore" and not exists(value):
            return (...)
    return True
```

(`exists` = `os.path.exists`.)

Portability note: the "an existing path is accepted" candidates below
deliberately use `"."` (the current working directory), never an
OS-specific absolute path -- `os.path.exists(".")` is `True` on every
platform this repo's CI matrix runs on (Linux/macOS/Windows), unlike e.g.
`"/etc/hosts"` or `"/"`, which aren't portable in the same way. This
keeps the battery hermetic despite `ssl_verify_validation`'s check being
inherently filesystem-dependent (docs/condarc_research.md §3's "not
something a portable, hermetic conformance suite can encode" caveat is
about the *general* case of asserting arbitrary real-path acceptance,
not this specific always-true special case).

Every candidate is still verified empirically against a real `conda`
installation before a fixture is written, for the same self-correcting
reason as the other generators in this directory -- notably, whether
`"truststore"` is accepted at all depends on the oracle interpreter's own
Python version (`sys.version_info >= (3, 10)`), so this script doesn't
hardcode that assumption; it just records whatever the oracle actually
does.

Usage:
    python3 scripts/generate_ssl_verify_passthrough_fixtures.py

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
FIXTURE_PREFIX = "ssl_verify_passthrough_accept_"

KEY = "ssl_verify"

# (slug, value) -- slug must be filesystem-unique even case-folded (macOS'
# default filesystem is case-insensitive), so casing is spelled out in
# words rather than baked into case-sensitive-only filename characters.
CANDIDATES: list[tuple[str, object]] = [
    # The literal "truststore" special-case in ssl_verify_validation --
    # not a boolify() token at all (it isn't complex()-parseable, isn't
    # boolish-true/false, isn't numeric), so it only survives via the
    # return_string=True passthrough, then the exact-string special case
    # in ssl_verify_validation itself (valid only on Python >= 3.10 --
    # verified empirically below, not assumed).
    ("truststore_literal", "truststore"),
    # Same token, but with leading/trailing whitespace: typify()
    # unconditionally strips any string value once, up front, before
    # boolify() ever runs (docs/condarc_research.md §8 item 10) -- so
    # this still resolves to the exact string "truststore" by the time
    # ssl_verify_validation sees it.
    ("truststore_whitespace_padded", "  truststore\t"),
    # "." (the current working directory) always exists, on every OS --
    # a passthrough string that satisfies ssl_verify_validation's
    # os.path.exists() branch without depending on any OS-specific
    # absolute path. Exercises the "directory containing certificates"
    # half of the validator's accepted shapes.
    ("existing_path_current_dir", "."),
    # Same idea, whitespace-padded -- typify()'s outer .strip() reduces
    # this to "." before boolify()/ssl_verify_validation ever see it.
    ("existing_path_whitespace_padded", " . "),
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
        "skipped (not accepted by ssl_verify_validation's passthrough path)."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
