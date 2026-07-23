#!/usr/bin/env python3
"""Generate the small, fixed battery of document-*root*-shape fixtures --
the handful of fixtures that predated every other `scripts/generate_*.py`
script and were, until now, hand-authored/not owned by any generator:

  - `conformance/condarc/valid/basic_channels.json` -- a basic, realistic
    multi-key `.condarc` smoke test (not an edge case; just "does a normal
    file work at all").
  - `conformance/condarc/valid/empty_object.json` -- `{}`.
  - `conformance/condarc/valid/null_root.json` -- a bare JSON/YAML `null`
    root.
  - `conformance/condarc/invalid/array_root.json` -- a list root.
  - `conformance/condarc/invalid/scalar_root.json` -- a bare string root.

Unlike every other generator in this directory, this one isn't sweeping a
`Context` parameter's coercion boundary -- it's a small, fixed set of
*document-level* shapes, per `docs/condarc_research.md` §1.1 ("Root
type"): a `.condarc`'s root **must be a mapping** for anything useful to
happen, and conda's three non-mapping root shapes each behave differently:

  - An **empty file, or a bare `~`/`null` root** parses to `None` in
    `YamlRawParameter.make_raw_parameters_from_file()`, which is falsy, so
    `make_raw_parameters` returns `EMPTY_MAP` -- **no error, no warning**,
    silently treated as "no settings in this file." Same outcome as
    `empty_object.json`'s `{}` (a real, present-but-empty mapping) -- both
    are valid, for different reasons, hence both are covered.
  - A **list root** iterates the list's *elements* as if they were dict
    keys, then indexes the list with those (string) elements -> an
    unhandled, uncaught `TypeError: list indices must be integers or
    slices, not str`. Not wrapped in `ConfigurationLoadError` or any
    conda-specific exception -- a genuine crash, not a clean rejection
    (same "crash, not clean rejection" shape as the collection-shape and
    null-literal findings elsewhere in this suite -- docs/
    condarc_research.md §8 items 7/8, and item 14 for the numeric keys).
  - A **scalar (string) root** hits the identical code path, iterating the
    string's *characters* as keys and indexing the string with them ->
    `TypeError: string indices must be integers` -- same bug shape,
    different message.

Every candidate is verified empirically against a real `conda`
installation before a fixture is written, for the same self-correcting
reason as every other generator in this directory -- an expected-valid
candidate that's unexpectedly *rejected*, or an expected-invalid candidate
that's unexpectedly *accepted*, is reported as SKIPPED rather than
silently written to the wrong directory.

Usage:
    python3 scripts/generate_root_shape_condarc_fixtures.py

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

# (directory, filename, root_value, expect_valid) -- root_value is the
# literal JSON *document root*, not a key/value pair to nest under a key
# (unlike every other generator in this directory, which always builds a
# `{key: value}` object). A raw Python `None`/`list`/`str` here serializes
# to a bare JSON `null`/array/string document root via `json.dumps`.
CASES: list[tuple[Path, str, object, bool]] = [
    (
        VALID_DIR,
        "basic_channels.json",
        {
            "channels": ["conda-forge", "defaults"],
            "channel_priority": "strict",
            "always_yes": True,
        },
        True,
    ),
    (VALID_DIR, "empty_object.json", {}, True),
    (VALID_DIR, "null_root.json", None, True),
    (INVALID_DIR, "array_root.json", ["a", "b"], False),
    (INVALID_DIR, "scalar_root.json", "just a string", False),
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


def check_candidate(python: str, root_value: object) -> tuple[bool, str]:
    """Returns (is_valid, reason). reason is empty when valid."""
    text = json.dumps(root_value)
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
    INVALID_DIR.mkdir(parents=True, exist_ok=True)

    # This script owns exactly these 5 filenames (by exact name, not a
    # shared prefix like every other generator -- there's no natural
    # common prefix across "basic_channels"/"empty_object"/"null_root"/
    # "array_root"/"scalar_root"). Remove any pre-existing copy before
    # regenerating so a renamed/removed case here doesn't leave orphans.
    for directory, filename, _root_value, _expect_valid in CASES:
        stale = directory / filename
        if stale.exists():
            stale.unlink()

    written: list[str] = []
    mismatches: list[str] = []
    for directory, filename, root_value, expect_valid in CASES:
        is_valid, reason = check_candidate(python, root_value)
        if is_valid == expect_valid:
            (directory / filename).write_text(json.dumps(root_value, indent=2) + "\n")
            written.append(filename)
            print(f"WROTE   {filename}  (expect {'valid' if expect_valid else 'invalid'})")
        else:
            mismatches.append(filename)
            outcome = "ACCEPTED" if is_valid else "REJECTED"
            print(
                f"MISMATCH {filename}: expected "
                f"{'valid' if expect_valid else 'invalid'}, conda {outcome} it "
                f"({reason[:160]}) -- NOT WRITTEN"
            )

    print(
        f"\n{len(written)} fixture(s) written, {len(mismatches)} mismatch(es)."
    )
    return 1 if mismatches else 0


if __name__ == "__main__":
    raise SystemExit(main())
